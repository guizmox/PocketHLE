//! One capture, storage, process-rendezvous and cold-restore path for all GUIs.
use crate::{Control, LaunchContext};
mod archive;
pub use archive::{
    prepare, prepare_async, queue_managed_captures, Preparation, PreparedRestore, RestoreTarget,
};
use archive::{write_checkpoint, Archive};
use pocket_core::{
    cpu::Cpu,
    kernel::{
        handles::HandleTable, memory_division::MemoryDivision, InputEvent, Process,
        ProcessHandleContext, ResumePoint,
    },
    DeviceRestoreGroup, Emulator, RestoredSession, SessionSnapshot,
};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, mpsc::Sender, Arc},
    time::{Duration, Instant},
};

pub type Reply = Sender<Result<u64, String>>;
#[derive(Default)]
pub struct ProcessSnapshots {
    pub domain: pocket_core::checkpoint_domain::CheckpointDomain<pocket_core::ProcessCheckpoint>,
    pub participant:
        Option<pocket_core::checkpoint_domain::Participant<pocket_core::ProcessCheckpoint>>,
    pub restore_group: Option<Arc<DeviceRestoreGroup>>,
    pub restore: Option<SessionSnapshot>,
    pub launch_config: Option<pocket_library::LauncherConfig>,
    pub managed_restore: Option<pocket_netcf::snapshot::ManagedSnapshot>,
    pub requests: Vec<Reply>,
    pub synchronous_launch: bool,
}
impl ProcessSnapshots {
    pub fn program_path(&self) -> Option<&str> {
        self.restore
            .as_ref()
            .map(SessionSnapshot::program_path)
            .or_else(|| self.managed_restore.as_ref().map(|s| s.program_path()))
    }
    pub fn child(&self) -> Self {
        let mut child = Self::default();
        child.domain = self.domain.clone();
        child.launch_config = self.launch_config.clone();
        child
    }
    pub fn requested(&self, pid: u32, active: bool) -> bool {
        self.synchronous_launch
            || self.domain.requested(pid)
            || (active && !self.requests.is_empty())
    }
    pub fn request(&mut self, pid: u32, state: &pocket_core::kernel::KernelState) {
        if self.requests.is_empty() || self.domain.requested(pid) {
            return;
        }
        let processes = state.object_handles.checkpoint_process_ids();
        if processes.len() > 1 || pid != 1 {
            if let Err(error) = self.domain.request(pid, processes, Duration::from_secs(10)) {
                for reply in self.requests.drain(..) {
                    let _ = reply.send(Err(error.clone()));
                }
            }
        }
    }
    pub fn register(&mut self, pid: u32) -> Result<(), String> {
        self.participant = Some(self.domain.register(pid)?);
        Ok(())
    }
}
impl Drop for ProcessSnapshots {
    fn drop(&mut self) {
        for reply in self.requests.drain(..) {
            let _ = reply.send(Err("Snapshot capture was interrupted".into()));
        }
    }
}

pub struct Child {
    pub context: LaunchContext,
    pub game: pocket_library::GameEntry,
    pub guest: String,
    pub table: HandleTable,
    pub parent: u32,
    pub ram: Option<MemoryDivision>,
    pub handles: Option<ProcessHandleContext>,
    pub command_line: Option<String>,
    pub snapshot: Option<SessionSnapshot>,
    pub restore_group: Option<Arc<DeviceRestoreGroup>>,
}
/// Only platform process construction and frame publication differ between GUIs.
pub trait Host {
    fn snapshots(&mut self) -> &mut ProcessSnapshots;
    fn control(&self) -> Control;
    fn pid(&self) -> u32;
    fn launch_context(&self) -> Option<LaunchContext>;
    fn spawn(&mut self, child: Child) -> Result<(), String>;
    fn reap_children(&mut self);
    fn reset_foreground(&mut self);
    fn fail(&mut self, error: String);
}

pub fn spawn_job<T: Send + 'static>(
    table: HandleTable,
    parent: u32,
    control: Control,
    run: impl FnOnce() -> (T, u32) + Send + 'static,
) -> std::io::Result<pocket_core::child_jobs::ChildJob<(T, u32)>> {
    let owned = table.clone();
    let pid = table.process_id();
    let clock = pocket_core::kernel::guest_clock::GuestClockSnapshot::capture();
    let at = Instant::now();
    let thread = std::thread::Builder::new()
        .name(format!("pockethle-process-{pid}"))
        .spawn(move || {
            let result = clock.run_at(at, run);
            owned.complete_process_exit(result.1);
            if owned.exit_code(pid, Some(0)).is_none() {
                owned.set_exit(Some(0), result.1);
            }
            owned.mark_inactive();
            let next = owned.focus_after_exit(parent);
            let _ =
                control
                    .foreground
                    .compare_exchange(pid, next, Ordering::AcqRel, Ordering::Acquire);
            result
        })?;
    Ok(pocket_core::child_jobs::ChildJob { table, thread })
}

pub fn restore(
    host: &mut impl Host,
    emulator: &mut Emulator,
    mut saved: SessionSnapshot,
) -> anyhow::Result<RestoredSession> {
    if let Some(archive) = saved.take_device() {
        anyhow::ensure!(
            host.snapshots().restore_group.is_none(),
            "nested device checkpoint"
        );
        let (shared, foreground, children) = archive.into_parts();
        let group = Arc::new(DeviceRestoreGroup::new(
            shared,
            foreground,
            &emulator.process().unwrap().state,
            Duration::from_secs(30),
        )?);
        host.snapshots().restore_group = Some(group.clone());
        host.control()
            .foreground
            .store(foreground, Ordering::Release);
        let context = host
            .launch_context()
            .ok_or_else(|| anyhow::anyhow!("missing device restore launch context"))?;
        for snapshot in children {
            let guest = snapshot.program_path().to_owned();
            let executable = emulator
                .process()
                .unwrap()
                .state
                .vfs
                .resolve(&guest)
                .ok_or_else(|| anyhow::anyhow!("saved executable is missing: {guest}"))?
                .canonicalize()?;
            let mut game = context.game.clone();
            game.executable = game.child_executable_reference(&context.library, &executable);
            let pid = snapshot.process_id();
            let table = group
                .process_table(pid)
                .ok_or_else(|| anyhow::anyhow!("missing restored process {pid}"))?;
            host.spawn(Child {
                context: context.clone(),
                game,
                guest,
                table,
                parent: saved.process_id(),
                ram: emulator.process().unwrap().state.memory_division.clone(),
                handles: None,
                command_line: None,
                snapshot: Some(snapshot),
                restore_group: Some(group.clone()),
            })
            .map_err(anyhow::Error::msg)?;
        }
    }
    match host.snapshots().restore_group.as_ref() {
        Some(group) => group.restore_process(emulator, saved),
        None => emulator.restore_snapshot(saved),
    }
}

pub fn capture(
    host: &mut impl Host,
    cpu: &mut dyn pocket_core::cpu::Cpu,
    process: &mut pocket_core::kernel::Process,
    point: pocket_core::kernel::ResumePoint,
    mut suspended: Option<&mut Option<pocket_core::winceapi::SuspendedSession>>,
) {
    let pid = host.pid();
    let control = host.control();
    let context = host.launch_context();
    let runtime = host.snapshots();
    let started = std::time::Instant::now();
    let gate = process.state.object_handles.execution_gate();
    let _gate = (!runtime.domain.requested(pid))
        .then(|| gate.lock().unwrap_or_else(|error| error.into_inner()));
    let result = (|| {
        let launch = context.as_ref().ok_or("missing snapshot launch context")?;
        let path = pocket_library::snapshot_slot::path_for(&launch.library, &launch.game);
        let mut snapshot = if runtime.domain.requested(pid) {
            process.state.publish_handles();
            let participant = runtime
                .participant
                .as_ref()
                .ok_or("missing checkpoint participant")?;
            let leader = !runtime.requests.is_empty();
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
                control.foreground.load(Ordering::Acquire),
            )
        } else {
            pocket_core::SessionSnapshot::capture(cpu, process, point)
        }
        .map_err(|error| format!("{error:#}"))?;
        snapshot.set_environment(crate::compatibility::environment(
            launch,
            crate::compatibility::Engine::Arm,
        ));
        Ok::<_, String>(Some((path, snapshot)))
    })();
    let replies = std::mem::take(&mut runtime.requests);
    match result {
        Ok(None) => {}
        Ok(Some((path, snapshot))) => {
            log::debug!("Snapshot capture took {:?}", started.elapsed());
            write_checkpoint(path, Archive::Native(snapshot), replies);
        }
        Err(error) => {
            for reply in replies {
                let _ = reply.send(Err(error.clone()));
            }
        }
    }
}

pub fn resolve_program(
    library: &PathBuf,
    game: &pocket_library::GameEntry,
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

/// Park an owner at its real continuation. Native resources never leave its thread.
pub fn wait_for_child(
    host: &mut impl Host,
    cpu: &mut dyn Cpu,
    process: &mut Process,
    point: ResumePoint,
) {
    host.snapshots().synchronous_launch = false;
    let restoring = process.state.suspended_process_launch.is_some();
    if !restoring {
        process.state.suspended_process_launch = process.state.pending_process_launch.take();
    }
    let Some(request) = process.state.suspended_process_launch.clone() else {
        return;
    };
    let control = host.control();
    let pid = host.pid();
    let Some(table) = process.state.object_handles.child(request.process_handle) else {
        host.fail("suspended launch has no child process".into());
        process.state.should_stop = true;
        return;
    };
    let child_pid = table.process_id();
    process.state.audio.suspend_output();
    let mut suspended = Some(pocket_core::suspend_host_session());
    if !restoring {
        let spawn = (|| {
            let context = host.launch_context().ok_or("missing launcher context")?;
            let executable = request
                .executable
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let root = context.card.canonicalize().map_err(|e| e.to_string())?;
            if !executable.starts_with(root) {
                return Err("executable is outside the mounted SD Card".into());
            }
            let mut game = context.game.clone();
            game.executable = game.child_executable_reference(&context.library, &executable);
            let handles = process
                .state
                .child_handle_context(request.process_handle)
                .ok_or("missing synchronous child handle context")?;
            host.spawn(Child {
                context,
                game,
                guest: request.guest_path.clone(),
                table: table.clone(),
                parent: pid,
                ram: process.state.memory_division.clone(),
                handles: Some(handles),
                command_line: Some(request.command_line.clone()),
                snapshot: None,
                restore_group: None,
            })
        })();
        if let Err(error) = spawn {
            log::warn!("cannot start synchronous child: {error}");
            table.complete_process_exit(0xc0000135);
            table.set_exit(Some(0), 0xc0000135);
            table.mark_inactive();
        } else {
            control.foreground.store(child_pid, Ordering::Release);
        }
    }
    loop {
        host.reap_children();
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
                host.fail(format!("suspended process control failed: {error}"));
                process.state.should_stop = true;
                break;
            }
            Ok(false) => {}
        }
        if host.snapshots().domain.requested(pid) {
            capture(host, cpu, process, point, Some(&mut suspended));
        }
        if table.exit_code(child_pid, None).is_some() {
            break;
        }
        if control.stop.load(Ordering::Acquire) {
            process.state.should_stop = true;
            break;
        }
        if table.is_active(child_pid) {
            let _ = control.foreground.compare_exchange(
                pid,
                child_pid,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
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
        let current = control.foreground.load(Ordering::Acquire);
        if current == child_pid || !table.is_active(current) {
            let _ = control.foreground.compare_exchange(
                current,
                pid,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        host.reset_foreground();
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
            host.fail(error);
            state.should_stop = true;
        }
        state.audio.resume_output();
        log::info!("foreground child exited with 0x{code:08x}; resuming suspended caller");
    }
}
