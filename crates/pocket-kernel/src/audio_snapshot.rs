//! Guest PCM queues and sample positions. Host device and recording handles
//! are supplied by the receiving frontend.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub struct AudioSnapshot {
    device_volume: u32,
    wave_streams: BTreeMap<u32, WaveStream>,
    mas_streams: BTreeMap<u32, (Option<WaveStream>, u32)>,
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
                    stream.advance_virtual(shared.device_active);
                }
                (key, (stream, *playback.1.lock().unwrap()))
            })
            .collect();
        let mut wave_streams = shared.wave_streams.clone();
        for stream in wave_streams.values_mut() {
            stream.advance_virtual(shared.device_active);
        }
        let virtual_cursor = shared.cursor();
        Ok(AudioSnapshot {
            device_volume: shared.device_volume,
            wave_streams,
            mas_streams,
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
        let valid_format = |format: GuestFormat| format.sample_rate != 0 && format.channels != 0;
        let invalid_wave = |wave: &WaveStream| {
            !valid_format(wave.format)
                || wave.consumed > wave.written
                || wave.loop_buffer.as_ref().is_some_and(|looping| {
                    looping.samples.is_empty()
                        || looping.index >= looping.samples.len()
                        || looping.remaining == 0
                })
        };
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
