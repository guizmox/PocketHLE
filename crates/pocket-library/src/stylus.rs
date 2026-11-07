//! Separate Pocket PC host controls: never aliases of guest hardware keys.
use serde::{Deserialize, Serialize};
pub const DIRECTIONS: [&str; 4] = ["Up", "Down", "Left", "Right"];
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StylusBindings {
    pub speed: u16,
    pub keyboard: [Vec<String>; 4],
}
impl Default for StylusBindings {
    fn default() -> Self {
        Self {
            speed: 240,
            keyboard: ["O", "L", "K", "M"].map(|s| vec![s.into()]),
        }
    }
}
impl StylusBindings {
    pub fn velocity(&self, strengths: [f32; 4], turns: u8) -> (i16, i16) {
        let x = (strengths[3] - strengths[2]).clamp(-1.0, 1.0);
        let y = (strengths[1] - strengths[0]).clamp(-1.0, 1.0);
        let (x, y) = match turns % 4 {
            1 => (y, -x),
            2 => (-x, -y),
            3 => (-y, x),
            _ => (x, y),
        };
        let scale = f32::from(self.speed.clamp(30, 1200)) / x.hypot(y).max(1.0);
        ((x * scale).round() as i16, (y * scale).round() as i16)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_defaults_are_independent_and_vectors_rotate_and_normalize() {
        let b: StylusBindings = serde_json::from_str("{}").unwrap();
        assert_eq!(controller_direction("RightStickUp"), Some(0));
        assert_eq!(b.keyboard[0], vec!["O"]);
        assert_eq!(b.velocity([0., 0., 0., 1.], 1), (0, -240));
        assert_eq!(b.velocity([1., 1., 0., 0.], 0), (0, 0));
        let (x, y) = b.velocity([1., 0., 0., 1.], 0);
        assert!((f32::from(x).hypot(f32::from(y)) - 240.).abs() < 1.);
        assert_eq!(
            b,
            serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap()
        );
    }
}

/// The physical right stick is owned by the stylus in both modes.
pub fn controller_direction(name: &str) -> Option<usize> {
    let direction = name.strip_prefix("RightStick")?;
    DIRECTIONS.iter().position(|d| *d == direction)
}
pub fn controller_reserved(name: &str, gizmondo: bool) -> bool {
    !gizmondo && (name == "RightThumb" || controller_direction(name).is_some())
}

/// Frontend pointer and double-click policy. Time is monotonic milliseconds.
/// A single click is deferred until the double-click window closes, so switching
/// modes never injects an unintended guest tap.
#[derive(Default)]
pub struct StylusPointer {
    pub pointer: bool,
    position: Option<(f32, f32)>,
    released_at: Option<u64>,
    tap_position: Option<(u16, u16)>,
    held: bool,
    second: bool,
}
impl StylusPointer {
    pub const DOUBLE_MS: u64 = 300;
    pub fn button(&mut self, down: bool, now: u64) -> bool {
        if down {
            if self.held {
                return false;
            }
            self.held = true;
            self.second = self
                .released_at
                .is_some_and(|t| now.saturating_sub(t) < Self::DOUBLE_MS);
            if self.second {
                self.released_at = None;
            }
        } else if self.held {
            self.held = false;
            if self.second {
                self.second = false;
                self.pointer = !self.pointer;
                return true;
            }
            self.released_at = Some(now);
            self.tap_position = self.position();
        }
        false
    }
    pub fn advance(&mut self, velocity: (i16, i16), seconds: f32, size: (u32, u32)) {
        let (w, h) = (size.0.max(1), size.1.max(1));
        let (mut x, mut y) = self.position.unwrap_or(((w / 2) as f32, (h / 2) as f32));
        if self.pointer {
            let dt = seconds.clamp(0.0, 0.05);
            x += f32::from(velocity.0) * dt;
            y += f32::from(velocity.1) * dt;
        }
        self.position = Some((x.clamp(0.0, (w - 1) as f32), y.clamp(0.0, (h - 1) as f32)));
    }
    pub fn needs_tick(&self, velocity: (i16, i16)) -> bool {
        self.pointer && (velocity != (0, 0) || self.released_at.is_some())
    }
    pub fn position(&self) -> Option<(u16, u16)> {
        self.position
            .map(|(x, y)| (x.round() as u16, y.round() as u16))
    }
    pub fn take_tap(&mut self, now: u64) -> Option<(u16, u16)> {
        if self
            .released_at
            .is_some_and(|t| now.saturating_sub(t) >= Self::DOUBLE_MS)
        {
            self.released_at = None;
            if self.pointer {
                return self.tap_position.take();
            }
        }
        None
    }
    pub fn cancel_click(&mut self) {
        self.released_at = None;
        self.tap_position = None;
        self.held = false;
        self.second = false;
    }
}
#[cfg(test)]
mod pointer_tests {
    use super::*;
    #[test]
    fn double_click_switches_without_tapping_and_single_click_only_taps_in_pointer_mode() {
        let mut p = StylusPointer::default();
        p.advance((0, 0), 0.0, (240, 320));
        p.button(true, 0);
        p.button(false, 10);
        assert_eq!(p.take_tap(310), None);
        p.button(true, 400);
        p.button(false, 410);
        p.button(true, 500);
        assert_eq!(p.take_tap(800), None);
        assert!(p.button(false, 810));
        assert!(p.pointer);
        p.button(true, 900);
        p.button(false, 910);
        assert_eq!(p.take_tap(1209), None);
        assert_eq!(p.take_tap(1210), Some((120, 160)));
        assert_eq!(p.take_tap(1211), None);
        p.button(true, 1300);
        p.button(false, 1310);
        p.button(true, 1400);
        p.button(false, 1410);
        assert!(!p.pointer);
        assert_eq!(p.take_tap(1800), None);
    }
    #[test]
    fn click_uses_release_position_and_legacy_enable_and_controller_fields_are_discarded() {
        let mut p = StylusPointer {
            pointer: true,
            ..Default::default()
        };
        p.advance((0, 0), 0.0, (240, 320));
        p.button(true, 0);
        p.button(false, 10);
        p.advance((240, 0), 0.05, (240, 320));
        assert_ne!(p.position(), Some((120, 160)));
        assert_eq!(p.take_tap(310), Some((120, 160)));
        let b: StylusBindings =
            serde_json::from_str(r#"{"enabled":false,"controller":[[],[],[],[]],"speed":300}"#)
                .unwrap();
        assert_eq!(b.velocity([0., 0., 0., 1.], 0), (300, 0));
        let saved = serde_json::to_value(b).unwrap();
        assert!(saved.get("enabled").is_none());
        assert!(saved.get("controller").is_none());
    }
    #[test]
    fn keyboard_mode_key_and_directions_remain_editable_but_are_reserved_from_grids() {
        use crate::{
            input_gestures::binding_key, keybindings::keyboard_button_for_profile, GuestButton,
            LauncherConfig,
        };
        let mut c = LauncherConfig::default();
        assert_eq!(
            keyboard_button_for_profile(&c.keybindings, "p", false),
            Some(GuestButton::StylusClick)
        );
        assert_eq!(keyboard_button_for_profile(&c.keybindings, "P", true), None);
        for key in ["P", "p", "O", "K", "L", "M"] {
            assert!(c.stylus_keyboard_reserved(key));
        }
        c.short_press_bindings
            .insert(binding_key(false, false, "P"), None);
        c.short_press_bindings.insert(
            binding_key(false, false, "J"),
            Some(GuestButton::StylusClick),
        );
        c.short_press_bindings
            .insert(binding_key(false, false, "O"), Some(GuestButton::Action));
        assert!(!c.stylus_keyboard_reserved("P"));
        assert!(!c.stylus_keyboard_reserved("O"));
        assert!(c.stylus_keyboard_reserved("J"));
        c.stylus_stick.keyboard[0] = vec!["I".into()];
        assert!(c.stylus_keyboard_reserved("I"));
    }
    #[test]
    fn pointer_clamps_and_cancel_discards_pending_click() {
        let mut p = StylusPointer::default();
        p.pointer = true;
        p.advance((1200, -1200), 5.0, (40, 40));
        assert_eq!(p.position(), Some((39, 0)));
        p.button(true, 0);
        p.button(true, 1);
        p.button(false, 10);
        p.cancel_click();
        assert_eq!(p.take_tap(1000), None);
        p.advance((0, 0), 0.0, (10, 10));
        assert_eq!(p.position(), Some((9, 0)));
    }
}
