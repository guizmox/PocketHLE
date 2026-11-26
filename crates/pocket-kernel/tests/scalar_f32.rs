#![cfg(feature = "unicorn-tests")]

use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};

const ENTRY: u32 = 0x1000;
const STOP: u32 = 0x1800;

fn setup() -> UnicornCpu {
    let mut cpu = UnicornCpu::new().unwrap();
    cpu.map_region(ENTRY, 0x2000, Prot::ALL).unwrap();
    cpu.add_code_hook(STOP).unwrap();
    cpu
}

fn code(cpu: &mut UnicornCpu, at: u32, words: &[u32]) {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    cpu.write_mem(at, &bytes).unwrap();
    cpu.flush_instruction_cache(None).unwrap();
}

#[test]
fn scalar_arithmetic_preserves_results_flags_and_special_values() {
    let mut cpu = setup();
    // Original complete ARM helper fixtures cover zeros, cancellation,
    // minimum normals, underflow, overflow, NaN payloads, DN/FZ and sticky flags.
    let cases = [
        (
            0xee300a20, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
        ),
        (
            0xee300a20, 0x80000000, 0x80000000, 0x0340000d, 0x80000000, 0x0340000d,
        ),
        (
            0xee300a20, 0x00800000, 0x00000000, 0x00000010, 0x00800000, 0x00000010,
        ),
        (
            0xee300a20, 0x00800000, 0x00800000, 0x01000014, 0x01000000, 0x01000014,
        ),
        (
            0xee300a20, 0x3f800000, 0x3f800000, 0x00400001, 0x40000000, 0x00400001,
        ),
        (
            0xee300a20, 0x3f800000, 0x7f7fffff, 0x00800002, 0x7f7fffff, 0x00800012,
        ),
        (
            0xee300a20, 0x7f7fffff, 0x3f800000, 0x0340000d, 0x7f800000, 0x0340001d,
        ),
        (
            0xee300a20, 0x7f7fffff, 0x7f800000, 0x00000010, 0x7f800000, 0x00000010,
        ),
        (
            0xee300a20, 0xff7fffff, 0x7f7fffff, 0x0280001a, 0x80000000, 0x0280001a,
        ),
        (
            0xee300a20, 0xff800000, 0x00000000, 0x0300000c, 0xff800000, 0x0300000c,
        ),
        (
            0xee300a20, 0x7f812345, 0x00000000, 0x01000004, 0x7fc12345, 0x01000005,
        ),
        (
            0xee300a20, 0x7f812345, 0x7f812345, 0x03c0000f, 0x7fc00000, 0x03c0000f,
        ),
        (
            0xee200a20, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
        ),
        (
            0xee200a20, 0x80000000, 0x80000000, 0x0340000d, 0x00000000, 0x0340000d,
        ),
        (
            0xee200a20, 0x00800000, 0x00000000, 0x00000010, 0x00000000, 0x00000010,
        ),
        (
            0xee200a20, 0x00800000, 0x00800000, 0x01000014, 0x00000000, 0x0100001c,
        ),
        (
            0xee200a20, 0x3f800000, 0x3f800000, 0x00400001, 0x3f800000, 0x00400001,
        ),
        (
            0xee200a20, 0x3f800000, 0x7f7fffff, 0x00800002, 0x7f7fffff, 0x00800002,
        ),
        (
            0xee200a20, 0x7f7fffff, 0x3f800000, 0x0340000d, 0x7f7fffff, 0x0340000d,
        ),
        (
            0xee200a20, 0x7f7fffff, 0x7f800000, 0x00000010, 0x7f800000, 0x00000010,
        ),
        (
            0xee200a20, 0xff7fffff, 0x7f7fffff, 0x0280001a, 0xff800000, 0x0280001e,
        ),
        (
            0xee200a20, 0xff800000, 0x00000000, 0x0300000c, 0x7fc00000, 0x0300000d,
        ),
        (
            0xee200a20, 0x7f812345, 0x00000000, 0x01000004, 0x7fc12345, 0x01000005,
        ),
        (
            0xee200a20, 0x7f812345, 0x7f812345, 0x03c0000f, 0x7fc00000, 0x03c0000f,
        ),
        (
            0xee300a20, 0x3fa00000, 0x3fc00000, 0xa0000013, 0x40300000, 0xa0000013,
        ),
        (
            0xee200a20, 0x3fa00000, 0x3fc00000, 0xa0000013, 0x3ff00000, 0xa0000013,
        ),
        (
            0xee300a60, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
        ),
        (
            0xee300a60, 0x80000000, 0x80000000, 0x0340000d, 0x00000000, 0x0340000d,
        ),
        (
            0xee300a60, 0x00800000, 0x00000000, 0x00000010, 0x00800000, 0x00000010,
        ),
        (
            0xee300a60, 0x00800000, 0x00800000, 0x01000014, 0x00000000, 0x01000014,
        ),
        (
            0xee300a60, 0x3f800000, 0x3f800000, 0x00400001, 0x00000000, 0x00400001,
        ),
        (
            0xee300a60, 0x3f800000, 0x7f7fffff, 0x00800002, 0xff7fffff, 0x00800012,
        ),
        (
            0xee300a60, 0x7f7fffff, 0x3f800000, 0x0340000d, 0x7f7fffff, 0x0340001d,
        ),
        (
            0xee300a60, 0x7f7fffff, 0x7f800000, 0x00000010, 0xff800000, 0x00000010,
        ),
        (
            0xee300a60, 0xff7fffff, 0x7f7fffff, 0x0280001a, 0xff800000, 0x0280001e,
        ),
        (
            0xee300a60, 0xff800000, 0x00000000, 0x0300000c, 0xff800000, 0x0300000c,
        ),
        (
            0xee300a60, 0x7f812345, 0x00000000, 0x01000004, 0x7fc12345, 0x01000005,
        ),
        (
            0xee300a60, 0x7f812345, 0x7f812345, 0x03c0000f, 0x7fc00000, 0x03c0000f,
        ),
        (
            0xee300a60, 0x3fc00000, 0x3fa00000, 0xa0000013, 0x3e800000, 0xa0000013,
        ),
        (
            0xee300a60, 0x3fa00000, 0x3fc00000, 0x00000010, 0xbe800000, 0x00000010,
        ),
        (
            0xee300a60, 0x3f800000, 0x3f800000, 0x00000010, 0x00000000, 0x00000010,
        ),
        (
            0xee300a60, 0x3f800000, 0x3f800000, 0x00800010, 0x80000000, 0x00800010,
        ),
        (
            0xee300a60, 0x00800001, 0x00800000, 0x00000010, 0x00000001, 0x00000010,
        ),
        (
            0xee300a60, 0x00800001, 0x00800000, 0x01000010, 0x00000000, 0x01000018,
        ),
        (
            0xee300a60, 0x3f800000, 0xffc12345, 0x00000000, 0xffc12345, 0x00000000,
        ),
        (
            0xee300a60, 0x7fc12345, 0x3f800000, 0x00000000, 0x7fc12345, 0x00000000,
        ),
    ];
    for (op, a, b, flags, result, status) in cases {
        code(
            &mut cpu,
            ENTRY,
            &[0xee000a10, 0xee001a90, op, 0xee100a10, 0xe12fff1e],
        );
        for (reg, value) in [
            (ArmReg::R0, a),
            (ArmReg::R1, b),
            (ArmReg::R4, 0x12345678),
            (ArmReg::R11, 0x87654321),
            (ArmReg::Sp, 0x2f00),
            (ArmReg::Lr, STOP),
        ] {
            cpu.write_reg(reg, value).unwrap();
        }
        cpu.write_fpscr(flags).unwrap();
        assert_eq!(
            cpu.run_until_hook(ENTRY, 0).unwrap(),
            StopReason::Hook(STOP)
        );
        assert_eq!(
            cpu.read_reg(ArmReg::R0).unwrap(),
            result,
            "op={op:x} a={a:x} b={b:x}"
        );
        assert_eq!(
            cpu.read_fpscr().unwrap(),
            status,
            "op={op:x} a={a:x} b={b:x}"
        );
        for (reg, value) in [
            (ArmReg::R4, 0x12345678),
            (ArmReg::R11, 0x87654321),
            (ArmReg::Sp, 0x2f00),
        ] {
            assert_eq!(cpu.read_reg(reg).unwrap(), value);
        }
    }
}

#[test]
fn guest_fpscr_changes_and_clear_inexact_take_effect_in_the_same_block() {
    let mut cpu = setup();
    // ADD(1,+half-ulp) and SUB(1,-half-ulp) must both honour each rounding mode.
    for (op, operand) in [(0xee300a20, 0x33800000), (0xee300a60, 0xb3800000)] {
        code(
            &mut cpu,
            ENTRY,
            &[
                0xee000a10, 0xee001a90, // VMOV operands
                0xeee13a10, // VMSR FPSCR,R3, replacing initially eligible controls
                op, 0xeef14a10, // arithmetic; VMRS R4,FPSCR
                0xee100a10, 0xe12fff1e,
            ],
        );
        for mode in 0..4 {
            cpu.write_fpscr(0x10).unwrap();
            for (reg, value) in [
                (ArmReg::R0, 0x3f800000),
                (ArmReg::R1, operand),
                (ArmReg::R3, mode << 22),
                (ArmReg::Lr, STOP),
            ] {
                cpu.write_reg(reg, value).unwrap();
            }
            assert_eq!(
                cpu.run_until_hook(ENTRY, 0).unwrap(),
                StopReason::Hook(STOP)
            );
            assert_eq!(
                cpu.read_reg(ArmReg::R0).unwrap(),
                if mode == 1 { 0x3f800001 } else { 0x3f800000 }
            );
            assert_eq!(cpu.read_reg(ArmReg::R4).unwrap(), (mode << 22) | 0x10);
        }
    }
}

#[test]
fn vfp_forwarding_respects_source_overwrites_aliases_predication_and_observers() {
    let mut cpu = setup();
    for predicate in [0, 1] {
        for (words, expected) in [
            (
                vec![0xee000a10, 0xe3a00000, 0xee100a10, 0xe12fff1e],
                0x11111111,
            ),
            (
                vec![0xec410b10, 0xec432b11, 0xeeb00b41, 0xee100a10, 0xe12fff1e],
                0x33333333,
            ),
            (
                vec![0xee000a10, 0xe3510000, 0x1e002a10, 0xee100a10, 0xe12fff1e],
                if predicate == 0 {
                    0x11111111
                } else {
                    0x33333333
                },
            ),
        ] {
            code(&mut cpu, ENTRY, &words);
            for (reg, value) in [
                (ArmReg::R0, 0x11111111),
                (ArmReg::R1, predicate),
                (ArmReg::R2, 0x33333333),
                (ArmReg::R3, 0x44444444),
                (ArmReg::Lr, STOP),
            ] {
                cpu.write_reg(reg, value).unwrap();
            }
            assert_eq!(
                cpu.run_until_hook(ENTRY, 0).unwrap(),
                StopReason::Hook(STOP)
            );
            assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), expected);
        }
    }
    let mut cpu = setup();
    code(&mut cpu, ENTRY, &[0xee000a10, 0xee100a10, 0xe12fff1e]);
    cpu.add_code_hook(ENTRY + 4).unwrap();
    cpu.write_reg(ArmReg::R0, 0x12345678).unwrap();
    cpu.write_reg(ArmReg::Lr, STOP).unwrap();
    assert_eq!(
        cpu.run_until_hook(ENTRY, 0).unwrap(),
        StopReason::Hook(ENTRY + 4)
    );
    cpu.write_reg(ArmReg::R0, 0x87654321).unwrap();
    code(&mut cpu, 0x1100, &[0xee100a10, 0xe12fff1e]);
    assert_eq!(
        cpu.run_until_hook(0x1100, 0).unwrap(),
        StopReason::Hook(STOP)
    );
    assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 0x12345678);
}
