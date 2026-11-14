//! CerfDemo's owner-drawn More arrow and the classic CE GDI frame APIs.
use super::*;
use pocket_kernel::windowing;

fn read_rect(ctx: &mut CallCtx<'_>, pointer: u32) -> Result<[i32; 4], KernelError> {
    let bytes = ctx.cpu.read_mem(pointer, 16)?;
    Ok(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
fn put_rect(ctx: &mut CallCtx<'_>, pointer: u32, r: [i32; 4]) -> Result<(), KernelError> {
    let bytes: Vec<_> = r.into_iter().flat_map(i32::to_le_bytes).collect();
    ctx.cpu.write_mem(pointer, &bytes)?;
    Ok(())
}
pub(super) fn frame_control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    let kind = ctx.arg_u32(2)?;
    let state = ctx.arg_u32(3)?;
    if pointer == 0
        || !windowing::frame_control_supported(kind, state)
        || ctx.kernel.gdi.dc(dc).is_none()
    {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let r = read_rect(ctx, pointer)?;
    if r[0] >= r[2] || r[1] >= r[3] {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    sync_dib_from_guest(ctx, dc)?;
    {
        let Some(mut surface) = surface_for_dc(ctx.kernel, dc) else {
            return Ok(DispatchOutcome::ReturnedR0(0));
        };
        windowing::draw_frame_control(&mut surface, r, kind, state);
        surface.mark_dirty();
    }
    sync_dst_dib_to_guest(ctx, dc)?;
    if kind == 4 && state & 0xff == 16 && state & 0x2000 != 0 {
        put_rect(ctx, pointer, [r[0] + 2, r[1] + 2, r[2] - 2, r[3] - 2])?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(super) fn edge(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    let edge = ctx.arg_u32(2)?;
    let flags = ctx.arg_u32(3)?;
    if pointer == 0 || ctx.kernel.gdi.dc(dc).is_none() {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let r = read_rect(ctx, pointer)?;
    sync_dib_from_guest(ctx, dc)?;
    let adjusted;
    {
        let Some(mut surface) = surface_for_dc(ctx.kernel, dc) else {
            return Ok(DispatchOutcome::ReturnedR0(0));
        };
        adjusted = windowing::draw_edge(&mut surface, r, edge, flags);
        surface.mark_dirty();
    }
    sync_dst_dib_to_guest(ctx, dc)?;
    if flags & 0x2000 != 0 {
        put_rect(ctx, pointer, adjusted)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(super) fn focus_rect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    if pointer == 0 || ctx.kernel.gdi.dc(dc).is_none() {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let r = read_rect(ctx, pointer)?;
    sync_dib_from_guest(ctx, dc)?;
    {
        let Some(mut surface) = surface_for_dc(ctx.kernel, dc) else {
            return Ok(DispatchOutcome::ReturnedR0(0));
        };
        let (w, h) = surface.dimensions();
        let old = surface.pixels().to_vec();
        for y in r[1].max(0)..r[3].min(h as i32) {
            for x in r[0].max(0)..r[2].min(w as i32) {
                if (x == r[0] || x == r[2] - 1 || y == r[1] || y == r[3] - 1) && (x + y) & 1 == 0 {
                    let i = ((y as u32 * w + x as u32) * 2) as usize;
                    surface.put_pixel(x, y, !u16::from_le_bytes([old[i], old[i + 1]]));
                }
            }
        }
        surface.mark_dirty();
    }
    sync_dst_dib_to_guest(ctx, dc)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}

pub(super) fn window_dc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    if !ctx.kernel.ce.windows.enabled
        || !ctx.kernel.window_rects.contains_key(&hwnd)
        || ctx.kernel.controls.get(hwnd).is_some()
        || ctx.kernel.controls.panel(hwnd).is_some()
    {
        return get_dc(ctx);
    }
    let dc = if let Some(&dc) = ctx.kernel.ce.windows.window_dcs.get(&hwnd) {
        dc
    } else {
        let dc = ctx.kernel.gdi.create_memory_dc();
        let bitmap = ctx.kernel.gdi.create_compatible_bitmap(1, 1);
        ctx.kernel.gdi.bitmap_mut(bitmap).unwrap().painted = Some(vec![0]);
        ctx.kernel.gdi.dc_mut(dc).unwrap().selected_bitmap = Some(bitmap);
        ctx.kernel.ce.windows.window_dcs.insert(hwnd, dc);
        dc
    };
    refresh_nonclient(ctx.kernel, hwnd);
    Ok(DispatchOutcome::ReturnedR0(dc))
}

pub(super) fn refresh_nonclient(kernel: &mut KernelState, hwnd: u32) {
    let Some(dc) = kernel.ce.windows.window_dcs.get(&hwnd).copied() else {
        return;
    };
    let Some((_, _, w, h)) = kernel.window_rects.get(&hwnd).copied() else {
        return;
    };
    let Some(bitmap) = kernel.gdi.dc(dc).and_then(|d| d.selected_bitmap) else {
        return;
    };
    let Some(old) = kernel.gdi.bitmap(bitmap).cloned() else {
        return;
    };
    let (w, h) = (w.clamp(1, 2048) as u32, h.clamp(1, 2048) as u32);
    let mut updated = pocket_kernel::gdi::Bitmap::new(w, h);
    let metrics = kernel.window_metrics(hwnd);
    let style = kernel.window_styles.get(&hwnd).map_or(0, |s| s.0);
    let title = kernel.window_titles.get(&hwnd).map_or("", String::as_str);
    let mut font = pocket_kernel::gdi::Font::new(-13);
    for (i, ch) in "Tahoma".encode_utf16().enumerate() {
        font.logfont[28 + i * 2..30 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    pocket_kernel::text::bind_resource(&kernel.gdi.installed_fonts, &mut font);
    windowing::draw_window_frame(
        &mut Surface::Bitmap(&mut updated),
        (0, 0, w as i32, h as i32),
        style,
        metrics,
        title,
        kernel.active_window == hwnd,
        None,
        Some(&font),
        kernel.window_icon(hwnd),
    );
    if let Some(client) = kernel
        .window_dcs
        .get(&hwnd)
        .and_then(|d| kernel.gdi.dc(*d))
        .and_then(|d| d.selected_bitmap)
        .and_then(|b| kernel.gdi.bitmap(b))
    {
        let [x, y, _, _] = metrics.insets();
        let (cw, ch) = metrics.client_size(w as i32, h as i32);
        Surface::Bitmap(&mut updated).blit_window_clipped(x, y, client, cw as u32, ch as u32);
    }
    // Readable default pixels are not application-owned non-client paint.
    // Caption state changes must still reveal the current system frame.
    updated.painted = Some(vec![0; (w * h) as usize]);
    if let Some(mask) = &old.painted {
        for y in 0..old.height.min(h) as usize {
            for x in 0..old.width.min(w) as usize {
                let src = y * old.width as usize + x;
                if mask[src] != 0 {
                    let dst = y * w as usize + x;
                    updated.pixels[dst * 2..dst * 2 + 2]
                        .copy_from_slice(&old.pixels[src * 2..src * 2 + 2]);
                    updated.painted.as_mut().unwrap()[dst] = 1;
                }
            }
        }
    }
    updated.paint_generation = old.paint_generation;
    updated.published_generation = old.published_generation;
    *kernel.gdi.bitmap_mut(bitmap).unwrap() = updated;
}
pub(super) fn adjust_window_rect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    if pointer == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let style = ctx.arg_u32(1)?;
    let menu = ctx.arg_u32(2)? != 0;
    let extended = ctx.arg_u32(3)?;
    let r = read_rect(ctx, pointer)?;
    // Preserve legacy game geometry unless a classic CE window profile is selected.
    if ctx.kernel.ce.windows.enabled {
        put_rect(
            ctx,
            pointer,
            windowing::Metrics::new(style, extended, menu).adjust(r),
        )?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
