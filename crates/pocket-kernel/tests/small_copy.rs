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
        slot + 0xf00
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
            for len in 0..=16 {
                for src in [0x2200, 0x2201, 0x2202, 0x2203, 0x2fff, 0x3ffb, 0x3ffc] {
                    if src + len > 0x4000 {
                        continue;
                    }
                    for delta in [-15i32, -7, -3, -2, -1, 0, 1, 2, 3, 7, 15, 32] {
                        for thumb in [false, true] {
                            let dst = if src >= 0x3ff0 {
                                0x2301
                            } else {
                                (src as i32 + delta) as u32
                            };
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
    for len in [17, 32, 80, 4096] {
        let before = vec![0xa5; 0x2000];
        cpu.write_mem(0x2000, &before).unwrap();
        args(&mut cpu, 0x2300, 0x2200, len, false);
        assert_eq!(
            cpu.run_until_hook(SLOT, 100).unwrap(),
            StopReason::Hook(SLOT + 28)
        );
        for (reg, value) in [
            (ArmReg::R0, 0x2300),
            (ArmReg::R1, 0x2200),
            (ArmReg::R2, len),
        ] {
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
    let mut cpu = setup(SLOT, "memcpy");
    cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
    args(&mut cpu, 0x3000, 0x2000, 4, false);
    assert!(cpu.run_until_hook(SLOT, 100).is_err());
}
