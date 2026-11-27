//! Menu/accelerator commands honor update vetoes and control reflection first.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const WINDOW: &str = "__PocketHLE_MFC_WindowCommand";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1840, "?OnCommand@CWnd@@MAAHIJ@Z", window),
    (1837, "?OnCommand@CFrameWnd@@MAAHIJ@Z", frame),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", WINDOW, window);
}
fn route(ctx: &mut CallCtx<'_>, f: &Frame, code: u32) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let id = f.arg(ctx, 1)? & 0xffff;
    let method = virtual_function(ctx, object, 5)?;
    f.call_args(ctx, 3, method, &[object, id, code, 0, 0])
}
fn window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let wp = f.arg(ctx, 1)?;
    let child = f.arg(ctx, 2)?;
    let id = wp & 0xffff;
    if f.fresh {
        if child == 0 {
            if id == 0 {
                return f.finish(ctx, 0);
            }
            let ui = f.address + 128;
            super::command_ui::initialize_probe(ctx, ui)?;
            put(ctx, ui + 4, id)?;
            let method = virtual_function(ctx, object, 5)?;
            return f.call_args(ctx, 1, method, &[object, id, u32::MAX, ui, 0]);
        }
        let state = super::application::current_thread_state(ctx)?;
        if word(ctx, state + 0x120)? == word(ctx, object + 0x20)? {
            return f.finish(ctx, 1);
        }
        if let Some(wrapper) = super::windows::permanent(ctx, child)? {
            let message = state + 0x3c;
            let args = [
                wrapper,
                word(ctx, message + 4)?,
                word(ctx, message + 8)?,
                word(ctx, message + 12)?,
                0,
            ];
            let method = virtual_function(ctx, wrapper, 42)?;
            return f.call_args(ctx, 2, method, &args);
        }
        let parent = ctx.kernel.window_parents.get(&child).copied().unwrap_or(0);
        if let Some(parent) = super::windows::permanent(ctx, parent)? {
            if word(ctx, parent + 0x3c)? != 0 {
                return Err(KernelError::Dispatch(
                    "MFC OLE command reflection is not implemented".into(),
                ));
            }
        }
        if id == 0 {
            return f.finish(ctx, 0);
        }
        return route(ctx, &f, wp >> 16);
    }
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if word(ctx, f.address + 128 + 0x28)? == 0 {
                return f.finish(ctx, 1);
            }
            route(ctx, &f, 0)
        }
        2 if returned != 0 => f.finish(ctx, 1),
        2 if id == 0 => f.finish(ctx, 0),
        2 => route(ctx, &f, wp >> 16),
        3 => f.finish(ctx, returned),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC window command continuation".into(),
        )),
    }
}
fn frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let wp = f.arg(ctx, 1)?;
    let lp = f.arg(ctx, 2)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::hierarchy::TOP, &[object]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            let id = wp & 0xffff;
            if result != 0
                && word(ctx, result + 0x68)? != 0
                && lp == 0
                && !matches!(id, 0xe145..=0xe147)
            {
                let hwnd = word(ctx, object + 0x20)?;
                return f.call_service(
                    ctx,
                    2,
                    "SendMessageW",
                    &[hwnd, 0x365, 0, id.wrapping_add(0x10000)],
                );
            }
            f.call_service(ctx, 4, WINDOW, &[object, wp, lp])
        }
        2 if result == 0 => {
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 3, "SendMessageW", &[hwnd, 0x111, 0xe147, 0])
        }
        2 | 3 => f.finish(ctx, 1),
        4 => f.finish(ctx, result),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame command continuation".into(),
        )),
    }
}
