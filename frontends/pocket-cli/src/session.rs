//! Native CLI CE sessions. Kernel CreateProcess owns validation, handles and
//! startup commit; this frontend services its requests like the desktop runner.
//! CerfDemo's wrapper can exit while its child desktop keeps running. Minifb
//! belongs to the main host thread, including after that wrapper has exited.
use super::{CpuBackend, ScheduledInputHook};
use anyhow::{Context, Result};
use pocket_core::{
    ce_desktop::{Desktop, ProcessFrame},
    kernel::{
        self, FrameAction, FrameHook, InputEvent, KernelState, ProcessHandleContext, ProcessLaunch,
    },
    Emulator,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) struct Config {
    pub backend: CpuBackend,
    pub image: Option<Arc<pocket_core::ce_image::WindowsCeImage>>,
    pub screen: (u32, u32),
    pub message_budget: u64,
    pub max_slices: u64,
    pub instructions_per_slice: u64,
    pub halt_on_unimplemented: bool,
    pub trace: Option<PathBuf>,
    pub display: bool,
    pub dump_dir: Option<PathBuf>,
    pub dump_stride: u64,
    pub max_frames: u64,
    pub scheduled: Vec<(u64, InputEvent)>,
}

#[derive(Clone)]
struct LaunchContext {
    backend: CpuBackend,
    image: Option<Arc<pocket_core::ce_image::WindowsCeImage>>,
    classic: bool,
    namespace: kernel::vfs::VfsNamespace,
    screen: (u32, u32),
    message_budget: u64,
    max_slices: u64,
    instructions_per_slice: u64,
    halt_on_unimplemented: bool,
    trace: Option<PathBuf>,
}

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    foreground: AtomicU32,
    desktop: Mutex<Desktop>,
    errors: Mutex<Vec<String>>,
}
impl Shared {
    fn exited(&self, table: &kernel::handles::HandleTable, parent: u32) {
        let pid = table.process_id();
        table.mark_inactive();
        let next = table.focus_after_exit(parent);
        self.desktop.lock().unwrap().remove(pid, next);
        let _ = self
            .foreground
            .compare_exchange(pid, next, Ordering::AcqRel, Ordering::Acquire);
    }
}

struct Startup {
    ack: Option<mpsc::Sender<std::result::Result<(), u32>>>,
    error: u32,
}
impl Drop for Startup {
    fn drop(&mut self) {
        if let Some(ack) = self.ack.take() {
            let _ = ack.send(Err(self.error));
        }
    }
}
struct ChildJob {
    table: kernel::handles::HandleTable,
    thread: thread::JoinHandle<()>,
}

struct ProcessHook {
    shared: Arc<Shared>,
    context: LaunchContext,
    children: Vec<ChildJob>,
    pid: u32,
    last_frame: u64,
    last_emit: Instant,
    was_foreground: bool,
    host: Option<Host>,
}
impl ProcessHook {
    fn poll_host(&mut self) {
        if let Some(host) = &mut self.host {
            if let Err(error) = host.poll(&self.shared) {
                self.shared
                    .errors
                    .lock()
                    .unwrap()
                    .push(format!("host presentation: {error:#}"));
                self.shared.stop.store(true, Ordering::Release);
            }
        }
    }

    fn publish(&mut self, state: &mut KernelState) {
        let counter = state.framebuffer.frame_counter;
        if counter == self.last_frame || self.last_emit.elapsed() < Duration::from_millis(16) {
            return;
        }
        self.last_frame = counter;
        self.last_emit = Instant::now();
        let frame = match ProcessFrame::capture(state) {
            Ok(frame) => frame,
            Err(error) => {
                self.shared.errors.lock().unwrap().push(error);
                self.shared.stop.store(true, Ordering::Release);
                return;
            }
        };
        if let Err(error) = self.shared.desktop.lock().unwrap().publish(self.pid, frame) {
            self.shared.errors.lock().unwrap().push(error);
            self.shared.stop.store(true, Ordering::Release);
        }
    }

    fn start_child(&mut self, state: &mut KernelState, request: ProcessLaunch) {
        let result = self.spawn_child(state, &request);
        if let Some(key) = request.call_key {
            state.process_launch_results.insert(key, result);
        } else if let Ok(handle) = result {
            if let Some(table) = state.object_handles.child(handle) {
                table.allow_start();
            }
        } else if let Err(error) = result {
            log::warn!(
                "CE child launch failed for {:?}: error {error}",
                request.guest_path
            );
        }
    }

    fn spawn_child(
        &mut self,
        state: &mut KernelState,
        request: &ProcessLaunch,
    ) -> std::result::Result<u32, u32> {
        let child_context = state
            .child_handle_context(request.process_handle)
            .ok_or(6u32)?;
        let table = child_context.table.clone();
        let completion = table.clone();
        let ram = state.memory_division.clone();
        let context = self.context.clone();
        let shared = self.shared.clone();
        let request_copy = request.clone();
        let parent = self.pid;
        let (ack, ready) = mpsc::channel();
        let startup = Startup {
            ack: Some(ack),
            error: 193,
        };
        let job = thread::Builder::new()
            .name(format!("pockethle-ce-{}", table.process_id()))
            .spawn(move || {
                run_child(
                    context,
                    shared,
                    request_copy,
                    child_context,
                    ram,
                    startup,
                    parent,
                );
            })
            .map_err(|_| {
                table.mark_inactive();
                8u32
            })?;
        self.children.push(ChildJob {
            table: table.clone(),
            thread: job,
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match ready.try_recv() {
                Ok(Ok(())) => break,
                Ok(Err(error)) => return Err(error),
                Err(mpsc::TryRecvError::Disconnected) => return Err(193),
                Err(mpsc::TryRecvError::Empty) => {}
            }
            self.poll_host();
            if Instant::now() >= deadline || self.shared.stop.load(Ordering::Acquire) {
                completion.terminate_remote_process(completion.process_id(), 0xc0000001);
                completion.allow_start();
                return Err(1460);
            }
            thread::sleep(Duration::from_millis(1));
        }
        self.shared
            .foreground
            .store(table.process_id(), Ordering::Release);
        state.reset_foreground_input();
        log::info!(
            "CE child ready: pid={} {:?}",
            table.process_id(),
            request.guest_path
        );
        if !request.concurrent {
            table.allow_start();
            state.audio.suspend_output();
            while self
                .children
                .last()
                .is_some_and(|job| !job.thread.is_finished())
            {
                self.poll_host();
                thread::sleep(Duration::from_millis(4));
            }
            if let Some(job) = self.children.pop() {
                let _ = job.thread.join();
            }
            state.audio.resume_output();
            self.shared.foreground.store(self.pid, Ordering::Release);
            self.last_frame = u64::MAX;
            state.reset_foreground_input();
        }
        Ok(request.process_handle)
    }

    fn join_children(&mut self) {
        while self.children.iter().any(|job| !job.thread.is_finished()) {
            self.poll_host();
            if self.shared.stop.load(Ordering::Acquire) {
                for job in &self.children {
                    job.table
                        .terminate_remote_process(job.table.process_id(), 0xc0000001);
                    job.table.allow_start();
                }
            }
            thread::sleep(Duration::from_millis(4));
        }
        for job in self.children.drain(..) {
            if job.thread.join().is_err() {
                self.shared
                    .errors
                    .lock()
                    .unwrap()
                    .push("CE process worker panicked".into());
            }
        }
    }
}
impl FrameHook for ProcessHook {
    fn on_frame(&mut self, state: &mut KernelState) -> FrameAction {
        if let Some(request) = state.take_process_launch() {
            self.start_child(state, request);
        }
        let foreground = self.shared.foreground.load(Ordering::Acquire) == self.pid;
        if foreground != self.was_foreground {
            state.reset_foreground_input();
            self.was_foreground = foreground;
        }
        self.publish(state);
        self.poll_host();
        self.shared
            .desktop
            .lock()
            .unwrap()
            .drain(self.pid, &mut state.pending_input);
        if self.shared.stop.load(Ordering::Acquire) {
            FrameAction::Stop
        } else {
            FrameAction::Continue
        }
    }
}

fn run_child(
    context: LaunchContext,
    shared: Arc<Shared>,
    request: ProcessLaunch,
    handles: ProcessHandleContext,
    ram: Option<kernel::memory_division::MemoryDivision>,
    mut startup: Startup,
    parent: u32,
) {
    let table = handles.table.clone();
    table.defer_process_exit();
    let mut hook = ProcessHook {
        context: context.clone(),
        shared: shared.clone(),
        children: Vec::new(),
        pid: table.process_id(),
        last_frame: u64::MAX,
        last_emit: Instant::now() - Duration::from_secs(1),
        was_foreground: false,
        host: None,
    };
    let result: Result<u32> = (|| {
        let mut emu = match context.backend {
            CpuBackend::Stub => Emulator::with_stub_cpu(),
            #[cfg(feature = "unicorn")]
            CpuBackend::Unicorn => Emulator::with_unicorn_cpu()?,
            #[cfg(feature = "unicorn")]
            CpuBackend::Mips => Emulator::with_unicorn_cpu_for_arch(pocket_core::cpu::Arch::Mips)?,
        };
        if let Some(image) = &context.image {
            emu.set_windows_ce_image(image.clone())?;
        }
        emu.set_classic_windows(context.classic);
        emu.set_screen_size(context.screen.0, context.screen.1);
        emu.set_halt_on_unimplemented(context.halt_on_unimplemented);
        emu.max_slices = context.max_slices;
        emu.instruction_budget_per_slice = context.instructions_per_slice;
        emu.load_pe(&request.executable)?;
        emu.process_mut()
            .unwrap()
            .state
            .attach_handle_context(handles);
        startup.error = 8;
        anyhow::ensure!(
            emu.set_memory_division(ram),
            "insufficient shared device RAM for CE child"
        );
        emu.process_mut()
            .unwrap()
            .state
            .vfs
            .inherit_namespace(&context.namespace);
        emu.set_module_path(&request.guest_path);
        if let Some((directory, _)) = request.guest_path.rsplit_once('\\') {
            emu.set_default_dir(&format!("{directory}\\"));
        }
        emu.set_synthetic_message_budget(context.message_budget);
        emu.process_mut().unwrap().state.process_launch_enabled = true;
        emu.set_startup_command_line(&request.command_line)?;
        if let Some(path) = &context.trace {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let child_trace =
                path.with_file_name(format!("{stem}.process-{}.jsonl", table.process_id()));
            startup.error = 29;
            emu.set_trace_sink(Box::new(std::io::BufWriter::new(std::fs::File::create(
                child_trace,
            )?)));
        }
        if let Some(ack) = startup.ack.take() {
            let _ = ack.send(Ok(()));
        }
        // No guest instruction executes before CreateProcess writes the caller's
        // PROCESS_INFORMATION and releases its existing startup transaction.
        while !table.start_allowed() && !shared.stop.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
        if shared.stop.load(Ordering::Acquire) {
            return Ok(0xc0000001);
        }
        emu.start_audio();
        let result = emu.run_with_hook(&mut hook);
        emu.stop_audio();
        emu.process().unwrap().state.registry.flush()?;
        let code = emu.process().unwrap().state.process_exit_code.unwrap_or(0);
        drop(emu);
        result?;
        Ok(code)
    })();
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            log::error!("CE child {:?}: {error:#}", request.guest_path);
            shared
                .errors
                .lock()
                .unwrap()
                .push(format!("{}: {error:#}", request.guest_path));
            0xc0000001
        }
    };
    drop(startup);
    table.complete_process_exit(code);
    if table.exit_code(table.process_id(), Some(0)).is_none() {
        table.set_exit(Some(0), code);
    }
    shared.exited(&table, parent);
    hook.join_children();
}

struct Host {
    scheduled: ScheduledInputHook,
    frames_seen: u64,
    dump_dir: Option<PathBuf>,
    dump_stride: u64,
    written: u64,
    max_frames: u64,
    last_pixels: Vec<u8>,
    last_image_size: (u32, u32),
    text_input: bool,
    input_file: Option<(PathBuf, usize)>,
    last_input_poll: Instant,
    #[cfg(feature = "display")]
    display: Option<super::display_window::DisplayHook>,
}
impl Host {
    fn new(config: &mut Config) -> Result<Self> {
        if let Some(dir) = &config.dump_dir {
            std::fs::create_dir_all(dir)?;
        }
        #[cfg(feature = "display")]
        let display = if config.display {
            Some(super::display_window::DisplayHook::new(
                config.screen.0,
                config.screen.1,
            )?)
        } else {
            None
        };
        #[cfg(not(feature = "display"))]
        anyhow::ensure!(
            !config.display,
            "--display requires the display Cargo feature"
        );
        Ok(Self {
            scheduled: ScheduledInputHook::new(std::mem::take(&mut config.scheduled)),
            frames_seen: 0,
            dump_dir: config.dump_dir.clone(),
            dump_stride: config.dump_stride.max(1),
            written: 0,
            max_frames: config.max_frames,
            last_pixels: Vec::new(),
            last_image_size: config.screen,
            text_input: false,
            input_file: std::env::var_os("POCKETHLE_INPUT_FILE").map(|p| (p.into(), 0)),
            last_input_poll: Instant::now() - Duration::from_secs(1),
            #[cfg(feature = "display")]
            display,
        })
    }

    fn send(&mut self, shared: &Shared, event: InputEvent) {
        let mut desktop = shared.desktop.lock().unwrap();
        desktop.queue(event, shared.foreground.load(Ordering::Acquire));
        shared
            .foreground
            .store(desktop.focused_process(), Ordering::Release);
    }

    fn read_input_file(&mut self, shared: &Shared) {
        if self.last_input_poll.elapsed() < Duration::from_millis(50) {
            return;
        }
        self.last_input_poll = Instant::now();
        let Some((path, offset)) = &mut self.input_file else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        if text.len() > 65536 {
            return;
        }
        if text.len() < *offset {
            *offset = 0;
        }
        let mut events = Vec::new();
        if let Some(tail) = text.get(*offset..) {
            for line in tail.split_inclusive('\n') {
                if !line.ends_with('\n') {
                    break;
                }
                *offset += line.len();
                let line = line.trim();
                if line == "quit" {
                    shared.stop.store(true, Ordering::Release);
                } else if let Some(key) = line
                    .strip_prefix("key ")
                    .and_then(|k| super::parse_virtual_key(k).ok())
                {
                    events.extend([
                        InputEvent::KeyDown { vk: key },
                        InputEvent::KeyUp { vk: key },
                    ]);
                } else if let Some(text) = line
                    .strip_prefix("text ")
                    .or_else(|| line.strip_prefix("command "))
                {
                    events.extend(text.chars().map(|character| InputEvent::Text { character }));
                    if line.starts_with("command ") {
                        events
                            .extend([InputEvent::KeyDown { vk: 13 }, InputEvent::KeyUp { vk: 13 }]);
                    }
                } else if let Some((kind, point)) = line.split_once(' ') {
                    if let Some((x, y)) = point
                        .split_once(',')
                        .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    {
                        match kind {
                            "tap" => events.extend([
                                InputEvent::PointerDown { x, y },
                                InputEvent::PointerUp { x, y },
                            ]),
                            "down" => events.push(InputEvent::PointerDown { x, y }),
                            "move" => events.push(InputEvent::PointerMove { x, y }),
                            "up" => events.push(InputEvent::PointerUp { x, y }),
                            _ => {}
                        }
                    }
                }
            }
        }
        for event in events {
            self.send(shared, event);
        }
    }

    fn poll(&mut self, shared: &Shared) -> Result<()> {
        // Use the same window/overlay ordering and input owners as the GUI.
        // The old per-process rectangle copier clipped CPL keyboards and sent
        // their dismiss tap to Control Panel behind the still-open child form.
        let composed = {
            let mut desktop = shared.desktop.lock().unwrap();
            desktop.activate(shared.foreground.load(Ordering::Acquire));
            let frame = desktop.compose().map_err(anyhow::Error::msg)?;
            shared
                .foreground
                .store(desktop.focused_process(), Ordering::Release);
            frame
        };
        if let Some(frame) = composed {
            self.text_input = frame.text_input;
            let (w, h) = (frame.image[0], frame.image[1]);
            let pixels: Vec<_> = frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|rgba| {
                    kernel::framebuffer::colorref_to_rgb565(
                        u32::from(rgba[0]) | u32::from(rgba[1]) << 8 | u32::from(rgba[2]) << 16,
                    )
                    .to_le_bytes()
                })
                .collect();
            // A state repaint with identical pixels is not a displayed image.
            if pixels != self.last_pixels || (w, h) != self.last_image_size {
                self.frames_seen += 1;
                if let Some(dir) = &self.dump_dir {
                    if (self.frames_seen - 1).is_multiple_of(self.dump_stride) {
                        let path = dir.join(format!("frame_{:06}.ppm", self.written));
                        std::fs::write(&path, ppm(w, h, &pixels))?;
                        self.written += 1;
                        log::info!("wrote {}", path.display());
                    }
                }
                #[cfg(feature = "display")]
                if let Some(display) = &mut self.display {
                    display.present_pixels(w, h, &pixels)?;
                }
                self.last_pixels = pixels;
                self.last_image_size = (w, h);
                let count = if self.dump_dir.is_some() {
                    self.written
                } else {
                    self.frames_seen
                };
                if self.max_frames != 0 && count >= self.max_frames {
                    shared.stop.store(true, Ordering::Release);
                }
            }
        }
        let mut scheduled = VecDeque::new();
        self.scheduled.poll(self.frames_seen, &mut scheduled);
        for event in scheduled {
            self.send(shared, event);
        }
        self.read_input_file(shared);
        #[cfg(feature = "display")]
        if let Some(display) = &mut self.display {
            display.set_text_input(self.text_input);
            let (open, events) = display.poll_input();
            if !open {
                shared.stop.store(true, Ordering::Release);
            }
            for event in events {
                self.send(shared, event);
            }
        }
        Ok(())
    }
}

fn ppm(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
    for bytes in pixels.chunks_exact(2) {
        let color = u16::from_le_bytes([bytes[0], bytes[1]]);
        let (r, g, b) = (
            (color >> 11) as u8,
            ((color >> 5) & 63) as u8,
            (color & 31) as u8,
        );
        out.extend_from_slice(&[
            (r << 3) | (r >> 2),
            (g << 2) | (g >> 4),
            (b << 3) | (b >> 2),
        ]);
    }
    out
}

pub(super) fn run(mut emu: Emulator, mut config: Config) -> Result<()> {
    let mut host = Host::new(&mut config)?;
    let state = &mut emu
        .process_mut()
        .context("CE root process is not loaded")?
        .state;
    let initial: Vec<_> = state
        .pending_input
        .drain(..)
        .map(|event| (0, event))
        .collect();
    host.scheduled.pending.extend(initial);
    let mut events: Vec<_> = host.scheduled.pending.drain(..).collect();
    events.sort_by_key(|e| e.0);
    host.scheduled.pending.extend(events);
    let table = state.object_handles.clone();
    table.defer_process_exit();
    state.process_launch_enabled = true;
    let shared = Arc::new(Shared::default());
    shared
        .foreground
        .store(table.process_id(), Ordering::Release);
    let context = LaunchContext {
        backend: config.backend,
        image: config.image,
        classic: state.ce.windows.enabled,
        namespace: state.vfs.namespace_snapshot(),
        screen: config.screen,
        message_budget: config.message_budget,
        max_slices: config.max_slices,
        instructions_per_slice: config.instructions_per_slice,
        halt_on_unimplemented: config.halt_on_unimplemented,
        trace: config.trace,
    };
    let mut hook = ProcessHook {
        shared: shared.clone(),
        context,
        children: Vec::new(),
        pid: table.process_id(),
        last_frame: u64::MAX,
        last_emit: Instant::now() - Duration::from_secs(1),
        was_foreground: true,
        host: Some(host),
    };
    emu.start_audio();
    let result = emu.run_with_hook(&mut hook);
    emu.stop_audio();
    let code = emu.process().unwrap().state.process_exit_code.unwrap_or(0);
    let flush = emu.process().unwrap().state.registry.flush();
    // Waiters and orphan children see completion only after all private CPU,
    // image, stack and heap charges have been released, as in the GUI runner.
    drop(emu);
    table.complete_process_exit(code);
    shared.exited(&table, 0);
    hook.join_children();
    if let Some(host) = hook.host {
        if !host.last_pixels.is_empty() {
            let path = config
                .dump_dir
                .unwrap_or_else(std::env::temp_dir)
                .join("ce6-final.ppm");
            std::fs::write(
                &path,
                ppm(
                    host.last_image_size.0,
                    host.last_image_size.1,
                    &host.last_pixels,
                ),
            )?;
            println!(
                "Final CE framebuffer: {} ({} displayed image changes)",
                path.display(),
                host.frames_seen
            );
        }
    }
    flush?;
    result?;
    let errors = shared.errors.lock().unwrap();
    anyhow::ensure!(
        errors.is_empty(),
        "CE session errors: {}",
        errors.join("; ")
    );
    if code == 0 {
        println!("CE session exited cleanly.");
    } else {
        println!("CE session ended with guest exit code 0x{code:08x}.");
    }
    Ok(())
}
