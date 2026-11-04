//! SendNotifyMessage keeps same-thread callbacks synchronous and queues others.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{DispatchOutcome, GuestCallFrame, KernelError, NotifyMessageFrame};
pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "SendNotifyMessageW", send);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn ce_notify_builtin_owner_draw_retires_its_nested_callback_and_scratch() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(
            0x50000000,
            0x10000,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )
        .unwrap();
        k.window_procs.insert(0x111, 0x2100);
        let button = k.controls.create(
            0x111,
            pocket_kernel::controls::ControlClass::Button,
            8,
            String::new(),
            0x5000000b,
            0,
            0,
            40,
            30,
        );
        cpu.write_reg(ArmReg::Lr, 0x1234).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendNotifyMessageW",
                &[button, 0x0f, 0, 0]
            ),
            DispatchOutcome::JumpTo(0x2100)
        );
        assert_eq!(k.gdi.owner_draw_frames.len(), 1);
        let scratch = cpu.read_reg(ArmReg::R3).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendNotifyMessageW",
                &[1]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
        assert!(k.gdi.owner_draw_frames.is_empty());
        assert!(k.notify_message_frames.is_empty());
        assert_eq!(k.heap.alloc(48), Some(scratch));
    }
    #[test]
    fn ce_notify_broadcast_preserves_sync_callbacks_and_queues_other_threads() {
        let (mut cpu, mut k, mut d) = setup();
        k.window_procs.insert(0x111, 0x2100);
        k.window_threads.insert(0x111, 0);
        k.window_procs.insert(0x112, 0x2200);
        k.window_threads.insert(0x112, 1);
        k.window_procs.insert(0x113, 0x2300);
        k.window_threads.insert(0x113, 0);
        cpu.write_reg(ArmReg::Lr, 0x1234).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendNotifyMessageW",
                &[0xffff, 0x1a, 0, 0]
            ),
            DispatchOutcome::JumpTo(0x2100)
        );
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 0x111);
        assert_eq!(k.posted_messages.back(), Some(&(0x112, 0x1a, 0, 0)));
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendNotifyMessageW",
                &[0]
            ),
            DispatchOutcome::JumpTo(0x2300)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendNotifyMessageW",
                &[999]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
        assert!(k.notify_message_frames.is_empty());
    }
    #[test]
    fn ce_notify_rejects_async_pointer_payloads_without_queueing() {
        let (mut cpu, mut k, mut d) = setup();
        k.window_procs.insert(0x111, 0x2100);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendNotifyMessageW",
                &[0xffff, 0x0c, 0, 0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(k.posted_messages.is_empty());
        assert!(k.notify_message_frames.is_empty());
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "coredll.dll", "GetLastError", &[]),
            DispatchOutcome::ReturnedR0(1159)
        );
    }
}
fn restore(ctx: &mut CallCtx<'_>, frame: GuestCallFrame) -> Result<(), KernelError> {
    for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(frame.args)
    {
        ctx.cpu.write_reg(reg, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Lr, frame.lr)?;
    Ok(())
}
fn fail(ctx: &mut CallCtx<'_>, code: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, code);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn advance(
    ctx: &mut CallCtx<'_>,
    mut state: NotifyMessageFrame,
) -> Result<DispatchOutcome, KernelError> {
    while let Some(hwnd) = state.recipients.pop() {
        // A previous recipient may have destroyed the next window.
        let proc = ctx
            .kernel
            .window_procs
            .get(&hwnd)
            .copied()
            .unwrap_or_else(|| {
                if hwnd == crate::coredll::FAKE_HWND {
                    ctx.kernel.wnd_proc
                } else {
                    0
                }
            });
        if proc == 0 && ctx.kernel.controls.get(hwnd).is_none() {
            continue;
        }
        ctx.cpu.write_reg(ArmReg::R0, hwnd)?;
        ctx.cpu.write_reg(ArmReg::R1, state.frame.args[1])?;
        ctx.cpu.write_reg(ArmReg::R2, state.frame.args[2])?;
        ctx.cpu.write_reg(ArmReg::R3, state.frame.args[3])?;
        if proc == 0 {
            let outcome = crate::coredll::send_message_w(ctx)?;
            if matches!(outcome, DispatchOutcome::JumpTo(_)) {
                ctx.kernel.notify_message_frames.push(state);
                return Ok(outcome);
            }
            continue;
        }
        ctx.cpu.write_reg(ArmReg::Lr, state.thunk)?;
        ctx.kernel.notify_message_frames.push(state);
        return Ok(DispatchOutcome::JumpTo(proc));
    }
    restore(ctx, state.frame)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn send(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if let Some(i) = ctx.kernel.notify_message_frames.iter().rposition(|f| {
        f.thread == ctx.kernel.current_thread && f.thunk == ctx.thunk.thunk_va && f.frame.sp == sp
    }) {
        let state = ctx.kernel.notify_message_frames.remove(i);
        if let Some(outcome) = crate::coredll::resume_notify_builtin(ctx)? {
            if matches!(outcome, DispatchOutcome::JumpTo(_)) {
                ctx.kernel.notify_message_frames.push(state);
                return Ok(outcome);
            }
        }
        return advance(ctx, state);
    }
    let args = [
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    ];
    let [hwnd, message, wp, lp] = args;
    // System messages with pointer payloads cannot be sent asynchronously.
    // Null WM_SETTINGCHANGE / WM_DEVMODECHANGE notifications have no payload.
    if lp != 0
        && matches!(
            message,
            1 | 0x0c
                | 0x0d
                | 0x1a
                | 0x1b
                | 0x24
                | 0x2b
                | 0x2c
                | 0x39
                | 0x46
                | 0x47
                | 0x4a
                | 0x4e
                | 0x81
                | 0x83
        )
    {
        return fail(ctx, 1159);
    }
    if hwnd == 0 {
        return fail(ctx, 1400);
    }
    if crate::shell::message(ctx, hwnd, message, wp)
        || crate::bluetooth::shell_message(ctx, hwnd, message, wp)
    {
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let mut same = Vec::new();
    let mut other = Vec::new();
    for window in crate::coredll::notification_recipients(ctx.kernel, hwnd) {
        if !ctx.kernel.window_procs.contains_key(&window)
            && ctx.kernel.controls.get(window).is_none()
            && !(window == crate::coredll::FAKE_HWND && ctx.kernel.wnd_proc != 0)
        {
            if hwnd != 0xffff {
                return fail(ctx, 1400);
            } else {
                continue;
            }
        }
        if ctx.kernel.window_threads.get(&window).copied().unwrap_or(0) == ctx.kernel.current_thread
        {
            same.push(window);
        } else {
            other.push(window);
        }
    }
    if ctx.kernel.posted_messages.len() + other.len() > 256 {
        return fail(ctx, 1816);
    }
    for window in other {
        crate::coredll::enqueue_window_message(ctx.kernel, window, message, wp, lp);
    }
    same.reverse();
    let lr = ctx.cpu.read_reg(ArmReg::Lr)?;
    advance(
        ctx,
        NotifyMessageFrame {
            thread: ctx.kernel.current_thread,
            thunk: ctx.thunk.thunk_va,
            frame: GuestCallFrame { args, sp, lr },
            recipients: same,
        },
    )
}
