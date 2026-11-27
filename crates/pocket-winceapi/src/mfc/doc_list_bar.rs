//! CE command-bar New routing preserves the real application's command chain.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2765, "?OnSharedNew@CCeCommandBar@@MAAXXZ", shared_new),
    (2766, "?OnSharedNew@CCeDocListCB@@MAAXXZ", doc_new),
    (
        2747,
        "?OnCmdMsg@CCeDocListCB@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
        command,
    ),
];
fn app(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let module = application::state(ctx)?;
    let app = word(ctx, module + 4)?;
    if app == 0 {
        return Err(KernelError::Dispatch(
            "Document command has no MFC application".into(),
        ));
    }
    Ok(app)
}
fn shared_new(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let app = app(ctx)?;
        return f.call_virtual(ctx, 1, app, 5, &[0xe100, 0, 0, 0]);
    }
    f.finish(ctx, 0)
}
fn doc_new(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let bar = f.arg(ctx, 0)?;
    if f.fresh {
        let app = app(ctx)?;
        return f.call_virtual(ctx, 1, app, 29, &[]);
    }
    match f.phase(ctx)? {
        1 => {
            let main = ctx.cpu.read_reg(ArmReg::R0)?;
            let hwnd = word(ctx, main + 0x20)?;
            f.call_service(ctx, 2, "PostMessageW", &[hwnd, 0x111, 0xe100, 0])
        }
        2 => {
            let owner = word(ctx, bar + 0xe0)?;
            f.call_virtual(ctx, 3, owner, 47, &[])
        }
        _ => f.finish(ctx, 0),
    }
}
fn command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let bar = f.arg(ctx, 0)?;
    let id = f.arg(ctx, 1)?;
    let code = f.arg(ctx, 2)?;
    let extra = f.arg(ctx, 3)?;
    if f.fresh {
        let info = ctx.arg_u32(4)?;
        f.set(ctx, 0, info)?;
        let app = app(ctx)?;
        return f.call_virtual(ctx, 1, app, 29, &[]);
    }
    let info = f.get(ctx, 0)?;
    if f.phase(ctx)? == 1 && code != 10 {
        let main = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.call_virtual(ctx, 2, main, 5, &[id, code, extra, info]);
    }
    if f.phase(ctx)? == 2 && ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        return f.finish(ctx, 1);
    }
    if f.phase(ctx)? <= 2 {
        return f.call_service(ctx, 3, command_routing::BASE, &[bar, id, code, extra, info]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, result)
}
