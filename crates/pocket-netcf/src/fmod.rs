//! FMOD CE sample playback through the shared PocketHLE mixer.

use pocket_kernel::{audio::decode_pcm_wave, AudioEngine, GuestFormat, VoiceParams};
use std::{collections::BTreeMap, io::Read, path::Path};

#[derive(Default)]
pub struct Fmod {
    channels: u32,
    next_channel: u32,
    next_sample: i32,
    samples: BTreeMap<i32, (GuestFormat, Vec<i16>)>,
}

impl Fmod {
    pub fn init(&mut self, audio: &AudioEngine, rate: i32, channels: i32) -> bool {
        if !(8000..=192000).contains(&rate) || !(1..=256).contains(&channels) {
            return false;
        }
        self.channels = channels as u32;
        self.next_channel = 0;
        audio.set_guest_format(GuestFormat {
            sample_rate: rate as u32,
            channels: 2,
            bits_per_sample: 16,
        });
        true
    }

    pub fn load(&mut self, root: &Path, name: &str) -> Option<i32> {
        const MAX_BYTES: u64 = 4 * 1024 * 1024;
        if self.channels == 0
            || self.samples.len() >= 32
            || !name.to_ascii_lowercase().ends_with(".wav")
        {
            return None;
        }
        let path = root.join(name);
        if !pocket_cab::plain_path_below(root, &path, false) {
            return None;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > MAX_BYTES {
            return None;
        }
        let (format, samples) = decode_pcm_wave(&bytes)?;
        if samples.is_empty()
            || samples.len() > 2 * 1024 * 1024
            || !(1..=2).contains(&format.channels)
            || !(8000..=192000).contains(&format.sample_rate)
        {
            return None;
        }
        self.next_sample = self.next_sample.checked_add(1)?;
        self.samples.insert(self.next_sample, (format, samples));
        Some(self.next_sample)
    }

    pub fn free(&mut self, sample: i32) -> bool {
        self.samples.remove(&sample).is_some()
    }

    pub fn play(&mut self, audio: &AudioEngine, channel: i32, sample: i32) -> Option<i32> {
        let (format, samples) = self.samples.get(&sample)?;
        let channel = if channel == -1 {
            let channel = self.next_channel;
            self.next_channel = (channel + 1) % self.channels;
            channel
        } else {
            u32::try_from(channel)
                .ok()
                .filter(|&channel| channel < self.channels)?
        };
        let group = channel + 1;
        audio.stop_voice_group(group);
        (audio.play_voice_with(
            samples,
            *format,
            VoiceParams {
                group,
                ..Default::default()
            },
        ) != 0)
            .then_some(channel as i32)
    }
}
