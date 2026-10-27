//! Per-game, display-relative stylus bindings. No guest or renderer dependencies.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlsMode {
    Normal,
    Zones,
    Grid,
}
impl ControlsMode {
    pub const ALL: [Self; 3] = [Self::Normal, Self::Zones, Self::Grid];
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Zones => "Touch steering",
            Self::Grid => "Touch zone mapping",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Normal => "Standard keyboard/controller bindings and touch controls. No button is converted into a screen-zone press.",
            Self::Zones => "Arrows / D-pad hold fixed touch zones; left-stick tilt positions the touch progressively, including diagonals. Release returns to the centre. Other buttons work normally.\nExample: steer in Racing Days.",
            Self::Grid => "Map keys, controller buttons or stick directions to touch zones. Hold to press; release to lift. Only assigned inputs are replaced; the last held zone takes priority.\nExample: map Tomb Raider's on-screen actions to controller buttons.",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "source", content = "name", rename_all = "snake_case")]
pub enum HostInput {
    Keyboard(String),
    Controller(String),
}
impl HostInput {
    pub fn label(&self) -> String {
        match self {
            Self::Keyboard(n) => format!("Key {n}"),
            Self::Controller(n) => n.clone(),
        }
    }
    pub fn canonical(self) -> Self {
        match self {
            Self::Keyboard(n) => {
                let n = n.strip_prefix("Arrow").unwrap_or(&n).to_owned();
                Self::Keyboard(n.to_ascii_uppercase())
            }
            Self::Controller(n) => Self::Controller(
                match n.as_str() {
                    "Back" => "Select",
                    "Guide" => "Mode",
                    "LeftStick" => "LeftThumb",
                    "RightStick" => "RightThumb",
                    _ => &n,
                }
                .to_owned(),
            ),
        }
    }
    /// Host actions take precedence on every platform, including after config edits.
    pub fn conflict(&self, fullscreen: Option<&str>) -> Option<&'static str> {
        match self.clone().canonical() {
            Self::Keyboard(n) if matches!(n.as_str(), "ESCAPE" | "F10" | "F11" | "F12") => {
                Some("Reserved by PocketHLE (Escape / F10 / F11 / F12).")
            }
            Self::Controller(n) if matches!(n.as_str(), "Select" | "Start" | "Mode" | "Misc1") => {
                Some("Reserved by PocketHLE (fullscreen, filters, recording or system button).")
            }
            Self::Controller(n)
                if fullscreen.is_some_and(|f| {
                    Self::Controller(f.into()).canonical() == Self::Controller(n.clone())
                }) =>
            {
                Some("This controller input is assigned to fullscreen in PocketHLE settings.")
            }
            _ => None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridBinding {
    pub cell: usize,
    pub input: HostInput,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StylusGrid {
    pub columns: u8,
    pub rows: u8,
    pub bindings: Vec<GridBinding>,
}
impl Default for StylusGrid {
    fn default() -> Self {
        Self {
            columns: 5,
            rows: 4,
            bindings: Vec::new(),
        }
    }
}
impl StylusGrid {
    pub const SIZES: [(u8, u8); 3] = [(4, 3), (5, 4), (6, 5)];
    pub fn valid_size(&self) -> bool {
        Self::SIZES.contains(&(self.columns, self.rows))
    }
    pub fn resize(&mut self, columns: u8, rows: u8) {
        if Self::SIZES.contains(&(columns, rows)) && (columns, rows) != (self.columns, self.rows) {
            self.columns = columns;
            self.rows = rows;
            self.bindings.clear();
        }
    }
    pub fn bind(
        &mut self,
        cell: usize,
        input: HostInput,
        fullscreen: Option<&str>,
    ) -> Result<(), &'static str> {
        if !self.valid_size() || cell >= usize::from(self.columns) * usize::from(self.rows) {
            return Err("Invalid grid cell.");
        }
        if let Some(error) = input.conflict(fullscreen) {
            return Err(error);
        }
        let input = input.canonical();
        self.bindings
            .retain(|b| b.cell != cell && b.input.clone().canonical() != input);
        self.bindings.push(GridBinding { cell, input });
        Ok(())
    }
    pub fn cell_for(&self, input: &HostInput, fullscreen: Option<&str>) -> Option<usize> {
        if !self.valid_size() || input.conflict(fullscreen).is_some() {
            return None;
        }
        self.bindings
            .iter()
            .find(|b| b.input.clone().canonical() == input.clone().canonical())
            .map(|b| b.cell)
            .filter(|c| *c < usize::from(self.columns) * usize::from(self.rows))
    }
    pub fn point(&self, cell: usize) -> (u16, u16) {
        let cols = usize::from(self.columns.max(1));
        let rows = usize::from(self.rows.max(1));
        (
            (((cell % cols) * 2 + 1) * 10000 / (cols * 2)) as u16,
            (((cell / cols) * 2 + 1) * 10000 / (rows * 2)) as u16,
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchAction {
    Down(u16, u16),
    Move(u16, u16),
    Up(u16, u16),
}
/// One physical stylus: most recent press wins, release restores another held zone.
#[derive(Debug, Default)]
pub struct GridRouter {
    held: Vec<(HostInput, usize)>,
    point: Option<(u16, u16)>,
}
impl GridRouter {
    pub fn active(&self) -> bool {
        self.point.is_some()
    }
    pub fn held(&self, input: &HostInput) -> bool {
        self.held
            .iter()
            .any(|(i, _)| *i == input.clone().canonical())
    }
    pub fn input(
        &mut self,
        grid: &StylusGrid,
        input: HostInput,
        down: bool,
        fullscreen: Option<&str>,
    ) -> (bool, Option<TouchAction>) {
        let input = input.canonical();
        let Some(cell) = grid.cell_for(&input, fullscreen) else {
            return (false, None);
        };
        let old = self.held.iter().position(|(i, _)| *i == input);
        if down {
            if old.is_some() {
                return (true, None);
            }
            self.held.push((input, cell));
        } else if let Some(i) = old {
            self.held.remove(i);
        } else {
            return (true, None);
        }
        let next = self.held.last().map(|(_, c)| grid.point(*c));
        let action = match (self.point, next) {
            (None, Some((x, y))) => Some(TouchAction::Down(x, y)),
            (Some(p), Some((x, y))) if p != (x, y) => Some(TouchAction::Move(x, y)),
            (Some((x, y)), None) => Some(TouchAction::Up(x, y)),
            _ => None,
        };
        self.point = next;
        (true, action)
    }
    pub fn cancel(&mut self) -> Option<TouchAction> {
        self.held.clear();
        self.point.take().map(|(x, y)| TouchAction::Up(x, y))
    }
}
/// Normalized display point -> unrotated guest pixels, independent of host scaling.
pub fn guest_point(x: u16, y: u16, turns: u8, w: u32, h: u32) -> (u16, u16) {
    let x = u32::from(x.min(10000));
    let y = u32::from(y.min(10000));
    let (x, y) = match turns % 4 {
        1 => (y, 10000 - x),
        2 => (10000 - x, 10000 - y),
        3 => (10000 - y, x),
        _ => (x, y),
    };
    (
        (x * w.clamp(1, 32767) / 10000).min(w.clamp(1, 32767) - 1) as u16,
        (y * h.clamp(1, 32767) / 10000).min(h.clamp(1, 32767) - 1) as u16,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hold_restore_release_and_duplicate_edges() {
        let mut g = StylusGrid::default();
        let a = HostInput::Keyboard("A".into());
        let b = HostInput::Controller("South".into());
        g.bind(0, a.clone(), None).unwrap();
        g.bind(19, b.clone(), None).unwrap();
        let mut r = GridRouter::default();
        assert_eq!(
            r.input(&g, a.clone(), true, None),
            (true, Some(TouchAction::Down(1000, 1250)))
        );
        assert_eq!(r.input(&g, a.clone(), true, None), (true, None));
        assert_eq!(
            r.input(&g, b.clone(), true, None).1,
            Some(TouchAction::Move(9000, 8750))
        );
        assert_eq!(
            r.input(&g, b, false, None).1,
            Some(TouchAction::Move(1000, 1250))
        );
        assert_eq!(
            r.input(&g, a, false, None).1,
            Some(TouchAction::Up(1000, 1250))
        );
    }
    #[test]
    fn conflicts_rotation_and_resize() {
        let mut g = StylusGrid::default();
        for name in ["F10", "F11", "F12", "Escape"] {
            assert!(g.bind(0, HostInput::Keyboard(name.into()), None).is_err());
        }
        for name in ["Start", "Select", "Guide", "Misc1"] {
            assert!(g.bind(0, HostInput::Controller(name.into()), None).is_err());
        }
        assert!(g
            .bind(0, HostInput::Controller("South".into()), Some("South"))
            .is_err());
        g.bind(0, HostInput::Keyboard("ArrowUp".into()), None)
            .unwrap();
        assert_eq!(g.cell_for(&HostInput::Keyboard("Up".into()), None), Some(0));
        assert_eq!(guest_point(2500, 7500, 0, 240, 320), (60, 240));
        assert_eq!(guest_point(2500, 7500, 1, 240, 320), (180, 240));
        assert_eq!(guest_point(2500, 7500, 2, 240, 320), (180, 80));
        assert_eq!(guest_point(2500, 7500, 3, 240, 320), (60, 80));
        g.resize(4, 3);
        assert!(g.bindings.is_empty());
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(g, serde_json::from_str(&json).unwrap());
    }
    #[test]
    fn old_settings_migrate_and_explicit_mode_wins() {
        use crate::GameSettings;
        let mut value = serde_json::to_value(GameSettings::default()).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("pocketpc_controls");
        obj.remove("pocketpc_stylus_grid");
        obj.insert("pocketpc_touch_steering".into(), true.into());
        let mut settings: GameSettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.controls_mode(), ControlsMode::Zones);
        settings.set_controls_mode(ControlsMode::Grid);
        assert!(!settings.pocketpc_touch_steering);
        let roundtrip: GameSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(roundtrip.controls_mode(), ControlsMode::Grid);
        assert_eq!(
            GameSettings::default().controls_mode(),
            ControlsMode::Normal
        );
    }
    #[test]
    fn centers_and_rotation_are_bounded_at_qvga_and_vga() {
        for (columns, rows) in StylusGrid::SIZES {
            let g = StylusGrid {
                columns,
                rows,
                bindings: Vec::new(),
            };
            for cell in 0..usize::from(columns) * usize::from(rows) {
                let (x, y) = g.point(cell);
                for turns in 0..4 {
                    for (w, h) in [(240, 320), (320, 240), (480, 640), (640, 480)] {
                        let (gx, gy) = guest_point(x, y, turns, w, h);
                        assert!(u32::from(gx) < w && u32::from(gy) < h);
                    }
                }
            }
        }
    }
    #[test]
    fn only_bound_inputs_are_muted_and_cancellation_releases_once() {
        let mut g = StylusGrid::default();
        let a = HostInput::Controller("RightStickLeft".into());
        g.bind(3, a.clone(), None).unwrap();
        let mut r = GridRouter::default();
        assert_eq!(
            r.input(
                &g,
                HostInput::Controller("RightStickRight".into()),
                true,
                None
            ),
            (false, None)
        );
        assert_eq!(
            r.input(&g, a.clone(), true, Some("RightStickLeft")),
            (false, None)
        );
        assert!(r.input(&g, a.clone(), true, None).1.is_some());
        assert!(r.active());
        assert!(r.cancel().is_some());
        assert!(!r.active());
        assert!(r.cancel().is_none());
        assert_eq!(r.input(&g, a, false, None), (true, None));
    }
}
