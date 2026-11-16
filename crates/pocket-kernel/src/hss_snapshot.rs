//! The HSS SDK's guest objects. The audio snapshot stores actual mixer voices.

use super::*;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct HssSnapshot {
    channels: HashMap<(u32, u32), HssChannel>,
    speakers: HashMap<u32, (u32, u32)>,
    suspended_speakers: std::collections::HashSet<u32>,
    clips: HashMap<u32, HssClip>,
    format: GuestFormat,
    sound_volume: u32,
    music_volume: u32,
}

impl HssState {
    pub fn capture_snapshot(&self) -> Option<HssSnapshot> {
        if self.speakers.is_empty()
            && self.channels.is_empty()
            && self.clips.is_empty()
            && self.sound_volume == 64
            && self.music_volume == 64
        {
            return None;
        }
        let mut channels = self.channels.clone();
        for channel in channels.values_mut() {
            channel.offset = channel.position();
        }
        Some(HssSnapshot {
            channels,
            speakers: self.speakers.clone(),
            suspended_speakers: self.suspended_speakers.clone(),
            clips: self.clips.clone(),
            format: self.format,
            sound_volume: self.sound_volume,
            music_volume: self.music_volume,
        })
    }
}

impl HssSnapshot {
    pub fn restore(self) -> Result<HssState, String> {
        let Self {
            mut channels,
            speakers,
            suspended_speakers,
            clips,
            format,
            sound_volume,
            music_volume,
        } = self;
        if sound_volume > 64
            || music_volume > 64
            || clips
                .values()
                .any(|clip| clip.format.channels == 0 || clip.format.sample_rate == 0)
            || channels
                .values()
                .any(|channel| channel.offset > channel.frames || !channel.master.is_finite())
            || suspended_speakers
                .iter()
                .any(|speaker| !speakers.contains_key(speaker))
        {
            return Err("invalid HSS audio snapshot".into());
        }
        let now = std::time::Instant::now();
        for channel in channels.values_mut() {
            channel.started = now;
        }
        Ok(HssState {
            channels,
            opened: !speakers.is_empty(),
            speakers,
            suspended_speakers,
            clips,
            decoded: Default::default(),
            format,
            sound_volume,
            music_volume,
        })
    }
}
