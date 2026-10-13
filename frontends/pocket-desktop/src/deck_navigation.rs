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

#[cfg(test)]
mod tests {
    use super::*;
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
