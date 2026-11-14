//! Explorer's taskbar asks another UI thread for WM_GETICON. Synchronous
//! replies run on the recipient's pump, with a bounded cooperative wait.
use super::*;
use pocket_kernel::ce::{PendingSend, SendCallback, SendOperation};
use std::time::{Duration, Instant};

const MAX_PENDING: usize = 256;
const MAX_CALLBACKS: usize = 128;
const TIMEOUT: u32 = 1460;

pub(super) fn register(d: &mut WinCeDispatcher, dll: &str) {
    d.register_handler(dll, "SendMessageTimeout", send);
}

fn live(kernel: &KernelState, hwnd: u32) -> bool {
    kernel.window_rects.contains_key(&hwnd)
        || kernel.controls.get(hwnd).is_some()
        || kernel.controls.panel(hwnd).is_some()
        || hwnd == FAKE_HWND && !kernel.ce.windows.enabled
}

fn thread_live(kernel: &KernelState, owner: usize) -> bool {
    owner == 0
        || owner
            .checked_sub(1)
            .and_then(|i| kernel.threads.get(i))
            .is_some_and(|t| !t.finished)
}

fn frame(ctx: &mut CallCtx<'_>) -> Result<GuestCallFrame, KernelError> {
    Ok(GuestCallFrame {
        args: [
            ctx.arg_u32(0)?,
            ctx.arg_u32(1)?,
            ctx.arg_u32(2)?,
            ctx.arg_u32(3)?,
        ],
        sp: ctx.cpu.read_reg(ArmReg::Sp)?,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    })
}

fn restore(ctx: &mut CallCtx<'_>, frame: GuestCallFrame) -> Result<(), KernelError> {
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

fn fail(ctx: &mut CallCtx<'_>, code: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, code);
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn output(
    ctx: &mut CallCtx<'_>,
    address: u32,
    result: u32,
) -> Result<DispatchOutcome, KernelError> {
    if address != 0 {
        if ctx.cpu.check_guest_access(address, 4, Prot::WRITE).is_err() {
            return fail(ctx, 87);
        }
        ctx.cpu.write_mem(address, &result.to_le_bytes())?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn prune(kernel: &mut KernelState) {
    let invalid: Vec<_> = kernel
        .ce
        .send_messages
        .pending
        .iter()
        .filter_map(|pending| {
            if !thread_live(kernel, pending.sender) {
                return Some((pending.id, None));
            }
            let error = if !live(kernel, pending.args[0]) || !thread_live(kernel, pending.recipient)
            {
                Some(1400)
            } else if Instant::now() >= pending.deadline && pending.result.is_none() {
                Some(TIMEOUT)
            } else {
                None
            };
            error.map(|code| (pending.id, Some(code)))
        })
        .collect();
    for (id, error) in invalid {
        if let Some(code) = error {
            reply(kernel, id, Err(code));
        } else {
            kernel.ce.send_messages.pending.retain(|p| p.id != id);
        }
    }
    let dead: Vec<_> = kernel
        .ce
        .send_messages
        .callbacks
        .iter()
        .filter(|c| !thread_live(kernel, c.thread))
        .map(|c| c.thread)
        .collect();
    kernel
        .ce
        .send_messages
        .callbacks
        .retain(|c| !dead.contains(&c.thread));
}

fn reply(kernel: &mut KernelState, id: u64, result: Result<u32, u32>) {
    let Some(pending) = kernel
        .ce
        .send_messages
        .pending
        .iter_mut()
        .find(|p| p.id == id && p.result.is_none())
    else {
        return;
    };
    pending.result = Some(if result.is_ok() && Instant::now() >= pending.deadline {
        Err(TIMEOUT)
    } else {
        result
    });
    let (sender, thunk, sp) = (pending.sender, pending.thunk, pending.sp);
    if let Some(thread) = sender
        .checked_sub(1)
        .and_then(|i| kernel.threads.get_mut(i))
    {
        thread.sleep_until_ms = 0;
    }
    if let Some(deadline) = kernel.wait_deadlines.get_mut(&(sender, thunk, sp)) {
        *deadline = 0;
    }
}

pub(super) fn ready(kernel: &KernelState, owner: usize) -> bool {
    kernel.ce.send_messages.pending.iter().any(|p| {
        (p.sender == owner && (p.result.is_some() || Instant::now() >= p.deadline))
            || (p.recipient == owner && !p.delivered && p.result.is_none())
    })
}

fn callback_at(ctx: &mut CallCtx<'_>, pending: bool) -> Result<Option<usize>, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    Ok(ctx.kernel.ce.send_messages.callbacks.iter().rposition(|c| {
        c.thread == ctx.kernel.current_thread
            && c.thunk == ctx.thunk.thunk_va
            && c.frame.sp == sp
            && c.pending.is_some() == pending
    }))
}

fn invoke(
    ctx: &mut CallCtx<'_>,
    args: [u32; 4],
    pending: Option<u64>,
    output: u32,
    operation: SendOperation,
) -> Result<DispatchOutcome, KernelError> {
    let saved = frame(ctx)?;
    let proc = ctx
        .kernel
        .window_procs
        .get(&args[0])
        .copied()
        .filter(|&p| p != 0 && Some(p) != builtin_window_proc(ctx.kernel, args[0]));
    ctx.kernel.ce.send_messages.callbacks.push(SendCallback {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame: saved,
        pending,
        output,
        builtin: proc.is_none(),
        operation,
    });
    for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(args)
    {
        ctx.cpu.write_reg(reg, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
    if operation == SendOperation::DestroyWindow {
        destroy_window(ctx)
    } else if let Some(proc) = proc {
        Ok(DispatchOutcome::JumpTo(proc))
    } else {
        send_message_w(ctx)
    }
}

/// GetMessage/PeekMessage and normal SendMessageTimeout waits service incoming
/// sends on their own thread. A callback returns through that same API thunk.
pub(super) fn service(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    if let Some(index) = callback_at(ctx, true)? {
        let callback = ctx.kernel.ce.send_messages.callbacks.remove(index);
        if callback.operation == SendOperation::DestroyWindow {
            match destroy_window(ctx)? {
                DispatchOutcome::ReturnedR0(value) => ctx.cpu.write_reg(ArmReg::R0, value)?,
                other => {
                    ctx.kernel.ce.send_messages.callbacks.push(callback);
                    return Ok(Some(other));
                }
            }
        } else if callback.builtin {
            if let Some(outcome) = resume_notify_builtin(ctx)? {
                match outcome {
                    DispatchOutcome::ReturnedR0(value) => ctx.cpu.write_reg(ArmReg::R0, value)?,
                    other => {
                        ctx.kernel.ce.send_messages.callbacks.push(callback);
                        return Ok(Some(other));
                    }
                }
            }
        }
        let value = ctx.cpu.read_reg(ArmReg::R0)?;
        reply(ctx.kernel, callback.pending.unwrap(), Ok(value));
        restore(ctx, callback.frame)?;
    }
    if ctx.kernel.dll_notification_frame.is_some() {
        return Ok(None);
    }
    prune(ctx.kernel);
    for _ in 0..MAX_PENDING {
        let Some(index) = ctx.kernel.ce.send_messages.pending.iter().position(|p| {
            p.recipient == ctx.kernel.current_thread && !p.delivered && p.result.is_none()
        }) else {
            return Ok(None);
        };
        let (id, args, operation) = {
            let pending = &mut ctx.kernel.ce.send_messages.pending[index];
            pending.delivered = true;
            (pending.id, pending.args, pending.operation)
        };
        if ctx.kernel.ce.send_messages.callbacks.len() >= MAX_CALLBACKS {
            reply(ctx.kernel, id, Err(1816));
            continue;
        }
        let outcome = invoke(ctx, args, Some(id), 0, operation)?;
        let value = match outcome {
            DispatchOutcome::ReturnedR0(value) => value,
            DispatchOutcome::ReturnedR0R1(value, _) => value,
            other => return Ok(Some(other)),
        };
        let callback = ctx.kernel.ce.send_messages.callbacks.pop().unwrap();
        restore(ctx, callback.frame)?;
        reply(ctx.kernel, id, Ok(value));
    }
    Ok(None)
}

fn send(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if let Some(index) = callback_at(ctx, false)? {
        let callback = ctx.kernel.ce.send_messages.callbacks.remove(index);
        let mut result = ctx.cpu.read_reg(ArmReg::R0)?;
        if callback.builtin {
            if let Some(outcome) = resume_notify_builtin(ctx)? {
                match outcome {
                    DispatchOutcome::ReturnedR0(value) => result = value,
                    other => {
                        ctx.kernel.ce.send_messages.callbacks.push(callback);
                        return Ok(other);
                    }
                }
            }
        }
        restore(ctx, callback.frame)?;
        return output(ctx, callback.output, result);
    }
    if let Some(outcome) = service(ctx)? {
        return Ok(outcome);
    }
    let owner = ctx.kernel.current_thread;
    let key = (owner, ctx.thunk.thunk_va, ctx.cpu.read_reg(ArmReg::Sp)?);
    if let Some(index) = ctx
        .kernel
        .ce
        .send_messages
        .pending
        .iter()
        .position(|p| (p.sender, p.thunk, p.sp) == key)
    {
        let pending = &ctx.kernel.ce.send_messages.pending[index];
        if let Some(result) = pending.result {
            let output_address = pending.output;
            ctx.kernel.ce.send_messages.pending.remove(index);
            ctx.kernel.wait_deadlines.remove(&key);
            return match result {
                Ok(value) => output(ctx, output_address, value),
                Err(code) => fail(ctx, code),
            };
        }
        let deadline = pending.deadline;
        return Ok(wait_display_until(ctx, deadline)?
            .unwrap_or(DispatchOutcome::JumpTo(ctx.thunk.thunk_va)));
    }
    let args = [
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    ];
    let flags = ctx.arg_u32(4)?;
    let timeout = ctx.arg_u32(5)?;
    let result_pointer = ctx.arg_u32(6)?;
    if flags != 0 {
        return fail(ctx, 87);
    } // CE 6 advertises SMTO_NORMAL only.
    if args[0] == 0xffff {
        return fail(ctx, 50);
    }
    if !live(ctx.kernel, args[0]) {
        return fail(ctx, 1400);
    }
    if result_pointer != 0
        && ctx
            .cpu
            .check_guest_access(result_pointer, 4, Prot::WRITE)
            .is_err()
    {
        return fail(ctx, 87);
    }
    let recipient = window_thread(ctx.kernel, args[0]);
    if !thread_live(ctx.kernel, recipient) {
        return fail(ctx, 1400);
    }
    if recipient == owner {
        if ctx.kernel.ce.send_messages.callbacks.len() >= MAX_CALLBACKS {
            return fail(ctx, 1816);
        }
        let outcome = invoke(ctx, args, None, result_pointer, SendOperation::Message)?;
        let value = match outcome {
            DispatchOutcome::ReturnedR0(value) => value,
            DispatchOutcome::ReturnedR0R1(value, _) => value,
            other => return Ok(other),
        };
        let callback = ctx.kernel.ce.send_messages.callbacks.pop().unwrap();
        restore(ctx, callback.frame)?;
        return output(ctx, result_pointer, value);
    }
    if timeout == 0 {
        return fail(ctx, TIMEOUT);
    }
    if ctx.kernel.ce.send_messages.pending.len() >= MAX_PENDING {
        return fail(ctx, 1816);
    }
    let Some(id) = ctx.kernel.ce.send_messages.next_id.checked_add(1) else {
        return fail(ctx, 1816);
    };
    ctx.kernel.ce.send_messages.next_id = id;
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout));
    ctx.kernel.ce.send_messages.pending.push(PendingSend {
        id,
        sender: owner,
        thunk: key.1,
        sp: key.2,
        output: result_pointer,
        recipient,
        args,
        operation: SendOperation::Message,
        deadline,
        result: None,
        delivered: false,
    });
    wake_window_thread(ctx.kernel, recipient);
    Ok(wait_display_until(ctx, deadline)?.unwrap_or(DispatchOutcome::JumpTo(ctx.thunk.thunk_va)))
}

/// CE Messenger disposes its startup window from the primary thread. Its
/// destruction procedure must run on the window thread, so PostQuitMessage
/// ends that startup pump rather than the application's main UI pump.
pub(super) fn destroy_on_owner(
    ctx: &mut CallCtx<'_>,
) -> Result<Option<DispatchOutcome>, KernelError> {
    let key = (
        ctx.kernel.current_thread,
        ctx.thunk.thunk_va,
        ctx.cpu.read_reg(ArmReg::Sp)?,
    );
    if ctx
        .kernel
        .window_destroy_frames
        .iter()
        .any(|frame| (frame.thread, frame.thunk, frame.frame.sp) == key)
    {
        return Ok(None);
    }
    prune(ctx.kernel);
    if let Some(index) = ctx.kernel.ce.send_messages.pending.iter().position(|p| {
        (p.sender, p.thunk, p.sp) == key && p.operation == SendOperation::DestroyWindow
    }) {
        let pending = &ctx.kernel.ce.send_messages.pending[index];
        if let Some(result) = pending.result {
            ctx.kernel.ce.send_messages.pending.remove(index);
            ctx.kernel.wait_deadlines.remove(&key);
            return Ok(Some(match result {
                Ok(value) => DispatchOutcome::ReturnedR0(value),
                Err(code) => fail(ctx, code)?,
            }));
        }
        let deadline = pending.deadline;
        return Ok(Some(
            wait_display_until(ctx, deadline)?
                .unwrap_or(DispatchOutcome::JumpTo(ctx.thunk.thunk_va)),
        ));
    }
    let hwnd = ctx.arg_u32(0)?;
    let recipient = window_thread(ctx.kernel, hwnd);
    if recipient == key.0 || !live(ctx.kernel, hwnd) {
        return Ok(None);
    }
    if !thread_live(ctx.kernel, recipient) {
        return fail(ctx, 1400).map(Some);
    }
    if ctx.kernel.ce.send_messages.pending.len() >= MAX_PENDING {
        return fail(ctx, 1816).map(Some);
    }
    let Some(id) = ctx.kernel.ce.send_messages.next_id.checked_add(1) else {
        return fail(ctx, 1816).map(Some);
    };
    ctx.kernel.ce.send_messages.next_id = id;
    let deadline = Instant::now() + Duration::from_millis(u64::from(u32::MAX));
    ctx.kernel.ce.send_messages.pending.push(PendingSend {
        id,
        sender: key.0,
        thunk: key.1,
        sp: key.2,
        output: 0,
        recipient,
        args: [hwnd, 0, 0, 0],
        operation: SendOperation::DestroyWindow,
        deadline,
        result: None,
        delivered: false,
    });
    wake_window_thread(ctx.kernel, recipient);
    Ok(Some(
        wait_display_until(ctx, deadline)?.unwrap_or(DispatchOutcome::JumpTo(ctx.thunk.thunk_va)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(ctx: &mut CallCtx<'_>, values: &[u32]) {
        for (i, &value) in values.iter().enumerate() {
            if i < 4 {
                ctx.cpu.write_arg(i as u8, value).unwrap();
            } else {
                let sp = ctx.cpu.read_reg(ArmReg::Sp).unwrap();
                ctx.cpu
                    .write_mem(sp + (i as u32 - 4) * 4, &value.to_le_bytes())
                    .unwrap();
            }
        }
    }
    fn windows(kernel: &mut KernelState) {
        kernel.window_rects.insert(0x111, (0, 0, 100, 100));
        kernel.window_procs.insert(0x111, 0x2100);
        kernel.window_threads.insert(0x111, 1);
        kernel.threads.push(GuestThread::new(
            0x2100, 0, 0x9000, 0x1000, 0x2200, 0, 0xdead7c00, [0; 17],
        ));
    }

    #[test]
    fn a_foreign_window_destruction_keeps_quit_on_the_window_thread() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        windows(&mut kernel);
        kernel.window_classes.insert(0x111, "startup".into());
        let sender = super::super::tests::dummy_thunk();
        let mut receiver = sender.clone();
        receiver.thunk_va += 32;
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &sender,
        };
        args(&mut ctx, &[0x111, 0, 0, 0]);
        ctx.cpu.write_reg(ArmReg::Lr, 0x4455).unwrap();
        assert_eq!(
            destroy_window(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(sender.thunk_va)
        );
        ctx.kernel.current_thread = 1;
        ctx.thunk = &receiver;
        args(&mut ctx, &[0x2000, 0, 0, 0]);
        ctx.cpu.write_reg(ArmReg::Lr, 0x5566).unwrap();
        assert_eq!(
            service(&mut ctx).unwrap(),
            Some(DispatchOutcome::JumpTo(0x2100))
        );
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), WM_DESTROY);
        ctx.cpu.write_reg(ArmReg::R0, 7).unwrap();
        post_quit_message(&mut ctx).unwrap();
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            service(&mut ctx).unwrap(),
            Some(DispatchOutcome::JumpTo(0x2100))
        );
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), 0x82);
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(service(&mut ctx).unwrap(), None);
        assert!(!ctx.kernel.window_rects.contains_key(&0x111));
        assert_eq!(
            ctx.kernel.threads[0].messages.front(),
            Some(&(WM_QUIT, 7, 0))
        );
        assert!(!ctx
            .kernel
            .posted_messages
            .iter()
            .any(|message| message.1 == WM_QUIT));
        assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), 0x2000);
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x5566);
        ctx.kernel.current_thread = 0;
        ctx.thunk = &sender;
        args(&mut ctx, &[0x111, 0, 0, 0]);
        ctx.cpu.write_reg(ArmReg::Lr, 0x4455).unwrap();
        assert_eq!(
            destroy_window(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(ctx.kernel.ce.send_messages.pending.is_empty());
        assert!(ctx.kernel.ce.send_messages.callbacks.is_empty());
    }

    #[test]
    fn foreign_reply_runs_on_receiver_and_restores_both_calls() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        windows(&mut kernel);
        let sender = super::super::tests::dummy_thunk();
        let mut receiver = sender.clone();
        receiver.thunk_va += 32;
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &sender,
        };
        ctx.cpu.write_mem(0x1800, &[0xcc; 4]).unwrap();
        ctx.cpu.write_reg(ArmReg::Lr, 0x4455).unwrap();
        args(&mut ctx, &[0x111, 0x7f, 1, 0, 0, 1000, 0x1800]);
        assert_eq!(
            send(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(sender.thunk_va)
        );
        assert!(ctx.kernel.ce.send_messages.callbacks.is_empty());
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0xcccccccc);
        ctx.kernel.current_thread = 1;
        ctx.thunk = &receiver;
        args(&mut ctx, &[0x2000, 0, 0, 0]);
        ctx.cpu.write_reg(ArmReg::Lr, 0x5566).unwrap();
        assert_eq!(
            service(&mut ctx).unwrap(),
            Some(DispatchOutcome::JumpTo(0x2100))
        );
        assert_eq!(ctx.kernel.current_thread, 1);
        assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), 0x111);
        ctx.cpu.write_reg(ArmReg::R0, 0xdead1234).unwrap();
        assert_eq!(service(&mut ctx).unwrap(), None);
        assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), 0x2000);
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x5566);
        ctx.kernel.current_thread = 0;
        ctx.thunk = &sender;
        args(&mut ctx, &[0x111, 0x7f, 1, 0]);
        ctx.cpu.write_reg(ArmReg::Lr, 0x4455).unwrap();
        assert_eq!(send(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0xdead1234);
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x4455);
        assert!(ctx.kernel.ce.send_messages.pending.is_empty());
        assert!(ctx.kernel.ce.send_messages.callbacks.is_empty());
    }

    #[test]
    fn timeout_and_destroyed_recipient_leave_result_untouched() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        windows(&mut kernel);
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        ctx.cpu.write_mem(0x1800, &[0xcc; 4]).unwrap();
        args(&mut ctx, &[0x111, 0x7f, 0, 0, 0, 1000, 0x1800]);
        assert!(matches!(
            send(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(_)
        ));
        ctx.kernel.ce.send_messages.pending[0].deadline = Instant::now();
        assert_eq!(send(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.kernel.thread_last_errors[&0], TIMEOUT);
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0xcccccccc);
        assert!(ctx.kernel.ce.send_messages.pending.is_empty());
        assert!(matches!(
            send(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(_)
        ));
        ctx.kernel.window_rects.remove(&0x111);
        assert_eq!(send(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.kernel.thread_last_errors[&0], 1400);
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0xcccccccc);
    }

    #[test]
    fn same_thread_result_and_invalid_output_follow_ce_contract() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        windows(&mut kernel);
        kernel.window_threads.insert(0x111, 0);
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        ctx.cpu.write_reg(ArmReg::Lr, 0x4455).unwrap();
        args(&mut ctx, &[0x111, 0x7f, 0, 0, 0, 0, 0x1800]);
        assert_eq!(send(&mut ctx).unwrap(), DispatchOutcome::JumpTo(0x2100));
        ctx.cpu.write_reg(ArmReg::R0, 99).unwrap();
        assert_eq!(send(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 99);
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x4455);
        ctx.cpu.map_region(0x6000, 0x1000, Prot::READ).unwrap();
        args(&mut ctx, &[0x111, 0x7f, 0, 0, 0, 1000, 0x6000]);
        assert_eq!(send(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.kernel.thread_last_errors[&0], 87);
        assert!(ctx.kernel.ce.send_messages.callbacks.is_empty());
    }

    #[test]
    fn a_late_callback_cannot_replace_a_timeout_with_success() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        windows(&mut kernel);
        let sender = super::super::tests::dummy_thunk();
        let mut receiver = sender.clone();
        receiver.thunk_va += 32;
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &sender,
        };
        ctx.cpu.write_mem(0x1800, &[0xcc; 4]).unwrap();
        args(&mut ctx, &[0x111, 0x7f, 0, 0, 0, 1000, 0x1800]);
        assert!(matches!(
            send(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(_)
        ));
        ctx.kernel.current_thread = 1;
        ctx.thunk = &receiver;
        args(&mut ctx, &[0x2000, 0, 0, 0]);
        assert!(matches!(
            service(&mut ctx).unwrap(),
            Some(DispatchOutcome::JumpTo(_))
        ));
        ctx.kernel.ce.send_messages.pending[0].deadline = Instant::now();
        ctx.cpu.write_reg(ArmReg::R0, 99).unwrap();
        assert_eq!(service(&mut ctx).unwrap(), None);
        ctx.kernel.current_thread = 0;
        ctx.thunk = &sender;
        args(&mut ctx, &[0x111, 0x7f, 0, 0]);
        assert_eq!(send(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.kernel.thread_last_errors[&0], TIMEOUT);
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0xcccccccc);
    }
}
