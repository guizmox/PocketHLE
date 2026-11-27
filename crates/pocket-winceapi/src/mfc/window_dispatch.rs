//! CE3 CWnd dispatch uses guest virtuals and the existing WinCE procedures.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2577, "?WindowProc@CWnd@@MAAJIIJ@Z", window_proc),
    (1024, "?DefWindowProcW@CWnd@@MAAJIIJ@Z", default_proc),
    (1695, "?IsFrameWnd@CFrameWnd@@UBAHXZ", is_frame),
    (1696, "?IsFrameWnd@CWnd@@UBAHXZ", is_not_frame),
    (1021, "?Default@CWnd@@IAAJXZ", last_message_default),
    (
        1573,
        "?GetSuperWndProcAddr@CWnd@@MAAPAP6AJPAUHWND__@@IIJ@ZXZ",
        super_address,
    ),
    (
        2098,
        "?PreCreateWindow@CWnd@@UAAHAAUtagCREATESTRUCTW@@@Z",
        pre_create,
    ),
    (2103, "?PreSubclassWindow@CWnd@@UAAXXZ", empty_base_hook),
    (2088, "?PostNcDestroy@CWnd@@MAAXXZ", empty_base_hook),
];

fn window_proc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    if frame.fresh {
        let result = frame.address + 64;
        put(ctx, result, 0)?;
        let args = [
            object,
            frame.arg(ctx, 1)?,
            frame.arg(ctx, 2)?,
            frame.arg(ctx, 3)?,
            result,
        ];
        let target = virtual_function(ctx, object, 39)?;
        return frame.call_args(ctx, 1, target, &args);
    }
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    match frame.phase(ctx)? {
        1 if returned != 0 => {
            let result = word(ctx, frame.address + 64)?;
            frame.finish(ctx, result)
        }
        1 => {
            let args = [
                object,
                frame.arg(ctx, 1)?,
                frame.arg(ctx, 2)?,
                frame.arg(ctx, 3)?,
            ];
            let target = virtual_function(ctx, object, 40)?;
            frame.call_args(ctx, 2, target, &args)
        }
        2 => frame.finish(ctx, returned),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC WindowProc continuation".into(),
        )),
    }
}

pub(super) fn last_message_default(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return frame.finish(ctx, result);
    }
    let object = frame.arg(ctx, 0)?;
    let message = super::application::current_thread_state(ctx)? + 0x3c;
    let args = [
        object,
        word(ctx, message + 4)?,
        word(ctx, message + 8)?,
        word(ctx, message + 12)?,
    ];
    let target = virtual_function(ctx, object, 40)?;
    frame.call_args(ctx, 1, target, &args)
}

fn default_proc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let procedure = if frame.fresh {
        let procedure = word(ctx, object + 0x30)?;
        if procedure == 0 {
            let target = virtual_function(ctx, object, 32)?;
            return frame.call_args(ctx, 1, target, &[object]);
        }
        procedure
    } else {
        match frame.phase(ctx)? {
            1 => {
                let pointer = ctx.cpu.read_reg(ArmReg::R0)?;
                word(ctx, pointer)?
            }
            2 => {
                let result = ctx.cpu.read_reg(ArmReg::R0)?;
                return frame.finish(ctx, result);
            }
            _ => {
                return Err(KernelError::Dispatch(
                    "Invalid MFC DefWindowProc continuation".into(),
                ))
            }
        }
    };
    let mut args = vec![
        word(ctx, object + 0x20)?,
        frame.arg(ctx, 1)?,
        frame.arg(ctx, 2)?,
        frame.arg(ctx, 3)?,
    ];
    let target = if procedure == 0 {
        service(ctx, "DefWindowProcW")?
    } else {
        args.insert(0, procedure);
        service(ctx, "CallWindowProcW")?
    };
    frame.call_args(ctx, 2, target, &args)
}

fn super_address(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 72, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    Ok(DispatchOutcome::ReturnedR0(object + 0x30))
}

fn pre_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let create = ctx.arg_u32(1)?;
    ctx.cpu
        .check_guest_access(object, 72, pocket_cpu::Prot::READ)?;
    ctx.cpu
        .check_guest_access(create, 48, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    if word(ctx, create + 40)? == 0 {
        let class = super::frame_loading::standard_class(ctx, false)?;
        put(ctx, create + 40, class)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn empty_base_hook(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 72, pocket_cpu::Prot::READ)?;
    // The retail CE3 base hooks have an empty void body. Overrides are still
    // reached through the object's vtable by the caller, never bypassed here.
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    use pocket_kernel::{Dispatcher, Thunk};

    pub(crate) fn drive(
        cpu: &mut dyn Cpu,
        kernel: &mut pocket_kernel::KernelState,
        dispatcher: &mut WinCeDispatcher,
        start: Thunk,
        mut guest: impl FnMut(u32, &mut dyn Cpu) -> u32,
    ) -> u32 {
        drive_with(cpu, kernel, dispatcher, start, 256, |pc, cpu, _, _| {
            guest(pc, cpu)
        })
    }
    pub(crate) fn drive_with(
        cpu: &mut dyn Cpu,
        kernel: &mut pocket_kernel::KernelState,
        dispatcher: &mut WinCeDispatcher,
        start: Thunk,
        budget: usize,
        mut guest: impl FnMut(
            u32,
            &mut dyn Cpu,
            &mut pocket_kernel::KernelState,
            &mut WinCeDispatcher,
        ) -> u32,
    ) -> u32 {
        drive_outcomes(
            cpu,
            kernel,
            dispatcher,
            start,
            budget,
            |pc, cpu, kernel, dispatcher| {
                DispatchOutcome::ReturnedR0(guest(pc, cpu, kernel, dispatcher))
            },
        )
    }
    pub(crate) fn drive_outcomes(
        cpu: &mut dyn Cpu,
        kernel: &mut pocket_kernel::KernelState,
        dispatcher: &mut WinCeDispatcher,
        start: Thunk,
        budget: usize,
        mut guest: impl FnMut(
            u32,
            &mut dyn Cpu,
            &mut pocket_kernel::KernelState,
            &mut WinCeDispatcher,
        ) -> DispatchOutcome,
    ) -> u32 {
        let mut thunks: std::collections::HashMap<_, _> = kernel.dynamic_exports[&1]
            .iter()
            .map(|(name, &va)| {
                (
                    va,
                    Thunk {
                        thunk_va: va,
                        iat_va: 0,
                        dll: "coredll.dll".into(),
                        binding: pocket_pe::ImportBinding::Name(name.clone()),
                        friendly_name: None,
                    },
                )
            })
            .collect();
        let mut pc = start.thunk_va;
        thunks.insert(pc, start);
        cpu.write_reg(ArmReg::Lr, 0x12345678).unwrap();
        // Nested Create/WM_CREATE/WM_SIZE traverse several real virtual maps.
        for _ in 0..budget {
            if pc == 0x12345678 {
                return cpu.read_reg(ArmReg::R0).unwrap();
            }
            let outcome = if let Some(thunk) = thunks.get(&pc) {
                dispatcher.dispatch(cpu, thunk, kernel).unwrap()
            } else {
                guest(pc, cpu, kernel, dispatcher)
            };
            pc = match outcome {
                DispatchOutcome::JumpTo(target) => target,
                DispatchOutcome::ReturnedR0(result) => {
                    cpu.write_reg(ArmReg::R0, result).unwrap();
                    cpu.read_reg(ArmReg::Lr).unwrap()
                }
                other => panic!("Unexpected {other:?}"),
            };
        }
        panic!("Window continuation did not return");
    }
    pub(in crate::mfc) fn entry(ordinal: u16) -> Thunk {
        Thunk {
            thunk_va: 0x70000000,
            iat_va: 0,
            dll: "mfcce300.dll".into(),
            binding: pocket_pe::ImportBinding::Ordinal(ordinal),
            friendly_name: None,
        }
    }

    #[test]
    fn window_proc_preserves_handled_lresult_and_calls_default_only_when_unhandled() {
        for handled in [false, true] {
            let (mut cpu, mut kernel, mut dispatcher) = super::super::tests::framework_setup();
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "mfcce300.dll",
                "ord:206",
                &[0x2000],
            );
            let table = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(table, 46 * 4).unwrap();
            cpu.write_mem(0x2400, &bytes).unwrap();
            for (slot, target) in [(39, 0x6000u32), (40, 0x6100)] {
                cpu.write_mem(0x2400 + slot * 4, &target.to_le_bytes())
                    .unwrap();
            }
            cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
            for (i, value) in [0x2000, 0x8001, 0xabc, 0xdef].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            let available = kernel.heap.free_bytes();
            let mut default_calls = 0;
            let result = drive(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                entry(2577),
                |pc, cpu| {
                    for (i, value) in [0x2000, 0x8001, 0xabc, 0xdef].into_iter().enumerate() {
                        assert_eq!(cpu.read_arg(i as u8).unwrap(), value);
                    }
                    if pc == 0x6000 {
                        let sp = cpu.read_reg(ArmReg::Sp).unwrap();
                        let result = cpu.read_u32_le(sp).unwrap();
                        cpu.write_mem(result, &0xfffffffeu32.to_le_bytes()).unwrap();
                        u32::from(handled)
                    } else {
                        assert_eq!(pc, 0x6100);
                        default_calls += 1;
                        0x87654321
                    }
                },
            );
            assert_eq!(result, if handled { 0xfffffffe } else { 0x87654321 });
            assert_eq!(default_calls, usize::from(!handled));
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert_eq!(kernel.heap.free_bytes(), available);
        }
    }

    #[test]
    fn superclass_lookup_override_and_direct_superclass_use_the_existing_call_service() {
        for direct in [false, true] {
            let (mut cpu, mut kernel, mut dispatcher) = super::super::tests::framework_setup();
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "mfcce300.dll",
                "ord:206",
                &[0x2000],
            );
            let table = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(table, 46 * 4).unwrap();
            cpu.write_mem(0x2400, &bytes).unwrap();
            cpu.write_mem(0x2480, &0x6200u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2020, &0xdead1000u32.to_le_bytes()).unwrap();
            cpu.write_mem(
                if direct { 0x2030 } else { 0x2800 },
                &0x6300u32.to_le_bytes(),
            )
            .unwrap();
            for (i, value) in [0x2000, 15, 7, 0x1234].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            let available = kernel.heap.free_bytes();
            let mut lookups = 0;
            let result = drive(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                entry(1024),
                |pc, cpu| {
                    if pc == 0x6200 {
                        lookups += 1;
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                        0x2800
                    } else {
                        assert_eq!(pc, 0x6300);
                        for (i, value) in [0xdead1000, 15, 7, 0x1234].into_iter().enumerate() {
                            assert_eq!(cpu.read_arg(i as u8).unwrap(), value);
                        }
                        0x87654321
                    }
                },
            );
            assert_eq!(lookups, usize::from(!direct));
            assert_eq!(result, 0x87654321);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert_eq!(kernel.heap.free_bytes(), available);
        }
    }
    #[test]
    fn default_uses_last_sent_message_and_the_current_virtual_default_procedure() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let thread = super::super::tests::result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "?AfxGetThreadState@@YAPAV_AFX_THREAD_STATE@@XZ",
            &[],
        ));
        let table = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(table, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &bytes).unwrap();
        cpu.write_mem(0x2400 + 40 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        for (i, value) in [0xdead1000u32, 0x8001, 0xabcdef, 0x12345678]
            .into_iter()
            .enumerate()
        {
            cpu.write_mem(thread + 0x3c + i as u32 * 4, &value.to_le_bytes())
                .unwrap();
        }
        cpu.write_arg(0, 0x2000).unwrap();
        let available = k.heap.free_bytes();
        let result = drive(&mut cpu, &mut k, &mut d, entry(1021), |pc, cpu| {
            assert_eq!(pc, 0x6000);
            for (i, value) in [0x2000, 0x8001, 0xabcdef, 0x12345678]
                .into_iter()
                .enumerate()
            {
                assert_eq!(cpu.read_arg(i as u8).unwrap(), value);
            }
            0xfffffffe
        });
        assert_eq!(result, 0xfffffffe);
        assert_eq!(k.heap.free_bytes(), available);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
}

// These two retail virtuals return their class identity without consulting an
// OS resource. Preserve that distinction for derived CWnd implementations.
fn is_frame(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn is_not_frame(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}
