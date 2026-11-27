//! Dialog keyboard/mouse preprocessing delegates navigation to WinCE's dialog manager.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
const INPUT: &str = "__PocketHLE_MFC_PreTranslateInput";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2104, "?PreTranslateInput@CWnd@@QAAHPAUtagMSG@@@Z", input),
    (
        2105,
        "?PreTranslateMessage@CDialog@@UAAHPAUtagMSG@@@Z",
        translate,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", INPUT, input);
}
fn input(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    let object = f.arg(ctx, 0)?;
    let msg = f.arg(ctx, 1)?;
    ctx.cpu.check_guest_access(msg, 28, Prot::READ)?;
    let message = word(ctx, msg + 4)?;
    if !(0x100..=0x108).contains(&message) && !(0x200..=0x209).contains(&message) {
        return f.finish(ctx, 0);
    }
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 1, "IsDialogMessageW", &[hwnd, msg])
}
fn translate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let msg = f.arg(ctx, 1)?;
    if f.fresh {
        ctx.cpu.check_guest_access(msg, 28, Prot::READ)?;
        return f.call_service(ctx, 1, super::hierarchy::TOP, &[object]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result != 0 && word(ctx, result + 0x68)? != 0 {
                return f.finish(ctx, 0);
            }
            let message = word(ctx, msg + 4)?;
            let key = word(ctx, msg + 8)?;
            if message == 0x100 && matches!(key, 0x1b | 3) {
                let hwnd = word(ctx, msg)?;
                return f.call_service(ctx, 2, "GetWindowLongW", &[hwnd, (-16i32) as u32]);
            }
            forward(ctx, f, object, msg)
        }
        2 => {
            let hwnd = word(ctx, msg)?;
            let edit = ctx
                .kernel
                .window_classes
                .get(&hwnd)
                .is_some_and(|name| name.eq_ignore_ascii_case("edit"))
                || ctx
                    .kernel
                    .controls
                    .get(hwnd)
                    .is_some_and(|c| c.class == pocket_kernel::controls::ControlClass::Edit);
            if result & 4 == 0 || !edit {
                return forward(ctx, f, object, msg);
            }
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 3, "GetDlgItem", &[hwnd, 2])
        }
        3 if result != 0 => f.call_service(ctx, 4, "IsWindowEnabled", &[result]),
        3 => cancel(ctx, f, object),
        4 if result != 0 => forward(ctx, f, object, msg),
        4 => cancel(ctx, f, object),
        5 => f.finish(ctx, 1),
        6 => f.finish(ctx, result),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC dialog pretranslation continuation".into(),
        )),
    }
}
fn forward(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    object: u32,
    msg: u32,
) -> Result<DispatchOutcome, KernelError> {
    f.call_service(ctx, 6, INPUT, &[object, msg])
}
fn cancel(ctx: &mut CallCtx<'_>, f: Frame, object: u32) -> Result<DispatchOutcome, KernelError> {
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 5, "SendMessageW", &[hwnd, 0x111, 2, 0])
}
