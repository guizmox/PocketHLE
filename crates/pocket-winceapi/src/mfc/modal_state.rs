//! Frame modal counters and saved HWND arrays remain in their native guest owners.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (812, "?BeginModalState@CFrameWnd@@UAAXXZ", begin_frame),
    (1135, "?EndModalState@CFrameWnd@@UAAXXZ", end_frame),
    (813, "?BeginModalState@CWnd@@UAAXXZ", begin_window),
    (1136, "?EndModalState@CWnd@@UAAXXZ", end_window),
    (1891, "?OnEnable@CFrameWnd@@IAAXH@Z", on_enable),
    (
        1810,
        "?OnCancelMode@CWnd@@IAAXXZ",
        super::window_dispatch::last_message_default,
    ),
];
fn begin_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    window(ctx, false)
}
fn end_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    window(ctx, true)
}
fn window(ctx: &mut CallCtx<'_>, enabled: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 1, "EnableWindow", &[hwnd, u32::from(enabled)])
}
fn next(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let hwnd = f.get(ctx, 2)?;
    f.call_service(ctx, 3, "GetWindow", &[hwnd, 2])
}
fn begin_frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu.check_guest_access(
            object,
            232,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )?;
        let count = word(ctx, object + 0xb4)?
            .checked_add(1)
            .ok_or_else(|| KernelError::Dispatch("MFC modal counter overflow".into()))?;
        put(ctx, object + 0xb4, count)?;
        if count > 1 {
            return f.finish(ctx, 0);
        }
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, super::pretranslation::ROOT, &[hwnd]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            f.set(ctx, 0, result)?;
            f.call_service(ctx, 2, super::pretranslation::MAIN, &[])
        }
        2 => {
            let main = if result == 0 {
                0
            } else {
                word(ctx, result + 0x20)?
            };
            f.set(ctx, 1, main)?;
            f.call_service(ctx, 3, "GetWindow", &[main, 0])
        }
        3 if result == 0 => {
            let count = f.get(ctx, 3)?;
            if f.get(ctx, 4)? != 0 || count == 0 {
                return f.finish(ctx, 0);
            }
            let array = alloc_cpp(ctx, (count + 1) * 4)?;
            ctx.cpu
                .write_mem(array, &vec![0; ((count + 1) * 4) as usize])?;
            put(ctx, object + 0xb8, array)?;
            f.set(ctx, 4, 1)?;
            let main = f.get(ctx, 1)?;
            f.call_service(ctx, 3, "GetWindow", &[main, 0])
        }
        3 => {
            let visits = f.get(ctx, 7)?;
            if visits >= 8192 {
                return Err(KernelError::Dispatch(
                    "MFC modal window traversal exceeded its bound".into(),
                ));
            }
            f.set(ctx, 7, visits + 1)?;
            f.set(ctx, 2, result)?;
            if super::windows::permanent(ctx, result)?.is_none() {
                return next(ctx, f);
            }
            f.call_service(ctx, 4, "IsWindowEnabled", &[result])
        }
        4 if result == 0 => next(ctx, f),
        4 => {
            f.set(ctx, 8, 0)?;
            let hwnd = f.get(ctx, 2)?;
            f.set(ctx, 6, hwnd)?;
            owned(ctx, f, hwnd)
        }
        5 => owned(ctx, f, result),
        6 if result != 0 => next(ctx, f),
        6 => {
            if f.get(ctx, 4)? == 0 {
                let count = f.get(ctx, 3)?;
                if count >= 4096 {
                    return Err(KernelError::Dispatch(
                        "MFC modal window count exceeded its bound".into(),
                    ));
                }
                f.set(ctx, 3, count + 1)?;
                return next(ctx, f);
            }
            let hwnd = f.get(ctx, 2)?;
            f.call_service(ctx, 7, "EnableWindow", &[hwnd, 0])
        }
        7 => {
            let at = f.get(ctx, 5)?;
            if at >= f.get(ctx, 3)? {
                return Err(KernelError::Dispatch(
                    "MFC modal window set grew during callbacks".into(),
                ));
            }
            let array = word(ctx, object + 0xb8)?;
            let hwnd = f.get(ctx, 2)?;
            put(ctx, array + at * 4, hwnd)?;
            f.set(ctx, 5, at + 1)?;
            next(ctx, f)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC begin-modal continuation".into(),
        )),
    }
}
fn owned(ctx: &mut CallCtx<'_>, f: Frame, hwnd: u32) -> Result<DispatchOutcome, KernelError> {
    if hwnd == 0 {
        return next(ctx, f);
    }
    if hwnd == f.get(ctx, 0)? {
        let target = f.get(ctx, 2)?;
        return f.call_service(ctx, 6, "SendMessageW", &[target, 0x36c, 0, 0]);
    }
    let depth = f.get(ctx, 8)?;
    if depth >= 128 {
        return Err(KernelError::Dispatch(
            "MFC modal owner chain exceeded its bound".into(),
        ));
    }
    f.set(ctx, 8, depth + 1)?;
    f.call_service(ctx, 5, super::pretranslation::PARENT, &[hwnd])
}
fn end_frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let count = word(ctx, object + 0xb4)?;
        if count == 0 {
            return f.finish(ctx, 0);
        }
        put(ctx, object + 0xb4, count - 1)?;
        if count > 1 {
            return f.finish(ctx, 0);
        }
        let array = word(ctx, object + 0xb8)?;
        if array == 0 {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, array)?;
    }
    if !f.fresh && !matches!(f.phase(ctx)?, 1 | 2) {
        return Err(KernelError::Dispatch(
            "Invalid MFC end-modal continuation".into(),
        ));
    }
    if !f.fresh && f.phase(ctx)? == 1 && ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        let hwnd = f.get(ctx, 2)?;
        return f.call_service(ctx, 2, "EnableWindow", &[hwnd, 1]);
    }
    let at = f.get(ctx, 1)?;
    if at > 4096 {
        return Err(KernelError::Dispatch(
            "MFC saved modal HWND array exceeded its bound".into(),
        ));
    }
    let array = f.get(ctx, 0)?;
    let bytes = ctx
        .kernel
        .heap
        .msize(array)
        .ok_or_else(|| KernelError::Dispatch("MFC modal HWND allocation is missing".into()))?;
    if (at + 1) * 4 > bytes {
        return Err(KernelError::Dispatch(
            "MFC modal HWND allocation is truncated".into(),
        ));
    }

    if word(ctx, object + 0xb8)? != array {
        return Err(KernelError::Dispatch(
            "MFC modal HWND owner changed during callbacks".into(),
        ));
    }
    let hwnd = word(ctx, array + at * 4)?;
    if hwnd == 0 {
        put(ctx, object + 0xb8, 0)?;
        ctx.kernel.heap.free(array);
        return f.finish(ctx, 0);
    }
    f.set(ctx, 1, at + 1)?;
    f.set(ctx, 2, hwnd)?;
    f.call_service(ctx, 1, "IsWindow", &[hwnd])
}
fn on_enable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let enabled = f.arg(ctx, 1)?;
    if f.fresh {
        let flags = word(ctx, object + 0x2c)?;
        if enabled != 0 && flags & 4 != 0 {
            let hwnd = word(ctx, object + 0x20)?;
            return f.call_service(ctx, 4, "EnableWindow", &[hwnd, 0]);
        }
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, "GetParent", &[hwnd]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if super::windows::permanent(ctx, result)?.is_some() {
                return f.finish(ctx, 0);
            }
            let flags = word(ctx, object + 0x2c)?;
            let slot = if enabled == 0 {
                if word(ctx, object + 0xb4)? != 0 {
                    return f.finish(ctx, 0);
                }
                put(ctx, object + 0x2c, flags | 0x80)?;
                34
            } else {
                if flags & 0x80 == 0 {
                    return f.finish(ctx, 0);
                }
                put(ctx, object + 0x2c, flags & !0x80)?;
                35
            };
            let target = virtual_function(ctx, object, slot)?;
            f.call_args(ctx, 2, target, &[object])
        }
        2 if enabled == 0 => f.finish(ctx, 0),
        2 => f.call_service(ctx, 3, "GetActiveWindow", &[]),
        3 => {
            let hwnd = word(ctx, object + 0x20)?;
            if result == hwnd {
                return f.call_service(ctx, 5, "SendMessageW", &[hwnd, 6, 1, 0]);
            }
            f.finish(ctx, 0)
        }
        4 => f.call_service(ctx, 5, "SetFocus", &[0]),
        5 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame enable continuation".into(),
        )),
    }
}
