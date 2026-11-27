use super::super::tests::{framework_setup, result};
use super::super::window_dispatch::tests::{drive_outcomes, entry};
use super::*;
use crate::bluetooth::tests::call;
use pocket_cpu::Cpu;
fn args(cpu: &mut dyn Cpu, values: &[u32]) {
    for (i, &v) in values.iter().enumerate() {
        if i < 4 {
            cpu.write_arg(i as u8, v).unwrap();
        } else {
            let sp = cpu.read_reg(ArmReg::Sp).unwrap();
            cpu.write_mem(
                sp + cpu.stack_arg_offset() + (i as u32 - 4) * 4,
                &v.to_le_bytes(),
            )
            .unwrap();
        }
    }
}
fn override_slot(cpu: &mut dyn Cpu, object: u32, copy: u32, count: u32, slot: u32, pc: u32) {
    let v = cpu.read_u32_le(object).unwrap();
    let bytes = cpu.read_mem(v, count * 4).unwrap();
    cpu.write_mem(copy, &bytes).unwrap();
    cpu.write_mem(copy + slot * 4, &pc.to_le_bytes()).unwrap();
    cpu.write_mem(object, &copy.to_le_bytes()).unwrap();
}
#[test]
fn window_throw_calls_the_actual_thread_hook_and_releases_the_owned_exception() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:201",
        &[0x2000, 0],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2400],
    );
    override_slot(&mut cpu, 0x2000, 0x2800, 39, 28, 0x6200);
    override_slot(&mut cpu, 0x2400, 0x2a00, 46, 38, 0x6100);
    let ts = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:651",
        &[],
    ));
    cpu.write_mem(ts + 0x3c, &[0x5a; 28]).unwrap();
    let mut available = None;
    for message in [0x111u32, 15, 1] {
        let exception = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "LocalAlloc",
            &[0, 8],
        ));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:93",
            &[exception, 1],
        );
        args(&mut cpu, &[0x2400, 7, message, 42, 0x12345678]);
        let mut seen = 0;
        let answer = drive_outcomes(
            &mut cpu,
            &mut k,
            &mut d,
            entry(606),
            512,
            |pc, cpu, k, _| match pc {
                0x6100 => {
                    assert_eq!(cpu.read_arg(3).unwrap(), 0x12345678);
                    args(cpu, &[exception]);
                    DispatchOutcome::JumpTo(
                        k.dynamic_exports[&1][super::super::exception_throw::THROW],
                    )
                }
                0x6200 => {
                    seen += 1;
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                    assert_eq!(cpu.read_arg(1).unwrap(), exception);
                    let msg = cpu.read_arg(2).unwrap();
                    assert_eq!(msg, ts + 0x3c);
                    assert_eq!(cpu.read_u32_le(msg).unwrap(), 7);
                    assert_eq!(cpu.read_u32_le(msg + 4).unwrap(), message);
                    assert_eq!(cpu.read_u32_le(msg + 8).unwrap(), 42);
                    assert_eq!(cpu.read_u32_le(msg + 12).unwrap(), 0x12345678);
                    DispatchOutcome::ReturnedR0(73)
                }
                _ => panic!("unexpected guest callback {pc:x}"),
            },
        );
        assert_eq!(answer, 73);
        assert_eq!(seen, 1);
        assert_eq!(cpu.read_mem(ts + 0x3c, 28).unwrap(), [0x5a; 28]);
        assert_eq!(cpu.read_u32_le(ts + 0x10).unwrap(), 0);
        assert!(k.heap.msize(exception).is_none());
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|s| s.starts_with("__PocketHLE_MFC_Frame:")));
        if let Some(n) = available {
            assert_eq!(k.heap.free_bytes(), n);
        }
        available = Some(k.heap.free_bytes());
    }
}
#[test]
fn replacement_exception_escapes_the_window_catch_and_restores_the_entire_previous_msg() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:201",
        &[0x2000, 0],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2400],
    );
    override_slot(&mut cpu, 0x2000, 0x2800, 39, 28, 0x6200);
    override_slot(&mut cpu, 0x2400, 0x2a00, 46, 38, 0x6100);
    let exception = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "LocalAlloc",
        &[0, 8],
    ));
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:93",
        &[exception, 1],
    );
    let ts = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:651",
        &[],
    ));
    cpu.write_mem(ts + 0x3c, &[0xa5; 28]).unwrap();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:33",
        &[0x3000],
    );
    cpu.write_reg(ArmReg::Lr, 0x12345678).unwrap();
    call(&mut cpu, &mut k, &mut d, "coredll.dll", "setjmp", &[0x3000]);
    args(&mut cpu, &[0x2400, 7, 0x111, 42, 0x1234]);
    let answer = drive_outcomes(
        &mut cpu,
        &mut k,
        &mut d,
        entry(606),
        1024,
        |pc, cpu, k, _| {
            if pc == 0x6100 {
                args(cpu, &[exception]);
                DispatchOutcome::JumpTo(k.dynamic_exports[&1][super::super::exception_throw::THROW])
            } else {
                assert_eq!(pc, 0x6200);
                DispatchOutcome::JumpTo(k.dynamic_exports[&1][super::super::exception_throw::USER])
            }
        },
    );
    assert_eq!(answer, 1);
    assert_eq!(cpu.read_mem(ts + 0x3c, 28).unwrap(), [0xa5; 28]);
    assert_eq!(cpu.read_u32_le(ts + 0x10).unwrap(), 0x3000);
    assert_ne!(cpu.read_u32_le(0x3034).unwrap(), 0);
    assert!(k.heap.msize(exception).is_none());
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|s| s.starts_with("__PocketHLE_MFC_Frame:")));
}
#[test]
fn default_thread_and_application_hooks_keep_ce_create_and_paint_contracts() {
    let (mut cpu, mut k, mut d) = framework_setup();
    for ordinal in [2115, 2116] {
        for (message, expected) in [(1u32, u32::MAX), (15, 0), (0x400, 0)] {
            if ordinal == 2115 && message == 0x400 {
                continue;
            }
            cpu.write_mem(0x1800, &7u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x1804, &message.to_le_bytes()).unwrap();
            // ValidateRect must consume a real pending paint, not fabricate success.
            k.window_classes.insert(7, "Test".into());
            k.window_visibility.insert(7, true);
            k.window_procs.insert(7, 0x6100);
            k.window_rects.insert(7, (0, 0, 100, 100));
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "InvalidateRect",
                &[7, 0, 0],
            );
            args(&mut cpu, &[0, 0, 0x1800]);
            assert_eq!(
                drive_outcomes(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    entry(ordinal),
                    128,
                    |pc, _, _, _| panic!("unexpected {pc:x}")
                ),
                expected
            );
            if message == 15 {
                // A second validation has no invalid region left to consume.
                assert!(!k.gdi.pending_window_updates.contains_key(&7));
                assert!(!k.gdi.generated_window_paints.contains(&7));
            }
        }
    }
}
#[test]
fn application_exception_reporting_distinguishes_memory_user_and_command_messages() {
    let (mut cpu, mut k, mut d) = framework_setup();
    for class in ["CException", "CMemoryException", "CUserException"] {
        let runtime = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            match class {
                "CMemoryException" => "ord:1496",
                "CUserException" => "ord:1537",
                _ => "ord:1468",
            },
            &[],
        ));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:93",
            &[0x2200, 0],
        );
        let base = cpu.read_u32_le(0x2200).unwrap();
        // Runtime identity is a real guest virtual override, as with derived apps.
        let bytes = cpu.read_mem(base, 28).unwrap();
        cpu.write_mem(0x2c00, &bytes).unwrap();
        cpu.write_mem(0x2c00, &0x6300u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2c18, &0x6200u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2200, &0x2c00u32.to_le_bytes()).unwrap();
        for (message, lparam, expected, id) in [
            (0x111u32, 0, 1, 0xf109u32),
            (0x111, 7, 1, 0xf108),
            (0x400, 0, 0, 0xf108),
        ] {
            for (p, value) in [(0x1804, message), (0x180c, lparam)] {
                cpu.write_mem(p, &value.to_le_bytes()).unwrap();
            }
            args(&mut cpu, &[0, 0x2200, 0x1800]);
            let mut reports = 0;
            let answer = drive_outcomes(
                &mut cpu,
                &mut k,
                &mut d,
                entry(2115),
                128,
                |pc, cpu, _, _| {
                    if pc == 0x6300 {
                        return DispatchOutcome::ReturnedR0(runtime);
                    }
                    assert_eq!(pc, 0x6200);
                    reports += 1;
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                    assert_eq!(
                        cpu.read_arg(1).unwrap(),
                        if class == "CMemoryException" {
                            0x30
                        } else {
                            0x10
                        }
                    );
                    assert_eq!(cpu.read_arg(2).unwrap(), id);
                    DispatchOutcome::ReturnedR0(1)
                },
            );
            assert_eq!(answer, expected);
            assert_eq!(reports, if class == "CUserException" { 0 } else { 1 });
        }
    }
}
#[test]
fn nested_window_callbacks_restore_the_outer_message_before_returning_to_the_caller() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2400],
    );
    override_slot(&mut cpu, 0x2400, 0x2a00, 46, 38, 0x6100);
    let ts = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:651",
        &[],
    ));
    cpu.write_mem(ts + 0x3c, &[0x7b; 28]).unwrap();
    args(&mut cpu, &[0x2400, 7, 0x401, 42, 0x1234]);
    let mut outer_lr = 0;
    let mut callbacks = 0;
    let answer = drive_outcomes(
        &mut cpu,
        &mut k,
        &mut d,
        entry(606),
        256,
        |pc, cpu, k, _| {
            callbacks += 1;
            if pc == 0x6400 {
                assert_eq!(cpu.read_u32_le(ts + 0x40).unwrap(), 0x401);
                assert_eq!(cpu.read_u32_le(ts + 0x48).unwrap(), 0x1234);
                cpu.write_reg(ArmReg::Lr, outer_lr).unwrap();
                return DispatchOutcome::ReturnedR0(77);
            }
            assert_eq!(pc, 0x6100);
            let message = cpu.read_arg(1).unwrap();
            assert_eq!(cpu.read_u32_le(ts + 0x40).unwrap(), message);
            if message == 0x401 {
                outer_lr = cpu.read_reg(ArmReg::Lr).unwrap();
                cpu.write_reg(ArmReg::Lr, 0x6400).unwrap();
                args(cpu, &[0x2400, 8, 0x402, 43, 0x5678]);
                DispatchOutcome::JumpTo(k.dynamic_exports[&1][CALL])
            } else {
                assert_eq!(message, 0x402);
                assert_eq!(cpu.read_u32_le(ts + 0x48).unwrap(), 0x5678);
                DispatchOutcome::ReturnedR0(66)
            }
        },
    );
    assert_eq!(answer, 77);
    assert_eq!(callbacks, 3);
    assert_eq!(cpu.read_mem(ts + 0x3c, 28).unwrap(), [0x7b; 28]);
    assert_eq!(cpu.read_u32_le(ts + 0x10).unwrap(), 0);
}
