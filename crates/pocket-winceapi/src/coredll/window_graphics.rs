//! File timestamps, class graphics attributes, and scrolling with retained pixels.
use super::*;
pub(crate) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("GetFileTime", get_file_time as crate::Handler),
        ("CompareFileTime", compare_file_time),
        ("SetClassLongW", set_class_long),
        ("GetClassLongW", get_class_long),
        ("ScrollWindowEx", scroll_window_ex),
        ("ScrollWindow", scroll_window),
        ("CreateRectRgn", create_region),
        ("SetRectRgn", set_region),
        ("GetRgnBox", get_region_box),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn ret(v: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(v))
}
fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, error);
    ret(0)
}
pub(super) fn get_file_time(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let Some(times) = ctx.kernel.vfs.file_times(handle) else {
        return fail(ctx, 6);
    };
    let pointers = [ctx.arg_u32(1)?, ctx.arg_u32(2)?, ctx.arg_u32(3)?];
    for p in pointers {
        if p != 0 {
            ctx.cpu.read_mem(p, 8)?;
        }
    }
    for (p, value) in pointers.into_iter().zip(times) {
        if p != 0 {
            ctx.cpu.write_mem(p, &value.to_le_bytes())?;
        }
    }
    ret(1)
}
pub(super) fn compare_file_time(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let a = ctx.arg_u32(0)?;
    let b = ctx.arg_u32(1)?;
    let a = u64::from_le_bytes(ctx.cpu.read_mem(a, 8)?.try_into().unwrap());
    let b = u64::from_le_bytes(ctx.cpu.read_mem(b, 8)?.try_into().unwrap());
    ret(match a.cmp(&b) {
        std::cmp::Ordering::Less => u32::MAX,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })
}
fn class_name(ctx: &CallCtx<'_>, hwnd: u32) -> Option<String> {
    ctx.kernel
        .window_classes
        .get(&hwnd)
        .map(|n| n.to_lowercase())
}
pub(super) fn get_class_long(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)? as i32;
    let Some(name) = class_name(ctx, hwnd) else {
        return fail(ctx, 1400);
    };
    ret(ctx
        .kernel
        .gdi
        .class_values
        .get(&(name.clone(), index))
        .copied()
        .unwrap_or_else(|| {
            if index == -24 {
                ctx.kernel
                    .window_class_procs
                    .iter()
                    .find_map(|(n, p)| n.eq_ignore_ascii_case(&name).then_some(*p))
                    .unwrap_or(0)
            } else {
                0
            }
        }))
}
pub(super) fn set_class_long(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)? as i32;
    let value = ctx.arg_u32(2)?;
    let Some(name) = class_name(ctx, hwnd) else {
        return fail(ctx, 1400);
    };
    if !matches!(index, -26 | -24 | -20 | -18 | -16 | -14 | -12 | -10 | -8) {
        return fail(ctx, 1413);
    }
    let old = ctx
        .kernel
        .gdi
        .class_values
        .insert((name.clone(), index), value)
        .unwrap_or_else(|| {
            if index == -24 {
                ctx.kernel
                    .window_class_procs
                    .iter()
                    .find_map(|(n, p)| n.eq_ignore_ascii_case(&name).then_some(*p))
                    .unwrap_or(0)
            } else {
                0
            }
        });
    if index == -24 {
        let key = ctx
            .kernel
            .window_class_procs
            .keys()
            .find(|n| n.eq_ignore_ascii_case(&name))
            .cloned()
            .unwrap_or(name.clone());
        ctx.kernel.window_class_procs.insert(key, value);
        let windows: Vec<_> = ctx
            .kernel
            .window_classes
            .iter()
            .filter(|(_, n)| n.eq_ignore_ascii_case(&name))
            .map(|(h, _)| *h)
            .collect();
        for h in windows {
            if ctx.kernel.window_procs.get(&h) == Some(&old) {
                ctx.kernel.window_procs.insert(h, value);
            }
        }
    }
    ret(old)
}
fn read_rect_or(ctx: &mut CallCtx<'_>, p: u32, default: [i32; 4]) -> Result<[i32; 4], KernelError> {
    if p == 0 {
        return Ok(default);
    }
    let b = ctx.cpu.read_mem(p, 16)?;
    Ok(std::array::from_fn(|i| {
        i32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
fn write_rect_value(ctx: &mut CallCtx<'_>, p: u32, r: [i32; 4]) -> Result<(), KernelError> {
    if p != 0 {
        ctx.cpu.write_mem(
            p,
            &r.into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>(),
        )?;
    }
    Ok(())
}
fn intersection(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
    let r = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    if r[0] >= r[2] || r[1] >= r[3] {
        [0; 4]
    } else {
        r
    }
}
fn subtract(a: [i32; 4], b: [i32; 4]) -> Vec<[i32; 4]> {
    if a[0] >= a[2] || a[1] >= a[3] {
        return vec![];
    }
    let b = intersection(a, b);
    if b == [0; 4] {
        return vec![a];
    }
    [
        [a[0], a[1], a[2], b[1]],
        [a[0], b[3], a[2], a[3]],
        [a[0], b[1], b[0], b[3]],
        [b[2], b[1], a[2], b[3]],
    ]
    .into_iter()
    .filter(|r| r[0] < r[2] && r[1] < r[3])
    .collect()
}
fn bounds(rects: &[[i32; 4]]) -> [i32; 4] {
    rects
        .iter()
        .copied()
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
        .unwrap_or([0; 4])
}
fn complexity(rects: &[[i32; 4]]) -> u32 {
    if rects.is_empty() {
        1
    } else if rects.len() == 1 {
        2
    } else {
        3
    }
}
fn create_region(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let r = [
        ctx.arg_u32(0)? as i32,
        ctx.arg_u32(1)? as i32,
        ctx.arg_u32(2)? as i32,
        ctx.arg_u32(3)? as i32,
    ];
    let rects = if r[0] < r[2] && r[1] < r[3] {
        vec![r]
    } else {
        vec![]
    };
    ret(ctx.kernel.gdi.create_region(rects))
}
fn set_region(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let r = [
        ctx.arg_u32(1)? as i32,
        ctx.arg_u32(2)? as i32,
        ctx.arg_u32(3)? as i32,
        ctx.arg_u32(4)? as i32,
    ];
    let Some(region) = ctx.kernel.gdi.regions.get_mut(&handle) else {
        return fail(ctx, 6);
    };
    *region = if r[0] < r[2] && r[1] < r[3] {
        vec![r]
    } else {
        vec![]
    };
    ret(1)
}
pub(super) fn get_region_box(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let Some(region) = ctx.kernel.gdi.regions.get(&handle) else {
        return fail(ctx, 6);
    };
    let r = bounds(region);
    let kind = complexity(region);
    write_rect_value(ctx, out, r)?;
    ret(kind)
}
fn scroll_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let dx = ctx.arg_u32(1)? as i32;
    let dy = ctx.arg_u32(2)? as i32;
    let source = ctx.arg_u32(3)?;
    let clip = ctx.arg_u32(4)?;
    let result = scroll(ctx, hwnd, dx, dy, source, clip, 0, 0, 6)?;
    ret(u32::from(result != 0))
}
pub(super) fn scroll_window_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let dx = ctx.arg_u32(1)? as i32;
    let dy = ctx.arg_u32(2)? as i32;
    let source = ctx.arg_u32(3)?;
    let clip = ctx.arg_u32(4)?;
    let region = ctx.arg_u32(5)?;
    let out = ctx.arg_u32(6)?;
    let flags = ctx.arg_u32(7)?;
    ret(scroll(ctx, hwnd, dx, dy, source, clip, region, out, flags)?)
}
#[allow(clippy::too_many_arguments)]
fn scroll(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    dx: i32,
    dy: i32,
    source: u32,
    clip: u32,
    region: u32,
    out: u32,
    flags: u32,
) -> Result<u32, KernelError> {
    if hwnd == 0
        || !(hwnd == FAKE_HWND
            || ctx.kernel.window_classes.contains_key(&hwnd)
            || ctx.kernel.controls.get(hwnd).is_some())
    {
        set_thread_error(ctx, 1400);
        return Ok(0);
    }
    if region != 0 && !ctx.kernel.gdi.regions.contains_key(&region) {
        set_thread_error(ctx, 6);
        return Ok(0);
    }
    if out != 0 {
        ctx.cpu.read_mem(out, 16)?;
    }
    let dc = window_dc(ctx, hwnd);
    sync_dib_from_guest(ctx, dc)?;
    let (w, h) = surface_for_dc(ctx.kernel, dc).unwrap().dimensions();
    let client = [0, 0, w as i32, h as i32];
    let source = intersection(read_rect_or(ctx, source, client)?, client);
    let clip = intersection(read_rect_or(ctx, clip, client)?, client);
    let shifted = [
        source[0].saturating_add(dx),
        source[1].saturating_add(dy),
        source[2].saturating_add(dx),
        source[3].saturating_add(dy),
    ];
    let dest = intersection(shifted, clip);
    let exposed = subtract(intersection(source, clip), dest);
    if dx != 0 || dy != 0 {
        let mut surf = surface_for_dc(ctx.kernel, dc).unwrap();
        let before = surf.pixels().to_vec();
        let mut changed = false;
        for y in dest[1]..dest[3] {
            for x in dest[0]..dest[2] {
                let sx = i64::from(x) - i64::from(dx);
                let sy = i64::from(y) - i64::from(dy);
                if sx < i64::from(source[0])
                    || sx >= i64::from(source[2])
                    || sy < i64::from(source[1])
                    || sy >= i64::from(source[3])
                {
                    continue;
                }
                let from = (sy as usize * w as usize + sx as usize) * 2;
                let to = (y as usize * w as usize + x as usize) * 2;
                if surf.pixels()[to..to + 2] != before[from..from + 2] {
                    changed = true;
                    surf.pixels_mut()[to..to + 2].copy_from_slice(&before[from..from + 2]);
                }
            }
        }
        if changed {
            surf.mark_dirty();
        }
    }
    if region != 0 {
        *ctx.kernel.gdi.regions.get_mut(&region).unwrap() = exposed.clone();
    }
    write_rect_value(ctx, out, bounds(&exposed))?;
    if flags & 1 != 0 {
        let mut children: Vec<_> = ctx
            .kernel
            .window_parents
            .iter()
            .filter(|(_, p)| **p == hwnd)
            .map(|(h, _)| *h)
            .collect();
        children.extend(ctx.kernel.controls.children_of(hwnd));
        children.sort();
        children.dedup();
        for child in children {
            let r = ctx
                .kernel
                .controls
                .get(child)
                .map(|c| (c.x, c.y, c.w, c.h))
                .or_else(|| ctx.kernel.window_rects.get(&child).copied());
            let Some((x, y, cw, ch)) = r else {
                continue;
            };
            if intersection([x, y, x.saturating_add(cw), y.saturating_add(ch)], source) == [0; 4] {
                continue;
            }
            let (nx, ny) = (x.saturating_add(dx), y.saturating_add(dy));
            if let Some(r) = ctx.kernel.window_rects.get_mut(&child) {
                r.0 = nx;
                r.1 = ny;
            }
            if let Some(c) = ctx.kernel.controls.get_mut(child) {
                c.x = nx;
                c.y = ny;
            }
            if let Some(p) = ctx.kernel.controls.panel_mut(child) {
                p.x = nx;
                p.y = ny;
            }
            if ctx.kernel.posted_messages.len() < 256 {
                ctx.kernel.posted_messages.push_back((
                    child,
                    3,
                    0,
                    (nx as u32 & 0xffff) | ((ny as u32 & 0xffff) << 16),
                ));
            }
            if (nx, ny) != (x, y) {
                ctx.kernel.framebuffer.mark_dirty();
            }
        }
    }
    if !exposed.is_empty() && flags & 6 != 0 {
        if flags & 6 == 6 {
            ctx.kernel
                .gdi
                .scroll_erases
                .entry(hwnd)
                .or_default()
                .extend(exposed.iter().copied());
            if ctx.kernel.posted_messages.len() < 256 {
                ctx.kernel
                    .posted_messages
                    .push_back((hwnd, WM_ERASEBKGND, dc, 0));
            }
        }
        queue_window_paint(ctx.kernel, hwnd);
    }
    sync_dst_dib_to_guest(ctx, dc)?;
    Ok(complexity(&exposed))
}
/// Apply deferred class-brush erasure only to newly exposed pixels.
pub(super) fn erase_scroll_region(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    dc: u32,
) -> Result<bool, KernelError> {
    let Some(rects) = ctx.kernel.gdi.scroll_erases.remove(&hwnd) else {
        return Ok(false);
    };
    let brush =
        class_name(ctx, hwnd).and_then(|n| ctx.kernel.gdi.class_values.get(&(n, -10)).copied());
    let color = brush
        .and_then(|b| class_background_color(ctx, b))
        .or(ctx.kernel.window_background);
    if let Some(color) = color {
        if let Some(mut surf) = surface_for_dc(ctx.kernel, dc) {
            for r in rects {
                surf.fill_rect(
                    r[0],
                    r[1],
                    r[2] - r[0],
                    r[3] - r[1],
                    colorref_to_rgb565(color),
                );
            }
            surf.mark_dirty();
        }
        sync_dst_dib_to_guest(ctx, dc)?;
    }
    Ok(true)
}
