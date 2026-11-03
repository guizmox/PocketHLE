//! Background runner that drives [`pocket_core::Emulator`] for the
//! desktop GUI.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::sync::Mutex;
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

#[derive(Debug, Clone)]
pub struct Runner {
    inner: Arc<Mutex<()>>,
    missing_api_logging: Arc<AtomicBool>,
    shared_context_source: Arc<Mutex<Option<pocket_core::winceapi::gles::SharedContextSource>>>,
    shared_export: Arc<AtomicBool>,
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            inner: Default::default(),
            missing_api_logging: Arc::new(AtomicBool::new(true)),
            shared_context_source: Default::default(),
            shared_export: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Runner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_shared_context_source(
        &self,
        source: Option<pocket_core::winceapi::gles::SharedContextSource>,
    ) {
        *self
            .shared_context_source
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = source;
    }

    pub fn shared_export_policy(&self) -> Arc<AtomicBool> {
        self.shared_export.clone()
    }

    /// Stop publication first, then allow the worker's final CPU snapshot while
    /// the UI sharing source is still alive. Closing it before join prevents a
    /// GPU-only worker from creating its first readback context at shutdown.
    pub(crate) fn stop_shared_worker(
        &self,
        context: &mut Option<pocket_core::winceapi::gles::ExternalGpuContext>,
        input: &mut Option<Sender<InputCommand>>,
        frames: &mut Option<Receiver<FrameSnapshot>>,
        worker: &mut Option<std::thread::JoinHandle<()>>,
    ) {
        self.shared_export.store(false, Ordering::Release);
        self.set_shared_context_source(None);
        if let Some(input) = input.take() {
            let _ = input.send(InputCommand::Stop);
        }
        frames.take();
        if let Some(worker) = worker.take() {
            let _ = worker.join();
        }
        if let Some(context) = context.as_mut() {
            context.close();
        }
        context.take();
    }

    pub fn set_missing_api_logging(&self, enabled: bool) {
        self.missing_api_logging.store(enabled, Ordering::Relaxed);
    }

    pub fn run_game(
        &self,
        library_root: PathBuf,
        game: GameEntry,
        live_tx: Option<Sender<FrameSnapshot>>,
        input_rx: Option<Receiver<InputCommand>>,
        gps_notice_tx: Option<Sender<(f64, f64)>>,
        dependency_notice_tx: Option<Sender<String>>,
    ) -> RunOutcome {
        let _guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut hook = RunHook::new(live_tx, input_rx);
        hook.gps_notice_tx = gps_notice_tx;
        hook.dependency_notice_tx = dependency_notice_tx;
        let card_root = game.gizmondo_card_root(&library_root);
        self.run_process(
            &library_root,
            &game,
            &card_root,
            None,
            None,
            None,
            None,
            &mut hook,
        )
        .0
    }

    fn run_process(
        &self,
        library_root: &PathBuf,
        game: &GameEntry,
        card_root: &PathBuf,
        guest_path: Option<&str>,
        inherited_ram: Option<pocket_core::kernel::memory_division::MemoryDivision>,
        handle_context: Option<pocket_core::kernel::ProcessHandleContext>,
        startup: Option<ProcessStartup>,
        hook: &mut RunHook,
    ) -> (RunOutcome, u32) {
        let source = self
            .shared_context_source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        pocket_core::winceapi::gles::with_shared_context_source(source, || {
            pocket_core::winceapi::gles::with_shared_frame_export(
                self.shared_export.clone(),
                || {
                    self.run_process_inner(
                        library_root,
                        game,
                        card_root,
                        guest_path,
                        inherited_ram,
                        handle_context,
                        startup,
                        hook,
                    )
                },
            )
        })
    }

    fn run_process_inner(
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
        let exe = if guest_path.is_some() {
            game.executable_path(library_root)
        } else {
            game.launch_path(library_root)
        };
        let mut summary_lines = vec![format!("Game: {}", game.display_name)];

        let metadata = pocket_core::pe::load_file(&exe).ok();
        let machine = metadata
            .as_ref()
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
        let launcher_config = pocket_library::Library::open(library_root)
            .map(|l| l.config().clone())
            .unwrap_or_default();
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
        emu.set_frame_rate_limit(game.settings.effective_frame_rate_limit(&launcher_config));
        emu.set_unimplemented_api_sink(Box::new(crate::unimplemented_log::UnimplementedLog::new(
            library_root.join("pockethle-unimplemented-apis.log"),
            game.display_name.clone(),
            guest_path
                .map(str::to_owned)
                .unwrap_or_else(|| exe.display().to_string()),
            Arc::clone(&self.missing_api_logging),
        )));
        emu.max_slices = game.settings.max_slices;
        emu.instruction_budget_per_slice = game.settings.instructions_per_slice;

        if let Err(e) = emu.load_pe(&exe) {
            summary_lines.push(format!("load_pe failed: {e:#}"));
            return (
                RunOutcome {
                    summary: summary_lines.join("\n"),
                    framebuffer: None,
                },
                0xc0000135,
            );
        }

        // The screen has to be sized before the game runs: a GAPI title
        // reads the display geometry once during start-up and lays its
        // HUD out around it. `set_screen_size` requires `load_pe` to have
        // created the process first, so this call must come after the
        // load succeeds.
        // Gizmondo hardware has a native 320x240 landscape LCD. Detect the
        // platform before the title starts so GAPI sees the real geometry.
        let is_gizmondo = is_gizmondo_game(&game, &library_root);
        emu.set_pocket_pc_input(!is_gizmondo);
        if is_gizmondo {
            emu.set_gizmondo_platform_identity();
        }
        emu.set_gizmondo_audio_compatibility(is_gizmondo);
        if let Some(process) = emu.process_mut() {
            process.state.touch_steering.enabled = !is_gizmondo
                && (game.settings.controls_mode()
                    == pocket_library::stylus_grid::ControlsMode::Zones);
        }
        if !is_gizmondo {
            let tx = hook.dependency_notice_tx.clone();
            emu.configure_pocketpc_dependencies(library_root, move |message| {
                if let Some(tx) = &tx {
                    let _ = tx.send(message);
                }
            });
        }
        if let Some(context) = handle_context {
            context.table.defer_process_exit();
            if let Some(process) = emu.process_mut() {
                process.state.attach_handle_context(context);
            }
        }
        let ram = inherited_ram.or_else(|| {
            is_gizmondo
                .then(pocket_core::kernel::memory_division::MemoryDivision::gizmondo_sdk_default)
        });
        if !emu.set_memory_division(ram) {
            if let Some(startup) = startup.as_mut() {
                startup.error = 8;
            }
            summary_lines.push("Insufficient device RAM to load process".to_string());
            return (
                RunOutcome {
                    summary: summary_lines.join("\n"),
                    framebuffer: None,
                },
                0xc0000017,
            );
        }
        if !is_gizmondo {
            if let Err(error) = emu.configure_pocketpc_storage(library_root) {
                if let Some(startup) = startup.as_mut() {
                    startup.error = 29;
                }
                return (
                    RunOutcome {
                        summary: format!("Cannot initialize PocketPC storage: {error:#}"),
                        framebuffer: None,
                    },
                    29,
                );
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
        emu.set_gpu_renderer(
            game.settings
                .gpu_renderer
                .unwrap_or(launcher_config.gpu_renderer),
        );
        emu.set_gizmondo_goforce_driver(is_gizmondo);
        emu.set_intel2700g_depth_bias(
            !is_gizmondo && launcher_config.pocketpc_intel2700g_depth_bias,
        );
        emu.set_gles_internal_resolution(launcher_config.gles_internal_resolution);
        log::info!("Gizmondo GPS configuration: gizmondo={} host_enabled={} fixed_enabled={} latitude={} longitude={}",is_gizmondo,launcher_config.gps_enabled,launcher_config.gps_fixed_enabled,launcher_config.gps_fixed_latitude,launcher_config.gps_fixed_longitude);
        let hardware = (
            launcher_config.bluetooth_enabled,
            launcher_config.camera_enabled,
            launcher_config.gps_enabled,
        );
        if let Some(process) = emu.process_mut() {
            if pocket_library::is_colors_game(&game, &library_root) {
                process
                    .state
                    .vfs
                    .set_colors_language(pocket_library::colors_launch_language(
                        &game,
                        &library_root,
                        &launcher_config.colors_language,
                    ));
            }
            hook.colors_random_gps = if pocket_library::is_colors_game(game, library_root)
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
            if pocket_library::is_colors_game(&game, &library_root)
                && launcher_config.gps_fixed_enabled
            {
                let gps = process.state.vfs.gps_service();
                if gps
                    .set_fixed_position(launcher_config.simulated_gps_position())
                    .is_err()
                {
                    if let Some(startup) = startup.as_mut() {
                        startup.error = 87;
                    }
                    summary_lines.push(
                        "Invalid fixed GPS position: latitude -90..90, longitude -180..180".into(),
                    );
                    return (
                        RunOutcome {
                            summary: summary_lines.join("\n"),
                            framebuffer: None,
                        },
                        87,
                    );
                }
                gps.set_allowed(true);
                log::info!("GPS1 fixed simulation activated; Windows location is not used");
                summary_lines.push(format!(
                    "GPS: simulated fixed position ({:.7}, {:.7})",
                    launcher_config.gps_fixed_latitude, launcher_config.gps_fixed_longitude
                ));
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
                if let Some(startup) = startup.as_mut() {
                    startup.error = 29;
                }
                summary_lines.push(format!("Cannot initialize device registry: {e}"));
                return (
                    RunOutcome {
                        summary: summary_lines.join("\n"),
                        framebuffer: None,
                    },
                    29,
                );
            }
            let (preset, platform) = launcher_config.device_identity_hex(is_gizmondo);
            if let Err(e) = pocket_core::kernel::device_identity::DeviceIdentity::configure(
                &mut process.state.registry,
                preset,
                platform,
            ) {
                summary_lines.push(format!("Invalid device identity: {e}"));
                return (
                    RunOutcome {
                        summary: summary_lines.join("\n"),
                        framebuffer: None,
                    },
                    29,
                );
            }
            if is_gizmondo {
                process
                    .state
                    .internet
                    .set_gprs_enabled(launcher_config.gprs_enabled);
                if let Err(e) = pocket_core::kernel::colors::configure(
                    &mut process.state,
                    if pocket_library::is_colors_game(&game, &library_root) {
                        &launcher_config.colors_server_url
                    } else {
                        ""
                    },
                    &launcher_config.colors_terminal_id,
                ) {
                    if let Some(startup) = startup.as_mut() {
                        startup.error = 87;
                    }
                    summary_lines.push(e);
                    return (
                        RunOutcome {
                            summary: summary_lines.join("\n"),
                            framebuffer: None,
                        },
                        87,
                    );
                }
            }
        }
        if !is_gizmondo {
            if let Err(e) = emu
                .set_pocketpc_identity(
                    &game.settings.pocketpc_owner_name,
                    &game.settings.pocketpc_device_name,
                    &game.settings.pocketpc_device_id,
                )
                .and_then(|_| emu.set_pocketpc_preset_hex(&game.settings.pocketpc_preset_id))
                .and_then(|_| emu.set_pocketpc_random_seed(&game.settings.pocketpc_random_seed))
                .and_then(|_| {
                    emu.set_pocketpc_oem_identity(
                        &game.settings.pocketpc_oem_info,
                        &game.settings.pocketpc_platform_type,
                    )
                })
                .map(|_| emu.set_pocketpc_gapi_columns(game.settings.pocketpc_gapi_columns))
            {
                summary_lines.push(format!("Invalid PocketPC identity: {e}"));
                return (
                    RunOutcome {
                        summary: summary_lines.join("\n"),
                        framebuffer: None,
                    },
                    87,
                );
            }
        }
        let (screen_w, screen_h) = if is_gizmondo {
            (320, 240)
        } else {
            game.settings.screen.resolve_image(metadata.as_ref())
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
            let saved = emu
                .process()
                .and_then(|p| p.state.registry.value(&value.key, &value.name));
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
            // internal `\Flash Disk`. Alternative builds need an isolated
            // backing directory because they reuse the standard card's paths.
            // Preserve guest WinCE paths such as
            // `\Flash Disk\MyGames\Midway\Midway96.sav` work unchanged.
            let flash_disk = game.gizmondo_flash_dir(&library_root);
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
        }
        // A game that keeps its assets in one archive opens it by
        // absolute path built from its own module path -- Spore Origins
        // asks for `\Program Files\EA\Spore v1.0.4\data.vfs`. Mount the
        // extracted directory where the cabinet said it would be
        // installed and report that path from `GetModuleFileNameW`,
        // the way the CLI already does for a cab. Without it the
        // archive never opens and the game calls through a pointer it
        // never stored, which surfaces as READ_UNMAPPED at 0x2.
        for (prefix, directory) in game.install_mounts(&library_root) {
            emu.mount_read_only_dir(&prefix, directory);
        }
        if let Some(paths) = game.guest_launch_paths(library_root) {
            emu.mount_read_only_dir(
                &paths.install_prefix,
                game.install_mount_dir(library_root, &paths.install_prefix),
            );
            emu.set_module_path(&paths.module_path);
            emu.set_default_dir(&paths.working_dir);
            summary_lines.push(format!("Module path: {}", paths.module_path));
            let save_dir = game.save_dir(library_root);
            emu.mount_save_dir(&paths.save_prefix, &save_dir);
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
        if !is_gizmondo && game.source_cab.to_ascii_lowercase().ends_with(".cab") {
            if let Some(install_dir) = game.install_dir.as_deref() {
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
        // Use the real card-relative executable path after installing aliases.
        // CoPilot derives pid.cfg from GetModuleFileNameW and exits immediately
        // when a nested application is falsely reported at the card root.
        if is_gizmondo {
            if let Some(guest_exe) = game.gizmondo_guest_exe_path(library_root) {
                emu.set_module_path(&guest_exe);
                if let Some((directory, _)) = guest_exe.rsplit_once('\\') {
                    emu.set_default_dir(&format!("{directory}\\"));
                }
                log::info!("Gizmondo module path: {guest_exe}");
                summary_lines.push(format!("Module path: {guest_exe}"));
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
            runner: self.clone(),
            library_root: library_root.clone(),
            game: game.clone(),
            card_root: card_root.clone(),
        });
        hook.pid = emu.process().unwrap().state.object_handles.process_id();
        if let Some(startup) = startup.as_mut() {
            if let Err(error) = emu.set_startup_command_line(&startup.command_line) {
                startup.error = 8;
                summary_lines.push(format!("command line failed: {error:#}"));
                return (
                    RunOutcome {
                        summary: summary_lines.join("\n"),
                        framebuffer: None,
                    },
                    0xc0000017,
                );
            }
            if let Some(ack) = startup.ack.take() {
                let _ = ack.send(Ok(()));
            }
            // The parent commits PROCESS_INFORMATION before releasing startup.
            while !startup.table.start_allowed() && !hook.stop_all.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            if hook.stop_all.load(Ordering::Acquire) {
                emu.process_mut().unwrap().state.should_stop = true;
            }
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

        let framebuffer = emu.process_mut().and_then(|p| {
            let frame = p
                .state
                .presentation_framebuffer()
                .map_err(|error| log::error!("final presentation failed: {error}"))
                .ok()?;
            (!frame.is_all_black()).then(|| FrameSnapshot::from_framebuffer(&frame))
        });
        let exit_code = if run_result.is_ok() {
            emu.process()
                .and_then(|p| p.state.process_exit_code)
                .unwrap_or(0)
        } else {
            0xc0000005
        };
        // A surviving child may wait for this process to exit. Publish completion
        // after private RAM is refunded, BEFORE joining those surviving children.
        // Otherwise a child waiting for its parent and the host joining that child
        // wait on each other. Thread exits remain independently observable.
        let table = emu.process().map(|p| p.state.object_handles.clone());
        drop(emu);
        pocket_core::winceapi::gles::deferred_frame::release_reader();
        if let Some(table) = table {
            table.complete_process_exit(exit_code);
        }
        hook.finish_children();
        (
            RunOutcome {
                summary: summary_lines.join("\n"),
                framebuffer,
            },
            exit_code,
        )
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
        Emulator::with_unicorn_cpu_for_arch(pocket_core::cpu::Arch::Arm)
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
    /// Identity of this produced image; clones/filter/rotation changes preserve it.
    pub image_id: u64,
    pub presentation_stats: pocket_core::kernel::framebuffer::PresentationStats,
    pub width: u32,
    pub height: u32,
    pub texture_width: u32,
    pub texture_height: u32,
    pub rgba: Vec<u8>,
    pub text_input: bool,
    pub shared: Option<pocket_core::winceapi::gles::SharedFrame>,
    pub deferred: Option<Arc<pocket_core::kernel::framebuffer::deferred::DeferredPresentation>>,
    pub backlight: u32,
}

impl FrameSnapshot {
    pub(crate) fn next_image_id() -> u64 {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
    pub(crate) fn from_shared(
        fb: &pocket_core::kernel::framebuffer::deferred::DeferredFramebuffer,
        shared: pocket_core::winceapi::gles::SharedFrame,
    ) -> Result<Self, String> {
        let source = fb
            .gpu_source()
            .cloned()
            .ok_or("missing GPU pixel authority")?;
        if source.logical_size() != fb.logical_size() || fb.bpp != 16 {
            return Err("shared logical geometry mismatch".into());
        }
        let deferred = Arc::new(
            pocket_core::kernel::framebuffer::deferred::DeferredPresentation::new(
                source,
                fb.backlight_level(),
            )?,
        );
        if deferred.size() != shared.size() {
            return Err("shared snapshot geometry mismatch".into());
        }
        Ok(Self {
            image_id: Self::next_image_id(),
            presentation_stats: fb.presentation_stats.clone(),
            width: fb.width,
            height: fb.height,
            texture_width: shared.size().0,
            texture_height: shared.size().1,
            rgba: Vec::new(),
            text_input: false,
            shared: Some(shared),
            deferred: Some(deferred),
            backlight: fb.backlight_level(),
        })
    }
    /// Detach a saved image from a previous UI share group. Call before a
    /// planned context teardown, or after teardown only if pixels are cached.
    pub(crate) fn cpu_only(&self) -> Result<Self, String> {
        Ok(Self {
            image_id: self.image_id,
            presentation_stats: self.presentation_stats.clone(),
            width: self.width,
            height: self.height,
            texture_width: self.texture_width,
            texture_height: self.texture_height,
            rgba: self.cpu_rgba()?.into_owned(),
            text_input: self.text_input,
            shared: None,
            deferred: None,
            backlight: self.backlight,
        })
    }
    /// CPU filters, captures and conversion failures request bytes explicitly.
    pub(crate) fn cpu_rgba(&self) -> Result<std::borrow::Cow<'_, [u8]>, String> {
        let expected = (self.texture_width as usize)
            .checked_mul(self.texture_height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("snapshot size overflow")?;
        if self.rgba.len() == expected {
            return Ok(std::borrow::Cow::Borrowed(&self.rgba));
        }
        let deferred = self
            .deferred
            .as_ref()
            .ok_or("CPU fallback pixels unavailable")?;
        if deferred.size() != (self.texture_width, self.texture_height) {
            return Err("CPU fallback geometry mismatch".into());
        }
        Ok(std::borrow::Cow::Owned((*deferred.rgba()?).clone()))
    }
    fn from_framebuffer(fb: &pocket_core::kernel::Framebuffer) -> Self {
        let mut rgba = Vec::new();
        fb.snapshot_presentation_rgba8888_into(&mut rgba);
        Self {
            image_id: Self::next_image_id(),
            presentation_stats: fb.presentation_stats.clone(),
            text_input: false,
            shared: None,
            deferred: None,
            backlight: fb.backlight_level(),
            width: fb.width,
            height: fb.height,
            texture_width: fb.presentation_size().0,
            texture_height: fb.presentation_size().1,
            rgba,
        }
    }

    /// Like [`FrameSnapshot::from_framebuffer`] but reuses an existing
    /// `rgba` buffer the caller owns. Saves the per-frame 300 KiB
    /// allocation that the naive constructor does.
    fn fill_from_framebuffer(fb: &pocket_core::kernel::Framebuffer, scratch: &mut Vec<u8>) -> Self {
        fb.snapshot_presentation_rgba8888_into(scratch);
        Self {
            image_id: Self::next_image_id(),
            presentation_stats: fb.presentation_stats.clone(),
            text_input: false,
            shared: None,
            deferred: None,
            backlight: fb.backlight_level(),
            width: fb.width,
            height: fb.height,
            texture_width: fb.presentation_size().0,
            texture_height: fb.presentation_size().1,
            // `std::mem::take` hands ownership of the scratch buffer
            // to the snapshot we are about to ship across the
            // channel and leaves an empty `Vec` behind. The next
            // call resizes it back up — same allocation, no churn.
            rgba: std::mem::take(scratch),
        }
    }
}

/// Command pushed by the GUI thread into the running emulator.
#[derive(Debug, Clone)]
pub enum InputCommand {
    RecordAudio(Option<Arc<pocket_core::kernel::audio::OutputRecording>>),
    /// User input (tap / D-pad / key) to forward to the guest.
    Input(InputEvent),
    /// Explicitly choose another Colors Eiffel Tower simulated fix.
    RefreshColorsGps,
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
    ack: Option<Sender<Result<(), u32>>>,
    table: pocket_core::kernel::handles::HandleTable,
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
    table: pocket_core::kernel::handles::HandleTable,
    thread: std::thread::JoinHandle<(RunOutcome, u32)>,
}

struct RunHook {
    recording_audio: Option<Arc<pocket_core::kernel::audio::OutputRecording>>,
    recording_audio_pending: bool,
    colors_random_gps: Option<pocket_library::LauncherConfig>,
    refresh_colors_gps: bool,
    gps_notice_tx: Option<Sender<(f64, f64)>>,
    dependency_notice_tx: Option<Sender<String>>,
    geofence_down: bool,
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
        let Some(request) = state.pending_process_launch.take() else {
            return;
        };
        let context = self.launch_context.clone();
        let relative = context.as_ref().and_then(|context| {
            let child = request.executable.canonicalize().ok()?;
            // CreateProcess uses a path already resolved through the guest VFS.
            // SDCreateProcess keeps its validated foreground card restriction.
            if request.concurrent {
                return Some(
                    context
                        .game
                        .child_executable_reference(&context.library_root, &child),
                );
            }
            let root = context.card_root.canonicalize().ok()?;
            if !child.starts_with(root) {
                return None;
            }
            let game_dir = context
                .library_root
                .join(context.game.relative_dir())
                .canonicalize()
                .ok()?;
            child.strip_prefix(game_dir).ok().map(PathBuf::from)
        });
        if request.concurrent {
            let result = if let (Some(context), Some(relative)) = (context, relative) {
                let mut child_game = context.game.clone();
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
                        let result = context.runner.run_process(
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
            let mut child_game = context.game.clone();
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
            let (outcome, code) = context.runner.run_process(
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
            self.stopped_by_user |= child_hook.stopped_by_user;
            // run_process has dropped its child Emulator and its media first.
            drop(saved);
            state.audio.resume_output();
            log::info!("child finished with code 0x{exit_code:08x}; resuming preserved parent");
            if exit_code != 0 {
                log::warn!("child outcome: {}", outcome.summary);
            }
            self.reset_process();
            // Repaint the preserved parent frame even if its counter is static.
            state.framebuffer.frame_counter = state.framebuffer.frame_counter.wrapping_add(1);
            state.pending_input.clear();
            if let Some(up) = state.stylus_stick.cancel() {
                state.pending_input.push_back(up);
            }
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

    fn drain_input(&mut self, pending: &mut std::collections::VecDeque<InputEvent>) -> bool {
        let mut stop_requested = self.stop_all.load(Ordering::Acquire);
        if !self.input_disconnected {
            if let Some(rx) = self.input_rx.as_ref() {
                let rx = rx.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    match rx.try_recv() {
                        Ok(InputCommand::Input(ev)) => {
                            match ev {
                                InputEvent::KeyDown { vk: 0x73 } => {
                                    if !self.geofence_down && self.colors_random_gps.is_some() {
                                        self.refresh_colors_gps = true;
                                    }
                                    self.geofence_down = true;
                                }
                                InputEvent::KeyUp { vk: 0x73 } => self.geofence_down = false,
                                _ => {}
                            }
                            // Keep Geofence's normal guest behavior for all input sources.
                            pending.push_back(ev);
                        }

                        Ok(InputCommand::RefreshColorsGps) => self.refresh_colors_gps = true,
                        Ok(InputCommand::RecordAudio(recording)) => {
                            self.recording_audio = recording;
                            self.recording_audio_pending = true;
                        }
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
        if self.stopped_by_user {
            self.stop_all.store(true, Ordering::Release);
        }
        stop_requested || self.input_disconnected
    }

    fn child_hook(&self) -> Self {
        let mut hook = Self::new(self.frame_tx.clone(), None);
        hook.dependency_notice_tx = self.dependency_notice_tx.clone();
        hook.recording_audio = self.recording_audio.clone();
        hook.recording_audio_pending = true;
        hook.input_rx = self.input_rx.clone();
        hook.stop_all = self.stop_all.clone();
        hook.foreground = self.foreground.clone();
        hook
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
            recording_audio: None,
            recording_audio_pending: false,
            colors_random_gps: None,
            refresh_colors_gps: false,
            gps_notice_tx: None,
            dependency_notice_tx: None,
            geofence_down: false,
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
        let mut stop_requested = if active {
            self.drain_input(&mut state.pending_input)
        } else {
            self.stop_all.load(Ordering::Acquire)
        };
        if std::mem::take(&mut self.recording_audio_pending) {
            state.audio.record_output(self.recording_audio.clone());
        }
        if std::mem::take(&mut self.refresh_colors_gps) && !stop_requested {
            if let Some(config) = self.colors_random_gps.as_ref() {
                if let Some(position) = config.simulated_gps_position() {
                    if state
                        .vfs
                        .gps_service()
                        .set_fixed_position(Some(position))
                        .is_ok()
                    {
                        if let Some(tx) = &self.gps_notice_tx {
                            let _ = tx.send(position);
                        }
                    }
                }
            }
        }
        if !stop_requested {
            self.run_pending_child(state);
        }
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
                    let now = Instant::now();
                    // Keep the presentation phase instead of restarting the
                    // interval after each GPU readback/scheduler delay. The
                    // latter accumulates jitter and periodically skips frames.
                    if let Some(phase) = presentation_phase(self.last_emit_at, now) {
                        let shared = if !state.shell_keyboard.visible
                            && state.gdi.shell_menus.tracking.is_none()
                        {
                            pocket_core::winceapi::gles::take_shared_window_frame(
                                &state.framebuffer,
                            )
                        } else {
                            None
                        };
                        let mut snapshot = if let Some(shared) = shared {
                            match FrameSnapshot::from_shared(&state.framebuffer, shared) {
                                Ok(frame) => frame,
                                Err(error) => {
                                    log::error!("GPU snapshot failed: {error}");
                                    state.should_stop = true;
                                    return FrameAction::Stop;
                                }
                            }
                        } else {
                            let black = match state.framebuffer.cpu() {
                                Ok(frame) => frame.is_all_black(),
                                Err(error) => {
                                    log::error!("framebuffer read failed: {error}");
                                    state.should_stop = true;
                                    return FrameAction::Stop;
                                }
                            };
                            if black && self.saw_non_black {
                                return if stop_requested {
                                    state.should_stop = true;
                                    FrameAction::Stop
                                } else {
                                    FrameAction::Continue
                                };
                            }
                            let presentation = match state.presentation_framebuffer() {
                                Ok(frame) => frame,
                                Err(error) => {
                                    log::error!("presentation read failed: {error}");
                                    state.should_stop = true;
                                    return FrameAction::Stop;
                                }
                            };
                            FrameSnapshot::fill_from_framebuffer(&presentation, &mut self.scratch)
                        };
                        // Valid GPU frames (including black clears) are accepted without a
                        // full CPU black scan. Final CPU snapshots keep the legacy heuristic.
                        self.saw_non_black = true;
                        self.last_frame = counter;
                        self.last_emit_at = Some(phase);
                        snapshot.text_input = state.gapi_action_key
                            == pocket_core::kernel::gapi::VK_RETURN
                            && state
                                .controls
                                .get(if state.shell_keyboard.sip {
                                    state.shell_keyboard.target
                                } else {
                                    state.controls.focus
                                })
                                .is_some_and(|c| {
                                    c.class == pocket_core::kernel::controls::ControlClass::Edit
                                        && c.visible
                                        && c.enabled
                                        && c.style & 0x800 == 0
                                });
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
        check_parallel_children_and_orphan(false);
    }

    #[cfg(feature = "unicorn")]
    #[test]
    fn desktop_child_process_publishes_exit_before_joining_its_orphan() {
        check_parallel_children_and_orphan(true);
    }

    #[cfg(feature = "unicorn")]
    fn check_parallel_children_and_orphan(deferred: bool) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pockethle-runner-proc-{}-{nonce}",
            std::process::id()
        ));
        let card = root.join("games/probe/extracted");
        let dir = card.join("GZHD999989/ram");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in [
            (
                "proctest.exe",
                include_bytes!(
                    "../../../tools/hardware-diag/suites/ram/dist/GZRT999999/proctest.exe"
                )
                .as_slice(),
            ),
            (
                "procworker.exe",
                include_bytes!(
                    "../../../tools/hardware-diag/suites/ram/dist/GZRT999999/procworker.exe"
                )
                .as_slice(),
            ),
            (
                "badproc.exe",
                include_bytes!(
                    "../../../tools/hardware-diag/suites/ram/dist/GZRT999999/badproc.exe"
                )
                .as_slice(),
            ),
        ] {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        std::fs::write(card.join("GZHD999989/GZHD999989"), 999999u32.to_le_bytes()).unwrap();
        let mut settings = pocket_library::GameSettings::default();
        settings.cpu_backend = CpuBackendPref::Unicorn;
        settings.max_slices = 3_000_000;
        let game = GameEntry {
            platform: None,
            id: "probe".into(),
            display_name: "Process diagnostic".into(),
            provider: None,
            executable: PathBuf::from("extracted/GZHD999989/ram/proctest.exe"),
            source_cab: "diagnostic.zip".into(),
            install_dir: None,
            install_dirs: vec![],
            save_prefix: None,
            registry: vec![],
            imported_at: 0,
            settings,
            icon: None,
            companions: vec![],
        };
        // The original top-level test does not defer exit publication. Exercise
        // the real child-process context too: that is where the deadlock occurred.
        let mut parent = if deferred {
            let mut emu = Emulator::with_unicorn_cpu().unwrap();
            emu.load_pe(dir.join("proctest.exe")).unwrap();
            Some(emu)
        } else {
            None
        };
        let context = parent.as_mut().map(|parent| {
            let state = &mut parent.process_mut().unwrap().state;
            let (handle, _) = state.object_handles.new_child().unwrap();
            state.child_handle_context(handle).unwrap()
        });
        let table = context.as_ref().map(|context| context.table.clone());
        let mut hook = RunHook::new(None, None);
        let (outcome, code) =
            Runner::new().run_process(&root, &game, &card, None, None, context, None, &mut hook);
        assert_eq!(code, 77, "{}", outcome.summary);
        if let Some(table) = table {
            assert_eq!(table.exit_code(table.process_id(), None), Some(77));
        }
        for (name, expected) in [
            ("PROCTEST.TXT", "PROCTEST_RESULT PASS"),
            ("ORPHANTEST.TXT", "ORPHANTEST_RESULT PASS"),
        ] {
            let path = std::fs::read_dir(root.join("flash"))
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| {
                    p.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(name)
                })
                .unwrap();
            let report = std::fs::read_to_string(path).unwrap();
            println!("{report}");
            assert!(
                report.contains(expected) && !report.contains("FAIL"),
                "{report}"
            );
        }
        drop(parent);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(feature = "unicorn")]
    #[test]
    fn desktop_runner_executes_vfs_diagnostic_and_cleans_probe_files() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("pockethle-runner-vfs-{nonce}"));
        let card = root.join("games/probe/extracted");
        let dir = card.join("GZHD999989/vfs");
        std::fs::create_dir_all(&dir).unwrap();
        for (n, b) in [
            (
                "AUTORUN.EXE",
                include_bytes!(
                    "../../../tools/hardware-diag/suites/vfs/dist/GZVT999998/AUTORUN.EXE"
                )
                .as_slice(),
            ),
            (
                "vfsworker.exe",
                include_bytes!(
                    "../../../tools/hardware-diag/suites/vfs/dist/GZVT999998/vfsworker.exe"
                )
                .as_slice(),
            ),
            (
                "asset.bin",
                include_bytes!("../../../tools/hardware-diag/suites/vfs/dist/GZVT999998/asset.bin")
                    .as_slice(),
            ),
        ] {
            std::fs::write(dir.join(n), b).unwrap();
        }
        std::fs::write(card.join("GZHD999989/GZHD999989"), 999998u32.to_le_bytes()).unwrap();
        let mut settings = pocket_library::GameSettings::default();
        settings.cpu_backend = CpuBackendPref::Unicorn;
        settings.max_slices = 3_000_000;
        let game = GameEntry {
            platform: None,
            id: "probe".into(),
            display_name: "VFS diagnostic".into(),
            provider: None,
            executable: PathBuf::from("extracted/GZHD999989/vfs/AUTORUN.EXE"),
            source_cab: "diagnostic.zip".into(),
            install_dir: None,
            install_dirs: vec![],
            save_prefix: None,
            registry: vec![],
            imported_at: 0,
            settings,
            icon: None,
            companions: vec![],
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let completed = Arc::new(AtomicBool::new(false));
        let finished = completed.clone();
        let input = std::thread::spawn(move || {
            while !finished.load(Ordering::Acquire) {
                let _ = tx.send(InputCommand::Input(InputEvent::KeyDown { vk: 0x0d }));
                let _ = tx.send(InputCommand::Input(InputEvent::KeyUp { vk: 0x0d }));
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let mut hook = RunHook::new(None, Some(rx));
        let (outcome, code) =
            Runner::new().run_process(&root, &game, &card, None, None, None, None, &mut hook);
        completed.store(true, Ordering::Release);
        input.join().unwrap();
        assert_eq!(code, 0, "{}", outcome.summary);
        let entries: Vec<_> = std::fs::read_dir(root.join("flash"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        let mut names: Vec<_> = entries
            .iter()
            .map(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_ascii_lowercase()
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["pockethle-vfstest-persist-v3.bin", "vfstest.txt"],
            "only the report and versioned persistence marker may remain"
        );
        let report_path = entries
            .iter()
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("VFSTEST.TXT")
            })
            .unwrap();
        let report = std::fs::read_to_string(report_path).unwrap();
        assert!(
            report.contains("VFSTEST_RESULT PASS checks=0x00000082 failures=0x00000000")
                && !report.lines().any(|line| line.starts_with("FAIL ")),
            "{report}"
        );
        let marker_path = entries
            .iter()
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("PocketHLE-VFSTEST-PERSIST-v3.bin")
            })
            .unwrap();
        let marker = std::fs::read(marker_path).unwrap();
        let expected: Vec<_> = [0x56544633u32, 1, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(
            marker, expected,
            "first successful run must persist its versioned marker"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn child_stop_cancels_background_parent_and_other_children() {
        let (tx, rx) = std::sync::mpsc::channel();
        let parent = RunHook::new(None, Some(rx));
        let mut child = parent.child_hook();
        let sibling = parent.child_hook();
        tx.send(InputCommand::Stop).unwrap();
        assert!(child.drain_input(&mut std::collections::VecDeque::new()));
        assert!(parent.stop_all.load(Ordering::Acquire));
        assert!(sibling.stop_all.load(Ordering::Acquire));
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
    #[test]
    fn geofence_preserves_guest_events_and_refreshes_only_on_press_in_eiffel_mode() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut hook = RunHook::new(None, Some(rx));
        let mut pending = std::collections::VecDeque::new();
        let down = InputEvent::KeyDown { vk: 0x73 };
        let up = InputEvent::KeyUp { vk: 0x73 };
        tx.send(InputCommand::Input(down)).unwrap();
        tx.send(InputCommand::Input(up)).unwrap();
        assert!(!hook.drain_input(&mut pending));
        assert!(!hook.refresh_colors_gps);
        let mut config = pocket_library::LauncherConfig::default();
        config.gps_fixed_enabled = true;
        config.gps_fixed_random = true;
        hook.colors_random_gps = Some(config);
        tx.send(InputCommand::Input(down)).unwrap();
        assert!(!hook.drain_input(&mut pending));
        assert!(std::mem::take(&mut hook.refresh_colors_gps));
        tx.send(InputCommand::Input(down)).unwrap();
        assert!(!hook.drain_input(&mut pending));
        assert!(!hook.refresh_colors_gps);
        tx.send(InputCommand::Input(up)).unwrap();
        assert!(!hook.drain_input(&mut pending));
        assert_eq!(
            pending.iter().copied().collect::<Vec<_>>(),
            vec![down, up, down, down, up]
        );
        tx.send(InputCommand::RefreshColorsGps).unwrap();
        assert!(!hook.drain_input(&mut pending));
        assert!(hook.refresh_colors_gps);
    }
}

#[cfg(test)]
mod internal_resolution_tests {
    use super::FrameSnapshot;
    #[test]
    fn physical_texture_does_not_change_logical_input_or_zoom_dimensions() {
        let mut fb = pocket_core::kernel::Framebuffer::new(2, 1);
        fb.set_render_presentation(4, 2, vec![255; 4 * 2 * 2]);
        let frame = FrameSnapshot::from_framebuffer(&fb);
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!((frame.texture_width, frame.texture_height), (4, 2));
        assert_eq!(frame.rgba.len(), 4 * 2 * 4);
        fb.mark_dirty();
        let frame = FrameSnapshot::from_framebuffer(&fb);
        assert_eq!((frame.texture_width, frame.texture_height), (2, 1));
        assert_eq!(frame.rgba.len(), 2 * 1 * 4);
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
    #[ignore = "requires native UI sharing and an execution thread"]
    fn shutdown_materializes_the_final_gpu_frame_before_closing_ui_source() {
        use pocket_core::kernel::framebuffer::{deferred::DeferredFramebuffer, Framebuffer};
        let ui = pocket_gles::gpu::Renderer::new(8, 8).unwrap();
        let mut owner = Some(unsafe {
            pocket_core::winceapi::gles::ExternalGpuContext::capture_current().unwrap()
        });
        let source = owner.as_ref().unwrap().source();
        let runner = Runner::new();
        runner.set_shared_context_source(Some(source.clone()));
        let policy = runner.shared_export_policy();
        policy.store(true, Ordering::Release);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (input_tx, input_rx) = std::sync::mpsc::channel();
        let (_frame_tx, frame_rx) = std::sync::mpsc::channel::<FrameSnapshot>();
        let (final_tx, final_rx) = std::sync::mpsc::channel();
        let worker_source = source.clone();
        let mut worker = Some(std::thread::spawn(move || {
            pocket_gles::gpu::with_shared_context_source(Some(worker_source.clone()), || {
                let mut c = pocket_gles::context::Context::new_rgb565(320, 240);
                c.enable_gpu().unwrap();
                c.clear_color = [1., 0., 0., 1.];
                c.clear(pocket_gles::GL_COLOR_BUFFER_BIT);
                let frame = c.snapshot_shared_frame().unwrap();
                let provider = Arc::new(
                    pocket_core::winceapi::gles::deferred_frame::GlesCpuPixelSource::new(
                        frame,
                        worker_source,
                        (320, 240),
                    )
                    .unwrap(),
                );
                let mut fb = DeferredFramebuffer::new(Framebuffer::new(320, 240)).unwrap();
                fb.publish_gpu(provider).unwrap();
                assert!(!fb.cpu_resident());
                ready_tx.send(()).unwrap();
                assert!(matches!(input_rx.recv().unwrap(), InputCommand::Stop));
                assert!(!policy.load(Ordering::Acquire));
                // No readback context existed before stop. The source must
                // remain open until this final CPU consumer has completed.
                let pixels = fb.cpu().unwrap().snapshot_rgba8888();
                final_tx.send(pixels[..4].to_vec()).unwrap();
                pocket_core::winceapi::gles::deferred_frame::release_reader();
            });
        }));
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let mut input = Some(input_tx);
        let mut frames = Some(frame_rx);
        runner.stop_shared_worker(&mut owner, &mut input, &mut frames, &mut worker);
        assert_eq!(
            final_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            [255, 0, 0, 255]
        );
        assert!(owner.is_none() && input.is_none() && frames.is_none() && worker.is_none());
        assert!(
            pocket_gles::gpu::SharedReadback::new(&source).is_err(),
            "source not closed after join"
        );
        runner.stop_shared_worker(&mut owner, &mut input, &mut frames, &mut worker);
        drop(ui);
    }

    #[test]
    #[ignore = "requires native shared OpenGL"]
    fn actual_swap_to_hook_keeps_gpu_pixels_until_a_capture_requests_them() {
        use pocket_core::{
            cpu::{regs::ArmReg, stub::StubCpu, Cpu, Prot},
            kernel::{DispatchOutcome, Dispatcher, Thunk},
            pe::ImportBinding,
        };
        fn api(
            d: &mut pocket_core::winceapi::WinCeDispatcher,
            cpu: &mut StubCpu,
            k: &mut KernelState,
            name: &str,
            args: &[u32],
        ) -> u32 {
            for (index, reg) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
                .into_iter()
                .enumerate()
            {
                cpu.write_reg(reg, args.get(index).copied().unwrap_or(0))
                    .unwrap();
            }
            let thunk = Thunk {
                thunk_va: 0x70000000,
                iat_va: 0,
                dll: "libGLES_CM.dll".into(),
                binding: ImportBinding::Name(name.into()),
                friendly_name: None,
            };
            match d.dispatch(cpu, &thunk, k).unwrap() {
                DispatchOutcome::ReturnedR0(value) => value,
                other => panic!("unexpected {name} result: {other:?}"),
            }
        }
        let ui = pocket_gles::gpu::Renderer::new(8, 8).unwrap();
        let source = ui.shared_context_source();
        let mut d = pocket_core::winceapi::WinCeDispatcher::new();
        let mut cpu = StubCpu::new();
        cpu.map_region(0x200000, 0x4000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.write_reg(ArmReg::Sp, 0x203000).unwrap();
        let mut process = process();
        let k = &mut process.state;
        k.framebuffer.resize(320, 240);
        let enabled = Arc::new(AtomicBool::new(true));
        pocket_core::winceapi::gles::with_shared_context_source(Some(source), || {
            pocket_core::winceapi::gles::with_shared_frame_export(enabled, || {
                let display = api(&mut d, &mut cpu, k, "eglGetDisplay", &[0]);
                assert_ne!(display, 0);
                assert_eq!(
                    api(&mut d, &mut cpu, k, "eglInitialize", &[display, 0, 0]),
                    1
                );
                assert_eq!(
                    api(
                        &mut d,
                        &mut cpu,
                        k,
                        "eglGetConfigs",
                        &[display, 0x200000, 1, 0x200004]
                    ),
                    1
                );
                let config =
                    u32::from_le_bytes(cpu.read_mem(0x200000, 4).unwrap().try_into().unwrap());
                let surface = api(
                    &mut d,
                    &mut cpu,
                    k,
                    "eglCreateWindowSurface",
                    &[display, config, 1, 0],
                );
                let context = api(
                    &mut d,
                    &mut cpu,
                    k,
                    "eglCreateContext",
                    &[display, config, 0, 0],
                );
                assert_ne!(surface, 0);
                assert_ne!(context, 0);
                assert_eq!(
                    api(
                        &mut d,
                        &mut cpu,
                        k,
                        "eglMakeCurrent",
                        &[display, surface, surface, context]
                    ),
                    1
                );
                pocket_core::winceapi::gles::set_gpu_renderer(true);
                let (tx, rx) = std::sync::mpsc::channel();
                let mut hook = RunHook::new(Some(tx), None);
                let mut frames = Vec::new();
                for color in [[65536, 0, 0, 65536], [0, 65536, 0, 65536]] {
                    api(&mut d, &mut cpu, k, "glClearColorx", &color);
                    api(&mut d, &mut cpu, k, "glClear", &[0x4000]);
                    // Pixel handoff test; cooperative pacing has its own native suite.
                    k.framebuffer.directdraw.last_present = None;
                    k.framebuffer.directdraw.pending.clear();
                    assert_eq!(
                        api(&mut d, &mut cpu, k, "eglSwapBuffers", &[display, surface]),
                        1
                    );
                    assert!(!k.framebuffer.cpu_resident());
                    hook.last_emit_at = None;
                    assert!(matches!(hook.on_frame(k), FrameAction::Continue));
                    assert!(
                        !k.framebuffer.cpu_resident(),
                        "display hook read GPU pixels"
                    );
                    let frame = rx.try_recv().unwrap();
                    assert!(frame.rgba.is_empty());
                    assert!(frame.shared.is_some());
                    assert!(frame.deferred.as_ref().unwrap().cached_rgba().is_none());
                    frames.push(frame);
                }
                pocket_core::winceapi::gles::set_gpu_renderer(false);
                assert_eq!(&frames[0].cpu_rgba().unwrap()[..4], &[255, 0, 0, 255]);
                assert_eq!(&frames[1].cpu_rgba().unwrap()[..4], &[0, 255, 0, 255]);
                assert!(
                    !k.framebuffer.cpu_resident(),
                    "UI capture populated kernel pixels"
                );
                pocket_core::winceapi::gles::deferred_frame::release_reader();
            });
        });
    }
    #[test]
    fn throttled_frames_never_read_gpu_and_due_read_failures_stop_without_stale_pixels() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut hook = RunHook::new(Some(tx), None);
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
        assert_eq!(&rx.try_recv().unwrap().rgba[..4], &[255, 0, 0, 255]);
    }
}
