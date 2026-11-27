//! CE3 scroll dispatch must preserve native signed narrowing and real owners.
use super::super::tests::{framework_setup, result};
use super::super::window_dispatch::tests::{drive, entry};
use super::*;
use crate::bluetooth::tests::call;
use pocket_cpu::Cpu;
fn args(cpu: &mut dyn Cpu, values: [u32; 4]) {
    for (i, value) in values.into_iter().enumerate() {
        cpu.write_arg(i as u8, value).unwrap();
    }
}
fn override_slot(cpu: &mut dyn Cpu, object: u32, copy: u32, slot: u32, pc: u32) {
    let v = cpu.read_u32_le(object).unwrap();
    let bytes = cpu.read_mem(v, 46 * 4).unwrap();
    cpu.write_mem(copy, &bytes).unwrap();
    cpu.write_mem(copy + slot * 4, &pc.to_le_bytes()).unwrap();
    cpu.write_mem(object, &copy.to_le_bytes()).unwrap();
}
#[test]
fn scroll_signatures_narrow_words_and_wrap_real_scrollbar_handles_without_returning_void_registers()
{
    for (signature, hwnd) in [(29u32, 0u32), (29, 9), (30, 0)] {
        for message in [0x114u32, 0x115] {
            let (mut cpu, mut k, mut d) = framework_setup();
            for p in [0x2000, 0x2200] {
                call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:206", &[p]);
            }
            cpu.write_mem(0x2220, &9u32.to_le_bytes()).unwrap();
            k.dynamic_exports
                .get_mut(&1)
                .unwrap()
                .insert("__PocketHLE_MFC_Window:00000009".into(), 0x2200);
            override_slot(&mut cpu, 0x2000, 0x2400, 12, 0x6000);
            cpu.write_mem(0x3000, &0x6100u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x3004, &0x3100u32.to_le_bytes()).unwrap();
            for (i, value) in [message, 0, 0, 0, signature, 0x6200]
                .into_iter()
                .enumerate()
            {
                cpu.write_mem(0x3100 + i as u32 * 4, &value.to_le_bytes())
                    .unwrap();
            }
            cpu.write_mem(0x3118, &[0; 24]).unwrap();
            args(&mut cpu, [0x2000, message, 0xfffe8001, hwnd]);
            cpu.write_mem(0x4000, &0x2800u32.to_le_bytes()).unwrap();
            let free = k.heap.free_bytes();
            let mut calls = 0;
            let result = drive(&mut cpu, &mut k, &mut d, entry(2060), |pc, cpu| match pc {
                0x6000 => 0x3000,
                0x6200 => {
                    calls += 1;
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                    assert_eq!(cpu.read_arg(1).unwrap(), 0xffff8001);
                    assert_eq!(cpu.read_arg(2).unwrap(), 0xfffffffe);
                    if signature == 29 {
                        assert_eq!(cpu.read_arg(3).unwrap(), if hwnd == 0 { 0 } else { 0x2200 });
                    }
                    0xdeadbeef
                }
                _ => panic!("unexpected {pc:x}"),
            });
            assert_eq!(result, 1);
            assert_eq!(calls, 1);
            assert_eq!(cpu.read_u32_le(0x2800).unwrap(), 0);
            assert_eq!(k.heap.free_bytes(), free);
        }
    }
}
#[test]
fn base_scroll_reflects_to_actual_child_and_only_falls_back_when_unhandled() {
    let (mut cpu, mut k, mut d) = framework_setup();
    for p in [0x2000, 0x2200] {
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:206", &[p]);
    }
    override_slot(&mut cpu, 0x2000, 0x2400, 40, 0x6300);
    override_slot(&mut cpu, 0x2200, 0x2600, 42, 0x6200);
    let ts = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:651",
        &[],
    ));
    for (i, v) in [7u32, 0x115, 0x000c0005, 9].into_iter().enumerate() {
        cpu.write_mem(ts + 0x3c + i as u32 * 4, &v.to_le_bytes())
            .unwrap();
    }
    for ordinal in [1932, 2058] {
        for handled in [0u32, 1] {
            args(&mut cpu, [0x2000, 5, 12, 0x2200]);
            let mut defaults = 0;
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(ordinal), |pc, cpu| {
                    if pc == 0x6200 {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                        assert_eq!(cpu.read_arg(1).unwrap(), 0x115);
                        assert_eq!(cpu.read_arg(2).unwrap(), 0x000c0005);
                        assert_eq!(cpu.read_arg(3).unwrap(), 9);
                        let sp = cpu.read_reg(ArmReg::Sp).unwrap();
                        assert_eq!(cpu.read_u32_le(sp + cpu.stack_arg_offset()).unwrap(), 0);
                        handled
                    } else {
                        assert_eq!(pc, 0x6300);
                        defaults += 1;
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                        assert_eq!(cpu.read_arg(1).unwrap(), 0x115);
                        123
                    }
                }),
                0
            );
            assert_eq!(defaults, u32::from(handled == 0));
        }
    }
}
#[test]
fn frame_scroll_forwards_original_message_parameters_to_the_active_view() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:103",
        &[0x2000],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2200],
    );
    cpu.write_mem(0x2220, &9u32.to_le_bytes()).unwrap();
    k.window_procs.insert(9, 0x6400);
    let ts = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:651",
        &[],
    ));
    cpu.write_mem(ts + 0x44, &0x12340005u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(ts + 0x48, &77u32.to_le_bytes()).unwrap();
    for (ordinal, message) in [(1929, 0x114u32), (2055, 0x115)] {
        for active in [0u32, 0x2200] {
            cpu.write_mem(0x20ac, &active.to_le_bytes()).unwrap();
            args(&mut cpu, [0x2000, 5, 0x1234, 0]);
            let mut calls = 0;
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(ordinal), |pc, cpu| {
                    assert_eq!(pc, 0x6400);
                    calls += 1;
                    for (i, v) in [9u32, message, 0x12340005, 77].into_iter().enumerate() {
                        assert_eq!(cpu.read_arg(i as u8).unwrap(), v);
                    }
                    123
                }),
                0
            );
            assert_eq!(calls, u32::from(active != 0));
        }
    }
}
