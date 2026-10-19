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
        // TBM_SETTICFREQ takes its frequency in wParam, not lParam.
        0x414 => {
            c.track_tick_freq = wp.max(1);
            0
        }
        0x404 => {
            let tick = lp as i32;
            if tick <= c.track_range.0 || tick >= c.track_range.1 {
                return Some(0);
            }
            if !c.track_ticks.contains(&tick) {
                c.track_ticks.push(tick);
            }
            1
        }
        0x409 => {
            c.track_ticks.clear();
            c.track_tick_freq = 0;
            0
        }
        _ => return None,
    };
    c.track_range.1 = c.track_range.1.max(c.track_range.0);
    c.track_pos = c.track_pos.clamp(c.track_range.0, c.track_range.1);
    if wp != 0 || msg == 0x414 {
        repaint_controls(ctx);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_trackbar_tick_messages_read_wparam_and_preserve_position() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        let hwnd = kernel.controls.create(
            FAKE_HWND,
            ControlClass::TrackBar,
            1,
            String::new(),
            0x50000001,
            0,
            0,
            100,
            30,
        );
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        assert_eq!(message(&mut ctx, hwnd, 0x414, 5, 99), Some(0));
        assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().track_tick_freq, 5);
        assert_eq!(message(&mut ctx, hwnd, 0x404, 0, 17), Some(1));
        assert_eq!(message(&mut ctx, hwnd, 0x404, 0, 101), Some(0));
        assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().track_ticks, vec![17]);
        assert_eq!(message(&mut ctx, hwnd, 0x409, 0, 0), Some(0));
        let c = ctx.kernel.controls.get(hwnd).unwrap();
        assert!(c.track_ticks.is_empty());
        assert_eq!(c.track_tick_freq, 0);
        assert_eq!(c.track_pos, 0);
        assert_eq!(c.track_range, (0, 100));
    }
}
