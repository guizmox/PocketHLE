//! Real exception metadata, owned deletion and error reporting through MFC UI.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (93, "??0CException@@QAA@H@Z", construct_flag),
    (94, "??0CException@@QAA@XZ", construct),
    (
        1283,
        "?GetErrorMessage@CException@@UAAHPAGIPAI@Z",
        empty_message,
    ),
    (
        1286,
        "?GetErrorMessage@CSimpleException@@UAAHPAGIPAI@Z",
        simple_message,
    ),
    (1640, "?InitString@CSimpleException@@QAAXXZ", init_string),
    (2223, "?ReportError@CException@@UAAHII@Z", report),
    (
        1468,
        "?GetRuntimeClass@CException@@UBAPAUCRuntimeClass@@XZ",
        runtime_base,
    ),
    (
        1496,
        "?GetRuntimeClass@CMemoryException@@UBAPAUCRuntimeClass@@XZ",
        runtime_memory,
    ),
    (
        1537,
        "?GetRuntimeClass@CUserException@@UBAPAUCRuntimeClass@@XZ",
        runtime_user,
    ),
];

fn runtime_base(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    runtime(ctx, "CException")
}
fn runtime_user(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    runtime(ctx, "CUserException")
}
fn runtime_memory(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    runtime(ctx, "CMemoryException")
}
fn runtime(ctx: &mut CallCtx<'_>, class: &str) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, class,
    )?))
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    new(ctx, 1)
}
fn construct_flag(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let flag = ctx.arg_u32(1)?;
    new(ctx, flag)
}
fn new(ctx: &mut CallCtx<'_>, flag: u32) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(p, 8, Prot::WRITE)?;
    let table = super::classes::vtable(ctx, "CException")?;
    put(ctx, p, table)?;
    put(ctx, p + 4, flag)?;
    Ok(DispatchOutcome::ReturnedR0(p))
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    ctx.cpu.check_guest_access(p, 8, Prot::READ)?;
    if flags & !1 != 0 {
        return Err(KernelError::Dispatch(
            "MFC exception vector destruction unsupported".into(),
        ));
    }
    if flags & 1 != 0 {
        ctx.kernel.heap.free(p);
    }
    Ok(DispatchOutcome::ReturnedR0(p))
}
fn empty_message(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let help = ctx.arg_u32(3)?;
    if help != 0 {
        put(ctx, help, 0)?;
    }
    if p != 0 && count != 0 {
        ctx.cpu.write_mem(p, &[0, 0])?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn initialize(ctx: &mut CallCtx<'_>, p: u32) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(p, 280, Prot::READ | Prot::WRITE)?;
    if word(ctx, p + 12)? == 0 {
        let id = word(ctx, p + 0x114)?;
        let text = super::resources::framework_string(ctx, id)?;
        let mut bytes: Vec<_> = text
            .into_iter()
            .take(127)
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let found = bytes.len() > 2;
        bytes.resize(256, 0);
        ctx.cpu.write_mem(p + 20, &bytes)?;
        put(ctx, p + 16, u32::from(found))?;
        put(ctx, p + 12, 1)?;
    }
    Ok(())
}
fn init_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    initialize(ctx, p)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn simple_message(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let help = ctx.arg_u32(3)?;
    initialize(ctx, p)?;
    if help != 0 {
        put(ctx, help, 0)?;
    }
    let found = word(ctx, p + 16)?;
    if count != 0 && out != 0 {
        let text = if found != 0 {
            super::strings::wide(ctx, p + 20)?
        } else {
            Vec::new()
        };
        let bytes: Vec<_> = text
            .into_iter()
            .take((count - 1) as usize)
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(out, &bytes)?;
    }
    Ok(DispatchOutcome::ReturnedR0(found))
}
fn report(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let buffer = alloc(ctx, 1032)?;
        f.own(ctx, buffer)?;
        f.set(ctx, 0, buffer)?;
        ctx.cpu.write_mem(buffer, &[0; 1032])?;
        let target = virtual_function(ctx, object, 5)?;
        return f.call_args(ctx, 1, target, &[object, buffer + 8, 512, buffer]);
    }
    if f.phase(ctx)? == 1 {
        let buffer = f.get(ctx, 0)?;
        let mut text = buffer + 8;
        let help = word(ctx, buffer)?;
        if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
            let id = f.arg(ctx, 2)?;
            let id = if id == 0 { 0xf020 } else { id };
            let resource = super::resources::framework_string(ctx, id)?;
            let buffer = f.get(ctx, 0)? + 8;
            let bytes: Vec<_> = resource
                .into_iter()
                .take(511)
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            ctx.cpu.write_mem(buffer, &bytes)?;
            text = buffer;
        }
        let target = service(ctx, super::message_box::TEXT)?;
        let style = f.arg(ctx, 1)?;
        return f.call_args(ctx, 2, target, &[text, style, help]);
    }
    let answer = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, answer)
}
