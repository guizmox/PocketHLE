//! CE3 modal control flow uses the existing thread pump and USER queue.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const RUN: &str = "__PocketHLE_MFC_RunModalLoop";
pub(super) const END: &str = "__PocketHLE_MFC_EndModalLoop";
const THREAD: u32 = 0;
const SHOW: u32 = 1;
const IDLE: u32 = 2;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2236, "?RunModalLoop@CWnd@@QAAHK@Z", run),
    (889, "?ContinueModal@CWnd@@UAAHXZ", continuing),
    (1134, "?EndModalLoop@CWnd@@UAAXH@Z", end),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", RUN, run);
    d.register_handler("coredll.dll", END, end);
}
fn continuing(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(
        word(ctx, object + 0x2c)? & 0x10,
    ))
}
fn end(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    ctx.cpu
        .check_guest_access(object, 72, Prot::READ | Prot::WRITE)?;
    let result = f.arg(ctx, 1)?;
    put(ctx, object + 0x34, result)?;
    let flags = word(ctx, object + 0x2c)?;
    if flags & 0x10 == 0 {
        return f.finish(ctx, 0);
    }
    put(ctx, object + 0x2c, flags & !0x10)?;
    let hwnd = word(ctx, object + 0x20)?;
    // Wake a pump blocked on GetMessage; the real WM_NULL remains in USER's
    // queue rather than a synthetic host-side termination flag.
    f.call_service(ctx, 1, "PostMessageW", &[hwnd, 0, 0, 0])
}
fn message(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<u32, KernelError> {
    Ok(f.get(ctx, THREAD)? + 0x34)
}
fn peek(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    phase: u32,
    remove: bool,
) -> Result<DispatchOutcome, KernelError> {
    let msg = message(ctx, f)?;
    f.call_service(
        ctx,
        phase,
        "PeekMessageW",
        &[msg, 0, 0, 0, u32::from(remove)],
    )
}
fn start(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let phase = if f.get(ctx, IDLE)? != 0 { 1 } else { 4 };
    peek(ctx, f, phase, false)
}
fn pump(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let thread = f.get(ctx, THREAD)?;
    let target = virtual_function(ctx, thread, 24)?;
    f.call_args(ctx, 7, target, &[thread])
}
fn continue_virtual(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let target = virtual_function(ctx, object, 28)?;
    f.call_args(ctx, 10, target, &[object])
}
fn after_message(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let msg = message(ctx, f)?;
    if f.get(ctx, SHOW)? != 0 && matches!(word(ctx, msg + 4)?, 0x118 | 0x104) {
        f.set(ctx, SHOW, 0)?;
        let object = f.arg(ctx, 0)?;
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 8, "ShowWindow", &[hwnd, 1]);
    }
    continue_virtual(ctx, f)
}
fn run(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(object, 72, Prot::READ | Prot::WRITE)?;
        let thread = super::application::thread_object(ctx)?;
        if thread == 0 {
            return Err(KernelError::Dispatch(
                "MFC modal loop requires a real CWinThread".into(),
            ));
        }
        let hwnd = word(ctx, object + 0x20)?;
        if !crate::coredll::window_exists(ctx.kernel, hwnd) {
            return Err(KernelError::Dispatch(
                "MFC modal loop has no live window".into(),
            ));
        }
        f.set(ctx, THREAD, thread)?;
        f.set(ctx, IDLE, 1)?;
        let flags = f.arg(ctx, 1)?;
        if flags & 4 != 0 {
            return f.call_service(ctx, 20, "GetWindowLongW", &[hwnd, (-16i32) as u32]);
        }
        return initialize(ctx, &f, object, false);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        20 => initialize(ctx, &f, object, result & 0x10000000 == 0),
        1 => {
            if result == 0 {
                f.set(ctx, IDLE, 0)?;
                if f.get(ctx, SHOW)? != 0 {
                    f.set(ctx, SHOW, 0)?;
                    let hwnd = word(ctx, object + 0x20)?;
                    return f.call_service(ctx, 2, "ShowWindow", &[hwnd, 1]);
                }
            }
            peek(ctx, &f, 4, false)
        }
        2 => {
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 3, "UpdateWindow", &[hwnd])
        }
        3 => peek(ctx, &f, 4, false),
        4 => {
            let msg = message(ctx, &f)?;
            if result != 0
                && f.get(ctx, SHOW)? != 0
                && (0x200..=0x209).contains(&word(ctx, msg + 4)?)
            {
                let hwnd = word(ctx, msg)?;
                return f.call_service(ctx, 5, "IsWindowEnabled", &[hwnd]);
            }
            pump(ctx, &f)
        }
        5 if result == 0 => peek(ctx, &f, 6, true),
        5 => pump(ctx, &f),
        6 => after_message(ctx, &f),
        7 if result == 0 => f.call_service(ctx, 13, "PostQuitMessage", &[0]),
        7 => after_message(ctx, &f),
        8 => {
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 9, "UpdateWindow", &[hwnd])
        }
        9 => continue_virtual(ctx, &f),
        10 if result == 0 => {
            let flags = word(ctx, object + 0x2c)?;
            put(ctx, object + 0x2c, flags & !0x18)?;
            let value = word(ctx, object + 0x34)?;
            f.finish(ctx, value)
        }
        10 => {
            let thread = f.get(ctx, THREAD)?;
            let msg = message(ctx, &f)?;
            let target = virtual_function(ctx, thread, 26)?;
            f.call_args(ctx, 11, target, &[thread, msg])
        }
        11 => {
            if result != 0 {
                f.set(ctx, IDLE, 1)?;
            }
            peek(ctx, &f, 12, false)
        }
        12 if result != 0 => peek(ctx, &f, 4, false),
        12 => start(ctx, &f),
        13 => f.finish(ctx, u32::MAX),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC modal-loop continuation".into(),
        )),
    }
}
fn initialize(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    object: u32,
    show: bool,
) -> Result<DispatchOutcome, KernelError> {
    f.set(ctx, SHOW, u32::from(show))?;
    let flags = word(ctx, object + 0x2c)?;
    put(ctx, object + 0x2c, flags | 0x18)?;
    start(ctx, f)
}
