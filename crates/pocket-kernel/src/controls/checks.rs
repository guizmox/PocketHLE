//! One check-state owner for messages, DDX, pointer/keyboard input and snapshots.
use super::*;
use std::collections::HashSet;
impl ChildWindow {
    pub fn check_state(&self) -> u32 {
        if self.indeterminate {
            2
        } else {
            u32::from(self.checked)
        }
    }
    pub fn set_check_state(&mut self, value: u32) {
        let max = match self.button_type() {
            Some(BS_3STATE | BS_AUTO3STATE) => 2,
            Some(BS_CHECKBOX | BS_AUTOCHECKBOX | BS_RADIOBUTTON | BS_AUTORADIOBUTTON) => 1,
            _ => 0,
        };
        let value = value.min(max);
        self.checked = value != 0;
        self.indeterminate = value == 2;
    }
    pub(super) fn advance_check(&mut self) {
        let modulus = if self.button_type() == Some(BS_AUTO3STATE) {
            3
        } else {
            2
        };
        self.set_check_state((self.check_state() + 1) % modulus);
    }
}
impl Controls {
    pub fn snapshot_checks(&self) -> HashSet<u32> {
        self.children
            .iter()
            .filter(|c| c.indeterminate)
            .map(|c| c.hwnd)
            .collect()
    }
    pub fn validate_checks(&self, checks: &HashSet<u32>) -> Result<(), String> {
        if checks.iter().any(|h| {
            self.get(*h).is_none_or(|c| {
                !c.checked || !matches!(c.button_type(), Some(BS_3STATE | BS_AUTO3STATE))
            })
        }) {
            return Err("checkpoint indeterminate state has no matching three-state button".into());
        }
        Ok(())
    }
    pub fn restore_checks(&mut self, checks: HashSet<u32>) {
        for child in &mut self.children {
            child.indeterminate = checks.contains(&child.hwnd);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_three_state_cycles_match_for_keyboard_and_pointer_and_manual_checks_do_not_toggle()
    {
        let mut c = Controls::default();
        let h = c.create(
            7,
            ControlClass::Button,
            1,
            "".into(),
            0x50000006,
            0,
            0,
            40,
            20,
        );
        c.focus = h;
        for value in [1, 2, 0, 1] {
            c.key_down(0x20);
            assert_eq!(c.get(h).unwrap().check_state(), value);
        }
        c.pointer_down(4, 4);
        c.pointer_up(4, 4);
        assert_eq!(c.get(h).unwrap().check_state(), 2);
        let states = c.snapshot_checks();
        assert!(c.validate_checks(&states).is_ok());
        let bytes = serde_json::to_vec(&c).unwrap();
        let mut restored: Controls = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.get(h).unwrap().check_state(), 1);
        restored.restore_checks(states);
        assert_eq!(restored.get(h).unwrap().check_state(), 2);
        restored.get_mut(h).unwrap().style = 0x50000002;
        assert!(restored
            .validate_checks(&restored.snapshot_checks())
            .is_err());
        restored.get_mut(h).unwrap().set_check_state(2);
        assert_eq!(restored.get(h).unwrap().check_state(), 1);
        restored.focus = h;
        restored.key_down(0x20);
        assert_eq!(restored.get(h).unwrap().check_state(), 1);
    }
}
