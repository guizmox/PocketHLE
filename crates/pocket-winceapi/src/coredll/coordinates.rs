//! Coordinate conversion uses the same client origins as control composition.
use super::*;
pub(super) fn origin(kernel: &KernelState, mut hwnd: u32) -> Option<(i32, i32)> {
    let (mut x, mut y) = (0i32, 0i32);
    for _ in 0..64 {
        if hwnd == 0 || hwnd == FAKE_DESKTOP_HWND || hwnd == FAKE_HWND {
            return Some((x, y));
        }
        if let Some(panel) = kernel.controls.panel(hwnd) {
            let (px, py) = panel.client_origin();
            return Some((x.saturating_add(px), y.saturating_add(py)));
        }
        if let Some(control) = kernel.controls.get(hwnd) {
            x = x.saturating_add(control.x);
            y = y.saturating_add(control.y);
            hwnd = control.parent;
            continue;
        }
        let rect = kernel.window_rects.get(&hwnd)?;
        x = x.saturating_add(rect.0);
        y = y.saturating_add(rect.1);
        if !kernel
            .window_styles
            .get(&hwnd)
            .is_some_and(|s| s.0 & 0x4000_0000 != 0)
        {
            return Some((x, y));
        }
        let parent = kernel.window_parents.get(&hwnd).copied()?;
        if parent == hwnd {
            return None;
        }
        hwnd = parent;
    }
    None
}

fn convert(
    ctx: &mut CallCtx<'_>,
    from: u32,
    to: u32,
    ptr: u32,
    count: u32,
) -> Result<u32, KernelError> {
    let (Some(a), Some(b)) = (origin(ctx.kernel, from), origin(ctx.kernel, to)) else {
        set_thread_error(ctx, 1400);
        return Ok(0);
    };
    if ptr == 0 || count > 65536 {
        set_thread_error(ctx, 87);
        return Ok(0);
    }
    let dx = a.0.wrapping_sub(b.0);
    let dy = a.1.wrapping_sub(b.1);
    let mut raw = ctx.cpu.read_mem(ptr, count * 8)?;
    for p in raw.chunks_exact_mut(8) {
        let x = i32::from_le_bytes(p[..4].try_into().unwrap()).wrapping_add(dx);
        let y = i32::from_le_bytes(p[4..].try_into().unwrap()).wrapping_add(dy);
        p[..4].copy_from_slice(&x.to_le_bytes());
        p[4..].copy_from_slice(&y.to_le_bytes());
    }
    ctx.cpu.write_mem(ptr, &raw)?;
    Ok(((dy as u32 & 0xffff) << 16) | (dx as u32 & 0xffff))
}
pub(super) fn client_to_screen(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    if origin(ctx.kernel, hwnd).is_none() || ptr == 0 {
        set_thread_error(ctx, 1400);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    convert(ctx, hwnd, 0, ptr, 1)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(super) fn screen_to_client(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    if origin(ctx.kernel, hwnd).is_none() || ptr == 0 {
        set_thread_error(ctx, 1400);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    convert(ctx, 0, hwnd, ptr, 1)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(super) fn map_window_points(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let from = ctx.arg_u32(0)?;
    let to = ctx.arg_u32(1)?;
    let ptr = ctx.arg_u32(2)?;
    let count = ctx.arg_u32(3)?;
    Ok(DispatchOutcome::ReturnedR0(convert(
        ctx, from, to, ptr, count,
    )?))
}
