//! CE safe-owner selection uses real parent/style/active-window queries.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const SAFE: &str = "__PocketHLE_MFC_SafeOwner";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[(
    1545,
    "?GetSafeOwner_@CWnd@@SAPAUHWND__@@PAU2@PAPAU2@@Z",
    safe,
)];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", SAFE, safe);
}
fn visit(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<(), KernelError> {
    let n = f.get(ctx, 3)?;
    if n >= 128 {
        return Err(KernelError::Dispatch(
            "MFC safe-owner parent traversal exceeded its bound".into(),
        ));
    }
    f.set(ctx, 3, n + 1)
}
fn start(ctx: &mut CallCtx<'_>, f: Frame, hwnd: u32) -> Result<DispatchOutcome, KernelError> {
    f.set(ctx, 0, hwnd)?;
    if hwnd == 0 {
        return finish(ctx, f, 0);
    }
    f.call_service(ctx, 2, "GetWindowLongW", &[hwnd, (-16i32) as u32])
}
fn finish(ctx: &mut CallCtx<'_>, f: Frame, owner: u32) -> Result<DispatchOutcome, KernelError> {
    let output = f.arg(ctx, 1)?;
    if output != 0 {
        put(ctx, output, 0)?;
    }
    f.finish(ctx, owner)
}
fn selected(ctx: &mut CallCtx<'_>, f: Frame, owner: u32) -> Result<DispatchOutcome, KernelError> {
    f.set(ctx, 2, owner)?;
    let output = f.arg(ctx, 1)?;
    let top = f.get(ctx, 1)?;
    if output == 0 || top == 0 || top == owner {
        return finish(ctx, f, owner);
    }
    f.call_service(ctx, 6, "IsWindowEnabled", &[top])
}
fn safe(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let output = f.arg(ctx, 1)?;
        if output != 0 {
            ctx.cpu.check_guest_access(output, 4, Prot::WRITE)?;
        }
        let hwnd = f.arg(ctx, 0)?;
        if hwnd != 0 {
            return start(ctx, f, hwnd);
        }
        let ts = super::application::current_thread_state(ctx)?;
        let routing = word(ctx, ts + 0x12c)?;
        if routing != 0 {
            let hwnd = word(ctx, routing + 0x20)?;
            return start(ctx, f, hwnd);
        }
        return f.call_service(ctx, 1, super::pretranslation::MAIN, &[]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            let hwnd = if result == 0 {
                0
            } else {
                word(ctx, result + 0x20)?
            };
            start(ctx, f, hwnd)
        }
        2 if result & 0x40000000 != 0 => {
            visit(ctx, &f)?;
            let hwnd = f.get(ctx, 0)?;
            f.call_service(ctx, 3, "GetParent", &[hwnd])
        }
        2 => {
            let hwnd = f.get(ctx, 0)?;
            f.set(ctx, 1, hwnd)?;
            visit(ctx, &f)?;
            f.call_service(ctx, 4, "GetParent", &[hwnd])
        }
        3 => start(ctx, f, result),
        4 if result != 0 && result != crate::coredll::FAKE_DESKTOP_HWND => {
            visit(ctx, &f)?;
            f.set(ctx, 1, result)?;
            f.call_service(ctx, 4, "GetParent", &[result])
        }
        4 => {
            let hwnd = f.get(ctx, 0)?;
            if f.arg(ctx, 0)? == 0 && hwnd != 0 {
                return f.call_service(ctx, 5, "GetActiveWindow", &[]);
            }
            selected(ctx, f, hwnd)
        }
        5 => selected(ctx, f, result),
        6 if result != 0 => {
            let top = f.get(ctx, 1)?;
            f.call_service(ctx, 7, "EnableWindow", &[top, 0])
        }
        7 => {
            let output = f.arg(ctx, 1)?;
            let top = f.get(ctx, 1)?;
            put(ctx, output, top)?;
            let owner = f.get(ctx, 2)?;
            f.finish(ctx, owner)
        }
        6 => {
            let owner = f.get(ctx, 2)?;
            finish(ctx, f, owner)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC safe-owner continuation".into(),
        )),
    }
}
