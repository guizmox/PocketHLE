#![cfg(feature = "unicorn-tests")]
use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};
use pocket_kernel::native_thunks::*;
fn setup(slot: u32) -> (UnicornCpu, u32) {
    let mut cpu = UnicornCpu::new().unwrap();
    cpu.map_region(slot, 0x1000, Prot::ALL).unwrap();
    cpu.map_region(TICK_PAGE_VA, 0x1000, Prot::ALL).unwrap();
    cpu.map_region(0x1000, 0x1000, Prot::ALL).unwrap();
    cpu.add_code_hook(0x1100).unwrap();
    let helper = if slot == 0x30000000 {
        slot + 0x800
    } else {
        SMALL_COPY_HELPER
    };
    install_small_copy_helper_at(&mut cpu, helper).unwrap();
    (cpu, helper)
}
fn reference(value: u64, signed: bool) -> u64 {
    let negative = signed && (value as i64) < 0;
    let magnitude = if negative {
        (-(value as i64 as i128)) as u128
    } else {
        value as u128
    };
    if magnitude == 0 {
        return 0;
    }
    let mut exponent = 127 - magnitude.leading_zeros();
    let mut significand = if exponent <= 52 {
        magnitude << (52 - exponent)
    } else {
        let shift = exponent - 52;
        let mut n = magnitude >> shift;
        let rest = magnitude & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        if rest > half || (rest == half && n & 1 != 0) {
            n += 1;
        }
        n
    };
    if significand == (1u128 << 53) {
        significand >>= 1;
        exponent += 1;
    }
    ((negative as u64) << 63)
        | ((u64::from(exponent) + 1023) << 52)
        | ((significand as u64) & ((1u64 << 52) - 1))
}
#[test]
fn native_integer_double_rounding_matches_integer_oracle_and_preserves_fpscr() {
    let mut values = vec![0, 1, u64::MAX, i64::MAX as u64, i64::MIN as u64];
    for n in 1..64 {
        for v in [(1u64 << n) - 1, 1u64 << n, (1u64 << n) + 1] {
            values.push(v);
            values.push(v.wrapping_neg());
        }
    }
    let mut seed = 0x123456789abcdeffu64;
    for _ in 0..1000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        values.push(seed);
    }
    for slot in [0x70000000, 0x30000000] {
        let (mut cpu, helper) = setup(slot);
        for (i, name) in ["__i64tod", "__u64tod"].iter().enumerate() {
            let address = slot + i as u32 * 32;
            cpu.write_mem(
                address,
                &thunk_bytes(
                    &native_thunk_with_helper("coredll.dll", name, address, helper).unwrap(),
                ),
            )
            .unwrap();
            for mode in 0..4 {
                for &v in &values {
                    for thumb in [false, true] {
                        for (reg, n) in [
                            (ArmReg::R0, v as u32),
                            (ArmReg::R1, (v >> 32) as u32),
                            (ArmReg::R2, 0x12345678),
                            (ArmReg::R4, 0x23456789),
                            (ArmReg::Sp, 0x1f00),
                            (ArmReg::Lr, 0x1100 | u32::from(thumb)),
                        ] {
                            cpu.write_reg(reg, n).unwrap();
                        }
                        let flags = (cpu.read_reg(ArmReg::Cpsr).unwrap() & 0x0fffffff) | 0xa0000000;
                        cpu.write_reg(ArmReg::Cpsr, flags).unwrap();
                        cpu.write_fpscr(0x03009f9f | (mode << 22)).unwrap();
                        let fpscr = cpu.read_fpscr().unwrap();
                        assert_eq!(
                            cpu.run_until_hook(address, 100).unwrap(),
                            StopReason::Hook(0x1100)
                        );
                        let result = u64::from(cpu.read_reg(ArmReg::R0).unwrap())
                            | (u64::from(cpu.read_reg(ArmReg::R1).unwrap()) << 32);
                        assert_eq!(result, reference(v, i == 0), "{name} {v:016x} mode={mode}");
                        assert_eq!(cpu.read_fpscr().unwrap(), fpscr);
                        assert_eq!(cpu.read_reg(ArmReg::R2).unwrap(), 0x12345678);
                        assert_eq!(cpu.read_reg(ArmReg::R4).unwrap(), 0x23456789);
                        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x1f00);
                        assert_eq!(cpu.read_reg(ArmReg::Cpsr).unwrap() & !0x20, flags & !0x20);
                        assert_eq!(
                            cpu.read_reg(ArmReg::Cpsr).unwrap() & 0x20,
                            if thumb { 0x20 } else { 0 }
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn double_integer_internal_path_keeps_runtime_edges_and_thumb_abi() {
    let (mut cpu, _) = setup(0x70000000);
    let values = [
        0.0,
        -0.0,
        0.25,
        -0.25,
        1.99,
        -1.99,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        2147483648.5,
        -2147483649.5,
        9007199254740992.0,
        9223372036854775808.0,
        -9223372036854775808.0,
        18446744073709551616.0,
    ];
    for (i, name) in ["__dtoi64", "__dtou64"].iter().enumerate() {
        let address = 0x70000000 + i as u32 * 32;
        assert!(install_inline_arithmetic64(&mut cpu, address, "coredll.dll", name).unwrap());
        for value in values {
            for thumb in [false, true] {
                let bits = value.to_bits();
                cpu.write_reg(ArmReg::R0, bits as u32).unwrap();
                cpu.write_reg(ArmReg::R1, (bits >> 32) as u32).unwrap();
                cpu.write_reg(ArmReg::R2, 0xdeadbeef).unwrap();
                cpu.write_reg(ArmReg::R3, 0xcafebabe).unwrap();
                cpu.write_reg(ArmReg::Lr, 0x1100 | u32::from(thumb))
                    .unwrap();
                cpu.write_fpscr(0x01800011).unwrap();
                let fpscr = cpu.read_fpscr().unwrap();
                assert_eq!(
                    cpu.run_until_hook(address, 100).unwrap(),
                    StopReason::Hook(0x1100)
                );
                let actual = u64::from(cpu.read_reg(ArmReg::R0).unwrap())
                    | (u64::from(cpu.read_reg(ArmReg::R1).unwrap()) << 32);
                let expected = if i == 0 {
                    (value as i64) as u64
                } else if !value.is_finite() || value < 0.0 {
                    0
                } else {
                    value as u64
                };
                assert_eq!(actual, expected, "{name} {value:?}");
                assert_eq!(cpu.read_fpscr().unwrap(), fpscr);
                assert_eq!(cpu.read_reg(ArmReg::R2).unwrap(), 0xdeadbeef);
                assert_eq!(cpu.read_reg(ArmReg::R3).unwrap(), 0xcafebabe);
                assert_eq!(
                    cpu.read_reg(ArmReg::Cpsr).unwrap() & 0x20,
                    if thumb { 0x20 } else { 0 }
                );
            }
        }
    }
}
