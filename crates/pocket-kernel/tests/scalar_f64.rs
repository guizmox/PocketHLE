#![cfg(feature = "unicorn-tests")]

use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};

const ENTRY: u32 = 0x1000;
const STOP: u32 = 0x1800;
const OPS: [u32; 3] = [0xee300b01, 0xee200b01, 0xee300b41]; // VADD/VMUL/VSUB D0,D0,D1

fn setup() -> UnicornCpu {
    let mut cpu = UnicornCpu::new().unwrap();
    cpu.map_region(ENTRY, 0x2000, Prot::ALL).unwrap();
    cpu.add_code_hook(STOP).unwrap();
    cpu
}

fn code(cpu: &mut UnicornCpu, words: &[u32]) {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    cpu.write_mem(ENTRY, &bytes).unwrap();
    cpu.flush_instruction_cache(None).unwrap();
}

fn operands(cpu: &mut UnicornCpu, a: u64, b: u64) {
    for (reg, value) in [
        (ArmReg::R0, a as u32),
        (ArmReg::R1, (a >> 32) as u32),
        (ArmReg::R2, b as u32),
        (ArmReg::R3, (b >> 32) as u32),
        (ArmReg::Lr, STOP),
    ] {
        cpu.write_reg(reg, value).unwrap();
    }
}

fn result(cpu: &mut UnicornCpu) -> u64 {
    u64::from(cpu.read_reg(ArmReg::R0).unwrap())
        | (u64::from(cpu.read_reg(ArmReg::R1).unwrap()) << 32)
}

#[test]
fn double_arithmetic_preserves_precision_special_values_and_flags() {
    struct Case {
        a: u64,
        b: u64,
        flags: u32,
        expected: [(u64, u32); 3],
    }
    // Original complete ARM helper fixtures, including bit 52 (lost in F32),
    // halfway rounding, tiny results, signed zeros and full-width NaN payloads.
    let cases = [
        Case {
            a: 0x3ff0000000000001,
            b: 0x3ff0000000000000,
            flags: 0x00000010,
            expected: [
                (0x4000000000000000, 0x00000010),
                (0x3ff0000000000001, 0x00000010),
                (0x3cb0000000000000, 0x00000010),
            ],
        },
        Case {
            a: 0x3ff4000000000000,
            b: 0x3ff8000000000000,
            flags: 0xa0000013,
            expected: [
                (0x4006000000000000, 0xa0000013),
                (0x3ffe000000000000, 0xa0000013),
                (0xbfd0000000000000, 0xa0000013),
            ],
        },
        Case {
            a: 0x0010000000000001,
            b: 0x0010000000000000,
            flags: 0x00000010,
            expected: [
                (0x0020000000000000, 0x00000010),
                (0x0000000000000000, 0x00000018),
                (0x0000000000000001, 0x00000010),
            ],
        },
        Case {
            a: 0x0010000000000001,
            b: 0x0010000000000000,
            flags: 0x01000010,
            expected: [
                (0x0020000000000000, 0x01000010),
                (0x0000000000000000, 0x01000018),
                (0x0000000000000000, 0x01000018),
            ],
        },
        Case {
            a: 0x3ff0000000000000,
            b: 0xfff8123456789abc,
            flags: 0x00000000,
            expected: [
                (0xfff8123456789abc, 0x00000000),
                (0xfff8123456789abc, 0x00000000),
                (0xfff8123456789abc, 0x00000000),
            ],
        },
        Case {
            a: 0x7fefffffffffffff,
            b: 0xffefffffffffffff,
            flags: 0x00000000,
            expected: [
                (0x0000000000000000, 0x00000000),
                (0xfff0000000000000, 0x00000014),
                (0x7ff0000000000000, 0x00000014),
            ],
        },
        Case {
            a: 0x8000000000000000,
            b: 0x0000000000000000,
            flags: 0x00000000,
            expected: [
                (0x0000000000000000, 0x00000000),
                (0x8000000000000000, 0x00000000),
                (0x8000000000000000, 0x00000000),
            ],
        },
        Case {
            a: 0x3ff0000000000000,
            b: 0x3ca0000000000000,
            flags: 0x00000000,
            expected: [
                (0x3ff0000000000000, 0x00000010),
                (0x3ca0000000000000, 0x00000000),
                (0x3fefffffffffffff, 0x00000000),
            ],
        },
        Case {
            a: 0x3ff0000000000000,
            b: 0x3ca0000000000000,
            flags: 0x00000010,
            expected: [
                (0x3ff0000000000000, 0x00000010),
                (0x3ca0000000000000, 0x00000010),
                (0x3fefffffffffffff, 0x00000010),
            ],
        },
        Case {
            a: 0x7ff0123456789abc,
            b: 0x3ff0000000000000,
            flags: 0x00000000,
            expected: [
                (0x7ff8123456789abc, 0x00000001),
                (0x7ff8123456789abc, 0x00000001),
                (0x7ff8123456789abc, 0x00000001),
            ],
        },
        Case {
            a: 0x7ff0000000000000,
            b: 0x7ff0000000000000,
            flags: 0x00000000,
            expected: [
                (0x7ff0000000000000, 0x00000000),
                (0x7ff0000000000000, 0x00000000),
                (0x7ff8000000000000, 0x00000001),
            ],
        },
        Case {
            a: 0x8000000000000000,
            b: 0x8000000000000000,
            flags: 0x00800010,
            expected: [
                (0x8000000000000000, 0x00800010),
                (0x0000000000000000, 0x00800010),
                (0x8000000000000000, 0x00800010),
            ],
        },
    ];
    let mut cpu = setup();
    for case in cases {
        for (op, (expected, flags)) in OPS.into_iter().zip(case.expected) {
            code(
                &mut cpu,
                &[0xec410b10, 0xec432b11, op, 0xec510b10, 0xe12fff1e],
            );
            operands(&mut cpu, case.a, case.b);
            cpu.write_fpscr(case.flags).unwrap();
            cpu.write_reg(ArmReg::R4, 0x12345678).unwrap();
            cpu.write_reg(ArmReg::R11, 0x87654321).unwrap();
            cpu.write_reg(ArmReg::Sp, 0x2f00).unwrap();
            assert_eq!(
                cpu.run_until_hook(ENTRY, 0).unwrap(),
                StopReason::Hook(STOP)
            );
            assert_eq!(
                result(&mut cpu),
                expected,
                "op={op:x} a={:x} b={:x}",
                case.a,
                case.b
            );
            assert_eq!(cpu.read_fpscr().unwrap(), flags);
            for (reg, value) in [
                (ArmReg::R4, 0x12345678),
                (ArmReg::R11, 0x87654321),
                (ArmReg::Sp, 0x2f00),
                (ArmReg::R2, case.b as u32),
                (ArmReg::R3, (case.b >> 32) as u32),
            ] {
                assert_eq!(cpu.read_reg(reg).unwrap(), value);
            }
        }
    }
}

#[test]
fn double_rounding_changes_apply_inside_the_block() {
    let mut cpu = setup();
    // The guest overwrites initially eligible controls after loading D0/D1.
    for (op, b) in [(OPS[0], 0x3ca0000000000000), (OPS[2], 0xbca0000000000000)] {
        code(
            &mut cpu,
            &[
                0xec410b10, 0xec432b11, 0xeee14a10, op, 0xeef15a10, 0xec510b10, 0xe12fff1e,
            ],
        );
        for mode in 0..4 {
            operands(&mut cpu, 0x3ff0000000000000, b);
            cpu.write_fpscr(0x10).unwrap();
            cpu.write_reg(ArmReg::R4, mode << 22).unwrap();
            assert_eq!(
                cpu.run_until_hook(ENTRY, 0).unwrap(),
                StopReason::Hook(STOP)
            );
            assert_eq!(
                result(&mut cpu),
                if mode == 1 {
                    0x3ff0000000000001
                } else {
                    0x3ff0000000000000
                }
            );
            assert_eq!(cpu.read_reg(ArmReg::R5).unwrap(), (mode << 22) | 0x10);
        }
    }
}

#[test]
fn double_writes_invalidate_overlapping_single_register_values() {
    let mut cpu = setup();
    // S0/S1 alias D0. A later D0 write must win over remembered S stores.
    code(
        &mut cpu,
        &[
            0xee004a10, 0xec410b10, 0xec432b11, OPS[0], 0xee100a10, 0xee101a90, 0xe12fff1e,
        ],
    );
    operands(&mut cpu, 0x3ff4000000000000, 0x3ff8000000000000);
    cpu.write_reg(ArmReg::R4, 0x12345678).unwrap();
    cpu.write_fpscr(0x10).unwrap();
    assert_eq!(
        cpu.run_until_hook(ENTRY, 0).unwrap(),
        StopReason::Hook(STOP)
    );
    assert_eq!(result(&mut cpu), 0x4006000000000000);
}
