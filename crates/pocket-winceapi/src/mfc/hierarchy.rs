//! Frame ancestors follow the shared live HWND hierarchy and IsFrameWnd virtuals.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const TOP: &str = "__PocketHLE_MFC_TopLevelFrame";
pub(super) const PARENT: &str = "__PocketHLE_MFC_ParentFrame";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1412, "?GetParentFrame@CWnd@@QBAPAVCFrameWnd@@XZ", parent),
    (1580, "?GetTopLevelFrame@CWnd@@QBAPAVCFrameWnd@@XZ", top),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", TOP, top);
    d.register_handler("coredll.dll", PARENT, parent);
}
fn next_parent(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    hwnd: u32,
) -> Result<DispatchOutcome, KernelError> {
    f.call_service(ctx, 1, "GetParent", &[hwnd])
}
fn parent(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let hwnd = if object == 0 {
            0
        } else {
            word(ctx, object + 0x20)?
        };
        if hwnd == 0 {
            return f.finish(ctx, 0);
        }
        return next_parent(ctx, &f, hwnd);
    }
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if value == 0 || value == crate::coredll::FAKE_DESKTOP_HWND {
                return f.finish(ctx, 0);
            }
            let count = f.get(ctx, 0)?;
            if count >= 128 {
                return Err(KernelError::Dispatch(
                    "MFC parent-frame chain exceeds its bound".into(),
                ));
            }
            f.set(ctx, 0, count + 1)?;
            f.set(ctx, 1, value)?;
            if let Some(object) = super::windows::permanent(ctx, value)? {
                f.set(ctx, 2, object)?;
                let target = virtual_function(ctx, object, 44)?;
                f.call_args(ctx, 2, target, &[object])
            } else {
                // An unbound OS window would have a temporary base CWnd whose
                // IsFrameWnd is FALSE. No intermediate wrapper escapes this search.
                next_parent(ctx, &f, value)
            }
        }
        2 if value != 0 => {
            let object = f.get(ctx, 2)?;
            f.finish(ctx, object)
        }
        2 => {
            let hwnd = f.get(ctx, 1)?;
            next_parent(ctx, &f, hwnd)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC parent-frame continuation".into(),
        )),
    }
}
fn top(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        if object == 0 || word(ctx, object + 0x20)? == 0 {
            return f.finish(ctx, 0);
        }
        let target = virtual_function(ctx, object, 44)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            let object = f.arg(ctx, 0)?;
            if value != 0 {
                f.set(ctx, 0, object)?;
                return f.call_service(ctx, 3, PARENT, &[object]);
            }
            f.call_service(ctx, 2, PARENT, &[object])
        }
        2 if value == 0 => f.finish(ctx, 0),
        2 | 3 if value != 0 => {
            let count = f.get(ctx, 1)?;
            if count >= 128 {
                return Err(KernelError::Dispatch(
                    "MFC top-level frame chain exceeds its bound".into(),
                ));
            }
            f.set(ctx, 1, count + 1)?;
            f.set(ctx, 0, value)?;
            f.call_service(ctx, 3, PARENT, &[value])
        }
        3 => {
            let object = f.get(ctx, 0)?;
            f.finish(ctx, object)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC top-level frame continuation".into(),
        )),
    }
}
