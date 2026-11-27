//! System cursor identities are portable guest handles. Frontends project their
//! shape through host widgets independently of emulated image publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemCursor {
    Arrow,
    Text,
    Wait,
    Crosshair,
    Up,
    SizeDiagonalDown,
    SizeDiagonalUp,
    SizeHorizontal,
    SizeVertical,
    Move,
    Forbidden,
    Hand,
    Progress,
    Help,
}
pub fn system_cursor_handle(id: u32) -> Option<u32> {
    match id {
        32512..=32516 | 32642..=32646 | 32648..=32651 => Some(0xfef00000 | id),
        _ => None,
    }
}
pub fn system_cursor(handle: u32) -> Option<SystemCursor> {
    if handle & 0xffff0000 != 0xfef00000 {
        return None;
    }
    Some(match handle & 0xffff {
        32512 => SystemCursor::Arrow,
        32513 => SystemCursor::Text,
        32514 => SystemCursor::Wait,
        32515 => SystemCursor::Crosshair,
        32516 => SystemCursor::Up,
        32642 => SystemCursor::SizeDiagonalDown,
        32643 => SystemCursor::SizeDiagonalUp,
        32644 => SystemCursor::SizeHorizontal,
        32645 => SystemCursor::SizeVertical,
        32646 => SystemCursor::Move,
        32648 => SystemCursor::Forbidden,
        32649 => SystemCursor::Hand,
        32650 => SystemCursor::Progress,
        32651 => SystemCursor::Help,
        _ => return None,
    })
}
