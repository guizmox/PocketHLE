//! CE3 TRY links live in guest stack memory and the existing per-thread context.
//! Throwing follows the same links through exception_throw.rs.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::Prot;
pub(super) const DELETE: &str = "__PocketHLE_MFC_DeleteException";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (33, "??0AFX_EXCEPTION_LINK@@QAA@XZ", construct),
    (768, "?AfxTryCleanup@@YAXXZ", cleanup),
    (1028, "?Delete@CException@@QAAXXZ", delete),
    (
        2649,
        "?AfxGetExceptionContext@@YAPAUAFX_EXCEPTION_CONTEXT@@XZ",
        context,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", DELETE, delete);
}
pub(super) fn context_address(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    Ok(super::application::current_thread_state(ctx)? + 0x10)
}
fn context(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(context_address(ctx)?))
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 56, Prot::READ | Prot::WRITE)?;
    let context = context_address(ctx)?;
    let previous = word(ctx, context)?;
    // Leave the caller's jmp_buf untouched: setjmp owns its register image.
    put(ctx, object + 0x2c, 0)?;
    put(ctx, object + 0x30, previous)?;
    put(ctx, object + 0x34, 0)?;
    put(ctx, context, object)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn cleanup(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let context = context_address(ctx)?;
        let link = word(ctx, context)?;
        if link == 0 {
            return Err(KernelError::Dispatch(
                "MFC TRY cleanup has no active link".into(),
            ));
        }
        ctx.cpu.check_guest_access(link, 56, Prot::READ)?;
        f.set(ctx, 0, context)?;
        f.set(ctx, 1, link)?;
        let exception = word(ctx, link + 0x34)?;
        if exception != 0 {
            let target = service(ctx, DELETE)?;
            return f.call_args(ctx, 1, target, &[exception]);
        }
    }
    let link = f.get(ctx, 1)?;
    let context = f.get(ctx, 0)?;
    let previous = word(ctx, link + 0x30)?;
    put(ctx, context, previous)?;
    f.finish(ctx, 0)
}
fn delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        ctx.cpu.check_guest_access(object, 8, Prot::READ)?;
        if word(ctx, object + 4)? as i32 > 0 {
            let target = virtual_function(ctx, object, 1)?;
            return f.call_args(ctx, 1, target, &[object, 1]);
        }
    }
    f.finish(ctx, 0)
}
/// Install a native macro TRY in an existing callback frame. The return alias
/// is the same one call_args will publish; longjmp resumes that exact frame.
pub(super) fn begin_catch(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    offset: u32,
) -> Result<(), KernelError> {
    if offset > 200 {
        return Err(KernelError::Dispatch("MFC TRY exceeds its frame".into()));
    }
    let context = context_address(ctx)?;
    let previous = word(ctx, context)?;
    let link = f.address + offset;
    ctx.cpu.write_arg(0, link)?;
    crate::coredll::setjmp(ctx)?;
    let sp = f.arg(ctx, 5)?;
    let home = ctx.cpu.stack_arg_offset();
    let reserved = ((home + 7) & !7).max(8);
    put(
        ctx,
        link + 32,
        sp.checked_sub(reserved)
            .ok_or_else(|| KernelError::Dispatch("MFC TRY stack underflow".into()))?,
    )?;
    let target = f.return_address(ctx)?;
    put(ctx, link + 36, target)?;
    put(ctx, link + 44, 0)?;
    put(ctx, link + 48, previous)?;
    put(ctx, link + 52, 0)?;
    put(ctx, context, link)
}
pub(super) fn unlink(ctx: &mut CallCtx<'_>, link: u32) -> Result<(), KernelError> {
    let context = context_address(ctx)?;
    if word(ctx, context)? != link {
        return Err(KernelError::Dispatch(
            "Unbalanced MFC exception links".into(),
        ));
    }
    let previous = word(ctx, link + 48)?;
    put(ctx, context, previous)
}

#[cfg(test)]
mod tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn try_links_preserve_jump_buffers_and_release_owned_exceptions_in_lifo_order() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        cpu.write_mem(0x2000, &[0x5a; 112]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:33",
            &[0x2000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:33",
            &[0x2038],
        );
        let DispatchOutcome::ReturnedR0(context) =
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:2649", &[])
        else {
            panic!()
        };
        assert_eq!(cpu.read_u32_le(context).unwrap(), 0x2038);
        k.current_thread = 1;
        let DispatchOutcome::ReturnedR0(other) =
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:2649", &[])
        else {
            panic!()
        };
        assert_ne!(context, other);
        assert_eq!(cpu.read_u32_le(other).unwrap(), 0);
        k.current_thread = 0;
        assert_eq!(cpu.read_u32_le(context).unwrap(), 0x2038);
        assert_eq!(cpu.read_mem(0x2000, 44).unwrap(), [0x5a; 44]);
        assert_eq!(cpu.read_mem(0x2038, 44).unwrap(), [0x5a; 44]);
        cpu.write_mem(0x2038 + 52, &0x2200u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2200, &0x2300u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2204, &1u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2304, &0x6100u32.to_le_bytes()).unwrap();
        let available = k.heap.free_bytes();
        let mut calls = 0;
        drive(&mut cpu, &mut k, &mut d, entry(768), |pc, cpu| {
            assert_eq!(pc, 0x6100);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
            assert_eq!(cpu.read_arg(1).unwrap(), 1);
            calls += 1;
            0
        });
        assert_eq!(calls, 1);
        assert_eq!(cpu.read_u32_le(context).unwrap(), 0x2000);
        cpu.write_mem(0x2000 + 52, &0x2200u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2204, &0u32.to_le_bytes()).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(768), |pc, _| {
            panic!("unexpected callback {pc:x}")
        });
        assert_eq!(cpu.read_u32_le(context).unwrap(), 0);
        assert_eq!(k.heap.free_bytes(), available);
    }
}
