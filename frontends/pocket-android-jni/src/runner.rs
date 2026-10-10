//! Session-based emulator runner exposed to the Android JNI layer.
//!
//! The desktop GUI ([`pocket_desktop::runner`]) drives the emulator
//! on a background thread and streams a [`FrameSnapshot`] to the UI
//! every time the guest produces a new framebuffer. The Android
//! frontend used to do something fundamentally different: a single
//! blocking JNI call (`runGame`) that ran the emulator to
//! completion, captured the **final** framebuffer, and only then
//! returned. With the trace-only stub backend that returned in a
//! few milliseconds and the user just saw a static screenshot. With
//! the real Unicorn backend wired up in
//! [PR #11](https://github.com/j92580498-max/PocketHLE/pull/11) the
//! emulator now actually executes ARM code and reaches the menu, so
//! `runGame` would happily churn through 1024 dispatch slices ×
//! 1 000 000 instructions/slice on the phone CPU before returning —
//! visually that looks identical to a hang ("infinite loading
//! spinner") and there is no way for the user to push input or
//! quit. That's the symptom this module fixes.
//!
//! The new flow mirrors the desktop runner, just over JNI:
//!
//! 1. Kotlin calls [`start`] with the library root and game id. We
//!    spawn a worker thread that owns the [`Emulator`] and runs it
//!    with a [`FrameHook`]. The worker shares a [`SessionState`]
//!    with the UI thread:
//!      * a `Mutex<Option<FrameSnapshot>>` slot holding the most
//!        recent framebuffer — the Kotlin polling loop drains it
//!        with [`poll_frame`];
//!      * an [`InputCommand`] channel — Kotlin pushes touches,
//!        D-pad presses and the "stop" signal with [`send_input`] /
//!        [`request_stop`].
//! 2. When Kotlin's [`finish`] runs (Back button or
//!    `onDestroy`), we set `should_stop`, join the worker thread
//!    and return a textual summary that the UI shows in its status
//!    panel.
//!
//! Sessions are owned by Kotlin via a `jlong` handle. The handle is
//! a `Box::into_raw`'d pointer to a [`Session`]; [`finish`]
//! reconstructs the box and drops it. The pointer is opaque to
//! Kotlin and, crucially, the JNI methods bounds-check it against
//! `null` and the dispatch refuses to operate on a freed session
//! (we set the in-flight `running` flag to `false` once the worker
//! exits, which lets the polling loop on the UI thread notice the
//! session ended and stop calling back in).

use pocket_core::kernel::{
    handles::HandleTable, memory_division::MemoryDivision, ProcessHandleContext,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Context;
use pocket_core::kernel::{FrameAction, FrameHook, InputEvent, KernelState};
use pocket_core::Emulator;
use pocket_library::{is_gizmondo_game, CpuBackendPref, GameEntry, Library};

const FRAME_PUSH_INTERVAL: Duration = Duration::from_millis(16);

/// Snapshot of the guest framebuffer plus the dimensions Kotlin
/// needs to paint it onto a `SurfaceView`.
#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl FrameSnapshot {
    pub(crate) fn from_framebuffer(fb: &pocket_core::kernel::Framebuffer) -> Self {
        Self {
            width: fb.width,
            height: fb.height,
            rgba: fb.snapshot_rgba8888(),
        }
    }

    fn from_framebuffer_into(fb: &pocket_core::kernel::Framebuffer, scratch: &mut Vec<u8>) -> Self {
        fb.snapshot_rgba8888_into(scratch);
        Self {
            width: fb.width,
            height: fb.height,
            rgba: std::mem::take(scratch),
        }
    }
}

/// Kotlin → emulator command. Mirrors `pocket_desktop::runner::InputCommand`.
#[derive(Debug, Clone, Copy)]
pub enum InputCommand {
    Input(InputEvent),
    Stop,
}

/// Shared between the worker thread and the UI thread for the
/// lifetime of one game session.
pub(crate) struct SessionState {
    stop_all: Arc<AtomicBool>,
    /// Latest framebuffer the guest produced. The polling loop on
    /// the UI thread drains this slot and paints it; a write
    /// overwrites whatever was there because the UI only ever
    /// cares about the newest frame.
    latest_frame: Mutex<Option<FrameSnapshot>>,
    /// `true` while the worker thread is still running. Flipped to
    /// `false` exactly once, just before the worker returns.
    running: Mutex<bool>,
    /// Final summary string. Populated by the worker right before
    /// it exits; read by [`finish`] after the join.
    summary: Mutex<Option<String>>,
    /// Pull handle on the guest's mixed PCM, published by the worker
    /// once the emulator exists. Android has no cpal device, so the
    /// Kotlin side drains this from an `AudioTrack` feeder thread
    /// instead of the kernel pushing to a host stream.
    audio: Mutex<Option<pocket_core::kernel::AudioTap>>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            stop_all: Arc::new(AtomicBool::new(false)),
            latest_frame: Mutex::new(None),
            running: Mutex::new(true),
            summary: Mutex::new(None),
            audio: Mutex::new(None),
        }
    }
}

/// Owned by Kotlin via a `Box::into_raw`'d pointer.
pub struct Session {
    state: Arc<SessionState>,
    input_tx: Sender<InputCommand>,
    worker: Option<JoinHandle<()>>,
}

impl Session {
    /// Move the latest framebuffer out of the shared slot.
    pub fn poll_frame(&self) -> Option<FrameSnapshot> {
        self.state
            .latest_frame
            .lock()
            .ok()
            .and_then(|mut g| g.take())
    }

    /// Copy up to `dst.len()` interleaved 16-bit samples out of the
    /// guest's mixer queue.
    pub fn poll_audio(&self, dst: &mut [i16]) -> usize {
        let guard = match self.state.audio.lock() {
            Ok(g) => g,
            Err(_) => return 0,
        };
        guard.as_ref().map_or(0, |tap| tap.drain_into(dst))
    }

    /// `(sample_rate, channels)` the guest opened its output with.
    pub fn audio_format(&self) -> Option<(u32, u16)> {
        let guard = self.state.audio.lock().ok()?;
        let tap = guard.as_ref()?;
        if !tap.format_ready() {
            return None;
        }
        let fmt = tap.guest_format();
        Some((fmt.sample_rate, fmt.channels))
    }

    pub fn send_input(&self, cmd: InputCommand) {
        // The receiver only goes away after the worker exits, in
        // which case we don't care about the input anymore.
        let _ = self.input_tx.send(cmd);
    }

    pub fn request_stop(&self) {
        self.state.stop_all.store(true, Ordering::Release);
        self.send_input(InputCommand::Stop);
    }

    pub fn is_running(&self) -> bool {
        self.state.running.lock().map(|g| *g).unwrap_or(false)
    }

    /// Join the worker thread (with a stop signal already sent) and
    /// return the textual summary captured while it was running.
    pub fn finish(mut self) -> String {
        // Belt-and-braces: ask the worker to stop in case the
        // caller forgot to.
        self.request_stop();
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        self.state
            .summary
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(|| "(no summary captured)".to_string())
    }
}

/// Spawn the worker thread that drives the emulator for a single
/// game. The returned `Session` is the handle Kotlin holds.
pub fn start(library_root: PathBuf, game_id: String) -> anyhow::Result<Session> {
    let lib = Library::open(&library_root).context("Library::open")?;
    let entry = lib
        .get(&game_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown game id {game_id}"))?;

    let state = Arc::new(SessionState::new());
    let (input_tx, input_rx) = channel::<InputCommand>();

    let state_for_worker = Arc::clone(&state);
    let worker = std::thread::Builder::new()
        .name(format!("pockethle-emu-{game_id}"))
        .spawn(move || {
            let summary =
                run_game_to_completion(&library_root, &entry, &state_for_worker, input_rx);
            if let Ok(mut slot) = state_for_worker.summary.lock() {
                *slot = Some(summary);
            }
            if let Ok(mut running) = state_for_worker.running.lock() {
                *running = false;
            }
        })
        .context("spawn pockethle worker thread")?;

    Ok(Session {
        state,
        input_tx,
        worker: Some(worker),
    })
}

/// Runs the emulator from start to finish, returning a summary
/// suitable for the UI's status panel. Streams framebuffers and
/// drains UI input via [`SessionHook`].
fn run_game_to_completion(
    library_root: &std::path::Path,
    entry: &GameEntry,
    state: &Arc<SessionState>,
    input_rx: Receiver<InputCommand>,
) -> String {
    let card_root = entry.extracted_dir(library_root);
    let mut hook = SessionHook::new(Arc::clone(state), input_rx);
    run_process(
        library_root,
        entry,
        &card_root,
        None,
        None,
        None,
        None,
        &mut hook,
    )
    .0
}

#[derive(Clone)]
struct LaunchContext {
    library_root: PathBuf,
    entry: GameEntry,
    card_root: PathBuf,
}

struct ProcessStartup {
    ack: Option<Sender<Result<(), u32>>>,
    table: HandleTable,
    command_line: String,
    error: u32,
}
impl Drop for ProcessStartup {
    fn drop(&mut self) {
        if let Some(ack) = self.ack.take() {
            let _ = ack.send(Err(self.error));
        }
    }
}
struct ChildJob {
    table: HandleTable,
    thread: JoinHandle<(String, u32)>,
}

fn run_process(
    library_root: &std::path::Path,
    entry: &GameEntry,
    card_root: &std::path::Path,
    guest_path: Option<&str>,
    inherited_ram: Option<MemoryDivision>,
    handle_context: Option<ProcessHandleContext>,
    mut startup: Option<ProcessStartup>,
    hook: &mut SessionHook,
) -> (String, u32) {
    let state = Arc::clone(&hook.state);
    let mut summary_lines = vec![
        format!("Game: {}", entry.display_name),
        format!("Backend: {}", entry.settings.cpu_backend.label()),
    ];
    let exe = if guest_path.is_some() {
        entry.executable_path(library_root)
    } else {
        entry.launch_path(library_root)
    };
    let image = pocket_core::pe::load_file(&exe);
    let machine = image
        .as_ref()
        .map(|image| image.machine)
        .unwrap_or(pocket_core::pe::machine::ARM);
    summary_lines.push(format!("Executable: {}", exe.display()));

    // Managed .NET Compact Framework images cannot run on the native
    // emulation path: there is no CLR to interpret the IL, so the
    // emulator would sit on a black frame forever. The desktop CLI
    // (pocket-cli) runs these through a host .NET runtime instead —
    // tell the user instead of leaving them on a black screen.
    if let Ok(image) = &image {
        if let Some(runtime) = &image.managed_runtime {
            let screen = entry.settings.screen.size();
            if guest_path.is_none() && crate::managed_game::supports(&exe) {
                summary_lines.push(
                    "Managed image: using the Android vAlienAttack compatibility renderer."
                        .to_string(),
                );
                summary_lines.push(format!("Screen: {}x{}", screen.0, screen.1));
                let renderer_summary =
                    crate::managed_game::run(&exe, &state, hook.take_input_receiver(), screen);
                return (
                    format!("{}\n{renderer_summary}", summary_lines.join("\n")),
                    0,
                );
            }
            summary_lines.push(format!(
                "Managed image: CLR metadata {runtime} (.NET Compact Framework). Android has no general CLR backend for this title."
            ));
            return (summary_lines.join("\n"), 0xc0000135);
        }
    }

    // Same Stub→Unicorn promotion logic as `pocket_desktop::runner`:
    // a user who clicks "Run" wants the real ARM core regardless of
    // what is persisted in their library.json.
    let requested_backend = entry.settings.cpu_backend;
    let mut effective_backend = requested_backend;
    let mut emu = match requested_backend {
        CpuBackendPref::Unicorn => match build_unicorn_for_machine(machine) {
            Ok(emu) => emu,
            Err(e) => {
                summary_lines.push(format!("Unicorn unavailable: {e}"));
                return (summary_lines.join("\n"), 0xc0000135);
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
            Err(e) => return (format!("Unicorn unavailable: {e}"), 0xc0000135),
        },
    };
    summary_lines.push(format!("Effective backend: {}", effective_backend.label()));

    emu.set_halt_on_unimplemented(entry.settings.halt_on_unimplemented);
    emu.max_slices = entry.settings.max_slices;
    emu.instruction_budget_per_slice = entry.settings.instructions_per_slice;

    if let Err(e) = emu.load_pe(&exe) {
        summary_lines.push(format!("load_pe failed: {e:#}"));
        return (summary_lines.join("\n"), 0xc0000135);
    }

    let is_gizmondo = is_gizmondo_game(entry, library_root);
    if let Some(context) = handle_context {
        context.table.defer_process_exit();
        emu.process_mut()
            .unwrap()
            .state
            .attach_handle_context(context);
    }
    let ram = inherited_ram.or_else(|| is_gizmondo.then(MemoryDivision::gizmondo_sdk_default));
    if !emu.set_memory_division(ram) {
        if let Some(startup) = startup.as_mut() {
            startup.error = 8;
        }
        summary_lines.push("Insufficient device RAM to load process".to_string());
        return (summary_lines.join("\n"), 0xc0000135);
    }
    let registry_path = library_root.join(if is_gizmondo {
        "registry-gizmondo.json"
    } else {
        "registry-pocketpc.json"
    });
    let launcher_config = pocket_library::Library::open(library_root)
        .map(|l| l.config().clone())
        .unwrap_or_default();
    emu.set_unimplemented_api_sink(Box::new(crate::unimplemented_log::UnimplementedLog::new(
        library_root.join("pockethle-unimplemented.log"),
        entry.display_name.clone(),
        exe.display().to_string(),
        Arc::new(std::sync::atomic::AtomicBool::new(
            launcher_config.log_unimplemented_apis,
        )),
    )));
    log::set_max_level(match launcher_config.verbosity {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Info,
        2 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    });
    let hardware = (
        launcher_config.bluetooth_enabled,
        launcher_config.camera_enabled,
        launcher_config.gps_enabled,
    );
    if let Some(process) = emu.process_mut() {
        process.state.vfs.bluetooth.service.set_allowed(hardware.0);
        process.state.vfs.camera_service().set_allowed(hardware.1);
        process.state.vfs.gps_service().set_allowed(hardware.2);
        if is_gizmondo && launcher_config.gps_fixed_enabled {
            let gps = process.state.vfs.gps_service();
            if let Err(e) = gps.set_fixed_position(Some((
                launcher_config.gps_fixed_latitude,
                launcher_config.gps_fixed_longitude,
            ))) {
                return (format!("Invalid fixed GPS coordinates: {e}"), 0xc000000d);
            }
            gps.set_allowed(true);
            log::info!("GPS1 fixed simulation activated on Android");
        }
        if let Err(e) = process.state.registry.configure_persistence(&registry_path) {
            summary_lines.push(format!("Cannot load device registry: {e}"));
            return (summary_lines.join("\n"), 0xc0000135);
        }
        if is_gizmondo {
            process
                .state
                .internet
                .set_gprs_enabled(launcher_config.gprs_enabled);
            if let Err(e) = pocket_core::kernel::colors::configure(
                &mut process.state,
                &launcher_config.colors_server_url,
                &launcher_config.colors_terminal_id,
            ) {
                summary_lines.push(e);
                return (summary_lines.join("\n"), 0xc0000135);
            }
        }
    }
    for value in &entry.registry {
        let registry_value = if let Some(text) = value.string.as_deref() {
            pocket_core::kernel::registry::RegistryValue::Sz(text.to_string())
        } else if let Some(number) = value.dword {
            pocket_core::kernel::registry::RegistryValue::Dword(number)
        } else {
            continue;
        };
        let saved = emu
            .process()
            .and_then(|p| p.state.registry.value(&value.key, &value.name));
        if saved.is_none() || value.name.eq_ignore_ascii_case("InstallDir") {
            emu.set_registry_value(&value.key, &value.name, registry_value);
        }
    }

    // The tap has to be published before the guest runs: the Kotlin
    // feeder thread starts polling as soon as the surface is up.
    if guest_path.is_none() {
        if let Ok(mut slot) = state.audio.lock() {
            *slot = emu.audio_tap();
        }
    }
    emu.start_audio();
    let (screen_width, screen_height) = if is_gizmondo {
        (320, 240)
    } else {
        entry.settings.screen.size()
    };
    emu.set_screen_size(screen_width, screen_height);
    summary_lines.push(format!("Screen: {screen_width}x{screen_height}"));
    let extracted = entry.extracted_dir(library_root);
    emu.mount_read_only_dir("\\Application\\", &extracted);
    emu.mount_read_only_dir("\\Program Files\\", &extracted);
    emu.mount_read_only_dir("\\Program Files\\Game\\", &extracted);
    if is_gizmondo {
        emu.mount_save_dir("\\Flash Disk\\", library_root.join("flash"));
        emu.mount_read_only_dir("\\SD Card\\", card_root);
        emu.mount_read_only_dir("\\Storage Card\\", card_root);
        emu.mount_read_only_dir("\\SD Card\\Game\\", &extracted);
        emu.mount_read_only_dir("\\Storage Card\\Game\\", &extracted);
        summary_lines.push(format!(
            "Mounted Gizmondo SD-card layout at {}",
            extracted.display()
        ));
        if let Some(name) = entry.executable.file_name().and_then(|name| name.to_str()) {
            let guest_exe = format!("\\SD Card\\{name}");
            emu.set_module_path(&guest_exe);
            emu.set_default_dir("\\SD Card\\");
            summary_lines.push(format!("Module path: {guest_exe}"));
        }
    }
    for prefix in &entry.install_dirs {
        emu.mount_read_only_dir(prefix, &extracted);
    }
    if let Some(prefix) = entry.guest_install_prefix() {
        emu.mount_read_only_dir(&prefix, &extracted);
        // Report the installed path so a game that builds absolute
        // asset paths off its own module name finds its archive.
        if let Some(guest_exe) = entry.guest_exe_path() {
            emu.set_module_path(&guest_exe);
            emu.set_default_dir(&prefix);
        }
        if let Some(save_prefix) = entry.guest_save_prefix() {
            let save_dir = entry.save_dir(library_root);
            emu.mount_save_dir(&save_prefix, &save_dir);
            summary_lines.push(format!(
                "Save data: {} -> {save_prefix:?}",
                save_dir.display()
            ));
        }
    }
    // Match the desktop GUI: a real user is in the loop, so don't
    // auto-fire WM_QUIT after a fixed number of synthetic messages.
    emu.set_synthetic_message_budget(0);

    if let Some(guest_path) = guest_path {
        emu.set_module_path(guest_path);
        if let Some((directory, _)) = guest_path.rsplit_once('\\') {
            emu.set_default_dir(&format!("{directory}\\"));
        }
    }
    let kernel = &mut emu.process_mut().unwrap().state;
    kernel.process_launch_enabled = true;
    hook.pid = kernel.object_handles.process_id();
    hook.launch_context = Some(LaunchContext {
        library_root: library_root.to_path_buf(),
        entry: entry.clone(),
        card_root: card_root.to_path_buf(),
    });
    if let Some(startup) = startup.as_mut() {
        if let Err(error) = emu.set_startup_command_line(&startup.command_line) {
            startup.error = 8;
            return (
                format!("Cannot initialize child command line: {error:#}"),
                0xc0000017,
            );
        }
        if let Some(ack) = startup.ack.take() {
            let _ = ack.send(Ok(()));
        }
        // Commit PROCESS_INFORMATION before any child instruction can execute.
        while !startup.table.start_allowed() && !hook.stop_all.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        if hook.stop_all.load(Ordering::Acquire) {
            emu.process_mut().unwrap().state.should_stop = true;
        }
    }
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

    // Push one last framebuffer so the UI ends up showing whatever
    // the guest left on screen even if it stopped between frames.
    if let Some(p) = emu.process() {
        if !p.state.framebuffer.is_all_black() {
            if hook.foreground.load(Ordering::Acquire) == hook.pid {
                push_frame(
                    &state,
                    FrameSnapshot::from_framebuffer(&p.state.framebuffer),
                );
            }
        }
    }

    let code = if run_result.is_ok() {
        emu.process()
            .and_then(|p| p.state.process_exit_code)
            .unwrap_or(0)
    } else {
        0xc0000005
    };
    // RAMTEST's orphan waits for its parent while remaining alive itself.
    // Refund private RAM and publish parent completion BEFORE joining children.
    let table = emu.process().map(|p| p.state.object_handles.clone());
    drop(emu);
    if let Some(table) = table {
        table.complete_process_exit(code);
    }
    hook.finish_children();
    (summary_lines.join("\n"), code)
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

pub(crate) fn push_frame(state: &Arc<SessionState>, frame: FrameSnapshot) {
    if let Ok(mut slot) = state.latest_frame.lock() {
        *slot = Some(frame);
    }
}

/// Bridges the UI thread (Kotlin) and the running emulator on the
/// worker thread.
struct SessionHook {
    state: Arc<SessionState>,
    input_rx: Arc<Mutex<Receiver<InputCommand>>>,
    stop_all: Arc<AtomicBool>,
    foreground: Arc<AtomicU32>,
    pid: u32,
    children: Vec<ChildJob>,
    launch_context: Option<LaunchContext>,
    last_frame: u64,
    input_disconnected: bool,
    last_emit_at: Option<Instant>,
    scratch: Vec<u8>,
    saw_non_black: bool,
}

impl SessionHook {
    fn run_pending_child(&mut self, state: &mut KernelState) {
        let Some(request) = state.pending_process_launch.take() else {
            return;
        };
        let context = self.launch_context.clone();
        let relative = context.as_ref().and_then(|context| {
            let child = request.executable.canonicalize().ok()?;
            // CreateProcess uses a path already resolved through the guest VFS.
            // SDCreateProcess keeps its validated foreground card restriction.
            if request.concurrent {
                return Some(child);
            }
            let root = context.card_root.canonicalize().ok()?;
            if !child.starts_with(root) {
                return None;
            }
            let game_dir = context
                .library_root
                .join(context.entry.relative_dir())
                .canonicalize()
                .ok()?;
            child.strip_prefix(game_dir).ok().map(PathBuf::from)
        });
        if request.concurrent {
            let result = if let (Some(context), Some(relative)) = (context, relative) {
                let mut child_game = context.entry.clone();
                child_game.executable = relative;
                let handle_context = state.child_handle_context(request.process_handle).unwrap();
                let table = handle_context.table.clone();
                let child_table = table.clone();
                let parent_pid = self.pid;
                let foreground = self.foreground.clone();
                let mut child_hook = self.child_hook();
                let ram = state.memory_division.clone();
                let command_line = request.command_line.clone();
                let guest_path = request.guest_path.clone();
                let (ready_tx, ready_rx) = std::sync::mpsc::channel();
                let startup = ProcessStartup {
                    ack: Some(ready_tx),
                    table: child_table.clone(),
                    command_line,
                    error: 193,
                };
                let spawned = std::thread::Builder::new()
                    .name(format!("pockethle-process-{}", table.process_id()))
                    .spawn(move || {
                        let result = run_process(
                            &context.library_root,
                            &child_game,
                            &context.card_root,
                            Some(&guest_path),
                            ram,
                            Some(handle_context),
                            Some(startup),
                            &mut child_hook,
                        );
                        child_table.complete_process_exit(result.1);
                        if child_table
                            .exit_code(child_table.process_id(), Some(0))
                            .is_none()
                        {
                            child_table.set_exit(Some(0), result.1);
                        }
                        child_table.mark_inactive();
                        let next = child_table.focus_after_exit(parent_pid);
                        let _ = foreground.compare_exchange(
                            child_table.process_id(),
                            next,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                        result
                    });
                match spawned {
                    Ok(thread) => {
                        self.children.push(ChildJob {
                            table: table.clone(),
                            thread,
                        });
                        match ready_rx.recv_timeout(Duration::from_secs(30)) {
                            Ok(Ok(())) => {
                                self.foreground.store(table.process_id(), Ordering::Release);
                                Ok(request.process_handle)
                            }
                            Ok(Err(error)) => Err(error),
                            Err(_) => {
                                table.terminate_remote_process(table.process_id(), 0xc0000001);
                                table.allow_start();
                                Err(1460)
                            }
                        }
                    }
                    Err(_) => {
                        table.set_exit(None, 0xc0000017);
                        table.mark_inactive();
                        Err(8)
                    }
                }
            } else {
                if let Some(table) = state.object_handles.child(request.process_handle) {
                    table.mark_inactive();
                }
                Err(5)
            };
            state
                .process_launch_results
                .insert(request.call_key, result);
            return;
        }
        let mut exit_code = 0xc0000135;
        if let (Some(context), Some(relative)) = (context, relative) {
            let mut child_game = context.entry.clone();
            child_game.executable = relative;
            state.audio.suspend_output();
            let saved = pocket_core::suspend_host_session();
            let mut child_hook = self.child_hook();
            let child_pid = state
                .object_handles
                .child_id(request.process_handle)
                .unwrap();
            self.foreground.store(child_pid, Ordering::Release);
            log::info!("parent suspended; entering child {}", request.guest_path);
            let (outcome, code) = run_process(
                &context.library_root,
                &child_game,
                &context.card_root,
                Some(&request.guest_path),
                state.memory_division.clone(),
                state.child_handle_context(request.process_handle),
                None,
                &mut child_hook,
            );
            exit_code = code;
            self.foreground.store(self.pid, Ordering::Release);
            self.input_disconnected = child_hook.input_disconnected;

            // run_process has dropped its child Emulator and its media first.
            drop(saved);
            state.audio.resume_output();
            log::info!("child finished with code 0x{exit_code:08x}; resuming preserved parent");
            if exit_code != 0 {
                log::warn!("child outcome: {}", outcome);
            }
            self.last_frame = 0;
            self.last_emit_at = None;
            self.saw_non_black = false;
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
            table.complete_process_exit(exit_code);
            table.set_exit(Some(0), exit_code);
            table.mark_inactive();
        }
        state.sync_transferred_handles();
        if let Some(child) = state.child_processes.get_mut(&request.process_handle) {
            child.exit_code = Some(exit_code);
        }
        for handle in [request.process_handle, request.thread_handle] {
            if let Some(mut event) = state.events.get_mut(&handle) {
                event.signalled = true;
            }
        }
    }

    fn finish_children(&mut self) {
        for job in self.children.drain(..) {
            if !job.table.start_allowed() {
                job.table
                    .terminate_remote_process(job.table.process_id(), 0xc0000001);
                job.table.allow_start();
            }
            if let Err(_) = job.thread.join() {
                self.stop_all.store(true, Ordering::Release);
            }
        }
    }
    fn reap_children(&mut self) {
        let mut index = 0;
        while index < self.children.len() {
            if self.children[index].thread.is_finished() {
                let job = self.children.swap_remove(index);
                if job.thread.join().is_err() {
                    job.table.complete_process_exit(0xc0000005);
                    job.table.set_exit(Some(0), 0xc0000005);
                    job.table.mark_inactive();
                }
            } else {
                index += 1;
            }
        }
    }

    fn child_hook(&self) -> Self {
        let (_, input_rx) = channel();
        let mut hook = Self::new(Arc::clone(&self.state), input_rx);
        hook.input_rx = Arc::clone(&self.input_rx);
        hook.stop_all = Arc::clone(&self.stop_all);
        hook.foreground = Arc::clone(&self.foreground);
        hook
    }
    fn take_input_receiver(&mut self) -> Receiver<InputCommand> {
        let (_, replacement) = channel();
        Arc::try_unwrap(std::mem::replace(
            &mut self.input_rx,
            Arc::new(Mutex::new(replacement)),
        ))
        .ok()
        .expect("managed root owns its input receiver")
        .into_inner()
        .unwrap()
    }

    fn new(state: Arc<SessionState>, input_rx: Receiver<InputCommand>) -> Self {
        Self {
            stop_all: Arc::clone(&state.stop_all),
            state,
            input_rx: Arc::new(Mutex::new(input_rx)),
            foreground: Arc::new(AtomicU32::new(1)),
            pid: 1,
            children: Vec::new(),
            launch_context: None,
            last_frame: 0,
            input_disconnected: false,
            last_emit_at: None,
            scratch: Vec::new(),
            saw_non_black: false,
        }
    }
}

impl FrameHook for SessionHook {
    fn on_frame(&mut self, kernel: &mut KernelState) -> FrameAction {
        // Drain any pending UI input into the kernel's queue.
        self.reap_children();
        let mut stop_requested = self.stop_all.load(Ordering::Acquire);
        if !self.input_disconnected && self.foreground.load(Ordering::Acquire) == self.pid {
            let input = self.input_rx.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                match input.try_recv() {
                    Ok(InputCommand::Input(ev)) => kernel.pending_input.push_back(ev),
                    Ok(InputCommand::Stop) => stop_requested = true,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.input_disconnected = true;
                        break;
                    }
                }
            }
        }

        stop_requested |= self.input_disconnected;
        if stop_requested {
            self.stop_all.store(true, Ordering::Release);
        }
        if !stop_requested {
            self.run_pending_child(kernel);
        }
        stop_requested |= self.stop_all.load(Ordering::Acquire);

        // Stream a fresh framebuffer if the guest produced one.
        if self.foreground.load(Ordering::Acquire) == self.pid {
            if let Ok(mut slot) = self.state.audio.lock() {
                *slot = Some(kernel.audio.tap());
            }
        }
        let counter = kernel.framebuffer.frame_counter;
        if self.foreground.load(Ordering::Acquire) == self.pid && counter != self.last_frame {
            if kernel.framebuffer.is_all_black() && self.saw_non_black {
                return if stop_requested {
                    kernel.should_stop = true;
                    FrameAction::Stop
                } else {
                    FrameAction::Continue
                };
            }
            self.saw_non_black = !kernel.framebuffer.is_all_black();
            let now = Instant::now();
            let due = self
                .last_emit_at
                .map(|t| now.duration_since(t) >= FRAME_PUSH_INTERVAL)
                .unwrap_or(true);
            if due {
                self.last_frame = counter;
                self.last_emit_at = Some(now);
                let frame =
                    FrameSnapshot::from_framebuffer_into(&kernel.framebuffer, &mut self.scratch);
                push_frame(&self.state, frame);
            }
        }

        if stop_requested {
            kernel.should_stop = true;
            FrameAction::Stop
        } else {
            FrameAction::Continue
        }
    }
}

#[cfg(all(test, feature = "unicorn"))]
#[path = "runner_tests.rs"]
mod tests;
