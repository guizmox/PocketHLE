//! Display-resolution MJPEG/PCM capture. Encoding and disk I/O never run in GL/audio callbacks.
use eframe::egui;
use pocket_core::kernel::{audio::OutputRecording, InputEvent};
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
const FPS: u64 = 30;
const RATE: u64 = 48_000;
const AUDIO_PER_FRAME: usize = 1600;
#[derive(Default)]
pub struct Recording {
    session: Option<Session>,
    worker: Option<std::thread::JoinHandle<()>>,
    result: Option<Receiver<Result<Vec<PathBuf>, String>>>,
    held_keys: Vec<u16>,
    pub label: Option<(String, Instant)>,
    pub direction: Option<(u16, Instant)>,
    pub lookup: Option<(String, Instant)>,
    pub point: Option<((u16, u16), Instant)>,
}
struct Session {
    tx: SyncSender<Frame>,
    audio: Arc<OutputRecording>,
    stop: Arc<Mutex<Option<Instant>>>,
    next: Instant,
}
struct Frame {
    at: Instant,
    pixels: image::RgbaImage,
    overlay: Overlay,
}
#[derive(Default)]
struct Overlay {
    direction: Option<u16>,
    button: Option<String>,
    lookup: Option<String>,
    point: Option<(f32, f32)>,
}
pub type SharedRecording = Arc<Mutex<Recording>>;
impl Recording {
    pub fn active(&self) -> bool {
        self.session.is_some()
    }
    pub fn busy(&self) -> bool {
        self.result.is_some()
    }
    pub fn start(
        &mut self,
        path: PathBuf,
        quality: &str,
        compressed: bool,
    ) -> Result<Arc<OutputRecording>, String> {
        if self.busy() {
            return Err("A recording is still being saved".into());
        }
        if compressed && !pocket_core::winceapi::recording_encoder::available() {
            return Err("MP4 requires FFmpeg with MPEG-4/AAC encoders and MP4 muxer enabled. Choose AVI or rebuild FFmpeg with --enable-encoder=mpeg4,aac --enable-muxer=mp4".into());
        }
        let quality = match quality {
            "low" => 50,
            "high" => 92,
            _ => 75,
        };
        let (tx, rx) = mpsc::sync_channel(3);
        let (done, ready) = mpsc::channel();
        let audio = Arc::new(OutputRecording::default());
        let worker_audio = audio.clone();
        let stop = Arc::new(Mutex::new(None));
        let worker_stop = stop.clone();
        let start = Instant::now();
        let worker = std::thread::Builder::new()
            .name("video-recorder".into())
            .spawn(move || {
                let result = encode(
                    rx,
                    worker_audio,
                    worker_stop,
                    path,
                    quality,
                    start,
                    compressed,
                )
                .map_err(|e| e.to_string());
                let _ = done.send(result);
            })
            .map_err(|e| e.to_string())?;
        self.worker = Some(worker);
        self.session = Some(Session {
            tx,
            audio: audio.clone(),
            stop,
            next: start,
        });
        self.result = Some(ready);
        self.held_keys.clear();
        self.label = None;
        self.direction = None;
        self.lookup = None;
        self.point = None;
        Ok(audio)
    }
    pub fn finish_and_wait(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
    pub fn stop(&mut self) {
        if let Some(s) = self.session.take() {
            *s.stop.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
            s.audio
                .stopped
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    pub fn poll(&mut self) -> Option<Result<Vec<PathBuf>, String>> {
        let result = self.result.as_ref()?.try_recv();
        match result {
            Ok(r) => {
                self.stop();
                self.result = None;
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
                Some(r)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.stop();
                self.result = None;
                Some(Err("Video encoder stopped unexpectedly".into()))
            }
            _ => None,
        }
    }
    pub fn observe(&mut self, event: InputEvent) {
        if !self.active() {
            return;
        }
        let now = Instant::now();
        match event {
            InputEvent::KeyDown { vk } => {
                // Repeated KeyDown must not change the priority of held keys.
                if !self.held_keys.contains(&vk) {
                    self.held_keys.push(vk);
                }
                self.refresh_keys(now);
            }
            InputEvent::KeyUp { vk } => {
                self.held_keys.retain(|key| *key != vk);
                self.refresh_keys(now);
            }
            InputEvent::StylusAim { x, y } => self.aim(i32::from(x), i32::from(y)),
            InputEvent::PointerDown { x, y } => {
                self.point = Some(((x, y), now));
            }
            InputEvent::PointerMove { x, y } if self.point.is_some() => {
                self.point = Some(((x, y), now));
            }
            InputEvent::PointerUp { .. } => {
                self.point = None;
            }
            _ => {}
        }
    }
    fn refresh_keys(&mut self, now: Instant) {
        self.direction = self.held_keys.iter().rev().copied()
            .find(|vk| (0x25..=0x28).contains(vk)).map(|vk| (vk, now));
        self.label = self.held_keys.iter().rev().copied()
            .find(|vk| !(0x25..=0x28).contains(vk)).map(|vk| (key_label(vk), now));
    }
    pub fn aim(&mut self, x: i32, y: i32) {
        if !self.active() { return; }
        if x == 0 && y == 0 {
            self.lookup = None;
            return;
        }
        let mut words = vec!["lookup"];
        if x < 0 {
            words.push("left")
        }
        if x > 0 {
            words.push("right")
        }
        if y < 0 {
            words.push("up")
        }
        if y > 0 {
            words.push("down")
        }
        self.lookup = Some((words.join(" "), Instant::now()));
    }
    fn overlay(&self, size: (u32, u32), turns: u8) -> Overlay {
        let point = self
            .point
            .map(|((x, y), _)| {
                let (u, v) = (
                    f32::from(x) / size.0.max(1) as f32,
                    f32::from(y) / size.1.max(1) as f32,
                );
                match turns % 4 {
                    1 => (1.0 - v, u),
                    2 => (1.0 - u, 1.0 - v),
                    3 => (v, 1.0 - u),
                    _ => (u, v),
                }
            });
        Overlay {
            direction: self
                .direction
                .map(|(v, _)| v),
            button: self
                .label
                .as_ref()
                .map(|(v, _)| v.clone()),
            lookup: self
                .lookup
                .as_ref()
                .map(|(v, _)| v.clone()),
            point,
        }
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        self.finish_and_wait();
    }
}
fn key_label(vk: u16) -> String {
    match vk {
        0x0d => "ACT".into(),
        0x1b => "Esc".into(),
        9 => "Tab".into(),
        0x20 => "B".into(),
        0x11 => "A".into(),
        0x10 => "C".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0xc1..=0xc4 => format!("P{}", vk - 0xc0),
        0x30..=0x5a => char::from_u32(vk as u32).unwrap().to_string(),
        _ => format!("Key {vk:02X}"),
    }
}
/// Ordered after the filter and egui input markers, before surrounding launcher UI.
pub fn capture(
    gl: &eframe::glow::Context,
    info: egui::PaintCallbackInfo,
    state: &SharedRecording,
    size: (u32, u32),
    turns: u8,
) {
    let now = Instant::now();
    let mut r = state.lock().unwrap_or_else(|e| e.into_inner());
    let overlay = r.overlay(size, turns);
    let Some(s) = r.session.as_mut() else {
        return;
    };
    if now < s.next {
        return;
    }
    s.next = now + Duration::from_secs_f64(1.0 / FPS as f64);
    match crate::screenshot::read_game_pixels(gl, info) {
        Ok(pixels) => {
            let _ = s.tx.try_send(Frame {
                at: now,
                pixels,
                overlay,
            });
        }
        Err(_) => {
            r.stop();
        }
    }
}
fn blend(im: &mut image::RgbaImage, x: i32, y: i32, color: [u8; 3], alpha: u32) {
    if x < 0 || y < 0 || x >= im.width() as i32 || y >= im.height() as i32 {
        return;
    }
    let p = im.get_pixel_mut(x as u32, y as u32);
    for c in 0..3 {
        p[c] = ((u32::from(p[c]) * (255 - alpha) + u32::from(color[c]) * alpha) / 255) as u8;
    }
}
fn circle(im: &mut image::RgbaImage, x: i32, y: i32, r: i32) {
    for dy in -r..=r {
        for dx in -r..=r {
            let d = dx * dx + dy * dy;
            if d <= r * r {
                if d >= (r - 2) * (r - 2) {
                    blend(im, x + dx, y + dy, [90, 195, 255], 210)
                } else {
                    blend(im, x + dx, y + dy, [8, 18, 32], 145)
                }
            }
        }
    }
}
fn line(im: &mut image::RgbaImage, a: (i32, i32), b: (i32, i32)) {
    let n = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
    for i in 0..=n {
        blend(
            im,
            a.0 + (b.0 - a.0) * i / n,
            a.1 + (b.1 - a.1) * i / n,
            [255; 3],
            235,
        );
    }
}
fn draw_overlay(im: &mut image::RgbaImage, o: &Overlay) {
    let scale = (im.width().min(im.height()) / 480).clamp(1, 2) as i32;
    let r = 14 * scale;
    let y = im.height() as i32 - r - 5 * scale;
    let x = r + 5 * scale;
    if let Some(vk) = o.direction {
        circle(im, x, y, r);
        let (dx, dy) = match vk {
            0x25 => (-5, 0),
            0x26 => (0, -5),
            0x27 => (5, 0),
            _ => (0, 5),
        };
        let (dx, dy) = (dx * scale, dy * scale);
        line(im, (x - dx, y - dy), (x + dx, y + dy));
        let tip = (x + dx, y + dy);
        line(im, tip, (x + dy / 2, y + dx / 2));
        line(im, tip, (x - dy / 2, y - dx / 2));
    }
    if let Some(text) = &o.button {
        let x = x + 2 * r + 5 * scale;
        circle(im, x, y, r);
        let label: String = text.chars().take(5).collect();
        let w = (label.len() as u32 * 6).max(1);
        let mut bitmap = pocket_core::kernel::gdi::Bitmap::new(w, 8);
        pocket_core::kernel::font::draw_str(
            &mut pocket_core::kernel::gdi::Surface::Bitmap(&mut bitmap),
            0,
            0,
            &label,
            0xffff,
        );
        for yy in 0..8i32 {
            for xx in 0..w as i32 {
                let p = ((yy as u32 * w + xx as u32) * 2) as usize;
                if bitmap.pixels[p] != 0 || bitmap.pixels[p + 1] != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            blend(
                                im,
                                x - w as i32 * scale / 2 + xx * scale + sx,
                                y - 4 * scale + yy * scale + sy,
                                [255; 3],
                                235,
                            );
                        }
                    }
                }
            }
        }
    }
    if let Some(text) = &o.lookup {
        let x = im.width() as i32 - r - 5 * scale;
        circle(im, x, y, r);
        let dx = if text.contains("left") {
            -5
        } else if text.contains("right") {
            5
        } else {
            0
        };
        let dy = if text.contains("up") {
            -5
        } else if text.contains("down") {
            5
        } else {
            0
        };
        line(im, (x, y), (x + dx * scale, y + dy * scale));
        circle(im, x + dx * scale, y + dy * scale, 3 * scale);
    }
    if let Some((u, v)) = o.point {
        let (x, y) = (
            (u * im.width() as f32) as i32,
            (v * im.height() as f32) as i32,
        );
        for dy in -5 * scale..=5 * scale {
            for dx in -5 * scale..=5 * scale {
                if dx * dx + dy * dy <= 25 * scale * scale {
                    blend(im, x + dx, y + dy, [255, 35, 35], 220);
                }
            }
        }
    }
}

struct AudioTimeline {
    chunks: VecDeque<(u64, Vec<[i16; 2]>)>,
    next: Option<u64>,
    rate: u32,
    remainder: u64,
    last: Option<Instant>,
}
impl AudioTimeline {
    fn new() -> Self {
        Self {
            chunks: VecDeque::new(),
            next: None,
            rate: 0,
            remainder: 0,
            last: None,
        }
    }
    fn collect(&mut self, audio: &OutputRecording, epoch: Instant) -> io::Result<()> {
        if audio.overflow.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(io::Error::other(
                "Audio recorder could not keep up; recording stopped",
            ));
        }
        for chunk in audio.drain() {
            let channels = chunk.channels.max(1) as usize;
            let frames = chunk.samples.len() / channels;
            if frames == 0 || chunk.rate == 0 {
                continue;
            }
            if self.rate != chunk.rate
                || self.last.is_some_and(|last| {
                    chunk.at.saturating_duration_since(last) > Duration::from_millis(250)
                })
            {
                self.next = None;
                self.remainder = 0;
            }
            self.rate = chunk.rate;
            self.last = Some(chunk.at);
            let start = self.next.unwrap_or_else(|| {
                (chunk.at.saturating_duration_since(epoch).as_secs_f64() * RATE as f64) as u64
            });
            let count = (frames as u64 * RATE + self.remainder) / chunk.rate as u64;
            self.remainder = (frames as u64 * RATE + self.remainder) % chunk.rate as u64;
            let mut samples = Vec::with_capacity(count as usize);
            for i in 0..count {
                let p = (i as f64 * frames as f64 / count.max(1) as f64).min((frames - 1) as f64);
                let j = p as usize;
                let k = (j + 1).min(frames - 1);
                let t = (p - j as f64) as f32;
                let mut pair = [0; 2];
                for c in 0..2 {
                    let c = c.min(channels - 1);
                    let v = chunk.samples[j * channels + c] * (1.0 - t)
                        + chunk.samples[k * channels + c] * t;
                    pair[c.min(1)] = (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                }
                if channels == 1 {
                    pair[1] = pair[0];
                }
                samples.push(pair);
            }
            self.next = Some(start + count);
            self.chunks.push_back((start, samples));
        }
        Ok(())
    }
    fn ready_frame(&mut self, audio: &OutputRecording, epoch: Instant, start: u64) -> io::Result<Vec<u8>> {
        let end = start + AUDIO_PER_FRAME as u64;
        let deadline = epoch + Duration::from_secs_f64(end as f64 / RATE as f64)
            + Duration::from_millis(100);
        loop {
            self.collect(audio, epoch)?;
            if self.next.is_some_and(|next| next >= end)
                || audio.stopped.load(std::sync::atomic::Ordering::Relaxed)
                || Instant::now() >= deadline
            {
                return Ok(self.frame(start));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn frame(&mut self, start: u64) -> Vec<u8> {
        let mut out = vec![0; AUDIO_PER_FRAME * 4];
        for i in 0..AUDIO_PER_FRAME {
            let at = start + i as u64;
            while self
                .chunks
                .front()
                .is_some_and(|(s, v)| s + v.len() as u64 <= at)
            {
                self.chunks.pop_front();
            }
            if let Some((s, v)) = self.chunks.front() {
                if at >= *s {
                    if let Some(pair) = v.get((at - *s) as usize) {
                        out[i * 4..i * 4 + 2].copy_from_slice(&pair[0].to_le_bytes());
                        out[i * 4 + 2..i * 4 + 4].copy_from_slice(&pair[1].to_le_bytes());
                    }
                }
            }
        }
        out
    }
}
enum Clip {
    Avi(Avi),
    Mp4 {
        encoder: pocket_core::winceapi::recording_encoder::Mp4Encoder,
        width: u32,
        height: u32,
    },
}
struct EncodedFrame {
    pixels: image::RgbaImage,
    jpeg: Vec<u8>,
}
impl Clip {
    fn new(path: &Path, w: u32, h: u32, quality: u8, compressed: bool) -> io::Result<Self> {
        if compressed {
            let q = if quality < 60 {
                0
            } else if quality > 85 {
                2
            } else {
                1
            };
            Ok(Self::Mp4 {
                encoder: pocket_core::winceapi::recording_encoder::Mp4Encoder::open(
                    path, w, h, FPS as u32, q,
                )?,
                width: w,
                height: h,
            })
        } else {
            Ok(Self::Avi(Avi::new(path, w, h)?))
        }
    }
    fn rollover(&self, w: u32, h: u32) -> bool {
        match self {
            Self::Avi(a) => a.width != w || a.height != h || a.position > 1_800_000_000,
            Self::Mp4 { width, height, .. } => *width != w || *height != h,
        }
    }
    fn frame(&mut self, video: &EncodedFrame, audio: &[u8]) -> io::Result<()> {
        match self {
            Self::Avi(a) => a.frame(&video.jpeg, audio),
            Self::Mp4 { encoder, .. } => encoder.frame(video.pixels.as_raw(), audio),
        }
    }
    fn finish(&mut self) -> io::Result<()> {
        match self {
            Self::Avi(a) => a.finish(),
            Self::Mp4 { encoder, .. } => encoder.finish(),
        }
    }
}
fn encode(
    rx: Receiver<Frame>,
    audio: Arc<OutputRecording>,
    stop: Arc<Mutex<Option<Instant>>>,
    path: PathBuf,
    quality: u8,
    epoch: Instant,
    compressed: bool,
) -> io::Result<Vec<PathBuf>> {
    let mut timeline = AudioTimeline::new();
    let mut clip: Option<Clip> = None;
    let mut paths = Vec::new();
    let mut last: Option<EncodedFrame> = None;
    let mut global = 0u64;
    while let Ok(mut frame) = rx.recv() {
        timeline.collect(&audio, epoch)?;
        let (w, h) = frame.pixels.dimensions();
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 33_554_432 {
            return Err(io::Error::other("Video dimensions exceed capture limit"));
        }
        let target = (frame.at.saturating_duration_since(epoch).as_secs_f64() * FPS as f64) as u64;
        if clip.as_ref().is_some_and(|c| c.rollover(w, h)) {
            if let Some(previous) = last.as_ref() {
                while global < target {
                    clip.as_mut()
                        .unwrap()
                        .frame(previous, &timeline.ready_frame(&audio, epoch, global * AUDIO_PER_FRAME as u64)?)?;
                    global += 1;
                }
            }

            if let Some(mut c) = clip.take() {
                c.finish()?;
            }
            last = None;
        }
        if clip.is_none() {
            let output = segment_path(&path, paths.len());
            clip = Some(Clip::new(&output, w, h, quality, compressed)?);
            paths.push(output);
        }
        let target = (frame.at.saturating_duration_since(epoch).as_secs_f64() * FPS as f64) as u64;
        draw_overlay(&mut frame.pixels, &frame.overlay);
        let mut jpeg = Vec::new();
        if !compressed {
            let rgb = image::DynamicImage::ImageRgba8(frame.pixels.clone()).into_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
                .encode_image(&rgb)
                .map_err(io::Error::other)?;
        }
        let current = EncodedFrame {
            pixels: frame.pixels,
            jpeg,
        };
        while global < target {
            clip.as_mut().unwrap().frame(
                last.as_ref().unwrap_or(&current),
                &timeline.ready_frame(&audio, epoch, global * AUDIO_PER_FRAME as u64)?,
            )?;
            global += 1;
        }
        clip.as_mut()
            .unwrap()
            .frame(&current, &timeline.ready_frame(&audio, epoch, global * AUDIO_PER_FRAME as u64)?)?;
        global += 1;
        last = Some(current);
    }
    timeline.collect(&audio, epoch)?;
    let end = stop
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .unwrap_or_else(Instant::now);
    let target = (end.saturating_duration_since(epoch).as_secs_f64() * FPS as f64).ceil() as u64;
    if let Some(mut c) = clip {
        if let Some(last) = last {
            while global < target {
                c.frame(&last, &timeline.ready_frame(&audio, epoch, global * AUDIO_PER_FRAME as u64)?)?;
                global += 1;
            }
        }
        c.finish()?;
    }
    if paths.is_empty() {
        return Err(io::Error::other("No visible game frames were recorded"));
    }
    Ok(paths)
}
fn segment_path(base: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return base.to_owned();
    }
    base.with_file_name(format!(
        "{}_part{:03}.{}",
        base.file_stem().unwrap_or_default().to_string_lossy(),
        index + 1,
        base.extension().unwrap_or_default().to_string_lossy()
    ))
}
struct Avi {
    file: std::io::BufWriter<File>,
    width: u32,
    height: u32,
    position: u64,
    movi: u64,
    frames: u32,
    total: u64,
    video_len: u64,
    audio_len: u64,
    index: Vec<([u8; 4], u32, u32)>,
    finished: bool,
}
impl Avi {
    fn bytes(&mut self, b: &[u8]) -> io::Result<()> {
        self.file.write_all(b)?;
        self.position += b.len() as u64;
        Ok(())
    }
    fn u32(&mut self, n: u32) -> io::Result<()> {
        self.bytes(&n.to_le_bytes())
    }
    fn patch(&mut self, p: u64, n: u32) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(p))?;
        self.file.write_all(&n.to_le_bytes())?;
        self.file.seek(SeekFrom::Start(self.position))?;
        Ok(())
    }
    fn list(&mut self, kind: &[u8; 4]) -> io::Result<u64> {
        self.bytes(b"LIST")?;
        let at = self.position;
        self.u32(0)?;
        self.bytes(kind)?;
        Ok(at)
    }
    fn end_list(&mut self, at: u64) -> io::Result<()> {
        self.patch(at, (self.position - at - 4) as u32)
    }
    fn new(path: &Path, width: u32, height: u32) -> io::Result<Self> {
        let file = File::options().write(true).create_new(true).open(path)?;
        let mut a = Self {
            file: std::io::BufWriter::new(file),
            width,
            height,
            position: 0,
            movi: 0,
            frames: 0,
            total: 0,
            video_len: 0,
            audio_len: 0,
            index: Vec::new(),
            finished: false,
        };
        a.bytes(b"RIFF")?;
        a.u32(0)?;
        a.bytes(b"AVI ")?;
        let hdrl = a.list(b"hdrl")?;
        a.bytes(b"avih")?;
        a.u32(56)?;
        for n in [1_000_000 / FPS as u32, 0, 0, 0x10] {
            a.u32(n)?;
        }
        a.total = a.position;
        a.u32(0)?;
        for n in [0, 2, width * height * 3, width, height, 0, 0, 0, 0] {
            a.u32(n)?;
        }
        let video = a.list(b"strl")?;
        a.bytes(b"strh")?;
        a.u32(56)?;
        a.bytes(b"vidsMJPG")?;
        for n in [0, 0, 0, 1, FPS as u32, 0] {
            a.u32(n)?;
        }
        a.video_len = a.position;
        a.u32(0)?;
        for n in [width * height * 3, u32::MAX, 0] {
            a.u32(n)?;
        }
        for v in [0i16, 0, width as i16, height as i16] {
            a.bytes(&v.to_le_bytes())?;
        }
        a.bytes(b"strf")?;
        a.u32(40)?;
        a.u32(40)?;
        a.u32(width)?;
        a.u32(height)?;
        a.bytes(&[1, 0, 24, 0])?;
        a.bytes(b"MJPG")?;
        for n in [width * height * 3, 0, 0, 0, 0] {
            a.u32(n)?;
        }
        a.end_list(video)?;
        let sound = a.list(b"strl")?;
        a.bytes(b"strh")?;
        a.u32(56)?;
        a.bytes(b"auds\0\0\0\0")?;
        for n in [0, 0, 0, 4, RATE as u32 * 4, 0] {
            a.u32(n)?;
        }
        a.audio_len = a.position;
        a.u32(0)?;
        for n in [AUDIO_PER_FRAME as u32 * 4, u32::MAX, 4, 0, 0] {
            a.u32(n)?;
        }
        a.bytes(b"strf")?;
        a.u32(16)?;
        a.bytes(&[1, 0, 2, 0])?;
        a.u32(RATE as u32)?;
        a.u32(RATE as u32 * 4)?;
        a.bytes(&[4, 0, 16, 0])?;
        a.end_list(sound)?;
        a.end_list(hdrl)?;
        a.movi = a.list(b"movi")?;
        Ok(a)
    }
    fn chunk(&mut self, id: [u8; 4], data: &[u8]) -> io::Result<()> {
        let offset = (self.position - (self.movi + 4)) as u32;
        self.bytes(&id)?;
        self.u32(data.len() as u32)?;
        self.bytes(data)?;
        if data.len() % 2 != 0 {
            self.bytes(&[0])?;
        }
        self.index.push((id, offset, data.len() as u32));
        Ok(())
    }
    fn frame(&mut self, jpeg: &[u8], audio: &[u8]) -> io::Result<()> {
        self.chunk(*b"00dc", jpeg)?;
        self.chunk(*b"01wb", audio)?;
        self.frames += 1;
        Ok(())
    }
    fn finish(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        self.end_list(self.movi)?;
        self.bytes(b"idx1")?;
        self.u32(self.index.len() as u32 * 16)?;
        for (id, off, len) in std::mem::take(&mut self.index) {
            self.bytes(&id)?;
            self.u32(if id == *b"00dc" { 0x10 } else { 0 })?;
            self.u32(off)?;
            self.u32(len)?;
        }
        self.patch(4, (self.position - 8) as u32)?;
        self.patch(self.total, self.frames)?;
        self.patch(self.video_len, self.frames)?;
        self.patch(self.audio_len, self.frames * AUDIO_PER_FRAME as u32)?;
        self.file.flush()
    }
}

impl Drop for Avi {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_markers_have_independent_columns_and_no_aim_tap_trail() {
        let (tx, _rx) = mpsc::sync_channel(3);
        let mut r = Recording::default();
        r.session = Some(Session {
            tx,
            audio: Arc::new(OutputRecording::default()),
            stop: Arc::new(Mutex::new(None)),
            next: Instant::now(),
        });
        r.observe(InputEvent::KeyDown { vk: 0x26 });
        r.observe(InputEvent::KeyDown { vk: 0xc2 });
        r.observe(InputEvent::StylusAim { x: 1, y: 0 });
        r.aim(1, 0);
        let overlay = r.overlay((240, 320), 0);
        assert_eq!(overlay.direction, Some(0x26));
        assert_eq!(overlay.button.as_deref(), Some("P2"));
        assert!(overlay.point.is_none());
        assert_eq!(overlay.lookup.as_deref(), Some("lookup right"));
        r.observe(InputEvent::PointerDown { x: 60, y: 160 });
        assert_eq!(r.overlay((240, 320), 1).point, Some((0.5, 0.25)));
        let old = Instant::now() - Duration::from_millis(1100);
        r.direction = Some((0x26, old));
        r.label = Some(("P2".into(), old));
        r.lookup = Some(("lookup right".into(), old));
        r.point = Some(((60, 160), old));
        // Held controls have no timeout and clear on their matching releases.
        assert_eq!(r.overlay((240, 320), 0).direction, Some(0x26));
        assert_eq!(r.overlay((240, 320), 0).button.as_deref(), Some("P2"));
        assert!(r.overlay((240, 320), 0).lookup.is_some());
        r.observe(InputEvent::KeyUp { vk: 0x26 });
        r.observe(InputEvent::KeyUp { vk: 0xc2 });
        r.observe(InputEvent::StylusAim { x: 0, y: 0 });
        assert!(r.overlay((240, 320), 0).point.is_some());
        r.observe(InputEvent::PointerMove { x: 120, y: 80 });
        assert_eq!(r.overlay((240, 320), 0).point, Some((0.5, 0.25)));
        r.observe(InputEvent::PointerUp { x: 120, y: 80 });
        r.observe(InputEvent::PointerMove { x: 200, y: 200 });
        let overlay = r.overlay((240, 320), 0);
        assert!(
            overlay.direction.is_none()
                && overlay.button.is_none()
                && overlay.lookup.is_none()
                && overlay.point.is_none()
        );
    }
    #[cfg(feature = "video-static")]
    #[test]
    fn compressed_recording_writes_mp4_through_native_ffmpeg() {
        let root = std::env::var_os("POCKETHLE_RECORDING_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("pockethle-video-test-{}", std::process::id()))
            });
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("compressed.mp4");
        let _ = std::fs::remove_file(&path);
        let (tx, rx) = mpsc::sync_channel(3);
        let epoch = Instant::now();
        tx.send(Frame {
            at: epoch,
            pixels: image::RgbaImage::from_pixel(321, 241, image::Rgba([40, 120, 180, 255])),
            overlay: Overlay {
                direction: Some(0x26),
                button: Some("P2".into()),
                lookup: Some("lookup right".into()),
                ..Default::default()
            },
        })
        .unwrap();
        drop(tx);
        let paths = encode(
            rx,
            Arc::new(OutputRecording::default()),
            Arc::new(Mutex::new(Some(epoch + Duration::from_secs(1)))),
            path,
            75,
            epoch,
            true,
        )
        .unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].metadata().unwrap().len() > 1000);
    }

    #[test]
    fn recording_resizes_into_exact_display_resolution_segments() {
        let root = std::env::var_os("POCKETHLE_RECORDING_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("pockethle-video-test-{}", std::process::id()))
            });
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("resize.avi");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(segment_path(&path, 1));
        let (tx, rx) = mpsc::sync_channel(3);
        let epoch = Instant::now();
        let small = image::RgbaImage::from_pixel(321, 241, image::Rgba([200, 50, 20, 255]));
        let large = image::RgbaImage::from_pixel(640, 480, image::Rgba([20, 50, 200, 255]));
        tx.send(Frame {
            at: epoch,
            pixels: small.clone(),
            overlay: Overlay {
                direction: Some(0x26),
                button: Some("P1".into()),
                ..Default::default()
            },
        })
        .unwrap();
        tx.send(Frame {
            at: epoch + Duration::from_millis(500),
            pixels: large,
            overlay: Overlay::default(),
        })
        .unwrap();
        drop(tx);
        let paths = encode(
            rx,
            Arc::new(OutputRecording::default()),
            Arc::new(Mutex::new(Some(epoch + Duration::from_secs(1)))),
            path,
            75,
            epoch,
            false,
        )
        .unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(small.get_pixel(15, 226).0, [200, 50, 20, 255]);
        assert!(paths.iter().all(|p| p.metadata().unwrap().len() > 1000));
    }
}
