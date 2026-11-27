//! Regression coverage follows nonlocal register returns, not fabricated BOOLs.
use super::tests::{framework_setup, result};
use super::window_dispatch::tests::{drive_with, entry};
use crate::bluetooth::tests::call;
use pocket_cpu::{regs::ArmReg, Cpu};
use pocket_kernel::{DispatchOutcome, Dispatcher, Thunk};
fn args(cpu: &mut dyn Cpu, values: &[u32]) {
    for (i, &v) in values.iter().enumerate() {
        cpu.write_arg(i as u8, v).unwrap();
    }
}
#[test]
fn update_data_catches_real_user_throw_and_restores_thread_state_without_leaking() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2400],
    );
    let table = cpu.read_u32_le(0x2400).unwrap();
    let bytes = cpu.read_mem(table, 46 * 4).unwrap();
    cpu.write_mem(0x2800, &bytes).unwrap();
    cpu.write_mem(0x2800 + 33 * 4, &0x6100u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(0x2400, &0x2800u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2420, &7u32.to_le_bytes()).unwrap();
    let ts = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "?AfxGetThreadState@@YAPAV_AFX_THREAD_STATE@@XZ",
        &[],
    ));
    cpu.write_mem(ts + 0x120, &73u32.to_le_bytes()).unwrap();
    let mut free = None;
    for _ in 0..2 {
        args(&mut cpu, &[0x2400, 1]);
        let value = drive_with(
            &mut cpu,
            &mut k,
            &mut d,
            entry(2495),
            512,
            |pc, cpu, k, d| {
                assert_eq!(pc, 0x6100);
                let dx = cpu.read_arg(1).unwrap();
                assert_eq!(cpu.read_u32_le(dx).unwrap(), 1);
                assert_eq!(cpu.read_u32_le(ts + 0x120).unwrap(), 7);
                // A guest tail call throws without returning to the DDX callback. The
                // dispatcher's nonlocal result restores the pending UpdateData return.
                let thunk = Thunk {
                    thunk_va: 0x70000020,
                    iat_va: 0,
                    dll: "mfcce300.dll".into(),
                    binding: pocket_pe::ImportBinding::Ordinal(711),
                    friendly_name: None,
                };
                let outcome = d.dispatch(cpu, &thunk, k).unwrap();
                // The default application is absent in this fixture: no wait-cursor call.
                match outcome {
                    DispatchOutcome::ReturnedR0(v) => v,
                    DispatchOutcome::JumpTo(_) => panic!("unexpected throw callback"),
                    _ => panic!("bad throw"),
                }
            },
        );
        assert_eq!(value, 0);
        assert_eq!(cpu.read_u32_le(ts + 0x120).unwrap(), 73);
        assert_eq!(cpu.read_u32_le(ts + 0x10).unwrap(), 0);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|s| s.starts_with("__PocketHLE_MFC_Frame:")));
        if let Some(n) = free {
            assert_eq!(k.heap.free_bytes(), n);
        }
        free = Some(k.heap.free_bytes());
    }
}
#[test]
fn guest_rethrow_skips_the_caught_link_and_restores_outer_registers() {
    let (mut cpu, mut k, mut d) = framework_setup();
    for (p, sp, lr) in [(0x2400, 0x3ff0, 0x6200), (0x2500, 0x3fe0, 0x6300)] {
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:33", &[p]);
        cpu.write_reg(ArmReg::Sp, sp).unwrap();
        cpu.write_reg(ArmReg::Lr, lr).unwrap();
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "setjmp", &[p]);
    }
    cpu.write_reg(ArmReg::Sp, 0x3f00).unwrap();
    let first = call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:711", &[]);
    assert_eq!(first, DispatchOutcome::ReturnedR0(1));
    assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x6300);
    let exception = cpu.read_u32_le(0x2534).unwrap();
    assert_ne!(exception, 0);
    let second = call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:704", &[0]);
    assert_eq!(second, DispatchOutcome::ReturnedR0(1));
    assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x6200);
    assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x3ff0);
    assert_eq!(cpu.read_u32_le(0x2434).unwrap(), exception);
    assert_eq!(cpu.read_u32_le(0x2534).unwrap(), 0);
}

#[test]
fn ddv_rejection_uses_real_message_box_selects_the_edit_and_keeps_the_dialog_alive() {
    use super::window_dispatch::tests::drive_outcomes;
    use pocket_kernel::{controls::ControlClass, InputEvent};
    let (mut cpu, mut k, mut d) = framework_setup();
    cpu.write_mem(0x1000, &[b'T', 0, 0, 0]).unwrap();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:201",
        &[0x2000, 0x1000],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:79",
        &[0x2400],
    );
    for (object, out, slot, pc) in [
        (0x2000, 0x2c00, 35, 0x6200u32),
        (0x2400, 0x2800, 33, 0x6100),
    ] {
        let v = cpu.read_u32_le(object).unwrap();
        let bytes = cpu.read_mem(v, 40 * 4).unwrap();
        cpu.write_mem(out, &bytes).unwrap();
        cpu.write_mem(out + slot * 4, &pc.to_le_bytes()).unwrap();
        cpu.write_mem(object, &out.to_le_bytes()).unwrap();
    }
    cpu.write_mem(0x2420, &7u32.to_le_bytes()).unwrap();
    k.window_classes.insert(7, "Dialog".into());
    k.window_styles.insert(7, (0x10000000, 0));
    k.window_visibility.insert(7, true);
    let hwnd = k.controls.create(
        7,
        ControlClass::Edit,
        10,
        "123".into(),
        0x50010000,
        2,
        2,
        100,
        20,
    );
    for (name, symbol, va) in [
        (
            "__test_DDV",
            "?DDV_MinMaxInt@@YAXPAVCDataExchange@@HHH@Z",
            0x700f0000,
        ),
        (
            "__test_Box",
            "?DoMessageBox@CWinApp@@UAAHPBGII@Z",
            0x700f0020,
        ),
    ] {
        d.register_handler("coredll.dll", name, super::handler_named(symbol).unwrap());
        k.dynamic_exports
            .get_mut(&1)
            .unwrap()
            .insert(name.into(), va);
    }
    let mut previous = None;
    let text: Vec<_> = "123"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    cpu.write_mem(0x1800, &text).unwrap();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:180",
        &[0x2b00, 0x1800, 3],
    );
    for ordinal in [995, 990, 994, 992] {
        let handler = super::data_validation::EXPORTS
            .iter()
            .find(|(n, _, _)| *n == ordinal)
            .unwrap()
            .2;
        d.register_handler("coredll.dll", "__test_DDV", handler);
        for _ in 0..2 {
            args(&mut cpu, &[0x2400, 1]);
            k.pending_input.push_back(InputEvent::KeyDown { vk: 13 });
            let mut shown = 0;
            let mut exchanges = 0;
            let answer = drive_outcomes(
                &mut cpu,
                &mut k,
                &mut d,
                entry(1969),
                2048,
                |pc, cpu, _, _| {
                    if pc == 0x6100 {
                        exchanges += 1;
                        let dx = cpu.read_arg(1).unwrap();
                        cpu.write_mem(dx + 8, &hwnd.to_le_bytes()).unwrap();
                        cpu.write_mem(dx + 12, &1u32.to_le_bytes()).unwrap();
                        match ordinal {
                            990 => args(cpu, &[dx, 0x2b00, 2]),
                            994 => {
                                cpu.write_mem(0x3100, &123.0f32.to_le_bytes()).unwrap();
                                args(cpu, &[dx, 0x3100, 1.0f32.to_bits(), 9.0f32.to_bits()]);
                            }
                            992 => {
                                cpu.write_mem(0x3100, &123.0f64.to_le_bytes()).unwrap();
                                let lo = 1.0f64.to_bits();
                                let hi = 9.0f64.to_bits();
                                let sp = cpu.read_reg(ArmReg::Sp).unwrap() - 8;
                                cpu.write_reg(ArmReg::Sp, sp).unwrap();
                                cpu.write_mem(sp, &(hi as u32).to_le_bytes()).unwrap();
                                cpu.write_mem(sp + 4, &((hi >> 32) as u32).to_le_bytes())
                                    .unwrap();
                                args(cpu, &[dx, 0x3100, lo as u32, (lo >> 32) as u32]);
                            }
                            _ => args(cpu, &[dx, 123, 1, 9]),
                        }
                        DispatchOutcome::JumpTo(0x700f0000)
                    } else {
                        assert_eq!(pc, 0x6200);
                        shown += 1;
                        let p = cpu.read_arg(1).unwrap();
                        let mut text = Vec::new();
                        let mut at = p;
                        loop {
                            let c = cpu.read_mem(at, 2).unwrap();
                            let c = u16::from_le_bytes(c.try_into().unwrap());
                            if c == 0 {
                                break;
                            }
                            text.push(c);
                            at += 2;
                        }
                        assert_eq!(
                            String::from_utf16(&text).unwrap(),
                            if ordinal == 990 {
                                "Enter no more than 2 characters."
                            } else {
                                "Enter a value between 1 and 9."
                            }
                        );
                        DispatchOutcome::JumpTo(0x700f0020)
                    }
                },
            );
            assert_eq!(answer, 0);
            assert_eq!(shown, 1, "ordinal {ordinal}, exchanges {exchanges}");
            assert!(k.modal.is_none());
            assert_eq!(k.controls.focus, hwnd);
            assert_eq!(k.controls.get(hwnd).unwrap().edit.selection, Some((0, 3)));
            assert!(k.window_classes.contains_key(&7));
            assert!(k.controls.get(hwnd).is_some());
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|s| s.starts_with("__PocketHLE_MFC_Frame:")
                    || s.starts_with("__PocketHLE_WinCE_Frame:")));
            if let Some(n) = previous {
                assert_eq!(k.heap.free_bytes(), n);
            }
            previous = Some(k.heap.free_bytes());
        }
    }
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:297",
        &[0x2b00],
    );
    // The selected invalid value is actually replaced, not appended to.
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "SendMessageW",
        &[hwnd, 0x102, b'7' as u32, 0],
    );
    assert_eq!(k.controls.get(hwnd).unwrap().text, "7");
}

#[test]
fn unwind_callbacks_execute_and_owned_caught_exceptions_are_deleted_before_outer_catch() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:33",
        &[0x2400],
    );
    cpu.write_reg(ArmReg::Lr, 0x12345678).unwrap();
    call(&mut cpu, &mut k, &mut d, "coredll.dll", "setjmp", &[0x2400]);
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:33",
        &[0x2600],
    );
    let owned = result(call(
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
        &[owned, 1],
    );
    cpu.write_mem(0x2634, &owned.to_le_bytes()).unwrap();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:33",
        &[0x2500],
    );
    cpu.write_mem(0x2500, &0x6100u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x252c, &1u32.to_le_bytes()).unwrap();
    let context = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:2649",
        &[],
    ));
    cpu.write_reg(ArmReg::Sp, 0x3f00).unwrap();
    let mut callbacks = 0;
    let answer = drive_with(
        &mut cpu,
        &mut k,
        &mut d,
        entry(711),
        256,
        |pc, cpu, k, d| {
            assert_eq!(pc, 0x6100);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2500);
            callbacks += 1;
            let outcome = d.dispatch(cpu, &entry(768), k).unwrap();
            assert_eq!(outcome, DispatchOutcome::ReturnedR0(0));
            0
        },
    );
    assert_eq!(answer, 1);
    assert_eq!(callbacks, 1);
    assert_eq!(cpu.read_u32_le(context).unwrap(), 0x2400);
    assert_ne!(cpu.read_u32_le(0x2434).unwrap(), 0);
    assert!(k.heap.msize(owned).is_none());
}

#[test]
fn an_exception_from_report_error_unwinds_ddx_to_the_outer_guest_try_and_restores_suppression() {
    use super::window_dispatch::tests::drive_outcomes;
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2400],
    );
    let v = cpu.read_u32_le(0x2400).unwrap();
    let bytes = cpu.read_mem(v, 40 * 4).unwrap();
    cpu.write_mem(0x2800, &bytes).unwrap();
    cpu.write_mem(0x2800 + 33 * 4, &0x6100u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(0x2400, &0x2800u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2420, &7u32.to_le_bytes()).unwrap();
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
    let v = cpu.read_u32_le(exception).unwrap();
    let bytes = cpu.read_mem(v, 7 * 4).unwrap();
    cpu.write_mem(0x2c00, &bytes).unwrap();
    cpu.write_mem(0x2c18, &0x6200u32.to_le_bytes()).unwrap();
    cpu.write_mem(exception, &0x2c00u32.to_le_bytes()).unwrap();
    let ts = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "?AfxGetThreadState@@YAPAV_AFX_THREAD_STATE@@XZ",
        &[],
    ));
    cpu.write_mem(ts + 0x120, &73u32.to_le_bytes()).unwrap();
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
    args(&mut cpu, &[0x2400, 1]);
    let mut report = 0;
    let answer = drive_outcomes(
        &mut cpu,
        &mut k,
        &mut d,
        entry(2495),
        1024,
        |pc, cpu, k, _| {
            if pc == 0x6100 {
                args(cpu, &[exception]);
                DispatchOutcome::JumpTo(k.dynamic_exports[&1][super::exception_throw::THROW])
            } else {
                assert_eq!(pc, 0x6200);
                report += 1;
                DispatchOutcome::JumpTo(k.dynamic_exports[&1][super::exception_throw::USER])
            }
        },
    );
    assert_eq!(answer, 1);
    assert_eq!(report, 1);
    assert_eq!(cpu.read_u32_le(ts + 0x120).unwrap(), 73);
    assert_eq!(cpu.read_u32_le(ts + 0x10).unwrap(), 0x3000);
    assert_ne!(cpu.read_u32_le(0x3034).unwrap(), 0);
    assert!(k.heap.msize(exception).is_none());
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|s| s.starts_with("__PocketHLE_MFC_Frame:")));
}

#[test]
fn numeric_boundaries_and_initialization_do_not_report_or_throw() {
    for (ordinal, values) in [
        (991, [0x1ff, 0xff, 0x1ff]),
        (993, [u32::MAX, 0, u32::MAX]),
        (995, [(-4i32) as u32, (-4i32) as u32, 4]),
        (996, [4, (-4i32) as u32, 4]),
        (997, [0xffff, (-1i32) as u32, 1]),
        (999, [0x80000000, 0x7fffffff, u32::MAX]),
    ] {
        let (mut cpu, mut k, mut d) = framework_setup();
        for save in [0u32, 1] {
            cpu.write_mem(0x2400, &save.to_le_bytes()).unwrap();
            args(&mut cpu, &[0x2400, values[0], values[1], values[2]]);
            assert_eq!(
                drive_with(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    entry(ordinal),
                    128,
                    |pc, _, _, _| panic!("unexpected {pc:x}")
                ),
                0
            );
            assert!(k.modal.is_none());
        }
    }
}

#[test]
fn max_chars_sets_the_actual_edit_limit_and_constrains_user_input() {
    use pocket_kernel::controls::ControlClass;
    let (mut cpu, mut k, mut d) = framework_setup();
    let hwnd = k.controls.create(
        7,
        ControlClass::Edit,
        10,
        "".into(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    for (offset, v) in [(0, 0), (8, hwnd), (12, 1)] {
        cpu.write_mem(0x2400 + offset, &v.to_le_bytes()).unwrap();
    }
    args(&mut cpu, &[0x2400, 0, 3]);
    assert_eq!(
        drive_with(
            &mut cpu,
            &mut k,
            &mut d,
            entry(990),
            256,
            |pc, _, _, _| panic!("unexpected {pc:x}")
        ),
        0
    );
    for c in "ABCD".chars() {
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SendMessageW",
            &[hwnd, 0x102, c as u32, 0],
        );
    }
    assert_eq!(k.controls.get(hwnd).unwrap().text, "ABC");
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SendMessageW",
            &[hwnd, 0xd5, 0, 0]
        )),
        3
    );
    let wide: Vec<_> = "ABCDE"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    cpu.write_mem(0x1800, &wide).unwrap();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "SendMessageW",
        &[hwnd, 0xc, 0, 0x1800],
    );
    assert_eq!(k.controls.get(hwnd).unwrap().text, "ABCDE");
    let multiline = k.controls.create(
        7,
        ControlClass::Edit,
        11,
        "".into(),
        0x50010004,
        0,
        25,
        100,
        20,
    );
    for (handle, requested, expected) in [
        (hwnd, 0, 0x7ffe),
        (hwnd, u32::MAX, 0x7ffe),
        (multiline, 0, 0xffff),
        (multiline, u32::MAX, 0x7ffffffe),
    ] {
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SendMessageW",
            &[handle, 0xc5, requested, 0],
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendMessageW",
                &[handle, 0xd5, 0, 0]
            )),
            expected
        );
    }
}

#[test]
fn invalid_longjmp_is_an_error_without_a_fake_return_or_register_mutation() {
    let (mut cpu, mut k, mut d) = framework_setup();
    cpu.write_reg(ArmReg::R4, 73).unwrap();
    cpu.write_reg(ArmReg::Lr, 0x6100).unwrap();
    args(&mut cpu, &[0xdead0000, 1]);
    let thunk = Thunk {
        thunk_va: 0x70000000,
        iat_va: 0,
        dll: "coredll.dll".into(),
        binding: pocket_pe::ImportBinding::Name("longjmp".into()),
        friendly_name: None,
    };
    assert!(d.dispatch(&mut cpu, &thunk, &mut k).is_err());
    assert_eq!(cpu.read_reg(ArmReg::R4).unwrap(), 73);
    assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x6100);
}

#[test]
fn cyclic_try_links_report_an_error_instead_of_jumping_back_into_a_broken_catch() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:33",
        &[0x2400],
    );
    cpu.write_mem(0x2430, &0x2400u32.to_le_bytes()).unwrap();
    let thunk = entry(711);
    assert!(d
        .dispatch(&mut cpu, &thunk, &mut k)
        .unwrap_err()
        .to_string()
        .contains("Cyclic"));
}

fn unavailable_brush(
    _: &mut crate::CallCtx<'_>,
) -> Result<DispatchOutcome, pocket_kernel::KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}
#[test]
fn failed_brush_creation_throws_a_real_static_resource_exception_and_retires_constructor_frames() {
    let (mut cpu, mut k, mut d) = framework_setup();
    d.register_handler("coredll.dll", "CreateSolidBrush", unavailable_brush);
    let mut baseline = None;
    let mut singleton = None;
    for _ in 0..32 {
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:33",
            &[0x2400],
        );
        cpu.write_reg(ArmReg::Sp, 0x3ff0).unwrap();
        cpu.write_reg(ArmReg::Lr, 0x12345678).unwrap();
        call(&mut cpu, &mut k, &mut d, "coredll.dll", "setjmp", &[0x2400]);
        cpu.write_reg(ArmReg::Sp, 0x3f00).unwrap();
        args(&mut cpu, &[0x2800, 0xff]);
        assert_eq!(
            drive_with(
                &mut cpu,
                &mut k,
                &mut d,
                entry(43),
                512,
                |pc, _, _, _| panic!("unexpected guest {pc:x}")
            ),
            1
        );
        assert_eq!(cpu.read_u32_le(0x2804).unwrap(), 0);
        let exception = cpu.read_u32_le(0x2434).unwrap();
        assert_eq!(*singleton.get_or_insert(exception), exception);
        assert_eq!(cpu.read_u32_le(exception + 4).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(exception + 0x114).unwrap(), 0xf022);
        args(&mut cpu, &[]);
        drive_with(&mut cpu, &mut k, &mut d, entry(768), 128, |pc, _, _, _| {
            panic!("unexpected guest {pc:x}")
        });
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")));
        let free = k.heap.free_bytes();
        assert_eq!(*baseline.get_or_insert(free), free);
    }
}
