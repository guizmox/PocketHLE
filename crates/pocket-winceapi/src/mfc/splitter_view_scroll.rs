//! View-initiated scrolling shares one fan-out engine for codes and pixel deltas.
//! Each sibling starts from the original shared-bar position, as in retail CE3.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1090, "?DoScroll@CSplitterWnd@@UAAHPAVCView@@IH@Z", codes),
    (
        1091,
        "?DoScrollBy@CSplitterWnd@@UAAHPAVCView@@VCSize@@H@Z",
        pixels,
    ),
];
fn codes(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll(ctx, false)
}
fn pixels(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll(ctx, true)
}
fn scroll(ctx: &mut CallCtx<'_>, pixels: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let view = f.arg(ctx, 1)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        if pixels {
            let perform = ctx.arg_u32(4)?;
            f.set(ctx, 7, perform)?;
        }
        return f.call_service(
            ctx,
            1,
            super::splitter_panes::CHILD,
            &[o, view, f.address + 128, f.address + 132],
        );
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            return f.call_virtual(ctx, 2, view, 26, &[1]);
        }
        2 => {
            f.set(ctx, 0, result)?;
            if result != 0 {
                let hwnd = word(ctx, result + 0x20)?;
                return f.call_service(ctx, 3, "GetScrollPos", &[hwnd, 2]);
            }
            return f.call_virtual(ctx, 4, view, 26, &[0]);
        }
        3 => {
            f.set(ctx, 1, result)?;
            return f.call_virtual(ctx, 4, view, 26, &[0]);
        }
        4 => {
            f.set(ctx, 2, result)?;
            if result != 0 {
                let hwnd = word(ctx, result + 0x20)?;
                return f.call_service(ctx, 5, "GetScrollPos", &[hwnd, 2]);
            }
            return invoke(ctx, f, view, pixels, None);
        }
        5 => {
            f.set(ctx, 3, result)?;
            return invoke(ctx, f, view, pixels, None);
        }
        6 => {
            if result != 0 {
                f.set(ctx, 4, 1)?;
            }
        }
        7 => {
            let axis = f.get(ctx, 5)?;
            let index = f.get(ctx, 6)?;
            let row = word(ctx, f.address + 128)?;
            let col = word(ctx, f.address + 132)?;
            return f.call_service(
                ctx,
                8,
                super::splitter_panes::GET,
                &[
                    o,
                    if axis == 0 { row } else { index },
                    if axis == 0 { index } else { col },
                ],
            );
        }
        8 => {
            let axis = f.get(ctx, 5)?;
            if result == 0 {
                return Err(KernelError::Dispatch(
                    "missing pane during splitter view scrolling".into(),
                ));
            }
            return invoke(ctx, f, result, pixels, Some(axis));
        }
        9 => {
            if result != 0 {
                f.set(ctx, 4, 1)?;
            }
            let index = f.get(ctx, 6)?;
            f.set(ctx, 6, index + 1)?;
        }
        _ => {
            return Err(KernelError::Dispatch(
                "invalid splitter view-scroll continuation".into(),
            ))
        }
    }
    loop {
        let axis = f.get(ctx, 5)?;
        if axis >= 2 {
            let handled = f.get(ctx, 4)?;
            return f.finish(ctx, handled);
        }
        let index = f.get(ctx, 6)?;
        let count = word(ctx, o + if axis == 0 { 0x78 } else { 0x74 })?;
        if count > 16 {
            return Err(KernelError::Dispatch(
                "invalid splitter scroll pane count".into(),
            ));
        }
        let bar = f.get(ctx, axis * 2)?;
        if bar == 0 || index >= count {
            f.set(ctx, 5, axis + 1)?;
            f.set(ctx, 6, 0)?;
            continue;
        }
        let source = word(ctx, f.address + if axis == 0 { 132 } else { 128 })?;
        if index == source {
            f.set(ctx, 6, index + 1)?;
            continue;
        }
        let hwnd = word(ctx, bar + 0x20)?;
        let original = f.get(ctx, axis * 2 + 1)?;
        return f.call_service(ctx, 7, "SetScrollPos", &[hwnd, 2, original, 0]);
    }
}
fn invoke(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    view: u32,
    pixels: bool,
    axis: Option<u32>,
) -> Result<DispatchOutcome, KernelError> {
    let first = f.arg(ctx, 2)?;
    let (x, y, perform) = if pixels {
        (
            if axis == Some(0) { 0 } else { first },
            if axis == Some(1) { 0 } else { f.arg(ctx, 3)? },
            f.get(ctx, 7)?,
        )
    } else {
        (
            match axis {
                Some(0) => (first & 0xff00) | 0xff,
                Some(1) => (first & 0xff) | 0xff00,
                _ => first,
            },
            0,
            f.arg(ctx, 3)?,
        )
    };
    f.call_virtual(
        ctx,
        if axis.is_some() { 9 } else { 6 },
        view,
        if pixels { 48 } else { 47 },
        &[x, y, perform],
    )
}
