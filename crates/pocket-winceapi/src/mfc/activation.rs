//! MFC activation/cursor preprocessing retains real window callbacks and focus.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const ACTIVATE: &str = "__PocketHLE_MFC_HandleActivate";
pub(super) const CURSOR: &str = "__PocketHLE_MFC_HandleSetCursor";
pub(super) const STYLE: &str = "__PocketHLE_MFC_WindowStyle";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (388, "?_AfxHandleActivate@@YAXPAVCWnd@@I0@Z", activate),
    (389, "?_AfxHandleSetCursor@@YAHPAVCWnd@@II@Z", cursor),
    (1571, "?GetStyle@CWnd@@QBAKXZ", style),
    (1212, "?GetActiveFrame@CFrameWnd@@UAAPAV1@XZ", active_frame),
    (
        1791,
        "?OnActivateTopLevel@CFrameWnd@@IAAJIJ@Z",
        top_activate,
    ),
    (
        1792,
        "?OnActivateTopLevel@CWnd@@IAAJIJ@Z",
        base_top_activate,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        (ACTIVATE, activate as crate::Handler),
        (CURSOR, cursor),
        (STYLE, style),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn style(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let value = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, value);
    }
    let object = f.arg(ctx, 0)?;
    let site = word(ctx, object + 0x40)?;
    if site != 0 {
        let target = virtual_function(ctx, site, 29)?;
        return f.call_args(ctx, 1, target, &[site]);
    }
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 1, "GetWindowLongW", &[hwnd, (-16i32) as u32])
}
fn activate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        return f.call_service(ctx, 1, STYLE, &[object]);
    }
    match f.phase(ctx)? {
        1 if value & 0x40000000 != 0 => f.finish(ctx, 0),
        1 => {
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 2, super::pretranslation::ROOT, &[hwnd])
        }
        2 => {
            f.set(ctx, 0, value)?;
            let other = f.arg(ctx, 2)?;
            let hwnd = if other == 0 {
                0
            } else {
                word(ctx, other + 0x20)?
            };
            f.call_service(ctx, 3, super::pretranslation::ROOT, &[hwnd])
        }
        3 => {
            let root = f.get(ctx, 0)?;
            if root == value {
                return f.finish(ctx, 0);
            }
            if root == 0 {
                return Err(KernelError::Dispatch(
                    "MFC activation has no top-level window".into(),
                ));
            }
            let hwnd = word(ctx, object + 0x20)?;
            let other = f.arg(ctx, 2)?;
            let other_hwnd = if other == 0 {
                0
            } else {
                word(ctx, other + 0x20)?
            };
            put(ctx, f.address + 128, hwnd)?;
            put(ctx, f.address + 132, other_hwnd)?;
            let state = f.arg(ctx, 1)?;
            let pair = f.address + 128;
            f.call_service(ctx, 4, "SendMessageW", &[root, 0x36e, state, pair])
        }
        4 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC activation continuation".into(),
        )),
    }
}
fn cursor(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let hit = f.arg(ctx, 1)?;
        let message = f.arg(ctx, 2)?;
        if hit != (-2i32) as u32 || !matches!(message, 0x201 | 0x204 | 0x207) {
            return f.finish(ctx, 0);
        }
        let object = f.arg(ctx, 0)?;
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, super::pretranslation::ROOT, &[hwnd]);
    }
    match f.phase(ctx)? {
        1 if value == 0 => f.finish(ctx, 0),
        1 => f.call_service(ctx, 2, "GetActiveWindow", &[]),
        2 if value == 0 => f.finish(ctx, 0),
        2 => {
            f.set(ctx, 0, value)?;
            f.call_service(ctx, 3, "GetDesktopWindow", &[])
        }
        3 => {
            let hwnd = f.get(ctx, 0)?;
            if value == hwnd {
                return f.finish(ctx, 0);
            }
            f.call_service(ctx, 4, "IsWindowEnabled", &[hwnd])
        }
        4 if value == 0 => f.finish(ctx, 0),
        4 => {
            let hwnd = f.get(ctx, 0)?;
            f.call_service(ctx, 5, "BringWindowToTop", &[hwnd])
        }
        5 => f.finish(ctx, 1),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC cursor preprocessing continuation".into(),
        )),
    }
}
fn active_frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(ctx.arg_u32(0)?))
}
fn base_top_activate(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // The retail base CWnd handler has no side effects and returns zero.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn kick_idle(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 3, "PostMessageW", &[hwnd, 0x36a, 0, 0])
}
fn notify_view(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    frame: u32,
) -> Result<DispatchOutcome, KernelError> {
    let view = word(ctx, frame + 0xac)?;
    if view == 0 {
        return kick_idle(ctx, f);
    }
    let target = virtual_function(ctx, view, 51)?;
    f.call_args(ctx, 2, target, &[view, 0, view, view])
}
fn top_activate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let thread = super::application::thread_object(ctx)?;
        if thread == 0 {
            return Err(KernelError::Dispatch(
                "MFC activation has no current thread object".into(),
            ));
        }
        if word(ctx, thread + 0x20)? != object {
            return kick_idle(ctx, &f);
        }
        if word(ctx, object + 0xac)? != 0 {
            return notify_view(ctx, &f, object);
        }
        let target = virtual_function(ctx, object, 48)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    match f.phase(ctx)? {
        1 => {
            let frame = ctx.cpu.read_reg(ArmReg::R0)?;
            if frame == 0 {
                return Err(KernelError::Dispatch(
                    "MFC GetActiveFrame returned NULL".into(),
                ));
            }
            notify_view(ctx, &f, frame)
        }
        2 => kick_idle(ctx, &f),
        3 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC top-level activation continuation".into(),
        )),
    }
}
