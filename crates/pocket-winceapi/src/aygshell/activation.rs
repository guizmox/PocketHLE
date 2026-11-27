//! Restore the caller's SIP/focus state through the existing shell services.
use crate::{CallCtx, WinCeDispatcher};
use pocket_kernel::{DispatchOutcome, KernelError};
pub(super) fn register(d: &mut WinCeDispatcher) {
    for name in ["SHHandleWMActivate", "ord:84", "#84"] {
        d.register_handler("aygshell.dll", name, handle);
    }
}
pub(crate) fn handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let state = ctx.arg_u32(1)? & 0xffff;
    let info = ctx.arg_u32(3)?;
    let flags = ctx.arg_u32(4)?;
    if flags != 0 {
        return Err(KernelError::Dispatch(format!(
            "Shell activation flags 0x{flags:x} are not implemented"
        )));
    }
    if !crate::coredll::window_exists(ctx.kernel, hwnd) {
        crate::coredll::set_thread_error(ctx, 1400);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    ctx.cpu
        .check_guest_access(info, 12, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let bytes = ctx.cpu.read_mem(info, 12)?;
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if word(0) != 12 || state > 2 {
        crate::coredll::set_thread_error(ctx, 87);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let mut bits = word(8);
    if state == 0 {
        let focus = ctx.kernel.controls.focus;
        let focus = if crate::coredll::focus_belongs_to_window(ctx.kernel, focus, hwnd) {
            focus
        } else {
            0
        };
        ctx.cpu.write_mem(info + 4, &focus.to_le_bytes())?;
        let up = u32::from(ctx.kernel.shell_keyboard.visible);
        bits = (bits & !7) | up | (up << 1);
    } else {
        crate::shell_keyboard::sip_show(ctx, bits & 1 != 0);
        let focus = word(4);
        if focus != 0 && crate::coredll::focus_belongs_to_window(ctx.kernel, focus, hwnd) {
            crate::coredll::set_focus_value(ctx, focus)?;
        }
        bits |= 4;
    }
    ctx.cpu.write_mem(info + 8, &bits.to_le_bytes())?;
    crate::coredll::set_thread_error(ctx, 0);
    Ok(DispatchOutcome::ReturnedR0(1))
}

#[cfg(test)]
mod tests {
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    use pocket_kernel::{controls::ControlClass, DispatchOutcome};
    #[test]
    fn enabled_queries_respect_control_state_and_top_level_disabled_style() {
        let (mut cpu, mut k, mut d) = setup();
        let button = k.controls.create(
            7,
            ControlClass::Button,
            1,
            String::new(),
            0x58000000,
            0,
            0,
            40,
            20,
        );
        k.window_classes.insert(7, "Frame".into());
        k.window_styles.insert(7, (0x98000000, 0));
        for hwnd in [button, 7] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "IsWindowEnabled",
                    &[hwnd]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
        }
        k.controls.get_mut(button).unwrap().enabled = true;
        k.window_styles.get_mut(&7).unwrap().0 &= !0x08000000;
        for hwnd in [button, 7] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "IsWindowEnabled",
                    &[hwnd]
                ),
                DispatchOutcome::ReturnedR0(1)
            );
        }
    }
    #[test]
    fn activation_restores_saved_sip_and_descendant_focus_preserving_reserved_bits() {
        let (mut cpu, mut k, mut d) = setup();
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        k.window_classes.insert(7, "Frame".into());
        k.window_visibility.insert(7, true);
        let edit = k.controls.create(
            7,
            ControlClass::Edit,
            1,
            String::new(),
            0x50000000,
            0,
            0,
            40,
            20,
        );
        k.controls.focus = edit;
        k.shell_keyboard.visible = true;
        for (at, value) in [(0x1800, 12u32), (0x1804, 0), (0x1808, 0x80000004)] {
            cpu.write_mem(at, &value.to_le_bytes()).unwrap();
        }
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "SHHandleWMActivate",
                &[7, 0, 0, 0x1800, 0]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_u32_le(0x1804).unwrap(), edit);
        assert_eq!(cpu.read_u32_le(0x1808).unwrap(), 0x80000003);
        k.controls.focus = 0;
        k.shell_keyboard.visible = false;
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "ord:84",
                &[7, 1, 0, 0x1800, 0]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(k.controls.focus, edit);
        assert!(k.shell_keyboard.visible);
        assert_eq!(cpu.read_u32_le(0x1808).unwrap(), 0x80000007);
        // A still-live control belonging to another window must not receive focus.
        k.controls.get_mut(edit).unwrap().parent = 8;
        k.controls.focus = 0;
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "aygshell.dll",
            "#84",
            &[7, 2, 0, 0x1800, 0],
        );
        assert_eq!(k.controls.focus, 0);
    }
    #[test]
    fn invalid_window_and_info_size_fail_without_modifying_caller_state() {
        let (mut cpu, mut k, mut d) = setup();
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        k.window_classes.insert(7, "Frame".into());
        k.window_visibility.insert(7, true);
        cpu.write_mem(0x1800, &[0xab; 12]).unwrap();
        for (hwnd, error) in [(99, 1400), (7, 87)] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "aygshell.dll",
                    "SHHandleWMActivate",
                    &[hwnd, 1, 0, 0x1800, 0]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
            assert_eq!(k.thread_last_errors[&k.current_thread], error);
            assert_eq!(cpu.read_mem(0x1800, 12).unwrap(), [0xab; 12]);
        }
    }
}
