#![cfg(feature = "unicorn-tests")]
use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};
use pocket_kernel::native_thunks::*;
const SLOT: u32 = 0x7000_0000;
fn setup(slot: u32, name: &str) -> UnicornCpu {
    let mut cpu = UnicornCpu::new().unwrap();
    cpu.map_region(slot, 0x1000, Prot::READ | Prot::EXEC)
        .unwrap();
    cpu.map_region(TICK_PAGE_VA, 0x1000, Prot::READ | Prot::WRITE | Prot::EXEC)
        .unwrap();
    let helper = if slot < 0x50000000 {
        slot + 0xc00
    } else {
        SMALL_COPY_HELPER
    };
    install_small_copy_helper_at(&mut cpu, helper).unwrap();
    cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::EXEC)
        .unwrap();
    cpu.map_region(0x2000, 0x2000, Prot::READ | Prot::WRITE)
        .unwrap();
    cpu.write_mem(
        slot,
        &thunk_bytes(&native_thunk_with_helper("coredll.dll", name, slot, helper).unwrap()),
    )
    .unwrap();
    cpu.add_code_hook(slot + 28).unwrap();
    cpu.add_code_hook(0x1100).unwrap();
    cpu
}
fn args(cpu: &mut UnicornCpu, dst: u32, src: u32, len: u32, thumb: bool) {
    for (reg, value) in [
        (ArmReg::R0, dst),
        (ArmReg::R1, src),
        (ArmReg::R2, len),
        (ArmReg::R4, 0x12345678),
        (ArmReg::R11, 0x87654321),
        (ArmReg::Sp, 0x2800),
        (ArmReg::Lr, 0x1100 | u32::from(thumb)),
    ] {
        cpu.write_reg(reg, value).unwrap();
    }
}
#[test]
fn copies_preserve_bytes_overlap_guards_and_arm_thumb_return() {
    for slot in [SLOT, 0x7100_0000, 0x30000000] {
        for name in ["memcpy", "memmove"] {
            let mut cpu = setup(slot, name);
            for len in (0..=16).chain([32, 48]) {
                for src in [
                    0x2200, 0x2201, 0x2202, 0x2203, 0x2fe0, 0x2fff, 0x3ffb, 0x3ffc,
                ] {
                    if src + len > 0x4000 {
                        continue;
                    }
                    for delta in [
                        -32i32, -15, -8, -7, -4, -3, -2, -1, 0, 1, 2, 3, 4, 7, 8, 15, 32,
                    ] {
                        for thumb in [false, true] {
                            let dst = if src >= 0x3ff0 {
                                0x2301
                            } else {
                                (src as i32 + delta) as u32
                            };
                            if len > 16
                                && ((src | dst) & 3 != 0
                                    || (src & 0xfff) + len >= 0x1000
                                    || (dst & 0xfff) + len >= 0x1000)
                            {
                                continue;
                            }
                            let original: Vec<u8> =
                                (0..0x2000).map(|i| (i * 31 + 7) as u8).collect();
                            cpu.write_mem(0x2000, &original).unwrap();
                            let mut expected = original.clone();
                            expected.copy_within(
                                (src - 0x2000) as usize..(src - 0x2000 + len) as usize,
                                (dst - 0x2000) as usize,
                            );
                            args(&mut cpu, dst, src, len, thumb);
                            assert_eq!(
                                cpu.run_until_hook(slot, 500).unwrap(),
                                StopReason::Hook(0x1100)
                            );
                            assert_eq!(cpu.read_mem(0x2000, 0x2000).unwrap(), expected);
                            for (reg, value) in [
                                (ArmReg::R0, dst),
                                (ArmReg::R4, 0x12345678),
                                (ArmReg::R11, 0x87654321),
                                (ArmReg::Sp, 0x2800),
                            ] {
                                assert_eq!(cpu.read_reg(reg).unwrap(), value);
                            }
                            assert_eq!((cpu.read_reg(ArmReg::Cpsr).unwrap() & 0x20) != 0, thumb);
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn other_sizes_reach_host_with_original_arguments_and_without_writes() {
    let mut cpu = setup(SLOT, "memcpy");
    for (dst, src, len) in [
        (0x2300, 0x2200, 17),
        (0x2300, 0x2200, 80),
        (0x2300, 0x2200, 4096),
        (0x2300, 0x2201, 32),
        (0x2301, 0x2200, 48),
        (0x2300, 0x2ff0, 48),
        (0x2ff0, 0x2200, 32),
        (pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE - 16, 0x2200, 32),
        (pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE, 0x2200, 48),
        (0xfffffff0, 0x2200, 48),
    ] {
        let before = vec![0xa5; 0x2000];
        cpu.write_mem(0x2000, &before).unwrap();
        args(&mut cpu, dst, src, len, false);
        assert_eq!(
            cpu.run_until_hook(SLOT, 100).unwrap(),
            StopReason::Hook(SLOT + 28)
        );
        for (reg, value) in [(ArmReg::R0, dst), (ArmReg::R1, src), (ArmReg::R2, len)] {
            assert_eq!(cpu.read_reg(reg).unwrap(), value);
        }
        assert_eq!(cpu.read_mem(0x2000, 0x2000).unwrap(), before);
    }
}
#[test]
fn native_copy_honours_unmapped_and_read_only_memory() {
    for (dst, src) in [
        (0x2000, 0x4000),
        (0x4000, 0x2000),
        (0x2000, 0x3ffe),
        (0x3ffe, 0x2000),
    ] {
        let mut cpu = setup(SLOT, "memcpy");
        args(&mut cpu, dst, src, 4, false);
        assert!(cpu.run_until_hook(SLOT, 100).is_err());
    }
    for len in [4, 32, 48] {
        let mut cpu = setup(SLOT, "memcpy");
        cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
        args(&mut cpu, 0x3000, 0x2000, len, false);
        assert!(cpu.run_until_hook(SLOT, 100).is_err());
        args(&mut cpu, 0x2300, 0x4000, len, false);
        assert!(cpu.run_until_hook(SLOT, 100).is_err());
    }
}

#[test]
fn fills_preserve_guards_truncate_values_and_return_to_arm_thumb() {
    for slot in [SLOT, 0x30000000] {
        let mut cpu = setup(slot, "memset");
        for len in 0..=16 {
            for dst in [0x2200, 0x2201, 0x2202, 0x2203, 0x2fff, 0x4000 - len] {
                for value in [0, 1, 255, 256, 0x12345678, u32::MAX] {
                    for thumb in [false, true] {
                        let original = vec![0xa5; 0x2000];
                        cpu.write_mem(0x2000, &original).unwrap();
                        let mut expected = original;
                        expected[(dst - 0x2000) as usize..(dst - 0x2000 + len) as usize]
                            .fill(value as u8);
                        args(&mut cpu, dst, value, len, thumb);
                        assert_eq!(
                            cpu.run_until_hook(slot, 200).unwrap(),
                            StopReason::Hook(0x1100)
                        );
                        assert_eq!(cpu.read_mem(0x2000, 0x2000).unwrap(), expected);
                        for (reg, wanted) in [
                            (ArmReg::R0, dst),
                            (ArmReg::R4, 0x12345678),
                            (ArmReg::R11, 0x87654321),
                            (ArmReg::Sp, 0x2800),
                        ] {
                            assert_eq!(cpu.read_reg(reg).unwrap(), wanted);
                        }
                        assert_eq!((cpu.read_reg(ArmReg::Cpsr).unwrap() & 0x20) != 0, thumb);
                    }
                }
            }
        }
        for len in [17, 32, 4096, u32::MAX] {
            cpu.write_mem(0x2000, &vec![0xa5; 0x2000]).unwrap();
            args(&mut cpu, 0x2300, 0x12345678, len, false);
            assert_eq!(
                cpu.run_until_hook(slot, 100).unwrap(),
                StopReason::Hook(slot + 28)
            );
            for (reg, wanted) in [
                (ArmReg::R0, 0x2300),
                (ArmReg::R1, 0x12345678),
                (ArmReg::R2, len),
            ] {
                assert_eq!(cpu.read_reg(reg).unwrap(), wanted);
            }
            assert_eq!(cpu.read_mem(0x2000, 0x2000).unwrap(), vec![0xa5; 0x2000]);
        }
        args(&mut cpu, 0, 123, 0, false);
        assert_eq!(
            cpu.run_until_hook(slot, 100).unwrap(),
            StopReason::Hook(0x1100)
        );
        for dst in [0, 0x4000, 0x3fff] {
            args(&mut cpu, dst, 1, 4, false);
            assert!(cpu.run_until_hook(slot, 100).is_err());
        }
    }
    let mut cpu = setup(SLOT, "memset");
    cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
    args(&mut cpu, 0x3000, 1, 4, false);
    assert!(cpu.run_until_hook(SLOT, 100).is_err());
}
