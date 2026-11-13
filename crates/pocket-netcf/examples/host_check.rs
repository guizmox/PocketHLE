use pocket_netcf::host::{run, Options};
use std::cell::Cell;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let executable = PathBuf::from(std::env::args_os().nth(1).ok_or("expected executable")?);
    let frames = Cell::new(0);
    let mut last = Vec::new();
    run(
        &executable,
        Options {
            size: (240, 320),
            save_root: None,
            guest_path: None,
            tilt: None,
            gpu_renderer: false,
            gles_internal_resolution: 1,
            frame_rate_limit: Some(60),
        },
        |_| frames.get() < 30,
        |frame| {
            frames.set(frames.get() + 1);
            frame.snapshot_presentation_rgba8888_into(&mut last);
        },
        |_| {},
    )?;
    if frames.get() != 30 || !last.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]) {
        return Err(format!(
            "No visible game frame after {} frames",
            frames.get()
        ));
    }
    println!(
        "{} frames through the shared PocketHLE frame transport",
        frames.get()
    );
    Ok(())
}
