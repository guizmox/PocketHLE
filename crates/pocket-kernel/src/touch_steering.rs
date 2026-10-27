//! Optional host control mapping: displayed left/right -> held stylus zones.
use crate::{gapi, InputEvent};
#[derive(Debug, Default)]
pub struct TouchSteering {
    pub enabled: bool,
    held: [bool; 2],
    selected: usize,
    point: Option<(u16, u16)>,
    release: Option<InputEvent>,
}
impl TouchSteering {
    pub fn cancel(&mut self) -> Option<InputEvent> {
        self.held = [false; 2];
        let pending = self.release.take();
        self.point
            .take()
            .map(|(x, y)| InputEvent::PointerUp { x, y })
            .or(pending)
    }
    /// Complete the center move with a release before any later host input.
    pub fn take_release(&mut self) -> Option<InputEvent> {
        self.release.take()
    }
    pub fn input(&mut self, event: InputEvent, turns: u8, w: u32, h: u32) -> Option<InputEvent> {
        let (vk, down) = match event {
            InputEvent::KeyDown { vk } => (vk, true),
            InputEvent::KeyUp { vk } => (vk, false),
            _ => return Some(event),
        };
        let index = match vk {
            gapi::VK_UP => 0,
            gapi::VK_RIGHT => 1,
            gapi::VK_DOWN => 2,
            gapi::VK_LEFT => 3,
            _ => return Some(event),
        };
        let side = match (index + turns as usize) % 4 {
            3 => 0,
            1 => 1,
            _ => return Some(event),
        };
        if self.held[side] == down {
            return None;
        } // Suppress keyboard repeats.
        self.held[side] = down;
        if down {
            self.selected = side;
        } else if self.held[1 - side] {
            self.selected = 1 - side;
        } else {
            self.point.take()?;
            let (x, y) = (
                (w.clamp(1, 32767) / 2) as u16,
                (h.clamp(1, 32767) / 2) as u16,
            );
            self.release = Some(InputEvent::PointerUp { x, y });
            return Some(InputEvent::PointerMove { x, y });
        }
        let (w, h) = (w.clamp(1, 32767) as u16, h.clamp(1, 32767) as u16);
        let (dw, dh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
        let x = ((u32::from(dw) * if self.selected == 0 { 1 } else { 3 }) / 4) as u16;
        let y = dh / 2;
        let (x, y) = match turns % 4 {
            1 => (y, h - 1 - x),
            2 => (w - 1 - x, h - 1 - y),
            3 => (w - 1 - y, x),
            _ => (x, y),
        };
        let was_down = self.point.replace((x, y)).is_some();
        Some(if was_down {
            InputEvent::PointerMove { x, y }
        } else {
            InputEvent::PointerDown { x, y }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_sides_match_all_rotations_and_release() {
        let dirs = [gapi::VK_UP, gapi::VK_RIGHT, gapi::VK_DOWN, gapi::VK_LEFT];
        for turns in 0..4u8 {
            for (display_index, side) in [(3, 0), (1, 1)] {
                let mut s = TouchSteering::default();
                let vk = dirs[(display_index + 4 - turns as usize) % 4];
                let Some(InputEvent::PointerDown { x, y }) =
                    s.input(InputEvent::KeyDown { vk }, turns, 240, 320)
                else {
                    panic!("missing down")
                };
                let (dx, dy, dw) = match turns {
                    1 => (319 - y, x, 320),
                    2 => (239 - x, 319 - y, 240),
                    3 => (y, 239 - x, 320),
                    _ => (x, y, 240),
                };
                assert_eq!(dx, dw * if side == 0 { 1 } else { 3 } / 4);
                assert!(dy > 0);
                assert!(s
                    .input(InputEvent::KeyDown { vk }, turns, 240, 320)
                    .is_none());
                assert_eq!(
                    s.input(InputEvent::KeyUp { vk }, turns, 240, 320),
                    Some(InputEvent::PointerMove { x: 120, y: 160 })
                );
                assert_eq!(
                    s.take_release(),
                    Some(InputEvent::PointerUp { x: 120, y: 160 })
                );
                assert!(s.take_release().is_none());
            }
        }
    }
    #[test]
    fn simultaneous_directions_restore_remaining_press() {
        let mut s = TouchSteering::default();
        assert!(matches!(
            s.input(InputEvent::KeyDown { vk: gapi::VK_LEFT }, 0, 240, 320),
            Some(InputEvent::PointerDown { x: 60, .. })
        ));
        assert!(matches!(
            s.input(InputEvent::KeyDown { vk: gapi::VK_RIGHT }, 0, 240, 320),
            Some(InputEvent::PointerMove { x: 180, .. })
        ));
        assert!(matches!(
            s.input(InputEvent::KeyUp { vk: gapi::VK_RIGHT }, 0, 240, 320),
            Some(InputEvent::PointerMove { x: 60, .. })
        ));
        assert!(matches!(s.cancel(), Some(InputEvent::PointerUp { .. })));
        assert!(s.cancel().is_none());
        assert_eq!(
            s.input(InputEvent::KeyDown { vk: gapi::VK_UP }, 0, 240, 320),
            Some(InputEvent::KeyDown { vk: gapi::VK_UP })
        );
    }
}
