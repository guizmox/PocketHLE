//! Stateful 32-bit WinCE toolbar messages for native subclasses.
use super::*;
use pocket_kernel::gdi::Bitmap;

/// Standard toolbar strips are supplied by the HLE control, including when
/// portable Explorer is launched without any system DLL resources.
pub(super) fn standard_bitmap(ctx: &mut CallCtx<'_>, id: u32) -> Option<(u32, u32, i32)> {
    let (view, size, count) = match id {
        0 => (false, 16, 15),
        1 => (false, 24, 15),
        4 => (true, 16, 12),
        5 => (true, 24, 12),
        _ => return None,
    };
    let face = colorref_to_rgb565(sys_color(15));
    let mut strip = Bitmap::new(size * count, size);
    for i in 0..count {
        let mut icon = Bitmap::new(16, 16);
        let mut s = Surface::Bitmap(&mut icon);
        s.fill_rect(0, 0, 16, 16, face);
        standard_icon(&mut s, view, i);
        for y in 0..size {
            for x in 0..size {
                let src = ((y * 16 / size) * 16 + x * 16 / size) as usize * 2;
                let dst = (y * size * count + i * size + x) as usize * 2;
                strip.pixels[dst..dst + 2].copy_from_slice(&icon.pixels[src..src + 2]);
            }
        }
    }
    Some((ctx.kernel.gdi.register_dib(strip), count, size as i32))
}

fn standard_icon(s: &mut Surface<'_>, view: bool, index: u32) {
    let blue = 0x001f;
    let navy = 0x0010;
    let white = 0xffff;
    let yellow = 0xff08;
    let black = 0;
    let grey = 0x8410;
    let red = 0xf800;
    let line = |s: &mut Surface<'_>, x0: i32, y0: i32, x1: i32, y1: i32, color| {
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for n in 0..=steps {
            s.put_pixel(
                x0 + (x1 - x0) * n / steps,
                y0 + (y1 - y0) * n / steps,
                color,
            );
        }
    };
    let paper = |s: &mut Surface<'_>, x, y, w, h| {
        s.fill_rect(x, y, w, h, navy);
        s.fill_rect(x + 1, y + 1, w - 2, h - 2, white);
        for yy in (y + 3..y + h - 2).step_by(3) {
            s.fill_rect(x + 2, yy, w - 4, 1, grey);
        }
    };
    let folder = |s: &mut Surface<'_>| {
        s.fill_rect(1, 5, 14, 9, navy);
        s.fill_rect(2, 3, 6, 3, navy);
        s.fill_rect(3, 4, 4, 2, yellow);
        s.fill_rect(2, 6, 12, 7, yellow);
        s.fill_rect(2, 6, 12, 1, white);
    };
    let cross = |s: &mut Surface<'_>| {
        line(s, 4, 4, 12, 12, red);
        line(s, 5, 4, 13, 12, red);
        line(s, 12, 4, 4, 12, red);
        line(s, 13, 4, 5, 12, red);
    };
    if view {
        match index {
            0 | 1 => {
                let (step, size) = if index == 0 { (7, 5) } else { (5, 3) };
                for y in (2..14).step_by(step) {
                    for x in (2..14).step_by(step) {
                        s.fill_rect(x, y, size, size, navy);
                        s.fill_rect(x + 1, y + 1, size - 1, size - 1, yellow);
                    }
                }
            }
            2..=7 => {
                for y in [3, 7, 11] {
                    s.fill_rect(2, y, 2, 2, blue);
                    s.fill_rect(6, y, 7, 1, black);
                }
                if index == 3 {
                    for x in [8, 12] {
                        s.fill_rect(x, 2, 1, 12, grey);
                    }
                }
                if index >= 4 {
                    for n in 0..3 {
                        s.fill_rect(12 - n, 2 + n, 1 + 2 * n, 1, blue);
                    }
                }
            }
            8 | 11 => {
                folder(s);
                if index == 8 {
                    s.fill_rect(7, 6, 2, 6, blue);
                    for n in 0..4 {
                        s.fill_rect(7 - n, 4 + n, 2 + 2 * n, 1, blue);
                    }
                } else {
                    s.fill_rect(11, 1, 1, 5, red);
                    s.fill_rect(9, 3, 5, 1, red);
                }
            }
            9 | 10 => {
                for (x, y) in [(1, 2), (8, 8)] {
                    s.fill_rect(x, y, 7, 5, navy);
                    s.fill_rect(x + 1, y + 1, 5, 3, blue);
                    s.fill_rect(x + 3, y + 5, 1, 1, black);
                }
                line(s, 4, 8, 11, 5, black);
                if index == 10 {
                    cross(s);
                }
            }
            _ => {}
        }
        return;
    }
    match index {
        0 => {
            for x in [2, 9] {
                s.fill_rect(x, 10, 5, 4, navy);
                s.fill_rect(x + 1, 11, 3, 2, white);
            }
            line(s, 4, 11, 12, 2, grey);
            line(s, 11, 11, 3, 2, grey);
        }
        1 => {
            paper(s, 2, 2, 8, 10);
            paper(s, 6, 5, 8, 10);
        }
        2 => {
            s.fill_rect(3, 2, 10, 12, navy);
            s.fill_rect(4, 3, 8, 10, yellow);
            paper(s, 6, 5, 8, 10);
            s.fill_rect(6, 1, 4, 3, grey);
        }
        3 | 4 => {
            let mirror = |x| if index == 3 { x } else { 15 - x };
            for (a, b) in [((3, 6), (9, 4)), ((9, 4), (13, 7)), ((13, 7), (13, 11))] {
                line(s, mirror(a.0), a.1, mirror(b.0), b.1, blue);
            }
            for n in 0..4 {
                s.fill_rect(mirror(3 + n), 3 + n, 1, 1 + 2 * (3 - n), blue);
            }
        }
        5 => cross(s),
        6 | 10 => {
            paper(s, 4, 1, 9, 14);
            if index == 6 {
                s.fill_rect(1, 3, 5, 1, red);
                s.fill_rect(3, 1, 1, 5, red);
            }
        }
        7 => folder(s),
        8 => {
            s.fill_rect(2, 2, 12, 12, navy);
            s.fill_rect(3, 3, 10, 10, blue);
            s.fill_rect(5, 3, 6, 4, white);
            s.fill_rect(5, 9, 6, 4, white);
            s.fill_rect(9, 3, 1, 3, black);
        }
        9 => {
            paper(s, 2, 1, 9, 13);
            s.fill_rect(7, 7, 6, 6, navy);
            s.fill_rect(8, 8, 4, 4, white);
            line(s, 12, 12, 15, 15, navy);
        }
        11 => {
            s.fill_rect(3, 1, 10, 14, blue);
            s.fill_rect(6, 3, 5, 1, white);
            s.fill_rect(10, 4, 1, 3, white);
            s.fill_rect(8, 7, 3, 1, white);
            s.fill_rect(7, 8, 1, 2, white);
            s.fill_rect(7, 12, 1, 1, white);
        }
        12 => {
            for x in [1, 9] {
                s.fill_rect(x, 6, 6, 8, black);
                s.fill_rect(x + 1, 7, 4, 3, navy);
                s.fill_rect(x + 1, 3, 4, 3, grey);
            }
            s.fill_rect(6, 7, 4, 2, black);
        }
        13 => {
            paper(s, 1, 2, 6, 10);
            paper(s, 10, 5, 5, 10);
            s.fill_rect(6, 7, 5, 2, blue);
            line(s, 9, 5, 12, 8, blue);
            line(s, 9, 11, 12, 8, blue);
        }
        14 => {
            paper(s, 4, 1, 8, 9);
            s.fill_rect(1, 6, 14, 7, black);
            s.fill_rect(2, 7, 12, 5, grey);
            s.fill_rect(4, 10, 8, 5, white);
            s.put_pixel(12, 8, blue);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn add_bitmap(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    instance: u32,
    id: u32,
    requested: u32,
    width: i32,
    height: i32,
) -> Result<u32, KernelError> {
    if requested > 4096
        || width <= 0
        || height <= 0
        || width > 4096
        || height > 4096
        || !ctx
            .kernel
            .controls
            .get(hwnd)
            .is_some_and(|c| c.class == ControlClass::Toolbar)
    {
        return Ok(u32::MAX);
    }
    let owned = instance != 0;
    let (handle, automatic) = if instance == u32::MAX {
        let Some((bitmap, count, _)) = standard_bitmap(ctx, id) else {
            return Ok(u32::MAX);
        };
        (bitmap, count)
    } else {
        let handle = if instance == 0 {
            id
        } else {
            load_bitmap_resource(ctx, instance, &ResourceKey::Id(id), true)?
        };
        if handle == 0 {
            return Ok(u32::MAX);
        }
        let Some(bitmap) = ctx.kernel.gdi.bitmap(handle) else {
            return Ok(u32::MAX);
        };
        let automatic = bitmap.width / width as u32;
        if bitmap.dib_bits_va.is_some() {
            let dc = ctx.kernel.gdi.create_memory_dc();
            ctx.kernel.gdi.select_into(dc, handle);
            let result = sync_dib_from_guest(ctx, dc);
            ctx.kernel.gdi.delete(dc);
            result?;
        }
        (handle, automatic)
    };
    let count = if requested == 0 { automatic } else { requested };
    let valid = ctx.kernel.gdi.bitmap(handle).is_some_and(|b| {
        count > 0
            && count <= 4096
            && u64::from(count) * width as u64 <= u64::from(b.width)
            && height as u32 <= b.height
    });
    let first: u32 = ctx
        .kernel
        .controls
        .get(hwnd)
        .unwrap()
        .toolbar
        .bitmaps
        .iter()
        .map(|b| b.1)
        .sum();
    if !valid || first > 4096 - count.min(4096) {
        if owned {
            ctx.kernel.gdi.delete(handle);
        }
        return Ok(u32::MAX);
    }
    let bar = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().toolbar;
    bar.bitmap_size = (width, height);
    bar.bitmaps.push((handle, count));
    if owned {
        bar.owned_bitmaps.insert(handle);
    }
    queue_window_paint(ctx.kernel, hwnd);
    Ok(first)
}

pub(super) fn autosize(ctx: &mut CallCtx<'_>, hwnd: u32) -> Result<(), KernelError> {
    let Some(control) = ctx
        .kernel
        .controls
        .get(hwnd)
        .filter(|c| c.class == ControlClass::Toolbar)
    else {
        return Ok(());
    };
    let (parent, height) = (control.parent, control.toolbar.button_size.1.max(1) + 2);
    if ctx.kernel.gdi.rebars.contains_key(&parent) {
        command_bar::layout(ctx, parent)?;
    } else {
        let width = ctx
            .kernel
            .window_client_size(parent)
            .map_or(control.w, |s| s.0);
        let control = ctx.kernel.controls.get_mut(hwnd).unwrap();
        control.w = width;
        control.h = height;
        queue_window_paint(ctx.kernel, parent);
    }
    queue_window_paint(ctx.kernel, hwnd);
    Ok(())
}

pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    if let Some(menu) = ctx.kernel.gdi.shell_menus.bars.get(&hwnd).map(|b| b.menu) {
        let items = ctx
            .kernel
            .menu_items
            .get(&menu)
            .cloned()
            .unwrap_or_default();
        match msg {
            0x416 => {
                let Some(items) = ctx.kernel.menu_items.get_mut(&menu) else {
                    return Ok(Some(0));
                };
                if wp as usize >= items.len() {
                    return Ok(Some(0));
                }
                items.remove(wp as usize);
                ctx.kernel.framebuffer.mark_dirty();
                return Ok(Some(1));
            }
            0x414 | 0x415 | 0x443 | 0x444 => {
                let insert = matches!(msg, 0x415 | 0x443);
                let at = if matches!(wp, 0xffff | u32::MAX) {
                    items.len()
                } else {
                    wp as usize
                };
                let count = if insert { 1 } else { wp };
                if count > 4096
                    || items.len() + count as usize > 4096
                    || (insert && at > items.len())
                {
                    return Ok(Some(0));
                }
                let Ok(bytes) = ctx.cpu.read_mem(lp, count * 20) else {
                    return Ok(Some(0));
                };
                let mut added = Vec::new();
                for record in bytes.chunks_exact(20) {
                    let word = |i| u32::from_le_bytes(record[i..i + 4].try_into().unwrap());
                    let text = if word(16) > 0xffff && word(16) != u32::MAX {
                        read_wstr(ctx, word(16), 256)
                            .unwrap_or_default()
                            .into_iter()
                            .take_while(|c| *c != 0)
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let submenu =
                        if record[9] & 8 != 0 && ctx.kernel.menu_items.contains_key(&word(12)) {
                            word(12)
                        } else {
                            0
                        };
                    added.push(pocket_kernel::MenuItem {
                        id: word(4),
                        text,
                        submenu,
                        data: word(12),
                        ..Default::default()
                    });
                }
                let target = ctx.kernel.menu_items.entry(menu).or_default();
                if insert {
                    target.insert(at, added.remove(0));
                } else {
                    target.extend(added);
                }
                ctx.kernel.framebuffer.mark_dirty();
                return Ok(Some(1));
            }
            0x41e => return Ok(Some(u32::from(wp == 20))),
            0x592 => return Ok(Some(menu)),
            0x591 => {
                return Ok(Some(
                    items.iter().find(|i| i.id == lp).map_or(0, |i| i.submenu),
                ))
            }
            0x590 => {
                let Some(item) = ctx
                    .kernel
                    .menu_items
                    .get_mut(&menu)
                    .and_then(|v| v.iter_mut().find(|i| i.id == wp))
                else {
                    return Ok(Some(0));
                };
                let old = item.submenu;
                item.submenu = lp;
                return Ok(Some(old));
            }
            0x418 => return Ok(Some(items.len() as u32)),
            0x417 => {
                let Some(item) = items.get(wp as usize) else {
                    return Ok(Some(0));
                };
                let record = [u32::MAX - 1, item.id, 4 | (0x18 << 8), item.submenu, 0];
                ctx.cpu.write_mem(
                    lp,
                    &record
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )?;
                return Ok(Some(1));
            }
            _ => {}
        }
    }
    if !ctx
        .kernel
        .controls
        .get(hwnd)
        .is_some_and(|c| c.class == ControlClass::Toolbar)
        || !(0x400..=0x460).contains(&msg)
    {
        return Ok(None);
    }
    if msg == 0x421 {
        autosize(ctx, hwnd)?;
        return Ok(Some(0)); // TB_AUTOSIZE has no return value.
    }
    if msg == 0x413 {
        if lp == 0 || ctx.cpu.check_guest_access(lp, 8, Prot::READ).is_err() {
            return Ok(Some(u32::MAX));
        }
        let b = ctx.cpu.read_mem(lp, 8)?;
        let instance = u32::from_le_bytes(b[..4].try_into().unwrap());
        let resource = u32::from_le_bytes(b[4..].try_into().unwrap());
        let size = ctx.kernel.controls.get(hwnd).unwrap().toolbar.bitmap_size;
        return add_bitmap(ctx, hwnd, instance, resource, wp, size.0, size.1).map(Some);
    }
    if msg == 0x451 {
        if wp > 4096
            || (wp != 0 && (lp == 0 || ctx.cpu.check_guest_access(lp, wp * 4, Prot::READ).is_err()))
        {
            return Ok(Some(0));
        }
        let mut tips = Vec::new();
        for i in 0..wp {
            let pointer = ctx.cpu.read_u32_le(lp + i * 4)?;
            tips.push(if pointer == 0 {
                Vec::new()
            } else {
                read_wstr(ctx, pointer, 4096)?
            });
        }
        ctx.kernel.controls.get_mut(hwnd).unwrap().toolbar.tooltips = tips;
        return Ok(Some(1));
    }
    if msg == 0x425 {
        // TB_SETPARENT changes notifications, not the window hierarchy.
        let parent = ctx.kernel.controls.get(hwnd).unwrap().parent;
        return Ok(Some(
            ctx.kernel
                .window_extra
                .insert((hwnd, -1201), wp)
                .unwrap_or(parent),
        ));
    }
    let insert = matches!(msg, 0x415 | 0x443);
    let records = if matches!(msg, 0x414 | 0x415 | 0x443 | 0x444) {
        let n = if insert { 1 } else { wp };
        if n > 4096 {
            return Ok(Some(0));
        }
        let Ok(b) = ctx.cpu.read_mem(lp, n * 20) else {
            return Ok(Some(0));
        };
        b.chunks_exact(20)
            .map(|b| {
                std::array::from_fn(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()))
            })
            .collect::<Vec<[u32; 5]>>()
    } else {
        Vec::new()
    };
    let bar = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().toolbar;
    let before = (
        bar.buttons.clone(),
        bar.button_size,
        bar.bitmap_size,
        bar.bitmaps.clone(),
    );
    let id = bar.buttons.iter().position(|b| b[1] == wp);
    let out = match msg {
        0x41e => u32::from(wp == 20),
        0x414 | 0x444 => {
            if bar.buttons.len() + records.len() > 4096 {
                0
            } else {
                bar.buttons.extend(records);
                1
            }
        }
        0x415 | 0x443 => {
            if wp as usize > bar.buttons.len() || bar.buttons.len() >= 4096 {
                0
            } else {
                bar.buttons.insert(wp as usize, records[0]);
                1
            }
        }
        0x416 => {
            if (wp as usize) < bar.buttons.len() {
                bar.buttons.remove(wp as usize);
                1
            } else {
                0
            }
        }
        0x417 => {
            if let Some(b) = bar.buttons.get(wp as usize) {
                u32::from(
                    ctx.cpu
                        .write_mem(
                            lp,
                            &b.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>(),
                        )
                        .is_ok(),
                )
            } else {
                0
            }
        }
        0x418 => bar.buttons.len() as u32,
        0x419 => id.map_or(u32::MAX, |i| i as u32),
        0x412 => id.map_or(u32::MAX, |i| bar.buttons[i][2] & 255),
        0x411 => {
            if let Some(i) = id {
                bar.buttons[i][2] = (bar.buttons[i][2] & !255) | (lp & 255);
                1
            } else {
                0
            }
        }
        0x401..=0x405 => {
            let mask = match msg {
                0x401 => 4,
                0x402 => 1,
                0x403 => 2,
                0x404 => 8,
                _ => 16,
            };
            if let Some(i) = id {
                bar.buttons[i][2] =
                    (bar.buttons[i][2] & !mask) | if lp & 65535 != 0 { mask } else { 0 };
                1
            } else {
                0
            }
        }
        0x409..=0x40d => {
            let mask = match msg {
                0x409 => 4,
                0x40a => 1,
                0x40b => 2,
                0x40c => 8,
                _ => 16,
            };
            id.map_or(0, |i| u32::from(bar.buttons[i][2] & mask != 0))
        }
        0x41f | 0x420 => {
            let size = ((lp & 65535) as i32, (lp >> 16) as i32);
            if size.0 == 0 || size.1 == 0 {
                0
            } else {
                if msg == 0x41f {
                    bar.button_size = size;
                } else {
                    bar.bitmap_size = size;
                }
                1
            }
        }
        0x43a => (bar.button_size.0 as u32) | ((bar.button_size.1 as u32) << 16),
        0x41d => {
            if let Some(b) = bar.buttons.get(wp as usize) {
                let x = bar.buttons[..wp as usize]
                    .iter()
                    .filter(|b| b[2] & 8 == 0)
                    .map(|b| {
                        if b[2] & 0x100 != 0 {
                            (b[0] as i32).max(1)
                        } else {
                            bar.button_size.0
                        }
                    })
                    .sum::<i32>();
                let w = if b[2] & 0x100 != 0 {
                    (b[0] as i32).max(1)
                } else {
                    bar.button_size.0
                };
                let bytes: Vec<_> = [x, 0, x + w, bar.button_size.1]
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect();
                u32::from(ctx.cpu.write_mem(lp, &bytes).is_ok())
            } else {
                0
            }
        }
        0x41c | 0x42b => 0,
        _ => return Ok(None),
    };
    let changed = before
        != (
            bar.buttons.clone(),
            bar.button_size,
            bar.bitmap_size,
            bar.bitmaps.clone(),
        );
    if changed {
        ctx.kernel.gdi.pending_window_erases.insert(hwnd);
        queue_window_paint(ctx.kernel, hwnd);
        autosize(ctx, hwnd)?;
    }
    Ok(Some(out))
}
