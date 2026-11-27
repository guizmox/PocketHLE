//! Edit selection/limits are control-owned; the optional snapshot record keeps
//! existing serialized ChildWindow layouts unchanged.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EditState {
    pub selection: Option<(u32, u32)>,
    pub limit: u32,
    pub modified: bool,
    pub undo: Option<(String, Option<(u32, u32)>)>,
    pub tab_stops: Vec<i32>,
    pub first_line: u32,
    pub scroll_x: i32,
}
impl Default for EditState {
    fn default() -> Self {
        Self {
            selection: None,
            limit: 30000,
            modified: false,
            undo: None,
            tab_stops: Vec::new(),
            first_line: 0,
            scroll_x: 0,
        }
    }
}
impl EditState {
    pub fn undo(&mut self, text: &mut String) -> bool {
        let Some((previous, selection)) = self.undo.take() else {
            return false;
        };
        self.undo = Some((std::mem::replace(text, previous), self.selection));
        self.selection = selection;
        self.modified = true;
        true
    }
    pub fn reset(&mut self) {
        self.selection = None;
        self.modified = false;
        self.undo = None;
        self.first_line = 0;
        self.scroll_x = 0;
    }
    pub fn range(&self, text: &str) -> (u32, u32) {
        let len = text.encode_utf16().count() as u32;
        let (a, b) = self.selection.unwrap_or((len, len));
        (a.min(b).min(len), a.max(b).min(len))
    }
    pub fn select(&mut self, text: &str, start: u32, end: u32) {
        let len = text.encode_utf16().count() as u32;
        let (a, b) = if start == u32::MAX {
            let (_, b) = self.range(text);
            (b, b)
        } else {
            (start.min(len), end.min(len))
        };
        self.selection = Some((a, b));
    }
    pub fn replace(&mut self, text: &mut String, insertion: &str, backspace: bool) -> bool {
        let old = text.clone();
        let selection = self.selection;
        let units: Vec<_> = text.encode_utf16().collect();
        let (mut a, b) = self.range(text);
        if backspace && a == b {
            a = previous_boundary(&units, a as usize) as u32;
        }
        let room = self.limit.saturating_sub(units.len() as u32 - (b - a)) as usize;
        let mut insert: Vec<_> = insertion.encode_utf16().take(room).collect();
        if insert.last().is_some_and(|c| (0xd800..=0xdbff).contains(c)) {
            insert.pop();
        }
        let end = a + insert.len() as u32;
        let mut result = units[..a as usize].to_vec();
        result.extend(insert);
        result.extend_from_slice(&units[b as usize..]);
        *text = String::from_utf16_lossy(&result);
        self.selection = Some((end, end));
        if *text != old {
            self.undo = Some((old, selection));
            self.modified = true;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_extensions_require_a_live_edit_owner() {
        use crate::controls::{ControlClass, Controls};
        let mut controls = Controls::default();
        let edit = controls.create(7, ControlClass::Edit, 1, "".into(), 0, 0, 0, 20, 20);
        let button = controls.create(7, ControlClass::Button, 2, "".into(), 0, 0, 0, 20, 20);
        for (hwnd, expected) in [(edit, true), (button, false), (0xdeadffff, false)] {
            let values = [(hwnd, EditState::default())].into_iter().collect();
            assert_eq!(controls.validate_edits(&values).is_ok(), expected);
        }
    }
    #[test]
    fn selected_utf16_ranges_replace_text_and_limits_preserve_whole_surrogates() {
        let mut edit = EditState::default();
        let mut text = "A😀B".to_string();
        edit.select(&text, 1, 3);
        assert!(edit.replace(&mut text, "X", false));
        assert_eq!(text, "AXB");
        edit.select(&text, 0, u32::MAX);
        edit.limit = 2;
        edit.replace(&mut text, "😀Z", false);
        assert_eq!(text, "😀");
        edit.select(&text, u32::MAX, 0);
        edit.replace(&mut text, "", true);
        assert_eq!(text, "");
        edit.limit = 1;
        edit.replace(&mut text, "😀", false);
        assert_eq!(text, "");
    }
}

#[cfg(test)]
mod snapshot_bounds {
    use super::*;
    use crate::controls::{ControlClass, Controls};
    #[test]
    fn invalid_selection_undo_and_tab_stops_are_rejected_before_restore() {
        let mut controls = Controls::default();
        let hwnd = controls.create(7, ControlClass::Edit, 1, "a😀".into(), 0, 0, 0, 20, 20);
        for edit in [
            EditState {
                selection: Some((0, 4)),
                ..EditState::default()
            },
            EditState {
                undo: Some(("abc".into(), Some((2, 5)))),
                ..EditState::default()
            },
            EditState {
                tab_stops: vec![8, 4],
                ..EditState::default()
            },
        ] {
            assert!(controls
                .validate_edits(&[(hwnd, edit)].into_iter().collect())
                .is_err());
        }
        let edit = EditState {
            selection: Some((1, 3)),
            limit: 0x7ffffffe,
            tab_stops: vec![4, 8],
            ..EditState::default()
        };
        assert!(controls
            .validate_edits(&[(hwnd, edit)].into_iter().collect())
            .is_ok());
    }
}

pub(super) fn previous_boundary(units: &[u16], at: usize) -> usize {
    let mut next = at.min(units.len()).saturating_sub(1);
    if next > 0
        && ((0xdc00..=0xdfff).contains(&units[next]) || units[next] == 10 && units[next - 1] == 13)
    {
        next -= 1;
    }
    next
}
pub(super) fn next_boundary(units: &[u16], at: usize) -> usize {
    if at >= units.len() {
        return units.len();
    }
    let pair = at + 1 < units.len()
        && ((0xd800..=0xdbff).contains(&units[at]) && (0xdc00..=0xdfff).contains(&units[at + 1])
            || units[at] == 13 && units[at + 1] == 10);
    at + if pair { 2 } else { 1 }
}
