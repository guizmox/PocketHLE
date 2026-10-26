//! Grid navigation for the Steam Deck catalogue, independent of guest inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryTab {
    Gizmondo,
    PocketPc,
    Recordings,
}
impl LibraryTab {
    pub fn step(self, forward: bool) -> Self {
        use LibraryTab::*;
        match (self, forward) {
            (Gizmondo, true) | (Recordings, false) => PocketPc,
            (PocketPc, true) | (Gizmondo, false) => Recordings,
            (Recordings, true) | (PocketPc, false) => Gizmondo,
        }
    }
}
pub fn tab_direction(control: &str) -> Option<bool> {
    match control {
        "LeftTrigger" | "LeftTrigger2" => Some(false),
        "RightTrigger" | "RightTrigger2" | "West" => Some(true),
        _ => None,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightFnAction {
    NextFilter,
    ToggleRecording,
}
pub fn right_fn_action(elapsed: std::time::Duration, released: bool) -> Option<RightFnAction> {
    if elapsed >= std::time::Duration::from_millis(650) {
        Some(RightFnAction::ToggleRecording)
    } else if released {
        Some(RightFnAction::NextFilter)
    } else {
        None
    }
}
pub fn is_direction(control: &str) -> bool {
    matches!(
        control,
        "DPadUp"
            | "DPadDown"
            | "DPadLeft"
            | "DPadRight"
            | "LeftStickUp"
            | "LeftStickDown"
            | "LeftStickLeft"
            | "LeftStickRight"
    )
}

pub fn next_index(index: usize, count: usize, columns: usize, control: &str) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let index = index.min(count - 1);
    let columns = columns.max(1);
    Some(match control {
        "DPadUp" | "LeftStickUp" => {
            if index >= columns {
                index - columns
            } else {
                index
            }
        }
        "DPadDown" | "LeftStickDown" => index.saturating_add(columns).min(count - 1),
        "DPadLeft" | "LeftStickLeft" => index.saturating_sub(1),
        "DPadRight" | "LeftStickRight" => index.saturating_add(1).min(count - 1),
        _ => index,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    NextFilter,
    ReturnToLibrary,
}

pub fn menu_action(elapsed: std::time::Duration, released: bool) -> Option<MenuAction> {
    if elapsed >= std::time::Duration::from_millis(650) {
        Some(MenuAction::ReturnToLibrary)
    } else if released {
        Some(MenuAction::NextFilter)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewAction {
    ToggleFullscreen,
    SecondaryKey,
}

pub fn view_action(elapsed: std::time::Duration, released: bool) -> Option<ViewAction> {
    if elapsed >= std::time::Duration::from_millis(650) {
        Some(ViewAction::SecondaryKey)
    } else if released {
        Some(ViewAction::ToggleFullscreen)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn view_hold_does_not_toggle_fullscreen_before_secondary_key() {
        use std::time::Duration;
        assert_eq!(view_action(Duration::from_millis(0), false), None);
        assert_eq!(view_action(Duration::from_millis(649), false), None);
        assert_eq!(
            view_action(Duration::from_millis(100), true),
            Some(ViewAction::ToggleFullscreen)
        );
        assert_eq!(
            view_action(Duration::from_millis(650), false),
            Some(ViewAction::SecondaryKey)
        );
        assert_eq!(
            view_action(Duration::from_millis(650), true),
            Some(ViewAction::SecondaryKey)
        );
    }

    #[test]
    fn menu_tap_and_hold_are_distinct_at_the_deadline() {
        use std::time::Duration;
        assert_eq!(menu_action(Duration::from_millis(100), false), None);
        assert_eq!(
            menu_action(Duration::from_millis(100), true),
            Some(MenuAction::NextFilter)
        );
        assert_eq!(
            menu_action(Duration::from_millis(649), true),
            Some(MenuAction::NextFilter)
        );
        assert_eq!(
            menu_action(Duration::from_millis(650), false),
            Some(MenuAction::ReturnToLibrary)
        );
        assert_eq!(
            menu_action(Duration::from_millis(650), true),
            Some(MenuAction::ReturnToLibrary)
        );
    }

    #[test]
    fn vertical_navigation_matches_the_grid_and_partial_last_row() {
        assert_eq!(next_index(1, 8, 3, "DPadDown"), Some(4));
        assert_eq!(next_index(5, 8, 3, "LeftStickDown"), Some(7));
        assert_eq!(next_index(7, 8, 3, "DPadUp"), Some(4));
        assert_eq!(next_index(1, 8, 3, "DPadUp"), Some(1));
    }
    #[test]
    fn empty_single_item_and_stale_selection_are_safe() {
        assert_eq!(next_index(0, 0, 0, "DPadRight"), None);
        assert_eq!(next_index(0, 1, 0, "DPadDown"), Some(0));
        assert_eq!(next_index(99, 3, 1, "DPadLeft"), Some(1));
        assert_eq!(next_index(0, 3, 2, "DPadLeft"), Some(0));
    }
    #[test]
    fn every_direction_stays_inside_resized_catalogues() {
        for count in 1..64 {
            for columns in 0..12 {
                for index in 0..count {
                    for direction in ["DPadUp", "DPadDown", "DPadLeft", "DPadRight"] {
                        assert!(next_index(index, count, columns, direction).unwrap() < count);
                    }
                }
            }
        }
        assert!(!is_direction("South"));
        assert!(is_direction("LeftStickDown"));
    }
}

#[cfg(test)]
mod catalogue_tests {
    use super::*;
    #[test]
    fn shoulders_wrap_the_three_tabs_in_both_directions() {
        for control in ["RightTrigger", "RightTrigger2"] {
            let dir = tab_direction(control).unwrap();
            assert_eq!(LibraryTab::Gizmondo.step(dir), LibraryTab::PocketPc);
            assert_eq!(LibraryTab::PocketPc.step(dir), LibraryTab::Recordings);
            assert_eq!(LibraryTab::Recordings.step(dir), LibraryTab::Gizmondo);
        }
        for control in ["LeftTrigger", "LeftTrigger2"] {
            let dir = tab_direction(control).unwrap();
            assert_eq!(LibraryTab::Gizmondo.step(dir), LibraryTab::Recordings);
            assert_eq!(LibraryTab::Recordings.step(dir), LibraryTab::PocketPc);
            assert_eq!(LibraryTab::PocketPc.step(dir), LibraryTab::Gizmondo);
        }
        assert_eq!(tab_direction("South"), None);
    }
    #[test]
    fn right_fn_records_while_left_exit_gesture_is_unchanged() {
        use std::time::Duration;
        assert_eq!(right_fn_action(Duration::from_millis(649), false), None);
        assert_eq!(
            right_fn_action(Duration::from_millis(649), true),
            Some(RightFnAction::NextFilter)
        );
        for released in [false, true] {
            assert_eq!(
                right_fn_action(Duration::from_millis(650), released),
                Some(RightFnAction::ToggleRecording)
            );
            assert_eq!(
                menu_action(Duration::from_millis(650), released),
                Some(MenuAction::ReturnToLibrary)
            );
        }
    }
}

/// Move inside the game actions popup without moving the catalogue selection.
pub fn menu_index(index: usize, control: &str, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    match control {
        "DPadUp" | "LeftStickUp" => Some((index.min(count - 1) + count - 1) % count),
        "DPadDown" | "LeftStickDown" => Some((index.min(count - 1) + 1) % count),
        _ => None,
    }
}
#[cfg(test)]
mod game_menu_tests {
    use super::*;
    #[test]
    fn popup_navigation_wraps_and_does_not_consume_activation_or_back() {
        for up in ["DPadUp", "LeftStickUp"] {
            assert_eq!(menu_index(0, up, 5), Some(4));
        }
        for down in ["DPadDown", "LeftStickDown"] {
            assert_eq!(menu_index(4, down, 5), Some(0));
        }
        for other in ["South", "East", "DPadLeft", "DPadRight"] {
            assert_eq!(menu_index(0, other, 5), None);
        }
        assert_eq!(menu_index(0, "DPadDown", 0), None);
    }
}
