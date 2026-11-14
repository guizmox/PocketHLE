//! CE CommandBar/CommandBands own real child windows and ordered rebar bands.
//! Portable Explorer uses the CE ordinal imports even without a system image.
use super::*;
use pocket_kernel::{
    controls::{RebarBand, RebarState},
    windowing,
};

pub(crate) fn register(d: &mut WinCeDispatcher) {
    for (ordinal, name, handler) in [
        (3, "CommandBar_Create", create_bar as crate::Handler),
        (4, "CommandBar_Show", show),
        (5, "CommandBar_AddBitmap", add_bitmap),
        (6, "CommandBar_InsertComboBox", insert_combo),
        (7, "CommandBar_InsertControl", insert_control),
        (8, "CommandBar_InsertMenubar", insert_menu),
        (9, "CommandBar_GetMenu", get_menu),
        (10, "CommandBar_AddAdornments", adorn),
        (11, "CommandBar_GetItemWindow", item_window),
        (12, "CommandBar_Height", height),
        (36, "CommandBands_Create", create_bands),
        (37, "CommandBands_AddBands", add_bands),
        (38, "CommandBands_GetCommandBar", get_bar),
        (39, "CommandBands_AddAdornments", adorn_bands),
        (40, "CommandBands_Show", show),
        (41, "CommandBands_GetRestoreInformation", restore_info),
        (42, "CommandBar_InsertMenubarEx", insert_menu_ex),
        (43, "CommandBar_DrawMenuBar", draw_menu),
        (44, "CommandBar_AlignAdornments", align_adornments),
    ] {
        d.register_handler("commctrl.dll", name, handler);
        d.register_handler("commctrl.dll", &format!("ord:{ordinal}"), handler);
    }
}
fn ret(value: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value))
}

fn child(ctx: &mut CallCtx<'_>, parent: u32, class: ControlClass, id: u32, height: i32) -> u32 {
    let width = ctx
        .kernel
        .window_client_size(parent)
        .map_or(ctx.kernel.framebuffer.width as i32, |s| s.0);
    let hwnd = ctx.kernel.controls.create(
        parent,
        class,
        id,
        String::new(),
        0x50000000,
        0,
        0,
        width,
        height,
    );
    ctx.kernel
        .window_threads
        .insert(hwnd, ctx.kernel.current_thread);
    hwnd
}
fn valid_parent(ctx: &CallCtx<'_>, parent: u32) -> bool {
    ctx.kernel.window_rects.contains_key(&parent)
        || ctx.kernel.controls.get(parent).is_some()
        || ctx.kernel.controls.panel(parent).is_some()
        || (!ctx.kernel.ce.windows.enabled && parent == FAKE_HWND)
}
fn create_bar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (parent, id) = (ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if !valid_parent(ctx, parent) {
        return ret(0);
    }
    ret(child(ctx, parent, ControlClass::Toolbar, id, 24))
}
fn create_bands(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (parent, id) = (ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if !valid_parent(ctx, parent) {
        return ret(0);
    }
    let hwnd = child(ctx, parent, ControlClass::Rebar, id, 0);
    ctx.kernel.controls.get_mut(hwnd).unwrap().style |= ctx.arg_u32(3)?;
    ctx.kernel.gdi.rebars.insert(
        hwnd,
        RebarState {
            command_bands: true,
            ..Default::default()
        },
    );
    ret(hwnd)
}
fn show(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let visible = ctx.arg_u32(1)? != 0;
    let Some(c) = ctx.kernel.controls.get_mut(hwnd) else {
        return ret(0);
    };
    c.visible = visible;
    c.style = (c.style & !0x10000000) | if visible { 0x10000000 } else { 0 };
    queue_window_paint(ctx.kernel, hwnd);
    ctx.kernel.composite_controls();
    ret(1)
}
fn band(ctx: &mut CallCtx<'_>, pointer: u32) -> Result<Option<(RebarBand, u32)>, KernelError> {
    if pointer == 0 || ctx.cpu.check_guest_access(pointer, 8, Prot::READ).is_err() {
        return Ok(None);
    }
    let size = ctx.cpu.read_u32_le(pointer)?;
    if !(56..=128).contains(&size)
        || size % 4 != 0
        || ctx
            .cpu
            .check_guest_access(pointer, size, Prot::READ)
            .is_err()
    {
        return Ok(None);
    }
    let raw = ctx.cpu.read_mem(pointer, size)?;
    let mut out = RebarBand::default();
    out.info[0] = size;
    out.info[1] = u32::from_le_bytes(raw[4..8].try_into().unwrap());
    out.info[7] = u32::MAX;
    // Explorer only initializes fields selected by fMask. In particular its
    // CommandBands_AddBands records do not initialize hwndChild at all.
    for (bit, fields) in band_fields() {
        if out.info[1] & bit != 0 {
            for &i in fields {
                if let Some(bytes) = raw.get(i * 4..i * 4 + 4) {
                    out.info[i] = u32::from_le_bytes(bytes.try_into().unwrap());
                }
            }
        }
    }
    if out.info[1] & 4 != 0 && out.info[5] != 0 {
        out.text = read_wstr(ctx, out.info[5], 4096)?;
    }
    Ok(Some((out, size)))
}

fn band_fields() -> [(u32, &'static [usize]); 11] {
    [
        (1, &[2]),
        (2, &[3, 4]),
        (4, &[5, 6]),
        (8, &[7]),
        (16, &[8]),
        (32, &[9, 10, 14, 15, 16]),
        (64, &[11]),
        (128, &[12]),
        (256, &[13]),
        (512, &[17]),
        (1024, &[18]),
    ]
}

pub(super) fn layout(ctx: &mut CallCtx<'_>, hwnd: u32) -> Result<(), KernelError> {
    let Some(c) = ctx.kernel.controls.get(hwnd) else {
        return Ok(());
    };
    let (width, parent, id, old_height) = (c.w.max(1), c.parent, c.id, c.h);
    let Some(rebar) = ctx.kernel.gdi.rebars.get_mut(&hwnd) else {
        return Ok(());
    };
    let mut x = 0i32;
    let mut y = 0i32;
    let mut row_height = 0i32;
    for b in &mut rebar.bands {
        if b.info[2] & 8 != 0 {
            b.rect = [0; 4];
            if let Some(c) = ctx.kernel.controls.get_mut(b.info[8]) {
                c.visible = false;
            }
            continue;
        }
        let preferred = ctx.kernel.controls.get(b.info[8]).map_or(0, |c| {
            if c.class == ControlClass::Toolbar {
                if let Some(menu) = ctx.kernel.gdi.shell_menus.bars.get(&c.hwnd) {
                    ctx.kernel.menu_items.get(&menu.menu).map_or(0, |items| {
                        items.iter().map(|i| i.text.len() as i32 * 6 + 16).sum()
                    })
                } else {
                    c.toolbar
                        .buttons
                        .iter()
                        .filter(|b| b[2] & 8 == 0)
                        .map(|b| {
                            if b[2] & 0x100 != 0 {
                                (b[0] as i32).max(1)
                            } else {
                                c.toolbar.button_size.0
                            }
                        })
                        .sum()
                }
            } else {
                c.w
            }
        });
        let bw = (b.info[11].min(i32::MAX as u32) as i32)
            .max(b.info[9].min(i32::MAX as u32) as i32)
            .max(preferred)
            .max(1)
            .min(width);
        let closed_height = ctx.kernel.controls.get(b.info[8]).map_or(24, |c| {
            if c.class == ControlClass::ComboBox {
                c.choice_closed_height()
            } else if c.class == ControlClass::Toolbar {
                c.toolbar.button_size.1 + 2
            } else {
                c.h.max(1)
            }
        });
        // Explorer's fixed-height address band leaves the extended cyChild
        // fields uninitialized. They only apply to RBBS_VARIABLEHEIGHT.
        let minimum = (b.info[10].min(2048) as i32).max(closed_height);
        let bh = if b.info[2] & 0x40 != 0 {
            (b.info[14].min(2048) as i32).max(minimum)
        } else {
            minimum
        };
        if x != 0 && (b.info[2] & 1 != 0 || x.saturating_add(bw) > width) {
            y += row_height;
            x = 0;
            row_height = 0;
        }
        b.rect = [x, y, x + bw, y + bh];
        if let Some(c) = ctx.kernel.controls.get_mut(b.info[8]) {
            c.parent = hwnd;
            c.x = x;
            c.y = y;
            c.w = bw;
            c.h = bh;
            c.visible = true;
        }
        x += bw;
        row_height = row_height.max(bh);
    }
    let height = y + row_height;
    ctx.kernel.controls.get_mut(hwnd).unwrap().h = height;
    if old_height != height {
        let mut pointer = ctx.kernel.gdi.rebars[&hwnd].notification;
        if pointer == 0 {
            if let DispatchOutcome::ReturnedR0(p) = do_alloc(ctx, 12)? {
                pointer = p;
            }
            ctx.kernel.gdi.rebars.get_mut(&hwnd).unwrap().notification = pointer;
        }
        if pointer != 0 {
            ctx.cpu.write_mem(
                pointer,
                &[hwnd, id, 0u32.wrapping_sub(831)]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            ctx.kernel
                .posted_messages
                .push_back((parent, 0x4e, id, pointer));
        }
        queue_window_paint(ctx.kernel, parent);
    }
    ctx.kernel.framebuffer.mark_dirty();
    Ok(())
}
fn add_bands(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let count = ctx.arg_u32(2)?;
    let mut pointer = ctx.arg_u32(3)?;
    let Some(existing) = ctx.kernel.gdi.rebars.get(&hwnd) else {
        return ret(0);
    };
    if count > 256 || existing.bands.len() + count as usize > 256 {
        return ret(0);
    }
    let mut added = Vec::new();
    for _ in 0..count {
        let Some((b, size)) = band(ctx, pointer)? else {
            return ret(0);
        };
        added.push(b);
        let Some(next) = pointer.checked_add(size) else {
            return ret(0);
        };
        pointer = next;
    }
    for mut b in added {
        if b.info[8] == 0 {
            b.info[8] = child(ctx, hwnd, ControlClass::Toolbar, b.info[13], 24);
        }
        ctx.kernel.gdi.rebars.get_mut(&hwnd).unwrap().bands.push(b);
    }
    layout(ctx, hwnd)?;
    ret(1)
}
fn get_bar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (hwnd, index) = (ctx.arg_u32(0)?, ctx.arg_u32(1)? as usize);
    ret(ctx
        .kernel
        .gdi
        .rebars
        .get(&hwnd)
        .and_then(|r| r.bands.get(index))
        .map_or(0, |b| b.info[8]))
}
fn get_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    ret(ctx
        .kernel
        .gdi
        .shell_menus
        .bars
        .get(&hwnd)
        .map_or(0, |b| b.menu))
}
fn item_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    ret(
        if ctx.arg_u32(1)? == 0 && ctx.kernel.gdi.shell_menus.bars.contains_key(&hwnd) {
            hwnd
        } else {
            0
        },
    )
}
fn height(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    ret(ctx
        .kernel
        .controls
        .get(hwnd)
        .map_or(0, |c| c.h.max(0) as u32))
}
fn attach_menu(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    menu: u32,
) -> Result<DispatchOutcome, KernelError> {
    if !ctx.kernel.menu_items.contains_key(&menu) {
        return ret(0);
    }
    let Some(c) = ctx
        .kernel
        .controls
        .get(hwnd)
        .filter(|c| c.class == ControlClass::Toolbar)
    else {
        return ret(0);
    };
    let parent = c.parent;
    let mut owner = parent;
    for _ in 0..64 {
        if let Some(c) = ctx.kernel.controls.get(owner) {
            owner = c.parent;
        } else {
            break;
        }
    }
    // Control.exe requests WS_BORDER and positions its list from the global
    // work area. A synthetic caption would cover File/View and the first icons.
    // In classic profiles put Close in its existing CE command bar instead.
    let owner_style = ctx.kernel.window_styles.get(&owner).map_or(0, |s| s.0);
    let close_button = ctx.kernel.ce.windows.enabled
        && owner_style & 0xc000_0000 == 0
        && owner_style & 0x0080_0000 != 0
        && owner_style & windowing::WS_CAPTION != windowing::WS_CAPTION;
    ctx.kernel.gdi.shell_menus.bars.insert(
        hwnd,
        pocket_kernel::shell_menu::Bar {
            owner,
            menu,
            sip: false,
            close_button,
            background: colorref_to_rgb565(sys_color(4)),
            bounds: Some([0, 0, 1, 24]),
        },
    );
    ctx.kernel
        .controls
        .get_mut(hwnd)
        .unwrap()
        .toolbar
        .native_paint = true;
    layout(ctx, parent)?;
    queue_window_paint(ctx.kernel, hwnd);
    ret(1)
}
fn insert_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (hwnd, instance, id) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let Some((entry, base)) = lookup_resource(
        ctx.kernel,
        instance,
        &ResourceKey::Id(4),
        &ResourceKey::Id(id & 0xffff),
    ) else {
        return ret(0);
    };
    let bytes = ctx.cpu.read_mem(
        base.checked_add(entry.data_rva)
            .ok_or(KernelError::Loader("menu resource overflow".into()))?,
        entry.size,
    )?;
    let Some(menu) = menu_api::from_bytes(ctx.kernel, &bytes) else {
        return ret(0);
    };
    attach_menu(ctx, hwnd, menu)
}
fn insert_menu_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (hwnd, instance, raw) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if instance == 0 {
        return attach_menu(ctx, hwnd, raw);
    }
    if raw <= 0xffff {
        return insert_menu(ctx);
    }
    let key = read_wide_resource_key(ctx, raw)?;
    let Some((entry, base)) = lookup_resource(ctx.kernel, instance, &ResourceKey::Id(4), &key)
    else {
        return ret(0);
    };
    let bytes = ctx.cpu.read_mem(
        base.checked_add(entry.data_rva)
            .ok_or(KernelError::Loader("menu resource overflow".into()))?,
        entry.size,
    )?;
    let Some(menu) = menu_api::from_bytes(ctx.kernel, &bytes) else {
        return ret(0);
    };
    attach_menu(ctx, hwnd, menu)
}
fn draw_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    if !ctx.kernel.gdi.shell_menus.bars.contains_key(&hwnd) {
        return ret(0);
    }
    queue_window_paint(ctx.kernel, hwnd);
    ctx.kernel.framebuffer.mark_dirty();
    ret(1)
}
fn add_bitmap(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (hwnd, instance, id, count, width, height) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
        ctx.arg_u32(5)?,
    );
    ret(toolbar::add_bitmap(
        ctx,
        hwnd,
        instance,
        id,
        count,
        width as i32,
        height as i32,
    )?)
}
fn insert_control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // The CE private export creates a control; it does not reparent an HWND.
    // pceshell's Browse dialog inserts its STATIC title with all seven args.
    let hwnd = ctx.arg_u32(0)?;
    let name_pointer = ctx.arg_u32(2)?;
    let name = read_wstr(ctx, name_pointer, 256)?;
    let Some(class) = ControlClass::from_class_name(&String::from_utf16_lossy(&name)) else {
        return ret(0);
    };
    let width = ctx.arg_u32(3)?;
    let style = ctx.arg_u32(4)?;
    let id = ctx.arg_u32(5)? & 0xffff;
    let raw_index = ctx.arg_u32(6)? & 0xffff;
    insert_child(ctx, hwnd, class, width, style, id, raw_index)
}

fn insert_combo(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // WordPad inserts its font list with the six-argument public CE ABI;
    // unlike InsertControl there is no class-name argument in this call.
    let hwnd = ctx.arg_u32(0)?;
    let width = ctx.arg_u32(2)?;
    let style = ctx.arg_u32(3)?;
    let id = ctx.arg_u32(4)? & 0xffff;
    let index = ctx.arg_u32(5)? & 0xffff;
    insert_child(ctx, hwnd, ControlClass::ComboBox, width, style, id, index)
}

fn insert_child(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    class: ControlClass,
    width: u32,
    style: u32,
    id: u32,
    raw_index: u32,
) -> Result<DispatchOutcome, KernelError> {
    let Some(bar) = ctx
        .kernel
        .controls
        .get(hwnd)
        .filter(|c| c.class == ControlClass::Toolbar)
    else {
        return ret(0);
    };
    let index = if raw_index == 0xffff {
        bar.toolbar.buttons.len()
    } else {
        raw_index as usize
    };
    if index > bar.toolbar.buttons.len() || width == 0 || width > 4096 {
        return ret(0);
    }
    let height = bar.toolbar.button_size.1;
    let x = bar.toolbar.buttons[..index]
        .iter()
        .filter(|b| b[2] & 8 == 0)
        .map(|b| {
            if b[2] & 0x100 != 0 {
                b[0] as i32
            } else {
                bar.toolbar.button_size.0
            }
        })
        .sum();
    let control = ctx.kernel.controls.create(
        hwnd,
        class,
        id,
        String::new(),
        style | 0x50000000,
        x,
        0,
        width as i32,
        height,
    );
    ctx.kernel
        .window_threads
        .insert(control, ctx.kernel.current_thread);
    ctx.kernel
        .controls
        .get_mut(hwnd)
        .unwrap()
        .toolbar
        .buttons
        .insert(index, [width, 0, 0x104, control, 0]);
    let parent = ctx.kernel.controls.get(hwnd).unwrap().parent;
    layout(ctx, parent)?;
    queue_window_paint(ctx.kernel, hwnd);
    ret(control)
}
fn adorn(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    if flags & !3 != 0
        || !ctx
            .kernel
            .controls
            .get(hwnd)
            .is_some_and(|c| c.class == ControlClass::Toolbar)
    {
        return ret(0);
    }
    if ctx.kernel.gdi.command_adornments.insert(hwnd) {
        ctx.kernel
            .controls
            .get_mut(hwnd)
            .unwrap()
            .toolbar
            .buttons
            .push([u32::MAX, 0xf060, 4, 0, 0]);
    }
    if let Some(bar) = ctx.kernel.gdi.shell_menus.bars.get_mut(&hwnd) {
        bar.close_button = true;
    }
    align_bar(ctx, hwnd);
    queue_window_paint(ctx.kernel, hwnd);
    ret(1)
}
fn align_bar(ctx: &mut CallCtx<'_>, hwnd: u32) {
    if !ctx.kernel.gdi.command_adornments.contains(&hwnd) {
        return;
    }
    let Some(bar) = ctx.kernel.controls.get_mut(hwnd) else {
        return;
    };
    bar.toolbar
        .buttons
        .retain(|button| !(button[1] == u32::MAX && button[2] & 0x100 != 0 && button[3] == 0));
    let Some(index) = bar
        .toolbar
        .buttons
        .iter()
        .position(|button| button[1] == 0xf060)
    else {
        return;
    };
    let used: i32 = bar
        .toolbar
        .buttons
        .iter()
        .filter(|button| button[2] & 8 == 0)
        .map(|button| {
            if button[2] & 0x100 != 0 {
                button[0].min(i32::MAX as u32) as i32
            } else {
                bar.toolbar.button_size.0
            }
        })
        .sum();
    let gap = bar.w.saturating_sub(used).max(0) as u32;
    if gap != 0 {
        bar.toolbar
            .buttons
            .insert(index, [gap, u32::MAX, 0x104, 0, 0]);
    }
}
fn align_adornments(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    align_bar(ctx, hwnd);
    queue_window_paint(ctx.kernel, hwnd);
    ret(0)
}
fn adorn_bands(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    if !ctx.kernel.gdi.rebars.contains_key(&hwnd) {
        return ret(0);
    }
    let toolbar = child(ctx, hwnd, ControlClass::Toolbar, u32::MAX, 24);
    ctx.kernel
        .controls
        .get_mut(toolbar)
        .unwrap()
        .toolbar
        .buttons
        .push([u32::MAX, 0xf060, 4, 0, 0]);
    ctx.kernel.gdi.command_adornments.insert(toolbar);
    let mut b = RebarBand::default();
    b.info[8] = toolbar;
    b.info[11] = 24;
    b.info[13] = u32::MAX;
    ctx.kernel.gdi.rebars.get_mut(&hwnd).unwrap().bands.push(b);
    layout(ctx, hwnd)?;
    ret(1)
}
fn restore_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (hwnd, index, output) = (ctx.arg_u32(0)?, ctx.arg_u32(1)? as usize, ctx.arg_u32(2)?);
    let Some(b) = ctx
        .kernel
        .gdi
        .rebars
        .get(&hwnd)
        .and_then(|r| r.bands.get(index))
    else {
        return ret(0);
    };
    if output == 0 || ctx.cpu.check_guest_access(output, 20, Prot::WRITE).is_err() {
        return ret(0);
    }
    ctx.cpu.write_mem(
        output,
        &[20, b.info[13], b.info[2], (b.rect[2] - b.rect[0]) as u32, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    ret(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::Cpu;

    fn args(ctx: &mut CallCtx<'_>, values: &[u32]) {
        for (i, &v) in values.iter().enumerate() {
            if i < 4 {
                ctx.cpu.write_arg(i as u8, v).unwrap();
            } else {
                let sp = ctx.cpu.read_reg(ArmReg::Sp).unwrap();
                ctx.cpu
                    .write_mem(sp + (i as u32 - 4) * 4, &v.to_le_bytes())
                    .unwrap();
            }
        }
    }
    fn value(out: DispatchOutcome) -> u32 {
        let DispatchOutcome::ReturnedR0(v) = out else {
            panic!("unexpected detour");
        };
        v
    }

    #[test]
    fn control_panel_close_uses_its_existing_command_bar_without_changing_client_geometry() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut state = super::super::tests::fresh_kernel();
        state.framebuffer = pocket_kernel::framebuffer::deferred::DeferredFramebuffer::new(
            pocket_kernel::framebuffer::Framebuffer::new(320, 240),
        )
        .unwrap();
        state.ce.windows.enabled = true;
        let owner = 0x111;
        let style = windowing::classic_style(0x10800000);
        assert_eq!(style, 0x10800000);
        state.window_rects.insert(owner, (0, 0, 320, 240));
        state.window_styles.insert(owner, (style, 0));
        state.window_visibility.insert(owner, true);
        state.active_window = owner;
        state.ce.windows.z_order.push(owner);
        state.menu_items.insert(
            0x222,
            vec![pocket_kernel::MenuItem {
                id: 11,
                text: "&File".encode_utf16().collect(),
                ..Default::default()
            }],
        );
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        args(&mut ctx, &[0, owner, 25]);
        let hwnd = value(create_bar(&mut ctx).unwrap());
        assert_eq!(
            attach_menu(&mut ctx, hwnd, 0x222).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        ctx.kernel.refresh_control_visibility();
        assert_eq!(ctx.kernel.window_client_origin(owner), Some((1, 1)));
        assert_eq!(ctx.kernel.window_client_size(owner), Some((318, 238)));
        assert_eq!(ctx.kernel.controls.screen_rect(hwnd), Some((1, 1, 318, 24)));
        let bar = &ctx.kernel.gdi.shell_menus.bars[&hwnd];
        let rect = bar.close_rect(bar.bounds.unwrap()).unwrap();
        let (x, y) = (
            ((rect[0] + rect[2]) / 2) as u16,
            ((rect[1] + rect[3]) / 2) as u16,
        );
        assert_eq!(
            crate::aygshell::menu_input(&mut ctx, InputEvent::PointerDown { x, y }),
            Some(None)
        );
        assert_eq!(
            crate::aygshell::menu_input(&mut ctx, InputEvent::PointerUp { x, y }),
            Some(Some((owner, 0x10, 0, 0)))
        );
        assert!(ctx.kernel.window_rects.contains_key(&owner));
        ctx.kernel.ce.windows.enabled = false;
        assert_eq!(
            attach_menu(&mut ctx, hwnd, 0x222).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(!ctx.kernel.gdi.shell_menus.bars[&hwnd].close_button);
    }

    #[test]
    fn command_bands_ignore_unselected_fields_and_fixed_height_extensions() {
        let mut cpu = super::super::tests::scanf_cpu();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let mut state = super::super::tests::fresh_kernel();
        state.window_rects.insert(0x111, (0, 0, 320, 240));
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        args(&mut ctx, &[0, 0x111, 25, 0, 0]);
        let rebar = value(create_bands(&mut ctx).unwrap());
        let mut record = [0xa5a5a5a5u32; 19];
        record[0] = 76;
        record[1] = 1 | 8 | 256;
        record[2] = 0;
        record[7] = 0;
        record[13] = 45;
        ctx.cpu
            .write_mem(
                0x1000,
                &record
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        args(&mut ctx, &[rebar, 0, 1, 0x1000]);
        assert_eq!(add_bands(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
        let child = ctx.kernel.gdi.rebars[&rebar].bands[0].info[8];
        assert_eq!(
            ctx.kernel.controls.get(child).unwrap().class,
            ControlClass::Toolbar
        );
        assert_eq!(ctx.kernel.controls.get(child).unwrap().id, 45);
        assert_eq!(ctx.kernel.controls.get(rebar).unwrap().h, 24);
        let combo = ctx.kernel.controls.create(
            0x111,
            ControlClass::ComboBox,
            50,
            String::new(),
            0x50000000,
            0,
            0,
            200,
            110,
        );
        record[1] = 1 | 16 | 32 | 256;
        record[2] = 1;
        record[8] = combo;
        record[9] = 0;
        record[10] = 22;
        record[13] = 50;
        ctx.cpu
            .write_mem(
                0x1000,
                &record
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        args(&mut ctx, &[rebar, 0, 1, 0x1000]);
        assert_eq!(add_bands(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
        assert!(ctx.kernel.controls.get(rebar).unwrap().h < 100);
        assert_eq!(ctx.kernel.controls.get(combo).unwrap().parent, rebar);
        assert!(ctx
            .kernel
            .posted_messages
            .iter()
            .any(|m| m.0 == 0x111 && m.1 == 0x4e));
    }

    #[test]
    fn private_control_insertion_uses_all_seven_ce_arguments() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut state = super::super::tests::fresh_kernel();
        let bar = state.controls.create(
            0x111,
            ControlClass::Toolbar,
            25,
            String::new(),
            0x50000000,
            0,
            0,
            320,
            24,
        );
        state
            .controls
            .get_mut(bar)
            .unwrap()
            .toolbar
            .buttons
            .push([0, 100, 4, 0, 0]);
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        ctx.cpu
            .write_mem(
                0x1000,
                &"STATIC\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        args(&mut ctx, &[bar, 0x10000000, 0x1000, 80, 0, 1050, 1]);
        let control = value(insert_control(&mut ctx).unwrap());
        let c = ctx.kernel.controls.get(control).unwrap();
        assert_eq!(
            (c.class, c.parent, c.id, c.x, c.w),
            (ControlClass::Static, bar, 1050, 24, 80)
        );
    }

    #[test]
    fn wordpad_font_combo_uses_six_arguments_and_owns_a_real_toolbar_slot() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut state = super::super::tests::fresh_kernel();
        state.window_rects.insert(0x111, (0, 0, 320, 240));
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        args(&mut ctx, &[0, 0x111, 25]);
        let bar = value(create_bar(&mut ctx).unwrap());
        ctx.kernel
            .controls
            .get_mut(bar)
            .unwrap()
            .toolbar
            .buttons
            .push([0, 100, 4, 0, 0]);
        args(&mut ctx, &[bar, 0x10000000, 100, 0x00300003, 1050, 0xffff]);
        let combo = value(insert_combo(&mut ctx).unwrap());
        let c = ctx.kernel.controls.get(combo).unwrap();
        assert_eq!(
            (c.class, c.parent, c.id, c.x, c.w),
            (ControlClass::ComboBox, bar, 1050, 24, 100)
        );
        assert_eq!(c.style & 0x00300003, 0x00300003);
        assert_eq!(
            ctx.kernel.controls.get(bar).unwrap().toolbar.buttons[1],
            [100, 0, 0x104, combo, 0]
        );
        args(&mut ctx, &[bar, 0, 100, 3, 1051, 10]);
        assert_eq!(
            insert_combo(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            ctx.kernel.controls.get(bar).unwrap().toolbar.buttons.len(),
            2
        );
    }

    #[test]
    fn posting_toolbar_messages_does_not_apply_them_synchronously() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut state = super::super::tests::fresh_kernel();
        let bar = state.controls.create(
            0x111,
            ControlClass::Toolbar,
            25,
            String::new(),
            0x50000000,
            0,
            0,
            320,
            24,
        );
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        ctx.cpu
            .write_mem(
                0x1000,
                &[0u32, 100, 4, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        args(&mut ctx, &[bar, 0x444, 1, 0x1000]);
        assert_eq!(
            post_message_w(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(ctx
            .kernel
            .controls
            .get(bar)
            .unwrap()
            .toolbar
            .buttons
            .is_empty());
        assert!(ctx
            .kernel
            .posted_messages
            .contains(&(bar, 0x444, 1, 0x1000)));
    }
}

pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    if !ctx
        .kernel
        .controls
        .get(hwnd)
        .is_some_and(|c| c.class == ControlClass::Rebar)
    {
        return Ok(None);
    }
    ctx.kernel.gdi.rebars.entry(hwnd).or_default();
    let insert = matches!(msg, 0x401 | 0x40a);
    if insert || matches!(msg, 0x406 | 0x40b) {
        let Some((mut b, _)) = band(ctx, lp)? else {
            return Ok(Some(0));
        };
        let current = &ctx.kernel.gdi.rebars[&hwnd];
        let index = if wp == u32::MAX && insert {
            current.bands.len()
        } else {
            wp as usize
        };
        if insert && (index > current.bands.len() || current.bands.len() >= 256) {
            return Ok(Some(0));
        }
        if insert && current.command_bands && b.info[8] == 0 {
            b.info[8] = child(ctx, hwnd, ControlClass::Toolbar, b.info[13], 24);
            b.info[1] |= 16;
        }
        let r = ctx.kernel.gdi.rebars.get_mut(&hwnd).unwrap();
        if insert {
            r.bands.insert(index, b);
        } else {
            let Some(target) = r.bands.get_mut(index) else {
                return Ok(Some(0));
            };
            for (bit, fields) in band_fields() {
                if b.info[1] & bit != 0 {
                    for &i in fields {
                        target.info[i] = b.info[i];
                    }
                }
            }
            if b.info[1] & 4 != 0 {
                target.text = b.text;
            }
            target.info[1] |= b.info[1];
        }
        layout(ctx, hwnd)?;
        return Ok(Some(1));
    }
    if matches!(msg, 0x405 | 0x41c | 0x41d) {
        let Some(b) = ctx.kernel.gdi.rebars[&hwnd].bands.get(wp as usize) else {
            return Ok(Some(0));
        };
        if lp == 0 || ctx.cpu.check_guest_access(lp, 8, Prot::READ).is_err() {
            return Ok(Some(0));
        }
        let size = ctx.cpu.read_u32_le(lp)?;
        if !(56..=128).contains(&size)
            || size % 4 != 0
            || ctx.cpu.check_guest_access(lp, size, Prot::WRITE).is_err()
        {
            return Ok(Some(0));
        }
        let mut data = ctx.cpu.read_mem(lp, size)?;
        let mask = u32::from_le_bytes(data[4..8].try_into().unwrap());
        for (bit, fields) in [
            (1, &[2][..]),
            (2, &[3, 4][..]),
            (8, &[7][..]),
            (16, &[8][..]),
            (32, &[9, 10, 14, 15, 16][..]),
            (64, &[11][..]),
            (128, &[12][..]),
            (256, &[13][..]),
            (512, &[17][..]),
            (1024, &[18][..]),
        ] {
            if mask & bit != 0 {
                for &i in fields {
                    if i * 4 + 4 <= data.len() {
                        data[i * 4..i * 4 + 4].copy_from_slice(&b.info[i].to_le_bytes());
                    }
                }
            }
        }
        ctx.cpu.write_mem(lp, &data)?;
        return Ok(Some(1));
    }
    let r = &mut ctx.kernel.gdi.rebars.get_mut(&hwnd).unwrap().bands;
    let value = match msg {
        0x40c => r.len() as u32,
        0x410 => r
            .iter()
            .position(|b| b.info[13] == wp)
            .map_or(u32::MAX, |i| i as u32),
        0x402 => {
            if wp as usize >= r.len() {
                return Ok(Some(0));
            }
            r.remove(wp as usize);
            layout(ctx, hwnd)?;
            1
        }
        0x423 => {
            let Some(b) = r.get_mut(wp as usize) else {
                return Ok(Some(0));
            };
            b.info[2] = (b.info[2] & !8) | if lp == 0 { 8 } else { 0 };
            layout(ctx, hwnd)?;
            1
        }
        0x41e | 0x41f => {
            let Some(b) = r.get_mut(wp as usize) else {
                return Ok(Some(0));
            };
            b.info[11] = if msg == 0x41f {
                ctx.kernel.controls.get(hwnd).unwrap().w.max(1) as u32
            } else {
                b.info[9].max(1)
            };
            layout(ctx, hwnd)?;
            1
        }
        0x409 => {
            let Some(b) = r.get(wp as usize) else {
                return Ok(Some(0));
            };
            if lp == 0 || ctx.cpu.check_guest_access(lp, 16, Prot::WRITE).is_err() {
                return Ok(Some(0));
            }
            ctx.cpu.write_mem(
                lp,
                &b.rect
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            1
        }
        0x41b => ctx.kernel.controls.get(hwnd).unwrap().h.max(0) as u32,
        0x40d => r
            .iter()
            .filter(|b| b.info[2] & 8 == 0)
            .map(|b| b.rect[1])
            .collect::<std::collections::HashSet<_>>()
            .len() as u32,
        5 => {
            layout(ctx, hwnd)?;
            0
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
