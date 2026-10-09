//! Background runner that drives [`pocket_core::Emulator`] for the
//! desktop GUI.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use pocket_core::kernel::{FrameAction, FrameHook, InputEvent, KernelState};
use pocket_core::Emulator;
use pocket_library::{is_gizmondo_game, CpuBackendPref, GameEntry};

/// Minimum wall-clock interval between two `FrameSnapshot`s pushed
/// from the runner thread to the GUI thread. The frame hook fires
/// after every dispatched WinCE API call (potentially thousands of
/// times per logical game frame, since every `FillRect` / `BitBlt`
/// bumps `frame_counter`); without throttling we would generate and
/// queue a fresh 300 KiB RGBA snapshot for every one of those calls,
/// which is exactly the per-frame cost the desktop launcher used to
/// drown in. ~60 fps is plenty for a 320×240 LCD preview.
const FRAME_PUSH_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug, Clone)]
pub struct Runner {
    inner: Arc<Mutex<()>>,
    missing_api_logging: Arc<AtomicBool>,
}

impl Default for Runner {
    fn default()->Self {Self{inner:Default::default(),missing_api_logging:Arc::new(AtomicBool::new(true))}}
}
impl Runner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_missing_api_logging(&self,enabled:bool) {
        self.missing_api_logging.store(enabled,Ordering::Relaxed);
    }

    pub fn run_game(
        &self,
        library_root: PathBuf,
        game: GameEntry,
        live_tx: Option<Sender<FrameSnapshot>>,
        input_rx: Option<Receiver<InputCommand>>,
    ) -> RunOutcome {
        let _guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut hook = RunHook::new(live_tx, input_rx);
        let card_root = game.extracted_dir(&library_root);
        self.run_process(&library_root, &game, &card_root, None, None, None, None, &mut hook).0
    }

    fn run_process(
        &self,
        library_root: &PathBuf,
        game: &GameEntry,
        card_root: &PathBuf,
        guest_path: Option<&str>,
        inherited_ram: Option<pocket_core::kernel::memory_division::MemoryDivision>,
        handle_context: Option<pocket_core::kernel::ProcessHandleContext>,
        mut startup: Option<ProcessStartup>,
        hook: &mut RunHook,
    ) -> (RunOutcome, u32) {
        let exe = if guest_path.is_some() { game.executable_path(library_root) }
            else { game.launch_path(library_root) };
        let mut summary_lines = vec![format!("Game: {}", game.display_name)];

        let machine = pocket_core::pe::load_file(&exe)
            .map(|image| image.machine)
            .unwrap_or(pocket_core::pe::machine::ARM);
        // The Stub CPU does not interpret instructions — it is a
        // trace-only harness that exists so loader-level code can be
        // unit-tested without pulling in unicorn-engine. Trying to
        // actually `run` a game on it is guaranteed to crash with
        // "guest jumped to unmapped address 0x00000000" because the
        // stub never sets LR while pretending to call a guest
        // function. End users who click "Run" in the GUI always
        // want the real ARM core, regardless of what is persisted in
        // their `library.json` — so when a Stub-backed game is
        // launched and unicorn is compiled in, we silently promote
        // the run to Unicorn. This is defense-in-depth on top of
        // [`pocket_library::Library::migrate_legacy_entries`], which
        // covers users who downgrade or who have a stale
        // `library.json` from before that migration existed.
        let requested_backend = game.settings.cpu_backend;
        let mut effective_backend = requested_backend;
        let mut emu = match requested_backend {
            CpuBackendPref::Unicorn => match build_unicorn_for_machine(machine) {
                Ok(emu) => emu,
                Err(e) => {
                    summary_lines.push(format!("Unicorn unavailable, falling back to stub: {e}"));
                    effective_backend = CpuBackendPref::Stub;
                    Emulator::with_stub_cpu()
                }
            },
            CpuBackendPref::Stub => match build_unicorn_for_machine(machine) {
                Ok(emu) => {
                    summary_lines.push(
                        "Saved CPU backend was Stub (trace-only); promoting to \
                         Unicorn so the game can actually execute."
                            .to_string(),
                    );
                    effective_backend = CpuBackendPref::Unicorn;
                    emu
                }
                Err(_) => Emulator::with_stub_cpu(),
            },
        };
        summary_lines.push(format!("Backend: {}", effective_backend.label()));
        summary_lines.push(format!("Executable: {}", exe.display()));

        emu.set_halt_on_unimplemented(game.settings.halt_on_unimplemented);
        emu.set_unimplemented_api_sink(Box::new(crate::unimplemented_log::UnimplementedLog::new(
            library_root.join("pockethle-unimplemented-apis.log"),game.display_name.clone(),
            guest_path.map(str::to_owned).unwrap_or_else(||exe.display().to_string()),
            Arc::clone(&self.missing_api_logging))));
        emu.max_slices = game.settings.max_slices;
        emu.instruction_budget_per_slice = game.settings.instructions_per_slice;

        if let Err(e) = emu.load_pe(&exe) {
            summary_lines.push(format!("load_pe failed: {e:#}"));
            return (RunOutcome {
                summary: summary_lines.join("\n"),
                framebuffer: None,
            }, 0xc0000135);
        }

        // The screen has to be sized before the game runs: a GAPI title
        // reads the display geometry once during start-up and lays its
        // HUD out around it. `set_screen_size` requires `load_pe` to have
        // created the process first, so this call must come after the
        // load succeeds.
        // Gizmondo hardware has a native 320x240 landscape LCD. Detect the
        // platform before the title starts so GAPI sees the real geometry.
        let is_gizmondo = is_gizmondo_game(&game, &library_root);
        if let Some(context) = handle_context {
            context.table.defer_process_exit();
            if let Some(process) = emu.process_mut() { process.state.attach_handle_context(context); }
        }
        let ram = inherited_ram.or_else(|| is_gizmondo.then(pocket_core::kernel::memory_division::MemoryDivision::gizmondo_sdk_default));
        if !emu.set_memory_division(ram) {
            if let Some(startup) = startup.as_mut() { startup.error = 8; }
            summary_lines.push("Insufficient device RAM to load process".to_string());
            return (RunOutcome { summary: summary_lines.join("\n"), framebuffer: None }, 0xc0000017);
        }
        let registry_path = library_root.join(if is_gizmondo { "registry-gizmondo.json" } else { "registry-pocketpc.json" });
        let launcher_config = pocket_library::Library::open(library_root).map(|l| l.config().clone()).unwrap_or_default();
        let hardware = (launcher_config.bluetooth_enabled, launcher_config.camera_enabled, launcher_config.gps_enabled);
        if let Some(process) = emu.process_mut() {
            process.state.vfs.bluetooth.service.set_allowed(hardware.0);
            process.state.vfs.camera_service().set_allowed(hardware.1);
            process.state.vfs.gps_service().set_allowed(hardware.2);
            if let Err(e) = process.state.registry.configure_persistence(&registry_path) {
                if let Some(startup) = startup.as_mut() { startup.error = 29; }
                summary_lines.push(format!("Cannot load device registry: {e}"));
                return (RunOutcome { summary: summary_lines.join("\n"), framebuffer: None }, 29);
            }
            if is_gizmondo {
                if let Err(e) = pocket_core::kernel::colors::configure(&mut process.state, &launcher_config.colors_server_url, &launcher_config.colors_terminal_id) {
                    if let Some(startup) = startup.as_mut() { startup.error = 87; }
                    summary_lines.push(e);
                    return (RunOutcome { summary: summary_lines.join("\n"), framebuffer: None }, 87);
                }
            }
        }
        let (screen_w, screen_h) = if is_gizmondo {
            (320, 240)
        } else {
            game.settings.screen.size()
        };
        emu.set_screen_size(screen_w, screen_h);
        summary_lines.push(format!(
            "Screen: {screen_w}x{screen_h}{}",
            if is_gizmondo { " (Gizmondo)" } else { "" }
        ));

        for value in &game.registry {
            let registry_value = if let Some(text) = value.string.as_deref() {
                pocket_core::kernel::registry::RegistryValue::Sz(text.to_string())
            } else if let Some(number) = value.dword {
                pocket_core::kernel::registry::RegistryValue::Dword(number)
            } else {
                continue;
            };
            let saved = emu.process().and_then(|p| p.state.registry.value(&value.key, &value.name));
            if saved.is_none() || value.name.eq_ignore_ascii_case("InstallDir") {
                emu.set_registry_value(&value.key, &value.name, registry_value);
            }
        }
        if !game
            .registry
            .iter()
            .any(|value| value.name.eq_ignore_ascii_case("SaveDir"))
        {
            if let Some(install_dir) = game
                .registry
                .iter()
                .find(|value| value.name.eq_ignore_ascii_case("InstallDir"))
                .and_then(|value| value.string.clone())
                .or_else(|| game.install_dir.clone())
            {
                emu.set_registry_value(
                    r"HKLM\SOFTWARE\Apps\Astraware Cubis",
                    "SaveDir",
                    pocket_core::kernel::registry::RegistryValue::Sz(install_dir),
                );
            }
        }

        let extracted = game.extracted_dir(&library_root);
        emu.mount_read_only_dir("\\Application\\", &extracted);
        emu.mount_read_only_dir("\\Program Files\\", &extracted);
        emu.mount_read_only_dir("\\Program Files\\Game\\", &extracted);
        emu.mount_read_only_dir("\\expresso\\", &extracted);
        if is_gizmondo {
            // A real Gizmondo has two distinct storage areas: the game lives on
            // the removable SD card while persistent save data lives on the
            // internal `\Flash Disk`.  Keep the latter writable and shared by
            // Gizmondo titles so WinCE paths such as
            // `\Flash Disk\MyGames\Midway\Midway96.sav` work unchanged.
            let flash_disk = library_root.join("flash");
            emu.mount_save_dir("\\Flash Disk\\", &flash_disk);
            summary_lines.push(format!(
                "Mounted Gizmondo Flash Disk at {}",
                flash_disk.display()
            ));

            emu.mount_read_only_dir("\\SD Card\\", card_root);
            emu.mount_read_only_dir("\\Storage Card\\", card_root);
            emu.mount_read_only_dir("\\SD Card\\Game\\", &extracted);
            emu.mount_read_only_dir("\\Storage Card\\Game\\", &extracted);
            summary_lines.push(format!(
                "Mounted Gizmondo SD-card layout at {}",
                extracted.display()
            ));
            if let Some(name) = game.executable.file_name().and_then(|name| name.to_str()) {
                let guest_exe = format!("\\SD Card\\{name}");
                emu.set_module_path(&guest_exe);
                emu.set_default_dir("\\SD Card\\");
                summary_lines.push(format!("Module path: {guest_exe}"));
            }
        }
        // A game that keeps its assets in one archive opens it by
        // absolute path built from its own module path -- Spore Origins
        // asks for `\Program Files\EA\Spore v1.0.4\data.vfs`. Mount the
        // extracted directory where the cabinet said it would be
        // installed and report that path from `GetModuleFileNameW`,
        // the way the CLI already does for a cab. Without it the
        // archive never opens and the game calls through a pointer it
        // never stored, which surfaces as READ_UNMAPPED at 0x2.
        for prefix in &game.install_dirs {
            emu.mount_read_only_dir(prefix, &extracted);
        }
        if let Some(prefix) = game.guest_install_prefix() {
            emu.mount_read_only_dir(&prefix, &extracted);
            summary_lines.push(format!("Mounted {} at {prefix:?}", extracted.display()));
            if let Some(guest_exe) = game.guest_exe_path() {
                emu.set_module_path(&guest_exe);
                emu.set_default_dir(&prefix);
                summary_lines.push(format!("Module path: {guest_exe}"));
            }
            if let Some(save_prefix) = game.guest_save_prefix() {
                let save_dir = game.save_dir(&library_root);
                emu.mount_save_dir(&save_prefix, &save_dir);
                summary_lines.push(format!(
                    "Save data: {} -> {save_prefix:?}",
                    save_dir.display()
                ));
            }
        }
        // GUI users actually play the game, so don't auto-fire
        // `WM_QUIT` after a fixed number of synthetic messages —
        // budget=0 means "run until the user stops or the game
        // calls ExitProcess". Real input from the virtual D-pad /
        // stylus tap arrives through `input_rx` and feeds the
        // synthetic message pump in pocket-winceapi.
        emu.set_synthetic_message_budget(0);

        if let Some(guest_exe) = guest_path {
            emu.set_module_path(guest_exe);
            if let Some((directory, _)) = guest_exe.rsplit_once('\\') {
                emu.set_default_dir(&format!("{directory}\\"));
            }
            if let Some(parent) = exe.parent() {
                emu.mount_read_only_dir("\\Application\\", parent);
                emu.mount_read_only_dir("\\Program Files\\Game\\", parent);
            }
        }
        if let Some(process) = emu.process_mut() {
            process.state.process_launch_enabled = true;
        }
        hook.launch_context = Some(LaunchContext {
            runner: self.clone(), library_root: library_root.clone(),
            game: game.clone(), card_root: card_root.clone(),
        });
        hook.pid = emu.process().unwrap().state.object_handles.process_id();
        if let Some(startup) = startup.as_mut() {
            if let Err(error) = emu.set_startup_command_line(&startup.command_line) {
                startup.error = 8;
                summary_lines.push(format!("command line failed: {error:#}"));
                return (RunOutcome { summary: summary_lines.join("\n"), framebuffer: None },0xc0000017);
            }
            if let Some(ack) = startup.ack.take() { let _ = ack.send(Ok(())); }
            // The parent commits PROCESS_INFORMATION before releasing startup.
            while !startup.table.start_allowed() && !hook.stop_all.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            if hook.stop_all.load(Ordering::Acquire) { emu.process_mut().unwrap().state.should_stop = true; }
        }
        emu.start_audio();
        let run_result = emu.run_with_hook(hook);
        match &run_result {
            Ok(()) => summary_lines.push("Emulator exited cleanly.".to_string()),
            Err(e) => summary_lines.push(format!("Emulator stopped: {e:#}")),
        }
        emu.stop_audio();

        if let Some(process) = emu.process() {
            if let Err(e) = process.state.registry.flush() {
                summary_lines.push(format!("Cannot save device registry: {e}"));
                log::error!("Cannot save device registry: {e}");
            }
        }

        let framebuffer = emu.process().and_then(|p| {
            (!p.state.framebuffer.is_all_black())
                .then(|| FrameSnapshot::from_framebuffer(&p.state.framebuffer))
        });
        let exit_code = if run_result.is_ok() {
            emu.process().and_then(|p| p.state.process_exit_code).unwrap_or(0)
        } else { 0xc0000005 };
        drop(emu); // Refund the parent's private RAM while its children remain alive.
        hook.finish_children();
        (RunOutcome {
            summary: summary_lines.join("\n"),
            framebuffer,
        }, exit_code)
    }
}

#[cfg(feature = "unicorn")]
fn build_unicorn_for_machine(machine: u16) -> anyhow::Result<Emulator> {
    if matches!(
        machine,
        pocket_core::pe::machine::MIPS_R3000 | pocket_core::pe::machine::MIPS_R4000
    ) {
        Emulator::with_unicorn_cpu_for_arch(pocket_core::cpu::Arch::Mips)
    } else {
        Emulator::with_unicorn_cpu()
    }
}

#[cfg(not(feature = "unicorn"))]
fn build_unicorn_for_machine(_machine: u16) -> anyhow::Result<Emulator> {
    Err(anyhow::anyhow!(
        "binary was not compiled with the `unicorn` feature"
    ))
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub summary: String,
    pub framebuffer: Option<FrameSnapshot>,
}

#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl FrameSnapshot {
    fn from_framebuffer(fb: &pocket_core::kernel::Framebuffer) -> Self {
        Self {
            width: fb.width,
            height: fb.height,
            rgba: fb.snapshot_rgba8888(),
        }
    }

    /// Like [`FrameSnapshot::from_framebuffer`] but reuses an existing
    /// `rgba` buffer the caller owns. Saves the per-frame 300 KiB
    /// allocation that the naive constructor does.
    fn fill_from_framebuffer(fb: &pocket_core::kernel::Framebuffer, scratch: &mut Vec<u8>) -> Self {
        fb.snapshot_rgba8888_into(scratch);
        Self {
            width: fb.width,
            height: fb.height,
            // `std::mem::take` hands ownership of the scratch buffer
            // to the snapshot we are about to ship across the
            // channel and leaves an empty `Vec` behind. The next
            // call resizes it back up — same allocation, no churn.
            rgba: std::mem::take(scratch),
        }
    }
}

/// Command pushed by the GUI thread into the running emulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputCommand {
    /// User input (tap / D-pad / key) to forward to the guest.
    Input(InputEvent),
    /// "Back to library" button — ask the run loop to stop cleanly.
    Stop,
}

/// Frame hook that:
///  - pushes one [`FrameSnapshot`] across `frame_tx` every time the
///    guest produces a new frame, so the GUI paints a live preview;
///  - drains pending [`InputCommand`]s from `input_rx` and forwards
///    them into the kernel's input queue / stop flag.
#[derive(Clone)]
struct LaunchContext {
    runner: Runner,
    library_root: PathBuf,
    game: GameEntry,
    card_root: PathBuf,
}

struct ProcessStartup {
    ack: Option<Sender<Result<(),u32>>>,
    table: pocket_core::kernel::handles::HandleTable,
    command_line: String,
    error: u32,
}
impl Drop for ProcessStartup {
    fn drop(&mut self) { if let Some(ack) = self.ack.take() { let _ = ack.send(Err(self.error)); } }
}
struct ChildJob {
    table: pocket_core::kernel::handles::HandleTable,
    thread: std::thread::JoinHandle<(RunOutcome,u32)>,
}

struct RunHook {
    frame_tx: Option<Sender<FrameSnapshot>>,
    input_rx: Option<Arc<Mutex<Receiver<InputCommand>>>>,
    stop_all: Arc<AtomicBool>,
    foreground: Arc<AtomicU32>,
    pid: u32,
    children: Vec<ChildJob>,
    last_frame: u64,
    frame_send_failed: bool,
    input_disconnected: bool,
    /// Wall-clock timestamp of the last snapshot we emitted. Used to
    /// rate-limit the snapshot conversion + channel send to roughly
    /// `FRAME_PUSH_INTERVAL` (60 fps) so a chatty guest that bumps
    /// `frame_counter` thousands of times per logical frame doesn't
    /// pin the runner thread doing redundant RGB565→RGBA8888 work
    /// the GUI never gets to paint anyway.
    last_emit_at: Option<Instant>,
    /// Reusable host-side scratch buffer for the RGBA conversion.
    /// `FrameSnapshot::fill_from_framebuffer` swaps it with the
    /// outgoing `Vec<u8>` so we keep amortising the same allocation
    /// for the entire run instead of growing one per delivered frame.
    scratch: Vec<u8>,
    saw_non_black: bool,
    stopped_by_user: bool,
    launch_context: Option<LaunchContext>,
}

impl RunHook {
    fn run_pending_child(&mut self, state: &mut KernelState) {
        let Some(request) = state.pending_process_launch.take() else { return; };
        let context = self.launch_context.clone();
        let relative = context.as_ref().and_then(|context| {
            let child = request.executable.canonicalize().ok()?;
            // CreateProcess uses a path already resolved through the guest VFS.
            // SDCreateProcess keeps its validated foreground card restriction.
            if request.concurrent { return Some(child); }
            let root = context.card_root.canonicalize().ok()?;
            if !child.starts_with(root) { return None; }
            let game_dir = context.library_root.join(context.game.relative_dir()).canonicalize().ok()?;
            child.strip_prefix(game_dir).ok().map(PathBuf::from)
        });
        if request.concurrent {
            let result = if let (Some(context),Some(relative)) = (context,relative) {
                let mut child_game = context.game.clone(); child_game.executable = relative;
                let handle_context = state.child_handle_context(request.process_handle).unwrap();
                let table = handle_context.table.clone();
                let child_table = table.clone(); let parent_pid = self.pid;
                let foreground = self.foreground.clone(); let mut child_hook = self.child_hook();
                let ram = state.memory_division.clone(); let command_line = request.command_line.clone();
                let guest_path = request.guest_path.clone();
                let (ready_tx,ready_rx) = std::sync::mpsc::channel();
                let startup = ProcessStartup { ack: Some(ready_tx), table: child_table.clone(), command_line, error: 193 };
                let spawned = std::thread::Builder::new().name(format!("pockethle-process-{}",table.process_id())).spawn(move || {
                    let result = context.runner.run_process(&context.library_root,&child_game,&context.card_root,
                        Some(&guest_path),ram,Some(handle_context),Some(startup),&mut child_hook);
                    child_table.complete_process_exit(result.1);
                    if child_table.exit_code(child_table.process_id(),Some(0)).is_none() { child_table.set_exit(Some(0),result.1); }
                    child_table.mark_inactive();
                    let next = child_table.focus_after_exit(parent_pid);
                    let _ = foreground.compare_exchange(child_table.process_id(),next,Ordering::AcqRel,Ordering::Acquire);
                    result
                });
                match spawned {
                    Ok(thread) => {
                        self.children.push(ChildJob { table: table.clone(),thread });
                        match ready_rx.recv_timeout(Duration::from_secs(30)) {
                            Ok(Ok(())) => { self.foreground.store(table.process_id(),Ordering::Release); Ok(request.process_handle) }
                            Ok(Err(error)) => Err(error),
                            Err(_) => { table.terminate_remote_process(table.process_id(),0xc0000001);table.allow_start();Err(1460) }
                        }
                    }
                    Err(_) => { table.set_exit(None,0xc0000017);table.mark_inactive();Err(8) }
                }
            } else {
                if let Some(table) = state.object_handles.child(request.process_handle) { table.mark_inactive(); }
                Err(5)
            };
            state.process_launch_results.insert(request.call_key,result);
            return;
        }
        let mut exit_code = 0xc0000135;
        if let (Some(context), Some(relative)) = (context, relative) {
            let mut child_game = context.game.clone();
            child_game.executable = relative;
            state.audio.suspend_output();
            let saved = pocket_core::suspend_host_session();
            let mut child_hook = self.child_hook();
            let child_pid = state.object_handles.child_id(request.process_handle).unwrap();
            self.foreground.store(child_pid,Ordering::Release);
            log::info!("parent suspended; entering child {}", request.guest_path);
            let (outcome, code) = context.runner.run_process(&context.library_root,
                &child_game, &context.card_root, Some(&request.guest_path), state.memory_division.clone(), state.child_handle_context(request.process_handle), None, &mut child_hook);
            exit_code = code;
            self.foreground.store(self.pid,Ordering::Release);
            self.input_disconnected = child_hook.input_disconnected;
            self.stopped_by_user |= child_hook.stopped_by_user;
            // run_process has dropped its child Emulator and its media first.
            drop(saved);
            state.audio.resume_output();
            log::info!("child finished with code 0x{exit_code:08x}; resuming preserved parent");
            if exit_code != 0 { log::warn!("child outcome: {}", outcome.summary); }
            self.reset_process();
            // Repaint the preserved parent frame even if its counter is static.
            state.framebuffer.frame_counter = state.framebuffer.frame_counter.wrapping_add(1);
            state.pending_input.clear();
            state.pressed_keys.fill(false);
            state.held_keys.clear();
            state.key_repeat_next_ms = None;
        } else {
            log::warn!("child launch refused: path outside card or missing runner context");
        }
        if let Some(table) = state.object_handles.child(request.process_handle) {
            table.complete_process_exit(exit_code); table.set_exit(Some(0), exit_code); table.mark_inactive();
        }
        state.sync_transferred_handles();
        if let Some(child) = state.child_processes.get_mut(&request.process_handle) {
            child.exit_code = Some(exit_code);
        }
        for handle in [request.process_handle, request.thread_handle] {
            if let Some(mut event) = state.events.get_mut(&handle) { event.signalled = true; }
        }
    }

    fn drain_input(&mut self, pending: &mut std::collections::VecDeque<InputEvent>) -> bool {
        let mut stop_requested = self.stop_all.load(Ordering::Acquire);
        if !self.input_disconnected {
            if let Some(rx) = self.input_rx.as_ref() {
                let rx = rx.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    match rx.try_recv() {
                        Ok(InputCommand::Input(ev)) => pending.push_back(ev),
                        Ok(InputCommand::Stop) => stop_requested = true,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            self.input_disconnected = true;
                            break;
                        }
                    }
                }
            }
        }
        self.stopped_by_user |= stop_requested || self.input_disconnected;
        if self.stopped_by_user { self.stop_all.store(true,Ordering::Release); }
        stop_requested || self.input_disconnected
    }

    fn child_hook(&self) -> Self {
        let mut hook = Self::new(self.frame_tx.clone(),None);
        hook.input_rx = self.input_rx.clone(); hook.stop_all = self.stop_all.clone();
        hook.foreground = self.foreground.clone(); hook
    }
    fn finish_children(&mut self) {
        for job in self.children.drain(..) {
            if !job.table.start_allowed() {
                job.table.terminate_remote_process(job.table.process_id(),0xc0000001); job.table.allow_start();
            }
            if let Err(_) = job.thread.join() { self.stop_all.store(true,Ordering::Release); }
        }
    }
    fn reap_children(&mut self) {
        let mut index = 0;
        while index < self.children.len() {
            if self.children[index].thread.is_finished() {
                let job = self.children.swap_remove(index);
                if job.thread.join().is_err() {
                    job.table.complete_process_exit(0xc0000005); job.table.set_exit(Some(0),0xc0000005);job.table.mark_inactive();
                }
            } else { index += 1; }
        }
    }

    fn reset_process(&mut self) {
        self.last_frame = 0;
        self.last_emit_at = None;
        self.saw_non_black = false;
    }

    fn new(
        frame_tx: Option<Sender<FrameSnapshot>>,
        input_rx: Option<Receiver<InputCommand>>,
    ) -> Self {
        Self {
            frame_tx,
            input_rx: input_rx.map(|rx| Arc::new(Mutex::new(rx))),
            stop_all: Arc::new(AtomicBool::new(false)),
            foreground: Arc::new(AtomicU32::new(1)),
            pid: 1,
            children: Vec::new(),
            last_frame: 0,
            frame_send_failed: false,
            input_disconnected: false,
            last_emit_at: None,
            scratch: Vec::new(),
            saw_non_black: false,
            stopped_by_user: false,
            launch_context: None,
        }
    }
}

impl FrameHook for RunHook {
    fn on_frame(&mut self, state: &mut KernelState) -> FrameAction {
        self.reap_children();
        let active = self.foreground.load(Ordering::Acquire) == self.pid;
        let mut stop_requested = if active { self.drain_input(&mut state.pending_input) }
            else { self.stop_all.load(Ordering::Acquire) };
        if !stop_requested { self.run_pending_child(state); }
        stop_requested |= self.stopped_by_user;

        // Stream the latest framebuffer up to the GUI, but at most
        // once per `FRAME_PUSH_INTERVAL`. The hook is invoked after
        // every dispatched WinCE call; without throttling a single
        // game tick that does ~2k `BitBlt`s would generate ~2k
        // 300 KiB snapshots — the work that turned a logical 60 fps
        // game into a sub-1 fps preview in the desktop launcher.
        if !self.frame_send_failed && self.foreground.load(Ordering::Acquire) == self.pid {
            if let Some(tx) = self.frame_tx.as_ref() {
                let counter = state.framebuffer.frame_counter;
                if counter != self.last_frame {
                    if state.framebuffer.is_all_black() && self.saw_non_black {
                        return if stop_requested {
                            state.should_stop = true;
                            FrameAction::Stop
                        } else {
                            FrameAction::Continue
                        };
                    }
                    self.saw_non_black = !state.framebuffer.is_all_black();
                    let now = Instant::now();
                    let due = self
                        .last_emit_at
                        .map(|t| now.duration_since(t) >= FRAME_PUSH_INTERVAL)
                        .unwrap_or(true);
                    if due {
                        self.last_frame = counter;
                        self.last_emit_at = Some(now);
                        let snapshot = FrameSnapshot::fill_from_framebuffer(
                            &state.framebuffer,
                            &mut self.scratch,
                        );
                        if tx.send(snapshot).is_err() {
                            // GUI thread dropped the receiver — the user
                            // closed the run screen / quit the launcher.
                            self.frame_send_failed = true;
                        }
                    }
                }
            }
        }

        if stop_requested {
            state.should_stop = true;
            FrameAction::Stop
        } else {
            FrameAction::Continue
        }
    }
}

#[cfg(test)]
mod launcher_return_tests {
    use super::*;

    #[test]
    fn launcher_return_library_stop_ends_the_whole_session() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut hook = RunHook::new(None, Some(rx));
        tx.send(InputCommand::Stop).unwrap();
        assert!(hook.drain_input(&mut std::collections::VecDeque::new()));
        assert!(hook.stopped_by_user);
    }

    #[test]
    fn launcher_return_ui_disconnect_ends_the_whole_session() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut hook = RunHook::new(None, Some(rx));
        drop(tx);
        assert!(hook.drain_input(&mut std::collections::VecDeque::new()));
        assert!(hook.stopped_by_user);
    }
    #[cfg(feature = "unicorn")]
    #[test]
    fn desktop_runner_executes_parallel_children_and_surviving_orphan() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("pockethle-runner-proc-{}-{nonce}",std::process::id()));
        let card = root.join("games/probe/extracted");let dir = card.join("GZRT999999");
        std::fs::create_dir_all(&dir).unwrap();
        for (name,bytes) in [
            ("proctest.exe",include_bytes!("../../../tools/ramtest/dist/GZRT999999/proctest.exe").as_slice()),
            ("procworker.exe",include_bytes!("../../../tools/ramtest/dist/GZRT999999/procworker.exe").as_slice()),
            ("badproc.exe",include_bytes!("../../../tools/ramtest/dist/GZRT999999/badproc.exe").as_slice())] {
            std::fs::write(dir.join(name),bytes).unwrap();
        }
        std::fs::write(dir.join("GZRT999999"),999999u32.to_le_bytes()).unwrap();
        let mut settings=pocket_library::GameSettings::default();
        settings.cpu_backend=CpuBackendPref::Unicorn;settings.max_slices=3_000_000;settings.halt_on_unimplemented=true;
        let game=GameEntry { id:"probe".into(),display_name:"Process diagnostic".into(),provider:None,
            executable:PathBuf::from("extracted/GZRT999999/proctest.exe"),source_cab:"diagnostic.zip".into(),
            install_dir:None,install_dirs:vec![],save_prefix:None,registry:vec![],imported_at:0,settings,icon:None,companions:vec![] };
        let mut hook=RunHook::new(None,None);
        let (outcome,code)=Runner::new().run_process(&root,&game,&card,None,None,None,None,&mut hook);
        assert_eq!(code,77,"{}",outcome.summary);
        for (name,expected) in [("PROCTEST.TXT","PROCTEST_RESULT PASS"),("ORPHANTEST.TXT","ORPHANTEST_RESULT PASS")] {
            let path=std::fs::read_dir(root.join("flash")).unwrap().map(|e|e.unwrap().path())
                .find(|p|p.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case(name)).unwrap();
            let report=std::fs::read_to_string(path).unwrap();println!("{report}");
            assert!(report.contains(expected)&&!report.contains("FAIL"),"{report}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }


    #[cfg(feature = "unicorn")]
    #[test]
    fn desktop_runner_executes_vfs_diagnostic_and_cleans_probe_files() {
        let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root=std::env::temp_dir().join(format!("pockethle-runner-vfs-{nonce}"));
        let card=root.join("games/probe/extracted");let dir=card.join("GZVT999998");std::fs::create_dir_all(&dir).unwrap();
        for (n,b) in [
            ("AUTORUN.EXE",include_bytes!("../../../tools/vfstest/dist/GZVT999998/AUTORUN.EXE").as_slice()),
            ("vfsworker.exe",include_bytes!("../../../tools/vfstest/dist/GZVT999998/vfsworker.exe").as_slice()),
            ("asset.bin",include_bytes!("../../../tools/vfstest/dist/GZVT999998/asset.bin").as_slice())]{std::fs::write(dir.join(n),b).unwrap();}
        std::fs::write(dir.join("GZVT999998"),999998u32.to_le_bytes()).unwrap();
        let mut settings=pocket_library::GameSettings::default();settings.cpu_backend=CpuBackendPref::Unicorn;
        settings.max_slices=3_000_000;settings.halt_on_unimplemented=true;
        let game=GameEntry{id:"probe".into(),display_name:"VFS diagnostic".into(),provider:None,
            executable:PathBuf::from("extracted/GZVT999998/AUTORUN.EXE"),source_cab:"diagnostic.zip".into(),
            install_dir:None,install_dirs:vec![],save_prefix:None,registry:vec![],imported_at:0,settings,icon:None,companions:vec![]};
        let (tx,rx)=std::sync::mpsc::channel();let completed=Arc::new(AtomicBool::new(false));let finished=completed.clone();
        let input=std::thread::spawn(move||{
            while !finished.load(Ordering::Acquire){
                let _=tx.send(InputCommand::Input(InputEvent::KeyDown{vk:0x0d}));
                let _=tx.send(InputCommand::Input(InputEvent::KeyUp{vk:0x0d}));
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let mut hook=RunHook::new(None,Some(rx));
        let (outcome,code)=Runner::new().run_process(&root,&game,&card,None,None,None,None,&mut hook);
        completed.store(true,Ordering::Release);input.join().unwrap();assert_eq!(code,0,"{}",outcome.summary);
        let entries:Vec<_>=std::fs::read_dir(root.join("flash")).unwrap().map(|e|e.unwrap().path()).collect();
        assert_eq!(entries.len(),1,"all fixture data must be removed");let report=std::fs::read_to_string(&entries[0]).unwrap();
        assert!(report.contains("VFSTEST_RESULT PASS")&&!report.contains("FAIL "),"{report}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn child_stop_cancels_background_parent_and_other_children() {
        let (tx,rx)=std::sync::mpsc::channel();let parent=RunHook::new(None,Some(rx));
        let mut child=parent.child_hook();let sibling=parent.child_hook();
        tx.send(InputCommand::Stop).unwrap();
        assert!(child.drain_input(&mut std::collections::VecDeque::new()));
        assert!(parent.stop_all.load(Ordering::Acquire));assert!(sibling.stop_all.load(Ordering::Acquire));
    }

}
