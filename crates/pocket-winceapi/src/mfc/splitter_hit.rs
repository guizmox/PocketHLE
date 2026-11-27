//! CE splitter hit codes and tracking rectangles, in guest client coordinates.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::Prot;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1616, "?HitTest@CSplitterWnd@@MBAHVCPoint@@@Z", hit),
    (
        1313,
        "?GetHitRect@CSplitterWnd@@MAAXHAAVCRect@@@Z",
        hit_rect,
    ),
];
pub(super) fn rect(ctx: &mut CallCtx<'_>, p: u32) -> Result<[i32; 4], KernelError> {
    Ok([
        word(ctx, p)? as i32,
        word(ctx, p + 4)? as i32,
        word(ctx, p + 8)? as i32,
        word(ctx, p + 12)? as i32,
    ])
}
pub(super) fn put_rect(ctx: &mut CallCtx<'_>, p: u32, r: [i32; 4]) -> Result<(), KernelError> {
    ctx.cpu.check_guest_access(p, 16, Prot::WRITE)?;
    for (i, v) in r.into_iter().enumerate() {
        put(ctx, p + i as u32 * 4, v as u32)?;
    }
    Ok(())
}
pub(super) fn offset(ctx: &mut CallCtx<'_>, p: u32, x: i32, y: i32) -> Result<(), KernelError> {
    let [l, t, r, b] = rect(ctx, p)?;
    put_rect(
        ctx,
        p,
        [
            l.wrapping_add(x),
            t.wrapping_add(y),
            r.wrapping_add(x),
            b.wrapping_add(y),
        ],
    )
}
fn contains(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}
fn deflated(ctx: &mut CallCtx<'_>, o: u32, p: u32) -> Result<[i32; 4], KernelError> {
    let [l, t, r, b] = rect(ctx, p)?;
    let x = word(ctx, o + 0x6c)? as i32;
    let y = word(ctx, o + 0x70)? as i32;
    Ok([
        l.wrapping_add(x),
        t.wrapping_add(y),
        r.wrapping_sub(x),
        b.wrapping_sub(y),
    ])
}
fn dimensions(ctx: &mut CallCtx<'_>, o: u32) -> Result<(u32, u32), KernelError> {
    let rows = word(ctx, o + 0x74)?;
    let cols = word(ctx, o + 0x78)?;
    if !(1..=16).contains(&rows) || !(1..=16).contains(&cols) {
        return Err(KernelError::Dispatch(
            "invalid splitter hit-test dimensions".into(),
        ));
    }
    Ok((rows, cols))
}
fn hit(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let hwnd = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 1, "GetClientRect", &[hwnd, f.address + 128]);
    }
    if f.phase(ctx)? == 1 {
        return f.call_virtual(ctx, 2, o, 64, &[f.address + 144]);
    }
    let x = f.arg(ctx, 1)? as i32;
    let y = f.arg(ctx, 2)? as i32;
    let full = deflated(ctx, o, f.address + 128)?;
    let inside = rect(ctx, f.address + 144)?;
    let (rows, cols) = dimensions(ctx, o)?;
    if word(ctx, o + 0x80)? != 0 && rows < word(ctx, o + 0x4c)? {
        let h = word(ctx, o + 0x58)? as i32 - 1;
        if contains([inside[2], full[1], full[2], full[1].wrapping_add(h)], x, y) {
            return f.finish(ctx, 1);
        }
    }
    if word(ctx, o + 0x7c)? != 0 && cols < word(ctx, o + 0x50)? {
        let w = word(ctx, o + 0x54)? as i32 - 1;
        if contains([full[0], inside[3], full[0].wrapping_add(w), full[3]], x, y) {
            return f.finish(ctx, 2);
        }
    }
    let mut col = None;
    let mut left = full[0];
    let cp = word(ctx, o + 0x84)?;
    let cx = word(ctx, o + 0x64)? as i32;
    for i in 0..cols - 1 {
        let size = word(ctx, cp + i * 12 + 8)? as i32;
        left = left.wrapping_add(size);
        if contains([left, full[1], left.wrapping_add(cx), full[3]], x, y) {
            col = Some(i);
            break;
        }
        left = left.wrapping_add(cx);
    }
    let mut row = None;
    let mut top = full[1];
    let rp = word(ctx, o + 0x88)?;
    let cy = word(ctx, o + 0x68)? as i32;
    for i in 0..rows - 1 {
        let size = word(ctx, rp + i * 12 + 8)? as i32;
        top = top.wrapping_add(size);
        if contains([full[0], top, full[2], top.wrapping_add(cy)], x, y) {
            row = Some(i);
            break;
        }
        top = top.wrapping_add(cy);
    }
    f.finish(
        ctx,
        match (row, col) {
            (Some(r), Some(c)) => 301 + r * 15 + c,
            (Some(r), None) => 101 + r,
            (None, Some(c)) => 201 + c,
            _ => 0,
        },
    )
}
fn hit_rect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let hit = f.arg(ctx, 1)?;
    let out = f.arg(ctx, 2)?;
    if f.fresh {
        ctx.cpu.check_guest_access(out, 16, Prot::WRITE)?;
        let hwnd = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 1, "GetClientRect", &[hwnd, f.address + 128]);
    }
    let [left, top, right, bottom] = deflated(ctx, o, f.address + 128)?;
    // The CE routine starts with equal one-pixel borders on both axes. Preserve
    // its actual RECT construction rather than introducing host window metrics.
    let mut x = top;
    let mut y = left;
    let mut width = right.wrapping_sub(left);
    let mut height = bottom.wrapping_sub(top);
    put(ctx, o + 0x94, 0)?;
    put(ctx, o + 0x98, 0)?;
    if hit == 1 || (101..=115).contains(&hit) {
        height = (word(ctx, o + 0x58)? as i32)
            .wrapping_add(1)
            .wrapping_sub((word(ctx, o + 0x70)? as i32).wrapping_mul(2));
        put(ctx, o + 0x98, (-(height / 2)) as u32)?;
        if hit >= 101 {
            let i = hit - 101;
            let (rows, _) = dimensions(ctx, o)?;
            if i >= rows - 1 {
                return Err(KernelError::Dispatch("invalid splitter row hit".into()));
            }
            let p = word(ctx, o + 0x88)?;
            let gap = word(ctx, o + 0x68)? as i32;
            for j in 0..i {
                y = y
                    .wrapping_add(word(ctx, p + j * 12 + 8)? as i32)
                    .wrapping_add(gap);
            }
            put(ctx, o + 0xa0, y as u32)?;
            y = y
                .wrapping_add(word(ctx, p + i * 12 + 8)? as i32)
                .wrapping_add(word(ctx, o + 0x60)? as i32)
                .wrapping_add(1);
        }
        let limit = word(ctx, o + 0xa8)? as i32;
        put(ctx, o + 0xa8, limit.wrapping_sub(height) as u32)?;
    } else if hit == 2 || (201..=215).contains(&hit) {
        width = (word(ctx, o + 0x54)? as i32)
            .wrapping_add(1)
            .wrapping_sub((word(ctx, o + 0x6c)? as i32).wrapping_mul(2));
        put(ctx, o + 0x94, (-(width / 2)) as u32)?;
        if hit >= 201 {
            let i = hit - 201;
            let (_, cols) = dimensions(ctx, o)?;
            if i >= cols - 1 {
                return Err(KernelError::Dispatch("invalid splitter column hit".into()));
            }
            let p = word(ctx, o + 0x84)?;
            let gap = word(ctx, o + 0x64)? as i32;
            for j in 0..i {
                x = x
                    .wrapping_add(word(ctx, p + j * 12 + 8)? as i32)
                    .wrapping_add(gap);
            }
            put(ctx, o + 0x9c, x as u32)?;
            x = x
                .wrapping_add(word(ctx, p + i * 12 + 8)? as i32)
                .wrapping_add(word(ctx, o + 0x5c)? as i32)
                .wrapping_add(1);
        }
        let limit = word(ctx, o + 0xa4)? as i32;
        put(ctx, o + 0xa4, limit.wrapping_sub(width) as u32)?;
    }
    put_rect(
        ctx,
        out,
        [x, y, x.wrapping_add(width), y.wrapping_add(height)],
    )?;
    f.finish(ctx, 0)
}
