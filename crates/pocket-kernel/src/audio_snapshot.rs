//! Guest PCM queues and sample positions. Host device and recording handles
//! are supplied by the receiving frontend.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub struct MasSnapshot {
    stream: Option<WaveStream>,
    volume: u32,
}
fn valid_format(format: GuestFormat) -> bool {
    format.sample_rate != 0 && format.channels != 0
}
fn invalid_wave(wave: &WaveStream) -> bool {
    !valid_format(wave.format)
        || wave.consumed > wave.written
        || wave.loop_buffer.as_ref().is_some_and(|looping| {
            looping.samples.is_empty()
                || looping.index >= looping.samples.len()
                || looping.remaining == 0
        })
}
impl MasPlayback {
    pub fn capture_checkpoint(&self) -> Result<MasSnapshot, String> {
        Ok(MasSnapshot {
            stream: self.0.lock().map_err(|_| "MAS transport poisoned")?.clone(),
            volume: *self.1.lock().map_err(|_| "MAS volume poisoned")?,
        })
    }
}
impl MasSnapshot {
    pub fn restore(mut self) -> Result<MasPlayback, String> {
        if self.stream.as_ref().is_some_and(invalid_wave) {
            return Err("invalid shared MAS transport".into());
        }
        if let Some(stream) = self.stream.as_mut() {
            stream.tick = Instant::now();
        }
        Ok(MasPlayback(
            Arc::new(Mutex::new(self.stream)),
            Arc::new(Mutex::new(self.volume)),
        ))
    }
}

#[derive(Serialize, Deserialize)]
pub struct AudioSnapshot {
    device_volume: u32,
    wave_streams: BTreeMap<u32, WaveStream>,
    mas_streams: BTreeMap<u32, (Option<WaveStream>, u32)>,
    shared_mas_keys: Vec<u32>,
    ring: Vec<i16>,
    guest_format: GuestFormat,
    mix_format: GuestFormat,
    mix_format_ready: bool,
    guest_format_ready: bool,
    resampler_phase: u64,
    written: u64,
    consumed: u64,
    virtual_cursor: u64,
    paused: bool,
    voices: Vec<AudioVoice>,
    conversions: conversion::Streams,
}

impl AudioEngine {
    /// Copy queue data and cursors under the mixer lock so a host audio
    /// callback cannot finish half of a saved submission during capture.
    pub fn capture_snapshot(&self) -> Result<AudioSnapshot, String> {
        self.capture_snapshot_at(Instant::now())
    }
    pub fn capture_snapshot_at(&self, at: Instant) -> Result<AudioSnapshot, String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "audio mixer lock poisoned")?;
        let mas_streams = shared
            .mas_streams
            .iter()
            .map(|(&key, playback)| {
                let mut stream = playback.0.lock().unwrap().clone();
                if let Some(stream) = stream.as_mut() {
                    stream.advance_virtual_at(shared.device_active, at);
                }
                (key, (stream, *playback.1.lock().unwrap()))
            })
            .collect();
        let mut wave_streams = shared.wave_streams.clone();
        for stream in wave_streams.values_mut() {
            stream.advance_virtual_at(shared.device_active, at);
        }
        let virtual_cursor = shared.cursor_at(at);
        Ok(AudioSnapshot {
            device_volume: shared.device_volume,
            wave_streams,
            mas_streams,
            shared_mas_keys: Vec::new(),
            ring: (0..shared.len)
                .map(|index| shared.ring[(shared.read + index) % shared.ring.len()])
                .collect(),
            guest_format: shared.guest_format,
            mix_format: shared.mix_format,
            mix_format_ready: shared.mix_format_ready,
            guest_format_ready: shared.guest_format_ready,
            resampler_phase: shared.resampler_phase,
            written: shared.written,
            consumed: shared.consumed,
            virtual_cursor,
            paused: shared.paused,
            voices: shared.voices.clone(),
            conversions: self.conversions.clone(),
        })
    }
}

impl AudioSnapshot {
    pub fn restore(self) -> Result<AudioEngine, String> {
        self.restore_with_shared_mas(None)
    }

    pub fn take_device_mas(
        &mut self,
        keys: &std::collections::BTreeSet<u32>,
    ) -> BTreeMap<u32, MasSnapshot> {
        let mut shared = BTreeMap::new();
        for &key in keys {
            if let Some((stream, volume)) = self.mas_streams.remove(&key) {
                self.shared_mas_keys.push(key);
                shared.insert(key, MasSnapshot { stream, volume });
            }
        }
        shared
    }

    pub fn restore_with_shared_mas(
        self,
        device: Option<&BTreeMap<u32, MasPlayback>>,
    ) -> Result<AudioEngine, String> {
        if !self.shared_mas_keys.is_empty() && device.is_none() {
            return Err("shared MAS streams require a device checkpoint".into());
        }
        if self.ring.len() > RING_CAPACITY_SAMPLES
            || self.virtual_cursor > self.written
            || !valid_format(self.guest_format)
            || !valid_format(self.mix_format)
            || self.wave_streams.values().any(invalid_wave)
            || self
                .mas_streams
                .values()
                .filter_map(|(stream, _)| stream.as_ref())
                .any(invalid_wave)
            || self.voices.iter().any(|voice| !valid_format(voice.format))
        {
            return Err("invalid saved audio stream state".into());
        }
        let mut engine = AudioEngine::new();
        engine.conversions = self.conversions;
        {
            let mut shared = engine
                .shared
                .lock()
                .map_err(|_| "audio mixer lock poisoned")?;
            shared.device_volume = self.device_volume;
            shared.wave_streams = self.wave_streams;
            for wave in shared.wave_streams.values_mut() {
                wave.tick = Instant::now();
            }
            shared.mas_streams = self
                .mas_streams
                .into_iter()
                .map(|(key, (mut stream, volume))| {
                    if let Some(stream) = stream.as_mut() {
                        stream.tick = Instant::now();
                    }
                    (
                        key,
                        MasPlayback(Arc::new(Mutex::new(stream)), Arc::new(Mutex::new(volume))),
                    )
                })
                .collect();
            for key in self.shared_mas_keys {
                let stream = device
                    .and_then(|device| device.get(&key))
                    .ok_or("missing shared MAS transport")?;
                if shared.mas_streams.insert(key, stream.clone()).is_some() {
                    return Err("duplicate saved MAS stream binding".into());
                }
            }
            shared.len = self.ring.len();
            shared.write = shared.len % shared.ring.len();
            shared.ring[..self.ring.len()].copy_from_slice(&self.ring);
            shared.guest_format = self.guest_format;
            shared.mix_format = self.mix_format;
            shared.mix_format_ready = self.mix_format_ready;
            shared.guest_format_ready = self.guest_format_ready;
            shared.resampler_phase = self.resampler_phase;
            shared.written = self.written;
            shared.consumed = self.consumed;
            shared.virtual_cursor = self.virtual_cursor;
            shared.paused = self.paused;
            shared.voices = self.voices;
        }
        Ok(engine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_audio_keeps_process_local_media_streams_private() {
        let engine = AudioEngine::new();
        let format = GuestFormat {
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
        };
        engine.queue_mas_samples(0x00130000, format, &[100, 200]);
        engine.queue_mas_samples(0xd3000001, format, &[300, 400]);
        let mut saved = engine.capture_snapshot().unwrap();
        let shared = saved.take_device_mas(&[0xd3000001].into());
        assert_eq!(shared.keys().copied().collect::<Vec<_>>(), vec![0xd3000001]);
        assert!(saved.mas_streams.contains_key(&0x00130000));
        assert_eq!(saved.shared_mas_keys, vec![0xd3000001]);
        let transports = shared
            .into_iter()
            .map(|(key, stream)| (key, stream.restore().unwrap()))
            .collect();
        let restored = saved.restore_with_shared_mas(Some(&transports)).unwrap();
        assert!(restored
            .capture_snapshot()
            .unwrap()
            .mas_streams
            .contains_key(&0x00130000));
    }
    #[test]
    fn audio_snapshot_resumes_mas_pcm_and_its_completion_cursor() {
        const STREAM: u32 = 0xd3000000;
        const FORMAT: GuestFormat = GuestFormat {
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 16,
        };
        let render = |engine: &AudioEngine, pairs: usize| {
            let mut pcm = vec![0.; pairs * 2];
            let mut mixer = engine.shared.lock().unwrap();
            mixer.device_active = true;
            mixer.render_frames(&mut pcm, FORMAT.sample_rate, FORMAT.channels);
            pcm
        };
        let engine = AudioEngine::new();
        engine.shared.lock().unwrap().device_active = true;
        engine.queue_mas_samples(STREAM, FORMAT, &[400, 800, 1200, 1600, 2000, 2400]);
        render(&engine, 1);
        let saved_cursor = engine.mas_playback_cursor(STREAM);
        let saved = engine.capture_snapshot().unwrap();

        let expected = render(&engine, 2);
        assert!(expected.iter().any(|&sample| sample != 0.));
        let restored = saved.restore().unwrap();
        restored.shared.lock().unwrap().device_active = true;
        assert_eq!(restored.mas_playback_cursor(STREAM), saved_cursor);
        assert_eq!(render(&restored, 2), expected);
        assert_eq!(
            restored.mas_playback_cursor(STREAM),
            engine.mas_playback_cursor(STREAM)
        );
    }

    #[test]
    fn audio_snapshot_resumes_pcm_at_saved_cursor() {
        let engine = AudioEngine::new();
        const FORMAT: GuestFormat = GuestFormat {
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 16,
        };
        engine.push_samples(&[1000, 2000, 3000, 4000, 5000, 6000]);
        let render = |engine: &AudioEngine, count| {
            let mut pcm = vec![0.; count];
            let mut mixer = engine.shared.lock().unwrap();
            mixer.device_active = true;
            mixer.guest_format = FORMAT;
            mixer.render_frames(&mut pcm, FORMAT.sample_rate, FORMAT.channels);
            pcm
        };
        render(&engine, 2);
        let bytes = serde_json::to_vec(&engine.capture_snapshot().unwrap()).unwrap();
        let expected = render(&engine, 4);
        assert_ne!(expected, [0.; 4]);
        drop(engine);

        let saved: AudioSnapshot = serde_json::from_slice(&bytes).unwrap();
        let restored = saved.restore().unwrap();
        assert_eq!(render(&restored, 4), expected);
    }
}
