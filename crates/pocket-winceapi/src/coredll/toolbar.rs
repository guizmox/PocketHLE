//! Stateful 32-bit WinCE toolbar messages for native subclasses.
use super::*;
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
    let records = if matches!(msg, 0x414 | 0x415) {
        let n = if msg == 0x415 { 1 } else { wp };
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
    let bitmap = if msg == 0x413 {
        if wp == 0 || wp > 4096 {
            return Ok(Some(u32::MAX));
        }
        let Ok(b) = ctx.cpu.read_mem(lp, 8) else {
            return Ok(Some(u32::MAX));
        };
        let m = u32::from_le_bytes(b[..4].try_into().unwrap());
        let h = u32::from_le_bytes(b[4..].try_into().unwrap());
        Some(if m == 0 {
            h
        } else {
            load_bitmap_resource(ctx, m, &ResourceKey::Id(h), true)?
        })
    } else {
        None
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
        0x414 => {
            if bar.buttons.len() + records.len() > 4096 {
                0
            } else {
                bar.buttons.extend(records);
                1
            }
        }
        0x415 => {
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
        0x413 => {
            let start: u32 = bar.bitmaps.iter().map(|b| b.1).sum();
            if start > 4096 - wp {
                u32::MAX
            } else {
                bar.bitmaps.push((bitmap.unwrap(), wp));
                start
            }
        }
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
        0x41c | 0x421 | 0x42b => 0,
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
    }
    Ok(Some(out))
}
