//! Common-control custom drawing. The parent may reflect WM_NOTIFY back to
//! a native subclass; that subclass owns its backgrounds, not the emulator.
use super::*;
use pocket_kernel::gdi::ListPaintFrame;

fn restore(ctx: &mut CallCtx<'_>, f: &ListPaintFrame) -> Result<DispatchOutcome, KernelError> {
    // The report header is painted independently of NM_CUSTOMDRAW for items.
    // Draw it on the same DC before the guest blits or the DIB is synchronized.
    if let Some(c) = local_control(ctx, f.hwnd).filter(|c| c.list_header_height() > 0) {
        let font = pocket_kernel::text::selected(&ctx.kernel.gdi, c.font);
        if let Some(mut surface) = surface_for_dc(ctx.kernel, f.dc) {
            c.render_list_header(&mut surface, font.as_ref());
        }
    }
    sync_dst_dib_to_guest(ctx, f.dc)?;
    // WM_PAINT may target a temporary double buffer before the control has
    // ever acquired a window DC. Retain the completed custom drawing for
    // presentation too; otherwise the HLE item renderer covers it, and the
    // guest destroys the only bitmap containing its native backgrounds.
    if f.frame.args[2] != 0 {
        let painted = ctx
            .kernel
            .gdi
            .dc(f.dc)
            .and_then(|dc| dc.selected_bitmap)
            .and_then(|bitmap| ctx.kernel.gdi.bitmap(bitmap))
            .cloned();
        if let Some(painted) = painted {
            let retained = window_dc(ctx, f.hwnd);
            if retained != f.dc {
                if let Some(mut surface) = surface_for_dc(ctx.kernel, retained) {
                    surface.blit_from_bytes(
                        0,
                        0,
                        0,
                        0,
                        painted.width as i32,
                        painted.height as i32,
                        &painted.pixels,
                        painted.width,
                        painted.height,
                    );
                }
            }
        }
    }
    do_free(ctx, f.scratch);
    for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(f.frame.args)
    {
        ctx.cpu.write_reg(reg, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, f.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, f.frame.lr)?;
    ctx.kernel.framebuffer.mark_dirty();
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn local_control(ctx: &CallCtx<'_>, hwnd: u32) -> Option<pocket_kernel::controls::ChildWindow> {
    let mut c = ctx.kernel.controls.get(hwnd)?.clone();
    c.x = 0;
    c.y = 0;
    Some(c)
}

fn default_item(ctx: &mut CallCtx<'_>, f: &ListPaintFrame) -> Result<(), KernelError> {
    let Some(mut c) = local_control(ctx, f.hwnd) else {
        return Ok(());
    };
    c.list_text_color = ctx.cpu.read_u32_le(f.scratch + 48)?;
    let background = ctx.cpu.read_u32_le(f.scratch + 52)?;
    if background != u32::MAX {
        c.list_background = background;
    }
    if f.item_flags & 2 != 0 {
        if let Some(dc) = ctx.kernel.gdi.dc(f.dc) {
            c.font = dc.selected_font;
        }
    }
    let Some(handle) = ctx.kernel.gdi.dc(f.dc).and_then(|d| d.selected_bitmap) else {
        return Ok(());
    };
    let Some(mut bitmap) = ctx.kernel.gdi.bitmap(handle).cloned() else {
        return Ok(());
    };
    c.render_custom_draw_item(&mut Surface::Bitmap(&mut bitmap), &ctx.kernel.gdi, f.item);
    if let Some(dst) = ctx.kernel.gdi.bitmap_mut(handle) {
        *dst = bitmap;
    }
    Ok(())
}

fn notify(
    ctx: &mut CallCtx<'_>,
    f: ListPaintFrame,
    stage: u32,
) -> Result<DispatchOutcome, KernelError> {
    let Some(c) = local_control(ctx, f.hwnd) else {
        return restore(ctx, &f);
    };
    let proc = ctx.kernel.window_procs.get(&c.parent).copied().unwrap_or(0);
    if proc == 0 {
        return restore(ctx, &f);
    }
    let rect = if stage & 0x10000 != 0 {
        let (x, y) = c.list_item_position(f.item);
        let height = if c.list_icon_view() {
            c.list_spacing.1
        } else {
            c.list_row_height()
        };
        [
            x,
            y.saturating_sub(c.list_scroll_y),
            x.saturating_add(c.list_spacing.0),
            y.saturating_sub(c.list_scroll_y).saturating_add(height),
        ]
    } else {
        [0, 0, c.w, c.h]
    };
    // NMLVCUSTOMDRAW: NMHDR, NMCUSTOMDRAW, clrText, clrTextBk, iSubItem.
    let fields = [
        c.hwnd,
        c.id,
        (-12i32) as u32,
        stage,
        f.dc,
        rect[0] as u32,
        rect[1] as u32,
        rect[2] as u32,
        rect[3] as u32,
        f.item as u32,
        u32::from(c.list_selected == Some(f.item)),
        c.list_items.get(f.item).map_or(0, |i| i.1),
        c.list_text_color,
        c.list_background,
        0,
    ];
    ctx.cpu.write_mem(
        f.scratch,
        &fields
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    ctx.cpu.write_reg(ArmReg::R0, c.parent)?;
    ctx.cpu.write_reg(ArmReg::R1, 0x4e)?;
    ctx.cpu.write_reg(ArmReg::R2, c.id)?;
    ctx.cpu.write_reg(ArmReg::R3, f.scratch)?;
    ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
    ctx.kernel.gdi.list_paint_frames.push(f);
    Ok(DispatchOutcome::JumpTo(proc & !1))
}

fn next_display_info(
    ctx: &mut CallCtx<'_>,
    mut f: ListPaintFrame,
) -> Result<DispatchOutcome, KernelError> {
    loop {
        let Some(c) = local_control(ctx, f.hwnd) else {
            return restore(ctx, &f);
        };
        if f.item >= c.list_items.len() {
            f.item = 0;
            f.display_column = 0;
            f.stage = 0;
            return notify(ctx, f, 1);
        }
        let mut columns = vec![0];
        if c.style & 3 == 1 {
            columns.extend(
                c.list_report_columns
                    .iter()
                    .map(|column| column.subitem)
                    .filter(|&subitem| subitem != 0),
            );
        }
        let Some(&subitem) = columns.get(f.display_column) else {
            f.item += 1;
            f.display_column = 0;
            continue;
        };
        let text = c.list_text_callbacks.get(f.item).copied().unwrap_or(false)
            && !c
                .list_cached_text
                .get(f.item)
                .is_some_and(|columns| columns.contains(&subitem));
        let image = subitem == 0 && c.list_image_callbacks.get(f.item).copied().unwrap_or(false);
        let mask = u32::from(text) | if image { 2 | 8 } else { 0 };
        if mask == 0 {
            f.display_column += 1;
            continue;
        }
        let proc = ctx.kernel.window_procs.get(&c.parent).copied().unwrap_or(0);
        if proc == 0 {
            return restore(ctx, &f);
        }
        // Portable Explorer inserts LPSTR_TEXTCALLBACK / I_IMAGECALLBACK.
        // NMLVDISPINFOW is NMHDR followed by the CE LVITEM layout, with
        // lParam retaining the guest's PIDL and writable text scratch.
        let fields: [u32; 12] = [
            c.hwnd,
            c.id,
            (-177i32) as u32,
            mask,
            f.item as u32,
            subitem,
            c.list_item_state(f.item),
            0xff00,
            f.scratch + 60,
            1024,
            c.list_images.get(f.item).copied().unwrap_or(-1) as u32,
            c.list_items[f.item].1,
        ];
        ctx.cpu.write_mem(
            f.scratch,
            &fields
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )?;
        ctx.cpu.write_mem(f.scratch + 60, &[0; 2048])?;
        ctx.cpu.write_reg(ArmReg::R0, c.parent)?;
        ctx.cpu.write_reg(ArmReg::R1, 0x4e)?;
        ctx.cpu.write_reg(ArmReg::R2, c.id)?;
        ctx.cpu.write_reg(ArmReg::R3, f.scratch)?;
        ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
        f.stage = 4;
        f.item_flags = mask;
        ctx.kernel.gdi.list_paint_frames.push(f);
        return Ok(DispatchOutcome::JumpTo(proc & !1));
    }
}

fn next_item(ctx: &mut CallCtx<'_>, mut f: ListPaintFrame) -> Result<DispatchOutcome, KernelError> {
    let count = ctx
        .kernel
        .controls
        .get(f.hwnd)
        .map_or(0, |c| c.list_items.len());
    if f.item < count {
        f.stage = 1;
        return notify(ctx, f, 0x10001);
    }
    if f.flags & 0x10 != 0 {
        f.stage = 3;
        return notify(ctx, f, 2);
    }
    restore(ctx, &f)
}

pub(super) fn resume(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    let Some(index) = callback_frame_index(
        &ctx.kernel.gdi.list_paint_frames,
        ctx.kernel.current_thread,
        ctx.thunk.thunk_va,
        sp,
        |f| (f.thread, f.thunk, f.frame.sp),
    ) else {
        return Ok(None);
    };
    let mut f = ctx.kernel.gdi.list_paint_frames.remove(index);
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    let outcome = match f.stage {
        4 => {
            let returned_mask = ctx.cpu.read_u32_le(f.scratch + 12)?;
            let fields = f.item_flags & returned_mask;
            let text = if fields & 1 != 0 {
                let pointer = ctx.cpu.read_u32_le(f.scratch + 32)?;
                Some(if pointer == 0 || pointer == u32::MAX {
                    String::new()
                } else {
                    String::from_utf16_lossy(&read_wstr(ctx, pointer, 1024)?)
                })
            } else {
                None
            };
            let subitem = ctx.cpu.read_u32_le(f.scratch + 20)?;
            let image = ctx.cpu.read_u32_le(f.scratch + 40)? as i32;
            let state = ctx.cpu.read_u32_le(f.scratch + 24)?;
            let state_mask = ctx.cpu.read_u32_le(f.scratch + 28)? & 0xff00;
            if let Some(c) = ctx
                .kernel
                .controls
                .get_mut(f.hwnd)
                .filter(|c| f.item < c.list_items.len())
            {
                if let Some(text) = text {
                    c.set_list_text(f.item, subitem, text);
                    if returned_mask & 0x1000 != 0 {
                        c.list_cached_text
                            .resize_with(c.list_items.len(), Default::default);
                        c.list_cached_text[f.item].insert(subitem);
                    }
                }
                if fields & 2 != 0 {
                    c.list_images.resize(c.list_items.len(), -1);
                    c.list_images[f.item] = image;
                    // Original CE6 Explorer sets LVIF_DI_SETITEM after resolving
                    // an icon. Repeating those native metadata callbacks during
                    // selection repaint delayed the second tap past 500 ms.
                    if returned_mask & 0x1000 != 0 {
                        c.list_image_callbacks.resize(c.list_items.len(), false);
                        c.list_image_callbacks[f.item] = false;
                    }
                }
                if fields & 8 != 0 {
                    c.set_list_item_state(f.item, state, state_mask);
                }
            }
            f.display_column += 1;
            next_display_info(ctx, f)?
        }
        0 => {
            f.flags = result;
            if result & 4 != 0 {
                restore(ctx, &f)?
            } else if result & 0x20 != 0 {
                next_item(ctx, f)?
            } else {
                let count = ctx
                    .kernel
                    .controls
                    .get(f.hwnd)
                    .map_or(0, |c| c.list_items.len());
                f.item_flags = result;
                for i in 0..count {
                    f.item = i;
                    default_item(ctx, &f)?;
                }
                f.item = count;
                next_item(ctx, f)?
            }
        }
        1 => {
            f.item_flags = result;
            if result & 4 == 0 {
                default_item(ctx, &f)?;
            }
            if result & 0x10 != 0 {
                f.stage = 2;
                notify(ctx, f, 0x10002)?
            } else {
                f.item += 1;
                next_item(ctx, f)?
            }
        }
        2 => {
            f.item += 1;
            next_item(ctx, f)?
        }
        _ => restore(ctx, &f)?,
    };
    Ok(Some(outcome))
}

pub(super) fn paint(ctx: &mut CallCtx<'_>, hwnd: u32) -> Result<DispatchOutcome, KernelError> {
    let Some(c) = local_control(ctx, hwnd) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let frame = GuestCallFrame {
        args: [
            ctx.arg_u32(0)?,
            ctx.arg_u32(1)?,
            ctx.arg_u32(2)?,
            ctx.arg_u32(3)?,
        ],
        sp: ctx.cpu.read_reg(ArmReg::Sp)?,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    let DispatchOutcome::ReturnedR0(scratch) = do_alloc(ctx, 60 + 2048)? else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    if scratch == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    // Built-in controls keep geometry in Controls, not window_rects.
    let position = ctx
        .kernel
        .controls
        .get(hwnd)
        .map(|c| (c.x, c.y, c.w, c.h))
        .unwrap();
    ctx.kernel.window_rects.insert(hwnd, position);
    // Native subclasses paint through a memory DC and copy it to their
    // window afterwards. Common controls honour that DC in WM_PAINT.wParam.
    // Painting into the window instead leaves their memory bitmap black.
    let requested_dc = frame.args[2];
    let dc = if requested_dc != 0 && ctx.kernel.gdi.dc(requested_dc).is_some() {
        requested_dc
    } else {
        window_dc(ctx, hwnd)
    };
    if let Some(meta) = ctx.kernel.gdi.dc_mut(dc) {
        meta.selected_font = c.font;
    }
    if let Some(mut surface) = surface_for_dc(ctx.kernel, dc) {
        surface.fill_rect(0, 0, c.w, c.h, colorref_to_rgb565(c.list_background));
    }
    let f = ListPaintFrame {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame,
        hwnd,
        dc,
        scratch,
        stage: 0,
        item: 0,
        flags: 0,
        item_flags: 0,
        display_column: 0,
    };
    next_display_info(ctx, f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::{stub::StubCpu, Cpu};

    #[test]
    fn shell_display_info_caches_only_requested_fields_and_requeries_after_callback_reset() {
        let mut cpu = super::super::tests::scanf_cpu();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let mut kernel = super::super::tests::fresh_kernel();
        let hwnd = kernel.controls.create(
            FAKE_HWND,
            ControlClass::ListView,
            3,
            String::new(),
            0x50000001,
            0,
            0,
            320,
            200,
        );
        let c = kernel.controls.get_mut(hwnd).unwrap();
        c.list_items.push((String::new(), 123));
        c.list_text_callbacks.push(true);
        c.list_image_callbacks.push(true);
        c.list_report_columns
            .push(pocket_kernel::controls::ListColumn {
                subitem: 1,
                title: "Details".into(),
                width: 80,
                format: 0,
            });
        kernel.window_procs.insert(FAKE_HWND, 0x6000);
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let start = |ctx: &mut CallCtx<'_>| {
            for (reg, value) in [
                (ArmReg::R0, hwnd),
                (ArmReg::R1, WM_PAINT),
                (ArmReg::R2, 0),
                (ArmReg::R3, 0),
            ] {
                ctx.cpu.write_reg(reg, value).unwrap();
            }
            assert_eq!(paint(ctx, hwnd).unwrap(), DispatchOutcome::JumpTo(0x6000));
            ctx.cpu.read_reg(ArmReg::R3).unwrap()
        };
        let ptr = start(&mut ctx);
        assert_eq!(ctx.cpu.read_u32_le(ptr + 8).unwrap(), (-177i32) as u32);
        assert_eq!(ctx.cpu.read_u32_le(ptr + 12).unwrap(), 11);
        ctx.cpu
            .write_mem(ptr + 12, &0x100bu32.to_le_bytes())
            .unwrap();
        ctx.cpu.write_mem(ptr + 40, &7u32.to_le_bytes()).unwrap();
        ctx.cpu
            .write_mem(
                ptr + 60,
                &"Explorer\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::JumpTo(0x6000))
        );
        assert_eq!(ctx.cpu.read_u32_le(ptr + 20).unwrap(), 1); // uncached report subitem
        assert_eq!(ctx.cpu.read_u32_le(ptr + 12).unwrap(), 1); // image is already cached
        ctx.cpu
            .write_mem(
                ptr + 60,
                &"Details\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::JumpTo(0x6000))
        );
        assert_eq!(ctx.cpu.read_u32_le(ptr + 8).unwrap(), (-12i32) as u32);
        ctx.cpu.write_reg(ArmReg::R0, 4).unwrap();
        assert_eq!(
            resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::ReturnedR0(0))
        );
        let c = ctx.kernel.controls.get(hwnd).unwrap();
        assert_eq!(c.list_items[0], ("Explorer".into(), 123));
        assert_eq!(c.list_images[0], 7);
        assert!(!c.list_image_callbacks[0]);
        assert!(c.list_cached_text[0].contains(&0));
        assert!(!c.list_cached_text[0].contains(&1));
        let ptr = start(&mut ctx);
        assert_eq!(ctx.cpu.read_u32_le(ptr + 20).unwrap(), 1);
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        resume(&mut ctx).unwrap();
        ctx.cpu.write_reg(ArmReg::R0, 4).unwrap();
        resume(&mut ctx).unwrap();
        let fields = [3, 0, 0, 0, 0, u32::MAX, 0, u32::MAX, 123];
        ctx.cpu
            .write_mem(
                0x1100,
                &fields
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert_eq!(
            list_view_message(&mut ctx, hwnd, 0x104c, 0, 0x1100).unwrap(),
            1
        );
        let ptr = start(&mut ctx);
        assert_eq!(ctx.cpu.read_u32_le(ptr + 20).unwrap(), 0);
        assert_eq!(ctx.cpu.read_u32_le(ptr + 12).unwrap(), 11);
    }

    #[test]
    fn navigator_skin_custom_background_survives_default_item_and_restores_caller() {
        let mut cpu = StubCpu::new();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        let mut kernel = super::super::tests::fresh_kernel();
        let hwnd = kernel.controls.create(
            FAKE_HWND,
            ControlClass::ListView,
            1558,
            String::new(),
            0x50000000,
            7,
            9,
            75,
            75,
        );
        let images = kernel.gdi.create_image_list(32, 32);
        let c = kernel.controls.get_mut(hwnd).unwrap();
        c.list_items.push(("A".into(), 123));
        c.list_image_lists[0] = images;
        c.list_image_sizes[0] = (32, 32);
        kernel.window_procs.insert(FAKE_HWND, 0x6000);
        let thunk = super::super::tests::dummy_thunk();
        for (r, v) in [
            (ArmReg::R0, hwnd),
            (ArmReg::R1, WM_PAINT),
            (ArmReg::R2, 0),
            (ArmReg::R3, 0),
            (ArmReg::Sp, 0x1800),
            (ArmReg::Lr, 0x8000),
        ] {
            cpu.write_reg(r, v).unwrap();
        }
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        ctx.cpu.write_mem(0x1100, &0u32.to_le_bytes()).unwrap();
        assert_eq!(
            list_view_message(&mut ctx, hwnd, 0x100e, 0, 0x1100).unwrap(),
            1
        );
        assert_eq!(ctx.cpu.read_u32_le(0x1108).unwrap(), 75);
        assert_eq!(
            paint(&mut ctx, hwnd).unwrap(),
            DispatchOutcome::JumpTo(0x6000)
        );
        let ptr = ctx.cpu.read_reg(ArmReg::R3).unwrap();
        assert_eq!(ctx.cpu.read_u32_le(ptr + 8).unwrap(), (-12i32) as u32);
        assert_eq!(ctx.cpu.read_u32_le(ptr + 12).unwrap(), 1);
        ctx.cpu.write_reg(ArmReg::R0, 0x20).unwrap();
        assert_eq!(
            resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::JumpTo(0x6000))
        );
        assert_eq!(ctx.cpu.read_u32_le(ptr + 12).unwrap(), 0x10001);
        let dc = ctx.cpu.read_u32_le(ptr + 16).unwrap();
        surface_for_dc(ctx.kernel, dc)
            .unwrap()
            .fill_rect(0, 0, 5, 5, 0x1234);
        ctx.cpu
            .write_mem(ptr + 52, &u32::MAX.to_le_bytes())
            .unwrap();
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::ReturnedR0(0))
        );
        let bitmap = ctx
            .kernel
            .gdi
            .dc(dc)
            .and_then(|d| d.selected_bitmap)
            .and_then(|b| ctx.kernel.gdi.bitmap(b))
            .unwrap();
        assert_eq!(
            u16::from_le_bytes(bitmap.pixels[..2].try_into().unwrap()),
            0x1234
        );
        assert_eq!(ctx.kernel.window_rects.get(&hwnd), Some(&(7, 9, 75, 75)));
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x8000);
        assert!(ctx.kernel.gdi.list_paint_frames.is_empty());
        let buffer = ctx.kernel.gdi.create_memory_dc();
        // Reproduce the first paint: there is no retained window surface yet.
        ctx.kernel.window_dcs.remove(&hwnd);
        let bitmap = ctx.kernel.gdi.create_compatible_bitmap(75, 75);
        ctx.kernel.gdi.dc_mut(buffer).unwrap().selected_bitmap = Some(bitmap);
        for (r, v) in [
            (ArmReg::R0, hwnd),
            (ArmReg::R1, WM_PAINT),
            (ArmReg::R2, buffer),
            (ArmReg::R3, 0),
        ] {
            ctx.cpu.write_reg(r, v).unwrap();
        }
        assert_eq!(
            paint(&mut ctx, hwnd).unwrap(),
            DispatchOutcome::JumpTo(0x6000)
        );
        let ptr = ctx.cpu.read_reg(ArmReg::R3).unwrap();
        assert_eq!(ctx.cpu.read_u32_le(ptr + 16).unwrap(), buffer);
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::ReturnedR0(0))
        );
        assert_ne!(ctx.kernel.window_dcs.get(&hwnd), Some(&buffer));
        let retained = ctx.kernel.window_dcs[&hwnd];
        let retained_bitmap = ctx
            .kernel
            .gdi
            .dc(retained)
            .unwrap()
            .selected_bitmap
            .unwrap();
        assert_eq!(
            ctx.kernel.gdi.bitmap(retained_bitmap).unwrap().pixels,
            ctx.kernel.gdi.bitmap(bitmap).unwrap().pixels
        );
        ctx.kernel.gdi.delete(buffer);
        ctx.kernel.gdi.delete(bitmap);
        assert!(ctx.kernel.gdi.bitmap(retained_bitmap).is_some());
        let bitmap = ctx.kernel.gdi.bitmap(retained_bitmap).unwrap();
        assert_eq!(
            u16::from_le_bytes(bitmap.pixels[..2].try_into().unwrap()),
            colorref_to_rgb565(0x00ffffff)
        );
    }
}
