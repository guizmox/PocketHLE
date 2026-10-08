//! Real-time audio output backed by [`cpal`].
//!
//! The Pocket PC `waveOut*` API and `PlaySoundW` / `PlaySoundA` /
//! `sndPlaySoundW` family are routed through this module so that
//! games which previously had no sound can drive real PCM samples
//! out of the host's default audio device. Each HWAVEOUT has an independent
//! PCM queue, format, pause state and completion clock. Both cpal and the
//! Android tap mix these queues concurrently using nearest-neighbour
//! resampling. The legacy ring remains available for push_samples callers.
//! Resetting a wave stream clears only that handle; already delivered host
//! frames can only disappear after the host's short output buffer drains.
//!
//! The cpal feature is optional. Without it, host frontends can still pull
//! mixed PCM through AudioTap (Android); headless completion uses a wall
//! clock when no output device consumes samples.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Maximum number of i16 samples we keep buffered. At 44.1 kHz
/// stereo this is about twelve seconds — plenty to absorb scheduling
/// jitter, but small enough that overflows produce dropped samples
/// instead of unbounded memory growth.
const RING_CAPACITY_SAMPLES: usize = 1 << 20; // 1048576

/// Audio format last requested by the guest. We remember it so the
/// cpal callback can decide whether to upsample or play 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
}

impl Default for GuestFormat {
    fn default() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            bits_per_sample: 16,
        }
    }
}

#[derive(Debug)]
#[cfg(feature = "audio-cpal")]
struct AudioVoice {
    samples: Vec<i16>,
    format: GuestFormat,
    position_q16: u64,
    looped: bool,
    volume: f32,
    group: u32,
    paused: bool,
}

/// How a voice submitted through [`AudioEngine::play_voice_with`]
/// should behave.
///
/// `group` exists so a caller can silence one class of sound without
/// touching the rest: `hss.dll`'s `stopMusics` must stop the music
/// without cutting off the sound effects that are still playing.
#[derive(Debug, Clone, Copy)]
pub struct VoiceParams {
    pub looped: bool,
    /// Tag for [`AudioEngine::stop_voice_group`]. Voices submitted
    /// through [`AudioEngine::play_voice`] land in group 0.
    pub group: u32,
    /// Linear gain. 1.0 plays the samples as submitted.
    pub volume: f32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self {
            looped: false,
            group: 0,
            volume: 1.0,
        }
    }
}

/// One HWAVEOUT owns its PCM, format, resampling phase and completion clock.
/// Keep complete submissions: Battlestations submits its entire menu music at
/// once, and a bounded global ring both loses its beginning and delays SFX.
#[derive(Clone)]
struct WaveStream {
    samples: std::collections::VecDeque<i16>,
    format: GuestFormat,
    phase: u64,
    written: u64,
    consumed: u64,
    paused: bool,
    tick: Instant,
    fraction: u128,
}

impl WaveStream {
    fn new(format: GuestFormat) -> Self {
        Self { samples: Default::default(), format, phase: 0, written: 0,
            consumed: 0, paused: false, tick: Instant::now(), fraction: 0 }
    }

    fn advance_virtual(&mut self, active: bool) {
        let now = Instant::now();
        if !active && !self.paused && self.consumed < self.written {
            let rate = self.format.sample_rate.max(1) as u128;
            let elapsed = now.duration_since(self.tick).as_nanos() * rate + self.fraction;
            let frames = elapsed / 1_000_000_000;
            self.fraction = elapsed % 1_000_000_000;
            self.consume((frames * self.format.channels.max(1) as u128)
                .min(usize::MAX as u128) as usize);
        }
        self.tick = now;
        if self.samples.is_empty() { self.fraction = 0; }
    }

    fn consume(&mut self, count: usize) {
        let count = count.min(self.samples.len());
        self.samples.drain(..count);
        self.consumed += count as u64;
    }

    fn frame(&mut self, rate: u32) -> (f32, f32) {
        let channels = self.format.channels.max(1) as usize;
        if self.paused || self.samples.len() < channels { return (0.0, 0.0); }
        let left = self.samples[0] as f32 / 32768.0;
        let right = if channels > 1 { self.samples[1] as f32 / 32768.0 } else { left };
        self.phase += ((self.format.sample_rate.max(1) as u64) << 16) / rate.max(1) as u64;
        let frames = self.phase >> 16;
        self.phase &= 0xffff;
        self.consume((frames as usize).saturating_mul(channels));
        if self.samples.is_empty() { self.phase = 0; }
        (left, right)
    }
}

/// Inner state shared between the emulator thread (which calls
/// [`AudioEngine::push_samples`]) and the cpal output callback.
struct Shared {
    wave_streams: std::collections::BTreeMap<u32, WaveStream>,
    ring: Vec<i16>,
    /// Number of samples currently in the ring.
    len: usize,
    /// Read cursor.
    read: usize,
    /// Write cursor.
    write: usize,
    /// Format of the most recently submitted guest stream.
    guest_format: GuestFormat,
    /// Stable output format used by host frontends for the lifetime of
    /// the audio session. Android AudioTrack must not be reconfigured
    /// halfway through a session when another waveOut device opens.
    mix_format: GuestFormat,
    mix_format_ready: bool,
    /// Track whether the guest explicitly opened audio so Android does
    /// not build AudioTrack from the fallback format.
    guest_format_ready: bool,
    /// Sub-sample fraction for the nearest-neighbour resampler (in
    /// units of 1/65536). Carried across cpal callbacks so we don't
    /// lose pitch on long playbacks.
    resampler_phase: u64,
    /// Total guest samples ever submitted through [`Shared::push`].
    written: u64,
    /// Total guest samples the host device has actually consumed.
    /// Only meaningful while `device_active` is set.
    consumed: u64,
    /// `true` once a cpal output stream is playing. With no host
    /// device we fall back to a wall-clock playback estimate so
    /// guests that wait for buffer-done notifications still make
    /// progress.
    device_active: bool,
    /// Wall-clock playback estimate, in guest samples.
    virtual_cursor: u64,
    /// When the wall-clock estimate was last advanced.
    virtual_tick: Option<Instant>,
    /// `true` between `waveOutPause` and `waveOutRestart`. The device
    /// keeps its queue but stops consuming, so the playback cursor
    /// freezes and buffer-done notifications stop until playback
    /// resumes.
    paused: bool,
    /// Independent sound-effect voices mixed over the waveOut stream.
    #[cfg(feature = "audio-cpal")]
    voices: Vec<AudioVoice>,
    /// Optional WAV tap so headless runs can verify that a game
    /// really produces sound on a machine with no audio hardware.
    capture: Option<WavCapture>,
}

impl Shared {
    fn new() -> Self {
        Self {
            wave_streams: Default::default(),
            ring: vec![0i16; RING_CAPACITY_SAMPLES],
            len: 0,
            read: 0,
            write: 0,
            guest_format: GuestFormat::default(),
            mix_format: GuestFormat::default(),
            mix_format_ready: false,
            guest_format_ready: false,
            resampler_phase: 0,
            written: 0,
            consumed: 0,
            device_active: false,
            virtual_cursor: 0,
            virtual_tick: None,
            paused: false,
            #[cfg(feature = "audio-cpal")]
            voices: Vec::new(),
            capture: None,
        }
    }

    /// Guest samples played so far. Backed by the host device when we
    /// have one and by a wall clock otherwise. Never runs ahead of
    /// what the guest actually submitted, so a game that stops
    /// feeding buffers doesn't see phantom progress.
    fn cursor(&mut self) -> u64 {
        if self.paused {
            // Reset the wall-clock reference so a long pause doesn't
            // release a burst of buffers the moment playback resumes.
            self.virtual_tick = None;
            return if self.device_active {
                self.consumed.min(self.written)
            } else {
                self.virtual_cursor
            };
        }
        if self.device_active {
            return self.consumed.min(self.written);
        }
        let rate =
            self.guest_format.sample_rate.max(1) as u64 * self.guest_format.channels.max(1) as u64;
        let now = Instant::now();
        let last = *self.virtual_tick.get_or_insert(now);
        let elapsed_us = now.saturating_duration_since(last).as_micros() as u64;
        if elapsed_us > 0 {
            self.virtual_tick = Some(now);
            self.virtual_cursor = self
                .virtual_cursor
                .saturating_add(elapsed_us.saturating_mul(rate) / 1_000_000);
        }
        self.virtual_cursor = self.virtual_cursor.min(self.written);
        self.virtual_cursor
    }

    fn push(&mut self, samples: &[i16]) {
        self.written = self.written.saturating_add(samples.len() as u64);
        let fmt = self.guest_format;
        if let Some(cap) = self.capture.as_mut() {
            cap.write(samples, fmt);
        }
        let cap = self.ring.len();
        for &s in samples {
            if self.len == cap {
                // Ring is full — drop the oldest sample to make room.
                self.read = (self.read + 1) % cap;
                self.len -= 1;
            }
            self.ring[self.write] = s;
            self.write = (self.write + 1) % cap;
            self.len += 1;
        }
    }

    fn pop_one(&mut self) -> Option<i16> {
        if self.len == 0 {
            return None;
        }
        let v = self.ring[self.read];
        self.read = (self.read + 1) % self.ring.len();
        self.len -= 1;
        self.consumed = self.consumed.saturating_add(1);
        Some(v)
    }

    fn clear(&mut self) {
        self.wave_streams.clear();
        self.len = 0;
        self.read = 0;
        self.write = 0;
        self.resampler_phase = 0;
        self.written = 0;
        self.consumed = 0;
        self.virtual_cursor = 0;
        self.virtual_tick = None;
        #[cfg(feature = "audio-cpal")]
        self.voices.clear();
    }

    #[cfg(feature = "audio-cpal")]
    fn stop_voices(&mut self) {
        self.voices.clear();
    }

    #[cfg(feature = "audio-cpal")]
    fn stop_voice_group(&mut self, group: u32) {
        self.voices.retain(|v| v.group != group);
    }

    #[cfg(feature = "audio-cpal")]
    fn pause_voice_group(&mut self, group: u32, paused: bool) {
        for voice in &mut self.voices {
            if voice.group == group {
                voice.paused = paused;
            }
        }
    }

    #[cfg(feature = "audio-cpal")]
    fn add_voice(&mut self, samples: Vec<i16>, format: GuestFormat, params: VoiceParams) {
        if samples.is_empty() {
            return;
        }
        if let Some(cap) = self.capture.as_mut() {
            cap.write(&samples, format);
        }
        self.voices.push(AudioVoice {
            samples,
            format,
            position_q16: 0,
            looped: params.looped,
            volume: params.volume,
            group: params.group,
            paused: false,
        });
    }

    fn render_frames(&mut self, output: &mut [f32], output_rate: u32, output_channels: u16) {
        let channels = output_channels.max(1) as usize;
        let frames = output.len() / channels;
        for frame in 0..frames {
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            if !self.paused {
                let guest_channels = self.guest_format.channels.max(1) as usize;
                if self.len >= guest_channels {
                    let left_sample = self.ring[self.read] as f32 / 32768.0;
                    let right_sample = if guest_channels > 1 {
                        self.ring[(self.read + 1) % self.ring.len()] as f32 / 32768.0
                    } else {
                        left_sample
                    };
                    left += left_sample;
                    right += right_sample;
                    let step_q16 = ((self.guest_format.sample_rate.max(1) as u64) << 16)
                        / output_rate.max(1) as u64;
                    self.resampler_phase = self.resampler_phase.saturating_add(step_q16);
                    let advance_frames = (self.resampler_phase >> 16) as usize;
                    self.resampler_phase &= 0xFFFF;
                    for _ in 0..advance_frames.saturating_mul(guest_channels) {
                        let _ = self.pop_one();
                    }
                } else {
                    self.resampler_phase = 0;
                }

                for stream in self.wave_streams.values_mut() {
                    let (l, r) = stream.frame(output_rate);
                    left += l;
                    right += r;
                }

                #[cfg(feature = "audio-cpal")]
                {
                let mut index = 0;
                while index < self.voices.len() {
                    let voice = &mut self.voices[index];
                    if voice.paused {
                        index += 1;
                        continue;
                    }
                    let voice_channels = voice.format.channels.max(1) as usize;
                    let frame_count = voice.samples.len() / voice_channels;
                    let source_frame = (voice.position_q16 >> 16) as usize;
                    if frame_count == 0 || source_frame >= frame_count {
                        if voice.looped && frame_count > 0 {
                            voice.position_q16 %= (frame_count as u64) << 16;
                        } else {
                            self.voices.swap_remove(index);
                            continue;
                        }
                    }
                    let source_frame = (voice.position_q16 >> 16) as usize;
                    let sample_index = source_frame * voice_channels;
                    let left_sample = voice.samples[sample_index] as f32 / 32768.0 * voice.volume;
                    let right_sample = if voice_channels > 1 {
                        voice.samples[sample_index + 1] as f32 / 32768.0 * voice.volume
                    } else {
                        left_sample
                    };
                    left += left_sample;
                    right += right_sample;
                    voice.position_q16 = voice.position_q16.saturating_add(
                        ((voice.format.sample_rate.max(1) as u64) << 16)
                            / output_rate.max(1) as u64,
                    );
                    index += 1;
                }
                }
            }
            left = left.clamp(-1.0, 1.0);
            right = right.clamp(-1.0, 1.0);
            for channel in 0..channels {
                output[frame * channels + channel] = if channel == 0 { left } else { right };
            }
        }
        for sample in &mut output[frames * channels..] {
            *sample = 0.0;
        }
    }
}

/// Minimal streaming WAV writer behind [`AudioEngine::capture_to`].
///
/// The RIFF sizes are rewritten after every submission rather than
/// only on drop: emulator runs are frequently killed by a signal
/// (timeouts, frame-budget harnesses) and a half-written header would
/// make the capture useless exactly when it is needed most.
struct WavCapture {
    file: std::fs::File,
    data_bytes: u64,
    header: Option<GuestFormat>,
}

impl WavCapture {
    fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            file: std::fs::File::create(path)?,
            data_bytes: 0,
            header: None,
        })
    }

    fn write(&mut self, samples: &[i16], fmt: GuestFormat) {
        if self.header.is_none() {
            // Capture is always 16-bit: `push_samples_u8` widens 8-bit
            // PCM before it reaches the ring.
            let fmt = GuestFormat {
                bits_per_sample: 16,
                ..fmt
            };
            if self.write_header(fmt).is_err() {
                return;
            }
            self.header = Some(fmt);
        }
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        if self.file.write_all(&bytes).is_err() {
            return;
        }
        self.data_bytes = self.data_bytes.saturating_add(bytes.len() as u64);
        let _ = self.patch_sizes();
    }

    fn write_header(&mut self, fmt: GuestFormat) -> std::io::Result<()> {
        let channels = fmt.channels.max(1);
        let rate = fmt.sample_rate.max(1);
        let block_align = channels * 2;
        let byte_rate = rate * block_align as u32;
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&0u32.to_le_bytes()); // patched by patch_sizes
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes()); // WAVE_FORMAT_PCM
        h.extend_from_slice(&channels.to_le_bytes());
        h.extend_from_slice(&rate.to_le_bytes());
        h.extend_from_slice(&byte_rate.to_le_bytes());
        h.extend_from_slice(&block_align.to_le_bytes());
        h.extend_from_slice(&16u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&0u32.to_le_bytes()); // patched by patch_sizes
        self.file.write_all(&h)
    }

    fn patch_sizes(&mut self) -> std::io::Result<()> {
        let data = self.data_bytes.min(u32::MAX as u64 - 36) as u32;
        self.file.seek(SeekFrom::Start(4))?;
        self.file.write_all(&(36 + data).to_le_bytes())?;
        self.file.seek(SeekFrom::Start(40))?;
        self.file.write_all(&data.to_le_bytes())?;
        self.file.seek(SeekFrom::End(0))?;
        Ok(())
    }
}

/// Public handle the rest of the emulator interacts with. Cheaply
/// cloneable via [`AudioEngine::clone`] (the underlying state is
/// behind an `Arc<Mutex<_>>`); the kernel keeps one copy and
/// [`AudioEngine::start`] / [`AudioEngine::stop`] hand additional
/// clones to the cpal callback.
pub struct AudioEngine {
    shared: Arc<Mutex<Shared>>,
    /// Whether the audio worker thread is alive. The thread itself
    /// owns the cpal `Stream` (which is `!Send` on some platforms),
    /// so we communicate with it via [`Shared`] and the
    /// [`Self::shutdown`] flag.
    #[cfg(feature = "audio-cpal")]
    worker: Option<std::thread::JoinHandle<()>>,
    #[cfg(not(feature = "audio-cpal"))]
    worker: Option<()>,
    /// Set by [`Self::stop`] to ask the audio thread to drop its
    /// stream and exit.
    #[cfg(feature = "audio-cpal")]
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    /// `true` once we've tried (and possibly failed) to open the
    /// device. Stops us from spamming the user log with retries on
    /// every `waveOutOpen`.
    init_attempted: bool,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AudioEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioEngine")
            .field("init_attempted", &self.init_attempted)
            .field("worker_alive", &self.worker.is_some())
            .finish()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared::new())),
            worker: None,
            #[cfg(feature = "audio-cpal")]
            shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            init_attempted: false,
        }
    }

    pub fn open_wave_stream(&self, handle: u32, format: GuestFormat) {
        self.set_guest_format(format);
        if let Ok(mut s) = self.shared.lock() {
            s.wave_streams.insert(handle, WaveStream::new(format));
        }
    }

    pub fn push_wave_samples(&self, handle: u32, samples: &[i16]) {
        if let Ok(mut s) = self.shared.lock() {
            if let Some(format) = s.wave_streams.get(&handle).map(|v| v.format) {
                if let Some(capture) = s.capture.as_mut() { capture.write(samples, format); }
            }
            let active = s.device_active;
            if let Some(stream) = s.wave_streams.get_mut(&handle) {
                stream.advance_virtual(active);
                stream.samples.extend(samples.iter().copied());
                stream.written += samples.len() as u64;
            }
        }
    }

    pub fn wave_written_samples(&self, handle: u32) -> u64 {
        self.shared.lock().ok().and_then(|s| s.wave_streams.get(&handle).map(|v| v.written)).unwrap_or(0)
    }

    pub fn wave_playback_cursor(&self, handle: u32) -> u64 {
        let Ok(mut s) = self.shared.lock() else { return 0; };
        let active = s.device_active;
        s.wave_streams.get_mut(&handle).map(|v| {
            v.advance_virtual(active);
            v.consumed
        }).unwrap_or(0)
    }

    pub fn reset_wave_stream(&self, handle: u32) {
        if let Ok(mut s) = self.shared.lock() {
            if let Some(v) = s.wave_streams.get_mut(&handle) { *v = WaveStream::new(v.format); }
        }
    }

    pub fn close_wave_stream(&self, handle: u32) {
        if let Ok(mut s) = self.shared.lock() { s.wave_streams.remove(&handle); }
    }

    pub fn pause_wave_stream(&self, handle: u32, paused: bool) {
        if let Ok(mut s) = self.shared.lock() {
            let active = s.device_active;
            if let Some(v) = s.wave_streams.get_mut(&handle) {
                v.advance_virtual(active);
                v.paused = paused;
            }
        }
    }

    /// Update the guest-side format. Called from `waveOutOpen` /
    /// `PlaySound` so the resampler knows what rate the i16 samples
    /// are coming in at.
    pub fn set_guest_format(&self, fmt: GuestFormat) {
        if let Ok(mut s) = self.shared.lock() {
            s.guest_format = fmt;
            if !s.mix_format_ready {
                s.mix_format = fmt;
                s.mix_format_ready = true;
            }
            s.guest_format_ready = true;
            s.resampler_phase = 0;
        }
    }

    /// Submit a chunk of interleaved 16-bit PCM. The samples are
    /// queued for the host audio thread to play. Returns the number
    /// of samples actually queued (we never block; if the ring is
    /// full the oldest samples get dropped).
    pub fn push_samples(&self, samples: &[i16]) -> usize {
        if let Ok(mut s) = self.shared.lock() {
            s.push(samples);
            samples.len()
        } else {
            0
        }
    }

    pub fn play_voice(&self, samples: &[i16], format: GuestFormat, looped: bool) -> usize {
        self.play_voice_with(
            samples,
            format,
            VoiceParams {
                looped,
                ..Default::default()
            },
        )
    }

    /// Submit a voice with an explicit group and gain — see
    /// [`VoiceParams`].
    pub fn play_voice_with(
        &self,
        samples: &[i16],
        format: GuestFormat,
        params: VoiceParams,
    ) -> usize {
        #[cfg(feature = "audio-cpal")]
        {
            if let Ok(mut s) = self.shared.lock() {
                s.add_voice(samples.to_vec(), format, params);
                samples.len()
            } else {
                0
            }
        }
        #[cfg(not(feature = "audio-cpal"))]
        {
            let _ = (format, params);
            self.push_samples(samples)
        }
    }

    pub fn stop_voices(&self) {
        #[cfg(feature = "audio-cpal")]
        if let Ok(mut s) = self.shared.lock() {
            s.stop_voices();
        }
    }

    /// Stop only the voices tagged with `group`, leaving every other
    /// voice playing. Without the `audio-cpal` feature there are no
    /// voices to stop and this does nothing.
    pub fn stop_voice_group(&self, group: u32) {
        #[cfg(feature = "audio-cpal")]
        if let Ok(mut s) = self.shared.lock() {
            s.stop_voice_group(group);
        }
        #[cfg(not(feature = "audio-cpal"))]
        let _ = group;
    }

    /// Pause or resume only one mixer voice group.  This is used by devices
    /// such as Gizmondo MAS1 whose transport is independent of waveOut.
    pub fn pause_voice_group(&self, group: u32, paused: bool) {
        #[cfg(feature = "audio-cpal")]
        if let Ok(mut s) = self.shared.lock() {
            s.pause_voice_group(group, paused);
        }
        #[cfg(not(feature = "audio-cpal"))]
        let _ = (group, paused);
    }

    /// Convenience for unsigned 8-bit PCM (the format `PlaySound` and
    /// some old WAV resources use). Each byte is mapped to the
    /// signed 16-bit range linearly.
    pub fn push_samples_u8(&self, bytes: &[u8]) -> usize {
        let mut buf = Vec::with_capacity(bytes.len());
        for &b in bytes {
            let v = (b as i16 - 128) * 256;
            buf.push(v);
        }
        self.push_samples(&buf)
    }

    /// Drop any queued samples. Called by `waveOutReset` and on
    /// engine shutdown.
    pub fn flush(&self) {
        if let Ok(mut s) = self.shared.lock() {
            s.clear();
            s.mix_format_ready = false;
            s.guest_format_ready = false;
            s.resampler_phase = 0;
        }
    }

    /// Flush only the WinMM/waveOut PCM stream. Independent mixer voices
    /// (for example Gizmondo MAS1 playback) keep running.
    pub fn flush_wave_out(&self) {
        if let Ok(mut s) = self.shared.lock() {
            s.wave_streams.clear();
            s.len = 0;
            s.read = 0;
            s.write = 0;
            s.resampler_phase = 0;
            s.written = 0;
            s.consumed = 0;
            s.virtual_cursor = 0;
            s.virtual_tick = None;
            s.mix_format_ready = false;
            s.guest_format_ready = false;
        }
    }

    /// Suspend or resume playback (`waveOutPause` / `waveOutRestart`).
    /// While paused the device keeps its queue but stops consuming, so
    /// [`Self::playback_cursor`] freezes and no further buffers are
    /// reported as finished.
    pub fn set_paused(&self, paused: bool) {
        if let Ok(mut s) = self.shared.lock() {
            s.paused = paused;
        }
    }

    /// Number of samples currently queued.
    pub fn buffered_samples(&self) -> usize {
        self.shared.lock().map(|s| s.len + s.wave_streams.values().map(|v| v.samples.len()).sum::<usize>()).unwrap_or(0)
    }

    /// Total guest samples submitted since the stream was opened (or
    /// since the last [`Self::flush`]).
    pub fn written_samples(&self) -> u64 {
        self.shared.lock().map(|s| s.written).unwrap_or(0)
    }

    /// Guest samples played back so far. `waveOutGetPosition` and the
    /// `WOM_DONE` bookkeeping in `waveOut*` are both driven from this.
    pub fn playback_cursor(&self) -> u64 {
        self.shared.lock().map(|mut s| s.cursor()).unwrap_or(0)
    }

    /// A cheap, clone-able handle that lets the *host* play the guest's
    /// PCM itself instead of leaving it to cpal.
    ///
    /// Android needs this: cpal's Android backend goes through
    /// `oboe-sys`, which needs an NDK C++ toolchain we do not ship, so
    /// the `audio-cpal` feature is off there and the ring buffer would
    /// otherwise just fill up and drop samples. The JNI layer pulls from
    /// a tap and writes into a Java `AudioTrack` in streaming mode — the
    /// same shape J2ME emulators such as J2ME Loader use for PCM output.
    ///
    /// The tap can be moved to another thread; it locks the same shared
    /// state the guest pushes into.
    pub fn tap(&self) -> AudioTap {
        AudioTap {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Tee every submitted sample into a 16-bit PCM WAV file. Used by
    /// `pockethle run --dump-audio-to` so a run on a machine with no
    /// sound card can still prove the game produced audio.
    pub fn capture_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        let capture = WavCapture::create(path)?;
        if let Ok(mut s) = self.shared.lock() {
            s.capture = Some(capture);
        }
        Ok(())
    }

    /// Open the host audio device and start streaming. Idempotent —
    /// subsequent calls are no-ops as long as the worker is still
    /// alive. When the `audio-cpal` feature is disabled this is a
    /// no-op.
    pub fn start(&mut self) {
        if self.worker.is_some() || self.init_attempted {
            return;
        }
        self.init_attempted = true;
        self.start_impl();
    }

    #[cfg(feature = "audio-cpal")]
    fn start_impl(&mut self) {
        // cpal::Stream is `!Send` on some platforms, so the stream
        // has to be owned by a single dedicated thread. We hand the
        // shared ring + a shutdown flag to the worker; the worker
        // builds the stream, plays it, and parks until told to
        // exit, at which point dropping the stream stops audio
        // playback.
        let shared = Arc::clone(&self.shared);
        let shutdown = Arc::clone(&self.shutdown);
        shutdown.store(false, std::sync::atomic::Ordering::SeqCst);
        let handle = match std::thread::Builder::new()
            .name("pockethle-audio".to_string())
            .spawn(move || run_audio_worker(shared, shutdown))
        {
            Ok(handle) => Some(handle),
            Err(e) => {
                log::warn!("AudioEngine: could not spawn audio thread — running silently: {e}");
                None
            }
        };
        self.worker = handle;
    }

    #[cfg(not(feature = "audio-cpal"))]
    fn start_impl(&mut self) {
        // A build with the feature off can never make a sound, and the
        // guest cannot tell — so this is a warning, not a note. It is
        // the first thing to rule out when a packaged binary is silent
        // but a local one is not.
        log::warn!("AudioEngine: built without the audio-cpal feature — running silently");
    }

    /// Stop the host stream and clear any pending samples. The
    /// engine can be re-`start`ed afterwards.
    pub fn stop(&mut self) {
        #[cfg(feature = "audio-cpal")]
        {
            self.shutdown
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(h) = self.worker.take() {
                let _ = h.join();
            }
        }
        #[cfg(not(feature = "audio-cpal"))]
        {
            self.worker = None;
        }
        self.flush();
        self.init_attempted = false;
    }
}

/// Host-side pull handle produced by [`AudioEngine::tap`].
///
/// Draining moves the playback cursor forward, so `waveOutGetPosition`
/// and the `WHDR_DONE` notifications stay in step with what the host has
/// actually played. While nothing drains the tap the engine keeps using
/// its wall-clock estimate, so a frontend that never calls
/// [`AudioTap::drain_into`] behaves exactly as it did before.
#[derive(Clone)]
pub struct AudioTap {
    shared: Arc<Mutex<Shared>>,
}

impl std::fmt::Debug for AudioTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioTap").finish_non_exhaustive()
    }
}

impl AudioTap {
    /// The format the guest is currently submitting samples in, so the
    /// host can configure its own output device to match.
    pub fn guest_format(&self) -> GuestFormat {
        self.shared
            .lock()
            .map(|s| {
                if s.mix_format_ready {
                    s.mix_format
                } else {
                    s.guest_format
                }
            })
            .unwrap_or_default()
    }

    pub fn format_ready(&self) -> bool {
        self.shared
            .lock()
            .map(|s| s.guest_format_ready)
            .unwrap_or(false)
    }

    /// Move up to `dst.len()` queued samples out of the ring into `dst`,
    /// returning how many were written. Never blocks: a starved host
    /// gets a short read and should pad the rest with silence.
    pub fn drain_into(&self, dst: &mut [i16]) -> usize {
        let Ok(mut s) = self.shared.lock() else {
            return 0;
        };
        s.device_active = true;
        if !s.wave_streams.is_empty() {
            let format = s.mix_format;
            let mut output = vec![0.0; dst.len()];
            s.render_frames(&mut output, format.sample_rate, format.channels);
            for (sample, mixed) in dst.iter_mut().zip(output) {
                *sample = (mixed * 32768.0).clamp(-32768.0, 32767.0) as i16;
            }
            return dst.len();
        }
        let mut n = 0;
        while n < dst.len() {
            match s.pop_one() {
                Some(v) => {
                    dst[n] = v;
                    n += 1;
                }
                None => break,
            }
        }
        n
    }

    /// Move up to `dst.len()` samples without taking ownership of the
    /// playback cursor. This is used by Android while it is probing the
    /// guest format; probing must not switch the engine to device-clock
    /// mode before AudioTrack has actually started.
    pub fn peek_into(&self, dst: &mut [i16]) -> usize {
        let Ok(s) = self.shared.lock() else { return 0; };
        if !s.wave_streams.is_empty() {
            let channels = s.mix_format.channels.max(1) as usize;
            let mut streams = s.wave_streams.clone();
            for frame in dst.chunks_exact_mut(channels) {
                let (mut left, mut right) = (0.0f32, 0.0f32);
                for stream in streams.values_mut() {
                    let (l, r) = stream.frame(s.mix_format.sample_rate);
                    left += l; right += r;
                }
                for (ch, sample) in frame.iter_mut().enumerate() {
                    *sample = ((if ch == 0 { left } else { right }) * 32768.0)
                        .clamp(-32768.0, 32767.0) as i16;
                }
            }
            return dst.len() / channels * channels;
        }
        let mut n = 0;
        let mut index = s.read;
        let mut remaining = s.len;
        while n < dst.len() && remaining > 0 {
            dst[n] = s.ring[index];
            n += 1;
            remaining -= 1;
            index = (index + 1) % s.ring.len();
        }
        n
    }

    /// How many samples are queued and ready to be drained.
    pub fn buffered_samples(&self) -> usize {
        self.shared.lock().map(|s| s.len + s.wave_streams.values().map(|v| v.samples.len()).sum::<usize>()).unwrap_or(0)
    }
}

#[cfg(feature = "audio-cpal")]
fn run_audio_worker(shared: Arc<Mutex<Shared>>, shutdown: Arc<std::sync::atomic::AtomicBool>) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => {
            // Every early return in this function means the run is
            // silent. That is a user-visible failure, not a detail, so
            // these log at `warn`: the desktop GUI is built with
            // `windows_subsystem = "windows"` and has no console, so an
            // `info` line reaches nobody and "no sound" arrives with no
            // explanation attached.
            log::warn!("AudioEngine: no default output device — running silently");
            return;
        }
    };
    let device_name = device.name().unwrap_or_else(|_| "<unnamed>".to_string());
    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            log::warn!(
                "AudioEngine: default_output_config() failed on {device_name:?} \
                 — running silently: {e}"
            );
            return;
        }
    };

    let host_rate = config.sample_rate().0;
    let host_channels = config.channels();
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.clone().into();
    let err_fn = |err| log::warn!("AudioEngine: cpal output error: {err}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            let shared = Arc::clone(&shared);
            device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| {
                    fill_output_f32(&shared, data, host_rate, host_channels);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let shared = Arc::clone(&shared);
            device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _| {
                    fill_output_i16(&shared, data, host_rate, host_channels);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let shared = Arc::clone(&shared);
            device.build_output_stream(
                &stream_config,
                move |data: &mut [u16], _| {
                    fill_output_u16(&shared, data, host_rate, host_channels);
                },
                err_fn,
                None,
            )
        }
        other => {
            log::warn!(
                "AudioEngine: host device {device_name:?} wants sample format \
                 {other:?}, which is not supported — running silently"
            );
            return;
        }
    };
    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "AudioEngine: build_output_stream failed on {device_name:?} \
                 ({host_rate} Hz / {host_channels} ch / {sample_format:?}) \
                 — running silently: {e}"
            );
            return;
        }
    };
    if let Err(e) = stream.play() {
        log::warn!("AudioEngine: stream.play() failed on {device_name:?} — running silently: {e}");
        return;
    }
    log::info!(
        "AudioEngine: opened {:?} at {} Hz / {} ch ({:?})",
        device_name,
        host_rate,
        host_channels,
        sample_format
    );
    if let Ok(mut s) = shared.lock() {
        s.device_active = true;
    }
    while !shutdown.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if let Ok(mut s) = shared.lock() {
        s.device_active = false;
    }
    drop(stream);
}

#[cfg(feature = "audio-cpal")]
fn fill_output_f32(
    shared: &Arc<Mutex<Shared>>,
    data: &mut [f32],
    host_rate: u32,
    host_channels: u16,
) {
    let mut s = match shared.lock() {
        Ok(g) => g,
        Err(_) => {
            data.fill(0.0);
            return;
        }
    };
    s.render_frames(data, host_rate, host_channels);
}

#[cfg(feature = "audio-cpal")]
fn fill_output_i16(
    shared: &Arc<Mutex<Shared>>,
    data: &mut [i16],
    host_rate: u32,
    host_channels: u16,
) {
    let mut tmp = vec![0f32; data.len()];
    fill_output_f32(shared, &mut tmp, host_rate, host_channels);
    for (i, v) in tmp.iter().enumerate() {
        let s = (v * 32767.0).clamp(-32768.0, 32767.0) as i16;
        data[i] = s;
    }
}

#[cfg(feature = "audio-cpal")]
fn fill_output_u16(
    shared: &Arc<Mutex<Shared>>,
    data: &mut [u16],
    host_rate: u32,
    host_channels: u16,
) {
    let mut tmp = vec![0f32; data.len()];
    fill_output_f32(shared, &mut tmp, host_rate, host_channels);
    for (i, v) in tmp.iter().enumerate() {
        let s = ((v + 1.0) * 32767.5).clamp(0.0, 65535.0) as u16;
        data[i] = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(rate: u32) -> GuestFormat {
        GuestFormat { sample_rate: rate, channels: 1, bits_per_sample: 16 }
    }

    #[test]
    fn wave_handles_mix_and_reset_only_the_requested_stream() {
        let e = AudioEngine::new();
        let tap = e.tap();
        tap.drain_into(&mut []); // deterministic device clock, no wall time
        e.open_wave_stream(1, mono(44100));
        e.open_wave_stream(2, mono(44100));
        e.push_wave_samples(1, &[1000; 8]);
        e.push_wave_samples(2, &[2000; 8]);
        let mut out = [0; 2];
        tap.drain_into(&mut out);
        assert_eq!(out, [3000; 2]);
        assert_eq!(e.wave_playback_cursor(1), 2);
        assert_eq!(e.wave_playback_cursor(2), 2);
        e.reset_wave_stream(1);
        tap.drain_into(&mut out);
        assert_eq!(out, [2000; 2]);
        assert_eq!(e.wave_playback_cursor(1), 0);
        assert_eq!(e.wave_written_samples(1), 0);
        assert_eq!(e.wave_playback_cursor(2), 4);
        e.push_wave_samples(1, &[4000; 2]);
        tap.drain_into(&mut out);
        assert_eq!(out, [6000; 2]);
        e.close_wave_stream(1);
        tap.drain_into(&mut out);
        assert_eq!(out, [2000; 2]);
    }

    #[test]
    fn wave_pause_and_formats_are_independent_on_android_tap() {
        let e = AudioEngine::new();
        let tap = e.tap();
        tap.drain_into(&mut []);
        e.open_wave_stream(1, mono(44100));
        e.open_wave_stream(2, mono(22050));
        e.push_wave_samples(1, &[1000; 8]);
        e.push_wave_samples(2, &[2000, 3000, 4000, 5000]);
        e.pause_wave_stream(1, true);
        let mut out = [0; 4];
        tap.drain_into(&mut out);
        assert_eq!(out, [2000, 2000, 3000, 3000]);
        assert_eq!(e.wave_playback_cursor(1), 0);
        assert_eq!(e.wave_playback_cursor(2), 2);
        assert_eq!(tap.guest_format(), mono(44100));
        e.pause_wave_stream(1, false);
        tap.drain_into(&mut out);
        assert_eq!(out, [5000, 5000, 6000, 6000]);
        assert_eq!(e.wave_playback_cursor(1), 4);
    }

    #[test]
    fn wave_keeps_entire_music_and_peek_does_not_consume_it() {
        let e = AudioEngine::new();
        let tap = e.tap();
        tap.drain_into(&mut []);
        e.open_wave_stream(1, mono(44100));
        e.push_wave_samples(1, &vec![1234; RING_CAPACITY_SAMPLES + 8]);
        assert_eq!(e.buffered_samples(), RING_CAPACITY_SAMPLES + 8);
        let mut out = [0; 4];
        tap.peek_into(&mut out);
        assert_eq!(out, [1234; 4]);
        assert_eq!(e.wave_playback_cursor(1), 0);
        tap.drain_into(&mut out);
        assert_eq!(e.wave_playback_cursor(1), 4);
    }

    #[test]
    fn wave_headless_clock_respects_pause_and_clamps_to_submission() {
        let mut stream = WaveStream::new(mono(1000));
        stream.samples.extend([42; 10]);
        stream.written = 10;
        stream.tick = Instant::now() - std::time::Duration::from_secs(1);
        stream.paused = true;
        stream.advance_virtual(false);
        assert_eq!(stream.consumed, 0);
        stream.paused = false;
        stream.tick = Instant::now() - std::time::Duration::from_secs(1);
        stream.advance_virtual(false);
        assert_eq!(stream.consumed, 10);
        assert!(stream.samples.is_empty());
    }

    #[test]
    fn engine_starts_silently_with_no_feature() {
        let mut e = AudioEngine::new();
        e.start();
        // start() must not panic regardless of host configuration.
        e.stop();
    }

    #[test]
    fn ring_drops_oldest_on_overflow() {
        let mut s = Shared::new();
        let big = vec![1i16; RING_CAPACITY_SAMPLES + 8];
        s.push(&big);
        assert_eq!(s.len, RING_CAPACITY_SAMPLES);
    }

    #[test]
    fn push_then_buffered_samples() {
        let e = AudioEngine::new();
        e.set_guest_format(GuestFormat {
            sample_rate: 22050,
            channels: 1,
            bits_per_sample: 16,
        });
        e.push_samples(&[0, 1, 2, 3]);
        assert_eq!(e.buffered_samples(), 4);
        e.flush();
        assert_eq!(e.buffered_samples(), 0);
    }

    #[test]
    fn push_u8_maps_to_signed_range() {
        let e = AudioEngine::new();
        let n = e.push_samples_u8(&[0x80, 0x00, 0xFF]);
        assert_eq!(n, 3);
        assert_eq!(e.buffered_samples(), 3);
    }
}
