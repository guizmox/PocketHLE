//! Optional long-press guest bindings. Ordinary bindings remain immediate unless
//! the same physical input has a long action. Time is supplied by the frontend.
use crate::GuestButton;
use std::collections::BTreeMap;
pub const HOLD_MS: u64 = 650;
pub const TAP_MS: u64 = 80;
pub fn binding_key(gizmondo: bool, controller: bool, input: &str) -> String {
    let input = if controller {
        match input {
            "Back" => "Select",
            "Guide" => "Mode",
            "LeftStick" => "LeftThumb",
            "RightStick" => "RightThumb",
            _ => input,
        }
        .to_owned()
    } else {
        match input {
            "ArrowUp" => "Up",
            "ArrowDown" => "Down",
            "ArrowLeft" => "Left",
            "ArrowRight" => "Right",
            "Minus" => "-",
            "Plus" => "+",
            "Equals" => "=",
            "OpenBracket" => "[",
            "CloseBracket" => "]",
            "Comma" => ",",
            "Period" => ".",
            "Slash" => "/",
            "Semicolon" => ";",
            "Colon" => ":",
            "Backslash" => "\\",
            "Pipe" => "|",
            "Questionmark" => "?",
            "Backtick" => "`",
            _ => input,
        }
        .to_ascii_lowercase()
    };
    format!(
        "{}:{}:{}",
        if gizmondo { "gizmondo" } else { "pocketpc" },
        if controller { "controller" } else { "keyboard" },
        input
    )
}
#[derive(Debug)]
struct Press {
    at: u64,
    short: Option<u16>,
    long: u16,
    fired: bool,
}
#[derive(Debug, Default)]
pub struct GestureTracker {
    presses: BTreeMap<String, Press>,
    pulses: Vec<(u16, u64)>,
    immediate: BTreeMap<String, Option<u16>>,
    held: BTreeMap<u16, usize>,
}
impl GestureTracker {
    fn change(&mut self, vk: u16, down: bool, out: &mut Vec<(u16, bool)>) {
        let count = self.held.entry(vk).or_default();
        if down {
            *count += 1;
            if *count == 1 {
                out.push((vk, true));
            }
        } else if *count > 0 {
            *count -= 1;
            if *count == 0 {
                out.push((vk, false));
            }
        }
    }
    pub fn immediate_edge(
        &mut self,
        token: String,
        down: bool,
        vk: Option<u16>,
    ) -> Vec<(u16, bool)> {
        let mut out = Vec::new();
        if down {
            if !self.immediate.contains_key(&token) {
                self.immediate.insert(token, vk);
                if let Some(vk) = vk {
                    self.change(vk, true, &mut out);
                }
            }
        } else if let Some(Some(vk)) = self.immediate.remove(&token) {
            self.change(vk, false, &mut out);
        }
        out
    }
    pub fn edge(
        &mut self,
        token: String,
        down: bool,
        now: u64,
        short: Option<u16>,
        long: u16,
    ) -> Vec<(u16, bool)> {
        let late = !down
            && self
                .presses
                .get(&token)
                .is_some_and(|p| !p.fired && now.saturating_sub(p.at) >= HOLD_MS);
        let mut out = self.tick(now);
        if down {
            self.presses.entry(token).or_insert(Press {
                at: now,
                short,
                long,
                fired: false,
            });
        } else if let Some(p) = self.presses.remove(&token) {
            if p.fired {
                if late {
                    self.pulses.push((p.long, now + TAP_MS));
                } else {
                    self.change(p.long, false, &mut out);
                }
            } else if let Some(vk) = p.short {
                self.change(vk, true, &mut out);
                self.pulses.push((vk, now + TAP_MS));
            }
        }
        out
    }
    pub fn tick(&mut self, now: u64) -> Vec<(u16, bool)> {
        let mut out = Vec::new();
        let mut fire = Vec::new();
        for p in self.presses.values_mut() {
            if !p.fired && now.saturating_sub(p.at) >= HOLD_MS {
                p.fired = true;
                fire.push(p.long);
            }
        }
        for vk in fire {
            self.change(vk, true, &mut out);
        }
        let mut expired = Vec::new();
        self.pulses.retain(|(vk, at)| {
            if now >= *at {
                expired.push(*vk);
                false
            } else {
                true
            }
        });
        for vk in expired {
            self.change(vk, false, &mut out);
        }
        out
    }
    pub fn cancel_prefix(&mut self, prefix: &str) -> Vec<(u16, bool)> {
        let keys: Vec<_> = self
            .presses
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        let mut out = Vec::new();
        for key in keys {
            if let Some(p) = self.presses.remove(&key) {
                if p.fired {
                    self.change(p.long, false, &mut out);
                }
            }
        }
        let keys: Vec<_> = self
            .immediate
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        for key in keys {
            out.extend(self.immediate_edge(key, false, None));
        }
        out
    }
    pub fn clear(&mut self) -> Vec<(u16, bool)> {
        let out = self
            .held
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(vk, _)| (*vk, false))
            .collect();
        *self = Self::default();
        out
    }
    pub fn active(&self) -> bool {
        !self.presses.is_empty() || !self.pulses.is_empty()
    }
}
pub type LongPressBindings = BTreeMap<String, GuestButton>;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_only_overrides_are_immediate_and_reference_counted() {
        let mut t = GestureTracker::default();
        assert_eq!(t.immediate_edge("a".into(), true, Some(1)), vec![(1, true)]);
        assert!(t.immediate_edge("a".into(), true, Some(1)).is_empty());
        assert!(t.immediate_edge("b".into(), true, Some(1)).is_empty());
        assert!(t.immediate_edge("a".into(), false, None).is_empty());
        assert_eq!(t.cancel_prefix("b"), vec![(1, false)]);
    }
    #[test]
    fn explicit_unbound_does_not_emit_input() {
        let mut t = GestureTracker::default();
        assert!(t.immediate_edge("a".into(), true, None).is_empty());
        assert!(t.immediate_edge("a".into(), false, None).is_empty());
        assert!(t.clear().is_empty());
    }
    #[test]
    fn short_waits_for_release_and_pulses() {
        let mut t = GestureTracker::default();
        assert!(t.edge("k".into(), true, 0, Some(1), 2).is_empty());
        assert_eq!(t.edge("k".into(), false, 649, Some(1), 2), vec![(1, true)]);
        assert!(t.tick(728).is_empty());
        assert_eq!(t.tick(729), vec![(1, false)]);
    }
    #[test]
    fn long_suppresses_short_and_repeats() {
        let mut t = GestureTracker::default();
        t.edge("k".into(), true, 0, Some(1), 2);
        t.edge("k".into(), true, 600, Some(1), 2);
        assert_eq!(t.tick(650), vec![(2, true)]);
        assert!(t.tick(900).is_empty());
        assert_eq!(
            t.edge("k".into(), false, 1000, Some(1), 2),
            vec![(2, false)]
        );
    }
    #[test]
    fn delayed_release_still_chooses_long() {
        let mut t = GestureTracker::default();
        t.edge("k".into(), true, 0, Some(1), 2);
        assert_eq!(t.edge("k".into(), false, 650, Some(1), 2), vec![(2, true)]);
        assert_eq!(t.tick(730), vec![(2, false)]);
    }
    #[test]
    fn cancellation_does_not_fire_short_and_sources_are_independent() {
        let mut t = GestureTracker::default();
        t.edge("pad:1:a".into(), true, 0, Some(1), 2);
        t.edge("pad:2:a".into(), true, 0, Some(1), 2);
        assert_eq!(t.tick(650), vec![(2, true)]);
        assert!(t.cancel_prefix("pad:1:").is_empty());
        assert_eq!(t.clear(), vec![(2, false)]);
        assert!(!t.active());
        t.edge("k".into(), true, 0, Some(1), 2);
        assert!(t.clear().is_empty());
    }
    #[test]
    fn keyboard_aliases_are_portable() {
        assert_eq!(binding_key(false, false, "Minus"), "pocketpc:keyboard:-");
        assert_eq!(
            binding_key(false, false, "ArrowUp"),
            binding_key(false, false, "Up")
        );
        assert_eq!(
            binding_key(true, true, "LeftStick"),
            "gizmondo:controller:LeftThumb"
        );
    }
    #[test]
    fn binding_config_round_trips() {
        let mut c = crate::LauncherConfig::default();
        c.long_press_bindings
            .insert(binding_key(false, false, "X"), GuestButton::Action);
        let r: crate::LauncherConfig =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(r.long_press_bindings, c.long_press_bindings);
        assert_eq!(r.keybindings, c.keybindings);
    }
    #[test]
    fn profile_keys_are_separate() {
        assert_ne!(
            binding_key(true, true, "South"),
            binding_key(false, true, "South")
        );
    }
}
