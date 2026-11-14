//! Shared text-input intent for host keyboards and guest message delivery.
use crate::{controls::ControlClass, gapi, KernelState};

impl KernelState {
    fn text_target_is_active(&self, hwnd: u32) -> bool {
        let mut current = hwnd;
        for _ in 0..64 {
            if self.controls.get(current).is_some_and(|c| !c.enabled)
                || self
                    .window_styles
                    .get(&current)
                    .is_some_and(|s| s.0 & 0x0800_0000 != 0)
            {
                return false;
            }
            if current == self.active_window || self.active_window == 0 {
                return true;
            }
            current = if let Some(control) = self.controls.get(current) {
                control.parent
            } else if self
                .window_styles
                .get(&current)
                .is_some_and(|s| s.0 & 0x4000_0000 != 0)
            {
                self.window_parents.get(&current).copied().unwrap_or(0)
            } else {
                return false;
            };
            if current == 0 {
                return false;
            }
        }
        false
    }

    pub fn editable_text_window(&self, hwnd: u32) -> bool {
        let editable = if let Some(control) = self.controls.get(hwnd) {
            control.is_text_editable() && control.enabled
        } else {
            // Pocket Word uses the image's native RichEdit window procedure,
            // rather than an HLE EDIT. Its WM_CHAR input must use the same
            // committed Unicode text path as ordinary edits.
            self.window_classes.get(&hwnd).is_some_and(|class| {
                ["RichEdit", "RichEdit20W", "RichEdit20A", "RichEdit50W"]
                    .iter()
                    .any(|name| class.eq_ignore_ascii_case(name))
            }) && self.window_procs.get(&hwnd).is_some_and(|proc| *proc != 0)
                && self
                    .window_styles
                    .get(&hwnd)
                    .is_none_or(|style| style.0 & (0x0800_0000 | 0x800) == 0)
        };
        editable && self.text_target_is_active(hwnd) && self.window_tree_visible(hwnd, |h| h != 0)
    }

    pub fn text_input_target(&self) -> Option<u32> {
        if let Some(view) = &self.console.view {
            if self.active_window == view.hwnd
                && self.window_tree_visible(view.hwnd, |_| false)
                && self.modal.is_none()
            {
                return Some(view.hwnd);
            }
        }
        if self.gapi_action_key != gapi::VK_RETURN {
            return None;
        }
        let candidates = [
            self.controls.focus,
            if self.shell_keyboard.sip {
                self.shell_keyboard.target
            } else {
                0
            },
        ];
        for hwnd in candidates {
            if hwnd != 0 && self.editable_text_window(hwnd) {
                return Some(hwnd);
            }
        }
        if self.shell_keyboard.sip && self.shell_keyboard.visible {
            for hwnd in [self.controls.focus, self.active_window] {
                if hwnd != 0
                    && self
                        .controls
                        .get(hwnd)
                        .map_or(self.window_classes.contains_key(&hwnd), |c| {
                            c.class != ControlClass::Edit && c.enabled
                        })
                    && self.window_tree_visible(hwnd, |h| h != 0)
                    && self.text_target_is_active(hwnd)
                {
                    return Some(hwnd);
                }
            }
        }
        None
    }

    pub fn accepts_text_input(&self) -> bool {
        self.text_input_target().is_some()
    }
}
