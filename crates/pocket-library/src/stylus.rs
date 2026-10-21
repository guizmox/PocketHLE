//! Separate Pocket PC host controls: never aliases of guest hardware keys.
use serde::{Deserialize, Serialize};
pub const DIRECTIONS: [&str; 4] = ["Up", "Down", "Left", "Right"];
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StylusBindings {
    pub enabled: bool,
    pub speed: u16,
    pub keyboard: [Vec<String>; 4],
    pub controller: [Vec<String>; 4],
}
impl Default for StylusBindings {
    fn default() -> Self {
        Self {
            enabled: true,
            speed: 240,
            keyboard: ["O", "L", "K", "M"].map(|s| vec![s.into()]),
            controller: DIRECTIONS.map(|s| vec![format!("RightStick{s}")]),
        }
    }
}
impl StylusBindings {
    pub fn velocity(&self, strengths: [f32; 4], turns: u8) -> (i16, i16) {
        if !self.enabled {
            return (0, 0);
        }
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
        assert_eq!(b.controller[0], vec!["RightStickUp"]);
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
