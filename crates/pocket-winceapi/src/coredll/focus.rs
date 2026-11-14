use super::*;
use pocket_kernel::ce::FocusChange;

fn advance(ctx: &mut CallCtx<'_>, mut change: FocusChange) -> Result<DispatchOutcome, KernelError> {
    while let Some((hwnd, message, other)) = change.messages.pop() {
        if message == WM_SETFOCUS && ctx.kernel.controls.focus != hwnd {
            continue;
        }
        let Some(proc) = ctx
            .kernel
            .window_procs
            .get(&hwnd)
            .copied()
            .filter(|proc| *proc != 0)
        else {
            continue;
        };
        ctx.cpu.write_arg(0, hwnd)?;
        ctx.cpu.write_arg(1, message)?;
        ctx.cpu.write_arg(2, other)?;
        ctx.cpu.write_arg(3, 0)?;
        ctx.cpu.write_reg(ArmReg::Lr, change.thunk)?;
        ctx.kernel.ce.focus_changes.push(change);
        return Ok(DispatchOutcome::JumpTo(proc));
    }
    for (i, value) in change.frame.args.into_iter().enumerate() {
        ctx.cpu.write_arg(i as u8, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, change.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, change.frame.lr)?;
    Ok(DispatchOutcome::ReturnedR0(change.previous))
}

pub(super) fn set(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if let Some(index) = ctx.kernel.ce.focus_changes.iter().rposition(|frame| {
        frame.thread == ctx.kernel.current_thread
            && frame.thunk == ctx.thunk.thunk_va
            && frame.frame.sp == sp
    }) {
        let change = ctx.kernel.ce.focus_changes.remove(index);
        return advance(ctx, change);
    }
    let hwnd = ctx.arg_u32(0)?;
    let previous = ctx.kernel.controls.focus;
    let return_value = if previous == 0 { FAKE_HWND } else { previous };
    let valid = hwnd == 0
        || ctx
            .kernel
            .controls
            .get(hwnd)
            .is_some_and(|c| c.is_focusable())
        || (ctx.kernel.window_procs.contains_key(&hwnd)
            && ctx
                .kernel
                .window_styles
                .get(&hwnd)
                .is_none_or(|s| s.0 & 0x0800_0000 == 0));
    if !valid || (hwnd != 0 && window_thread(ctx.kernel, hwnd) != ctx.kernel.current_thread) {
        set_thread_error(ctx, 1400);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if previous == hwnd {
        return Ok(DispatchOutcome::ReturnedR0(return_value));
    }
    let frame = GuestCallFrame {
        args: [
            ctx.arg_u32(0)?,
            ctx.arg_u32(1)?,
            ctx.arg_u32(2)?,
            ctx.arg_u32(3)?,
        ],
        sp,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    ctx.kernel.controls.focus = hwnd;
    remember_window_focus(ctx.kernel);
    crate::shell_keyboard::service(ctx);
    repaint_controls(ctx);
    // RichEdit changes its internal text/caret state in WM_SETFOCUS. Merely
    // recording HWND (the previous implementation) left Pocket Word unable
    // to accept WM_CHAR. Deliver both notifications before SetFocus returns.
    let mut messages = Vec::new();
    if hwnd != 0 {
        messages.push((hwnd, WM_SETFOCUS, previous));
    }
    if previous != 0 {
        messages.push((previous, 8, hwnd));
    }
    advance(
        ctx,
        FocusChange {
            thread: ctx.kernel.current_thread,
            thunk: ctx.thunk.thunk_va,
            frame,
            previous: return_value,
            messages,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_focus_change_sends_notifications_synchronously_and_skips_stale_setfocus() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        for (hwnd, proc) in [(10, 0x2000), (20, 0x2100), (30, 0x2200)] {
            kernel.window_procs.insert(hwnd, proc);
            kernel.window_classes.insert(hwnd, "RichEdit20W".into());
            kernel.window_styles.insert(hwnd, (0x10000000, 0));
            kernel.window_visibility.insert(hwnd, true);
        }
        kernel.controls.focus = 10;
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        ctx.cpu.write_reg(ArmReg::Lr, 0x1235).unwrap();
        ctx.cpu.write_arg(0, 20).unwrap();
        assert_eq!(set(&mut ctx).unwrap(), DispatchOutcome::JumpTo(0x2000));
        assert_eq!(
            [
                ctx.arg_u32(0).unwrap(),
                ctx.arg_u32(1).unwrap(),
                ctx.arg_u32(2).unwrap()
            ],
            [10, 8, 20]
        );
        ctx.cpu.write_reg(ArmReg::Sp, 0x8f00).unwrap();
        ctx.cpu.write_reg(ArmReg::Lr, 0x2004).unwrap();
        ctx.cpu.write_arg(0, 30).unwrap();
        assert_eq!(set(&mut ctx).unwrap(), DispatchOutcome::JumpTo(0x2100));
        assert_eq!(ctx.arg_u32(1).unwrap(), 8);
        ctx.cpu.write_arg(0, 0).unwrap();
        assert_eq!(set(&mut ctx).unwrap(), DispatchOutcome::JumpTo(0x2200));
        assert_eq!(
            [
                ctx.arg_u32(0).unwrap(),
                ctx.arg_u32(1).unwrap(),
                ctx.arg_u32(2).unwrap()
            ],
            [30, 7, 20]
        );
        ctx.cpu.write_arg(0, 0).unwrap();
        assert_eq!(set(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(20));
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x2004);
        ctx.cpu.write_reg(ArmReg::Sp, 0x9000).unwrap();
        ctx.cpu.write_arg(0, 0).unwrap();
        assert_eq!(set(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(10));
        assert_eq!(ctx.kernel.controls.focus, 30);
        assert!(ctx.kernel.ce.focus_changes.is_empty());
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x1235);
        assert_eq!(ctx.arg_u32(0).unwrap(), 20);
    }
}
