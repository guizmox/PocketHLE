//! Snapshot coordination on the owning process threads.
use super::*;

pub(super) fn resolve_snapshot_program(
    library: &PathBuf,
    game: &GameEntry,
    card: &PathBuf,
    guest: &str,
) -> Result<PathBuf, String> {
    if guest
        .split(['\\', '/'])
        .any(|part| part == "." || part == ".." || part.contains(':'))
    {
        return Err("invalid saved executable path".into());
    }
    let extracted = game.extracted_dir(library);
    let mut vfs = pocket_core::kernel::vfs::Vfs::new();
    let mut roots = vec![extracted.clone(), card.clone()];
    for prefix in [
        r"\Application\",
        r"\Program Files\",
        r"\Program Files\Game\",
        r"\expresso\",
    ] {
        vfs.mount_read_only(prefix, &extracted);
    }
    for prefix in [r"\SD Card\", r"\Storage Card\"] {
        vfs.mount_read_only(prefix, card);
    }
    for (prefix, directory) in game.install_mounts(library) {
        vfs.mount_read_only(&prefix, &directory);
        roots.push(directory);
    }
    if let Some(paths) = game.guest_launch_paths(library) {
        let directory = game.install_mount_dir(library, &paths.install_prefix);
        vfs.mount_read_only(&paths.install_prefix, &directory);
        roots.push(directory);
    }
    let path = vfs
        .resolve(guest)
        .ok_or("saved executable is missing from current mounts")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.is_file()
        || !roots
            .iter()
            .any(|root| root.canonicalize().is_ok_and(|root| path.starts_with(root)))
    {
        return Err("saved executable is outside current application mounts".into());
    }
    Ok(path)
}

impl RunHook {
    pub(super) fn restore_checkpoint(
        &mut self,
        emulator: &mut pocket_core::Emulator,
        mut saved: pocket_core::SessionSnapshot,
    ) -> anyhow::Result<pocket_core::RestoredSession> {
        if let Some(archive) = saved.take_device() {
            anyhow::ensure!(self.restore_group.is_none(), "nested device checkpoint");
            let (shared, foreground, children) = archive.into_parts();
            let group = Arc::new(pocket_core::DeviceRestoreGroup::new(
                shared,
                foreground,
                &emulator.process().unwrap().state,
                Duration::from_secs(30),
            )?);
            self.restore_group = Some(group.clone());
            self.foreground.store(foreground, Ordering::Release);
            let context = self
                .launch_context
                .clone()
                .ok_or_else(|| anyhow::anyhow!("missing device restore launch context"))?;
            for child in children {
                let guest_path = child.program_path().to_owned();
                let executable = emulator
                    .process()
                    .unwrap()
                    .state
                    .vfs
                    .resolve(&guest_path)
                    .ok_or_else(|| anyhow::anyhow!("saved executable is missing: {guest_path}"))?
                    .canonicalize()?;
                let mut game = context.game.clone();
                game.executable =
                    game.child_executable_reference(&context.library_root, &executable);
                let pid = child.process_id();
                let table = group
                    .process_table(pid)
                    .ok_or_else(|| anyhow::anyhow!("missing restored process {pid}"))?;
                let child_table = table.clone();
                let mut hook = self.child_hook();
                hook.restore_snapshot = Some(child);
                hook.restore_group = Some(group.clone());
                let context = context.clone();
                let parent = saved.process_id();
                let foreground = self.foreground.clone();
                let ram = emulator.process().unwrap().state.memory_division.clone();
                let thread = std::thread::Builder::new()
                    .name(format!("pockethle-restore-{pid}"))
                    .spawn(move || {
                        let result = context.runner.run_process(
                            &context.library_root,
                            &game,
                            &context.card_root,
                            Some(&guest_path),
                            ram,
                            None,
                            None,
                            &mut hook,
                        );
                        child_table.complete_process_exit(result.1);
                        child_table.set_exit(Some(0), result.1);
                        child_table.mark_inactive();
                        let next = child_table.focus_after_exit(parent);
                        let _ = foreground.compare_exchange(
                            pid,
                            next,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                        result
                    })?;
                self.children.push(ChildJob { table, thread });
            }
        }
        match self.restore_group.as_ref() {
            Some(group) => group.restore_process(emulator, saved),
            None => emulator.restore_snapshot(saved),
        }
    }

    pub(super) fn capture_checkpoint(
        &mut self,
        cpu: &mut dyn pocket_core::cpu::Cpu,
        process: &mut pocket_core::kernel::Process,
        point: pocket_core::kernel::ResumePoint,
    ) {
        self.capture_checkpoint_with_runtime(cpu, process, point, None);
    }

    fn capture_checkpoint_with_runtime(
        &mut self,
        cpu: &mut dyn pocket_core::cpu::Cpu,
        process: &mut pocket_core::kernel::Process,
        point: pocket_core::kernel::ResumePoint,
        mut suspended: Option<&mut Option<pocket_core::winceapi::SuspendedSession>>,
    ) {
        let started = std::time::Instant::now();
        let gate = process.state.object_handles.execution_gate();
        let _gate = (!self.checkpoint_domain.requested(self.pid))
            .then(|| gate.lock().unwrap_or_else(|error| error.into_inner()));
        let result = (|| {
            let launch = self
                .launch_context
                .as_ref()
                .ok_or("missing snapshot launch context")?;
            let path = pocket_library::snapshot_slot::path_for(&launch.library_root, &launch.game);
            let snapshot = if self.checkpoint_domain.requested(self.pid) {
                process.state.publish_handles();
                let participant = self
                    .checkpoint_participant
                    .as_ref()
                    .ok_or("missing checkpoint participant")?;
                let leader = !self.snapshot_requests.is_empty();
                let captured = participant.capture(|at| {
                    let mut capture = || {
                        pocket_core::SessionSnapshot::capture_device_process(
                            cpu, process, point, leader, at,
                        )
                        .map_err(|error| format!("{error:#}"))
                    };
                    match suspended.as_mut() {
                        Some(slot) => pocket_core::winceapi::SuspendedSession::with_current_at(
                            slot, at.host, capture,
                        ),
                        None => capture(),
                    }
                })?;
                let Some(captured) = captured else {
                    return Ok(None);
                };
                pocket_core::SessionSnapshot::assemble_device(
                    captured,
                    self.foreground.load(Ordering::Acquire),
                )
            } else {
                pocket_core::SessionSnapshot::capture(cpu, process, point)
            }
            .map_err(|error| format!("{error:#}"))?;
            Ok::<_, String>(Some((path, snapshot)))
        })();
        let replies = std::mem::take(&mut self.snapshot_requests);
        match result {
            Ok(None) => {}
            Ok(Some((path, snapshot))) => {
                log::debug!("Snapshot capture took {:?}", started.elapsed());
                let worker_replies = replies.clone();
                if let Err(error) = std::thread::Builder::new()
                    .name("pockethle-snapshot".into())
                    .spawn(move || {
                        let started = std::time::Instant::now();
                        let result = pocket_library::snapshot_slot::save(&path, |output| {
                            snapshot.write(output)
                        })
                        .map_err(|error| error.to_string());
                        log::debug!(
                            "Snapshot compression and write took {:?}",
                            started.elapsed()
                        );
                        for reply in worker_replies {
                            let _ = reply.send(result.clone());
                        }
                    })
                {
                    for reply in replies {
                        let _ = reply.send(Err(format!("cannot start snapshot writer: {error}")));
                    }
                }
            }
            Err(error) => {
                for reply in replies {
                    let _ = reply.send(Err(error.clone()));
                }
            }
        }
    }
}

impl RunHook {
    /// The launcher remains on its owning thread, with its CPU stopped at the
    /// actual launch continuation. A child has its own native TLS/GL context.
    /// This also services future captures of the parent while the child runs.
    pub(super) fn run_suspended_child(
        &mut self,
        cpu: &mut dyn pocket_core::cpu::Cpu,
        process: &mut pocket_core::kernel::Process,
        point: pocket_core::kernel::ResumePoint,
    ) {
        self.synchronous_launch = false;
        let restoring = process.state.suspended_process_launch.is_some();
        if !restoring {
            process.state.suspended_process_launch = process.state.pending_process_launch.take();
        }
        let Some(request) = process.state.suspended_process_launch.clone() else {
            return;
        };
        let Some(table) = process.state.object_handles.child(request.process_handle) else {
            self.runtime_error = Some("suspended launch has no child process".into());
            self.stop_all.store(true, Ordering::Release);
            process.state.should_stop = true;
            return;
        };
        let child_pid = table.process_id();
        process.state.audio.suspend_output();
        let mut suspended = Some(pocket_core::suspend_host_session());
        if !restoring {
            let spawn = (|| {
                let context = self
                    .launch_context
                    .clone()
                    .ok_or("missing launcher context")?;
                let reference = context.child_executable_reference(&request)?;
                let mut game = context.game.clone();
                game.executable = reference;
                let handles = process
                    .state
                    .child_handle_context(request.process_handle)
                    .ok_or("missing synchronous child handle context")?;
                let ram = process.state.memory_division.clone();
                let mut hook = self.child_hook();
                let child_table = table.clone();
                let foreground = self.foreground.clone();
                let parent = self.pid;
                let guest = request.guest_path.clone();
                let command_line = request.command_line.clone();
                let clock = pocket_core::kernel::guest_clock::GuestClockSnapshot::capture();
                let at = Instant::now();
                let thread = std::thread::Builder::new()
                    .name(format!("pockethle-foreground-{child_pid}"))
                    .spawn(move || {
                        let result = clock.run_at(at, || {
                            context.runner.run_process(
                                &context.library_root,
                                &game,
                                &context.card_root,
                                Some(&guest),
                                ram,
                                Some(handles),
                                Some(ProcessStartup {
                                    ack: None,
                                    table: child_table.clone(),
                                    command_line,
                                    error: 193,
                                }),
                                &mut hook,
                            )
                        });
                        child_table.complete_process_exit(result.1);
                        child_table.set_exit(Some(0), result.1);
                        child_table.mark_inactive();
                        let next = child_table.focus_after_exit(parent);
                        let _ = foreground.compare_exchange(
                            child_pid,
                            next,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                        result
                    })
                    .map_err(|error| error.to_string())?;
                self.children.push(ChildJob {
                    table: table.clone(),
                    thread,
                });
                Ok::<_, String>(())
            })();
            if let Err(error) = spawn {
                log::warn!("cannot start synchronous child: {error}");
                table.complete_process_exit(0xc0000135);
                table.set_exit(Some(0), 0xc0000135);
                table.mark_inactive();
            } else {
                self.foreground.store(child_pid, Ordering::Release);
            }
        }
        loop {
            self.reap_children();
            let gate = process.state.object_handles.execution_gate();
            let termination = {
                let _guard = gate.lock().unwrap_or_else(|error| error.into_inner());
                process.state.apply_suspended_process_termination(cpu)
            };
            match termination {
                Ok(true) => {
                    process.state.should_stop = true;
                    break;
                }
                Err(error) => {
                    self.runtime_error = Some(format!("suspended process control failed: {error}"));
                    self.stop_all.store(true, Ordering::Release);
                    process.state.should_stop = true;
                    break;
                }
                Ok(false) => {}
            }
            if self.checkpoint_domain.requested(self.pid) {
                self.capture_checkpoint_with_runtime(cpu, process, point, Some(&mut suspended));
            }
            if table.exit_code(child_pid, None).is_some() {
                break;
            }
            if self.stop_all.load(Ordering::Acquire) {
                process.state.should_stop = true;
                break;
            }
            // Orphan/background completion can focus an active ancestor. A
            // suspended caller redirects that focus down its still-live chain.
            if table.is_active(child_pid) {
                let _ = self.foreground.compare_exchange(
                    self.pid,
                    child_pid,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            }
            // No WinCE execution gate is held while a suspended owner waits.
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(suspended);
        let state = &mut process.state;
        if state.should_stop {
            return;
        }
        if let Some(code) = table.exit_code(child_pid, None) {
            state.sync_transferred_handles();
            if let Some(child) = state.child_processes.get_mut(&request.process_handle) {
                child.exit_code = Some(code);
            }
            for handle in [request.process_handle, request.thread_handle] {
                if let Some(mut event) = state.events.get_mut(&handle) {
                    event.signalled = true;
                }
            }
            state.suspended_process_launch = None;
            let current = self.foreground.load(Ordering::Acquire);
            if current == child_pid || !table.is_active(current) {
                let _ = self.foreground.compare_exchange(
                    current,
                    self.pid,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            }
            self.reset_process();
            state.framebuffer.frame_counter = state.framebuffer.frame_counter.wrapping_add(1);
            let mut releases = state.snapshot_input_releases();
            for &event in &state.pending_input {
                if matches!(
                    event,
                    InputEvent::KeyUp { .. }
                        | InputEvent::PointerUp { .. }
                        | InputEvent::MouseMessage {
                            message: 0x205 | 0x208,
                            ..
                        }
                ) && !releases.contains(&event)
                {
                    releases.push(event);
                }
            }
            if let Err(error) = state.restore_snapshot_input(releases) {
                self.runtime_error = Some(error);
                self.stop_all.store(true, Ordering::Release);
                state.should_stop = true;
            }
            state.audio.resume_output();
            log::info!("foreground child exited with 0x{code:08x}; resuming suspended caller");
        }
    }
}
