//! CE non-client input goes through the guest procedure. CerfDemo can handle
//! WM_CLOSE before DefWindowProc destroys its About window, just as on CE.
use super::*;
use pocket_kernel::windowing::{self, Drag};

fn native(kernel: &KernelState, hwnd: u32) -> bool {
    kernel.ce.windows.enabled
        && kernel.window_rects.contains_key(&hwnd)
        && kernel.controls.get(hwnd).is_none()
        && kernel.controls.panel(hwnd).is_none()
}

pub(super) fn window_at(kernel: &KernelState, x: i32, y: i32) -> Option<u32> {
    let mut order = kernel.ce.windows.z_order.clone();
    let mut missing: Vec<_> = kernel
        .window_rects
        .keys()
        .copied()
        .filter(|h| !order.contains(h))
        .collect();
    missing.sort_unstable();
    order.extend(missing);
    order.sort_by_key(|hwnd| !kernel.is_desktop_window(*hwnd));
    order.into_iter().rev().find(|&hwnd| {
        // Explorer's taskbar still receives input when classic frames are
        // disabled; decoration policy must not select the mouse recipient.
        if kernel.controls.get(hwnd).is_some()
            || kernel.controls.panel(hwnd).is_some()
            || !window_is_visible(kernel, hwnd)
        {
            return false;
        }
        let Some(r) = kernel.native_window_screen_rect(hwnd) else {
            return false;
        };
        windowing::contains(
            [r.0, r.1, r.0.saturating_add(r.2), r.1.saturating_add(r.3)],
            x,
            y,
        )
    })
}

fn activate(kernel: &mut KernelState, hwnd: u32) {
    let old = kernel.active_window;
    if old != hwnd {
        if old != 0 && kernel.window_procs.contains_key(&old) {
            kernel
                .posted_messages
                .push_back((old, WM_ACTIVATE, 0, hwnd));
        }
        kernel.active_window = hwnd;
        kernel.controls.focus = hwnd;
        kernel
            .posted_messages
            .push_back((hwnd, WM_ACTIVATE, 1, old));
        kernel
            .posted_messages
            .push_back((hwnd, WM_SETFOCUS, old, 0));
    }
    raise_window_panels(kernel, hwnd);
}

fn change_bounds(
    kernel: &mut KernelState,
    hwnd: u32,
    bounds: (i32, i32, i32, i32),
    size_kind: u32,
) {
    let old = kernel.window_rects.insert(hwnd, bounds);
    if old == Some(bounds) {
        return;
    }
    let insets = kernel.window_metrics(hwnd).insets();
    let x = bounds.0.saturating_add(insets[0]);
    let y = bounds.1.saturating_add(insets[1]);
    kernel.posted_messages.retain(|m| m.0 != hwnd || m.1 != 3);
    kernel
        .posted_messages
        .push_back((hwnd, 3, 0, (x as u32 & 65535) | ((y as u32 & 65535) << 16)));
    queue_window_size(kernel, hwnd, bounds.2, bounds.3);
    if let Some(message) = kernel
        .posted_messages
        .iter_mut()
        .rev()
        .find(|m| m.0 == hwnd && m.1 == WM_SIZE)
    {
        message.2 = size_kind;
    }
    resize_window_dc(kernel, hwnd);
    queue_visible_window_paints(kernel, hwnd);
    kernel.composite_controls();
}

pub(super) fn show_state(kernel: &mut KernelState, hwnd: u32, command: u32) -> bool {
    if !native(kernel, hwnd) {
        return false;
    }
    let Some(bounds) = kernel.window_rects.get(&hwnd).copied() else {
        return false;
    };
    let style = kernel.window_styles.get(&hwnd).map_or(0, |s| s.0);
    match command & 0xfff0 {
        0xf020 => {
            kernel.ce.windows.restored.entry(hwnd).or_insert(bounds);
            kernel.window_styles.get_mut(&hwnd).unwrap().0 = (style & !0x0100_0000) | 0x2000_0000;
            let m = kernel.window_metrics(hwnd);
            let height = m.caption + 2 * m.border + 2 * m.client_edge;
            let x = (kernel
                .ce
                .windows
                .z_order
                .iter()
                .position(|h| *h == hwnd)
                .unwrap_or(0) as i32
                * 164)
                % (kernel.framebuffer.width as i32).max(1);
            change_bounds(
                kernel,
                hwnd,
                (
                    x,
                    (kernel.framebuffer.height as i32 - height).max(0),
                    160,
                    height,
                ),
                1,
            );
        }
        0xf030 => {
            kernel.ce.windows.restored.entry(hwnd).or_insert(bounds);
            kernel.window_styles.get_mut(&hwnd).unwrap().0 = (style & !0x2000_0000) | 0x0100_0000;
            change_bounds(
                kernel,
                hwnd,
                (
                    0,
                    0,
                    kernel.framebuffer.width as i32,
                    kernel.framebuffer.height as i32,
                ),
                2,
            );
        }
        0xf120 => {
            kernel.window_styles.get_mut(&hwnd).unwrap().0 = style & !0x2100_0000;
            let restore = kernel.ce.windows.restored.remove(&hwnd).unwrap_or(bounds);
            change_bounds(kernel, hwnd, restore, 0);
        }
        _ => return false,
    }
    let event = match command & 0xfff0 {
        0xf020 if style & 0x20000000 == 0 => {
            Some(pocket_kernel::system_sound::SystemEvent::Minimize)
        }
        0xf030 if style & 0x01000000 == 0 => {
            Some(pocket_kernel::system_sound::SystemEvent::Maximize)
        }
        0xf120 if style & 0x20000000 != 0 => {
            Some(pocket_kernel::system_sound::SystemEvent::Maximize)
        }
        0xf120 if style & 0x01000000 != 0 => {
            Some(pocket_kernel::system_sound::SystemEvent::Minimize)
        }
        _ => None,
    };
    if let Some(event) = event {
        system_sound::window_event(kernel, hwnd, event);
    }
    true
}

pub(super) fn take_input(
    ctx: &mut CallCtx<'_>,
    event: InputEvent,
) -> Option<Option<(u32, u32, u32, u32)>> {
    if !ctx.kernel.ce.windows.enabled
        || ctx.kernel.modal.is_some()
        || ctx
            .kernel
            .controls
            .panel(ctx.kernel.active_window)
            .is_some()
    {
        return None;
    }
    let (x, y, message) = match event {
        InputEvent::PointerDown { x, y } => (i32::from(x), i32::from(y), 0xa1),
        InputEvent::PointerUp { x, y } => (i32::from(x), i32::from(y), 0xa2),
        InputEvent::PointerMove { x, y } => (i32::from(x), i32::from(y), 0xa0),
        _ => return None,
    };
    let lp = (x as u32 & 65535) | ((y as u32 & 65535) << 16);
    if let Some(drag) = ctx.kernel.ce.windows.drag {
        if message == 0xa2 {
            ctx.kernel.ce.windows.drag = None;
            return Some(None);
        }
        if message == 0xa0 {
            let (dx, dy) = (x - drag.start.0, y - drag.start.1);
            let (mut l, mut t, w, h) = drag.bounds;
            let (mut r, mut b) = (l + w, t + h);
            if drag.hit == 2 {
                l += dx;
                t = (t + dy).max(0);
                r = l + w;
                b = t + h;
            } else {
                if matches!(drag.hit, 10 | 13 | 16) {
                    l = (l + dx).min(r - 112);
                }
                if matches!(drag.hit, 11 | 14 | 17) {
                    r = (r + dx).max(l + 112);
                }
                if matches!(drag.hit, 12..=14) {
                    t = (t + dy).min(b - 48);
                }
                if matches!(drag.hit, 15..=17) {
                    b = (b + dy).max(t + 48);
                }
            }
            change_bounds(ctx.kernel, drag.hwnd, (l, t, r - l, b - t), 0);
        }
        return Some(None);
    }
    if let Some((hwnd, hit)) = ctx.kernel.ce.windows.pressed {
        if message == 0xa2 {
            return Some(Some((hwnd, message, hit, lp)));
        }
        return Some(None);
    }
    if let Some(hwnd) = ctx.kernel.ce.windows.system_menu {
        if message == 0xa1 {
            ctx.kernel.ce.windows.system_menu = None;
            let rect = ctx.kernel.native_window_screen_rect(hwnd)?;
            let popup = windowing::system_menu_rect(rect, ctx.kernel.window_metrics(hwnd));
            if windowing::contains(popup, x, y) {
                let row = ((y - popup[1] - 2).max(0) / windowing::MENU_HEIGHT) as usize;
                if let Some(&(command, _, true)) =
                    windowing::system_menu_items(ctx.kernel.window_styles[&hwnd].0).get(row)
                {
                    return Some(Some((hwnd, 0x112, command, lp)));
                }
            }
            ctx.kernel.composite_controls();
        }
        return Some(None);
    }
    let hwnd = window_at(ctx.kernel, x, y)?;
    if message == 0xa1 {
        activate(ctx.kernel, hwnd);
    }
    let rect = ctx.kernel.native_window_screen_rect(hwnd)?;
    let hit = windowing::hit_test(
        rect,
        ctx.kernel.window_styles[&hwnd].0,
        ctx.kernel.window_metrics(hwnd),
        x,
        y,
    );
    if hit != 1 {
        Some(Some((hwnd, message, hit, lp)))
    } else {
        None
    }
}

pub(super) fn default_message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
) -> Result<Option<DispatchOutcome>, KernelError> {
    if !native(ctx.kernel, hwnd) {
        return Ok(None);
    }
    let message = ctx.arg_u32(1)?;
    let wp = ctx.arg_u32(2)?;
    let lp = ctx.arg_u32(3)?;
    let (x, y) = ((lp as u16 as i16) as i32, ((lp >> 16) as u16 as i16) as i32);
    let metrics = ctx.kernel.window_metrics(hwnd);
    let rect = ctx.kernel.native_window_screen_rect(hwnd).unwrap();
    let style = ctx.kernel.window_styles[&hwnd].0;
    let result = match message {
        0x84 => windowing::hit_test(rect, style, metrics, x, y),
        0x83 => {
            if lp != 0 {
                let mut raw = ctx.cpu.read_mem(lp, 16)?;
                let [l, t, r, b] = metrics.insets();
                for (index, delta) in [l, t, -r, -b].into_iter().enumerate() {
                    let offset = index * 4;
                    let value = i32::from_le_bytes(raw[offset..offset + 4].try_into().unwrap())
                        .saturating_add(delta);
                    raw[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                ctx.cpu.write_mem(lp, &raw)?;
            }
            0
        }
        0x85 | 0x86 => {
            ctx.kernel.composite_controls();
            1
        }
        0xa1 => {
            if matches!(wp, 2 | 10..=17) {
                if wp != 2 || style & 0x0100_0000 == 0 {
                    ctx.kernel.ce.windows.drag = Some(Drag {
                        hwnd,
                        hit: wp,
                        start: (x, y),
                        bounds: ctx.kernel.window_rects[&hwnd],
                    });
                }
            } else if matches!(wp, 3 | 8 | 9 | 20) {
                ctx.kernel.ce.windows.pressed = Some((hwnd, wp));
            }
            ctx.kernel.composite_controls();
            0
        }
        0xa2 => {
            let pressed = ctx.kernel.ce.windows.pressed.take();
            ctx.kernel.ce.windows.drag = None;
            if pressed == Some((hwnd, wp)) && windowing::hit_test(rect, style, metrics, x, y) == wp
            {
                if wp == 3 {
                    ctx.kernel.ce.windows.system_menu = Some(hwnd);
                } else if let Some(button) = windowing::caption_buttons(rect, style, metrics)
                    .into_iter()
                    .find(|b| b.hit == wp)
                {
                    ctx.kernel
                        .posted_messages
                        .push_back((hwnd, 0x112, button.command, lp));
                }
            }
            ctx.kernel.composite_controls();
            0
        }
        0xa3 if wp == 2 && style & windowing::WS_MAXIMIZEBOX != 0 => {
            show_state(
                ctx.kernel,
                hwnd,
                if style & 0x0100_0000 != 0 {
                    0xf120
                } else {
                    0xf030
                },
            );
            0
        }
        0x112 => {
            if wp & 0xfff0 == 0xf060 {
                ctx.kernel.posted_messages.push_back((hwnd, 0x10, 0, 0));
            } else if !show_state(ctx.kernel, hwnd, wp) && matches!(wp & 0xfff0, 0xf010 | 0xf000) {
                ctx.kernel.ce.windows.drag = Some(Drag {
                    hwnd,
                    hit: if wp & 0xfff0 == 0xf010 { 2 } else { 17 },
                    start: (x, y),
                    bounds: ctx.kernel.window_rects[&hwnd],
                });
            }
            0
        }
        _ => return Ok(None),
    };
    Ok(Some(DispatchOutcome::ReturnedR0(result)))
}
