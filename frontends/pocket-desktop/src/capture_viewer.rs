//! Local capture playback. The worker owns all decoder/audio contexts; the UI
//! owns its texture. One replaceable frame slot bounds memory even while hidden.
use eframe::egui::{self, ColorImage, TextureHandle, Vec2};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

enum Command {
    #[cfg_attr(not(feature = "video-static"), allow(dead_code))]
    Seek(f64),
    Stop,
}
#[derive(Default)]
struct Snapshot {
    image: Option<ColorImage>,
    dimensions: [usize; 2],
    duration: f64,
    position: f64,
    error: Option<String>,
    ready: bool,
}
pub struct Viewer {
    title: String,
    command: mpsc::Sender<Command>,
    snapshot: Arc<Mutex<Snapshot>>,
    texture: Option<TextureHandle>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Viewer {
    pub fn open(path: PathBuf, video: bool, ctx: &egui::Context) -> Self {
        let title = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let worker = Arc::clone(&snapshot);
        let repaint = ctx.clone();
        let worker_thread = std::thread::spawn(move || {
            let result = if video {
                #[cfg(feature = "video-static")]
                {
                    play_video(&path, rx, &worker, &repaint)
                }
                #[cfg(not(feature = "video-static"))]
                {
                    let _ = rx;
                    Err("Video playback requires the video-static build feature.".to_owned())
                }
            } else {
                load_image(&path, &worker)
            };
            if let Err(error) = result {
                worker.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error);
            }
            repaint.request_repaint();
        });
        Self {
            title,
            command: tx,
            snapshot,
            texture: None,
            worker: Some(worker_thread),
        }
    }
    /// True keeps the viewer open. B/Escape/Close all stop the worker on drop.
    pub fn show(&mut self, ctx: &egui::Context, actions: &[String]) -> bool {
        if actions.iter().any(|a| a == "East") || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return false;
        }
        let (dimensions, duration, mut position, ready, error) = {
            let mut state = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(image) = state.image.take() {
                if let Some(texture) = &mut self.texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ctx.load_texture(
                        "capture-player-frame",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            (
                state.dimensions,
                state.duration,
                state.position,
                state.ready,
                state.error.clone(),
            )
        };
        let bounds = ctx.screen_rect().shrink(12.0);
        let native = Vec2::new(dimensions[0].max(1) as f32, dimensions[1].max(1) as f32);
        let scale = ((bounds.width() - 24.0) / native.x)
            .min((bounds.height() - 110.0) / native.y)
            .min(1.0)
            .max(0.01);
        let size = native * scale;
        let mut open = true;
        let mut close = false;
        let mut seek = None;
        for action in actions {
            match action.as_str() {
                "DPadLeft" | "LeftStickLeft" => seek = Some((position - 5.0).max(0.0)),
                "DPadRight" | "LeftStickRight" => seek = Some((position + 5.0).min(duration)),
                _ => {}
            }
        }
        egui::Window::new(&self.title)
            .id(egui::Id::new("integrated_capture_viewer"))
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .constrain_to(bounds)
            .collapsible(false)
            .fixed_size(Vec2::new(
                size.x.max(200.0).min((bounds.width() - 24.0).max(1.0)),
                size.y + 76.0,
            ))
            .show(ctx, |ui| {
                if let Some(texture) = &self.texture {
                    ui.image((texture.id(), size));
                } else if error.is_none() && !ready {
                    ui.spinner();
                    ui.label("Opening capture…");
                }
                if let Some(error) = error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                if duration > 0.0 {
                    let response =
                        ui.add(egui::Slider::new(&mut position, 0.0..=duration).show_value(false));
                    if response.changed() {
                        seek = Some(position);
                    }
                    ui.label(format!(
                        "{} / {}",
                        time_label(position),
                        time_label(duration)
                    ));
                }
                close = crate::controller_ui::button(ui, "Close (B)").clicked();
            });
        if let Some(seconds) = seek {
            let _ = self.command.send(Command::Seek(seconds));
        }
        if !ready || (duration > 0.0 && position < duration) {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        open && !close
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.command.send(Command::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn time_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
fn load_image(path: &std::path::Path, slot: &Mutex<Snapshot>) -> Result<(), String> {
    let image = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let dimensions = [image.width() as usize, image.height() as usize];
    let mut state = slot.lock().unwrap_or_else(|e| e.into_inner());
    state.image = Some(ColorImage::from_rgba_unmultiplied(
        dimensions,
        image.as_raw(),
    ));
    state.dimensions = dimensions;
    state.ready = true;
    Ok(())
}

#[cfg(feature = "video-static")]
fn play_video(
    path: &std::path::Path,
    commands: mpsc::Receiver<Command>,
    slot: &Mutex<Snapshot>,
    ctx: &egui::Context,
) -> Result<(), String> {
    use pocket_core::winceapi::media_static::Decoder;
    use std::time::Instant;
    let mut video = Decoder::open(path, true, 0, 0)?.ok_or("No video stream")?;
    let (duration, width, height, _, _) = video.info();
    if width == 0 || height == 0 {
        return Err("Invalid video dimensions".into());
    }
    let mut audio = Decoder::open(path, false, 0, 0)?;
    // SDL is already bundled for controllers. AudioQueue copies PCM and remains
    // on this worker; no host executable, file association or child process.
    // The controller worker owns Rust-SDL2's main-thread context. Initialize
    // only SDL's independent audio subsystem here, with balanced C refcounts.
    let queue = if audio.is_some() {
        Some(AudioOutput::open()?)
    } else {
        None
    };
    let mut offset = 0.0;
    let mut frame = video.next()?;
    let mut anchor = Instant::now();
    let mut video_end = frame.is_none();
    let mut audio_end = audio.is_none();
    let mut audio_trim = 0.0;
    let mut seeking = false;
    {
        let mut state = slot.lock().unwrap_or_else(|e| e.into_inner());
        state.dimensions = [width as usize, height as usize];
        state.duration = duration;
        state.ready = true;
    }
    loop {
        let mut target = None;
        loop {
            match commands.try_recv() {
                Ok(Command::Stop) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Ok(Command::Seek(seconds)) => target = Some(seconds.clamp(0.0, duration.max(0.0))),
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if let Some(target) = target {
            if let Some(queue) = &queue {
                queue.pause();
                queue.clear();
            }
            video.seek(target)?;
            if let Some(audio) = &mut audio {
                audio.seek(target)?;
            }
            offset = target;
            audio_trim = target;
            anchor = Instant::now();
            seeking = true;
            frame = video.next()?;
            video_end = frame.is_none();
            audio_end = audio.is_none();
        }
        // Bound seek preroll per iteration so a new drag/close cancels promptly.
        if seeking {
            for _ in 0..32 {
                if frame.as_ref().is_none_or(|(_, time)| *time >= offset) {
                    seeking = false;
                    break;
                }
                let next = video.next()?;
                if next.is_none() {
                    seeking = false;
                    video_end = true;
                    break;
                }
                frame = next;
            }
            if seeking {
                continue;
            }
            anchor = Instant::now();
        }
        // Keep only ~200ms PCM queued: bounded memory and immediate seek/close.
        if let (Some(decoder), Some(queue)) = (&mut audio, &queue) {
            for _ in 0..64 {
                if audio_end || queue.size() >= 35280 {
                    break;
                }
                match decoder.next()? {
                    Some((bytes, pts)) => {
                        let skip = (((audio_trim - pts).max(0.0) * 44100.0).ceil() as usize)
                            .saturating_mul(4)
                            .min(bytes.len());
                        let samples: Vec<i16> = bytes[skip..]
                            .chunks_exact(2)
                            .map(|s| i16::from_le_bytes([s[0], s[1]]))
                            .collect();
                        if !samples.is_empty() {
                            queue.queue_audio(&samples)?;
                        }
                    }
                    None => audio_end = true,
                }
            }
            queue.resume();
        }
        let position = offset + anchor.elapsed().as_secs_f64();
        // Decode on the worker and retain only the newest due frame if late.
        let mut due = None;
        for _ in 0..16 {
            if frame.as_ref().is_none_or(|(_, pts)| *pts > position) {
                break;
            }
            due = frame.take();
            frame = video.next()?;
            if frame.is_none() {
                video_end = true;
                break;
            }
        }
        let updated = due.is_some();
        {
            let mut state = slot.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((pixels, _)) = due {
                let expected = width as usize * height as usize * 2;
                if pixels.len() != expected {
                    return Err("Invalid decoded video frame".into());
                }
                let mut rgb = Vec::with_capacity(expected / 2 * 3);
                for p in pixels.chunks_exact(2) {
                    let v = u16::from_le_bytes([p[0], p[1]]);
                    let (r, g, b) = ((v >> 11) as u8, ((v >> 5) & 63) as u8, (v & 31) as u8);
                    rgb.extend_from_slice(&[
                        (r << 3) | (r >> 2),
                        (g << 2) | (g >> 4),
                        (b << 3) | (b >> 2),
                    ]);
                }
                state.image = Some(ColorImage::from_rgb(
                    [width as usize, height as usize],
                    &rgb,
                ));
            }
            state.position = if duration > 0.0 {
                position.min(duration)
            } else {
                position
            };
        }
        if updated {
            ctx.request_repaint();
        }
        // At EOF retain the last image, but continue accepting seek/B/Close.
        if video_end && frame.is_none() && audio_end && queue.as_ref().is_none_or(|q| q.size() == 0)
        {
            if duration > 0.0 {
                slot.lock().unwrap_or_else(|e| e.into_inner()).position = duration;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(feature = "video-static")]
struct AudioOutput(u32);
#[cfg(feature = "video-static")]
impl AudioOutput {
    fn open() -> Result<Self, String> {
        // SDL_AudioSpec is a C POD: zero is valid for every field, including
        // callback=None (queued audio). This device is owned by this worker.
        unsafe {
            if sdl2::sys::SDL_InitSubSystem(sdl2::sys::SDL_INIT_AUDIO) != 0 {
                return Err(sdl2::get_error());
            }
            let mut desired: sdl2::sys::SDL_AudioSpec = std::mem::zeroed();
            desired.freq = 44100;
            desired.format = sdl2::sys::AUDIO_S16SYS as u16;
            desired.channels = 2;
            desired.samples = 1024;
            let mut obtained = std::mem::zeroed();
            let device =
                sdl2::sys::SDL_OpenAudioDevice(std::ptr::null(), 0, &desired, &mut obtained, 0);
            if device == 0 {
                let error = sdl2::get_error();
                sdl2::sys::SDL_QuitSubSystem(sdl2::sys::SDL_INIT_AUDIO);
                return Err(error);
            }
            Ok(Self(device))
        }
    }
    fn pause(&self) {
        unsafe {
            sdl2::sys::SDL_PauseAudioDevice(self.0, 1);
        }
    }
    fn resume(&self) {
        unsafe {
            sdl2::sys::SDL_PauseAudioDevice(self.0, 0);
        }
    }
    fn clear(&self) {
        unsafe {
            sdl2::sys::SDL_ClearQueuedAudio(self.0);
        }
    }
    fn size(&self) -> u32 {
        unsafe { sdl2::sys::SDL_GetQueuedAudioSize(self.0) }
    }
    fn queue_audio(&self, samples: &[i16]) -> Result<(), String> {
        let length = samples
            .len()
            .checked_mul(2)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or("Audio block too large")?;
        if unsafe { sdl2::sys::SDL_QueueAudio(self.0, samples.as_ptr().cast(), length) } != 0 {
            Err(sdl2::get_error())
        } else {
            Ok(())
        }
    }
}
#[cfg(feature = "video-static")]
impl Drop for AudioOutput {
    fn drop(&mut self) {
        unsafe {
            sdl2::sys::SDL_CloseAudioDevice(self.0);
            sdl2::sys::SDL_QuitSubSystem(sdl2::sys::SDL_INIT_AUDIO);
        }
    }
}
