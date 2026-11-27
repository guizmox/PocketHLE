//! CDC solid/3-D rectangles share actual USER/GDI DC state and clipping.
use super::callbacks::Frame;
use super::splitter_hit::{put_rect, rect};
use super::*;
pub(super) const SOLID: &str = "__PocketHLE_MFC_FillSolidRect";
pub(super) const BORDER: &str = "__PocketHLE_MFC_Draw3dRect";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1152, "?FillSolidRect@CDC@@QAAXHHHHK@Z", solid_xy),
    (1153, "?FillSolidRect@CDC@@QAAXPBUtagRECT@@K@Z", solid),
    (1097, "?Draw3dRect@CDC@@QAAXHHHHKK@Z", border_xy),
    (1098, "?Draw3dRect@CDC@@QAAXPBUtagRECT@@KK@Z", border),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", SOLID, solid);
    d.register_handler("coredll.dll", BORDER, border);
}
fn solid_xy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let cdc = f.arg(ctx, 0)?;
        let x = f.arg(ctx, 1)? as i32;
        let y = f.arg(ctx, 2)? as i32;
        let w = f.arg(ctx, 3)? as i32;
        let h = ctx.arg_u32(4)? as i32;
        let color = ctx.arg_u32(5)?;
        put_rect(
            ctx,
            f.address + 128,
            [
                x.min(x.wrapping_add(w)),
                y.min(y.wrapping_add(h)),
                x.max(x.wrapping_add(w)),
                y.max(y.wrapping_add(h)),
            ],
        )?;
        return f.call_service(ctx, 1, SOLID, &[cdc, f.address + 128, color]);
    }
    f.finish(ctx, 0)
}
fn solid(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let cdc = f.arg(ctx, 0)?;
    let dc = word(ctx, cdc + 4)?;
    if f.fresh {
        let p = f.arg(ctx, 1)?;
        let bounds = rect(ctx, p)?;
        put_rect(ctx, f.address + 128, bounds)?;
        let color = f.arg(ctx, 2)?;
        return f.call_service(ctx, 1, "SetBkColor", &[dc, color]);
    }
    if f.phase(ctx)? == 1 {
        return f.call_service(
            ctx,
            2,
            "ExtTextOutW",
            &[dc, 0, 0, 2, f.address + 128, 0, 0, 0],
        );
    }
    f.finish(ctx, 0)
}
fn border_xy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let cdc = f.arg(ctx, 0)?;
        let x = f.arg(ctx, 1)? as i32;
        let y = f.arg(ctx, 2)? as i32;
        let w = f.arg(ctx, 3)? as i32;
        let h = ctx.arg_u32(4)? as i32;
        let tl = ctx.arg_u32(5)?;
        let br = ctx.arg_u32(6)?;
        put_rect(
            ctx,
            f.address + 128,
            [x, y, x.wrapping_add(w), y.wrapping_add(h)],
        )?;
        return f.call_service(ctx, 1, BORDER, &[cdc, f.address + 128, tl, br]);
    }
    f.finish(ctx, 0)
}
fn border(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let cdc = f.arg(ctx, 0)?;
    if f.fresh {
        let p = f.arg(ctx, 1)?;
        let bounds = rect(ctx, p)?;
        put_rect(ctx, f.address + 128, bounds)?;
    } else {
        let i = f.get(ctx, 0)?;
        f.set(ctx, 0, i + 1)?;
    }
    let i = f.get(ctx, 0)?;
    if i == 4 {
        return f.finish(ctx, 0);
    }
    let [l, t, r, b] = rect(ctx, f.address + 128)?;
    let area = match i {
        0 => [l, t, r.wrapping_sub(1), t.wrapping_add(1)],
        1 => [l, t, l.wrapping_add(1), b.wrapping_sub(1)],
        2 => [r.wrapping_sub(1), t, r, b],
        _ => [l, b.wrapping_sub(1), r, b],
    };
    put_rect(ctx, f.address + 144, area)?;
    let color = f.arg(ctx, if i < 2 { 2 } else { 3 })?;
    f.call_service(ctx, 1, SOLID, &[cdc, f.address + 144, color])
}
