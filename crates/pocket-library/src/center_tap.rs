//! Host-only, one-shot stylus pulse. The command tag is never a guest VK.
pub const COMMAND: u16 = 0x1000;
pub const DURATION_MS: u64 = 60;

#[derive(Default)]
pub struct CenterTap {
    held: Option<(u64, u16, u16)>,
}

impl CenterTap {
    pub fn active(&self) -> bool {
        self.held.is_some()
    }

    pub fn press(&mut self, now: u64, width: u32, height: u32) -> Option<(u16, u16)> {
        if self.active() || width == 0 || height == 0 {
            return None;
        }
        let x = (width / 2).min(u16::MAX as u32) as u16;
        let y = (height / 2).min(u16::MAX as u32) as u16;
        self.press_at(now, x, y)
    }

    pub fn press_at(&mut self, now: u64, x: u16, y: u16) -> Option<(u16, u16)> {
        if self.active() {
            return None;
        }
        self.held = Some((now.saturating_add(DURATION_MS), x, y));
        Some((x, y))
    }

    pub fn release_due(&mut self, now: u64) -> Option<(u16, u16)> {
        if self.held.is_some_and(|(due, _, _)| now >= due) {
            self.cancel()
        } else {
            None
        }
    }

    pub fn cancel(&mut self) -> Option<(u16, u16)> {
        self.held.take().map(|(_, x, y)| (x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_pulse_at_logical_center_ignores_repeat_and_releases_once() {
        let mut tap = CenterTap::default();
        assert_eq!(tap.press(100, 240, 320), Some((120, 160)));
        assert_eq!(tap.press(110, 640, 480), None);
        assert_eq!(tap.release_due(159), None);
        assert_eq!(tap.release_due(160), Some((120, 160)));
        assert_eq!(tap.release_due(200), None);
        assert_eq!(tap.press(200, 640, 480), Some((320, 240)));
        assert_eq!(tap.cancel(), Some((320, 240)));
        assert!(!tap.active());
        assert_eq!(tap.cancel(), None);
        assert_eq!(tap.press(0, 0, 320), None);
    }
}
