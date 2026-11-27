//! Runtime ancestry follows actual guest class/getter callbacks, not class names.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const DERIVED: &str = "__PocketHLE_MFC_IsDerivedFrom";
pub(super) const CREATE: &str = "__PocketHLE_MFC_RuntimeCreateObject";
pub(super) const KIND: &str = "__PocketHLE_MFC_IsKindOf";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", KIND, kind);
    d.register_handler("coredll.dll", CREATE, create);
    d.register_handler("coredll.dll", DERIVED, derived);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        975,
        "?CreateObject@CRuntimeClass@@QAAPAVCObject@@XZ",
        create,
    ),
    (1687, "?IsDerivedFrom@CRuntimeClass@@QBAHPBU1@@Z", derived),
    (1700, "?IsKindOf@CObject@@QBAHPBUCRuntimeClass@@@Z", kind),
];
fn kind(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if frame.fresh {
        let object = frame.arg(ctx, 0)?;
        let target = virtual_function(ctx, object, 0)?;
        return frame.call_args(ctx, 1, target, &[object]);
    }
    let descriptor = ctx.cpu.read_reg(ArmReg::R0)?;
    walk(ctx, frame, descriptor)
}
fn derived(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let descriptor = if frame.fresh {
        frame.arg(ctx, 0)?
    } else {
        ctx.cpu.read_reg(ArmReg::R0)?
    };
    walk(ctx, frame, descriptor)
}
fn walk(
    ctx: &mut CallCtx<'_>,
    frame: Frame,
    descriptor: u32,
) -> Result<DispatchOutcome, KernelError> {
    if descriptor == 0 {
        return frame.finish(ctx, 0);
    }
    ctx.cpu
        .check_guest_access(descriptor, 24, pocket_cpu::Prot::READ)?;
    if descriptor == frame.arg(ctx, 1)? {
        return frame.finish(ctx, 1);
    }
    let count = frame.get(ctx, 0)?;
    if count >= 32 {
        return Err(KernelError::Dispatch(
            "MFC runtime-class ancestry exceeds its bound".into(),
        ));
    }
    for index in 0..count {
        if word(ctx, frame.address + 96 + index * 4)? == descriptor {
            return Err(KernelError::Dispatch(
                "Cyclic MFC runtime-class ancestry".into(),
            ));
        }
    }
    put(ctx, frame.address + 96 + count * 4, descriptor)?;
    frame.set(ctx, 0, count + 1)?;
    let getter = word(ctx, descriptor + 16)?;
    frame.call_args(ctx, 2, getter, &[])
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn ancestry_preserves_guest_runtime_overrides_and_callable_base_getters() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let base = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1542",
            &[],
        ));
        let original = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(original, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &bytes).unwrap();
        cpu.write_mem(0x2400, &0x6000u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2800, &[0; 24]).unwrap();
        cpu.write_mem(0x2810, &0x6100u32.to_le_bytes()).unwrap();
        let free = k.heap.free_bytes();
        let mut calls = Vec::new();
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, base).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1700), |pc, cpu| {
                calls.push(pc);
                match pc {
                    0x6000 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                        0x2800
                    }
                    0x6100 => base,
                    _ => panic!("unexpected {pc:x}"),
                }
            }),
            1
        );
        assert_eq!(calls, [0x6000, 0x6100]);
        assert_eq!(k.heap.free_bytes(), free);
        // The same descriptor chain is also queryable without an object/vtable.
        cpu.write_arg(0, 0x2800).unwrap();
        cpu.write_arg(1, base).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1687), |pc, _| {
                assert_eq!(pc, 0x6100);
                base
            }),
            1
        );
        cpu.write_arg(0, 0x2800).unwrap();
        cpu.write_arg(1, 0x2900).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1687), |pc, _| {
                assert_eq!(pc, 0x6100);
                0
            }),
            0
        );
        assert_eq!(k.heap.free_bytes(), free);
    }
}

/// Invoke the descriptor's real factory with the CE3 TRY boundary. The factory
/// owns construction; neither its size nor its C++ dynamic type is guessed.
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let class = f.arg(ctx, 0)?;
        ctx.cpu
            .check_guest_access(class, 24, pocket_cpu::Prot::READ)?;
        let target = word(ctx, class + 12)?;
        if target == 0 {
            return f.finish(ctx, 0);
        }
        super::exception_links::begin_catch(ctx, &f, 160)?;
        return f.call_args(ctx, 1, target, &[]);
    }
    match f.phase(ctx)? {
        1 => {
            let exception = word(ctx, f.address + 160 + 52)?;
            if exception != 0 {
                let target = super::callbacks::service(ctx, super::exception_links::DELETE)?;
                return f.call_args(ctx, 2, target, &[exception]);
            }
            let result = ctx.cpu.read_reg(ArmReg::R0)?;
            super::exception_links::unlink(ctx, f.address + 160)?;
            f.finish(ctx, result)
        }
        2 => {
            super::exception_links::unlink(ctx, f.address + 160)?;
            f.finish(ctx, 0)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC runtime factory continuation".into(),
        )),
    }
}
