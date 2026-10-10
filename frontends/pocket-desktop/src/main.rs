//! Cross-platform desktop launcher GUI for PocketHLE.
//!
//! Targets Linux and Windows. The interface is deliberately modeled
//! after [`j2me-loader`](https://github.com/nikita36078/j2me-loader): a
//! library screen with cards for every imported game, an "Import" button
//! that pulls a `.CAB` file in via a native file dialog, a "Settings"
//! screen, and a per-game settings sheet. Selecting a card and pressing
//! "Run" opens a separate emulator viewport that displays the
//! framebuffer produced by [`pocket_core::Emulator`].

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod fullscreen_layout;
mod gamepad;
mod pocketpc_layout;
mod reconstruction;
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
struct Tee {
    file: std::fs::File,
}

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // stderr is best-effort: under `windows_subsystem = "windows"`
        // there is no console attached and every write fails. That must
        // not stop the file from being written.
        let _ = std::io::stderr().write_all(buf);
        self.file.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        self.file.flush()
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
        builder.target(env_logger::Target::Pipe(Box::new(Tee { file })));
    }
    builder.init();
    result
}

fn main() -> Result<()> {
    let library_root = default_library_root();
    let log_path = init_logging(&library_root);
    log::info!("Using library root: {}", library_root.display());
    if let Some(path) = log_path {
        log::info!("Logging to {}", path.display());
    }
    let library = Library::open(&library_root).context("opening PocketHLE library")?;

    let icon = image::load_from_memory(include_bytes!("../assets/pockethle_logo.png"))
        .context("decoding PocketHLE logo")?
        .to_rgba8();
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(std::sync::Arc::new(eframe::egui::IconData {
                width: icon.width(),
                height: icon.height(),
                rgba: icon.into_raw(),
            }))
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([640.0, 420.0])
            .with_fullscreen(library.config().fullscreen)
            .with_title("PocketHLE"),
        ..Default::default()
    };
    let mut library_slot = Some(library);
    eframe::run_native(
        "PocketHLE",
        native_options,
        Box::new(move |cc| {
            let lib = library_slot.take().expect("PocketLauncher built twice");
            Box::new(app::PocketLauncher::new(cc, lib)) as Box<dyn eframe::App>
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    Ok(())
}
