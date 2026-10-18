//! Stateful subset of the WinCE trackbar messages used by native dialogs.
use super::*;

pub(super) fn message(ctx: &mut CallCtx<'_>, hwnd: u32, msg: u32, wp: u32, lp: u32) -> Option<u32> {
    let c = ctx.kernel.controls.get_mut(hwnd)?;
    if c.class != ControlClass::TrackBar {
        return None;
    }
    let result = match msg {
        0x400 => return Some(c.track_pos as u32),
        0x401 => return Some(c.track_range.0 as u32),
        0x402 => return Some(c.track_range.1 as u32),
        0x405 => {
            c.track_pos = (lp as i32).clamp(c.track_range.0, c.track_range.1);
            0
        }
        0x406 => {
            c.track_range = (lp as i16 as i32, (lp >> 16) as i16 as i32);
            0
        }
        0x407 => {
            c.track_range.0 = lp as i32;
            0
        }
        0x408 => {
            c.track_range.1 = lp as i32;
            0
        }
        0x416 => return Some(c.track_page as u32),
        0x415 => {
            let old = c.track_page;
            c.track_page = (lp as i32).max(0);
            old as u32
        }
        0x418 => return Some(c.track_line as u32),
        0x417 => {
            let old = c.track_line;
            c.track_line = (lp as i32).max(0);
            old as u32
        }
        // Tick frequency changes presentation, not the range/position.
        0x414 => return Some(0),
        _ => return None,
    };
    c.track_range.1 = c.track_range.1.max(c.track_range.0);
    c.track_pos = c.track_pos.clamp(c.track_range.0, c.track_range.1);
    if wp != 0 {
        repaint_controls(ctx);
    }
    Some(result)
}
