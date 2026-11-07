// Cross-platform desktop launcher GUI for PocketHLE.
//
// Targets Linux and Windows. The interface is deliberately modeled
// after [`j2me-loader`](https://github.com/nikita36078/j2me-loader): a
// library screen with cards for every imported game, an "Import" button
// that pulls a `.CAB` file in via a controller-friendly file dialog, a "Settings"
// screen, and a per-game settings sheet. Selecting a card and pressing
// "Run" opens a separate emulator viewport that displays the
// framebuffer produced by [`pocket_core::Emulator`].

mod app;
mod bindings_visual;
mod controller_ui;
mod deck_navigation;
mod desktop_pointer;
mod file_dialog;
mod frame_pacing;
mod fullscreen_layout;
mod gamepad;
mod gamepad_host;
mod gizmondo_layout;
mod host_gpu;
mod latest_frame;
mod launch_args;
mod native_share;
mod reconstruction;
mod recording;
mod recordings_gallery;
mod rumble;
mod runner;
mod screenshot;
mod smaa;
mod unimplemented_log;

use anyhow::{Context, Result};

use pocket_library::{default_library_root, Library};

/// Fan log output out to stderr *and* a file.
///
/// On Windows this binary is built with `windows_subsystem = "windows"`
/// (see the attribute above) so that launching it does not flash up a
/// console. The cost is that there is no stderr to read: a run that
/// fails to open an audio device, or that falls back to the stub CPU,
/// says so through `log` and the user sees nothing at all. Writing the
/// same lines to a file under the library root means "there is no
/// sound" can be answered by reading a log instead of by rebuilding
/// with a console subsystem.
// The restore transaction may replace the library directory. Keep ownership
// controllable so our own logger does not pin that directory on Windows.
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

struct Tee;

pub(crate) fn with_suspended_library_log<T>(
    root: &std::path::Path,
    operation: impl FnOnce() -> T,
) -> T {
    struct ResumeLog(Option<std::path::PathBuf>);
    impl Drop for ResumeLog {
        fn drop(&mut self) {
            if let Some(path) = self.0.take() {
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    Ok(file) => {
                        *LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
                    }
                    Err(error) => {
                        eprintln!("Cannot resume library log {}: {error}", path.display())
                    }
                }
            }
        }
    }
    let file = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()).take();
    let resume = ResumeLog(file.as_ref().map(|_| root.join("pockethle-gui.log")));
    if let Some(mut file) = file {
        let _ = std::io::Write::flush(&mut file);
        drop(file);
    }
    let result = operation();
    drop(resume);
    result
}

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // stderr is best-effort: under `windows_subsystem = "windows"`
        // there is no console attached and every write fails. That must
        // not stop the file from being written.
        let _ = std::io::stderr().write_all(buf);
        if let Some(file) = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            file.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        if let Some(file) = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            file.flush()?;
        }
        Ok(())
    }
}

/// Start logging, teeing to `<library root>/pockethle-gui.log` when that
/// file can be opened. The log is truncated on every launch: it exists
/// to explain the run the user just did, and an append-forever file in
/// a user data directory grows without anyone watching it.
fn init_logging(library_root: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    let _ = std::fs::create_dir_all(library_root);
    let path = library_root.join("pockethle-gui.log");
    let opened = std::fs::File::create(&path).ok();
    let result = opened.is_some().then(|| path.clone());
    if let Some(file) = opened {
        *LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
        builder.target(env_logger::Target::Pipe(Box::new(Tee)));
    }
    builder.init();
    result
}

fn main() -> Result<()> {
    let result = run_launcher();
    if let Err(error) = &result {
        log::error!("{error:#}");
        rfd::MessageDialog::new()
            .set_title("PocketHLE startup error")
            .set_description(format!("{error:#}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    result
}

fn run_launcher() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        println!("{}", launch_args::HELP);
        #[cfg(windows)]
        rfd::MessageDialog::new()
            .set_title("PocketHLE command line")
            .set_description(launch_args::HELP)
            .show();
        return Ok(());
    }
    let launch = launch_args::parse(args).map_err(anyhow::Error::msg)?;
    let title = if cfg!(feature = "steam-deck") {
        "PocketHLE — Steam Deck"
    } else {
        "PocketHLE"
    };
    let library_root = default_library_root();
    let log_path = init_logging(&library_root);
    log::info!("Using library root: {}", library_root.display());
    if let Some(path) = log_path {
        log::info!("Logging to {}", path.display());
    }
    let library = Library::open(&library_root).context("opening PocketHLE library")?;

    let launch_game = launch
        .as_ref()
        .map(|args| {
            launch_args::find_game(
                &args.game,
                library
                    .games()
                    .iter()
                    .map(|g| (g.id.as_str(), g.display_name.as_str())),
            )
            .map(|id| library.games().iter().find(|g| g.id == id).unwrap().clone())
            .map_err(anyhow::Error::msg)
        })
        .transpose()?;
    let explicit_fullscreen = launch.as_ref().and_then(|a| a.fullscreen);
    let fullscreen = launch_args::startup_fullscreen(
        cfg!(feature = "steam-deck"),
        library.config().fullscreen,
        explicit_fullscreen,
    );

    let icon = image::load_from_memory(include_bytes!("../assets/pockethle_logo.png"))
        .context("decoding PocketHLE logo")?
        .to_rgba8();
    let ui_vsync = library.config().desktop_vsync;
    log::info!(
        "HOST_UI_VSYNC_REQUEST={} (host window; game FPS setting is independent)",
        if ui_vsync { "ON" } else { "OFF" }
    );
    let native_options = eframe::NativeOptions {
        vsync: ui_vsync,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(std::sync::Arc::new(eframe::egui::IconData {
                width: icon.width(),
                height: icon.height(),
                rgba: icon.into_raw(),
            }))
            .with_app_id("PocketHLE")
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([640.0, 420.0])
            .with_fullscreen(fullscreen)
            .with_title(title),
        ..Default::default()
    };
    let mut library_slot = Some(library);
    eframe::run_native(
        title,
        native_options,
        Box::new(move |cc| {
            let lib = library_slot.take().expect("PocketLauncher built twice");
            let mut app = app::PocketLauncher::new(cc, lib);
            if let Some(game) = &launch_game {
                app.launch_from_command_line(game, launch.as_ref().and_then(|a| a.filter));
            }
            // Reapply after native window creation/restoration. Do this once;
            // the user can subsequently leave fullscreen using the normal controls.
            if cfg!(feature = "steam-deck") || explicit_fullscreen.is_some() {
                app.request_startup_fullscreen(fullscreen);
            }
            Box::new(app) as Box<dyn eframe::App>
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod restore_log_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn restore_releases_its_log_and_resumes_after_success_error_and_unwind() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp =
            std::env::temp_dir().join(format!("pockethle-log-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        let source = temp.join("source");
        drop(pocket_library::Library::open(&source).unwrap());
        std::fs::write(source.join("marker"), b"restored").unwrap();
        let archive = temp.join("backup.zip");
        pocket_library::backup::save(&source, &archive).unwrap();
        for (index, mode) in [
            pocket_library::backup::RestoreMode::MergeNoOverwrite,
            pocket_library::backup::RestoreMode::MergeOverwrite,
            pocket_library::backup::RestoreMode::Replace,
        ]
        .into_iter()
        .enumerate()
        {
            let root = temp.join(format!("target-{index}"));
            drop(pocket_library::Library::open(&root).unwrap());
            *LOG_FILE.lock().unwrap() =
                Some(std::fs::File::create(root.join("pockethle-gui.log")).unwrap());
            with_suspended_library_log(&root, || {
                assert!(LOG_FILE.lock().unwrap().is_none());
                pocket_library::backup::restore(&root, &archive, mode)
            })
            .unwrap();
            let mut tee = Tee;
            tee.write_all(b"after restore\n").unwrap();
            tee.flush().unwrap();
            assert_eq!(std::fs::read(root.join("marker")).unwrap(), b"restored");
            assert!(std::fs::read_to_string(root.join("pockethle-gui.log"))
                .unwrap()
                .contains("after restore"));
            let result: Result<(), &str> = with_suspended_library_log(&root, || Err("test error"));
            assert!(result.is_err());
            assert!(LOG_FILE.lock().unwrap().is_some());
            let panic = std::panic::catch_unwind(|| {
                with_suspended_library_log(&root, || panic!("test unwind"))
            });
            assert!(panic.is_err());
            assert!(LOG_FILE.lock().unwrap().is_some());
            LOG_FILE.lock().unwrap().take();
        }
        std::fs::remove_dir_all(temp).unwrap();
    }
}
