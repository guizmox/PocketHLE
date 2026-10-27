//! Optional host mapping: displayed D-pad directions -> held stylus zones.
use crate::{gapi, InputEvent};

#[derive(Debug, Default)]
pub struct TouchSteering {
    pub enabled: bool,
    // Display order: up, right, down, left. Opposing keys use last press per axis.
    held: [bool; 4],
    selected: [usize; 2],
    point: Option<(u16, u16)>,
    release: Option<InputEvent>,
    analog: (i16, i16),
}

impl TouchSteering {
    pub fn cancel(&mut self) -> Option<InputEvent> {
        self.held = [false; 4];
        self.analog = (0, 0);
        let pending = self.release.take();
        self.point
            .take()
            .map(|(x, y)| InputEvent::PointerUp { x, y })
            .or(pending)
    }

    /// Complete the center move before consuming any later host input.
    pub fn take_release(&mut self) -> Option<InputEvent> {
        self.release.take()
    }

    pub fn input(&mut self, event: InputEvent, turns: u8, w: u32, h: u32) -> Option<InputEvent> {
        let (vk, down) = match event {
            InputEvent::KeyDown { vk } => (vk, true),
            InputEvent::KeyUp { vk } => (vk, false),
            _ => return Some(event),
        };
        // Frontends already inverse-rotate directional keys for the guest.
        let index = match vk {
            gapi::VK_UP => 0,
            gapi::VK_RIGHT => 1,
            gapi::VK_DOWN => 2,
            gapi::VK_LEFT => 3,
            _ => return Some(event),
        };
        let direction = (index + turns as usize) % 4;
        if self.held[direction] == down {
            return None;
        }
        self.held[direction] = down;
        let axis = if direction % 2 == 1 { 0 } else { 1 };
        let opposite = (direction + 2) % 4;
        if down {
            self.selected[axis] = direction;
        } else if self.held[opposite] {
            self.selected[axis] = opposite;
        }

        self.emit(turns, w, h)
    }

    /// Display-relative normalized axes. Full tilt matches the D-pad's zones.
    pub fn position(&mut self, x: i16, y: i16, turns: u8, w: u32, h: u32) -> Option<InputEvent> {
        fn axis(value: i16) -> i16 {
            let value = i32::from(value).clamp(-1000, 1000);
            let magnitude = ((value.abs() - 180).max(0) * 1000 / 820) as i16;
            if value < 0 {
                -magnitude
            } else {
                magnitude
            }
        }
        self.analog = (axis(x), axis(y));
        self.emit(turns, w, h)
    }

    fn emit(&mut self, turns: u8, w: u32, h: u32) -> Option<InputEvent> {
        let (w, h) = (w.clamp(1, 32767) as u16, h.clamp(1, 32767) as u16);
        let horizontal = if self.held[1] || self.held[3] {
            if self.selected[0] == 3 {
                -1000
            } else {
                1000
            }
        } else {
            i32::from(self.analog.0)
        };
        let vertical = if self.held[0] || self.held[2] {
            if self.selected[1] == 0 {
                -1000
            } else {
                1000
            }
        } else {
            i32::from(self.analog.1)
        };
        if horizontal == 0 && vertical == 0 {
            self.point.take()?;
            let (x, y) = (w / 2, h / 2);
            self.release = Some(InputEvent::PointerUp { x, y });
            return Some(InputEvent::PointerMove { x, y });
        }
        let (dw, dh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
        let x = (i32::from(dw) / 2 + horizontal * i32::from(dw) / 4000) as u16;
        let y = (i32::from(dh) / 2 + vertical * i32::from(dh) / 4000) as u16;
        let (x, y) = match turns % 4 {
            1 => (y, h - 1 - x),
            2 => (w - 1 - x, h - 1 - y),
            3 => (w - 1 - y, x),
            _ => (x, y),
        };
        if self.point == Some((x, y)) {
            return None;
        }
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
    fn key(direction: usize, turns: u8, down: bool) -> InputEvent {
        let keys = [gapi::VK_UP, gapi::VK_RIGHT, gapi::VK_DOWN, gapi::VK_LEFT];
        let vk = keys[(direction + 4 - turns as usize) % 4];
        if down {
            InputEvent::KeyDown { vk }
        } else {
            InputEvent::KeyUp { vk }
        }
    }
    fn displayed(event: InputEvent, turns: u8) -> (u16, u16) {
        let (x, y) = match event {
            InputEvent::PointerDown { x, y } | InputEvent::PointerMove { x, y } => (x, y),
            _ => panic!("expected stylus down/move"),
        };
        match turns {
            1 => (319 - y, x),
            2 => (239 - x, 319 - y),
            3 => (y, 239 - x),
            _ => (x, y),
        }
    }
    #[test]
    fn analog_is_proportional_and_returns_to_center() {
        let mut s = TouchSteering::default();
        assert!(s.position(100, 0, 0, 240, 320).is_none());
        let Some(InputEvent::PointerDown { x, y: 160 }) = s.position(590, 0, 0, 240, 320) else {
            panic!("no analog down")
        };
        assert!(x > 120 && x < 180);
        assert_eq!(
            s.position(1000, -1000, 0, 240, 320),
            Some(InputEvent::PointerMove { x: 180, y: 80 })
        );
        assert_eq!(
            s.position(0, 0, 0, 240, 320),
            Some(InputEvent::PointerMove { x: 120, y: 160 })
        );
        assert_eq!(
            s.take_release(),
            Some(InputEvent::PointerUp { x: 120, y: 160 })
        );
        assert!(s.position(0, 0, 0, 240, 320).is_none());
    }
    #[test]
    fn analog_and_arrows_share_display_zones_in_every_rotation() {
        for turns in 0..4 {
            let mut analog = TouchSteering::default();
            let mut arrows = TouchSteering::default();
            arrows.input(key(0, turns, true), turns, 240, 320);
            let diagonal = arrows.input(key(3, turns, true), turns, 240, 320).unwrap();
            let position = analog.position(-1000, -1000, turns, 240, 320).unwrap();
            assert_eq!(displayed(position, turns), displayed(diagonal, turns));
        }
    }
    #[test]
    fn diagonal_release_preserves_other_axis_in_every_rotation() {
        for turns in 0..4 {
            let mut s = TouchSteering::default();
            let (w, h) = if turns % 2 == 1 {
                (320, 240)
            } else {
                (240, 320)
            };
            assert_eq!(
                displayed(
                    s.input(key(0, turns, true), turns, 240, 320).unwrap(),
                    turns
                ),
                (w / 2, h / 4)
            );
            assert_eq!(
                displayed(
                    s.input(key(3, turns, true), turns, 240, 320).unwrap(),
                    turns
                ),
                (w / 4, h / 4)
            );
            assert!(s.input(key(3, turns, true), turns, 240, 320).is_none());
            assert_eq!(
                displayed(
                    s.input(key(3, turns, false), turns, 240, 320).unwrap(),
                    turns
                ),
                (w / 2, h / 4)
            );
            assert_eq!(
                s.input(key(0, turns, false), turns, 240, 320),
                Some(InputEvent::PointerMove { x: 120, y: 160 })
            );
            assert_eq!(
                s.take_release(),
                Some(InputEvent::PointerUp { x: 120, y: 160 })
            );
            assert!(s.take_release().is_none());
        }
    }
    #[test]
    fn opposing_keys_are_independent_per_axis() {
        let mut s = TouchSteering::default();
        s.input(key(0, 0, true), 0, 240, 320);
        s.input(key(3, 0, true), 0, 240, 320);
        assert_eq!(
            s.input(key(2, 0, true), 0, 240, 320),
            Some(InputEvent::PointerMove { x: 60, y: 240 })
        );
        assert_eq!(
            s.input(key(2, 0, false), 0, 240, 320),
            Some(InputEvent::PointerMove { x: 60, y: 80 })
        );
        assert_eq!(s.cancel(), Some(InputEvent::PointerUp { x: 60, y: 80 }));
        assert!(s.cancel().is_none());
    }
}
