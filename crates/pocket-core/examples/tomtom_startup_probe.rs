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
        "\\SD Card\\",
        "\\Storage Card\\",
    ] {
        emu.mount_read_only_dir(prefix, &root);
    }
    emu.mount_read_only_dir("\\SD Card\\", root.parent().unwrap());
    emu.mount_read_only_dir("\\Storage Card\\", root.parent().unwrap());
    emu.mount_save_dir("\\Flash Disk\\", out.join("flash"));
    emu.set_module_path(&format!("\\SD Card\\TomTom\\{exe}"));
    emu.set_default_dir("\\SD Card\\TomTom\\");
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
        "",
    )
    .map_err(anyhow::Error::msg)?;
    if std::env::var_os("POCKETHLE_PROBE_TRACE").is_some() {
        emu.set_trace_sink(Box::new(Trace {
            file: std::io::BufWriter::new(std::fs::File::create(out.join("api.trace"))?),
            start,
        }));
    }
    let mut last = Instant::now();
    let mut stage = 0;
    let result = emu.run_with_hook(&mut |state: &mut KernelState| {
        let elapsed = start.elapsed();
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
    result
}
