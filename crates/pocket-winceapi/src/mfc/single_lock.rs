//! CSingleLock borrows a real CSyncObject and preserves its virtual wait/release.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (163, "??0CSingleLock@@QAA@PAVCSyncObject@@H@Z", construct),
    (1730, "?Lock@CSingleLock@@QAAHK@Z", lock),
    (2480, "?Unlock@CSingleLock@@QAAHJPAJ@Z", unlock_count),
    (2481, "?Unlock@CSingleLock@@QAAHXZ", unlock),
];
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if !f.fresh {
        let acquired = ctx.cpu.read_reg(ArmReg::R0)?;
        put(ctx, object + 8, acquired)?;
        return f.finish(ctx, object);
    }
    ctx.cpu
        .check_guest_access(object, 12, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let sync = f.arg(ctx, 1)?;
    let handle = word(ctx, sync + 4)?;
    put(ctx, object, sync)?;
    put(ctx, object + 4, handle)?;
    put(ctx, object + 8, 0)?;
    if f.arg(ctx, 2)? == 0 {
        return f.finish(ctx, object);
    }
    let target = virtual_function(ctx, sync, 5)?;
    f.call_args(ctx, 1, target, &[sync, u32::MAX])
}
fn lock(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if !f.fresh {
        let acquired = ctx.cpu.read_reg(ArmReg::R0)?;
        put(ctx, object + 8, acquired)?;
        return f.finish(ctx, acquired);
    }
    let sync = word(ctx, object)?;
    let timeout = f.arg(ctx, 1)?;
    let target = virtual_function(ctx, sync, 5)?;
    f.call_args(ctx, 1, target, &[sync, timeout])
}
fn unlock(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    release(ctx, false)
}
fn unlock_count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    release(ctx, true)
}
fn release(ctx: &mut CallCtx<'_>, counted: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if !f.fresh {
        let released = ctx.cpu.read_reg(ArmReg::R0)? != 0;
        put(ctx, object + 8, u32::from(!released))?;
        return f.finish(ctx, u32::from(released));
    }
    if word(ctx, object + 8)? == 0 {
        return f.finish(ctx, 1);
    }
    let sync = word(ctx, object)?;
    let target = virtual_function(ctx, sync, if counted { 6 } else { 7 })?;
    let args = if counted {
        vec![sync, f.arg(ctx, 1)?, f.arg(ctx, 2)?]
    } else {
        vec![sync]
    };
    f.call_args(ctx, 1, target, &args)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn event_lock_waits_on_the_real_handle_and_does_not_own_it() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (i, value) in [0x2000u32, 0, 1, 0].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        cpu.write_mem(0x4000, &0u32.to_le_bytes()).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(92), |pc, _| {
            panic!("unexpected guest {pc:x}")
        });
        let handle = cpu.read_u32_le(0x2004).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:163",
            &[0x2200, 0x2000, 0],
        );
        cpu.write_arg(0, 0x2200).unwrap();
        cpu.write_arg(1, 0).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1730), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            0
        );
        assert_eq!(cpu.read_u32_le(0x2208).unwrap(), 0);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SetEvent",
                &[handle]
            )),
            1
        );
        cpu.write_arg(0, 0x2200).unwrap();
        cpu.write_arg(1, 0).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1730), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            1
        );
        assert_eq!(cpu.read_u32_le(0x2208).unwrap(), 1);
        cpu.write_arg(0, 0x2200).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(2481), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            1
        );
        assert_eq!(cpu.read_u32_le(0x2208).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), handle);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "ResetEvent",
                &[handle]
            )),
            1
        );
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
    #[test]
    fn failed_virtual_release_preserves_acquired_state_and_counted_arguments() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (at, value) in [
            (0x2000, 0x2600u32),
            (0x2004, 123),
            (0x2618, 0x6100),
            (0x261c, 0x6200),
        ] {
            cpu.write_mem(at, &value.to_le_bytes()).unwrap();
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:163",
            &[0x2200, 0x2000, 0],
        );
        cpu.write_mem(0x2208, &1u32.to_le_bytes()).unwrap();
        let free = k.heap.free_bytes();
        for (ordinal, expected_pc, answer) in [(2480, 0x6100, 0), (2481, 0x6200, 1)] {
            for (i, value) in [0x2200u32, 3, 0x2400].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(ordinal), |pc, cpu| {
                    assert_eq!(pc, expected_pc);
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                    if ordinal == 2480 {
                        assert_eq!(cpu.read_arg(1).unwrap(), 3);
                        assert_eq!(cpu.read_arg(2).unwrap(), 0x2400);
                    }
                    answer
                }),
                answer
            );
            assert_eq!(cpu.read_u32_le(0x2208).unwrap(), u32::from(answer == 0));
            assert_eq!(k.heap.free_bytes(), free);
        }
    }
}
