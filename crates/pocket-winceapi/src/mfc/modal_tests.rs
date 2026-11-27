//! Modal tests use live USER queues/owners and actual framework continuations.
use super::tests::{framework_setup, result};
use super::window_dispatch::tests::{drive, entry};
use crate::bluetooth::tests::call;
use pocket_cpu::{regs::ArmReg, Cpu};
use pocket_kernel::DispatchOutcome;
fn args(cpu: &mut dyn Cpu, values: &[u32]) {
    for (i, &value) in values.iter().enumerate() {
        if i < 4 {
            cpu.write_arg(i as u8, value).unwrap();
        } else {
            let sp = cpu.read_reg(ArmReg::Sp).unwrap();
            cpu.write_mem(sp + (i as u32 - 4) * 4, &value.to_le_bytes())
                .unwrap();
        }
    }
}
#[test]
fn end_modal_loop_records_the_result_and_posts_one_real_wakeup_only_while_continuing() {
    let (mut cpu, mut k, mut d) = framework_setup();
    k.window_classes.insert(7, "Modal".into());
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2400],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:804",
        &[0x2400, 7],
    );
    cpu.write_mem(0x242c, &0x418u32.to_le_bytes()).unwrap();
    args(&mut cpu, &[0x2400, 42]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1134), |pc, _| panic!(
            "unexpected {pc:x}"
        )),
        0
    );
    assert_eq!(cpu.read_u32_le(0x2434).unwrap(), 42);
    assert_eq!(cpu.read_u32_le(0x242c).unwrap(), 0x408);
    assert!(k.posted_messages.contains(&(7, 0, 0, 0)));
    let queued = k.posted_messages.len();
    args(&mut cpu, &[0x2400, 73]);
    drive(&mut cpu, &mut k, &mut d, entry(1134), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert_eq!(k.posted_messages.len(), queued);
    assert_eq!(cpu.read_u32_le(0x2434).unwrap(), 73);
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:889",
            &[0x2400]
        )),
        0
    );
    cpu.write_mem(0x242c, &0x418u32.to_le_bytes()).unwrap();
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:889",
            &[0x2400]
        )),
        0x10
    );
}
#[test]
fn modal_loop_shows_on_idle_calls_the_actual_thread_pump_and_releases_its_frames() {
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
    k.window_classes.insert(7, "Modal".into());
    k.window_styles.insert(7, (0, 0));
    k.window_visibility.insert(7, false);
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:804",
        &[0x2400, 7],
    );
    let original = cpu.read_u32_le(0x2000).unwrap();
    let table = cpu.read_mem(original, 40 * 4).unwrap();
    cpu.write_mem(0x2800, &table).unwrap();
    cpu.write_mem(0x2800 + 24 * 4, &0x6100u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
    k.pending_startup.clear();
    k.pending_message = None;
    k.pending_create = None;
    k.posted_messages.clear();
    k.synthetic_paint_next_ms = u64::MAX;
    let mut free = None;
    for value in [42u32, 73] {
        cpu.write_mem(0x242c, &0x400u32.to_le_bytes()).unwrap();
        args(&mut cpu, &[0x2400, 4]);
        let mut pumped = 0;
        let answer = drive(&mut cpu, &mut k, &mut d, entry(2236), |pc, cpu| {
            assert_eq!(pc, 0x6100);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
            pumped += 1;
            let flags = cpu.read_u32_le(0x242c).unwrap();
            assert_eq!(flags & 0x18, 0x18);
            cpu.write_mem(0x242c, &(flags & !0x10).to_le_bytes())
                .unwrap();
            cpu.write_mem(0x2434, &value.to_le_bytes()).unwrap();
            1
        });
        assert_eq!(answer, value);
        assert_eq!(pumped, 1);
        assert_eq!(cpu.read_u32_le(0x242c).unwrap(), 0x400);
        assert!(k.window_visibility[&7]);
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")));
        if let Some(previous) = free {
            assert_eq!(k.heap.free_bytes(), previous);
        }
        free = Some(k.heap.free_bytes());
    }
}
#[test]
fn modal_loop_reposts_quit_and_returns_minus_one_instead_of_inventing_a_dialog_answer() {
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
    k.window_classes.insert(7, "Modal".into());
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:804",
        &[0x2400, 7],
    );
    let original = cpu.read_u32_le(0x2000).unwrap();
    let table = cpu.read_mem(original, 40 * 4).unwrap();
    cpu.write_mem(0x2800, &table).unwrap();
    cpu.write_mem(0x2800 + 24 * 4, &0x6100u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
    args(&mut cpu, &[0x2400, 0]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(2236), |pc, _| {
            assert_eq!(pc, 0x6100);
            0
        }),
        u32::MAX
    );
    assert!(k.posted_messages.iter().any(|m| m.1 == 0x12 && m.2 == 0));
    // The CE3 quit branch does not run the normal flags-clearing epilogue.
    assert_eq!(cpu.read_u32_le(0x242c).unwrap() & 0x18, 0x18);
}
#[test]
fn modal_template_initializers_borrow_storage_and_keep_an_existing_parent() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:77",
        &[0x2400, 12, 0x2200],
    );
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1637",
            &[0x2400, 0xabcdef00, 0x2300]
        )),
        1
    );
    assert_eq!(cpu.read_u32_le(0x2464).unwrap(), 0xabcdef00);
    assert_eq!(cpu.read_u32_le(0x2470).unwrap(), 0x2200);
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1638",
            &[0x2400, 0xabcdee00, 0x2300, 0xabcdea00]
        )),
        1
    );
    assert_eq!(cpu.read_u32_le(0x2468).unwrap(), 0xabcdee00);
    assert_eq!(cpu.read_u32_le(0x246c).unwrap(), 0xabcdea00);
    assert_eq!(cpu.read_u32_le(0x2470).unwrap(), 0x2200);
}
#[test]
fn missing_modal_resource_does_not_disable_or_publish_a_window() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:77",
        &[0x2400, 0x7777, 0],
    );
    let windows = k.window_classes.len();
    args(&mut cpu, &[0x2400]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1079), |pc, _| panic!(
            "unexpected {pc:x}"
        )),
        u32::MAX
    );
    assert_eq!(k.window_classes.len(), windows);
    assert_eq!(cpu.read_u32_le(0x2420).unwrap(), 0);
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|key| key.starts_with("__PocketHLE_MFC_Create:")));
}

fn modal_template() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x80000000u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for n in [1u16, 0, 0, 100, 70, 0, 0, 0] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
    bytes.extend_from_slice(&0x50010001u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for n in [5u16, 5, 40, 12, 1, 0xffff, 0x80] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    for n in "OK".encode_utf16().chain([0, 0]) {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    bytes
}
#[test]
fn do_modal_uses_real_cancel_restores_parent_and_releases_owned_windows_and_bar_on_repeat() {
    exercise_modal_exception(0);
}
#[test]
fn modal_pump_exception_closes_with_failure_and_reenables_the_parent_on_repeat() {
    exercise_modal_exception(1);
}
#[test]
fn modal_initialization_user_exception_is_caught_by_window_dispatch_and_allows_real_cancel() {
    exercise_modal_exception(2);
}
fn exercise_modal_exception(mode: u32) {
    use super::window_dispatch::tests::drive_outcomes;
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
    k.window_classes.insert(7, "Owner".into());
    k.window_procs.insert(7, 0x6200);
    k.window_styles.insert(7, (0x10000000, 0));
    k.window_visibility.insert(7, true);
    k.window_rects.insert(7, (0, 0, 240, 320));
    k.active_window = 7;
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:804",
        &[0x2400, 7],
    );
    cpu.write_mem(0x2020, &0x2400u32.to_le_bytes()).unwrap();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:79",
        &[0x2600],
    );
    cpu.write_mem(0x2648, &0u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x3000, &modal_template()).unwrap();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:1638",
        &[0x2600, 0x3000, 0x2400, 0],
    );
    if mode == 2 {
        let original = cpu.read_u32_le(0x2600).unwrap();
        let bytes = cpu.read_mem(original, 54 * 4).unwrap();
        cpu.write_mem(0x2a00, &bytes).unwrap();
        cpu.write_mem(0x2a00 + 47 * 4, &0x6300u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2600, &0x2a00u32.to_le_bytes()).unwrap();
    }
    let original = cpu.read_u32_le(0x2000).unwrap();
    let table = cpu.read_mem(original, 40 * 4).unwrap();
    cpu.write_mem(0x2800, &table).unwrap();
    cpu.write_mem(0x2800 + 24 * 4, &0x6100u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
    let mut previous_free = None;
    for _ in 0..2 {
        let mut enabled = Vec::new();
        let mut pumps = 0;
        args(&mut cpu, &[0x2600]);
        let answer = drive_outcomes(
            &mut cpu,
            &mut k,
            &mut d,
            entry(1079),
            2048,
            |pc, cpu, k, d| {
                if pc == 0x6200 {
                    if cpu.read_arg(1).unwrap() == 0xa {
                        enabled.push(cpu.read_arg(2).unwrap());
                    }
                    return DispatchOutcome::ReturnedR0(0);
                }
                if mode == 1 || (mode == 2 && pc == 0x6300) {
                    assert_eq!(pc, if mode == 1 { 0x6100 } else { 0x6300 });
                    pumps += 1;
                    return DispatchOutcome::JumpTo(
                        k.dynamic_exports[&1][super::exception_throw::USER],
                    );
                }
                assert_eq!(pc, 0x6100);
                pumps += 1;
                assert_ne!(cpu.read_u32_le(0x2620).unwrap(), 0);
                assert!(!crate::window_enable::locally_enabled(k, 7));
                let saved: Vec<_> = [
                    ArmReg::R0,
                    ArmReg::R1,
                    ArmReg::R2,
                    ArmReg::R3,
                    ArmReg::Lr,
                    ArmReg::Sp,
                ]
                .into_iter()
                .map(|r| (r, cpu.read_reg(r).unwrap()))
                .collect();
                let sp = cpu.read_reg(ArmReg::Sp).unwrap();
                cpu.write_reg(ArmReg::Sp, sp - 0x80).unwrap();
                args(cpu, &[0x2600]);
                drive(cpu, k, d, entry(1805), |target, _| {
                    panic!("unexpected cancel callback {target:x}")
                });
                for (r, value) in saved {
                    cpu.write_reg(r, value).unwrap();
                }
                DispatchOutcome::ReturnedR0(1)
            },
        );
        assert_eq!(answer, if mode == 1 { u32::MAX } else { 2 });
        assert_eq!(pumps, if mode == 2 { 2 } else { 1 });
        assert_eq!(enabled, [0, 1]);
        assert!(crate::window_enable::locally_enabled(&k, 7));
        assert_eq!(cpu.read_u32_le(0x2620).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x264c).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2674).unwrap(), 0);
        assert!(k.controls.children_of(7).is_empty());
        assert!(k.dialog_frame.is_none());
        assert!(k.dialog_frame_stack.is_empty());
        assert!(k.dialog_default_frames.is_empty());
        assert!(!k.window_classes.values().any(|name| name == "#32770"));
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")
                || key.starts_with("__PocketHLE_MFC_Create:")
                || key.starts_with("__PocketHLE_MFC_DialogBar:")));
        if let Some(free) = previous_free {
            assert_eq!(k.heap.free_bytes(), free);
        }
        previous_free = Some(k.heap.free_bytes());
    }
}
