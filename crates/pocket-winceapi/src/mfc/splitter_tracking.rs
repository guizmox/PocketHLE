//! Tracking state belongs to CSplitterWnd's native guest fields. Continuations
//! use the same virtual callbacks and USER capture/focus as ordinary windows.
use super::callbacks::Frame;
use super::splitter_hit::{offset, rect};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2455, "?StartTracking@CSplitterWnd@@MAAXH@Z", start),
    (2457, "?StopTracking@CSplitterWnd@@MAAXH@Z", stop),
    (
        2463,
        "?TrackColumnSize@CSplitterWnd@@MAAXHH@Z",
        track_column,
    ),
    (2466, "?TrackRowSize@CSplitterWnd@@MAAXHH@Z", track_row),
];
fn start(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let hit = f.arg(ctx, 1)?;
    if f.fresh {
        if hit == 0 {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, hit)?;
        put(ctx, o + 0x90, 0)?;
        return f.call_virtual(ctx, 1, o, 64, &[o + 0x9c]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            let primary = if hit == 3 {
                1
            } else if (301..=525).contains(&hit) {
                101 + (hit - 301) / 15
            } else {
                hit
            };
            f.call_virtual(ctx, 2, o, 65, &[primary, o + 0xac])
        }
        2 => {
            if hit == 3 || (301..=525).contains(&hit) {
                let y = word(ctx, o + 0x98)?;
                f.set(ctx, 1, y)?;
                put(ctx, o + 0x90, 1)?;
                let secondary = if hit == 3 { 2 } else { 201 + (hit - 301) % 15 };
                return f.call_virtual(ctx, 3, o, 65, &[secondary, o + 0xbc]);
            }
            active_start(ctx, &f, o)
        }
        3 => {
            let y = f.get(ctx, 1)?;
            put(ctx, o + 0x98, y)?;
            if hit == 3 {
                let [l, t, r, b] = rect(ctx, o + 0x9c)?;
                offset(ctx, o + 0xac, 0, b.wrapping_sub(t) / 2)?;
                offset(ctx, o + 0xbc, r.wrapping_sub(l) / 2, 0)?;
            }
            active_start(ctx, &f, o)
        }
        4 => {
            if result == 0 {
                return capture(ctx, &f, o);
            }
            f.set(ctx, 2, result)?;
            let class = super::classes::runtime(ctx, "CView")?;
            f.call_service(ctx, 5, super::class_queries::KIND, &[result, class])
        }
        5 => {
            if result == 0 {
                return capture(ctx, &f, o);
            }
            f.call_service(ctx, 6, super::hierarchy::PARENT, &[o])
        }
        6 => {
            let view = f.get(ctx, 2)?;
            f.call_virtual(ctx, 7, view, 52, &[0, result])
        }
        7 => capture(ctx, &f, o),
        8 => {
            let hwnd = word(ctx, o + 0x20)?;
            f.call_service(ctx, 9, "SetFocus", &[hwnd])
        }
        9 => {
            let hwnd = word(ctx, o + 0x20)?;
            f.call_service(ctx, 10, "RedrawWindow", &[hwnd, 0, 0, 0x180])
        }
        10 => {
            put(ctx, o + 0x8c, 1)?;
            f.call_virtual(ctx, 11, o, 49, &[o + 0xac])
        }
        11 => {
            if word(ctx, o + 0x90)? != 0 {
                return f.call_virtual(ctx, 12, o, 49, &[o + 0xbc]);
            }
            finish_start(ctx, &f, o, hit)
        }
        12 => finish_start(ctx, &f, o, hit),
        13 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter tracking start continuation".into(),
        )),
    }
}
fn active_start(ctx: &mut CallCtx<'_>, f: &Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    f.call_virtual(ctx, 4, o, 56, &[0, 0])
}
fn capture(ctx: &mut CallCtx<'_>, f: &Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let hwnd = word(ctx, o + 0x20)?;
    f.call_service(ctx, 8, "SetCapture", &[hwnd])
}
fn finish_start(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    o: u32,
    hit: u32,
) -> Result<DispatchOutcome, KernelError> {
    put(ctx, o + 0xcc, hit)?;
    f.call_virtual(ctx, 13, o, 69, &[hit])
}
fn stop(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        if word(ctx, o + 0x8c)? == 0 {
            return f.finish(ctx, 0);
        }
        return f.call_service(ctx, 1, "ReleaseCapture", &[]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => f.call_virtual(ctx, 2, o, 49, &[o + 0xac]),
        2 => {
            if word(ctx, o + 0x90)? != 0 {
                return f.call_virtual(ctx, 3, o, 49, &[o + 0xbc]);
            }
            stop_active(ctx, &f, o)
        }
        3 => stop_active(ctx, &f, o),
        4 => {
            f.set(ctx, 0, result)?;
            offset(ctx, o + 0xac, -1, -1)?;
            offset(ctx, o + 0xbc, -1, -1)?;
            if f.arg(ctx, 1)? != 0 {
                let hit = word(ctx, o + 0xcc)?;
                let primary = rect(ctx, o + 0xac)?;
                let secondary = rect(ctx, o + 0xbc)?;
                let actions: Vec<(u32, u32, u32)> = match hit {
                    1 => vec![(52, primary[1] as u32, 0)],
                    2 => vec![(53, primary[0] as u32, 0)],
                    3 => vec![(52, primary[1] as u32, 0), (53, secondary[0] as u32, 0)],
                    101..=115 => vec![(66, primary[1] as u32, hit - 101), (47, 0, 0)],
                    201..=215 => vec![(67, primary[0] as u32, hit - 201), (47, 0, 0)],
                    301..=525 => vec![
                        (66, primary[1] as u32, (hit - 301) / 15),
                        (67, secondary[0] as u32, (hit - 301) % 15),
                        (47, 0, 0),
                    ],
                    _ => vec![],
                };
                f.set(ctx, 2, actions.len() as u32)?;
                for (i, (slot, a, b)) in actions.into_iter().enumerate() {
                    for (j, v) in [slot, a, b].into_iter().enumerate() {
                        put(ctx, f.address + 128 + i as u32 * 12 + j as u32 * 4, v)?;
                    }
                }
            }
            next_action(ctx, f, o)
        }
        5 => {
            let i = f.get(ctx, 1)?;
            f.set(ctx, 1, i + 1)?;
            next_action(ctx, f, o)
        }
        6 => {
            let old = f.get(ctx, 0)?;
            if old == 0 || old != result {
                return f.finish(ctx, 0);
            }
            f.call_virtual(ctx, 7, o, 57, &[u32::MAX, u32::MAX, old])
        }
        7 => {
            let old = f.get(ctx, 0)?;
            let hwnd = word(ctx, old + 0x20)?;
            f.call_service(ctx, 8, "SetFocus", &[hwnd])
        }
        8 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter tracking stop continuation".into(),
        )),
    }
}
fn stop_active(ctx: &mut CallCtx<'_>, f: &Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    put(ctx, o + 0x90, 0)?;
    put(ctx, o + 0x8c, 0)?;
    f.call_virtual(ctx, 4, o, 56, &[0, 0])
}
fn next_action(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let i = f.get(ctx, 1)?;
    if i >= f.get(ctx, 2)? {
        return f.call_virtual(ctx, 6, o, 56, &[0, 0]);
    }
    let p = f.address + 128 + i * 12;
    let slot = word(ctx, p)?;
    let a = word(ctx, p + 4)?;
    let b = word(ctx, p + 8)?;
    f.call_virtual(ctx, 5, o, slot, &[a, b])
}
fn track_row(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    track(ctx, true)
}
fn track_column(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    track(ctx, false)
}
fn track(ctx: &mut CallCtx<'_>, row: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let index = f.arg(ctx, 2)?;
    if f.fresh {
        let pos = f.arg(ctx, 1)?;
        let count = word(ctx, o + if row { 0x74 } else { 0x78 })?;
        if index >= count || count > 16 {
            return Err(KernelError::Dispatch(
                "invalid tracked splitter index".into(),
            ));
        }
        put(ctx, f.address + 128, if row { 0 } else { pos })?;
        put(ctx, f.address + 132, if row { pos } else { 0 })?;
        let hwnd = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 1, "ClientToScreen", &[hwnd, f.address + 128]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => f.call_service(
            ctx,
            2,
            super::splitter_panes::GET,
            &[o, if row { index } else { 0 }, if row { 0 } else { index }],
        ),
        2 => {
            if result == 0 {
                return Err(KernelError::Dispatch(
                    "tracked splitter pane is missing".into(),
                ));
            }
            let hwnd = word(ctx, result + 0x20)?;
            f.call_service(ctx, 3, "ScreenToClient", &[hwnd, f.address + 128])
        }
        3 => {
            let p = word(ctx, o + if row { 0x88 } else { 0x84 })? + index * 12;
            let pos = word(ctx, f.address + if row { 132 } else { 128 })? as i32;
            let min = word(ctx, p)? as i32;
            put(ctx, p + 4, pos as u32)?;
            let mut deleted = u32::MAX;
            if pos < min {
                put(ctx, p + 4, 0)?;
                deleted = index;
            } else {
                let count = word(ctx, o + if row { 0x74 } else { 0x78 })?;
                if index + 1 < count {
                    let current = word(ctx, p + 8)? as i32;
                    let next_current = word(ctx, p + 20)? as i32;
                    let next_min = word(ctx, p + 12)? as i32;
                    if current.wrapping_add(next_current) < pos.wrapping_add(next_min) {
                        deleted = index + 1;
                    }
                }
            }
            if deleted == u32::MAX {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 0, deleted)?;
            let hwnd = word(ctx, o + 0x20)?;
            f.call_service(ctx, 4, "GetWindowLongW", &[hwnd, (-16i32) as u32])
        }
        4 => {
            if result & 1 == 0 {
                return f.finish(ctx, 0);
            }
            let deleted = f.get(ctx, 0)?;
            f.call_virtual(ctx, 5, o, if row { 54 } else { 55 }, &[deleted])
        }
        5 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter size tracking continuation".into(),
        )),
    }
}
