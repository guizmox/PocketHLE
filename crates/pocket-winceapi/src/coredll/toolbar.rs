//! Stateful 32-bit WinCE toolbar messages for native subclasses.
use super::*;
pub(crate) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    let shell_menu = ctx.kernel.gdi.shell_menus.bars.get(&hwnd).map(|b| b.menu);
    if let Some(menu) = shell_menu {
        match msg {
            0x592 => {
                return Ok(Some(crate::aygshell::menu_bar::source_menu(
                    ctx.kernel, hwnd, menu,
                )))
            }
            0x591 => {
                return Ok(Some(
                    ctx.kernel
                        .menu_items
                        .get(&menu)
                        .and_then(|items| items.iter().find(|i| i.id == lp))
                        .map_or(0, |i| i.submenu),
                ))
            }
            0x41d => {
                let Some(rect) = ctx.kernel.gdi.shell_menus.item_rect(
                    hwnd,
                    wp as usize,
                    ctx.kernel.framebuffer.width as i32,
                    &ctx.kernel.menu_items,
                    &ctx.kernel.gdi.shell_toolbars,
                ) else {
                    return Ok(Some(0));
                };
                let bytes = rect
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>();
                return Ok(Some(u32::from(ctx.cpu.write_mem(lp, &bytes).is_ok())));
            }
            0x590 => {
                let Some(item) = ctx
                    .kernel
                    .menu_items
                    .get_mut(&menu)
                    .and_then(|items| items.iter_mut().find(|i| i.id == wp))
                else {
                    return Ok(Some(0));
                };
                let old = item.submenu;
                item.submenu = lp;
                item.data = lp;
                if let Some(toolbar) = ctx.kernel.gdi.shell_toolbars.get_mut(&hwnd) {
                    if let Some(record) = toolbar.state.buttons.iter_mut().find(|b| b[1] == wp) {
                        record[3] = lp;
                    }
                }
                ctx.kernel.framebuffer.mark_dirty();
                return Ok(Some(old));
            }
            _ => {}
        }
    }
    if shell_menu.is_none()
        && !ctx
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
        let parent = ctx
            .kernel
            .controls
            .get(hwnd)
            .map(|c| c.parent)
            .or_else(|| ctx.kernel.gdi.shell_menus.bars.get(&hwnd).map(|b| b.owner))
            .unwrap_or(0);
        return Ok(Some(
            ctx.kernel
                .window_extra
                .insert((hwnd, -1201), wp)
                .unwrap_or(parent),
        ));
    }
    let mut records = if matches!(msg, 0x414 | 0x415 | 0x443 | 0x444) {
        let n = if matches!(msg, 0x415 | 0x443) { 1 } else { wp };
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
    if msg == 0x413 {
        if wp == 0 || wp > 4096 {
            return Ok(Some(u32::MAX));
        }
        let Ok(b) = ctx.cpu.read_mem(lp, 8) else {
            return Ok(Some(u32::MAX));
        };
        let module = u32::from_le_bytes(b[..4].try_into().unwrap());
        let resource = u32::from_le_bytes(b[4..].try_into().unwrap());
        return Ok(Some(add_bitmap(ctx, hwnd, module, resource, wp)?));
    }
    let mut labels = if records.is_empty() {
        Vec::new()
    } else {
        let mut labels = Vec::with_capacity(records.len());
        for record in &records {
            let pointer = record[4];
            let label = if pointer > 0xffff && pointer != u32::MAX {
                read_wstr(ctx, pointer, 4096)?
            } else {
                Vec::new()
            };
            labels.push(label);
        }
        labels
    };
    let mut strings = shell_menu.map(|menu| {
        ctx.kernel
            .gdi
            .shell_toolbars
            .get(&hwnd)
            .map(|t| t.strings.clone())
            .unwrap_or_else(|| {
                ctx.kernel
                    .menu_items
                    .get(&menu)
                    .into_iter()
                    .flatten()
                    .map(|i| i.text.clone())
                    .collect()
            })
    });
    if let Some(pool) = strings.as_mut() {
        for (record, label) in records.iter_mut().zip(&mut labels) {
            if record[4] > 0xffff && record[4] != u32::MAX {
                if pool.len() >= 4096 {
                    return Ok(Some(0));
                }
                record[4] = pool.len() as u32;
                pool.push(label.clone());
            } else if record[4] != u32::MAX {
                *label = pool.get(record[4] as usize).cloned().unwrap_or_default();
            }
        }
    }
    let mut bar = state(ctx.kernel, hwnd).unwrap();
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
                bar.buttons.extend_from_slice(&records);
                1
            }
        }
        0x415 | 0x443 => {
            let at = if matches!(wp, 0xffff | u32::MAX) {
                bar.buttons.len()
            } else {
                wp as usize
            };
            if at > bar.buttons.len() || bar.buttons.len() >= 4096 {
                0
            } else {
                bar.buttons.insert(at, records[0]);
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
            if size.0 == 0 || size.1 == 0 || size.0 > 4096 || size.1 > 4096 {
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
                            (b[0] as i32).clamp(1, 4096)
                        } else {
                            bar.button_size.0
                        }
                    })
                    .sum::<i32>();
                let w = if b[2] & 0x100 != 0 {
                    (b[0] as i32).clamp(1, 4096)
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
    if out != 0 && matches!(msg, 0x414 | 0x415 | 0x443 | 0x444 | 0x416) {
        if let Some(menu) = shell_menu {
            let items = ctx.kernel.menu_items.entry(menu).or_default();
            match msg {
                0x416 => {
                    items.remove(wp as usize);
                }
                0x415 | 0x443 => {
                    let at = if matches!(wp, 0xffff | u32::MAX) {
                        items.len()
                    } else {
                        wp as usize
                    };
                    let item = projection_from_record(records[0], labels.remove(0));
                    items.insert(at, item);
                }
                _ => {
                    let added = records
                        .iter()
                        .zip(labels)
                        .map(|(record, text)| projection_from_record(*record, text))
                        .collect::<Vec<_>>();
                    ctx.kernel.menu_items.entry(menu).or_default().extend(added);
                }
            }
        }
    }
    if changed {
        if let Some(menu) = shell_menu {
            // State changes affect both menu hit testing and raw TB_GETBUTTON.
            for (item, record) in ctx
                .kernel
                .menu_items
                .entry(menu)
                .or_default()
                .iter_mut()
                .zip(&bar.buttons)
            {
                item.kind = if record[2] & 0x100 != 0 { 0x800 } else { 0 };
                let flags = ctx
                    .kernel
                    .menus
                    .entry(menu)
                    .or_default()
                    .entry(item.id)
                    .or_default();
                *flags = (*flags & !11)
                    | if record[2] & 4 == 0 || record[2] & 8 != 0 {
                        3
                    } else {
                        0
                    }
                    | if record[2] & 1 != 0 { 8 } else { 0 };
            }
            let toolbar = ctx.kernel.gdi.shell_toolbars.entry(hwnd).or_default();
            toolbar.state = bar;
            toolbar.strings = strings.unwrap();
            ctx.kernel.framebuffer.mark_dirty();
        } else {
            ctx.kernel.controls.get_mut(hwnd).unwrap().toolbar = bar;
            ctx.kernel.gdi.pending_window_erases.insert(hwnd);
            queue_window_paint(ctx.kernel, hwnd);
        }
    }
    Ok(Some(out))
}

fn projection_from_record(record: [u32; 5], text: Vec<u16>) -> pocket_kernel::MenuItem {
    pocket_kernel::MenuItem {
        id: record[1],
        text: if record[0] == u32::MAX - 1 || record[2] & 0x4000 != 0 {
            text
        } else {
            Vec::new()
        },
        submenu: if record[2] & 0x800 != 0 { record[3] } else { 0 },
        data: record[3],
        kind: if record[2] & 0x100 != 0 { 0x800 } else { 0 },
        ..Default::default()
    }
}
fn state(kernel: &KernelState, hwnd: u32) -> Option<pocket_kernel::controls::ToolbarState> {
    if let Some(bar) = kernel.gdi.shell_menus.bars.get(&hwnd) {
        return Some(
            kernel
                .gdi
                .shell_toolbars
                .get(&hwnd)
                .map(|t| t.state.clone())
                .unwrap_or_else(|| {
                    let mut state = pocket_kernel::controls::ToolbarState::default();
                    state.bitmap_size = (16, 16);
                    state.buttons = kernel
                        .menu_items
                        .get(&bar.menu)
                        .into_iter()
                        .flatten()
                        .enumerate()
                        .map(|(index, item)| {
                            [
                                u32::MAX - 1,
                                item.id,
                                4 | (0x18 << 8),
                                item.submenu,
                                index as u32,
                            ]
                        })
                        .collect();
                    state
                }),
        );
    }
    kernel
        .controls
        .get(hwnd)
        .filter(|c| c.class == ControlClass::Toolbar)
        .map(|c| c.toolbar.clone())
}
/// One service for TB_ADDBITMAP and CommandBar_AddBitmap. Resource strips are
/// privately owned, caller-provided HBITMAPs are borrowed until window teardown.
pub(crate) fn add_bitmap(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    module: u32,
    resource: u32,
    count: u32,
) -> Result<u32, KernelError> {
    let Some(mut bar) = state(ctx.kernel, hwnd) else {
        return Ok(u32::MAX);
    };
    let Some(start) = bar.bitmaps.iter().try_fold(0u32, |n, b| n.checked_add(b.1)) else {
        return Ok(u32::MAX);
    };
    if count == 0 || count > 4096 || start > 4096 - count || bar.bitmaps.len() >= 4096 {
        return Ok(u32::MAX);
    }
    let bitmap = if module == 0 {
        resource
    } else {
        load_bitmap_resource(ctx, module, &ResourceKey::Id(resource), false)?
    };
    if bitmap == 0 || ctx.kernel.gdi.bitmap(bitmap).is_none() {
        return Ok(u32::MAX);
    }
    bar.bitmaps.push((bitmap, count));
    if ctx.kernel.gdi.shell_menus.bars.contains_key(&hwnd) {
        let initial_strings = ctx
            .kernel
            .gdi
            .shell_menus
            .bars
            .get(&hwnd)
            .and_then(|b| ctx.kernel.menu_items.get(&b.menu))
            .into_iter()
            .flatten()
            .map(|i| i.text.clone())
            .collect::<Vec<_>>();
        let owner = ctx
            .kernel
            .gdi
            .shell_toolbars
            .entry(hwnd)
            .or_insert_with(|| pocket_kernel::shell_menu::Toolbar {
                strings: initial_strings,
                ..Default::default()
            });
        owner.state = bar;
        if module != 0 {
            owner.owned_bitmaps.push(bitmap);
        }
        ctx.kernel.framebuffer.mark_dirty();
    } else {
        if module != 0 {
            ctx.kernel
                .window_extra
                .insert((hwnd, -65536 - bar.bitmaps.len() as i32), bitmap);
        }
        ctx.kernel.controls.get_mut(hwnd).unwrap().toolbar = bar;
        queue_window_paint(ctx.kernel, hwnd);
    }
    Ok(start)
}
pub(super) fn discard(ctx: &mut CallCtx<'_>, hwnd: u32) {
    if let Some(toolbar) = ctx.kernel.gdi.shell_toolbars.remove(&hwnd) {
        for bitmap in toolbar.owned_bitmaps {
            ctx.kernel.gdi.delete(bitmap);
        }
    }
    let owned = ctx
        .kernel
        .window_extra
        .iter()
        .filter_map(|(&(h, key), &bitmap)| {
            (h == hwnd && (-69632..-65536).contains(&key)).then_some(bitmap)
        })
        .collect::<Vec<_>>();
    for bitmap in owned {
        ctx.kernel.gdi.delete(bitmap);
    }
}

#[cfg(test)]
mod tests {
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    use pocket_kernel::{controls::ControlClass, DispatchOutcome};
    use pocket_pe::{ResourceEntry, ResourceKey};
    fn value(out: DispatchOutcome) -> u32 {
        let DispatchOutcome::ReturnedR0(v) = out else {
            panic!("unexpected callback")
        };
        v
    }
    fn resource(cpu: &mut dyn Cpu, k: &mut pocket_kernel::KernelState) {
        let mut dib = vec![0u8; 40 + 32 * 16 * 3];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&32i32.to_le_bytes());
        dib[8..12].copy_from_slice(&16i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        for y in 0..16 {
            for x in 0..32 {
                let at = 40 + (y * 32 + x) * 3;
                dib[at..at + 3].copy_from_slice(if x < 16 { &[0, 0, 255] } else { &[0, 255, 0] });
            }
        }
        k.image_base = 0x1000;
        k.resources.push(ResourceEntry {
            ty: ResourceKey::Id(2),
            name: ResourceKey::Id(99),
            native_leaves: Vec::new(),
            data_rva: 0x2000,
            size: dib.len() as u32,
            codepage: 0,
        });
        cpu.write_mem(0x3000, &dib).unwrap();
    }
    #[test]
    fn command_bitmap_returns_real_indices_rejects_missing_resources_and_releases_only_owned_strips(
    ) {
        let (mut cpu, mut k, mut d) = setup();
        resource(&mut cpu, &mut k);
        let hwnd = k.controls.create(
            7,
            ControlClass::Toolbar,
            10,
            String::new(),
            0x50000000,
            0,
            0,
            80,
            24,
        );
        let borrowed = k.gdi.create_compatible_bitmap(16, 16);
        assert_eq!(
            value(call(
                &mut cpu,
                &mut k,
                &mut d,
                "commctrl.dll",
                "ord:5",
                &[hwnd, 0x10000000, 99, 2, 0, 0]
            )),
            0
        );
        let owned = k.controls.get(hwnd).unwrap().toolbar.bitmaps[0].0;
        assert_ne!(owned, 0);
        assert_ne!(owned, borrowed);
        assert_eq!(
            value(call(
                &mut cpu,
                &mut k,
                &mut d,
                "commctrl.dll",
                "ord:5",
                &[hwnd, 0, borrowed, 1, 0, 0]
            )),
            2
        );
        assert_eq!(
            value(call(
                &mut cpu,
                &mut k,
                &mut d,
                "commctrl.dll",
                "ord:5",
                &[hwnd, 0x10000000, 100, 1, 0, 0]
            )),
            u32::MAX
        );
        assert_eq!(k.controls.get(hwnd).unwrap().toolbar.bitmaps.len(), 2);
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("DestroyWindow".into()),
            friendly_name: None,
        };
        super::discard(
            &mut crate::CallCtx {
                cpu: &mut cpu,
                kernel: &mut k,
                thunk: &thunk,
            },
            hwnd,
        );
        assert!(k.gdi.bitmap(owned).is_none());
        assert!(k.gdi.bitmap(borrowed).is_some());
    }
    #[test]
    fn shell_toolbar_keeps_button_records_images_labels_and_disabled_hit_targets() {
        let (mut cpu, mut k, mut d) = setup();
        resource(&mut cpu, &mut k);
        k.window_classes.insert(7, "Frame".into());
        let info = [32u32, 7, 1, 0, 0, 0, 0, 0];
        cpu.write_mem(
            0x2000,
            &info
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            value(call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "SHCreateMenuBar",
                &[0x2000]
            )),
            1
        );
        let hwnd = cpu.read_u32_le(0x201c).unwrap();
        assert_eq!(
            value(call(
                &mut cpu,
                &mut k,
                &mut d,
                "commctrl.dll",
                "ord:5",
                &[hwnd, 0x10000000, 99, 2, 0, 0]
            )),
            0
        );
        // A zero high word denotes a string-pool index in the toolbar ABI.
        cpu.map_region(
            0x10000,
            0x1000,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )
        .unwrap();
        cpu.write_mem(0x10000, &[71, 0, 111, 0, 0, 0]).unwrap();
        let record = [1u32, 81, 4, 0x1234, 0x10000];
        cpu.write_mem(
            0x2200,
            &record
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            value(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendMessageW",
                &[hwnd, 0x414, 1, 0x2200]
            )),
            1
        );
        assert_eq!(
            value(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendMessageW",
                &[hwnd, 0x417, 0, 0x2400]
            )),
            1
        );
        assert_eq!(
            cpu.read_mem(0x2400, 16).unwrap(),
            cpu.read_mem(0x2200, 16).unwrap()
        );
        assert_eq!(cpu.read_u32_le(0x2410).unwrap(), 0);
        cpu.write_mem(0x10000, &[0; 6]).unwrap();
        assert_eq!(k.gdi.shell_toolbars[&hwnd].strings, vec![vec![71, 111]]);
        let menu = k.gdi.shell_menus.bars[&hwnd].menu;
        assert_eq!(k.menu_items[&menu][0].data, 0x1234);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SendMessageW",
            &[hwnd, 0x401, 81, 0],
        );
        assert_eq!(k.menus[&menu][&81] & 3, 3);
        let mut frame = pocket_kernel::Framebuffer::new(240, 320);
        k.gdi.shell_menus.render(
            hwnd,
            &mut pocket_kernel::gdi::Surface::Screen(&mut frame),
            &k.gdi,
            &k.menu_items,
            &k.menus,
        );
        assert_eq!(
            u16::from_le_bytes(
                frame.pixels[(299 * 240 + 4) * 2..(299 * 240 + 4) * 2 + 2]
                    .try_into()
                    .unwrap()
            ),
            0x07e0
        );
    }
}
