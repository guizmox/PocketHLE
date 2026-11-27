//! Native CE3 setting/display notifications share USER, SIP and descendant routing.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
const SHELL: &str = "__PocketHLE_MFC_ShellSettingChange";
const BASE: &str = "__PocketHLE_MFC_WindowSettingChange";
const DISPLAY: &str = "__PocketHLE_MFC_WindowDisplayChange";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2018, "?OnSettingChange@CWnd@@IAAXIPBG@Z", window),
    (1874, "?OnDisplayChange@CWnd@@IAAJIJ@Z", display),
    (2796, "?OnSettingChange@CDialog@@IAAXIPBG@Z", dialog),
    (2797, "?OnSettingChange@CFrameWnd@@IAAXIPBG@Z", frame),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", SHELL, crate::aygshell::settings::handle);
    d.register_handler("coredll.dll", BASE, window);
    d.register_handler("coredll.dll", DISPLAY, display);
}
fn base(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let args = [f.arg(ctx, 0)?, f.arg(ctx, 1)?, f.arg(ctx, 2)?];
    f.call_service(ctx, 3, BASE, &args)
}
fn shell(ctx: &mut CallCtx<'_>, f: &Frame, offset: u32) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let thread = application::current_thread_state(ctx)?;
    let wp = word(ctx, thread + 0x44)?;
    let lp = word(ctx, thread + 0x48)?;
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 2, SHELL, &[hwnd, wp, lp, object + offset])
}
fn dialog(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    derived(ctx, true)
}
fn frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    derived(ctx, false)
}
fn derived(ctx: &mut CallCtx<'_>, is_dialog: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        return f.call_service(
            ctx,
            1,
            if is_dialog {
                activation::STYLE
            } else {
                hierarchy::TOP
            },
            &[object],
        );
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 if is_dialog && result & 0x40000000 == 0 => shell(ctx, &f, 0x50),
        1 if !is_dialog && result == f.arg(ctx, 0)? => shell(ctx, &f, 0xcc),
        1 | 2 => base(ctx, &f),
        3 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC setting-change continuation".into(),
        )),
    }
}
fn window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        return f.call_service(ctx, 1, DISPLAY, &[object, 0, 0]);
    }
    f.finish(ctx, 0)
}
fn display(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        // Native AUX_DATA caches are not duplicated in HLE: USER/GDI query
        // current metrics directly. Preserve the actual native broadcast and
        // Default path, including the thread's original WM_SETTINGCHANGE MSG.
        return f.call_service(ctx, 1, activation::STYLE, &[object]);
    }
    match f.phase(ctx)? {
        1 => {
            let style = ctx.cpu.read_reg(ArmReg::R0)?;
            if style & 0x40000000 == 0 {
                let thread = application::current_thread_state(ctx)?;
                let args = [
                    word(ctx, object + 0x20)?,
                    word(ctx, thread + 0x40)?,
                    word(ctx, thread + 0x44)?,
                    word(ctx, thread + 0x48)?,
                    1,
                    1,
                ];
                return f.call_service(ctx, 2, descendants::TARGET, &args);
            }
            f.call_service(ctx, 3, "__PocketHLE_MFC_WindowDefault", &[object])
        }
        2 => f.call_service(ctx, 3, "__PocketHLE_MFC_WindowDefault", &[object]),
        3 => {
            let result = ctx.cpu.read_reg(ArmReg::R0)?;
            f.finish(ctx, result)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC display-change continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn dialog_setting_changes_follow_live_sip_geometry_preserve_msg_and_release_frames() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:79",
            &[0x2000],
        );
        let thread = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:651",
            &[],
        ));
        let hwnd = 0xdead0001u32;
        cpu.write_mem(0x2020, &hwnd.to_le_bytes()).unwrap();
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        k.framebuffer = pocket_kernel::framebuffer::deferred::DeferredFramebuffer::new(
            pocket_kernel::Framebuffer::new(240, 320),
        )
        .unwrap();
        k.window_classes.insert(hwnd, "Frame".into());
        k.window_styles.insert(hwnd, (0x10000000, 0));
        k.window_visibility.insert(hwnd, true);
        k.window_rects.insert(hwnd, (0, 26, 240, 294));
        k.active_window = hwnd;
        let msg: Vec<_> = [hwnd, 0x1a, 0xe0, 0, 123, 4, 5]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        cpu.write_mem(thread + 0x3c, &msg).unwrap();
        cpu.write_mem(0x2058, &0x80000006u32.to_le_bytes()).unwrap();
        let mut baseline = None;
        for cycle in 0..16 {
            for up in [true, false] {
                k.shell_keyboard.visible = up;
                k.shell_keyboard.sip = true;
                cpu.write_arg(0, 0x2000).unwrap();
                cpu.write_arg(1, 0xe0).unwrap();
                cpu.write_arg(2, 0).unwrap();
                assert_eq!(
                    drive(&mut cpu, &mut k, &mut d, entry(2796), |pc, _| panic!(
                        "unexpected callback {pc:x}"
                    )),
                    0
                );
                assert_eq!(
                    k.window_rects[&hwnd],
                    (0, 26, 240, if up { 188 } else { 294 })
                );
                assert_eq!(cpu.read_u32_le(0x2058).unwrap(), 0x80000006 | u32::from(up));
                assert_eq!(cpu.read_mem(thread + 0x3c, 28).unwrap(), msg);
            }
            if cycle == 0 {
                baseline = Some(k.heap.free_bytes());
            }
            assert_eq!(Some(k.heap.free_bytes()), baseline);
        }
    }
}

#[cfg(test)]
mod splitter_notification_tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use pocket_cpu::Cpu;
    #[test]
    fn splitter_display_notification_only_recalculates_a_live_visible_window() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2800 + 47 * 4, &0x6200u32.to_le_bytes())
            .unwrap();
        let baseline = k.heap.free_bytes();
        for (exists, visible) in [(true, true), (true, false), (false, true)] {
            if exists {
                k.window_classes.insert(7, "Splitter".into());
            } else {
                k.window_classes.remove(&7);
            }
            k.window_styles.insert(7, (0x10000000, 0));
            k.window_visibility.insert(7, visible);
            cpu.write_arg(0, 0x2000).unwrap();
            let mut calls = 0;
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(1873), |pc, cpu| {
                    calls += 1;
                    assert_eq!(pc, 0x6200);
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                    0
                }),
                0
            );
            assert_eq!(calls, usize::from(exists && visible));
            assert_eq!(k.heap.free_bytes(), baseline);
        }
    }
}
