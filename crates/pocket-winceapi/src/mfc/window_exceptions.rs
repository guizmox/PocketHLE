//! CE3 AfxCallWndProc catches through the same guest TRY protocol as UpdateData.
//! MSG restoration is frame-owned, including a replacement exception escaping
//! the application's virtual ProcessWndProcException hook.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const CALL: &str = "__PocketHLE_MFC_CallWndProc";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        606,
        "?AfxCallWndProc@@YAJPAVCWnd@@PAUHWND__@@IIJ@Z",
        call_window,
    ),
    (
        2115,
        "?ProcessWndProcException@CWinApp@@UAAJPAVCException@@PBUtagMSG@@@Z",
        app_exception,
    ),
    (
        2116,
        "?ProcessWndProcException@CWinThread@@UAAJPAVCException@@PBUtagMSG@@@Z",
        thread_exception,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", CALL, call_window);
}
fn call_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let message = f.arg(ctx, 2)?;
    if f.fresh {
        let lparam = ctx.arg_u32(4)?;
        f.set(ctx, 3, lparam)?;
        let slot = super::application::current_thread_state(ctx)? + 0x3c;
        f.set(ctx, 0, slot)?;
        f.save_region(ctx, 96, slot, 28)?;
        let hwnd = f.arg(ctx, 1)?;
        let wparam = f.arg(ctx, 3)?;
        for (i, value) in [hwnd, message, wparam, lparam].into_iter().enumerate() {
            put(ctx, slot + i as u32 * 4, value)?;
        }
        super::exception_links::begin_catch(ctx, &f, 160)?;
        if message == 2 {
            let container = word(ctx, object + 0x3c)?;
            if container != 0 {
                let target = virtual_function(ctx, container, 24)?;
                return f.call_args(ctx, 1, target, &[container, 0]);
            }
        }
        return dispatch_window(ctx, &f);
    }
    let exception = word(ctx, f.address + 212)?;
    let phase = f.phase(ctx)?;
    if exception != 0 && phase <= 2 {
        let thread = super::application::thread_object(ctx)?;
        if thread == 0 {
            return Err(KernelError::Dispatch(
                "MFC window exception has no CWinThread".into(),
            ));
        }
        let target = virtual_function(ctx, thread, 28)?;
        let msg = f.get(ctx, 0)?;
        return f.call_args(ctx, 3, target, &[thread, exception, msg]);
    }
    match phase {
        1 => dispatch_window(ctx, &f),
        2 | 3 => {
            let result = ctx.cpu.read_reg(ArmReg::R0)?;
            f.set(ctx, 1, result)?;
            if exception != 0 {
                let target = service(ctx, super::exception_links::DELETE)?;
                return f.call_args(ctx, 4, target, &[exception]);
            }
            finish_window(ctx, f)
        }
        4 => finish_window(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC AfxCallWndProc continuation".into(),
        )),
    }
}
fn dispatch_window(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let message = f.arg(ctx, 2)?;
    let wparam = f.arg(ctx, 3)?;
    let lparam = f.get(ctx, 3)?;
    let target = virtual_function(ctx, object, 38)?;
    f.call_args(ctx, 2, target, &[object, message, wparam, lparam])
}
fn finish_window(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    super::exception_links::unlink(ctx, f.address + 160)?;
    let result = f.get(ctx, 1)?;
    f.finish(ctx, result)
}
fn thread_exception(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    process_thread(ctx, f)
}
fn process_thread(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if f.fresh {
        let msg = f.arg(ctx, 2)?;
        ctx.cpu.check_guest_access(msg, 28, Prot::READ)?;
        match word(ctx, msg + 4)? {
            1 => return f.finish(ctx, u32::MAX), // Failed WM_CREATE aborts creation.
            15 => {
                let hwnd = word(ctx, msg)?;
                let target = service(ctx, "ValidateRect")?;
                return f.call_args(ctx, 1, target, &[hwnd, 0]);
            }
            _ => (),
        }
    }
    f.finish(ctx, 0)
}
fn app_exception(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let exception = f.arg(ctx, 1)?;
    if !f.fresh && f.get(ctx, 4)? != 0 {
        return process_thread(ctx, f);
    }
    if f.fresh {
        let msg = f.arg(ctx, 2)?;
        ctx.cpu.check_guest_access(msg, 28, Prot::READ)?;
        let message = word(ctx, msg + 4)?;
        if matches!(message, 1 | 15) {
            // The CE3 application delegates these to the thread implementation.
            f.set(ctx, 4, 1)?;
            return process_thread(ctx, f);
        }
        let command = message == 0x111;
        f.set(ctx, 0, u32::from(command))?;
        let id = if command && word(ctx, msg + 12)? == 0 {
            0xf109
        } else {
            0xf108
        };
        f.set(ctx, 1, id)?;
        let class = super::classes::runtime(ctx, "CMemoryException")?;
        let target = service(ctx, super::class_queries::KIND)?;
        return f.call_args(ctx, 1, target, &[exception, class]);
    }
    let matched = ctx.cpu.read_reg(ArmReg::R0)? != 0;
    match f.phase(ctx)? {
        1 if matched => report(ctx, &f, exception, 0x30),
        1 => {
            let class = super::classes::runtime(ctx, "CUserException")?;
            let target = service(ctx, super::class_queries::KIND)?;
            f.call_args(ctx, 2, target, &[exception, class])
        }
        2 if !matched => report(ctx, &f, exception, 0x10),
        2 | 3 => {
            let answer = f.get(ctx, 0)?;
            f.finish(ctx, answer)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC window exception continuation".into(),
        )),
    }
}
fn report(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    exception: u32,
    style: u32,
) -> Result<DispatchOutcome, KernelError> {
    let target = virtual_function(ctx, exception, 6)?;
    let id = f.get(ctx, 1)?;
    f.call_args(ctx, 3, target, &[exception, style, id])
}

#[cfg(test)]
#[path = "window_exception_tests.rs"]
mod tests;
