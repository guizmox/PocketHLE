//! Host input adaptation shared by Desktop and Android before guest delivery.
use pocket_core::kernel::InputEvent;

/// The setting changes the established Tab/Escape controls, not the GAPI key
/// table or any other key. Configure once per launch, including child processes.
/// Both edges must pass through the same mapping so releases cannot get stuck.
pub fn phone_softkeys(event: InputEvent, enabled: bool) -> InputEvent {
    if !enabled {
        return event;
    }
    let key = |vk| match vk {
        0x09 => 0x70,
        0x1b => 0x71,
        _ => vk,
    };
    match event {
        InputEvent::KeyDown { vk } => InputEvent::KeyDown { vk: key(vk) },
        InputEvent::KeyUp { vk } => InputEvent::KeyUp { vk: key(vk) },
        _ => event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_context_keys_map_both_edges_without_changing_other_inputs() {
        for enabled in [false, true] {
            for (vk, mapped) in [
                (0x09, 0x70),
                (0x1b, 0x71),
                (0x0d, 0x0d),
                (0xc1, 0xc1),
                (0x26, 0x26),
            ] {
                let expected = if enabled { mapped } else { vk };
                assert!(
                    matches!(phone_softkeys(InputEvent::KeyDown { vk }, enabled), InputEvent::KeyDown { vk } if vk == expected)
                );
                assert!(
                    matches!(phone_softkeys(InputEvent::KeyUp { vk }, enabled), InputEvent::KeyUp { vk } if vk == expected)
                );
            }
            assert!(matches!(
                phone_softkeys(InputEvent::PointerDown { x: 12, y: 34 }, enabled),
                InputEvent::PointerDown { x: 12, y: 34 }
            ));
        }
    }
}

/// Per-session physical tilt conversion. Calibration and held keys are host input
/// state: they are deliberately absent from snapshots. No sensor means no keys.
#[derive(Default)]
pub struct AccelerometerKeys {
    mode: pocket_library::AccelerometerArrows,
    neutral: Option<[f32; 3]>,
    filtered: Option<[f32; 3]>,
    last_poll: Option<std::time::Instant>,
    turns: u8,
    sensor: u8,
    physical: u8,
    aim: Option<pocket_library::stylus::StylusBindings>,
    size: (u32, u32),
    sensor_aim: (i16, i16),
    physical_aim: (i16, i16),
    pointer_down: bool,
    zones: bool,
    zone_stick_down: bool,
}
impl AccelerometerKeys {
    const KEYS: [u16; 4] = [0x26, 0x27, 0x28, 0x25];
    pub fn configure(&mut self, mode: pocket_library::AccelerometerArrows, turns: u8) {
        *self = Self {
            mode,
            turns: turns % 4,
            ..Self::default()
        };
    }
    /// A foreground process transfer already clears guest input. Discard the
    /// matching host-held state and calibrate again when this process returns.
    pub fn reset(&mut self) {
        let aim = self.aim.take();
        let size = self.size;
        let zones = self.zones;
        self.configure(self.mode, self.turns);
        self.aim = aim;
        self.size = size;
        self.zones = zones;
    }
    pub fn set_aim(
        &mut self,
        bindings: pocket_library::stylus::StylusBindings,
        size: (u32, u32),
        zones: bool,
    ) {
        self.zones = zones;
        self.aim = Some(bindings);
        self.size = size;
    }
    pub fn set_size(&mut self, size: (u32, u32)) {
        self.size = size;
    }
    fn combined_aim(&self) -> (i16, i16) {
        (
            self.physical_aim
                .0
                .saturating_add(self.sensor_aim.0)
                .clamp(-1200, 1200),
            self.physical_aim
                .1
                .saturating_add(self.sensor_aim.1)
                .clamp(-1200, 1200),
        )
    }
    /// Merge arrows with keyboard/gamepad input so releasing one source cannot
    /// release a direction still held by the other. Preserve keyboard repeats.
    pub fn accept(&mut self, event: InputEvent) -> Option<InputEvent> {
        if !self.mode.enabled() {
            return Some(event);
        }
        if let InputEvent::KeyboardRotation { turns } = event {
            if self.turns != turns % 4 {
                self.turns = turns % 4;
                self.neutral = None;
                self.filtered = None;
            }
        }
        if self.aim.is_some() {
            match event {
                InputEvent::StylusAim { x, y } => {
                    self.physical_aim = (x, y);
                    let (x, y) = self.combined_aim();
                    return Some(InputEvent::StylusAim { x, y });
                }
                InputEvent::KeyDown { vk } | InputEvent::KeyUp { vk } if self.zones => {
                    if let Some(i) = Self::KEYS.iter().position(|key| *key == vk) {
                        if matches!(event, InputEvent::KeyDown { .. }) {
                            self.physical |= 1 << i;
                        } else {
                            self.physical &= !(1 << i);
                        }
                    }
                }
                InputEvent::TouchSteeringPosition { x, y } if self.zones => {
                    self.zone_stick_down = x.unsigned_abs() > 180 || y.unsigned_abs() > 180
                }
                InputEvent::PointerDown { .. } => self.pointer_down = true,
                InputEvent::PointerUp { .. } => self.pointer_down = false,
                _ => {}
            }
            return Some(event);
        }
        match event {
            InputEvent::KeyDown { vk } | InputEvent::KeyUp { vk } => {
                if let Some(index) = Self::KEYS.iter().position(|key| *key == vk) {
                    let bit = 1 << index;
                    if matches!(event, InputEvent::KeyDown { .. }) {
                        self.physical |= bit;
                    } else {
                        self.physical &= !bit;
                        if self.sensor & bit != 0 {
                            return None;
                        }
                    }
                }
            }
            _ => {}
        }
        Some(event)
    }
    pub fn poll(&mut self, active: bool, output: &mut std::collections::VecDeque<InputEvent>) {
        if !self.mode.enabled() {
            return;
        }
        self.sample(
            if active {
                pocket_core::kernel::accelerometer::sample()
            } else {
                None
            },
            std::time::Instant::now(),
            output,
        );
    }
    fn sample(
        &mut self,
        sample: Option<[f32; 3]>,
        now: std::time::Instant,
        output: &mut std::collections::VecDeque<InputEvent>,
    ) {
        let sample = sample
            .filter(|v| v.iter().all(|x| x.is_finite()))
            .and_then(|v| {
                let length = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                (length > 1.0).then(|| v.map(|x| x / length))
            });
        let mut desired = 0;
        let old_aim = self.combined_aim();
        self.sensor_aim = (0, 0);
        if let Some(sample) = sample {
            let neutral = *self.neutral.get_or_insert(sample);
            let alpha = self.last_poll.map_or(1.0, |last| {
                1.0 - (-(now.saturating_duration_since(last).as_secs_f32()) / 0.045).exp()
            });
            let filtered = self.filtered.get_or_insert(sample);
            for axis in 0..3 {
                filtered[axis] += alpha * (sample[axis] - filtered[axis]);
            }
            let threshold = match self.mode {
                pocket_library::AccelerometerArrows::Low => 20.0f32.to_radians(),
                pocket_library::AccelerometerArrows::Medium => 12.0f32.to_radians(),
                pocket_library::AccelerometerArrows::High => 6.0f32.to_radians(),
                pocket_library::AccelerometerArrows::Disabled => return,
            };
            // Angular displacement works both flat and upright: a Y-component
            // dead zone alone barely responds to pitch on a vertical phone.
            let roll = |v: [f32; 3]| v[0].atan2(v[1].hypot(v[2]));
            let pitch = |v: [f32; 3]| v[1].atan2(v[2]);
            let dx = roll(*filtered) - roll(neutral);
            let dy = (pitch(*filtered) - pitch(neutral) + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            let directions = [dy, -dx, -dy, dx];
            if let Some(bindings) = self.aim.as_ref() {
                if !self.pointer_down
                    && !(self.zones && (self.physical != 0 || self.zone_stick_down))
                {
                    // Continuous aim, with a small neutral band; full travel at
                    // twice the corresponding digital activation threshold.
                    let strengths = [directions[0], directions[2], directions[3], directions[1]]
                        .map(|v| ((v - threshold * 0.4) / (threshold * 1.6)).clamp(0.0, 1.0));
                    self.sensor_aim =
                        bindings.velocity_for_size(strengths, self.turns, self.size.0, self.size.1);
                }
            } else {
                for (direction, value) in directions.into_iter().enumerate() {
                    let guest = (direction + 4 - usize::from(self.turns)) % 4;
                    let bit = 1 << guest;
                    if value > threshold * if self.sensor & bit != 0 { 0.6 } else { 1.0 } {
                        desired |= bit;
                    }
                }
            }
            self.last_poll = Some(now);
        } else {
            self.neutral = None;
            self.filtered = None;
            self.last_poll = None;
        }
        if self.aim.is_some() {
            let (x, y) = self.combined_aim();
            if old_aim != (x, y) {
                output.push_back(InputEvent::StylusAim { x, y });
            }
            return;
        }
        let before = self.sensor | self.physical;
        self.sensor = desired;
        let after = self.sensor | self.physical;
        for (i, vk) in Self::KEYS.into_iter().enumerate() {
            let bit = 1 << i;
            if (before ^ after) & bit != 0 {
                output.push_back(if after & bit != 0 {
                    InputEvent::KeyDown { vk }
                } else {
                    InputEvent::KeyUp { vk }
                });
            }
        }
    }
}

#[cfg(test)]
mod accelerometer_tests {
    use super::*;
    use pocket_library::AccelerometerArrows;
    use std::{
        collections::VecDeque,
        time::{Duration, Instant},
    };
    #[test]
    fn foreground_reset_keeps_policy_but_discards_old_holds_and_neutral() {
        let mut keys = AccelerometerKeys::default();
        keys.configure(AccelerometerArrows::High, 0);
        let now = Instant::now();
        let mut output = VecDeque::new();
        keys.sample(Some([0.0, 0.0, 9.8]), now, &mut output);
        keys.sample(
            Some([-3.0, 0.0, 9.3]),
            now + Duration::from_secs(1),
            &mut output,
        );
        assert!(!output.is_empty());
        output.clear();
        keys.reset();
        keys.sample(
            Some([-3.0, 0.0, 9.3]),
            now + Duration::from_secs(2),
            &mut output,
        );
        assert!(output.is_empty());
        assert_eq!(keys.mode, AccelerometerArrows::High);
    }
    #[test]
    fn zones_keep_priority_over_motion_aiming() {
        for event in [
            InputEvent::KeyDown { vk: 0x26 },
            InputEvent::TouchSteeringPosition { x: 500, y: 0 },
        ] {
            let mut keys = AccelerometerKeys::default();
            keys.configure(AccelerometerArrows::Medium, 0);
            keys.set_aim(
                pocket_library::stylus::StylusBindings::default(),
                (240, 320),
                true,
            );
            let mut output = VecDeque::new();
            let now = Instant::now();
            keys.sample(Some([0.0, 0.0, 9.8]), now, &mut output);
            keys.accept(event);
            keys.sample(
                Some([-3.0, 0.0, 9.3]),
                now + Duration::from_secs(1),
                &mut output,
            );
            assert!(output.is_empty());
        }
    }
    #[test]
    fn pitch_works_with_a_phone_held_upright() {
        let now = Instant::now();
        let mut keys = AccelerometerKeys::default();
        keys.configure(AccelerometerArrows::Medium, 0);
        let mut output = VecDeque::new();
        keys.sample(Some([0.0, 9.8, 0.0]), now, &mut output);
        keys.sample(
            Some([0.0, 9.2, 3.35]),
            now + Duration::from_secs(1),
            &mut output,
        );
        assert!(matches!(
            output.pop_front(),
            Some(InputEvent::KeyDown { vk: 0x28 })
        ));
    }
    #[test]
    fn stick_only_adjusts_relative_aim_and_yields_to_direct_touch() {
        let now = Instant::now();
        let mut keys = AccelerometerKeys::default();
        keys.configure(AccelerometerArrows::Medium, 1);
        keys.set_aim(
            pocket_library::stylus::StylusBindings::default(),
            (480, 640),
            false,
        );
        let mut output = VecDeque::new();
        keys.sample(Some([0.0, 0.0, 9.8]), now, &mut output);
        keys.sample(
            Some([-3.0, 0.0, 9.3]),
            now + Duration::from_secs(1),
            &mut output,
        );
        assert!(matches!(output.pop_front(),Some(InputEvent::StylusAim { x:0,y }) if y < 0));
        assert!(output.is_empty());
        keys.accept(InputEvent::PointerDown { x: 10, y: 20 });
        keys.sample(
            Some([-3.0, 0.0, 9.3]),
            now + Duration::from_secs(2),
            &mut output,
        );
        assert!(matches!(
            output.pop_front(),
            Some(InputEvent::StylusAim { x: 0, y: 0 })
        ));
        assert!(output.is_empty());
        keys.accept(InputEvent::PointerUp { x: 10, y: 20 });
        keys.sample(
            Some([-3.0, 0.0, 9.3]),
            now + Duration::from_secs(3),
            &mut output,
        );
        assert!(matches!(
            output.pop_front(),
            Some(InputEvent::StylusAim { .. })
        ));
        keys.sample(None, now + Duration::from_secs(4), &mut output);
        assert!(matches!(
            output.pop_front(),
            Some(InputEvent::StylusAim { x: 0, y: 0 })
        ));
        assert!(output.is_empty());
    }

    #[test]
    fn calibrated_tilt_rotates_and_does_not_release_another_input_source() {
        for turns in 0..4 {
            let mut keys = AccelerometerKeys::default();
            keys.configure(AccelerometerArrows::Medium, turns);
            let mut output = VecDeque::new();
            let now = Instant::now();
            keys.sample(Some([0.0, 0.0, 9.8]), now, &mut output);
            assert!(output.is_empty());
            keys.sample(
                Some([-5.0, 0.0, 8.4]),
                now + Duration::from_secs(1),
                &mut output,
            );
            let vk = AccelerometerKeys::KEYS[(5 - turns as usize) % 4];
            assert!(
                matches!(output.pop_front(), Some(InputEvent::KeyDown { vk: key }) if key == vk)
            );
            assert!(keys.accept(InputEvent::KeyDown { vk }).is_some());
            keys.sample(None, now + Duration::from_secs(2), &mut output);
            assert!(output.is_empty());
            assert!(keys.accept(InputEvent::KeyUp { vk }).is_some());
        }
    }
    #[test]
    fn sensitivity_hysteresis_and_sensor_loss_release_keys() {
        let now = Instant::now();
        for (mode, expected) in [
            (AccelerometerArrows::Low, false),
            (AccelerometerArrows::Medium, false),
            (AccelerometerArrows::High, true),
        ] {
            let mut keys = AccelerometerKeys::default();
            keys.configure(mode, 0);
            let mut output = VecDeque::new();
            keys.sample(Some([0.0, 0.0, 9.8]), now, &mut output);
            keys.sample(
                Some([-1.5, 0.0, 9.68]),
                now + Duration::from_secs(1),
                &mut output,
            );
            assert_eq!(!output.is_empty(), expected);
            output.clear();
            keys.sample(None, now + Duration::from_secs(2), &mut output);
            assert_eq!(!output.is_empty(), expected);
            if expected {
                assert!(matches!(
                    output.front(),
                    Some(InputEvent::KeyUp { vk: 0x27 })
                ));
            }
        }
    }
}
