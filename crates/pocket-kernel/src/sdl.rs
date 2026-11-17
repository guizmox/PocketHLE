//! Session-owned SDL 1.2 software video and input resources.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
#[derive(Clone, Serialize, Deserialize)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bytes: u32,
    pub pixels: u32,
    pub format: u32,
    pub masks: [u32; 4],
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SdlState {
    pub initialized: u32,
    pub screen: u32,
    pub surfaces: HashMap<u32, Surface>,
    pub events: VecDeque<[u8; 20]>,
    #[serde(with = "serde_bytes")]
    pub keys: [u8; 323],
    pub key_ptr: u32,
    pub error: String,
    pub error_ptr: u32,
    pub version_ptr: u32,
    pub mouse: (u16, u16, bool),
}
impl Default for SdlState {
    fn default() -> Self {
        Self {
            initialized: 0,
            screen: 0,
            surfaces: HashMap::new(),
            events: VecDeque::new(),
            keys: [0; 323],
            key_ptr: 0,
            error: String::new(),
            error_ptr: 0,
            version_ptr: 0,
            mouse: (0, 0, false),
        }
    }
}

impl SdlState {
    pub(crate) fn validate_snapshot(&self) -> Result<(), String> {
        if self.events.len() > 1348
            || self.surfaces.len() > 128
            || (self.screen != 0 && !self.surfaces.contains_key(&self.screen))
        {
            return Err("invalid saved SDL state".into());
        }
        for (&id, surface) in &self.surfaces {
            if id == 0
                || surface.width == 0
                || surface.height == 0
                || surface.width > 4096
                || surface.height > 4096
                || surface.pixels == 0
                || surface.format == 0
                || !(1..=4).contains(&surface.bytes)
                || surface
                    .width
                    .checked_mul(surface.bytes)
                    .is_none_or(|row| row > surface.pitch)
                || surface
                    .pitch
                    .checked_mul(surface.height)
                    .is_none_or(|size| {
                        size > 64 * 1024 * 1024 || surface.pixels.checked_add(size).is_none()
                    })
            {
                return Err("invalid saved SDL surface".into());
            }
        }
        Ok(())
    }

    pub(crate) fn release_snapshot_inputs(&mut self) {
        // Pending host key/button events are transient. Application-pushed
        // window/user events remain, followed by releases already delivered.
        self.events.retain(|event| !matches!(event[0], 2..=6));
        for (symbol, key) in self.keys.iter_mut().enumerate() {
            if std::mem::take(key) != 0 {
                let mut event = [0; 20];
                event[0] = 3; // SDL_KEYUP
                event[8..12].copy_from_slice(&(symbol as u32).to_le_bytes());
                self.events.push_back(event);
            }
        }
        if std::mem::take(&mut self.mouse.2) {
            let mut event = [0; 20];
            event[0] = 6; // SDL_MOUSEBUTTONUP
            event[2] = 1;
            event[4..6].copy_from_slice(&self.mouse.0.to_le_bytes());
            event[6..8].copy_from_slice(&self.mouse.1.to_le_bytes());
            self.events.push_back(event);
        }
    }
}
