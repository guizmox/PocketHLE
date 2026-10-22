//! Grid navigation for the Steam Deck catalogue, independent of guest inputs.
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
