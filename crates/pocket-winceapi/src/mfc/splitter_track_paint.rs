//! Tracker inversion and splitter bevels use shared GDI primitives and owners.
use super::callbacks::Frame;
use super::splitter_hit::{put_rect, rect};
use super::*;
use pocket_cpu::regs::ArmReg;
const HALF: &str = "__PocketHLE_MFC_HalftoneHandle";
const HALF_ROOT: &str = "__PocketHLE_MFC_HalftoneBrush";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[(
    1948,
    "?OnInvertTracker@CSplitterWnd@@MAAXABVCRect@@@Z",
    invert,
)];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", HALF, halftone);
}
fn cached(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let owner = super::application::owner(ctx)?;
    let handle = ctx.kernel.dynamic_exports[&owner]
        .get(HALF_ROOT)
        .copied()
        .unwrap_or(0);
    Ok(if ctx.kernel.gdi.brush(handle).is_some() {
        handle
    } else {
        0
    })
}
fn halftone(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let handle = cached(ctx)?;
        if handle != 0 {
            return f.finish(ctx, handle);
        }
        let p = f.address + 128;
        for y in 0..8 {
            let bits = 0x5555u16 << (y & 1);
            ctx.cpu.write_mem(p + y * 2, &bits.to_le_bytes())?;
        }
        return f.call_service(ctx, 1, "CreateBitmap", &[8, 8, 1, 1, p]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 0, result)?;
            f.call_service(ctx, 2, "CreatePatternBrush", &[result])
        }
        2 => {
            f.set(ctx, 1, result)?;
            let existing = cached(ctx)?;
            if result != 0 && existing == 0 {
                let owner = super::application::owner(ctx)?;
                ctx.kernel
                    .dynamic_exports
                    .get_mut(&owner)
                    .unwrap()
                    .insert(HALF_ROOT.into(), result);
            } else if result != 0 {
                f.set(ctx, 1, existing)?;
                f.set(ctx, 2, result)?;
            }
            let bitmap = f.get(ctx, 0)?;
            f.call_service(ctx, 3, "DeleteObject", &[bitmap])
        }
        3 => {
            let redundant = f.get(ctx, 2)?;
            if redundant != 0 {
                return f.call_service(ctx, 4, "DeleteObject", &[redundant]);
            }
            let brush = f.get(ctx, 1)?;
            f.finish(ctx, brush)
        }
        4 => {
            let brush = f.get(ctx, 1)?;
            f.finish(ctx, brush)
        }
        _ => Err(KernelError::Dispatch(
            "invalid halftone brush continuation".into(),
        )),
    }
}
fn invert(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let p = f.arg(ctx, 1)?;
        let area = rect(ctx, p)?;
        put_rect(ctx, f.address + 128, area)?;
        let hwnd = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 1, "GetDC", &[hwnd]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            f.set(ctx, 0, result)?;
            f.call_service(ctx, 2, HALF, &[])
        }
        2 => {
            if result != 0 {
                let dc = f.get(ctx, 0)?;
                return f.call_service(ctx, 3, "SelectObject", &[dc, result]);
            }
            pat(ctx, &f)
        }
        3 => {
            f.set(ctx, 1, result)?;
            pat(ctx, &f)
        }
        4 => {
            let old = f.get(ctx, 1)?;
            if old != 0 {
                let dc = f.get(ctx, 0)?;
                return f.call_service(ctx, 5, "SelectObject", &[dc, old]);
            }
            release(ctx, &f, o)
        }
        5 => release(ctx, &f, o),
        6 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter inversion continuation".into(),
        )),
    }
}
fn pat(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let [l, t, r, b] = rect(ctx, f.address + 128)?;
    let dc = f.get(ctx, 0)?;
    f.call_service(
        ctx,
        4,
        "PatBlt",
        &[
            dc,
            l as u32,
            t as u32,
            r.wrapping_sub(l) as u32,
            b.wrapping_sub(t) as u32,
            0x5a0049,
        ],
    )
}
fn release(ctx: &mut CallCtx<'_>, f: &Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let hwnd = word(ctx, o + 0x20)?;
    let dc = f.get(ctx, 0)?;
    f.call_service(ctx, 6, "ReleaseDC", &[hwnd, dc])
}
pub(super) fn draw(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let cdc = f.arg(ctx, 1)?;
    if f.fresh {
        let p = f.arg(ctx, 3)?;
        let area = rect(ctx, p)?;
        put_rect(ctx, f.address + 128, area)?;
        if cdc == 0 {
            let hwnd = word(ctx, o + 0x20)?;
            return f.call_service(ctx, 20, "RedrawWindow", &[hwnd, p, 0, 0x41]);
        }
        let kind = f.arg(ctx, 2)?;
        if kind > 3 {
            return Err(KernelError::Dispatch(
                "invalid splitter drawing kind".into(),
            ));
        }
        return f.call_service(
            ctx,
            if kind == 0 { 10 } else { 1 },
            "GetSysColor",
            &[if kind == 3 { 6 } else { 15 }],
        );
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => f.call_service(
            ctx,
            2,
            super::solid_drawing::SOLID,
            &[cdc, f.address + 128, result],
        ),
        10 => {
            f.set(ctx, 0, result)?;
            f.call_service(ctx, 11, "GetSysColor", &[6])
        }
        11 => {
            let face = f.get(ctx, 0)?;
            f.call_service(
                ctx,
                12,
                super::solid_drawing::BORDER,
                &[cdc, f.address + 128, face, result],
            )
        }
        12 => {
            shrink(ctx, f.address + 128)?;
            f.call_service(ctx, 13, "GetSysColor", &[20])
        }
        13 => {
            f.set(ctx, 1, result)?;
            f.call_service(ctx, 14, "GetSysColor", &[16])
        }
        14 => {
            let highlight = f.get(ctx, 1)?;
            f.call_service(
                ctx,
                15,
                super::solid_drawing::BORDER,
                &[cdc, f.address + 128, highlight, result],
            )
        }
        15 => {
            shrink(ctx, f.address + 128)?;
            let face = f.get(ctx, 0)?;
            f.call_service(
                ctx,
                2,
                super::solid_drawing::SOLID,
                &[cdc, f.address + 128, face],
            )
        }
        2 | 20 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter painting continuation".into(),
        )),
    }
}
fn shrink(ctx: &mut CallCtx<'_>, p: u32) -> Result<(), KernelError> {
    let [l, t, r, b] = rect(ctx, p)?;
    put_rect(
        ctx,
        p,
        [
            l.wrapping_add(1),
            t.wrapping_add(1),
            r.wrapping_sub(1),
            b.wrapping_sub(1),
        ],
    )
}
