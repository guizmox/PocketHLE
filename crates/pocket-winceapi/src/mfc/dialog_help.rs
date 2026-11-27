//! Help availability queries actual command handlers without executing them.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const ENABLED: &str = "__PocketHLE_MFC_HelpEnabled";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] =
    &[(653, "?AfxHelpEnabled@@YAHXZ", enabled)];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", ENABLED, enabled);
}
fn query(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    phase: u32,
    object: u32,
) -> Result<DispatchOutcome, KernelError> {
    let target = virtual_function(ctx, object, 5)?;
    f.call_args(ctx, phase, target, &[object, 0xe146, 0, 0, f.address + 128])
}
fn enabled(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let module = super::application::state(ctx)?;
        let app = word(ctx, module + 4)?;
        if app == 0 {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, app)?;
        let target = service(ctx, super::pretranslation::MAIN)?;
        return f.call_args(ctx, 1, target, &[]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 if result != 0 => query(ctx, f, 2, result),
        1 | 2 if result == 0 => {
            let app = f.get(ctx, 0)?;
            query(ctx, f, 3, app)
        }
        2 => f.finish(ctx, 1),
        3 => f.finish(ctx, result),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC help-query continuation".into(),
        )),
    }
}
