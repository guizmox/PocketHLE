//! Observed GMSHELLCLASS text-panel messages. Unknown commands fail rather
//! than claiming hardware/shell services that are not implemented.
use crate::CallCtx;
use pocket_kernel::{controls::ControlClass, DispatchOutcome, KernelError};
fn string(ctx: &mut CallCtx<'_>, ptr: u32) -> Result<String, KernelError> {
    if ptr == 0 {
        return Ok(String::new());
    }
    let mut units = Vec::new();
    for i in 0..128 {
        let raw = ctx.cpu.read_mem(ptr + i * 2, 2)?;
        let u = u16::from_le_bytes([raw[0], raw[1]]);
        if u == 0 {
            break;
        }
        units.push(u);
    }
    Ok(String::from_utf16_lossy(&units))
}
pub(crate) fn find(ctx: &mut CallCtx<'_>) -> Result<bool, KernelError> {
    let class = ctx.arg_u32(0)?;
    if class == 0 || class <= 0xffff {
        return Ok(false);
    }
    let class = string(ctx, class)?;
    if !class.eq_ignore_ascii_case("GMSHELLCLASS") {
        return Ok(false);
    }
    let title_ptr = ctx.arg_u32(1)?;
    let title = string(ctx, title_ptr)?;
    Ok(title.is_empty() || title.eq_ignore_ascii_case("GMSHELL"))
}
fn edit_target(ctx: &CallCtx<'_>) -> u32 {
    let focus = ctx.kernel.controls.focus;
    if ctx.kernel.shell_keyboard.sip {
        if ctx.kernel.gapi_action_key != pocket_kernel::gapi::VK_RETURN { return 0; }
        for hwnd in [focus, ctx.kernel.shell_keyboard.target] {
            if ctx.kernel.controls.get(hwnd).is_some_and(|c| {
                c.class == ControlClass::Edit
                    && c.enabled
                    && c.style & 0x800 == 0
                    && crate::coredll::window_is_visible(ctx.kernel, hwnd)
            }) {
                return hwnd;
            }
        }
        // Explicit PocketPC SIP requests also work for custom WM_CHAR windows.
        // EDIT controls retain their stricter enabled/read-only/visibility rules.
        for hwnd in [focus, ctx.kernel.active_window] {
            if hwnd != 0
                && ctx.kernel.controls.get(hwnd).map_or(
                    ctx.kernel.window_classes.contains_key(&hwnd),
                    |c| c.class != ControlClass::Edit && c.enabled,
                )
                && crate::coredll::window_is_visible(ctx.kernel, hwnd)
            { return hwnd; }
        }
        return 0;
    }
    let Some(control) = ctx.kernel.controls.get(focus) else {
        return 0;
    };
    if control.class == ControlClass::Edit
        && control.enabled
        && crate::coredll::window_is_visible(ctx.kernel, focus)
    {
        return focus;
    }
    ctx.kernel
        .controls
        .children_of(control.parent)
        .into_iter()
        .find(|h| {
            ctx.kernel.controls.get(*h).is_some_and(|c| {
                c.class == ControlClass::Edit
                    && c.visible
                    && c.enabled
                    && crate::coredll::window_is_visible(ctx.kernel, *h)
            })
        })
        .unwrap_or(0)
}
fn post(ctx: &mut CallCtx<'_>, hwnd: u32, msg: u32, wp: u32, lp: u32) {
    if hwnd != 0 && ctx.kernel.posted_messages.len() < 256 {
        ctx.kernel.posted_messages.push_back((hwnd, msg, wp, lp));
    }
}
fn open(ctx: &mut CallCtx<'_>) -> bool {
    let target = edit_target(ctx);
    if target == 0 {
        return false;
    }
    ctx.kernel.shell_keyboard.target = target;
    ctx.kernel.shell_keyboard.visible = true;
    true
}
/// A show request may precede SetFocus during native page construction.
/// Resolve it when the application returns to its message pump, without
/// showing a text panel over list-only pages.
pub(crate) fn service(ctx: &mut CallCtx<'_>) {
    if !ctx.kernel.shell_keyboard.requested {
        return;
    }
    let target = edit_target(ctx);
    let was_visible = ctx.kernel.shell_keyboard.visible;
    ctx.kernel.shell_keyboard.visible = target != 0 && ctx.kernel.modal.is_none();
    if target != 0 {
        ctx.kernel.shell_keyboard.target = target;
    }
    if was_visible != ctx.kernel.shell_keyboard.visible {
        if ctx.kernel.shell_keyboard.sip {
            notify(ctx);
        }
        ctx.kernel.framebuffer.mark_dirty();
    }
}

pub(crate) fn message(
    ctx: &mut CallCtx<'_>,
    msg: u32,
    key: u32,
    lp: u32,
) -> Result<DispatchOutcome, KernelError> {
    let result = match msg {
        0x4a => {
            let raw = ctx.cpu.read_mem(lp, 12)?;
            let size = u32::from_le_bytes(raw[4..8].try_into().unwrap());
            let ptr = u32::from_le_bytes(raw[8..12].try_into().unwrap());
            if !(3..=256).contains(&size) || ptr == 0 {
                return Ok(DispatchOutcome::ReturnedR0(0));
            }
            let data = ctx.cpu.read_mem(ptr, size)?;
            let command = u16::from_le_bytes([data[0], data[1]]);
            log::debug!("GMSHELL COPYDATA command={command} bytes={data:?}");
            match command {
                0x90 | 0x190 => {
                    if data[2] != 0 {
                        ctx.kernel.shell_keyboard.requested = true;
                        service(ctx);
                        1
                    } else {
                        ctx.kernel.shell_keyboard.visible = false;
                        ctx.kernel.shell_keyboard.requested = false;
                        1
                    }
                }
                _ => 0,
            }
        }
        0x101 => 1,
        0x100 => {
            if !ctx.kernel.shell_keyboard.visible {
                if open(ctx) {
                    ctx.kernel.shell_keyboard.requested = true;
                    return Ok(DispatchOutcome::ReturnedR0(1));
                }
                return Ok(DispatchOutcome::ReturnedR0(0));
            }
            let focused = edit_target(ctx);
            if focused != 0 {
                ctx.kernel.shell_keyboard.target = focused;
            }
            let target = ctx.kernel.shell_keyboard.target;
            match key {
                0x09 => {
                    if let Some(control) = ctx.kernel.controls.get(target) {
                        let edits: Vec<_> = ctx
                            .kernel
                            .controls
                            .children_of(control.parent)
                            .into_iter()
                            .filter(|h| {
                                ctx.kernel.controls.get(*h).is_some_and(|c| {
                                    c.class == ControlClass::Edit
                                        && c.enabled
                                        && crate::coredll::window_is_visible(ctx.kernel, *h)
                                })
                            })
                            .collect();
                        if let Some(i) = edits.iter().position(|h| *h == target) {
                            let next = edits[(i + 1) % edits.len()];
                            ctx.kernel.controls.focus = next;
                            ctx.kernel.shell_keyboard.target = next;
                        }
                    }
                }
                0x25..=0x28 => ctx.kernel.shell_keyboard.navigate(key),
                0xd => {
                    let ch = ctx.kernel.shell_keyboard.character();
                    post(ctx, target, 0x102, ch, 1);
                }
                0x11 | 8 => post(ctx, target, 0x102, 8, 1),
                0x20 | 0x1b => {
                    ctx.kernel.shell_keyboard.visible = false;
                    ctx.kernel.shell_keyboard.requested = false;
                    // Returning from the shell's input panel to the application
                    // re-enters its pointer-input mode before the Done key.
                    let active = ctx
                        .kernel
                        .controls
                        .get(target)
                        .map(|c| c.parent)
                        .unwrap_or(ctx.kernel.active_window);
                    post(ctx, active, 0x200, 0, 0);
                }
                0x30..=0x39 | 0x41..=0x5a | 0xbd | 0xbe => post(
                    ctx,
                    target,
                    0x102,
                    match key {
                        0xbd => 45,
                        0xbe => 46,
                        _ => key,
                    },
                    1,
                ),
                _ => {}
            }
            1
        }
        _ => 0,
    };
    Ok(DispatchOutcome::ReturnedR0(result))
}

/// SIP shares the shell panel, but does not alter the Gizmondo show protocol.
fn notify(ctx: &mut CallCtx<'_>) {
    let owner = ctx
        .kernel
        .modal_dialog
        .as_ref()
        .map(|d| d.hwnd)
        .unwrap_or(ctx.kernel.active_window);
    post(ctx, owner, 0x1a, 0xe0, 0);
}
pub(crate) fn sip_show(ctx: &mut CallCtx<'_>, show: bool) {
    let was_visible = ctx.kernel.shell_keyboard.visible;
    if !show && was_visible {
        notify(ctx);
    }
    ctx.kernel.shell_keyboard.sip = show;
    ctx.kernel.shell_keyboard.requested = show;
    ctx.kernel.shell_keyboard.visible = false;
    ctx.kernel.framebuffer.mark_dirty();
    if show {
        service(ctx);
    }
}
pub(crate) fn input(ctx: &mut CallCtx<'_>, ev: pocket_kernel::InputEvent) -> bool {
    use pocket_kernel::InputEvent;
    if let InputEvent::KeyboardRotation { turns } = ev {
        ctx.kernel.shell_keyboard.rotation = turns % 4;
        ctx.kernel.framebuffer.mark_dirty();
        return true;
    }
    if ctx.kernel.gapi_action_key != pocket_kernel::gapi::VK_RETURN {
        return false;
    }
    if let InputEvent::PointerDown { x, y } = ev {
        if ctx.kernel.shell_keyboard.visible && ctx.kernel.shell_keyboard.sip {
            let (w, h) = (
                ctx.kernel.framebuffer.width as i32,
                ctx.kernel.framebuffer.height as i32,
            );
            let (x, y, w, h) = ctx
                .kernel
                .shell_keyboard
                .display_point(x.into(), y.into(), w, h);
            let lowercase = ctx.kernel.shell_keyboard.lowercase;
            if let Some(ch) = ctx.kernel.shell_keyboard.tap(x, y, w, h) {
                if ctx.kernel.shell_keyboard.lowercase != lowercase {
                    ctx.kernel.framebuffer.mark_dirty();
                }
                ctx.kernel.shell_keyboard.pointer_captured = true;
                if ch == 0 {
                    sip_show(ctx, false);
                } else if ch != u32::MAX {
                    post(ctx, ctx.kernel.shell_keyboard.target, 0x102, ch, 1);
                }
                return true;
            }
        }
        // Let the ordinary control path establish focus and notify its parent.
        // The next pump resolves the SIP target after the guest has handled the tap.
        if let Some(hwnd) = ctx
            .kernel
            .controls
            .hit_test(x.into(), y.into())
            .filter(|h| {
                ctx.kernel.controls.get(*h).is_some_and(|c| {
                    c.class == ControlClass::Edit && c.enabled && c.style & 0x800 == 0
                })
            })
        {
            ctx.kernel.shell_keyboard.target = hwnd;
            ctx.kernel.shell_keyboard.sip = true;
            ctx.kernel.shell_keyboard.requested = true;
            service(ctx);
        }
    }
    if matches!(
        ev,
        InputEvent::PointerUp { .. } | InputEvent::PointerMove { .. }
    ) && ctx.kernel.shell_keyboard.pointer_captured
    {
        if matches!(ev, InputEvent::PointerUp { .. }) {
            ctx.kernel.shell_keyboard.pointer_captured = false;
        }
        return true;
    }
    false
}

pub(crate) fn sip_info(ctx: &mut CallCtx<'_>, p: u32) -> Result<DispatchOutcome, KernelError> {
    if p == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let cb = ctx.cpu.read_u32_le(p)?;
    if cb < 48 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let w = ctx.kernel.framebuffer.width;
    let h = ctx.kernel.framebuffer.height;
    let on = ctx.kernel.shell_keyboard.visible;
    let height = pocket_kernel::shell_keyboard::ShellKeyboard::panel_height(if ctx
        .kernel
        .shell_keyboard
        .rotation
        % 2
        == 0
    {
        h
    } else {
        w
    } as i32) as u32;
    let (desktop, panel) = if !on {
        ([0, 0, w, h], [0, 0, 0, 0])
    } else {
        match ctx.kernel.shell_keyboard.rotation % 4 {
            1 => ([0, 0, w - height, h], [w - height, 0, w, h]),
            2 => ([0, height, w, h], [0, 0, w, height]),
            3 => ([height, 0, w, h], [0, 0, height, h]),
            _ => ([0, 0, w, h - height], [0, h - height, w, h]),
        }
    };
    let mut bytes = [0u8; 48];
    bytes[0..4].copy_from_slice(&cb.to_le_bytes());
    bytes[4..8].copy_from_slice(&(if on { 3u32 } else { 0 }).to_le_bytes());
    for (i, value) in desktop.into_iter().chain(panel).enumerate() {
        bytes[8 + i * 4..12 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    ctx.cpu.write_mem(p, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(crate) fn sip_set_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    set_info(ctx, p)
}
pub(crate) fn set_info(ctx: &mut CallCtx<'_>, p: u32) -> Result<DispatchOutcome, KernelError> {
    if p == 0 || ctx.cpu.read_u32_le(p)? < 48 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let show = ctx.cpu.read_u32_le(p + 4)? & 1 != 0;
    sip_show(ctx, show);
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(crate) fn preference(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pref = ctx.arg_u32(1)?;
    match pref {
        0 => sip_show(ctx, true),
        1 | 2 => sip_show(ctx, false),
        _ => return Ok(DispatchOutcome::ReturnedR0(0)),
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

pub(crate) fn accepts_text(ctx: &CallCtx<'_>) -> bool {
    ctx.kernel.gapi_action_key == pocket_kernel::gapi::VK_RETURN
        && ((ctx.kernel.shell_keyboard.sip && ctx.kernel.shell_keyboard.visible && edit_target(ctx) != 0)
            || ctx.kernel.controls.get(edit_target(ctx)).is_some_and(|c| {
                c.class == ControlClass::Edit && c.enabled && c.style & 0x800 == 0
                    && crate::coredll::window_is_visible(ctx.kernel, c.hwnd)
            }))
}

#[cfg(test)]
mod sip_tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn explicit_sip_toggles_on_custom_windows_but_not_on_gizmondo() {
        let (mut cpu, mut k, mut d) = setup();
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        k.active_window = 0x1234;
        k.window_classes.insert(0x1234, "CustomGameWindow".into());
        k.window_visibility.insert(0x1234, true);
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "SipShowIM", &[1]);
        assert!(k.shell_keyboard.visible);
        assert_eq!(k.shell_keyboard.target, 0x1234);
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "SipShowIM", &[0]);
        assert!(!k.shell_keyboard.visible);
        assert!(!k.shell_keyboard.requested);
        k.gapi_action_key = 0x73;
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "SipShowIM", &[1]);
        assert!(!k.shell_keyboard.visible);
    }
    #[test]
    fn sip_presentation_never_changes_guest_pixels_and_hides_without_residue() {
        let (mut cpu, mut k, mut d) = setup();
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        let edit = k.controls.create(
            0,
            ControlClass::Edit,
            1,
            String::new(),
            0x10000000,
            10,
            20,
            100,
            25,
        );
        k.controls.focus = edit;
        let raw = k.framebuffer.pixels.clone();
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "SipShowIM", &[1]);
        let counter = k.framebuffer.frame_counter;
        assert_ne!(k.presentation_framebuffer().pixels, raw);
        assert_eq!(k.framebuffer.pixels, raw);
        assert_eq!(k.framebuffer.frame_counter, counter);
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "SipShowIM", &[0]);
        assert_eq!(k.presentation_framebuffer().pixels, raw);
    }
    fn pointer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
        let x = ctx.arg_u32(0)? as u16;
        let y = ctx.arg_u32(1)? as u16;
        let up = ctx.arg_u32(2)? != 0;
        let ev = if up {
            pocket_kernel::InputEvent::PointerUp { x, y }
        } else {
            pocket_kernel::InputEvent::PointerDown { x, y }
        };
        Ok(DispatchOutcome::ReturnedR0(u32::from(input(ctx, ev))))
    }
    #[test]
    fn sip_taps_deliver_text_and_capture_the_release_even_after_done() {
        let (mut cpu, mut k, mut d) = setup();
        d.register_handler("coredll.dll", "test_pointer", pointer);
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        let edit = k.controls.create(
            0,
            ControlClass::Edit,
            1,
            String::new(),
            0x10000000,
            10,
            20,
            100,
            25,
        );
        k.controls.focus = edit;
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "SipShowIM", &[1]);
        let h = k.framebuffer.height;
        let w = k.framebuffer.width;
        let top = h - pocket_kernel::shell_keyboard::ShellKeyboard::panel_height(h as i32) as u32;
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "test_pointer",
            &[10, top + 10, 0],
        );
        assert!(k
            .posted_messages
            .iter()
            .any(|m| *m == (edit, 0x102, 'A' as u32, 1)));
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "test_pointer",
                &[10, top + 10, 1]
            ),
            DispatchOutcome::ReturnedR0(1)
        ));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "test_pointer",
            &[w * 3 / 4, h - 10, 0],
        );
        assert!(!k.shell_keyboard.visible);
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "test_pointer",
                &[w * 3 / 4, h - 10, 1]
            ),
            DispatchOutcome::ReturnedR0(1)
        ));
    }
    #[test]
    fn gizmondo_edit_taps_do_not_request_the_pocket_pc_panel() {
        let (mut cpu, mut k, mut d) = setup();
        d.register_handler("coredll.dll", "test_pointer", pointer);
        k.controls.create(
            0,
            ControlClass::Edit,
            1,
            String::new(),
            0x10000000,
            10,
            20,
            100,
            25,
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "test_pointer",
            &[20, 25, 0],
        );
        assert!(!k.shell_keyboard.visible);
        assert!(!k.shell_keyboard.requested);
    }
    #[test]
    fn sip_state_roundtrip_and_short_buffer_preservation() {
        let (mut cpu, mut k, mut d) = setup();
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        let edit = k.controls.create(
            0,
            ControlClass::Edit,
            1,
            String::new(),
            0x10000000,
            10,
            20,
            100,
            25,
        );
        k.controls.focus = edit;
        assert!(matches!(
            call(&mut cpu, &mut k, &mut d, "coredll.dll", "SipShowIM", &[1]),
            DispatchOutcome::ReturnedR0(1)
        ));
        assert!(k.shell_keyboard.visible);
        cpu.write_mem(0x1100, &48u32.to_le_bytes()).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "aygshell.dll",
            "SHSipInfo",
            &[0xe1, 0, 0x1100, 0],
        );
        assert_eq!(cpu.read_u32_le(0x1104).unwrap(), 3);
        cpu.write_mem(0x1100, &4u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1104, &0x12345678u32.to_le_bytes()).unwrap();
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SipGetInfo",
                &[0x1100]
            ),
            DispatchOutcome::ReturnedR0(0)
        ));
        assert_eq!(cpu.read_u32_le(0x1104).unwrap(), 0x12345678);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "aygshell.dll",
            "SHSipPreference",
            &[edit, 2],
        );
        assert!(!k.shell_keyboard.visible);
        assert!(!k.shell_keyboard.requested);
    }
    #[test]
    fn sip_set_legacy_action_and_no_edit_do_not_fabricate_target() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.write_mem(0x1100, &48u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1104, &1u32.to_le_bytes()).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "aygshell.dll",
            "SHSipInfo",
            &[0x100e0, 0, 0x1100, 0],
        );
        assert!(k.shell_keyboard.requested);
        assert!(!k.shell_keyboard.visible);
        assert_eq!(k.shell_keyboard.target, 0);
    }
    #[test]
    fn sip_case_button_redraws_without_typing_and_lowercase_reaches_the_edit() {
        let (mut cpu, mut k, mut d) = setup();
        d.register_handler("coredll.dll", "test_pointer", pointer);
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        let edit = k.controls.create(
            0,
            ControlClass::Edit,
            1,
            String::new(),
            0x10000000,
            10,
            20,
            100,
            25,
        );
        k.controls.focus = edit;
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "SipShowIM", &[1]);
        k.posted_messages.clear();
        let h = k.framebuffer.height;
        let count = k.framebuffer.frame_counter;
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "test_pointer",
            &[10, h - 10, 0],
        );
        assert!(k.shell_keyboard.lowercase);
        assert!(k.shell_keyboard.visible);
        assert!(k.posted_messages.is_empty());
        assert!(k.framebuffer.frame_counter > count);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "test_pointer",
            &[10, h - 10, 1],
        );
        let top = h - pocket_kernel::shell_keyboard::ShellKeyboard::panel_height(h as i32) as u32;
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "test_pointer",
            &[10, top + 10, 0],
        );
        assert_eq!(
            k.posted_messages
                .iter()
                .filter(|m| m.1 == 0x102)
                .copied()
                .collect::<Vec<_>>(),
            vec![(edit, 0x102, 'a' as u32, 1)]
        );
    }
}
