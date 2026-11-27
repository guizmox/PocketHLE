//! Window enable transitions share native state, synchronous callbacks and guest frames.
use crate::guest_callbacks::{self, Frame, WINCE};
use crate::{CallCtx, WinCeDispatcher};
use pocket_kernel::{DispatchOutcome, KernelError, KernelState};
const WS_DISABLED: u32 = 0x08000000;
const PREVIOUS: u32 = 0;
const ENABLED: u32 = 1;
const LOST_FOCUS: u32 = 2;
const CAPTURED: u32 = 3;
pub(crate) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "EnableWindow", enable);
}
pub(crate) fn locally_enabled(kernel: &KernelState, hwnd: u32) -> bool {
    if let Some(c) = kernel.controls.get(hwnd) {
        c.enabled
    } else if let Some(&(style, _)) = kernel.window_styles.get(&hwnd) {
        style & WS_DISABLED == 0
    } else {
        crate::coredll::window_exists(kernel, hwnd)
    }
}
pub(crate) fn input_enabled(kernel: &KernelState, mut hwnd: u32) -> bool {
    for _ in 0..128 {
        if hwnd == 0 {
            return true;
        }
        if !locally_enabled(kernel, hwnd) {
            return false;
        }
        if let Some(c) = kernel.controls.get(hwnd) {
            hwnd = c.parent;
        } else if kernel
            .window_styles
            .get(&hwnd)
            .is_some_and(|s| s.0 & 0x40000000 != 0)
        {
            hwnd = kernel.window_parents.get(&hwnd).copied().unwrap_or(0);
        } else {
            return true;
        }
    }
    false
}
fn call(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    phase: u32,
    name: &str,
    args: &[u32],
) -> Result<DispatchOutcome, KernelError> {
    let target = guest_callbacks::service(ctx, name)?;
    f.call_args(ctx, phase, target, args)
}
fn notify(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let hwnd = f.arg(ctx, 0)?;
    let enabled = f.get(ctx, ENABLED)?;
    if !crate::coredll::window_exists(ctx.kernel, hwnd) {
        let previous = f.get(ctx, PREVIOUS)?;
        return f.finish(ctx, previous);
    }
    call(ctx, &f, 3, "SendMessageW", &[hwnd, 0x0a, enabled, 0])
}
fn after_cancel(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let focus = f.get(ctx, LOST_FOCUS)?;
    if focus != 0 && crate::coredll::window_exists(ctx.kernel, focus) {
        return call(ctx, &f, 2, "SendMessageW", &[focus, 8, 0, 0]);
    }
    notify(ctx, f)
}
fn enable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let owner = guest_callbacks::owner(ctx, WINCE.continue_name)?;
    let f = Frame::enter(ctx, WINCE, owner)?;
    let hwnd = f.arg(ctx, 0)?;
    if !f.fresh {
        return match f.phase(ctx)? {
            1 if f.get(ctx, CAPTURED)? != 0 => {
                let capture = f.get(ctx, CAPTURED)?;
                if ctx
                    .kernel
                    .controls
                    .pointer_capture
                    .get(&ctx.kernel.current_thread)
                    == Some(&capture)
                {
                    return call(ctx, &f, 4, "ReleaseCapture", &[]);
                }
                after_cancel(ctx, f)
            }
            1 | 4 => after_cancel(ctx, f),
            2 => notify(ctx, f),
            3 => {
                let previous = f.get(ctx, PREVIOUS)?;
                f.finish(ctx, previous)
            }
            _ => Err(KernelError::Dispatch(
                "Invalid window-enable continuation".into(),
            )),
        };
    }
    if !crate::coredll::window_exists(ctx.kernel, hwnd) {
        crate::coredll::set_thread_error(ctx, 1400);
        return f.finish(ctx, 0);
    }
    if ctx
        .kernel
        .window_threads
        .get(&hwnd)
        .is_some_and(|t| *t != ctx.kernel.current_thread)
    {
        return Err(KernelError::Dispatch(
            "Cross-thread synchronous EnableWindow callbacks are not implemented".into(),
        ));
    }
    let enabled = f.arg(ctx, 1)? != 0;
    let was_enabled = locally_enabled(ctx.kernel, hwnd);
    let previous = u32::from(!was_enabled);
    if enabled == was_enabled {
        return f.finish(ctx, previous);
    }
    f.set(ctx, PREVIOUS, previous)?;
    f.set(ctx, ENABLED, u32::from(enabled))?;
    let style = ctx.kernel.window_styles.entry(hwnd).or_insert((0, 0));
    if enabled {
        style.0 &= !WS_DISABLED;
    } else {
        style.0 |= WS_DISABLED;
    }
    if let Some(control) = ctx.kernel.controls.get_mut(hwnd) {
        control.enabled = enabled;
        if enabled {
            control.style &= !WS_DISABLED;
        } else {
            control.style |= WS_DISABLED;
        }
    }
    if enabled {
        crate::coredll::repaint_controls(ctx);
        return notify(ctx, f);
    }
    let controls: Vec<_> = ctx
        .kernel
        .controls
        .window_handles()
        .filter(|h| crate::coredll::focus_belongs_to_window(ctx.kernel, *h, hwnd))
        .collect();
    for control in controls {
        ctx.kernel.controls.cancel_input(control);
    }
    let capture = ctx
        .kernel
        .controls
        .pointer_capture
        .get(&ctx.kernel.current_thread)
        .copied()
        .unwrap_or(0);
    if capture != 0 && crate::coredll::focus_belongs_to_window(ctx.kernel, capture, hwnd) {
        f.set(ctx, CAPTURED, capture)?;
    }
    let focus = ctx.kernel.controls.focus;
    if focus != 0 && crate::coredll::focus_belongs_to_window(ctx.kernel, focus, hwnd) {
        f.set(ctx, LOST_FOCUS, focus)?;
        crate::coredll::set_focus_value(ctx, 0)?;
    }
    crate::coredll::repaint_controls(ctx);
    call(ctx, &f, 1, "SendMessageW", &[hwnd, 0x1f, 0, 0])
}

#[cfg(test)]
mod tests {
    use crate::mfc::drive;
    use crate::mfc::tests::framework_setup;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::Thunk;
    use pocket_pe::ImportBinding;
    fn entry() -> Thunk {
        Thunk {
            thunk_va: 0x70000000,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: ImportBinding::Name("EnableWindow".into()),
            friendly_name: None,
        }
    }
    #[test]
    fn enable_transitions_send_synchronously_return_previous_disabled_state_and_release_frames() {
        let (mut cpu, mut k, mut d) = framework_setup();
        k.window_classes.insert(7, "Window".into());
        k.window_styles.insert(7, (0, 0));
        k.window_procs.insert(7, 0x6100);
        k.window_threads.insert(7, 0);
        let baseline = k.heap.free_bytes();
        assert_eq!(pocket_kernel::Dispatcher::constant_for(&d, &entry()), None);
        for (enabled, expected, messages) in
            [(0, 0, vec![0x1f, 0xa]), (0, 1, vec![]), (1, 1, vec![0xa])]
        {
            cpu.write_arg(0, 7).unwrap();
            cpu.write_arg(1, enabled).unwrap();
            let mut seen = Vec::new();
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(), |pc, cpu| {
                    assert_eq!(pc, 0x6100);
                    assert_eq!(cpu.read_arg(0).unwrap(), 7);
                    let message = cpu.read_arg(1).unwrap();
                    seen.push(message);
                    if message == 0xa {
                        assert_eq!(cpu.read_arg(2).unwrap(), enabled);
                    }
                    123
                }),
                expected
            );
            assert_eq!(seen, messages);
            assert_eq!(super::locally_enabled(&k, 7), enabled != 0);
            assert_eq!(k.heap.free_bytes(), baseline);
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|key| key.starts_with("__PocketHLE_WinCE_Frame:")));
            assert!(!k.dynamic_exports[&1].contains_key("__PocketHLE_MFC_ModuleState"));
        }
    }
    #[test]
    fn disabled_parent_blocks_child_input_but_keeps_owned_popups_and_local_bits_independent() {
        let (_, mut k, _) = framework_setup();
        k.window_classes.insert(7, "Root".into());
        k.window_styles.insert(7, (0x08000000, 0));
        let child = k.controls.create(
            7,
            pocket_kernel::controls::ControlClass::Button,
            10,
            "Child".into(),
            0x50000000,
            0,
            0,
            40,
            20,
        );
        k.window_classes.insert(8, "Popup".into());
        k.window_styles.insert(8, (0x80000000, 0));
        k.window_parents.insert(8, 7);
        assert!(super::locally_enabled(&k, child));
        assert!(!super::input_enabled(&k, child));
        assert!(super::input_enabled(&k, 8));
        k.window_styles.get_mut(&7).unwrap().0 = 0;
        assert!(super::input_enabled(&k, child));
    }
    #[test]
    fn disabling_a_parent_clears_descendant_focus_and_notifies_focus_loss_before_enable() {
        let (mut cpu, mut k, mut d) = framework_setup();
        k.window_classes.insert(7, "Root".into());
        k.window_styles.insert(7, (0, 0));
        k.window_procs.insert(7, 0x6100);
        let child = k.controls.create(
            7,
            pocket_kernel::controls::ControlClass::Edit,
            10,
            "Text".into(),
            0x50000000,
            0,
            0,
            40,
            20,
        );
        k.window_procs.insert(child, 0x6200);
        k.controls.focus = child;
        cpu.write_reg(ArmReg::R0, 7).unwrap();
        cpu.write_reg(ArmReg::R1, 0).unwrap();
        let mut messages = Vec::new();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(), |pc, cpu| {
                let msg = cpu.read_arg(1).unwrap();
                messages.push((pc, msg));
                0
            }),
            0
        );
        assert_eq!(messages, [(0x6100, 0x1f), (0x6200, 8), (0x6100, 0xa)]);
        assert_eq!(k.controls.focus, 0);
        assert!(k.controls.get(child).unwrap().enabled);
    }
}
