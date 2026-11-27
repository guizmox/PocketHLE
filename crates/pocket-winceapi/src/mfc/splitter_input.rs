//! Mouse and keyboard splitter commands use native hit/track virtuals and the
//! ordinary WinCE input queue, including capture changes and cursor movement.
use super::callbacks::Frame;
use super::splitter_hit::{offset, rect};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1952,
        "?OnLButtonDblClk@CSplitterWnd@@IAAXIVCPoint@@@Z",
        double_click,
    ),
    (1953, "?OnLButtonDown@CSplitterWnd@@IAAXIVCPoint@@@Z", down),
    (1954, "?OnLButtonUp@CSplitterWnd@@IAAXIVCPoint@@@Z", up),
    (1957, "?OnMouseMove@CSplitterWnd@@IAAXIVCPoint@@@Z", motion),
    (1809, "?OnCancelMode@CSplitterWnd@@IAAXXZ", cancel),
    (1949, "?OnKeyDown@CSplitterWnd@@IAAXIII@Z", key),
    (2423, "?SetSplitCursor@CSplitterWnd@@MAAXH@Z", cursor),
    (
        2009,
        "?OnSetCursor@CSplitterWnd@@IAAHPAVCWnd@@II@Z",
        set_cursor,
    ),
    (
        1076,
        "?DoKeyboardSplit@CSplitterWnd@@UAAHXZ",
        keyboard_split,
    ),
];
fn down(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        if word(ctx, o + 0x8c)? != 0 {
            return f.finish(ctx, 0);
        }
        let x = f.arg(ctx, 2)?;
        let y = f.arg(ctx, 3)?;
        return f.call_virtual(ctx, 1, o, 63, &[x, y]);
    }
    if f.phase(ctx)? == 1 {
        let hit = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.call_virtual(ctx, 2, o, 70, &[hit]);
    }
    f.finish(ctx, 0)
}
fn up(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    stop(ctx, 1)
}
fn cancel(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    stop(ctx, 0)
}
fn stop(ctx: &mut CallCtx<'_>, accept: u32) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_virtual(ctx, 1, o, 71, &[accept]);
    }
    f.finish(ctx, 0)
}
fn cursor(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let hit = f.arg(ctx, 1)?;
        let id = match hit {
            1 | 101..=115 => 32645,
            2 | 201..=215 => 32644,
            3 | 301..=525 => 32646,
            _ => 32512,
        };
        return f.call_service(ctx, 1, "LoadCursorW", &[0, id]);
    }
    if f.phase(ctx)? == 1 {
        let cursor = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.call_service(ctx, 2, "SetCursor", &[cursor]);
    }
    f.finish(ctx, 0)
}
fn set_cursor(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        if f.arg(ctx, 1)? == o && f.arg(ctx, 2)? == 1 && word(ctx, o + 0x8c)? == 0 {
            return f.finish(ctx, 1);
        }
        return f.call_service(ctx, 1, "__PocketHLE_MFC_WindowDefault", &[o]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, result)
}
fn keyboard_split(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let rows = word(ctx, o + 0x74)?;
        let cols = word(ctx, o + 0x78)?;
        let hit = if rows > 1 && cols > 1 {
            301
        } else if rows > 1 {
            101
        } else if cols > 1 {
            201
        } else {
            match (word(ctx, o + 0x4c)? > 1, word(ctx, o + 0x50)? > 1) {
                (true, true) => 3,
                (true, false) => 1,
                (false, true) => 2,
                _ => 0,
            }
        };
        if hit == 0 {
            return f.finish(ctx, 0);
        }
        return f.call_virtual(ctx, 1, o, 70, &[hit]);
    }
    f.finish(ctx, 1)
}
fn clamp(v: i32, min: i32, max: i32) -> i32 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}
fn motion(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, "GetCapture", &[]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            let hwnd = word(ctx, o + 0x20)?;
            if result != hwnd {
                return f.call_virtual(ctx, 2, o, 71, &[0]);
            }
            begin_motion(ctx, f, o)
        }
        2 => begin_motion(ctx, f, o),
        3 => {
            let at = f.get(ctx, 5)?;
            let axis = f.get(ctx, 6)?;
            let value = f.get(ctx, 7)? as i32;
            let old = word(ctx, at + axis)? as i32;
            offset(
                ctx,
                at,
                if axis == 0 {
                    value.wrapping_sub(old)
                } else {
                    0
                },
                if axis == 4 {
                    value.wrapping_sub(old)
                } else {
                    0
                },
            )?;
            f.call_virtual(ctx, 4, o, 49, &[at])
        }
        4 => {
            let i = f.get(ctx, 3)?;
            f.set(ctx, 3, i + 1)?;
            next_motion(ctx, f, o)
        }
        5 => {
            let hit = ctx.cpu.read_reg(ArmReg::R0)?;
            f.call_virtual(ctx, 6, o, 69, &[hit])
        }
        6 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter pointer tracking continuation".into(),
        )),
    }
}
fn begin_motion(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let x = f.arg(ctx, 2)? as i32;
    let y = f.arg(ctx, 3)? as i32;
    if word(ctx, o + 0x8c)? == 0 {
        return f.call_virtual(ctx, 5, o, 63, &[x as u32, y as u32]);
    }
    let [l, t, r, b] = rect(ctx, o + 0x9c)?;
    let x = x.wrapping_add(word(ctx, o + 0x94)? as i32);
    let y = y.wrapping_add(word(ctx, o + 0x98)? as i32);
    f.set(ctx, 0, clamp(x, l, r) as u32)?;
    f.set(ctx, 1, clamp(y, t, b) as u32)?;
    next_motion(ctx, f, o)
}
fn next_motion(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let hit = word(ctx, o + 0xcc)?;
    let row = hit == 1 || (101..=115).contains(&hit);
    let col = hit == 2 || (201..=215).contains(&hit);
    let both = hit == 3 || (301..=525).contains(&hit);
    loop {
        let i = f.get(ctx, 3)?;
        if i >= if both { 2 } else { 1 } || (!row && !col && !both) {
            return f.finish(ctx, 0);
        }
        let at = if i == 0 { o + 0xac } else { o + 0xbc };
        let axis = if row || (both && i == 0) { 4 } else { 0 };
        let value = f.get(ctx, if axis == 0 { 0 } else { 1 })?;
        if word(ctx, at + axis)? == value {
            f.set(ctx, 3, i + 1)?;
            continue;
        }
        f.set(ctx, 5, at)?;
        f.set(ctx, 6, axis)?;
        f.set(ctx, 7, value)?;
        return f.call_virtual(ctx, 3, o, 49, &[at]);
    }
}
fn key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let key = f.arg(ctx, 1)?;
    if f.fresh {
        if key == 13 || key == 27 {
            return f.call_virtual(ctx, 10, o, 71, &[u32::from(key == 13)]);
        }
        if !(0x25..=0x28).contains(&key) {
            return f.call_service(ctx, 10, "__PocketHLE_MFC_WindowDefault", &[o]);
        }
        return f.call_service(ctx, 1, "GetCursorPos", &[f.address + 128]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => f.call_service(ctx, 2, "GetKeyState", &[0x11]),
        2 => {
            let step = if (result as u16 as i16) < 0 { 1i32 } else { 16 };
            let hit = word(ctx, o + 0xcc)?;
            let row = hit == 1 || (101..=115).contains(&hit);
            let col = hit == 2 || (201..=215).contains(&hit);
            let x = word(ctx, f.address + 128)? as i32;
            let y = word(ctx, f.address + 132)? as i32;
            let dx = if row {
                0
            } else {
                match key {
                    0x25 => -step,
                    0x27 => step,
                    _ => 0,
                }
            };
            let dy = if col {
                0
            } else {
                match key {
                    0x26 => -step,
                    0x28 => step,
                    _ => 0,
                }
            };
            put(ctx, f.address + 128, x.wrapping_add(dx) as u32)?;
            put(ctx, f.address + 132, y.wrapping_add(dy) as u32)?;
            let hwnd = word(ctx, o + 0x20)?;
            f.call_service(ctx, 3, "ScreenToClient", &[hwnd, f.address + 128])
        }
        3 => {
            let [l, t, r, b] = rect(ctx, o + 0x9c)?;
            let x = word(ctx, f.address + 128)? as i32;
            let y = word(ctx, f.address + 132)? as i32;
            put(ctx, f.address + 128, clamp(x, l, r) as u32)?;
            put(ctx, f.address + 132, clamp(y, t, b) as u32)?;
            let hwnd = word(ctx, o + 0x20)?;
            f.call_service(ctx, 4, "ClientToScreen", &[hwnd, f.address + 128])
        }
        4 => {
            let x = word(ctx, f.address + 128)?;
            let y = word(ctx, f.address + 132)?;
            f.call_service(ctx, 10, "SetCursorPos", &[x, y])
        }
        10 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter keyboard tracking continuation".into(),
        )),
    }
}

fn double_click(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let x = f.arg(ctx, 2)?;
        let y = f.arg(ctx, 3)?;
        return f.call_virtual(ctx, 1, o, 63, &[x, y]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            f.set(ctx, 0, result)?;
            f.call_virtual(ctx, 2, o, 71, &[0])
        }
        2 => {
            let hwnd = word(ctx, o + 0x20)?;
            f.call_service(ctx, 3, "GetWindowLongW", &[hwnd, (-16i32) as u32])
        }
        3 => {
            if result & 1 == 0 {
                return f.finish(ctx, 0);
            }
            let hit = f.get(ctx, 0)?;
            if hit == 1 || hit == 2 {
                let p = word(ctx, o + if hit == 1 { 0x88 } else { 0x84 })?;
                let size = word(ctx, p + 8)? as i32;
                return f.call_virtual(
                    ctx,
                    6,
                    o,
                    if hit == 1 { 52 } else { 53 },
                    &[(size / 2) as u32],
                );
            }
            if !(101..=115).contains(&hit)
                && !(201..=215).contains(&hit)
                && !(301..=525).contains(&hit)
            {
                return f.finish(ctx, 0);
            }
            f.call_virtual(ctx, 4, o, 56, &[f.address + 128, f.address + 132])
        }
        4 => {
            let hit = f.get(ctx, 0)?;
            let active = result != 0;
            if (101..=115).contains(&hit) {
                let mut row = hit - 101;
                if active && word(ctx, f.address + 128)? == row {
                    row += 1;
                }
                return f.call_virtual(ctx, 6, o, 54, &[row]);
            }
            if (201..=215).contains(&hit) {
                let mut col = hit - 201;
                if active && word(ctx, f.address + 132)? == col {
                    col += 1;
                }
                return f.call_virtual(ctx, 6, o, 55, &[col]);
            }
            let mut row = (hit - 301) / 15;
            let mut col = (hit - 301) % 15;
            if active {
                if word(ctx, f.address + 128)? == row {
                    row += 1;
                }
                if word(ctx, f.address + 132)? == col {
                    col += 1;
                }
            }
            f.set(ctx, 1, col)?;
            f.call_virtual(ctx, 5, o, 54, &[row])
        }
        5 => {
            let col = f.get(ctx, 1)?;
            f.call_virtual(ctx, 6, o, 55, &[col])
        }
        6 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter double-click continuation".into(),
        )),
    }
}
