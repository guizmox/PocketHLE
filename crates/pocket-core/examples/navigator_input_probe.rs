//! Headless reproduction of desktop card mounts and module-path selection.
use pocket_core::{
    kernel::{FrameAction, InputEvent, KernelState},
    Emulator,
};

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
struct Logger;
impl log::Log for Logger {
 fn enabled(&self, m: &log::Metadata<'_>) -> bool { m.level() <= log::Level::Info }
 fn log(&self, r: &log::Record<'_>) { if self.enabled(r.metadata()) { eprintln!("{} {}", r.level(), r.args()); } }
 fn flush(&self) {}
}
fn main() -> anyhow::Result<()> {
    log::set_logger(&Logger).unwrap(); log::set_max_level(log::LevelFilter::Info);
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).expect("library root"));
    let legacy = args.get(2).is_some_and(|s| s == "legacy");
    let out = args.get(3).expect("snapshot path");
    let mut emu = Emulator::with_unicorn_cpu()?;
    emu.load_pe(root.join("CoPilot.exe"))?;
    for va in [0x115c8, 0x13574, 0x1c128, 0x20924, 0x20ea8, 0x22c00,
        0x24ddc, 0x25694, 0x273e8, 0x2934c, 0x2a974, 0x31170,
        0x47e00, 0x47f74, 0x4adc0, 0x591ec, 0x59c80, 0x5b4d8,
        0x5c408, 0x5d764, 0x639cc, 0x66e68, 0x6bbf0] {
        emu.add_register_trace(va, "Back-key comparison")?;
    }
    emu.set_memory_division(Some(
        pocket_core::kernel::memory_division::MemoryDivision::gizmondo_sdk_default(),
    ));
    emu.set_screen_size(320, 240);
    let extracted = root.clone();
    for prefix in [
        "\\Application\\",
        "\\Program Files\\",
        "\\Program Files\\Game\\",
        "\\expresso\\",
        "\\SD Card\\Game\\",
        "\\Storage Card\\Game\\",
    ] {
        emu.mount_read_only_dir(prefix, &extracted);
    }
    emu.mount_read_only_dir("\\SD Card\\", root.parent().unwrap());
    emu.mount_read_only_dir("\\Storage Card\\", root.parent().unwrap());
    emu.mount_read_only_dir("\\SD Card\\GZAP250006\\", &root);
    emu.mount_read_only_dir("\\Storage Card\\GZAP250006\\", &root);
    emu.mount_save_dir("\\Flash Disk\\", root.join("flash-probe"));
    let module: String = if legacy {
        "\\SD Card\\CoPilot.exe".into()
    } else {
        "\\SD Card\\GZAP250006\\CoPilot.exe".into()
    };
    emu.set_module_path(&module);
    emu.set_default_dir(&format!("{}\\", module.rsplit_once('\\').unwrap().0));
    emu.set_synthetic_message_budget(0);
    emu.set_trace_sink(Box::new(std::io::BufWriter::new(std::fs::File::create("runtime-nav/api.trace")?)));
    emu.max_slices = 0;
    let gps = emu.process().unwrap().state.vfs.gps_service();
    gps.set_fixed_position(Some((47.6062, -122.3321))).unwrap();
    gps.set_allowed(true);
    println!("legacy={legacy} module={module:?} GPS=47.6062,-122.3321 SD=read-only");
    let start = Instant::now();
    let mut stopped = false;
    let mut stage = 0;
    let input = args.get(4).map(PathBuf::from);
    let mut consumed = 0;
    emu.run_with_hook(&mut |state: &mut KernelState| {
        let elapsed = start.elapsed();
        if let Some(input) = &input {
            if let Ok(text) = std::fs::read_to_string(input) {
                let lines: Vec<_> = text.lines().collect();
                for line in lines.iter().skip(consumed) {
                    if *line == "quit" {
                        stopped = true;
                        return FrameAction::Stop;
                    }
                    if let Some(path) = line.strip_prefix("snapshot ") {
                        let _ = std::fs::write(path, state.framebuffer.snapshot_ppm());
                    }
                    if let Some(value) = line.strip_prefix("tap ") {
                        if let Some((x, y)) = value.split_once(' ') {
                            if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                                state.pending_input.push_back(InputEvent::PointerDown { x, y });
                                state.pending_input.push_back(InputEvent::PointerUp { x, y });
                            }
                        }
                    }
                    if let Some(path) = line.strip_prefix("dump ") {
                        let mut details = format!("active={:x} focus={:x}\ncontrols={:#?}\n", state.active_window, state.controls.focus, state.controls);
                        let mut windows: Vec<_> = state.window_dcs.iter().collect();
                        windows.sort_by_key(|(h, _)| **h);
                        for (&hwnd, &dc) in windows {
                            details.push_str(&format!("hwnd={hwnd:08x} dc={dc:08x} rect={:?} parent={:?} visible={:?} class={:?} dcmeta={:?}\n", state.window_rects.get(&hwnd), state.window_parents.get(&hwnd), state.window_visibility.get(&hwnd), state.window_classes.get(&hwnd), state.gdi.dc(dc)));
                            if let Some(bm) = state.gdi.dc(dc).and_then(|d| d.selected_bitmap).and_then(|b| state.gdi.bitmap(b)) {
                                let mut ppm = format!("P6\n{} {}\n255\n", bm.width, bm.height).into_bytes();
                                for pair in bm.pixels.chunks_exact(2) {
                                    let c = u16::from_le_bytes([pair[0], pair[1]]);
                                    ppm.extend_from_slice(&[(((c >> 11) * 255 / 31) as u8), ((((c >> 5) & 63) * 255 / 63) as u8), (((c & 31) * 255 / 31) as u8)]);
                                }
                                let _ = std::fs::write(format!("{path}-{hwnd:08x}.ppm"), ppm);
                            }
                        }
                        let _ = std::fs::write(format!("{path}.txt"), details);
                        let _ = std::fs::write(format!("{path}.ppm"), state.framebuffer.snapshot_ppm());
                    }
                    if *line == "windows" {
                        log::info!(
                            "active={:x} focus={:x} controls={:#?}",
                            state.active_window,
                            state.controls.focus,
                            state.controls
                        );
                    }
                    if let Some((command, value)) = line.split_once(' ') {
                        let vk = match value {
                            "enter" => Some(13),
                            "escape" => Some(27),
                            "space" => Some(32),
                            "left" => Some(37),
                            "up" => Some(38),
                            "right" => Some(39),
                            "down" => Some(40),
                            v => u16::from_str_radix(v.trim_start_matches("0x"), 16).ok(),
                        };
                        if let Some(vk) = vk {
                            match command {
                                "keydown" => {
                                    state.pending_input.push_back(InputEvent::KeyDown { vk })
                                }
                                "keyup" => state.pending_input.push_back(InputEvent::KeyUp { vk }),
                                _ => (),
                            }
                        }
                    }
                }
                consumed = lines.len();
            }
        }
        if input.is_none() && stage == 0 && elapsed > Duration::from_secs(8) {
            state
                .pending_input
                .push_back(InputEvent::KeyDown { vk: 13 });
            state.pending_input.push_back(InputEvent::KeyUp { vk: 13 });
            stage = 1;
        }
        if elapsed > Duration::from_secs(if input.is_some() { 1800 } else { 15 }) {
            stopped = true;
            FrameAction::Stop
        } else {
            FrameAction::Continue
        }
    })?;
    let state = &emu.process().unwrap().state;
    println!(
        "requested_stop={stopped} modules={} framebuffer={} active=0x{:08x}",
        state.modules.len(),
        state.framebuffer.frame_counter,
        state.active_window
    );
    std::fs::write(out, state.framebuffer.snapshot_ppm())?;
    if legacy {
        anyhow::ensure!(!stopped, "legacy failure did not reproduce");
    } else {
        anyhow::ensure!(
            stopped && state.modules.len() > 5,
            "startup still terminates before requested stop"
        );
    }
    Ok(())
}
