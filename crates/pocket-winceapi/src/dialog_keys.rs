//! WinCE dialog navigation. Gizmondo Play/Soft1/Soft2 already emit
//! Return/Tab/Escape; preserve those shared bindings and consult WM_GETDLGCODE.
use crate::CallCtx;
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{
    controls::ControlClass, DialogKeyFrame, DispatchOutcome, GuestCallFrame, KernelError,
    KernelState,
};

fn parent(kernel: &KernelState, hwnd: u32) -> Option<u32> {
    kernel
        .controls
        .get(hwnd)
        .map(|c| c.parent)
        .or_else(|| kernel.window_parents.get(&hwnd).copied())
}
fn belongs(kernel: &KernelState, mut hwnd: u32, dialog: u32) -> bool {
    for _ in 0..64 {
        if hwnd == dialog {
            return true;
        }
        let Some(next) = parent(kernel, hwnd) else {
            return false;
        };
        if next == hwnd {
            return false;
        }
        hwnd = next;
    }
    false
}
/// Shared tab traversal uses the same real child/sibling order as GetWindow.
/// WS_EX_CONTROLPARENT containers contribute their nested controls in place.
fn next_tab(
    kernel: &KernelState,
    dialog: u32,
    current: u32,
    previous: bool,
) -> Result<u32, KernelError> {
    let mut order = Vec::new();
    let mut pending = vec![crate::coredll::related_window(kernel, dialog, 5)];
    let mut seen = std::collections::HashSet::new();
    while let Some(hwnd) = pending.pop() {
        if hwnd == 0 {
            continue;
        }
        if !seen.insert(hwnd) || seen.len() > 4096 {
            return Err(KernelError::Dispatch(
                "Dialog tab traversal is cyclic or exceeds its bound".into(),
            ));
        }
        pending.push(crate::coredll::related_window(kernel, hwnd, 2));
        let style = kernel
            .controls
            .get(hwnd)
            .map(|c| c.style)
            .or_else(|| kernel.window_styles.get(&hwnd).map(|s| s.0))
            .unwrap_or(0);
        let ex_style = kernel.window_styles.get(&hwnd).map(|s| s.1).unwrap_or(0);
        order.push((
            hwnd,
            style & 0x10000 != 0
                && crate::coredll::window_is_visible(kernel, hwnd)
                && crate::window_enable::input_enabled(kernel, hwnd),
        ));
        if ex_style & 0x10000 != 0 {
            pending.push(crate::coredll::related_window(kernel, hwnd, 5));
        }
    }
    if order.is_empty() {
        return Ok(0);
    }
    let start = order.iter().position(|(h, _)| *h == current);
    if current != 0 && start.is_none() {
        return Ok(0);
    }
    for step in 1..=order.len() {
        let index = match start {
            Some(i) if previous => (i + order.len() - step % order.len()) % order.len(),
            Some(i) => (i + step) % order.len(),
            None if previous => order.len() - step,
            None => step - 1,
        };
        if order[index].1 {
            return Ok(order[index].0);
        }
    }
    Ok(current)
}
pub(crate) fn get_next_tab_item(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dialog = ctx.arg_u32(0)?;
    let current = ctx.arg_u32(1)?;
    let previous = ctx.arg_u32(2)? != 0;
    if !crate::coredll::window_exists(ctx.kernel, dialog)
        || (current != 0 && !belongs(ctx.kernel, current, dialog))
    {
        crate::coredll::set_thread_error(ctx, 1400);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let next = next_tab(ctx.kernel, dialog, current, previous)?;
    Ok(DispatchOutcome::ReturnedR0(next))
}

fn restore(ctx: &mut CallCtx<'_>, frame: GuestCallFrame) -> Result<(), KernelError> {
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(frame.args)
    {
        ctx.cpu.write_reg(r, v)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, frame.lr)?;
    Ok(())
}
fn invoke(
    ctx: &mut CallCtx<'_>,
    state: DialogKeyFrame,
    proc: u32,
    args: [u32; 4],
) -> Result<DispatchOutcome, KernelError> {
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(args)
    {
        ctx.cpu.write_reg(r, v)?;
    }
    ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
    ctx.kernel.dialog_key_frames.push(state);
    Ok(DispatchOutcome::JumpTo(proc))
}
fn wants_key(code: u32, key: u32) -> bool {
    code & 4 != 0 || (key == 9 && code & 2 != 0)
}
fn command_target(kernel: &KernelState, dialog: u32, key: u32) -> (u32, u32) {
    if key == 0x1b {
        return (2, 0);
    }
    let focus = kernel.controls.get(kernel.controls.focus);
    if let Some(c) = focus.filter(|c| {
        c.parent == dialog
            && c.class == ControlClass::Button
            && c.enabled
            && c.visible
            && c.style & 0xf <= 1
    }) {
        return (c.id, c.hwnd);
    }
    let default_id = kernel.window_extra.get(&(dialog, -1200)).copied();
    let button = kernel
        .controls
        .children_of(dialog)
        .into_iter()
        .filter_map(|h| kernel.controls.get(h))
        .find(|c| {
            c.class == ControlClass::Button
                && c.visible
                && c.enabled
                && default_id.map_or(c.style & 0xf == 1, |id| c.id == id)
        });
    button
        .map(|c| (c.id, c.hwnd))
        .unwrap_or((default_id.unwrap_or(1), 0))
}
fn process(
    ctx: &mut CallCtx<'_>,
    mut state: DialogKeyFrame,
    code: u32,
) -> Result<DispatchOutcome, KernelError> {
    log::debug!(
        "IsDialogMessage dialog=0x{:08x} target=0x{:08x} key=0x{:x} dlgcode=0x{:x}",
        state.dialog,
        state.target,
        state.key,
        code
    );
    if wants_key(code, state.key) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if state.key == 9 {
        let shift = ctx.kernel.pressed_keys[0x10]
            || ctx.kernel.pressed_keys[0xa0]
            || ctx.kernel.pressed_keys[0xa1];
        let next = next_tab(ctx.kernel, state.dialog, ctx.kernel.controls.focus, shift)?;
        let next = if next == 0 {
            next_tab(ctx.kernel, state.dialog, 0, shift)?
        } else {
            next
        };
        if next != 0 {
            ctx.kernel.controls.focus = next;
        }
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let (id, button) = command_target(ctx.kernel, state.dialog, state.key);
    log::debug!("dialog key command id={} button=0x{:08x}", id, button);
    let proc = ctx
        .kernel
        .window_procs
        .get(&state.dialog)
        .copied()
        .unwrap_or(0);
    if proc == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    state.stage = 1;
    invoke(ctx, state, proc, [state.dialog, 0x111, id, button])
}
pub(crate) fn is_dialog_message(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if ctx.kernel.dialog_key_frames.last().is_some_and(|s| {
        s.thread == ctx.kernel.current_thread && s.thunk == ctx.thunk.thunk_va && s.frame.sp == sp
    }) {
        let code = ctx.cpu.read_reg(ArmReg::R0)?;
        let state = ctx.kernel.dialog_key_frames.pop().unwrap();
        restore(ctx, state.frame)?;
        return if state.stage == 1 {
            Ok(DispatchOutcome::ReturnedR0(1))
        } else {
            process(ctx, state, code)
        };
    }
    let dialog = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    if dialog == 0 || ptr == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let msg = ctx.cpu.read_mem(ptr, 16)?;
    let field = |i| u32::from_le_bytes(msg[i..i + 4].try_into().unwrap());
    let target = field(0);
    let key = field(8);
    if field(4) != 0x100 || !matches!(key, 9 | 0xd | 0x1b) || !belongs(ctx.kernel, target, dialog) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let frame = GuestCallFrame {
        args: [dialog, ptr, ctx.arg_u32(2)?, ctx.arg_u32(3)?],
        sp,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    let state = DialogKeyFrame {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame,
        dialog,
        target,
        key,
        stage: 0,
    };
    if let Some(proc) = ctx
        .kernel
        .window_procs
        .get(&target)
        .copied()
        .filter(|p| *p != 0)
    {
        return invoke(ctx, state, proc, [target, 0x87, key, ptr]);
    }
    let code = ctx.kernel.controls.get(target).map_or(0, |c| {
        if c.class == ControlClass::Edit && c.style & 0x1004 == 0x1004 {
            4
        } else {
            0
        }
    });
    process(ctx, state, code)
}
#[cfg(test)]
mod tests {
    use super::wants_key;
    #[test]
    fn dialog_respects_control_key_requests() {
        assert!(wants_key(4, 0xd));
        assert!(wants_key(4, 0x1b));
        assert!(wants_key(2, 9));
        assert!(!wants_key(2, 0xd));
        assert!(!wants_key(0x81, 0xd));
    }
}

#[cfg(test)]
mod tab_tests {
    use super::*;
    #[test]
    fn tab_queries_share_nested_order_skip_disabled_and_do_not_move_focus() {
        let (mut cpu, mut kernel, mut dispatcher) = crate::mfc::tests::framework_setup();
        kernel.window_classes.insert(7, "Dialog".into());
        kernel.window_visibility.insert(7, true);
        kernel.window_styles.insert(7, (0x10000000, 0));
        let a = kernel.controls.create(
            7,
            ControlClass::Button,
            10,
            "A".into(),
            0x50010000,
            0,
            0,
            20,
            20,
        );
        let container = kernel.controls.create(
            7,
            ControlClass::Static,
            11,
            "".into(),
            0x50000000,
            20,
            0,
            20,
            20,
        );
        kernel
            .window_styles
            .insert(container, (0x50000000, 0x10000));
        let b = kernel.controls.create(
            container,
            ControlClass::Edit,
            12,
            "B".into(),
            0x50010000,
            0,
            0,
            20,
            20,
        );
        let c = kernel.controls.create(
            7,
            ControlClass::Button,
            13,
            "C".into(),
            0x58010000,
            40,
            0,
            20,
            20,
        );
        kernel.controls.get_mut(c).unwrap().enabled = false;
        for (current, previous, expected) in [
            (0, false, a),
            (a, false, b),
            (b, false, a),
            (0, true, b),
            (b, true, a),
        ] {
            assert_eq!(next_tab(&kernel, 7, current, previous).unwrap(), expected);
        }
        assert_eq!(kernel.controls.focus, 0);
        use crate::bluetooth::tests::call;
        let outcome = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "GetNextDlgTabItem",
            &[7, a, 0],
        );
        assert!(matches!(outcome, DispatchOutcome::ReturnedR0(value) if value == b));
        kernel.controls.get_mut(a).unwrap().enabled = false;
        kernel.controls.get_mut(b).unwrap().enabled = false;
        assert_eq!(next_tab(&kernel, 7, a, false).unwrap(), a);
    }
}
