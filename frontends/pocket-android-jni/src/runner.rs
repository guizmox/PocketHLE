//! Session-based emulator runner exposed to the Android JNI layer.
//!
//! A worker owns the emulator, streams snapshots and accepts input. The UI
//! never waits for the complete guest run before receiving its first frame.
//!
//! 1. Kotlin calls [`start_with_presentation`] with the library root and game id. We
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
use pocket_core::winceapi::{self, SystemMediaCommand, SystemMediaInput, SystemMediaStatus};
use pocket_core::Emulator;
use pocket_library::{is_gizmondo_game, CpuBackendPref, GameEntry, Library};

const FRAME_PUSH_INTERVAL: Duration = Duration::from_millis(16);

fn presentation_phase(last: Option<Instant>, now: Instant) -> Option<Instant> {
    let Some(last) = last else {
        return Some(now);
    };
    let elapsed = now.saturating_duration_since(last);
    if elapsed < FRAME_PUSH_INTERVAL {
        return None;
    }
    // Skip missed slots after a pause; never send a burst of catch-up frames.
    let remainder =
        Duration::from_nanos((elapsed.as_nanos() % FRAME_PUSH_INTERVAL.as_nanos()) as u64);
    Some(now - remainder)
}

/// Snapshot of the guest framebuffer plus the dimensions Kotlin
/// needs to paint it onto a `SurfaceView`.
#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    pub presentation_stats: pocket_core::kernel::framebuffer::PresentationStats,
    pub width: u32,
    pub height: u32,
    pub texture_width: u32,
    pub texture_height: u32,
    /// Transport pixels: packed RGB565 when rgb565 is true, RGBA otherwise.
    pub rgba: Vec<u8>,
    pub rgb565: bool,
    pub shared: Option<pocket_core::winceapi::gles::SharedFrame>,
    pub backlight: u32,
}

impl FrameSnapshot {
    pub(crate) fn from_framebuffer(fb: &pocket_core::kernel::Framebuffer) -> Self {
        Self::from_framebuffer_into(fb, &mut Vec::new())
    }

    fn from_framebuffer_into(fb: &pocket_core::kernel::Framebuffer, scratch: &mut Vec<u8>) -> Self {
        let rgb565 = if let Some(pixels) = fb.presentation_rgb565_unadjusted() {
            scratch.clear();
            scratch.extend_from_slice(pixels);
            true
        } else {
            fb.snapshot_presentation_rgba8888_into(scratch);
            false
        };
        Self {
            presentation_stats: fb.presentation_stats.clone(),
            width: fb.width,
            height: fb.height,
            texture_width: fb.presentation_size().0,
            texture_height: fb.presentation_size().1,
            rgba: std::mem::take(scratch),
            rgb565,
            shared: None,
            backlight: fb.backlight_level(),
        }
    }
}

/// Kotlin → emulator command. Mirrors `pocket_desktop::runner::InputCommand`.
#[derive(Debug, Clone, Copy)]
pub enum InputCommand {
    SystemMedia(SystemMediaCommand),
    Input(InputEvent),
    Stop,
}

/// Shared between the worker thread and the UI thread for the
/// lifetime of one game session.
pub(crate) struct SessionState {
    presentation: Option<Arc<crate::graphics::Presentation>>,
    system_media: Mutex<Option<SystemMediaStatus>>,
    stop_all: Arc<AtomicBool>,
    /// Latest framebuffer the guest produced. The polling loop on
    /// the UI thread drains this slot and paints it; a write
    /// overwrites whatever was there because the UI only ever
    /// cares about the newest frame.
    latest_frame: Mutex<Option<FrameSnapshot>>,
    presentation_stats: Mutex<Option<pocket_core::kernel::framebuffer::PresentationStats>>,
    gps_notice: Mutex<Option<(f64, f64)>>,
    dependency_notices: Mutex<std::collections::VecDeque<String>>,
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
            presentation: None,
            stop_all: Arc::new(AtomicBool::new(false)),
            latest_frame: Mutex::new(None),
            system_media: Mutex::new(None),
            presentation_stats: Mutex::new(None),
            gps_notice: Mutex::new(None),
            dependency_notices: Mutex::new(Default::default()),
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
    diagnostic_enabled: bool,
    diagnostic: Mutex<pocket_core::winceapi::gles::graphics_diagnostic::Profiler>,
}

impl Session {
    pub(crate) fn presentation(&self) -> Option<Arc<crate::graphics::Presentation>> {
        self.state.presentation.clone()
    }
    pub fn diagnostic_enabled(&self) -> bool {
        self.diagnostic_enabled
    }
    pub fn record_jni(&self, elapsed: std::time::Duration) {
        if self.diagnostic_enabled {
            if let Ok(mut p) = self.diagnostic.lock() {
                p.record(
                    pocket_core::winceapi::gles::graphics_diagnostic::Stage::Jni,
                    elapsed,
                );
            }
        }
    }
    pub fn system_media(&self) -> Option<SystemMediaStatus> {
        self.state.system_media.lock().ok()?.clone()
    }

    pub fn poll_dependency_notice(&self) -> Option<String> {
        self.state.dependency_notices.lock().ok()?.pop_front()
    }
    pub fn poll_gps_notice(&self) -> Option<(f64, f64)> {
        self.state.gps_notice.lock().ok()?.take()
    }
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

    pub fn published_image_ips(&self) -> Option<f32> {
        self.state.presentation_stats.lock().ok()?.as_ref()?.ips()
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

/// Spawn the worker thread that drives one game and returns its session handle.
pub(crate) fn start_with_presentation(
    library_root: PathBuf,
    game_id: String,
    presentation: Option<Arc<crate::graphics::Presentation>>,
) -> anyhow::Result<Session> {
    let lib = Library::open(&library_root).context("Library::open")?;
    let entry = lib
        .get(&game_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown game id {game_id}"))?;

    let diagnostic_enabled = lib.config().android_graphics_diagnostics;
    let mut state = SessionState::new();
    state.presentation = presentation;
    let state = Arc::new(state);
    let (input_tx, input_rx) = channel::<InputCommand>();

    let state_for_worker = Arc::clone(&state);
    let worker = std::thread::Builder::new()
        .name(format!("pockethle-emu-{game_id}"))
        .spawn(move || {
            let summary = pocket_core::winceapi::gles::graphics_diagnostic::with_enabled(
                diagnostic_enabled,
                || run_game_to_completion(&library_root, &entry, &state_for_worker, input_rx),
            );
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
        diagnostic_enabled,
        diagnostic: Mutex::new(
            pocket_core::winceapi::gles::graphics_diagnostic::Profiler::new(
                diagnostic_enabled,
                "jni",
            ),
        ),
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
    let card_root = entry.gizmondo_storage_card_root(library_root);
    if is_gizmondo_game(entry, library_root)
        && entry.settings.storage_card_dir.is_some()
        && !card_root.is_dir()
    {
        let reason = format!("SD Card folder does not exist: {}", card_root.display());
        let diagnostic = pocket_library::session_diagnostic::startup_failure(reason.clone(), Vec::new());
        return serde_json::json!({"summary": reason, "diagnostic": diagnostic}).to_string();
    }
    let mut hook = SessionHook::new(Arc::clone(state), input_rx);
    let ((summary, code), log) = pocket_library::session_diagnostic::capture(|| {
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
    });
    let diagnostic = pocket_library::session_diagnostic::classify(
        false,
        hook.started,
        code,
        hook.runtime_error.as_deref(),
        &summary,
        log,
    );
    serde_json::json!({"summary": summary, "diagnostic": diagnostic}).to_string()
}

#[derive(Clone)]
struct LaunchContext {
    library_root: PathBuf,
    entry: GameEntry,
    card_root: PathBuf,
}

impl LaunchContext {
    fn child_executable_reference(
        &self,
        request: &pocket_core::kernel::ProcessLaunch,
    ) -> Result<PathBuf, String> {
        let child = request
            .executable
            .canonicalize()
            .map_err(|error| format!("cannot resolve executable: {error}"))?;
        if !request.concurrent {
            let root = self
                .card_root
                .canonicalize()
                .map_err(|error| format!("cannot resolve SD Card root: {error}"))?;
            if !child.starts_with(&root) {
                return Err("executable is outside the mounted SD Card".into());
            }
        }
        // GTShell lives in Flash Disk2 while its SD Card can be a separate
        // restored folder. Preserve internal child install/save namespaces.
        Ok(self
            .entry
            .child_executable_reference(&self.library_root, &child))
    }
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
    startup: Option<ProcessStartup>,
    hook: &mut SessionHook,
) -> (String, u32) {
    let bridge = hook.state.presentation.clone();
    let source = bridge.as_ref().and_then(|p| p.source());
    let enabled = bridge
        .as_ref()
        .map(|p| p.enabled.clone())
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    pocket_core::winceapi::gles::with_shared_context_source(source, || {
        pocket_core::winceapi::gles::with_shared_frame_export(enabled, || {
            run_process_inner(
                library_root,
                entry,
                card_root,
                guest_path,
                inherited_ram,
                handle_context,
                startup,
                hook,
            )
        })
    })
}
fn run_process_inner(
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
            let screen = entry.settings.screen.resolve_image(Some(image));
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
    let launcher_config = pocket_library::Library::open(library_root)
        .map(|l| l.config().clone())
        .unwrap_or_default();
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
    emu.set_frame_rate_limit(entry.settings.effective_frame_rate_limit(&launcher_config));
    emu.max_slices = entry.settings.max_slices;
    emu.instruction_budget_per_slice = entry.settings.instructions_per_slice;

    let is_gizmondo = is_gizmondo_game(entry, library_root);
    if let Err(e) = emu.load_pe_with_executable_heap(
        &exe,
        is_gizmondo || entry.settings.effective_executable_heap(&launcher_config),
    ) {
        summary_lines.push(format!("load_pe failed: {e:#}"));
        return (summary_lines.join("\n"), 0xc0000135);
    }

    emu.set_pocket_pc_input(!is_gizmondo);
    if is_gizmondo {
        emu.set_gizmondo_platform_identity();
    }
    emu.set_gizmondo_audio_compatibility(is_gizmondo);
    if let Some(process) = emu.process_mut() {
        if !is_gizmondo {
            use pocket_core::kernel::accelerometer::TiltAxes;
            use pocket_library::TiltInput;
            process.state.virtual_tilt_axes =
                match entry.settings.effective_tilt_input(&launcher_config) {
                    TiltInput::Accelerometer => None,
                    TiltInput::LeftRight => Some(TiltAxes::Horizontal),
                    TiltInput::FourDirections => Some(TiltAxes::Both),
                };
        }
        process.state.touch_steering.enabled = !is_gizmondo
            && (entry.settings.controls_mode() == pocket_library::stylus_grid::ControlsMode::Zones);
    }
    if !is_gizmondo {
        let state = Arc::clone(&hook.state);
        emu.configure_pocketpc_dependencies(library_root, move |message| {
            if let Ok(mut notices) = state.dependency_notices.lock() {
                notices.push_back(message);
            }
        });
    }
    if is_gizmondo {
        if let Ok(image) = &image {
            crate::colors_trace::install(&mut emu, image);
        }
    }
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
    if !is_gizmondo {
        if let Err(error) = emu.configure_pocketpc_storage(library_root) {
            if let Some(startup) = startup.as_mut() {
                startup.error = 29;
            }
            return (format!("Cannot initialize PocketPC storage: {error:#}"), 29);
        }
    }
    let registry_path = library_root.join(if is_gizmondo {
        "registry-gizmondo.json"
    } else {
        "registry-pocketpc.json"
    });

    if !is_gizmondo {
        if let Err(error) = emu.set_pocketpc_processor(&launcher_config.pocketpc_processor) {
            log::warn!("{error}; using default PXA270 identity");
            let _ = emu.set_pocketpc_processor("pxa270");
        }
    }
    let profile = entry.settings.gpu_compatibility_profile;
    let programmable = !is_gizmondo
        && (profile.programmable_gles()
            || image
                .as_ref()
                .is_ok_and(pocket_library::uses_programmable_gles));
    emu.set_gpu_renderer(launcher_config.gpu_renderer);
    emu.set_gizmondo_goforce_driver(is_gizmondo);
    emu.set_programmable_gles(programmable);
    emu.set_goforce_5500_profile(!is_gizmondo && profile.nvidia_combiners());
    emu.set_nv_combiner_allowed(is_gizmondo || profile.nvidia_combiners());
    emu.set_intel2700g_depth_bias(!is_gizmondo && profile.intel_depth_bias());
    emu.set_gles_internal_resolution(launcher_config.gles_internal_resolution);
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
    if pocket_core::winceapi::gles::graphics_diagnostic::enabled() {
        log::info!(
            "GPU_DIAG session_start game={} requested_gpu={} internal_policy={} filter={} measure_ips={}",
            entry.id,
            programmable || launcher_config.gpu_renderer,
            launcher_config.gles_internal_resolution,
            launcher_config.upscale_filter,
            launcher_config.show_ips
        );
    }
    let hardware = (
        launcher_config.bluetooth_enabled,
        launcher_config.camera_enabled,
        launcher_config.gps_enabled,
    );
    if let Some(process) = emu.process_mut() {
        if pocket_library::is_colors_game(entry, library_root) {
            process
                .state
                .vfs
                .set_colors_language(pocket_library::colors_launch_language(
                    entry,
                    library_root,
                    &launcher_config.colors_language,
                ));
        }
        hook.colors_random_gps = if pocket_library::is_colors_game(entry, library_root)
            && launcher_config.gps_fixed_enabled
            && launcher_config.gps_fixed_random
        {
            Some(launcher_config.clone())
        } else {
            None
        };
        process.state.vfs.bluetooth.service.set_allowed(hardware.0);
        process.state.vfs.camera_service().set_allowed(hardware.1);
        process.state.vfs.gps_service().set_allowed(hardware.2);
        if pocket_library::is_colors_game(entry, library_root) && launcher_config.gps_fixed_enabled
        {
            let gps = process.state.vfs.gps_service();
            if let Err(e) = gps.set_fixed_position(launcher_config.simulated_gps_position()) {
                return (format!("Invalid fixed GPS coordinates: {e}"), 0xc000000d);
            }
            gps.set_allowed(true);
            log::info!("GPS1 fixed simulation activated on Android");
        }
        let registry_result = (|| {
            process
                .state
                .registry
                .configure_persistence(&registry_path)?;
            if is_gizmondo && guest_path.is_none() {
                process.state.registry.reset_gizmondo_backlight()?;
                process.state.sync_backlight();
            }
            Ok::<(), std::io::Error>(())
        })();
        if let Err(e) = registry_result {
            summary_lines.push(format!("Cannot initialize device registry: {e}"));
            return (summary_lines.join("\n"), 0xc0000135);
        }
        let (preset, platform) = launcher_config.device_identity_hex(is_gizmondo);
        if let Err(e) = pocket_core::kernel::device_identity::DeviceIdentity::configure(
            &mut process.state.registry,
            preset,
            platform,
        ) {
            summary_lines.push(format!("Invalid device identity: {e}"));
            return (summary_lines.join("\n"), 0xc000000d);
        }
        if is_gizmondo {
            process
                .state
                .internet
                .set_gprs_enabled(launcher_config.gprs_enabled);
            if let Err(e) = pocket_core::kernel::colors::configure(
                &mut process.state,
                if pocket_library::is_colors_game(entry, library_root) {
                    &launcher_config.colors_server_url
                } else {
                    ""
                },
                &launcher_config.colors_terminal_id,
            ) {
                summary_lines.push(e);
                return (summary_lines.join("\n"), 0xc0000135);
            }
        }
    }
    if !is_gizmondo {
        if let Err(error) = emu
            .set_pocketpc_identity(
                &entry.settings.pocketpc_owner_name,
                &entry.settings.pocketpc_device_name,
                &entry.settings.pocketpc_device_id,
            )
            .and_then(|_| emu.set_pocketpc_preset_hex(&entry.settings.pocketpc_preset_id))
            .and_then(|_| emu.set_pocketpc_random_seed(&entry.settings.pocketpc_random_seed))
            .and_then(|_| {
                emu.set_pocketpc_oem_identity(
                    &entry.settings.pocketpc_oem_info,
                    &entry.settings.pocketpc_platform_type,
                )
            })
        {
            summary_lines.push(format!("Invalid PocketPC identity: {error}"));
            return (summary_lines.join("\n"), 87);
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
        entry.settings.screen.gizmondo_size()
    } else {
        entry.settings.screen.resolve_image(image.as_ref().ok())
    };
    emu.set_screen_size(screen_width, screen_height);
    emu.set_egl_rgba8888_only(!is_gizmondo && entry.settings.pocketpc_egl_rgba8888_only);
    emu.set_pocketpc_gapi_columns(if is_gizmondo {
        (screen_width, screen_height) == (320, 240)
            && image
                .as_ref()
                .ok()
                .is_some_and(pocket_library::analysis::uses_rotated_gizmondo_lcd)
    } else {
        entry.settings.pocketpc_gapi_columns
    });
    if launcher_config.show_ips {
        if let Some(process) = emu.process_mut() {
            process.state.framebuffer.enable_ips();
        }
    }
    summary_lines.push(format!("Screen: {screen_width}x{screen_height}"));
    let extracted = entry.extracted_dir(library_root);

    emu.mount_read_only_dir("\\Application\\", &extracted);
    emu.mount_read_only_dir("\\Program Files\\", &extracted);
    emu.mount_read_only_dir("\\Program Files\\Game\\", &extracted);
    if is_gizmondo {
        let flash_disk = entry.gizmondo_flash_dir(library_root);
        emu.mount_save_dir("\\Flash Disk\\", &flash_disk);
        summary_lines.push(format!(
            "Mounted Gizmondo Flash Disk at {}",
            flash_disk.display()
        ));
        emu.mount_read_only_dir("\\SD Card\\", card_root);
        emu.mount_read_only_dir("\\Storage Card\\", card_root);
        if entry.settings.storage_card_dir.is_none() {
            emu.mount_read_only_dir("\\SD Card\\Game\\", &extracted);
            emu.mount_read_only_dir("\\Storage Card\\Game\\", &extracted);
        }
        log::info!("Gizmondo SD Card directory: {}", card_root.display());
        summary_lines.push(format!(
            "Mounted Gizmondo SD-card layout at {}",
            card_root.display()
        ));
    }
    for (prefix, directory) in entry.install_mounts(library_root) {
        emu.mount_read_only_dir(&prefix, directory);
    }
    if let Some(paths) = entry.guest_launch_paths(library_root) {
        emu.mount_read_only_dir(
            &paths.install_prefix,
            entry.install_mount_dir(library_root, &paths.install_prefix),
        );
        emu.set_module_path(&paths.module_path);
        emu.set_default_dir(&paths.working_dir);
        let save_dir = entry.save_dir(library_root);
        emu.mount_save_dir(&paths.save_prefix, &save_dir);
        if let Some(process) = emu.process_mut() {
            process.state.cedb.path = Some(save_dir.join("cedb-contacts.json"));
        }
        if !is_gizmondo
            && !paths
                .save_prefix
                .trim_end_matches('\\')
                .eq_ignore_ascii_case(r"\My Documents")
        {
            emu.mount_isolated_save_dir(r"\My Documents\", save_dir.join("my-documents"));
        }
        summary_lines.push(format!(
            "Save data: {} -> {:?}",
            save_dir.display(),
            paths.save_prefix
        ));
    }
    if !is_gizmondo && entry.source_cab.to_ascii_lowercase().ends_with(".cab") {
        if let Some(install_dir) = entry.install_dir.as_deref() {
            match emu.complete_cab_registry_setup(&extracted, install_dir) {
                Ok(count) if count != 0 => {
                    summary_lines.push(format!("CAB registry setup completed: {count}"))
                }
                Ok(_) => {}
                Err(error) => {
                    log::warn!("CAB registry setup failed: {error:#}");
                    summary_lines.push(format!("CAB registry setup failed: {error:#}"));
                }
            }
        }
    }
    // Keep the executable's directory inside the card, as on desktop. Native
    // DLLs use GetModuleFileName to locate their configuration and assets.
    if is_gizmondo {
        if let Some(guest_exe) = entry.gizmondo_guest_exe_path(library_root) {
            emu.set_module_path(&guest_exe);
            if let Some((directory, _)) = guest_exe.rsplit_once('\\') {
                emu.set_default_dir(&format!("{directory}\\"));
            }
            log::info!("Gizmondo module path: {guest_exe}");
            summary_lines.push(format!("Module path: {guest_exe}"));
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
    hook.started = true;
    let run_result = emu.run_with_hook(hook);
    if let Err(error) = &run_result {
        hook.runtime_error = Some(format!("{error:#}"));
    }
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
    if let Some(p) = emu.process_mut() {
        match p.state.presentation_framebuffer() {
            Ok(frame)
                if !frame.is_all_black() && hook.foreground.load(Ordering::Acquire) == hook.pid =>
            {
                push_frame(&state, FrameSnapshot::from_framebuffer(&frame));
            }
            Err(error) => log::error!("final presentation failed: {error}"),
            _ => {}
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
    pocket_core::winceapi::gles::deferred_frame::release_reader();
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
        Emulator::with_unicorn_cpu_for_arch(pocket_core::cpu::Arch::Arm)
    }
}

#[cfg(not(feature = "unicorn"))]
fn build_unicorn_for_machine(_machine: u16) -> anyhow::Result<Emulator> {
    Err(anyhow::anyhow!(
        "binary was not compiled with the `unicorn` feature"
    ))
}

pub(crate) fn push_frame(state: &Arc<SessionState>, frame: FrameSnapshot) {
    if let Ok(mut stats) = state.presentation_stats.lock() {
        *stats = Some(frame.presentation_stats.clone());
    }
    let old = state
        .latest_frame
        .lock()
        .ok()
        .and_then(|mut slot| slot.replace(frame));
    drop(old);
}

/// Bridges the UI thread (Kotlin) and the running emulator on the
/// worker thread.
struct SessionHook {
    media_input: SystemMediaInput,
    media_commands: Vec<SystemMediaCommand>,
    colors_random_gps: Option<pocket_library::LauncherConfig>,
    geofence_down: bool,
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
    last_shared: Option<bool>,
    scratch: Vec<u8>,
    saw_non_black: bool,
    started: bool,
    runtime_error: Option<String>,
}

impl SessionHook {
    fn queue_input(
        &mut self,
        ev: InputEvent,
        gps: &pocket_core::kernel::gps::Service,
        pending: &mut std::collections::VecDeque<InputEvent>,
    ) {
        match ev {
            InputEvent::KeyDown { vk: 0x73 } => {
                if !self.geofence_down {
                    if let Some(config) = self.colors_random_gps.as_ref() {
                        if let Some(position) = config.simulated_gps_position() {
                            if gps.set_fixed_position(Some(position)).is_ok() {
                                if let Ok(mut slot) = self.state.gps_notice.lock() {
                                    *slot = Some(position);
                                }
                            }
                        }
                    }
                }
                self.geofence_down = true;
            }
            InputEvent::KeyUp { vk: 0x73 } => self.geofence_down = false,
            _ => {}
        }
        // The simulator action supplements, rather than consumes, the hardware key.
        pending.push_back(ev);
    }

    fn run_pending_child(&mut self, state: &mut KernelState) {
        let Some(request) = state.take_process_launch() else {
            return;
        };
        let context = self.launch_context.clone();
        let executable_reference = context
            .as_ref()
            .ok_or_else(|| "missing runner context".to_string())
            .and_then(|context| context.child_executable_reference(&request));
        if let Err(reason) = &executable_reference {
            log::warn!(
                "child launch refused: guest={:?}, executable={}, card_root={:?}: {reason}",
                request.guest_path,
                request.executable.display(),
                context.as_ref().map(|context| &context.card_root),
            );
        }
        if request.concurrent {
            let result = if let (Some(context), Ok(executable_reference)) =
                (context, executable_reference)
            {
                let mut child_game = context.entry.clone();
                child_game.executable = executable_reference;
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
                                state.reset_foreground_input();
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
            if let Some(key) = request.call_key {
                state.process_launch_results.insert(key, result);
            } else if let Ok(handle) = result {
                if let Some(table) = state.object_handles.child(handle) {
                    table.allow_start();
                }
            } else if let Err(error) = result {
                log::warn!(
                    "CeRunAppAtTime: guest launch failed for {:?}: {error}",
                    request.guest_path
                );
            }
            return;
        }
        let mut exit_code = 0xc0000135;
        if let (Some(context), Ok(executable_reference)) = (context, executable_reference) {
            let mut child_game = context.entry.clone();
            child_game.executable = executable_reference;
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
            state.reset_foreground_input();
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
            colors_random_gps: None,
            geofence_down: false,
            media_input: SystemMediaInput::default(),
            media_commands: Vec::new(),
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
            last_shared: None,
            scratch: Vec::new(),
            saw_non_black: false,
            started: false,
            runtime_error: None,
        }
    }
}

impl FrameHook for SessionHook {
    fn on_frame(&mut self, kernel: &mut KernelState) -> FrameAction {
        // Drain any pending UI input into the kernel's queue.
        self.reap_children();
        let mut stop_requested = self.stop_all.load(Ordering::Acquire);
        if !self.input_disconnected && self.foreground.load(Ordering::Acquire) == self.pid {
            let status = winceapi::system_media_status();
            let media_active = status.is_some();
            let input_rx = Arc::clone(&self.input_rx);
            let input = input_rx.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                match input.try_recv() {
                    Ok(InputCommand::Input(ev)) => {
                        if !self
                            .media_input
                            .capture(ev, media_active, &mut self.media_commands)
                        {
                            self.queue_input(
                                ev,
                                &kernel.vfs.gps_service(),
                                &mut kernel.pending_input,
                            );
                        }
                    }
                    Ok(InputCommand::SystemMedia(command)) => self.media_commands.push(command),
                    Ok(InputCommand::Stop) => stop_requested = true,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.input_disconnected = true;
                        break;
                    }
                }
            }
            drop(input);
            let status = if self.media_commands.is_empty() {
                status
            } else {
                for command in self.media_commands.drain(..) {
                    winceapi::system_media_command(kernel, command);
                }
                winceapi::system_media_status()
            };
            if let Ok(mut slot) = self.state.system_media.lock() {
                *slot = status;
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
        let cpu_refresh = self
            .state
            .presentation
            .as_ref()
            .is_some_and(|p| p.cpu_refresh.load(Ordering::Acquire));
        if self.foreground.load(Ordering::Acquire) == self.pid
            && (counter != self.last_frame || cpu_refresh)
        {
            let now = Instant::now();
            if let Some(phase) = presentation_phase(self.last_emit_at, now) {
                let forced_cpu_refresh = self
                    .state
                    .presentation
                    .as_ref()
                    .is_some_and(|p| p.cpu_refresh.swap(false, Ordering::AcqRel));
                let export = self
                    .state
                    .presentation
                    .as_ref()
                    .is_some_and(|p| p.enabled.load(Ordering::Acquire));
                let shared = if forced_cpu_refresh
                    || !export
                    || kernel.framebuffer.has_system_video()
                    || kernel.shell_keyboard.visible
                    || kernel.gdi.shell_menus.tracking.is_some()
                {
                    None
                } else {
                    pocket_core::winceapi::gles::take_shared_window_frame(&kernel.framebuffer)
                };
                let is_shared = shared.is_some();
                if self.last_shared != Some(is_shared) {
                    log::info!(
                        "PRESENTATION={} (Android)",
                        if is_shared {
                            "GPU_SHARED"
                        } else {
                            "CPU_UPLOAD"
                        }
                    );
                    self.last_shared = Some(is_shared);
                }
                if let Some(shared) = shared {
                    let size = shared.size();
                    self.saw_non_black = true;
                    self.last_frame = counter;
                    self.last_emit_at = Some(phase);
                    push_frame(
                        &self.state,
                        FrameSnapshot {
                            presentation_stats: kernel.framebuffer.presentation_stats.clone(),
                            width: kernel.framebuffer.width,
                            height: kernel.framebuffer.height,
                            texture_width: size.0,
                            texture_height: size.1,
                            rgba: Vec::new(),
                            rgb565: false,
                            backlight: kernel.framebuffer.backlight_level(),
                            shared: Some(shared),
                        },
                    );
                } else {
                    let materialize = pocket_core::winceapi::gles::graphics_diagnostic::Span::new(
                        pocket_core::winceapi::gles::graphics_diagnostic::Stage::Materialize,
                    );
                    let black = match kernel.framebuffer.cpu() {
                        Ok(frame) => frame.is_all_black(),
                        Err(error) => {
                            log::error!("framebuffer read failed: {error}");
                            self.runtime_error = Some(format!("framebuffer read failed: {error}"));
                            kernel.should_stop = true;
                            return FrameAction::Stop;
                        }
                    };
                    if black
                        && self.saw_non_black
                        && !kernel.framebuffer.has_system_video()
                        && !forced_cpu_refresh
                    {
                        return if stop_requested {
                            kernel.should_stop = true;
                            FrameAction::Stop
                        } else {
                            FrameAction::Continue
                        };
                    }

                    self.saw_non_black = !black;

                    self.last_frame = counter;
                    self.last_emit_at = Some(phase);
                    if !kernel.gx_explicit_present {
                        kernel.framebuffer.observe_ips();
                    }
                    let presentation = match kernel.presentation_framebuffer() {
                        Ok(frame) => frame,
                        Err(error) => {
                            log::error!("presentation read failed: {error}");
                            self.runtime_error = Some(format!("presentation read failed: {error}"));
                            kernel.should_stop = true;
                            return FrameAction::Stop;
                        }
                    };
                    drop(materialize);
                    let pack = pocket_core::winceapi::gles::graphics_diagnostic::Span::new(
                        pocket_core::winceapi::gles::graphics_diagnostic::Stage::Pack,
                    );
                    let frame =
                        FrameSnapshot::from_framebuffer_into(&presentation, &mut self.scratch);
                    drop(pack);
                    push_frame(&self.state, frame);
                }
            }
        }

        if stop_requested {
            kernel.should_stop = true;
            FrameAction::Stop
        } else if self.foreground.load(Ordering::Acquire) != self.pid {
            FrameAction::ContinueInBackground
        } else {
            FrameAction::Continue
        }
    }
}

#[cfg(all(test, feature = "arm-diagnostics"))]
#[path = "runner_tests.rs"]
mod tests;

#[cfg(test)]
mod child_card_tests {
    use super::*;

    fn with_shell_and_card(test: impl FnOnce(LaunchContext)) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pockethle-android-card-{}-{nonce}",
            std::process::id()
        ));
        let mut entry: GameEntry = serde_json::from_value(serde_json::json!({
            "platform": "gizmondo", "id": "shell", "display_name": "Shell",
            "executable": "extracted/GTShell.exe", "source_cab": "manual:GTShell.exe",
            "install_dir": "\\Flash Disk2\\", "save_prefix": "\\Flash Disk2\\"
        }))
        .unwrap();
        let card = root.join("cards/sd-card");
        std::fs::create_dir_all(entry.extracted_dir(&root)).unwrap();
        std::fs::create_dir_all(&card).unwrap();
        std::fs::write(card.join("autorun.exe"), b"card game").unwrap();
        std::fs::write(entry.extracted_dir(&root).join("worker.exe"), b"worker").unwrap();
        entry.settings.storage_card_dir = Some("cards/sd-card".into());
        assert_eq!(entry.gizmondo_storage_card_root(&root), card);
        test(LaunchContext {
            library_root: root.clone(),
            entry,
            card_root: card,
        });
        std::fs::remove_dir_all(root).unwrap();
    }

    fn request(executable: PathBuf, concurrent: bool) -> pocket_core::kernel::ProcessLaunch {
        pocket_core::kernel::ProcessLaunch {
            executable,
            guest_path: "\\SD Card\\autorun.exe".into(),
            process_handle: 1,
            thread_handle: 2,
            concurrent,
            command_line: String::new(),
            creation_flags: 0,
            call_key: Some((0, 0, 0)),
        }
    }

    #[test]
    fn shell_launches_separate_restored_card() {
        with_shell_and_card(|context| {
            for concurrent in [false, true] {
                let mut child = context.entry.clone();
                child.executable = context
                    .child_executable_reference(&request(
                        context.card_root.join("autorun.exe"),
                        concurrent,
                    ))
                    .unwrap();
                assert!(child.executable.is_absolute());
                assert_eq!(
                    std::fs::read(child.executable_path(&context.library_root)).unwrap(),
                    b"card game"
                );
            }
            assert_eq!(
                context.entry.guest_exe_path().as_deref(),
                Some("\\Flash Disk2\\GTShell.exe")
            );
        });
    }

    #[test]
    fn foreground_launch_rejects_escape_from_restored_card() {
        with_shell_and_card(|context| {
            let outside = context.library_root.join("cards/sd-card-copy/outside.exe");
            std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
            std::fs::write(&outside, b"outside").unwrap();
            for path in [
                outside,
                context.card_root.join("../sd-card-copy/outside.exe"),
            ] {
                assert!(context
                    .child_executable_reference(&request(path, false))
                    .is_err());
            }
        });
    }

    #[test]
    fn internal_child_preserves_installation_and_saves() {
        with_shell_and_card(|context| {
            let mut child = context.entry.clone();
            child.executable = context
                .child_executable_reference(&request(
                    context
                        .entry
                        .extracted_dir(&context.library_root)
                        .join("worker.exe"),
                    true,
                ))
                .unwrap();
            assert_eq!(child.executable, PathBuf::from("extracted/worker.exe"));
            let paths = child.guest_launch_paths(&context.library_root).unwrap();
            assert_eq!(paths.module_path, "\\Flash Disk2\\worker.exe");
            assert_eq!(paths.save_prefix, "\\Flash Disk2\\");
        });
    }

    #[test]
    fn missing_explicit_card_reports_error_before_guest_execution() {
        with_shell_and_card(|mut context| {
            context.entry.settings.storage_card_dir = Some("cards/missing".into());
            let (_, rx) = channel();
            let summary = run_game_to_completion(
                &context.library_root,
                &context.entry,
                &Arc::new(SessionState::new()),
                rx,
            );
            let body: serde_json::Value = serde_json::from_str(&summary).unwrap();
            assert!(body["summary"].as_str().unwrap().starts_with("SD Card folder does not exist:"));
            assert_eq!(body["diagnostic"]["title"], "Could not start the game");
        });
    }
}

#[cfg(test)]
mod presentation_clock_tests {
    use super::*;

    #[test]
    fn jitter_keeps_phase_and_pause_does_not_create_a_catchup_burst() {
        let base = Instant::now();
        let mut phase = None;
        for ms in [0, 17, 33, 49, 66, 83, 99] {
            phase = Some(presentation_phase(phase, base + Duration::from_millis(ms)).unwrap());
        }
        assert_eq!(phase.unwrap(), base + Duration::from_millis(96));
        assert!(presentation_phase(phase, base + Duration::from_millis(100)).is_none());
        let after_pause = base + Duration::from_millis(1001);
        phase = presentation_phase(phase, after_pause);
        assert!(phase.is_some());
        assert!(presentation_phase(phase, after_pause).is_none());
    }
}

#[cfg(test)]
mod geofence_refresh_tests {
    use super::*;
    use pocket_core::kernel::gps::Service;
    fn coordinates(gps: &Service) -> [u8; 8] {
        gps.open().unwrap().lock().unwrap().packet().unwrap()[21..29]
            .try_into()
            .unwrap()
    }
    #[test]
    fn geofence_adds_one_refresh_per_press_and_preserves_guest_events() {
        let (_, rx) = channel();
        let mut hook = SessionHook::new(Arc::new(SessionState::new()), rx);
        let gps = Service::default();
        gps.set_allowed(true);
        gps.set_fixed_position(Some((12., 34.))).unwrap();
        let original = coordinates(&gps);
        let down = InputEvent::KeyDown { vk: 0x73 };
        let up = InputEvent::KeyUp { vk: 0x73 };
        let mut pending = std::collections::VecDeque::new();
        // Other games and host/manual modes have no random Colors config.
        hook.queue_input(down, &gps, &mut pending);
        hook.queue_input(up, &gps, &mut pending);
        assert_eq!(coordinates(&gps), original);
        let mut config = pocket_library::LauncherConfig::default();
        config.gps_fixed_enabled = true;
        config.gps_fixed_random = true;
        hook.colors_random_gps = Some(config);
        hook.queue_input(down, &gps, &mut pending);
        let refreshed = coordinates(&gps);
        assert_ne!(refreshed, original);
        hook.queue_input(down, &gps, &mut pending);
        assert_eq!(coordinates(&gps), refreshed);
        hook.queue_input(up, &gps, &mut pending);
        assert_eq!(
            pending.into_iter().collect::<Vec<_>>(),
            vec![down, up, down, down, up]
        );
    }
}

#[cfg(test)]
mod internal_resolution_tests {
    use super::FrameSnapshot;
    #[test]
    fn packed_snapshot_freezes_bytes_and_brightness_without_expansion() {
        let mut fb = pocket_core::kernel::Framebuffer::new(3, 1);
        fb.pixels.copy_from_slice(&[0, 248, 224, 7, 31, 0]);
        fb.set_backlight_level(21);
        let frame = FrameSnapshot::from_framebuffer(&fb);
        assert!(frame.rgb565);
        assert_eq!(frame.rgba, [0, 248, 224, 7, 31, 0]);
        assert_eq!(frame.backlight, 21);
        fb.pixels.fill(0);
        fb.set_backlight_level(1);
        assert_eq!(frame.rgba, [0, 248, 224, 7, 31, 0]);
        assert_eq!(frame.backlight, 21);
    }
    #[test]
    fn physical_texture_does_not_change_logical_input_or_zoom_dimensions() {
        let mut fb = pocket_core::kernel::Framebuffer::new(2, 1);
        fb.set_render_presentation(4, 2, vec![255; 4 * 2 * 2]);
        let frame = FrameSnapshot::from_framebuffer(&fb);
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!((frame.texture_width, frame.texture_height), (4, 2));
        assert_eq!(frame.rgba.len(), 4 * 2 * 4);
        assert!(!frame.rgb565);
        fb.mark_dirty();
        let frame = FrameSnapshot::from_framebuffer(&fb);
        assert_eq!((frame.texture_width, frame.texture_height), (2, 1));
        assert_eq!(frame.rgba.len(), 2 * 1 * 2);
        assert!(frame.rgb565);
    }
}

#[cfg(test)]
mod deferred_read_tests {
    use super::*;
    use pocket_core::kernel::framebuffer::deferred::{CpuPixelSource, MaterializedPixels};
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct Image {
        reads: AtomicUsize,
        fail: AtomicBool,
    }
    impl CpuPixelSource for Image {
        fn logical_size(&self) -> (u32, u32) {
            (3, 2)
        }
        fn native_size(&self) -> (u32, u32) {
            (3, 2)
        }
        fn materialize(&self) -> Result<MaterializedPixels, String> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            if self.fail.load(Ordering::Relaxed) {
                return Err("test read failure".into());
            }
            Ok(MaterializedPixels {
                logical_size: (3, 2),
                native_size: (3, 2),
                rgb565: vec![0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8],
                native_rgba: None,
            })
        }
    }
    fn process() -> pocket_core::kernel::Process {
        use pocket_core::{
            cpu::stub::StubCpu,
            pe::{LoadedImage, LoadedSection},
        };
        let mut cpu = StubCpu::new();
        let image = LoadedImage {
            source_path: "hook-test".into(),
            machine: pocket_core::pe::machine::ARM,
            subsystem: pocket_core::pe::subsystem::WINDOWS_CE_GUI,
            image_base: 0x10000,
            stack_reserve: 0x10000,
            size_of_image: 0x2000,
            entry_point: 0x1000,
            sections: vec![LoadedSection {
                name: ".text".into(),
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                characteristics: 0x60000020,
                data: vec![0; 0x1000],
            }],
            imports: vec![],
            exports: Default::default(),
            resources: vec![],
            managed_runtime: None,
        };
        pocket_core::kernel::Process::map_into(
            image,
            &mut cpu,
            &|_, _| None,
            &pocket_core::kernel::NullDispatcher,
        )
        .unwrap()
    }
    #[test]
    fn cpu_fallback_refreshes_a_static_image_without_new_guest_publication() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let bridge = Arc::new(crate::graphics::Presentation::new());
        let mut state = SessionState::new();
        state.presentation = Some(bridge.clone());
        let state = Arc::new(state);
        let mut hook = SessionHook::new(state.clone(), rx);
        let mut process = process();
        let kernel = &mut process.state;
        kernel.framebuffer.resize(3, 2);
        let source = Arc::new(Image {
            reads: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
        });
        kernel.framebuffer.publish_gpu(source.clone()).unwrap();
        hook.last_frame = kernel.framebuffer.frame_counter;
        let revision = kernel.framebuffer.frame_counter;
        bridge.cpu_refresh.store(true, Ordering::Release);
        assert!(matches!(hook.on_frame(kernel), FrameAction::Continue));
        assert_eq!(source.reads.load(Ordering::Relaxed), 1);
        assert_eq!(kernel.framebuffer.frame_counter, revision);
        assert!(!bridge.cpu_refresh.load(Ordering::Acquire));
        assert!(state
            .latest_frame
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .shared
            .is_none());
    }
    #[test]
    fn throttled_frames_never_read_gpu_and_due_read_failures_stop_without_stale_pixels() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let state = Arc::new(SessionState::new());
        let mut hook = SessionHook::new(state.clone(), rx);
        let mut process = process();
        let kernel = &mut process.state;
        kernel.framebuffer.resize(3, 2);
        let source = Arc::new(Image {
            reads: AtomicUsize::new(0),
            fail: AtomicBool::new(true),
        });
        kernel.framebuffer.publish_gpu(source.clone()).unwrap();
        hook.last_emit_at = Some(Instant::now() + Duration::from_secs(60));
        assert!(matches!(hook.on_frame(kernel), FrameAction::Continue));
        assert_eq!(source.reads.load(Ordering::Relaxed), 0);
        assert!(!kernel.framebuffer.cpu_resident());
        hook.last_emit_at = None;
        assert!(matches!(hook.on_frame(kernel), FrameAction::Stop));
        assert!(kernel.should_stop);
        assert_eq!(source.reads.load(Ordering::Relaxed), 1);
        assert!(!kernel.framebuffer.cpu_resident());
        kernel.should_stop = false;
        source.fail.store(false, Ordering::Relaxed);
        assert!(matches!(hook.on_frame(kernel), FrameAction::Continue));
        assert_eq!(source.reads.load(Ordering::Relaxed), 2);
        assert!(kernel.framebuffer.cpu_resident());
        assert_eq!(
            &state.latest_frame.lock().unwrap().as_ref().unwrap().rgba[..4],
            &[0, 248, 0, 248]
        );
        assert!(state.latest_frame.lock().unwrap().as_ref().unwrap().rgb565);
    }
}
