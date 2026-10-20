//! Headless, bounded startup profiling. No game-specific emulation changes.
use pocket_core::{
    kernel::{FrameAction, KernelState},
    Emulator,
};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
struct Trace {
    file: std::io::BufWriter<std::fs::File>,
    start: Instant,
}
impl Write for Trace {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        write!(self.file, "{} ", self.start.elapsed().as_micros())?;
        self.file.write_all(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
struct Logger;
impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record<'_>) {
        if self.enabled(r.metadata()) {
            eprintln!("{} {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}
fn main() -> anyhow::Result<()> {
    log::set_logger(&Logger).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let a: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(a.get(1).expect("extracted game directory")).canonicalize()?;
    let exe = a.get(2).expect("executable filename");
    let out = PathBuf::from(a.get(3).expect("output directory"));
    std::fs::create_dir_all(&out)?;
    let limit: u64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(30);
    let start = Instant::now();
    let mut emu = Emulator::with_unicorn_cpu()?;
    eprintln!("LOAD_BEGIN {:?}", start.elapsed());
    emu.load_pe(root.join(exe))?;
    eprintln!("LOAD_END {:?}", start.elapsed());
    emu.set_screen_size(320, 240);
    emu.set_memory_division(Some(
        pocket_core::kernel::memory_division::MemoryDivision::gizmondo_sdk_default(),
    ));
    for prefix in [
        "\\SD Card\\TomTom\\",
        "\\Storage Card\\TomTom\\",
        "\\Application\\",
        "\\Program Files\\Game\\",
    ] {
        let host = if std::env::var_os("POCKETHLE_CARD_ROOT").is_some() && prefix.contains("TomTom")
        {
            root.join("TomTom")
        } else {
            root.clone()
        };
        emu.mount_read_only_dir(prefix, host);
    }
    emu.mount_read_only_dir(
        "\\SD Card\\",
        std::env::var_os("POCKETHLE_CARD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.parent().unwrap().to_path_buf()),
    );
    emu.mount_read_only_dir(
        "\\Storage Card\\",
        std::env::var_os("POCKETHLE_CARD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.parent().unwrap().to_path_buf()),
    );
    emu.mount_save_dir("\\Flash Disk\\", out.join("flash"));
    if let Some(fixture) = std::env::var_os("POCKETHLE_MAP_FIXTURE") {
        emu.mount_read_only_dir("\\SD Card\\France 7.20\\", PathBuf::from(fixture));
    }
    emu.set_module_path(&format!("\\SD Card\\TomTom\\{exe}"));
    emu.set_default_dir("\\SD Card\\TomTom\\");
    emu.process_mut().unwrap().state.process_launch_enabled = true;
    emu.max_slices = 0;
    emu.set_synthetic_message_budget(0);
    emu.process_mut()
        .unwrap()
        .state
        .registry
        .configure_persistence(&out.join("device-registry.json"))?;
    pocket_core::kernel::device_identity::DeviceIdentity::configure(
        &mut emu.process_mut().unwrap().state.registry,
        "470069007A006D006F006E0064006F000000",
        "AD0D71F3E65E5B9A47E4773478FA926B",
    )
    .map_err(anyhow::Error::msg)?;
    if std::env::var_os("POCKETHLE_PROBE_TRACE").is_some() {
        emu.set_trace_sink(Box::new(Trace {
            file: std::io::BufWriter::new(std::fs::File::create(out.join("api.trace"))?),
            start,
        }));
    }
    if std::env::var_os("POCKETHLE_TOMTOM_DIAG").is_some() {
        emu.add_register_trace(0x2167b8, "map-validator-entry")?;
        emu.add_register_trace(0x216fe8, "map-validator-result")?;
        emu.add_register_trace(0x26b304, "pna-search-entry")?;
    }
    if std::env::var_os("POCKETHLE_DISABLE_FLASHCARD_API").is_some() {
        emu.process_mut()
            .unwrap()
            .state
            .dynamic_exports
            .remove(&pocket_core::kernel::NOTE_PRJ_MODULE_HANDLE);
    }
    let mut last = Instant::now();
    let mut stage = 0;
    let mut clicked = false;
    let result = emu.run_with_hook(&mut |state: &mut KernelState| {
        let elapsed = start.elapsed();
        if let Some(request) = state.pending_process_launch.as_ref() {
            eprintln!("LAUNCH_REQUEST {request:?}");
            return FrameAction::Stop;
        }
        if !clicked
            && elapsed >= Duration::from_secs(4)
            && std::env::var_os("POCKETHLE_PROBE_CLICK_DONE").is_some()
        {
            clicked = true;
            if let Ok(v) = std::env::var("PROBE_KEY") {
                let vk = v.parse().unwrap();
                state
                    .pending_input
                    .push_back(pocket_core::kernel::InputEvent::KeyDown { vk });
                state
                    .pending_input
                    .push_back(pocket_core::kernel::InputEvent::KeyUp { vk });
            }
            state
                .pending_input
                .push_back(pocket_core::kernel::InputEvent::PointerDown {
                    x: std::env::var("PROBE_X")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(265),
                    y: std::env::var("PROBE_Y")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(219),
                });
            state
                .pending_input
                .push_back(pocket_core::kernel::InputEvent::PointerUp {
                    x: std::env::var("PROBE_X")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(265),
                    y: std::env::var("PROBE_Y")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(219),
                });
            eprintln!(
                "MAPDIAG CLICK DONE windows={:?} active={:x} wndproc={:x} procs={:?}",
                state.window_rects, state.active_window, state.wnd_proc, state.window_procs
            );
        }
        if last.elapsed() >= Duration::from_secs(2) {
            stage += 1;
            last = Instant::now();
            eprintln!(
                "PROGRESS {:?} frame={} windows={} threads={}",
                elapsed,
                state.framebuffer.frame_counter,
                state.window_rects.len(),
                state.threads.len()
            );
            eprintln!(
                "THREADS current={} main_exit={:?} owners={:?} workers={:?}",
                state.current_thread,
                state.main_thread.exit_code,
                state.window_threads,
                state
                    .threads
                    .iter()
                    .map(|t| (t.id, t.finished, t.worker_regs[15], t.messages.len()))
                    .collect::<Vec<_>>()
            );
            let _ = std::fs::write(
                out.join(format!("frame-{stage:02}.ppm")),
                state.framebuffer.snapshot_ppm(),
            );
        }
        if elapsed >= Duration::from_secs(limit) {
            FrameAction::Stop
        } else {
            FrameAction::Continue
        }
    });
    eprintln!("END {:?} result={result:?}", start.elapsed());
    let state = &emu.process().unwrap().state;
    eprintln!(
        "SURFACES rects={:?} visible={:?} dcs={:?}",
        state.window_rects, state.window_visibility, state.window_dcs
    );
    for (&hwnd, &dc) in &state.window_dcs {
        if let Some(bm) = state
            .gdi
            .dc(dc)
            .and_then(|d| d.selected_bitmap)
            .and_then(|b| state.gdi.bitmap(b))
        {
            let mut ppm = format!("P6\n{} {}\n255\n", bm.width, bm.height).into_bytes();
            for p in bm.pixels.chunks_exact(2) {
                let c = u16::from_le_bytes([p[0], p[1]]);
                ppm.extend_from_slice(&[
                    ((c >> 11) * 255 / 31) as u8,
                    (((c >> 5) & 63) * 255 / 63) as u8,
                    ((c & 31) * 255 / 31) as u8,
                ]);
            }
            std::fs::write(out.join(format!("surface-{hwnd:08x}.ppm")), ppm)?;
        }
    }

    if std::env::var_os("POCKETHLE_TOMTOM_DIAG").is_some() {
        let mut memory = vec![0u8; 0x1000000];
        for addr in (0x10000..0x1000000u32).step_by(4096) {
            if let Ok(data) = emu.read_guest_memory(addr, 4096) {
                memory[addr as usize..addr as usize + 4096].copy_from_slice(&data);
            }
        }
        std::fs::write(out.join("guest-low-16m.bin"), memory)?;
    }
    result
}
