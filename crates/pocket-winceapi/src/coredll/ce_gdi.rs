//! CE shell drawing helpers. Portable Explorer excludes its wallpaper areas
//! and uses a DIB pattern brush for the desktop selection rectangle.
use super::*;

pub(super) fn register(d: &mut WinCeDispatcher, dll: &str) {
    d.register_handler(dll, "ExcludeClipRect", exclude_clip_rect);
    d.register_handler(dll, "IntersectClipRect", intersect_clip_rect);
    d.register_handler(dll, "CharUpperBuffW", char_upper_buff);
    d.register_handler(dll, "CreateDIBPatternBrushPt", dib_pattern_brush);
    d.register_handler(dll, "CreatePatternBrush", pattern_brush);
    d.register_handler(dll, "SetBrushOrgEx", set_brush_origin);
    d.register_handler(dll, "GetBrushOrgEx", get_brush_origin);
    d.register_handler(dll, "CeOidGetInfo", oid_info);
}

pub(super) fn dimensions(k: &KernelState, dc: u32) -> Option<(u32, u32)> {
    let dc = k.gdi.dc(dc)?;
    match dc.surface {
        pocket_kernel::gdi::DcSurface::Screen => Some((k.framebuffer.width, k.framebuffer.height)),
        pocket_kernel::gdi::DcSurface::Memory => {
            let b = k.gdi.bitmap(dc.selected_bitmap?)?;
            Some((b.width, b.height))
        }
    }
}

pub(super) fn intersect_clip_rect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let Some((width, height)) = dimensions(ctx.kernel, dc) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let bounds = [
        ctx.arg_u32(1)? as i32,
        ctx.arg_u32(2)? as i32,
        ctx.arg_u32(3)? as i32,
        ctx.arg_u32(4)? as i32,
    ];
    // Pocket Word's native RichEdit clips its text layout before painting.
    // Keep this in the DC so SaveDC/RestoreDC and every primitive share it;
    // changing a clip does not read or publish any framebuffer pixels.
    let input = ctx
        .kernel
        .gdi
        .dc(dc)
        .unwrap()
        .clip_rects
        .clone()
        .unwrap_or_else(|| vec![[0, 0, width as i32, height as i32]]);
    let out: Vec<_> = input
        .into_iter()
        .map(|rect| {
            [
                rect[0].max(bounds[0]),
                rect[1].max(bounds[1]),
                rect[2].min(bounds[2]),
                rect[3].min(bounds[3]),
            ]
        })
        .filter(|rect| rect[0] < rect[2] && rect[1] < rect[3])
        .collect();
    let kind = clipping::region_kind(&out);
    ctx.kernel.gdi.dc_mut(dc).unwrap().clip_rects = Some(out);
    Ok(DispatchOutcome::ReturnedR0(kind))
}

fn exclude_clip_rect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let Some((w, h)) = dimensions(ctx.kernel, dc) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let cut = [
        ctx.arg_u32(1)? as i32,
        ctx.arg_u32(2)? as i32,
        ctx.arg_u32(3)? as i32,
        ctx.arg_u32(4)? as i32,
    ];
    let input = ctx
        .kernel
        .gdi
        .dc(dc)
        .unwrap()
        .clip_rects
        .clone()
        .unwrap_or_else(|| vec![[0, 0, w as i32, h as i32]]);
    let mut out = Vec::new();
    for r in input {
        let c = [
            r[0].max(cut[0]),
            r[1].max(cut[1]),
            r[2].min(cut[2]),
            r[3].min(cut[3]),
        ];
        if c[0] >= c[2] || c[1] >= c[3] {
            out.push(r);
            continue;
        }
        out.extend(
            [
                [r[0], r[1], r[2], c[1]],
                [r[0], c[3], r[2], r[3]],
                [r[0], c[1], c[0], c[3]],
                [c[2], c[1], r[2], c[3]],
            ]
            .into_iter()
            .filter(|r| r[0] < r[2] && r[1] < r[3]),
        );
    }
    let kind = clipping::region_kind(&out);
    ctx.kernel.gdi.dc_mut(dc).unwrap().clip_rects = Some(out);
    Ok(DispatchOutcome::ReturnedR0(kind))
}

fn char_upper_buff(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    let count = ctx.arg_u32(1)?;
    let Some(size) = count.checked_mul(2) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    if count == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if pointer == 0
        || ctx
            .cpu
            .check_guest_access(pointer, size, Prot::READ | Prot::WRITE)
            .is_err()
    {
        set_thread_error(ctx, 87);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let mut bytes = ctx.cpu.read_mem(pointer, size)?;
    for unit in bytes.chunks_exact_mut(2) {
        let value = u16::from_le_bytes([unit[0], unit[1]]);
        if let Some(ch) = char::from_u32(u32::from(value)) {
            let mut upper = ch.to_uppercase();
            let first = upper.next().unwrap();
            // CE maps one WCHAR in place; expansions and unpaired surrogates
            // must not overwrite the following character in a fixed buffer.
            if upper.next().is_none() && first as u32 <= 0xffff {
                unit.copy_from_slice(&(first as u16).to_le_bytes());
            }
        }
    }
    ctx.cpu.write_mem(pointer, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(count))
}

fn oid_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Host-backed VFS find records advertise no CE object-store OIDs. Unknown
    // IDs must fail without inventing a CEFILEINFO or writing the output.
    let oid = ctx.arg_u32(0)?;
    set_thread_error(ctx, if oid == 0 { 87 } else { 2 });
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn pattern_brush(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    if ctx.kernel.gdi.bitmap(handle).is_none() {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let dc = ctx.kernel.gdi.create_memory_dc();
    ctx.kernel.gdi.select_into(dc, handle);
    let result = sync_dib_from_guest(ctx, dc);
    ctx.kernel.gdi.delete(dc);
    result?;
    let source = ctx.kernel.gdi.bitmap(handle).unwrap();
    let mut bitmap = pocket_kernel::gdi::Bitmap::new(source.width, source.height);
    bitmap.pixels.clone_from(&source.pixels);
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel.gdi.create_pattern_brush(bitmap),
    ))
}

fn dib_pattern_brush(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    let usage = ctx.arg_u32(1)?;
    if usage != 0 {
        set_thread_error(ctx, 50);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if pointer == 0 || ctx.cpu.check_guest_access(pointer, 40, Prot::READ).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let head = ctx.cpu.read_mem(pointer, 40)?;
    let field = |i| u32::from_le_bytes(head[i..i + 4].try_into().unwrap());
    let (width, height) = (field(4) as i32, field(8) as i32);
    let bpp = u16::from_le_bytes([head[14], head[15]]);
    if !(40..=124).contains(&field(0))
        || width <= 0
        || height == 0
        || u16::from_le_bytes([head[12], head[13]]) != 1
        || field(16) != 0
        || !matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32)
        || u64::from(width as u32) * u64::from(height.unsigned_abs())
            > pocket_kernel::imaging::MAX_IMAGE_PIXELS
    {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let colors = if bpp <= 8 {
        if field(32) == 0 {
            1 << bpp
        } else {
            field(32)
        }
    } else {
        0
    };
    if bpp <= 8 && colors > 1 << bpp {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let stride = (u64::from(width as u32) * u64::from(bpp)).div_ceil(32) * 4;
    let length =
        u64::from(field(0)) + u64::from(colors) * 4 + stride * u64::from(height.unsigned_abs());
    let Ok(length) = u32::try_from(length) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    if ctx
        .cpu
        .check_guest_access(pointer, length, Prot::READ)
        .is_err()
    {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let raw = ctx.cpu.read_mem(pointer, length)?;
    let Some(decoded) = decode_dib_to_rgb565(&raw) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let mut bitmap = pocket_kernel::gdi::Bitmap::new(decoded.dims.0, decoded.dims.1);
    bitmap.pixels = decoded.bytes;
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel.gdi.create_pattern_brush(bitmap),
    ))
}

fn set_brush_origin(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let (x, y, output) = (
        ctx.arg_u32(1)? as i32,
        ctx.arg_u32(2)? as i32,
        ctx.arg_u32(3)?,
    );
    let Some(old) = ctx.kernel.gdi.dc(dc).map(|d| d.brush_origin) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    if output != 0 {
        if ctx.cpu.check_guest_access(output, 8, Prot::WRITE).is_err() {
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        ctx.cpu
            .write_mem(output, &[old.0.to_le_bytes(), old.1.to_le_bytes()].concat())?;
    }
    ctx.kernel.gdi.dc_mut(dc).unwrap().brush_origin = (x, y);
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn get_brush_origin(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let output = ctx.arg_u32(1)?;
    let Some(origin) = ctx.kernel.gdi.dc(dc).map(|d| d.brush_origin) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    if output == 0 || ctx.cpu.check_guest_access(output, 8, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    ctx.cpu.write_mem(
        output,
        &[origin.0.to_le_bytes(), origin.1.to_le_bytes()].concat(),
    )?;
    Ok(DispatchOutcome::ReturnedR0(1))
}

pub(super) fn fill_pattern(
    surface: &mut Surface<'_>,
    rect: [i32; 4],
    bitmap: &pocket_kernel::gdi::Bitmap,
    origin: (i32, i32),
    rop: u32,
) {
    let (w, h) = surface.dimensions();
    for y in rect[1].max(0)..rect[3].min(h as i32) {
        for x in rect[0].max(0)..rect[2].min(w as i32) {
            let px = (i64::from(x) - i64::from(origin.0)).rem_euclid(i64::from(bitmap.width));
            let py = (i64::from(y) - i64::from(origin.1)).rem_euclid(i64::from(bitmap.height));
            let at = (py as usize * bitmap.width as usize + px as usize) * 2;
            let pattern = u16::from_le_bytes([bitmap.pixels[at], bitmap.pixels[at + 1]]);
            let at = (y as usize * w as usize + x as usize) * 2;
            let dst = u16::from_le_bytes([surface.pixels()[at], surface.pixels()[at + 1]]);
            surface.put_pixel(x, y, rop3::apply(rop, pattern, 0, dst));
        }
    }
}
