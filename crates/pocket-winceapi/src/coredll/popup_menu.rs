//! Modal menu callbacks stay on their owning guest thread. Portable Explorer
//! supplies itemData rather than labels and draws its Start menu and banner.
use super::*;
use pocket_kernel::shell_menu::{Action, MenuCallback, MenuPaint, TrackingMenu, TRACKING_HANDLE};

fn restore_call(ctx: &mut CallCtx<'_>, frame: GuestCallFrame) -> Result<(), KernelError> {
    for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(frame.args)
    {
        ctx.cpu.write_reg(reg, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, frame.lr)?;
    Ok(())
}

fn callback(
    ctx: &mut CallCtx<'_>,
    message: u32,
    wp: u32,
    lp: u32,
    kind: MenuCallback,
) -> Result<DispatchOutcome, KernelError> {
    let t = ctx.kernel.gdi.shell_menus.tracking.as_mut().unwrap();
    t.callback = Some(kind);
    let owner = t.owner;
    let proc = ctx.kernel.window_procs.get(&owner).copied().unwrap_or(0);
    ctx.cpu.write_reg(ArmReg::R0, owner)?;
    ctx.cpu.write_reg(ArmReg::R1, message)?;
    ctx.cpu.write_reg(ArmReg::R2, wp)?;
    ctx.cpu.write_reg(ArmReg::R3, lp)?;
    ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
    Ok(DispatchOutcome::JumpTo(proc & !1))
}

fn discard_paint(ctx: &mut CallCtx<'_>) {
    let paints = ctx
        .kernel
        .gdi
        .shell_menus
        .tracking
        .as_mut()
        .map(|t| std::mem::take(&mut t.paints))
        .unwrap_or_default();
    for p in paints {
        ctx.kernel.gdi.delete(p.dc);
        ctx.kernel.gdi.delete(p.bitmap);
    }
}

fn finish(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let Some(result) = ctx
        .kernel
        .gdi
        .shell_menus
        .tracking
        .as_ref()
        .and_then(|t| t.result)
    else {
        return Ok(None);
    };
    discard_paint(ctx);
    let t = ctx.kernel.gdi.shell_menus.tracking.take().unwrap();
    do_free(ctx, t.scratch);
    ctx.kernel.gdi.shell_menus.close();
    ctx.kernel.gdi.shell_menus.bars.remove(&TRACKING_HANDLE);
    ctx.kernel.framebuffer.mark_dirty();
    restore_call(ctx, t.frame)?;
    if result != 0 && t.flags & (0x100 | 0x80) == 0 {
        ctx.kernel
            .posted_messages
            .push_back((t.owner, WM_COMMAND, result, 0));
    }
    Ok(Some(DispatchOutcome::ReturnedR0(if t.flags & 0x100 != 0 {
        result
    } else {
        u32::from(result != 0)
    })))
}

fn scratch(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let current = ctx
        .kernel
        .gdi
        .shell_menus
        .tracking
        .as_ref()
        .unwrap()
        .scratch;
    if current != 0 {
        return Ok(current);
    }
    let DispatchOutcome::ReturnedR0(pointer) = do_alloc(ctx, 48)? else {
        return Ok(0);
    };
    ctx.kernel
        .gdi
        .shell_menus
        .tracking
        .as_mut()
        .unwrap()
        .scratch = pointer;
    if pointer == 0 {
        ctx.kernel.gdi.shell_menus.tracking.as_mut().unwrap().result = Some(0);
    }
    Ok(pointer)
}

fn prepare_paint(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let shell = &ctx.kernel.gdi.shell_menus;
    let t = shell.tracking.as_ref().unwrap();
    let owner = t.owner;
    if ctx.kernel.window_procs.get(&owner).copied().unwrap_or(0) == 0 {
        return Ok(None);
    }
    let layouts = shell.popup_layouts(
        ctx.kernel.framebuffer.width as i32,
        ctx.kernel.framebuffer.height as i32,
        &ctx.kernel.menu_items,
    );
    let measure = layouts.iter().find_map(|layout| {
        ctx.kernel
            .menu_items
            .get(&layout.menu)?
            .iter()
            .enumerate()
            .find(|(index, item)| {
                item.kind & 0x100 != 0 && !shell.measurements.contains_key(&(layout.menu, *index))
            })
            .map(|(index, item)| (layout.menu, index, item.clone()))
    });
    if let Some((menu, index, item)) = measure {
        let pointer = scratch(ctx)?;
        if pointer == 0 {
            return Ok(None);
        }
        let fields: [u32; 6] = [1, 0, item.id, 64, 20, item.data];
        let bytes: Vec<_> = fields.into_iter().flat_map(u32::to_le_bytes).collect();
        ctx.cpu.write_mem(pointer, &bytes)?;
        return callback(ctx, 0x2c, 0, pointer, MenuCallback::Measure { menu, index }).map(Some);
    }
    let snapshots: Vec<_> = layouts
        .into_iter()
        .map(|layout| {
            let items = ctx
                .kernel
                .menu_items
                .get(&layout.menu)
                .cloned()
                .unwrap_or_default();
            let states: Vec<_> = items
                .iter()
                .map(|item| {
                    ctx.kernel
                        .menus
                        .get(&layout.menu)
                        .and_then(|m| m.get(&item.id))
                        .copied()
                        .unwrap_or(0)
                })
                .collect();
            (layout, items, states)
        })
        .collect();
    let old = std::mem::take(&mut ctx.kernel.gdi.shell_menus.tracking.as_mut().unwrap().paints);
    for p in old {
        if snapshots.iter().any(|(layout, items, states)| {
            p.layout == *layout && p.items == *items && p.states == *states
        }) {
            ctx.kernel
                .gdi
                .shell_menus
                .tracking
                .as_mut()
                .unwrap()
                .paints
                .push(p);
        } else {
            ctx.kernel.gdi.delete(p.dc);
            ctx.kernel.gdi.delete(p.bitmap);
        }
    }
    for (layout, items, states) in snapshots {
        if ctx
            .kernel
            .gdi
            .shell_menus
            .tracking
            .as_ref()
            .unwrap()
            .paints
            .iter()
            .any(|p| p.layout.menu == layout.menu)
        {
            continue;
        }
        let dc = ctx.kernel.gdi.create_memory_dc();
        let bitmap = ctx
            .kernel
            .gdi
            .create_compatible_bitmap(layout.width as u32, layout.height as u32);
        ctx.kernel.gdi.select_into(dc, bitmap);
        let color = colorref_to_rgb565(profile_sys_color(ctx.kernel, 4));
        let bm = ctx.kernel.gdi.bitmap_mut(bitmap).unwrap();
        Surface::Bitmap(bm).fill_rect(0, 0, layout.width, layout.height, color);
        ctx.kernel
            .gdi
            .shell_menus
            .tracking
            .as_mut()
            .unwrap()
            .paints
            .push(MenuPaint {
                layout,
                items,
                states,
                dc,
                bitmap,
                next: 0,
                ready: false,
            });
    }
    loop {
        let Some(p) = ctx
            .kernel
            .gdi
            .shell_menus
            .tracking
            .as_mut()
            .unwrap()
            .paints
            .iter_mut()
            .find(|p| !p.ready)
        else {
            return Ok(None);
        };
        let Some(&(index, r)) = p.layout.rows.get(p.next) else {
            p.ready = true;
            ctx.kernel.framebuffer.mark_dirty();
            continue;
        };
        p.next += 1;
        let item = &p.items[index];
        if item.kind & 0x100 == 0 {
            continue;
        }
        let state = u32::from(index == p.layout.selected)
            | if p.states[index] & 3 != 0 { 4 } else { 0 }
            | (p.states[index] & 8);
        let fields: [u32; 12] = [
            1,
            0,
            item.id,
            1,
            state,
            p.layout.menu,
            p.dc,
            (r[0] - p.layout.left) as u32,
            (r[1] - p.layout.top) as u32,
            (r[2] - p.layout.left) as u32,
            (r[3] - p.layout.top) as u32,
            item.data,
        ];
        let dc = p.dc;
        let saved = ctx.kernel.gdi.save_dc(dc).unwrap_or(0);
        let pointer = scratch(ctx)?;
        if pointer == 0 {
            return Ok(None);
        }
        let bytes: Vec<_> = fields.into_iter().flat_map(u32::to_le_bytes).collect();
        ctx.cpu.write_mem(pointer, &bytes)?;
        return callback(ctx, 0x2b, 0, pointer, MenuCallback::Draw { dc, saved }).map(Some);
    }
}

pub(super) fn track(ctx: &mut CallCtx<'_>, ex: bool) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    let thunk = ctx.thunk.thunk_va;
    let thread = ctx.kernel.current_thread;
    if let Some(t) = ctx.kernel.gdi.shell_menus.tracking.as_mut() {
        if t.thread != thread || t.thunk != thunk || t.frame.sp != sp {
            set_thread_error(ctx, 1446);
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        let pending = t.callback.take();
        let (frame, pointer) = (t.frame, t.scratch);
        if let Some(MenuCallback::Measure { menu, index }) = pending {
            let width = ctx
                .cpu
                .read_u32_le(pointer + 12)?
                .clamp(1, ctx.kernel.framebuffer.width.max(1));
            let height = ctx
                .cpu
                .read_u32_le(pointer + 16)?
                .clamp(1, ctx.kernel.framebuffer.height.max(1));
            ctx.kernel
                .gdi
                .shell_menus
                .measurements
                .insert((menu, index), (width, height));
        }
        if let Some(MenuCallback::Draw { dc, saved }) = pending {
            if saved != 0 {
                ctx.kernel.gdi.restore_dc(dc, saved as i32);
            }
        }
        if pending.is_some() {
            restore_call(ctx, frame)?;
        }
    } else {
        let menu = ctx.arg_u32(0)?;
        let flags = ctx.arg_u32(1)?;
        let x = ctx.arg_u32(2)? as i32;
        let y = ctx.arg_u32(3)? as i32;
        let owner = ctx.arg_u32(if ex { 4 } else { 5 })?;
        if !ctx.kernel.menu_items.contains_key(&menu) || !window_is_visible(ctx.kernel, owner) {
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        let frame = GuestCallFrame {
            args: [menu, flags, x as u32, y as u32],
            sp,
            lr: ctx.cpu.read_reg(ArmReg::Lr)?,
        };
        let proc = ctx.kernel.window_procs.get(&owner).copied().unwrap_or(0);
        ctx.kernel.gdi.shell_menus.begin_tracking(
            TrackingMenu {
                thread,
                thunk,
                frame,
                owner,
                flags,
                point: (x, y),
                result: None,
                callback: None,
                scratch: 0,
                paints: Vec::new(),
            },
            menu,
        );
        system_sound::play_event(
            ctx.kernel,
            pocket_kernel::system_sound::SystemEvent::MenuPopup,
        );
        ctx.kernel.framebuffer.mark_dirty();
        if proc != 0 && flags & 0x80 == 0 {
            return callback(ctx, 0x117, menu, 0, MenuCallback::Initialize);
        }
    }
    if let Some(outcome) = finish(ctx)? {
        return Ok(outcome);
    }
    if let Some(outcome) = prepare_paint(ctx)? {
        return Ok(outcome);
    }
    if let Some(outcome) = finish(ctx)? {
        return Ok(outcome);
    }
    while let Some(event) = take_pending_input(ctx) {
        update_key_state(ctx, event);
        let action = ctx.kernel.gdi.shell_menus.input(
            TRACKING_HANDLE,
            event,
            ctx.kernel.framebuffer.width as i32,
            ctx.kernel.framebuffer.height as i32,
            &ctx.kernel.menu_items,
            &ctx.kernel.menus,
        );
        ctx.kernel.framebuffer.mark_dirty();
        match action {
            Some(Action::Command(_, id)) => {
                system_sound::play_event(
                    ctx.kernel,
                    pocket_kernel::system_sound::SystemEvent::MenuSelect,
                );
                ctx.kernel.gdi.shell_menus.tracking.as_mut().unwrap().result = Some(id)
            }
            Some(Action::Open(owner, menu)) => {
                system_sound::play_event(
                    ctx.kernel,
                    pocket_kernel::system_sound::SystemEvent::MenuPopup,
                );
                if ctx.kernel.window_procs.get(&owner).copied().unwrap_or(0) != 0
                    && ctx.kernel.gdi.shell_menus.tracking.as_ref().unwrap().flags & 0x80 == 0
                {
                    return callback(ctx, 0x117, menu, 0, MenuCallback::Initialize);
                }
            }
            _ => {}
        }
        if ctx
            .kernel
            .gdi
            .shell_menus
            .tracking
            .as_ref()
            .unwrap()
            .result
            .is_some()
        {
            break;
        }
    }
    if let Some(outcome) = finish(ctx)? {
        return Ok(outcome);
    }
    if let Some(outcome) = prepare_paint(ctx)? {
        return Ok(outcome);
    }
    if let Some(outcome) = wait_display_until(
        ctx,
        std::time::Instant::now() + std::time::Duration::from_millis(1),
    )? {
        return Ok(outcome);
    }
    Ok(DispatchOutcome::JumpTo(thunk))
}
