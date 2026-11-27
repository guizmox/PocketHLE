//! CSyncObject uses the same waitable resources and scheduling as WinCE APIs.
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        92,
        "??0CEvent@@QAA@HHPBGPAU_SECURITY_ATTRIBUTES@@@Z",
        construct_event,
    ),
    (
        1467,
        "?GetRuntimeClass@CEvent@@UBAPAUCRuntimeClass@@XZ",
        runtime_event,
    ),
    (241, "??1CEvent@@UAA@XZ", destruct),
    (2476, "?Unlock@CEvent@@UAAHXZ", unlock_event),
    (184, "??0CSyncObject@@QAA@PBG@Z", construct),
    (300, "??1CSyncObject@@UAA@XZ", destruct),
    (
        1530,
        "?GetRuntimeClass@CSyncObject@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (1731, "?Lock@CSyncObject@@UAAHK@Z", lock),
];
fn construct_event(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::Frame;
    use pocket_cpu::regs::ArmReg;
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    if !frame.fresh {
        let handle = ctx.cpu.read_reg(ArmReg::R0)?;
        if handle == 0 {
            return Err(KernelError::Dispatch(
                "MFC CEvent could not create its WinCE event".into(),
            ));
        }
        put(ctx, object + 4, handle)?;
        return frame.finish(ctx, object);
    }
    construct(ctx)?;
    let table = super::classes::vtable(ctx, "CEvent")?;
    put(ctx, object, table)?;
    let security = ctx.arg_u32(4)?;
    let args = [
        security,
        frame.arg(ctx, 2)?,
        frame.arg(ctx, 1)?,
        frame.arg(ctx, 3)?,
    ];
    let target = super::callbacks::service(ctx, "CreateEventW")?;
    frame.call_args(ctx, 1, target, &args)
}
fn runtime_event(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CEvent",
    )?))
}
fn unlock_event(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 8, pocket_cpu::Prot::READ)?;
    // CE3 CEvent::Unlock deliberately leaves the event state untouched and
    // returns TRUE. SetEvent/ResetEvent own signaling; Unlock is not SetEvent.
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 8, pocket_cpu::Prot::WRITE)?;
    let table = super::classes::vtable(ctx, "CSyncObject")?;
    put(ctx, object, table)?;
    put(ctx, object + 4, 0)?;
    // The CE base constructor does not consume the optional name; derived
    // synchronization classes pass it to their real create/open operation.
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 8, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let table = super::classes::vtable(ctx, "CObject")?;
    let handle = word(ctx, object + 4)?;
    if handle != 0 {
        crate::coredll::close_handle_value(ctx, handle)?;
        put(ctx, object + 4, 0)?;
    }
    put(ctx, object, table)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CSyncObject",
    )?))
}
fn lock(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::Frame;
    use pocket_cpu::regs::ArmReg;
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        let wait = ctx.cpu.read_reg(ArmReg::R0)?;
        return frame.finish(ctx, u32::from(wait == 0));
    }
    let object = frame.arg(ctx, 0)?;
    let timeout = frame.arg(ctx, 1)?;
    let handle = word(ctx, object + 4)?;
    let owner = super::application::owner(ctx)?;
    let target = *ctx.kernel.dynamic_exports[&owner]
        .get("WaitForSingleObject")
        .ok_or_else(|| KernelError::Dispatch("WinCE wait service export missing".into()))?;
    frame.call_args(ctx, 1, target, &[handle, timeout])
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::DispatchOutcome;
    #[test]
    fn base_object_waits_on_a_real_event_and_destruction_closes_its_owned_handle() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:184",
            &[0x2000, 0],
        );
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 0);
        let descriptor = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1530",
            &[0x2000],
        ));
        assert_eq!(cpu.read_u32_le(descriptor + 4).unwrap(), 8);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:300",
            &[0x2000],
        );
        let baseline = k.heap.free_bytes();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:184",
            &[0x2000, 0],
        );
        let handle = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreateEventW",
            &[0, 1, 1, 0],
        ));
        assert_ne!(handle, 0);
        cpu.write_mem(0x2004, &handle.to_le_bytes()).unwrap();
        let target = k.dynamic_exports[&1]["WaitForSingleObject"];
        let jump = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1731",
            &[0x2000, 0],
        );
        assert!(matches!(jump,DispatchOutcome::JumpTo(p) if p==target));
        let wait = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "WaitForSingleObject",
            &[],
        ));
        assert_eq!(wait, 0);
        cpu.write_reg(ArmReg::R0, wait).unwrap();
        super::super::callbacks::return_from_callback(&mut cpu, &mut k, &mut d);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1731",
                &[]
            )),
            1
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:241",
            &[0x2000],
        );
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 0);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "WaitForSingleObject",
                &[handle, 0]
            )),
            u32::MAX
        );
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

#[cfg(test)]
mod event_tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn event_constructor_preserves_manual_reset_and_inherited_destructor_closes_the_handle() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        for (i, value) in [0x2000, 1, 1, 0].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        cpu.write_mem(0x4000, &0u32.to_le_bytes()).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(92), |pc, _| panic!(
                "Unexpected callback {pc:x}"
            )),
            0x2000
        );
        let handle = cpu.read_u32_le(0x2004).unwrap();
        assert_ne!(handle, 0);
        for _ in 0..2 {
            assert!(matches!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "WaitForSingleObject",
                    &[handle, 0]
                ),
                pocket_kernel::DispatchOutcome::ReturnedR0(0)
            ));
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "ResetEvent",
            &[handle],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2476",
            &[0x2000],
        );
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "WaitForSingleObject",
                &[handle, 0]
            ),
            pocket_kernel::DispatchOutcome::ReturnedR0(258)
        ));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:300",
            &[0x2000],
        );
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 0);
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "WaitForSingleObject",
                &[handle, 0]
            ),
            pocket_kernel::DispatchOutcome::ReturnedR0(u32::MAX)
        ));
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
}
