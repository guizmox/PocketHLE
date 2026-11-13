//! Drive a Compact Framework session through the frontends' existing frames and input.

use crate::Session;
use pocket_kernel::accelerometer::{self, TiltAxes};
use pocket_kernel::{Framebuffer, InputEvent};
use std::path::Path;
use std::time::{Duration, Instant};

const INSTRUCTIONS_PER_CALLBACK: usize = 500_000;

#[derive(Default)]
struct Input {
    directions: [bool; 4],
    stick: [i16; 2],
    turns: usize,
}

impl Input {
    fn accept(&mut self, event: InputEvent) {
        match event {
            InputEvent::KeyDown { vk } | InputEvent::KeyUp { vk } => {
                if let Some(index) = vk.checked_sub(0x25).filter(|index| *index < 4) {
                    self.directions[usize::from(index)] =
                        matches!(event, InputEvent::KeyDown { .. });
                }
            }
            InputEvent::KeyboardRotation { turns } => self.turns = usize::from(turns % 4),
            InputEvent::TouchSteeringPosition { x, y } => self.stick = [x, y],
            _ => {}
        }
    }

    fn acceleration(&self, tilt: Option<TiltAxes>) -> [f32; 3] {
        let Some(axes) = tilt else {
            return accelerometer::sample().unwrap_or([0.0, 0.0, 9.8]);
        };
        let axis = |negative: bool, positive: bool, stick: i16| {
            if negative || positive {
                f32::from(i8::from(positive) - i8::from(negative))
            } else {
                let value = f32::from(stick.clamp(-1000, 1000));
                value.signum() * (value.abs() - 120.0).max(0.0) / 880.0
            }
        };
        let directions = [1, 2, 3, 0];
        let held = |direction: usize| self.directions[directions[(direction + 4 - self.turns) % 4]];
        let x = 0.5 * axis(held(3), held(1), self.stick[0]);
        let y = match axes {
            TiltAxes::Horizontal => 0.0,
            TiltAxes::Both => 0.5 * axis(held(2), held(0), -self.stick[1].clamp(-1000, 1000)),
        };
        [x * 9.8, y * 9.8, (1.0 - x * x - y * y).sqrt() * 9.8]
    }
}

pub struct Options {
    pub size: (u32, u32),
    pub save_root: Option<std::path::PathBuf>,
    pub guest_path: Option<String>,
    pub tilt: Option<TiltAxes>,
    pub frame_rate_limit: Option<u32>,
}

pub fn run(
    executable: &Path,
    options: Options,
    mut poll: impl FnMut(&mut Vec<InputEvent>) -> bool,
    mut present: impl FnMut(&Framebuffer),
) -> Result<(), String> {
    use std::io::Read;
    const MAX_IMAGE: u64 = 32 * 1024 * 1024;
    let file =
        std::fs::File::open(executable).map_err(|error| format!("Open managed image: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Read managed image: {error}"))?;
    if bytes.len() as u64 > MAX_IMAGE {
        return Err("Managed image exceeds the 32 MiB limit".into());
    }
    let mut session = Session::start_in(
        &bytes,
        options.size,
        INSTRUCTIONS_PER_CALLBACK,
        options.save_root,
        options.guest_path,
    )
    .map_err(|error| format!("Managed startup: {error:?}"))?;
    let mut framebuffer = Framebuffer::new(options.size.0, options.size.1);
    let mut generation = 0;
    let mut input = Input::default();
    let mut events = Vec::new();
    let start = Instant::now();
    let mut last_present = None;
    let interval = options
        .frame_rate_limit
        .filter(|fps| *fps > 0)
        .map(|fps| Duration::from_secs_f64(1.0 / f64::from(fps)));
    while session.running() && poll(&mut events) {
        for event in events.drain(..) {
            if let InputEvent::PointerUp { x, y } = event {
                session
                    .click(x, y, INSTRUCTIONS_PER_CALLBACK)
                    .map_err(|error| format!("Managed input: {error:?}"))?;
            } else {
                input.accept(event);
            }
        }
        session.set_acceleration(input.acceleration(options.tilt));
        let now = Instant::now();
        session
            .tick(
                now.duration_since(start).as_millis() as u64,
                INSTRUCTIONS_PER_CALLBACK,
            )
            .map_err(|error| format!("Managed event: {error:?}"))?;
        if last_present
            .zip(interval)
            .is_none_or(|(last, interval)| now.duration_since(last) >= interval)
        {
            if let Some((next, size, pixels)) = session.frame() {
                if next != generation && size == options.size {
                    framebuffer.pixels.copy_from_slice(pixels);
                    session.paint_controls(&mut framebuffer);
                    framebuffer.mark_dirty();
                    framebuffer.presentation_stats.record_present();
                    present(&framebuffer);
                    last_present = Some(now);
                    generation = next;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    session
        .close(INSTRUCTIONS_PER_CALLBACK)
        .map_err(|error| format!("Managed shutdown: {error:?}"))
}
