//! Real resource-backed icon/image-list APIs, shared by COREDLL and COMMCTRL.
use super::*;
use pocket_kernel::imaging::Image;

pub(crate) fn register_for(d: &mut WinCeDispatcher, dll: &str) {
    for (name, handler) in [
        ("ImageList_Create", create as crate::Handler),
        ("ImageList_Destroy", destroy),
        ("ImageList_Add", add),
        ("ImageList_AddMasked", add_masked),
        ("ImageList_Replace", replace),
        ("ImageList_ReplaceIcon", replace_icon),
        ("ImageList_GetIcon", get_icon),
        ("ImageList_GetImageCount", count),
        ("ImageList_GetIconSize", get_size),
        ("ImageList_SetIconSize", set_size),
        ("ImageList_SetBkColor", set_background),
        ("ImageList_GetBkColor", get_background),
        ("ImageList_Draw", draw),
        ("ImageList_DrawEx", draw_ex),
        ("ImageList_LoadImage", load_list),
        ("ImageList_LoadImageW", load_list),
        ("ImageList_Remove", remove),
    ] {
        d.register_handler(dll, name, handler);
    }
}
pub(crate) fn register(d: &mut WinCeDispatcher) {
    register_for(d, "coredll.dll");
    for (name, handler) in [
        ("DrawIcon", draw_icon as crate::Handler),
        ("DrawIconEx", draw_icon_ex),
        ("DestroyIcon", destroy_icon),
        ("LoadImageW", load_image),
        ("LoadCursorW", load_cursor),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn ret(value: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn resource(
    ctx: &mut CallCtx<'_>,
    instance: u32,
    ty: u32,
    name: &ResourceKey,
) -> Result<Option<Vec<u8>>, KernelError> {
    let (entries, base) = ctx.kernel.resource_scope(instance);
    let entry = entries
        .iter()
        .find(|e| e.ty == ResourceKey::Id(ty) && e.name == *name)
        .cloned();
    match entry {
        Some(e) if e.size <= 32 * 1024 * 1024 => Ok(Some(
            ctx.cpu.read_mem(base.wrapping_add(e.data_rva), e.size)?,
        )),
        _ => Ok(None),
    }
}
fn dib_image(raw: &[u8]) -> Option<Image> {
    let Some(dib) = decode_dib_to_rgb565(raw) else {
        // Resource DIBs have no BMP file header. Reconstruct it for the
        // existing BMP decoder's RGB565 bitfields and compressed formats.
        if raw.len() < 40 {
            return None;
        }
        let field = |i| u32::from_le_bytes(raw[i..i + 4].try_into().unwrap());
        let width = i32::from_le_bytes(raw[4..8].try_into().ok()?);
        let height = i32::from_le_bytes(raw[8..12].try_into().ok()?);
        if width <= 0
            || height == 0
            || u64::from(width as u32) * u64::from(height.unsigned_abs())
                > pocket_kernel::imaging::MAX_IMAGE_PIXELS
        {
            return None;
        }
        let bpp = u16::from_le_bytes([raw[14], raw[15]]);
        let palette = if bpp <= 8 {
            if field(32) == 0 {
                1u32.checked_shl(u32::from(bpp))?
            } else {
                field(32)
            }
        } else {
            0
        };
        let masks = if field(0) == 40 {
            match field(16) {
                3 => 12,
                6 => 16,
                _ => 0,
            }
        } else {
            0
        };
        let offset = 14u32
            .checked_add(field(0))?
            .checked_add(palette.checked_mul(4)?)?
            .checked_add(masks)?;
        if offset as usize > raw.len() + 14 {
            return None;
        }
        let mut bmp = vec![0u8; 14];
        bmp[..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&((raw.len() + 14) as u32).to_le_bytes());
        bmp[10..14].copy_from_slice(&offset.to_le_bytes());
        bmp.extend_from_slice(raw);
        let decoded = image::load_from_memory(&bmp).ok()?.to_rgb8();
        let mut out = Image::new(decoded.width(), decoded.height())?;
        for (i, p) in decoded.pixels().enumerate() {
            out.pixels[i] = bgrx_to_rgb565(p[2], p[1], p[0]);
        }
        return Some(out);
    };
    let mut image = Image::new(dib.dims.0, dib.dims.1)?;
    for (p, b) in image.pixels.iter_mut().zip(dib.bytes.chunks_exact(2)) {
        *p = u16::from_le_bytes([b[0], b[1]]);
    }
    Some(image)
}
/// ICO DIB heights include XOR and AND planes; alpha-less 32-bit icons
/// still use the AND mask. Do not treat their zero alpha bytes as invisible.
pub(super) fn decode_icon(raw: &[u8]) -> Option<Image> {
    if raw.starts_with(b"\x89PNG\r\n\x1a\n") {
        if raw.len() < 24 {
            return None;
        }
        let w = u32::from_be_bytes(raw[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(raw[20..24].try_into().ok()?);
        if w == 0
            || h == 0
            || u64::from(w) * u64::from(h) > pocket_kernel::imaging::MAX_IMAGE_PIXELS
        {
            return None;
        }
        let png = image::load_from_memory(raw).ok()?.to_rgba8();
        let mut out = Image::new(png.width(), png.height())?;
        for (i, p) in png.pixels().enumerate() {
            out.pixels[i] = bgrx_to_rgb565(p[2], p[1], p[0]);
            out.alpha[i] = p[3];
        }
        return Some(out);
    }
    if raw.len() < 40 {
        return None;
    }
    let u32_at = |p| u32::from_le_bytes(raw[p..p + 4].try_into().unwrap());
    let width = i32::from_le_bytes(raw[4..8].try_into().unwrap());
    let doubled = i32::from_le_bytes(raw[8..12].try_into().unwrap());
    let bpp = u16::from_le_bytes([raw[14], raw[15]]);
    if width <= 0 || doubled == 0 || doubled.unsigned_abs() % 2 != 0 {
        return None;
    }
    let height = doubled.unsigned_abs() / 2;
    let _ = Image::new(width as u32, height)?;
    let palette = if bpp <= 8 {
        let used = u32_at(32);
        if used == 0 {
            1u32.checked_shl(u32::from(bpp))?
        } else {
            used
        }
    } else {
        0
    };
    let masks = if u32_at(0) == 40 {
        match u32_at(16) {
            3 => 12,
            6 => 16,
            _ => 0,
        }
    } else {
        0
    };
    let start = (u32_at(0) as usize)
        .checked_add((palette as usize).checked_mul(4)?)?
        .checked_add(masks)?;
    let stride = ((width as usize)
        .checked_mul(bpp as usize)?
        .checked_add(31)?
        / 32)
        .checked_mul(4)?;
    let mask_start = start.checked_add(stride.checked_mul(height as usize)?)?;
    if mask_start > raw.len() {
        return None;
    }
    let mut dib = raw.to_vec();
    let signed = if doubled < 0 {
        -(height as i32)
    } else {
        height as i32
    };
    dib[8..12].copy_from_slice(&signed.to_le_bytes());
    let mut image = dib_image(&dib)?;
    let has_alpha = bpp == 32
        && (0..height as usize)
            .any(|y| (0..width as usize).any(|x| raw[start + y * stride + x * 4 + 3] != 0));
    let mask_stride = (width as usize).div_ceil(32) * 4;
    let has_mask = mask_start.checked_add(mask_stride.checked_mul(height as usize)?)? <= raw.len();
    for y in 0..height as usize {
        for x in 0..width as usize {
            let row = if doubled > 0 {
                height as usize - 1 - y
            } else {
                y
            };
            let i = y * width as usize + x;
            let transparent =
                has_mask && raw[mask_start + row * mask_stride + x / 8] & (0x80 >> (x % 8)) != 0;
            image.alpha[i] = if has_alpha {
                raw[start + row * stride + x * 4 + 3]
            } else if transparent {
                0
            } else {
                255
            };
            image.invert[i] = bpp == 1 && transparent && image.pixels[i] != 0;
        }
    }
    Some(image)
}
fn resource_icon(
    ctx: &mut CallCtx<'_>,
    instance: u32,
    name: &ResourceKey,
    cursor: bool,
    w: u32,
    h: u32,
) -> Result<Option<Image>, KernelError> {
    let group_ty = if cursor { 12 } else { 14 };
    let image_ty = if cursor { 1 } else { 3 };
    let Some(group) = resource(ctx, instance, group_ty, name)? else {
        return Ok(None);
    };
    if group.len() < 6 {
        return Ok(None);
    }
    let n = u16::from_le_bytes([group[4], group[5]]) as usize;
    if n == 0 || n > 1024 || 6 + n * 14 > group.len() {
        return Ok(None);
    }
    let mut entries: Vec<_> = (0..n)
        .map(|i| {
            let p = 6 + i * 14;
            let (width, height) = if cursor {
                (
                    u16::from_le_bytes([group[p], group[p + 1]]) as u32,
                    u16::from_le_bytes([group[p + 2], group[p + 3]]) as u32,
                )
            } else {
                (
                    if group[p] == 0 {
                        256
                    } else {
                        u32::from(group[p])
                    },
                    if group[p + 1] == 0 {
                        256
                    } else {
                        u32::from(group[p + 1])
                    },
                )
            };
            let depth = u16::from_le_bytes([group[p + 6], group[p + 7]]);
            (
                width.abs_diff(w) + height.abs_diff(h),
                std::cmp::Reverse(depth),
                p,
            )
        })
        .collect();
    entries.sort();
    for (_, _, p) in entries {
        let id = u16::from_le_bytes([group[p + 12], group[p + 13]]);
        let Some(raw) = resource(ctx, instance, image_ty, &ResourceKey::Id(u32::from(id)))? else {
            continue;
        };
        let data = if cursor {
            raw.get(4..)
        } else {
            Some(raw.as_slice())
        };
        if let Some(image) = data.and_then(decode_icon).and_then(|im| im.scaled(w, h)) {
            return Ok(Some(image));
        }
    }
    Ok(None)
}
pub(super) fn load_icon(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    load_icon_kind(ctx, false)
}
pub(crate) fn load_cursor(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    load_icon_kind(ctx, true)
}
fn load_icon_kind(ctx: &mut CallCtx<'_>, cursor: bool) -> Result<DispatchOutcome, KernelError> {
    let instance = ctx.arg_u32(0)?;
    let raw = ctx.arg_u32(1)?;
    // Preserve the virtual system cursor handle. Application icons must
    // actually decode; never substitute this cursor for a missing asset.
    if instance == 0 && raw >= 32512 && raw <= 32651 {
        return ret(FAKE_ICON);
    }
    let name = read_wide_resource_key(ctx, raw)?;
    match resource_icon(ctx, instance, &name, cursor, 32, 32)? {
        Some(image) => ret(ctx.kernel.gdi.register_icon(image)),
        None => ret(0),
    }
}
pub(crate) fn load_image(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let instance = ctx.arg_u32(0)?;
    let name_raw = ctx.arg_u32(1)?;
    let name = read_wide_resource_key(ctx, name_raw)?;
    let kind = ctx.arg_u32(2)?;
    let w = ctx.arg_u32(3)?;
    let h = ctx.arg_u32(4)?;
    let flags = ctx.arg_u32(5)?;
    if flags & 0x10 != 0 {
        set_thread_error(ctx, 50);
        return ret(0);
    }
    if kind == 1 || kind == 2 {
        return match resource_icon(
            ctx,
            instance,
            &name,
            kind == 2,
            if w == 0 { 32 } else { w },
            if h == 0 { 32 } else { h },
        )? {
            Some(image) => ret(ctx.kernel.gdi.register_icon(image)),
            None => ret(0),
        };
    }
    if kind != 0 {
        set_thread_error(ctx, 87);
        return ret(0);
    }
    let Some(image) = resource(ctx, instance, 2, &name)?
        .as_deref()
        .and_then(dib_image)
    else {
        return ret(0);
    };
    let Some(image) = image.scaled(
        if w == 0 { image.width } else { w },
        if h == 0 { image.height } else { h },
    ) else {
        return ret(0);
    };
    let handle = ctx
        .kernel
        .gdi
        .create_compatible_bitmap(image.width, image.height);
    let bitmap = ctx.kernel.gdi.bitmap_mut(handle).unwrap();
    for (bytes, pixel) in bitmap.pixels.chunks_exact_mut(2).zip(image.pixels) {
        bytes.copy_from_slice(&pixel.to_le_bytes());
    }
    ret(handle)
}
pub(super) fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let w = ctx.arg_u32(0)?;
    let h = ctx.arg_u32(1)?;
    ret(ctx.kernel.gdi.create_image_list(w, h))
}
pub(super) fn destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    ret(u32::from(
        ctx.kernel.gdi.image_lists.remove(&handle).is_some(),
    ))
}
pub(super) fn destroy_icon(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    ret(u32::from(
        handle == FAKE_ICON || ctx.kernel.gdi.icons.remove(&handle).is_some(),
    ))
}
pub(super) fn count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    ret(ctx
        .kernel
        .gdi
        .image_lists
        .get(&handle)
        .map_or(0, |l| l.images.len() as u32))
}
fn bitmap_image(ctx: &mut CallCtx<'_>, handle: u32) -> Result<Option<Image>, KernelError> {
    let Some(bitmap) = ctx.kernel.gdi.bitmap(handle) else {
        return Ok(None);
    };
    if bitmap.dib_bits_va.is_some() {
        let dc = ctx.kernel.gdi.create_memory_dc();
        ctx.kernel.gdi.dc_mut(dc).unwrap().selected_bitmap = Some(handle);
        let result = sync_dib_from_guest(ctx, dc);
        ctx.kernel.gdi.delete(dc);
        result?;
    }
    Ok(ctx.kernel.gdi.bitmap(handle).and_then(Image::from_bitmap))
}
pub(super) fn add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add_impl(ctx, false)
}
pub(super) fn add_masked(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add_impl(ctx, true)
}
fn add_impl(ctx: &mut CallCtx<'_>, color_mask: bool) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let bitmap = ctx.arg_u32(1)?;
    let mask = ctx.arg_u32(2)?;
    let Some(mut image) = bitmap_image(ctx, bitmap)? else {
        return ret(u32::MAX);
    };
    if !color_mask && mask != 0 {
        let Some(bits) = bitmap_image(ctx, mask)? else {
            return ret(u32::MAX);
        };
        if bits.width != image.width || bits.height < image.height {
            return ret(u32::MAX);
        }
        for (i, a) in image.alpha.iter_mut().enumerate() {
            if bits.pixels[i] != 0 {
                *a = 0;
            }
        }
    }
    let key = if color_mask && mask != 0xffff_ffff {
        Some(if mask == 0xff00_0000 {
            image.pixels[0]
        } else {
            colorref_to_rgb565(mask)
        })
    } else {
        None
    };
    ret(ctx
        .kernel
        .gdi
        .image_lists
        .get_mut(&handle)
        .and_then(|l| l.add_strip(&image, key))
        .map_or(u32::MAX, |i| i as u32))
}
pub(super) fn replace_icon(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)? as i32;
    let icon = ctx.arg_u32(2)?;
    let Some(image) = ctx.kernel.gdi.icons.get(&icon).cloned() else {
        return ret(u32::MAX);
    };
    ret(ctx
        .kernel
        .gdi
        .image_lists
        .get_mut(&handle)
        .and_then(|l| l.replace(index, &image))
        .map_or(u32::MAX, |i| i as u32))
}
pub(super) fn replace(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)? as i32;
    let bitmap = ctx.arg_u32(2)?;
    let mask = ctx.arg_u32(3)?;
    if index < 0 {
        return ret(0);
    }
    let Some(mut image) = bitmap_image(ctx, bitmap)? else {
        return ret(0);
    };
    if mask != 0 {
        let Some(bits) = bitmap_image(ctx, mask)? else {
            return ret(0);
        };
        if bits.width != image.width || bits.height < image.height {
            return ret(0);
        }
        for (i, a) in image.alpha.iter_mut().enumerate() {
            if bits.pixels[i] != 0 {
                *a = 0;
            }
        }
    }
    ret(u32::from(
        ctx.kernel
            .gdi
            .image_lists
            .get_mut(&handle)
            .and_then(|l| l.replace(index, &image))
            .is_some(),
    ))
}
pub(super) fn get_icon(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let Some(image) = ctx
        .kernel
        .gdi
        .image_lists
        .get(&handle)
        .and_then(|l| l.images.get(index as usize))
        .cloned()
    else {
        return ret(0);
    };
    ret(ctx.kernel.gdi.register_icon(image))
}
pub(super) fn remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let Some(list) = ctx.kernel.gdi.image_lists.get_mut(&handle) else {
        return ret(0);
    };
    if index == u32::MAX {
        list.images.clear();
    } else if (index as usize) < list.images.len() {
        list.images.remove(index as usize);
    } else {
        return ret(0);
    }
    ret(1)
}
pub(super) fn get_size(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let x = ctx.arg_u32(1)?;
    let y = ctx.arg_u32(2)?;
    let Some(list) = ctx.kernel.gdi.image_lists.get(&handle) else {
        return ret(0);
    };
    let (w, h) = (list.width, list.height);
    if x == 0 || y == 0 {
        return ret(0);
    }
    ctx.cpu.write_mem(x, &w.to_le_bytes())?;
    ctx.cpu.write_mem(y, &h.to_le_bytes())?;
    ret(1)
}
pub(super) fn set_size(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let w = ctx.arg_u32(1)?;
    let h = ctx.arg_u32(2)?;
    if pocket_kernel::imaging::ImageList::new(w, h).is_none() {
        return ret(0);
    }
    let Some(list) = ctx.kernel.gdi.image_lists.get_mut(&handle) else {
        return ret(0);
    };
    list.width = w;
    list.height = h;
    list.images.clear();
    ret(1)
}
pub(super) fn set_background(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let color = ctx.arg_u32(1)?;
    let Some(list) = ctx.kernel.gdi.image_lists.get_mut(&handle) else {
        return ret(u32::MAX);
    };
    let old = list.background;
    list.background = color;
    ret(old)
}
pub(super) fn get_background(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    ret(ctx
        .kernel
        .gdi
        .image_lists
        .get(&handle)
        .map_or(u32::MAX, |l| l.background))
}
pub(super) fn load_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let instance = ctx.arg_u32(0)?;
    let name_raw = ctx.arg_u32(1)?;
    let name = read_wide_resource_key(ctx, name_raw)?;
    let cx = ctx.arg_u32(2)?;
    let mask = ctx.arg_u32(4)?;
    let kind = ctx.arg_u32(5)?;
    let flags = ctx.arg_u32(6)?;
    if flags & 0x10 != 0 {
        set_thread_error(ctx, 50);
        return ret(0);
    }
    let image = if kind == 0 {
        resource(ctx, instance, 2, &name)?
            .as_deref()
            .and_then(dib_image)
    } else if kind == 1 || kind == 2 {
        resource_icon(
            ctx,
            instance,
            &name,
            kind == 2,
            if cx == 0 { 32 } else { cx },
            if cx == 0 { 32 } else { cx },
        )?
    } else {
        None
    };
    let Some(image) = image else {
        return ret(0);
    };
    let width = if cx == 0 { image.width } else { cx };
    let handle = ctx.kernel.gdi.create_image_list(width, image.height);
    let key = if mask == u32::MAX {
        None
    } else {
        Some(if mask == 0xff00_0000 {
            image.pixels[0]
        } else {
            colorref_to_rgb565(mask)
        })
    };
    if handle == 0 {
        return ret(0);
    }
    if ctx
        .kernel
        .gdi
        .image_lists
        .get_mut(&handle)
        .unwrap()
        .add_strip(&image, key)
        .is_none()
    {
        ctx.kernel.gdi.image_lists.remove(&handle);
        return ret(0);
    }
    ret(handle)
}
fn paint(
    ctx: &mut CallCtx<'_>,
    image: &Image,
    dc: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    background: Option<u16>,
) -> Result<bool, KernelError> {
    if ctx.kernel.gdi.dc(dc).is_none() {
        return Ok(false);
    }
    sync_dib_from_guest(ctx, dc)?;
    let Some(mut surf) = surface_for_dc(ctx.kernel, dc) else {
        return Ok(false);
    };
    let (sw, sh) = surf.dimensions();
    if let Some(color) = background {
        surf.fill_rect(x, y, w as i32, h as i32, color);
    }
    image.draw(&mut surf, x, y, w, h, [0, 0, sw as i32, sh as i32]);
    surf.mark_dirty();
    drop(surf);
    sync_dst_dib_to_guest(ctx, dc)?;
    Ok(true)
}
pub(super) fn draw(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    draw_impl(ctx, false)
}
pub(super) fn draw_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    draw_impl(ctx, true)
}
fn draw_impl(ctx: &mut CallCtx<'_>, extended: bool) -> Result<DispatchOutcome, KernelError> {
    let list = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let dc = ctx.arg_u32(2)?;
    let x = ctx.arg_u32(3)? as i32;
    let y = ctx.arg_u32(4)? as i32;
    let Some(list) = ctx.kernel.gdi.image_lists.get(&list) else {
        return ret(0);
    };
    let Some(mut image) = list.images.get(index as usize).cloned() else {
        return ret(0);
    };
    let background = list.background;
    let (style, bg) = if extended {
        let w = ctx.arg_u32(5)?;
        let h = ctx.arg_u32(6)?;
        if w != 0 || h != 0 {
            let Some(cropped) = image.crop(
                0,
                0,
                if w == 0 {
                    image.width
                } else {
                    w.min(image.width)
                },
                if h == 0 {
                    image.height
                } else {
                    h.min(image.height)
                },
            ) else {
                return ret(0);
            };
            image = cropped;
        }
        (ctx.arg_u32(9)?, ctx.arg_u32(7)?)
    } else {
        (ctx.arg_u32(5)?, 0xff00_0000)
    };
    let bg = if bg == 0xff00_0000 { background } else { bg };
    if style & 0x10 != 0 {
        for (p, a) in image.pixels.iter_mut().zip(&mut image.alpha) {
            *p = if *a == 0 { 0xffff } else { 0 };
            *a = 255;
        }
        image.invert.fill(false);
    } else if style & 0x20 != 0 {
        image.alpha.fill(255);
        image.invert.fill(false);
    }
    if style & 6 != 0 && style & 0x10 == 0 {
        let foreground = if extended {
            ctx.arg_u32(8)?
        } else {
            0xff00_0000
        };
        let color = if foreground == 0xff00_0000 {
            sys_color(13)
        } else {
            foreground
        };
        for y in 0..image.height {
            for x in 0..image.width {
                let pattern = if style & 4 != 0 {
                    (x + y) & 1 == 0
                } else {
                    x & 1 == 0 && y & 1 == 0
                };
                if pattern {
                    let i = (y * image.width + x) as usize;
                    if color == u32::MAX {
                        image.alpha[i] = 0;
                    } else {
                        image.pixels[i] = colorref_to_rgb565(color);
                    }
                }
            }
        }
    }
    let background = if style & 1 == 0 && bg != u32::MAX {
        Some(colorref_to_rgb565(bg))
    } else {
        None
    };
    ret(u32::from(paint(
        ctx,
        &image,
        dc,
        x,
        y,
        image.width,
        image.height,
        background,
    )?))
}
pub(super) fn draw_icon(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    draw_icon_impl(ctx, false)
}
pub(super) fn draw_icon_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    draw_icon_impl(ctx, true)
}
fn draw_icon_impl(ctx: &mut CallCtx<'_>, extended: bool) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let x = ctx.arg_u32(1)? as i32;
    let y = ctx.arg_u32(2)? as i32;
    let handle = ctx.arg_u32(3)?;
    let Some(mut image) = ctx.kernel.gdi.icons.get(&handle).cloned() else {
        return ret(0);
    };
    let (w, h) = if extended {
        (ctx.arg_u32(4)?, ctx.arg_u32(5)?)
    } else {
        (32, 32)
    };
    let background = if extended {
        let flags = ctx.arg_u32(8)?;
        match flags & 3 {
            1 => {
                for (pixel, alpha) in image.pixels.iter_mut().zip(&mut image.alpha) {
                    *pixel = if *alpha == 0 { 0xffff } else { 0 };
                    *alpha = 255;
                }
                image.invert.fill(false);
            }
            2 => {
                image.alpha.fill(255);
                image.invert.fill(false);
            }
            3 => {}
            _ => return ret(0),
        }
        let brush = ctx.arg_u32(7)?;
        ctx.kernel
            .gdi
            .brush(brush)
            .map(|b| colorref_to_rgb565(b.color))
    } else {
        None
    };
    ret(u32::from(paint(
        ctx,
        &image,
        dc,
        x,
        y,
        if w == 0 { image.width } else { w },
        if h == 0 { image.height } else { h },
        background,
    )?))
}
