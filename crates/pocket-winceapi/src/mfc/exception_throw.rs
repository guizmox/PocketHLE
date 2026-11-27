//! CE3 macro exceptions walk guest TRY/cleanup links and use the ARM CRT jump ABI.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const MEMORY: &str = "__PocketHLE_MFC_ThrowMemory";
pub(super) const THROW: &str = "__PocketHLE_MFC_Throw";
pub(super) const RESOURCE: &str = "__PocketHLE_MFC_ThrowResource";
pub(super) const USER: &str = "__PocketHLE_MFC_ThrowUser";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (710, "?AfxThrowResourceException@@YAXXZ", resource),
    (704, "?AfxThrow@@YAXPAVCException@@@Z", throw),
    (708, "?AfxThrowMemoryException@@YAXXZ", memory),
    (711, "?AfxThrowUserException@@YAXXZ", user),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", RESOURCE, resource);
    d.register_handler("coredll.dll", THROW, throw);
    d.register_handler("coredll.dll", MEMORY, memory);
    d.register_handler("coredll.dll", USER, user);
}
fn resource(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    singleton(ctx, "CResourceException", 0xf022)
}
fn user(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    singleton(ctx, "CUserException", 0xf024)
}
fn memory(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    singleton(ctx, "CMemoryException", 0xf023)
}
fn singleton(ctx: &mut CallCtx<'_>, class: &str, id: u32) -> Result<DispatchOutcome, KernelError> {
    let table = super::classes::vtable(ctx, class)?;
    let (p, fresh) =
        super::application::allocation(ctx, &format!("__PocketHLE_MFC_Static:{class}"), 280)?;
    if fresh {
        put(ctx, p, table)?;
        put(ctx, p + 0x114, id)?;
        // The retail static simple exceptions have m_bAutoDelete == FALSE.
    }
    ctx.cpu.write_arg(0, p)?;
    throw(ctx)
}
fn throw(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let context = super::exception_links::context_address(ctx)?;
        let mut exception = f.arg(ctx, 0)?;
        if exception == 0 {
            let head = word(ctx, context)?;
            if head == 0 {
                return Err(KernelError::Dispatch(
                    "MFC rethrow without an active exception".into(),
                ));
            }
            exception = word(ctx, head + 0x34)?;
        }
        if exception == 0 {
            return Err(KernelError::Dispatch(
                "MFC throw has no exception object".into(),
            ));
        }
        ctx.cpu.check_guest_access(exception, 8, Prot::READ)?;
        f.set(ctx, 0, context)?;
        f.set(ctx, 1, exception)?;
        let module = super::application::state(ctx)?;
        let app = word(ctx, module + 4)?;
        if app != 0 {
            let target = virtual_function(ctx, app, 36)?;
            return f.call_args(ctx, 1, target, &[app, 0x80000000]);
        }
    } else {
        match f.phase(ctx)? {
            2 => pop(ctx, &f)?,
            3 => {
                let context = f.get(ctx, 0)?;
                if word(ctx, context)? == f.get(ctx, 2)? {
                    return Err(KernelError::Dispatch(
                        "MFC unwind callback did not remove its link".into(),
                    ));
                }
            }
            1 => (),
            _ => {
                return Err(KernelError::Dispatch(
                    "Invalid MFC throw continuation".into(),
                ))
            }
        }
    }
    walk(ctx, f)
}
fn pop(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<(), KernelError> {
    let link = f.get(ctx, 2)?;
    put(ctx, link + 0x34, 0)?;
    let previous = word(ctx, link + 0x30)?;
    let context = f.get(ctx, 0)?;
    put(ctx, context, previous)
}
fn walk(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    // Validation is synchronous, so no host traversal state must survive a
    // callback or snapshot. Revalidate after each real cleanup/deletion call.
    let context = f.get(ctx, 0)?;
    let mut link = word(ctx, context)?;
    let mut seen = std::collections::HashSet::new();
    while link != 0 {
        if !seen.insert(link) || seen.len() > 256 {
            return Err(KernelError::Dispatch(
                "Cyclic/excessive MFC exception chain".into(),
            ));
        }
        ctx.cpu.check_guest_access(link, 56, Prot::READ)?;
        link = word(ctx, link + 48)?;
    }
    for _ in 0..256 {
        let context = f.get(ctx, 0)?;
        let link = word(ctx, context)?;
        let exception = f.get(ctx, 1)?;
        if link == 0 {
            return Err(KernelError::Dispatch(format!(
                "Unhandled MFC exception at 0x{exception:08x}"
            )));
        }
        ctx.cpu
            .check_guest_access(link, 56, Prot::READ | Prot::WRITE)?;
        let count = f.get(ctx, 3)?;
        if count >= 256 {
            return Err(KernelError::Dispatch(
                "Cyclic/excessive MFC exception chain".into(),
            ));
        }
        f.set(ctx, 3, count + 1)?;
        f.set(ctx, 2, link)?;
        let old = word(ctx, link + 0x34)?;
        if old != 0 {
            if old != exception {
                let target = service(ctx, super::exception_links::DELETE)?;
                return f.call_args(ctx, 2, target, &[old]);
            }
            pop(ctx, &f)?;
            continue;
        }
        if word(ctx, link + 0x2c)? != 0 {
            let target = word(ctx, link)?;
            return f.call_args(ctx, 3, target, &[link]);
        }
        // Validate the jump before publishing the exception or freeing frames.
        let target_sp = word(ctx, link + 32)?;
        let target_pc = word(ctx, link + 36)?;
        let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
        if target_sp < sp || target_pc == 0 {
            return Err(KernelError::Dispatch(
                "Invalid MFC nonlocal return target".into(),
            ));
        }
        ctx.cpu.check_guest_access(target_sp, 4, Prot::READ)?;
        put(ctx, link + 0x34, exception)?;
        ctx.cpu.write_arg(0, link)?;
        ctx.cpu.write_arg(1, 1)?;
        return crate::coredll::longjmp(ctx);
    }
    Err(KernelError::Dispatch(
        "MFC exception chain exceeds its bound".into(),
    ))
}
