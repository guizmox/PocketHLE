//! Headless, bounded startup profiling. No game-specific emulation changes.
use pocket_core::{
    kernel::{FrameAction, InputEvent, KernelState},
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
    emu.set_pocket_pc_input(true);
    for prefix in [
        "\\SD Card\\Game\\",
        "\\Storage Card\\Game\\",
        "\\Application\\",
        "\\Program Files\\Game\\",
        "\\SD Card\\",
        "\\Storage Card\\",
    ] {
        emu.mount_read_only_dir(prefix, &root);
    }
    emu.mount_save_dir("\\Flash Disk\\", out.join("flash"));
    emu.set_module_path(&format!("\\SD Card\\Game\\{exe}"));
    emu.set_default_dir("\\SD Card\\Game\\");
    emu.mount_save_dir("\\SD Card\\Game\\", out.join("save"));
    emu.max_slices = 0;
    emu.set_synthetic_message_budget(0);
    if std::env::var_os("POCKETHLE_PROBE_TRACE").is_some() {
        emu.set_trace_sink(Box::new(Trace {
            file: std::io::BufWriter::new(std::fs::File::create(out.join("api.trace"))?),
            start,
        }));
    }
    let mut last = Instant::now();
    let mut stage = 0;
    let mut consumed = 0;
    let mut release: Option<(Instant, InputEvent)> = None;
    let result = emu.run_with_hook(&mut |state: &mut KernelState| {
        if release
            .as_ref()
            .is_some_and(|(time, _)| time.elapsed() >= Duration::from_millis(150))
        {
            state.pending_input.push_back(release.take().unwrap().1);
        }
        if let Ok(input) = std::fs::read_to_string(out.join("input.txt")) {
            let lines: Vec<_> = input.lines().collect();
            for line in lines.iter().skip(consumed) {
                let fields: Vec<_> = line.split_whitespace().collect();
                match fields.as_slice() {
                    ["key", vk] => {
                        if let Ok(vk) = u16::from_str_radix(vk, 16) {
                            state.pending_input.push_back(InputEvent::KeyDown { vk });
                            release = Some((Instant::now(), InputEvent::KeyUp { vk }));
                        }
                    }
                    ["tap", x, y] => {
                        if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                            state
                                .pending_input
                                .push_back(InputEvent::PointerDown { x, y });
                            release = Some((Instant::now(), InputEvent::PointerUp { x, y }));
                        }
                    }
                    _ => (),
                }
            }
            consumed = lines.len();
        }

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
