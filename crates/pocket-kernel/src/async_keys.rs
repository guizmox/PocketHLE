//! Optional Win32-compatible press history for GetAsyncKeyState's low bit.
//! CE documents that bit as unspecified; it must never replace the held-state bit.
use crate::InputEvent;
use std::collections::VecDeque;

pub struct AsyncKeys {
    down: [bool; 256],
    pressed: [bool; 256],
    seen: usize,
}
impl Default for AsyncKeys {
    fn default() -> Self {
        Self {
            down: [false; 256],
            pressed: [false; 256],
            seen: 0,
        }
    }
}
impl AsyncKeys {
    pub fn observe(&mut self, event: InputEvent) {
        let (key, down) = match event {
            InputEvent::KeyDown { vk } => (usize::from(vk), true),
            InputEvent::KeyUp { vk } => (usize::from(vk), false),
            InputEvent::PointerDown { .. } => (1, true),
            InputEvent::PointerUp { .. } => (1, false),
            InputEvent::MouseMessage { message, .. } => match message {
                0x204 => (2, true),
                0x205 => (2, false),
                0x207 => (4, true),
                0x208 => (4, false),
                _ => return,
            },
            _ => return,
        };
        if key >= 256 {
            return;
        }
        if down && !self.down[key] {
            self.pressed[key] = true;
        }
        self.down[key] = down;
    }
    pub fn sync(&mut self, pending: &VecDeque<InputEvent>, legacy_aliases: bool) {
        if self.seen > pending.len() {
            self.seen = 0;
        }
        for event in pending.iter().skip(self.seen) {
            self.observe(*event);
            if legacy_aliases {
                let alias = |vk: u16| match vk {
                    0xc1..=0xc4 => Some(vk + 0x10),
                    0xd1..=0xd4 => Some(vk - 0x10),
                    _ => None,
                };
                let mirrored = match *event {
                    InputEvent::KeyDown { vk } => alias(vk).map(|vk| InputEvent::KeyDown { vk }),
                    InputEvent::KeyUp { vk } => alias(vk).map(|vk| InputEvent::KeyUp { vk }),
                    _ => None,
                };
                if let Some(event) = mirrored {
                    self.observe(event);
                }
            }
        }
        self.seen = pending.len();
    }
    pub fn removed(&mut self) {
        self.seen = self.seen.saturating_sub(1);
    }
    pub fn requeued_front(&mut self) {
        self.seen += 1;
    }
    pub fn take(&mut self, key: usize) -> bool {
        self.pressed.get_mut(key).is_some_and(|p| std::mem::take(p))
    }
}
