//! Host-only velocity -> real, bounded stylus gestures. No guest keycodes.
use crate::InputEvent;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct StylusStick {
    velocity: (i16, i16),
    position: (f64, f64),
    last: Option<Instant>,
    down: bool,
    recenter: bool,
    size: (u32, u32),
}
impl StylusStick {
    pub fn velocity(&mut self, x: i16, y: i16) {
        self.velocity = (x.clamp(-1200, 1200), y.clamp(-1200, 1200));
    }
    pub fn cancel(&mut self) -> Option<InputEvent> {
        self.velocity = (0, 0);
        self.last = None;
        self.recenter = false;
        self.down.then(|| {
            self.down = false;
            InputEvent::PointerUp {
                x: self.position.0 as u16,
                y: self.position.1 as u16,
            }
        })
    }
    pub fn tick(&mut self, now: Instant, width: u32, height: u32) -> Option<InputEvent> {
        if self.velocity == (0, 0) || width < 3 || height < 3 {
            return self.cancel();
        }
        let size = (width.min(32767), height.min(32767));
        if self.down && (self.recenter || self.size != size) {
            self.down = false;
            self.recenter = false;
            return Some(InputEvent::PointerUp {
                x: self.position.0 as u16,
                y: self.position.1 as u16,
            });
        }
        if !self.down {
            self.size = size;
            self.position = ((size.0 / 2) as f64, (size.1 / 2) as f64);
            self.down = true;
            self.last = Some(now);
            return Some(InputEvent::PointerDown {
                x: self.position.0 as u16,
                y: self.position.1 as u16,
            });
        }
        let elapsed = now.saturating_duration_since(self.last.unwrap_or(now));
        if elapsed < Duration::from_millis(16) {
            return None;
        }
        self.last = Some(now);
        let dt = elapsed.min(Duration::from_millis(50)).as_secs_f64();
        let old = (self.position.0 as u16, self.position.1 as u16);
        self.position.0 =
            (self.position.0 + f64::from(self.velocity.0) * dt).clamp(1.0, (size.0 - 2) as f64);
        self.position.1 =
            (self.position.1 + f64::from(self.velocity.1) * dt).clamp(1.0, (size.1 - 2) as f64);
        self.recenter = self.position.0 <= 1.0
            || self.position.1 <= 1.0
            || self.position.0 >= (size.0 - 2) as f64
            || self.position.1 >= (size.1 - 2) as f64;
        let new = (self.position.0 as u16, self.position.1 as u16);
        (old != new).then_some(InputEvent::PointerMove { x: new.0, y: new.1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gestures_recenter_and_stop_without_crossing_or_catching_up() {
        let mut stick = StylusStick::default();
        let now = Instant::now();
        stick.velocity(1200, 0);
        assert_eq!(
            stick.tick(now, 100, 80),
            Some(InputEvent::PointerDown { x: 50, y: 40 })
        );
        assert_eq!(
            stick.tick(now + Duration::from_secs(10), 100, 80),
            Some(InputEvent::PointerMove { x: 98, y: 40 })
        );
        assert_eq!(
            stick.tick(now + Duration::from_secs(10), 100, 80),
            Some(InputEvent::PointerUp { x: 98, y: 40 })
        );
        assert_eq!(
            stick.tick(now + Duration::from_secs(10), 100, 80),
            Some(InputEvent::PointerDown { x: 50, y: 40 })
        );
        stick.velocity(0, 0);
        assert_eq!(
            stick.tick(now, 100, 80),
            Some(InputEvent::PointerUp { x: 50, y: 40 })
        );
        assert_eq!(stick.tick(now, 100, 80), None);
    }
}
