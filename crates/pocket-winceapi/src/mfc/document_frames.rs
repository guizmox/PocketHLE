//! Frame initialization uses the real descendant tree, view and activation virtuals.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const INITIAL: &str = "__PocketHLE_MFC_InitialUpdateFrame";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1634,
        "?InitialUpdateFrame@CFrameWnd@@QAAXPAVCDocument@@H@Z",
        initial,
    ),
    (558, "?ActivateFrame@CFrameWnd@@UAAXH@Z", activate),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", INITIAL, initial);
}
fn finish_initial(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let doc = f.arg(ctx, 1)?;
    if doc != 0 {
        let target = virtual_function(ctx, doc, 41)?;
        f.call_args(ctx, 10, target, &[doc])
    } else {
        title(ctx, f)
    }
}
fn title(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let o = f.arg(ctx, 0)?;
    let target = virtual_function(ctx, o, 55)?;
    f.call_args(ctx, 11, target, &[o, 1])
}
fn show_initial(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if f.arg(ctx, 2)? == 0 {
        return finish_initial(ctx, f);
    }
    let o = f.arg(ctx, 0)?;
    let hwnd = word(ctx, o + 0x20)?;
    f.call_service(
        ctx,
        3,
        super::descendants::TARGET,
        &[hwnd, 0x364, 0, 0, 1, 1],
    )
}
fn initial(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        if word(ctx, o + 0xac)? != 0 {
            return show_initial(ctx, f);
        }
        let hwnd = word(ctx, o + 0x20)?;
        let view = super::descendants::find_permanent(ctx, hwnd, 0xe900)?;
        if view == 0 {
            return show_initial(ctx, f);
        }
        f.set(ctx, 0, view)?;
        let class = super::classes::runtime(ctx, "CView")?;
        return f.call_service(ctx, 1, super::class_queries::KIND, &[view, class]);
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                f.set(ctx, 0, 0)?;
                return show_initial(ctx, f);
            }
            let view = f.get(ctx, 0)?;
            f.call_service(ctx, 2, super::views::ACTIVE, &[o, view, 0])
        }
        2 => show_initial(ctx, f),
        3 => {
            let view = f.get(ctx, 0)?;
            if view != 0 {
                let target = virtual_function(ctx, view, 52)?;
                return f.call_args(ctx, 4, target, &[view, 0, o]);
            }
            activate_initial(ctx, f)
        }
        4 => activate_initial(ctx, f),
        5 => {
            let view = f.get(ctx, 0)?;
            if view != 0 {
                let target = virtual_function(ctx, view, 51)?;
                return f.call_args(ctx, 6, target, &[view, 1, view, view]);
            }
            finish_initial(ctx, f)
        }
        6 => finish_initial(ctx, f),
        10 => title(ctx, f),
        11 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid frame initial update continuation".into(),
        )),
    }
}
fn activate_initial(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let o = f.arg(ctx, 0)?;
    let module = super::application::state(ctx)?;
    let app = word(ctx, module + 4)?;
    let mut show = u32::MAX;
    if app != 0 && word(ctx, app + 0x20)? == o {
        show = word(ctx, app + 0x74)?;
        put(ctx, app + 0x74, u32::MAX)?;
    }
    let target = virtual_function(ctx, o, 51)?;
    f.call_args(ctx, 5, target, &[o, show])
}
fn activate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let hwnd = word(ctx, o + 0x20)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let show = f.arg(ctx, 1)?;
        f.set(ctx, 0, show)?;
        if show == u32::MAX {
            return f.call_service(ctx, 1, "IsWindowVisible", &[hwnd]);
        }
        return f.call_service(ctx, 3, "ShowWindow", &[hwnd, show]);
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                f.set(ctx, 0, 1)?;
                return f.call_service(ctx, 3, "ShowWindow", &[hwnd, 1]);
            }
            f.call_service(ctx, 2, "IsIconic", &[hwnd])
        }
        2 => {
            let show = if result == 0 { u32::MAX } else { 13 };
            f.set(ctx, 0, show)?;
            if show == u32::MAX {
                return f.finish(ctx, 0);
            }
            f.call_service(ctx, 3, "ShowWindow", &[hwnd, show])
        }
        3 => f.call_service(ctx, 4, "SetActiveWindow", &[hwnd]),
        4 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid frame activation continuation".into(),
        )),
    }
}
