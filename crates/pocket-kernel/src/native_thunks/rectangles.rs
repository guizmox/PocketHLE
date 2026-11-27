//! Pure RECT operations executed by the ARM JIT, with no USER/GDI state.
//! Keep these helpers in the loader-owned pool so DLL unload and snapshots
//! retain the same memory ownership as the existing CRT helpers.
use super::{pad, BX_LR};

const INFLATE: u32 = 0x300;
const CONTAINS: u32 = 0x340;

pub(super) fn thunk(name: &str, address: u32, helper: u32) -> Option<[u32; 8]> {
    let offset = match name {
        "InflateRect" => INFLATE,
        "PtInRect" => CONTAINS,
        _ => return None,
    };
    let target = helper.checked_add(offset)?;
    let delta = i64::from(target) - (i64::from(address) + 8);
    if delta % 4 != 0 || !(-(1i64 << 25)..(1i64 << 25)).contains(&delta) {
        return None;
    }
    Some(pad(&[0xea000000 | (((delta >> 2) as u32) & 0xffffff)]))
}

pub(super) fn install(
    cpu: &mut dyn pocket_cpu::Cpu,
    address: u32,
) -> Result<(), pocket_cpu::CpuError> {
    // BOOL InflateRect(RECT *r, int dx, int dy). ARM arithmetic wraps at
    // 32 bits, just like WinCE LONG. Only caller-saved registers are touched.
    let inflate: [u32; 16] = [
        0xe3500000, // cmp r0,#0
        0x012fff1e, // bxeq lr (NULL returns FALSE)
        0xe5903000, // ldr r3,[r0]
        0xe0433001, // sub r3,r3,r1
        0xe5803000, // str r3,[r0]
        0xe5903004, // ldr r3,[r0,#4]
        0xe0433002, // sub r3,r3,r2
        0xe5803004, // str r3,[r0,#4]
        0xe5903008, // ldr r3,[r0,#8]
        0xe0833001, // add r3,r3,r1
        0xe5803008, // str r3,[r0,#8]
        0xe590300c, // ldr r3,[r0,#12]
        0xe0833002, // add r3,r3,r2
        0xe580300c, // str r3,[r0,#12]
        0xe3a00001, // mov r0,#1
        BX_LR,
    ];
    // BOOL PtInRect(const RECT *r, POINT p). POINT is passed by value in
    // R1/R2. Left/top are inclusive, right/bottom exclusive, signed compares.
    let contains: [u32; 20] = [
        0xe3500000, // cmp r0,#0
        0x012fff1e, // bxeq lr
        0xe5903000, // ldr r3,[r0]
        0xe1510003, // cmp r1,r3
        0xb3a00000, // movlt r0,#0
        0xb12fff1e, // bxlt lr
        0xe5903004, // ldr r3,[r0,#4]
        0xe1520003, // cmp r2,r3
        0xb3a00000, // movlt r0,#0
        0xb12fff1e, // bxlt lr
        0xe5903008, // ldr r3,[r0,#8]
        0xe1510003, // cmp r1,r3
        0xa3a00000, // movge r0,#0
        0xa12fff1e, // bxge lr
        0xe590300c, // ldr r3,[r0,#12]
        0xe1520003, // cmp r2,r3
        0xa3a00000, // movge r0,#0
        0xa12fff1e, // bxge lr
        0xe3a00001, // mov r0,#1
        BX_LR,
    ];
    for (offset, words) in [
        (INFLATE, inflate.as_slice()),
        (CONTAINS, contains.as_slice()),
    ] {
        let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        cpu.write_mem(address + offset, &bytes)?;
    }
    Ok(())
}

#[cfg(all(test, feature = "unicorn-tests"))]
mod tests {
    use super::*;
    use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};
    const SLOT: u32 = 0x1200;
    const RETURN: u32 = 0x1100;
    const RECT: u32 = 0x3ff0;

    fn cpu(name: &str) -> UnicornCpu {
        let mut cpu = UnicornCpu::new().unwrap();
        cpu.map_region(0x1000, 0x1000, Prot::ALL).unwrap();
        cpu.map_region(0x3000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.map_region(0x6000, 0x1000, Prot::ALL).unwrap();
        super::super::install_small_copy_helper_at(&mut cpu, 0x6000).unwrap();
        let words =
            super::super::native_thunk_with_helper("COREDLL.dll", name, SLOT, 0x6000).unwrap();
        cpu.write_mem(SLOT, &super::super::thunk_bytes(&words))
            .unwrap();
        cpu.add_code_hook(RETURN).unwrap();
        for reg in [
            ArmReg::R4,
            ArmReg::R5,
            ArmReg::R6,
            ArmReg::R7,
            ArmReg::R8,
            ArmReg::R9,
            ArmReg::R10,
            ArmReg::R11,
            ArmReg::Sp,
        ] {
            cpu.write_reg(reg, 0x76543210).unwrap();
        }
        cpu
    }
    fn store(cpu: &mut dyn Cpu, rect: [i32; 4]) {
        let bytes: Vec<_> = rect.iter().flat_map(|v| v.to_le_bytes()).collect();
        cpu.write_mem(RECT, &bytes).unwrap();
    }
    fn call(cpu: &mut dyn Cpu, ptr: u32, x: i32, y: i32) -> u32 {
        for (reg, value) in [
            (ArmReg::R0, ptr),
            (ArmReg::R1, x as u32),
            (ArmReg::R2, y as u32),
            (ArmReg::Lr, RETURN),
        ] {
            cpu.write_reg(reg, value).unwrap();
        }
        assert_eq!(
            cpu.run_until_hook(SLOT, 0).unwrap(),
            StopReason::Hook(RETURN)
        );
        for reg in [
            ArmReg::R4,
            ArmReg::R5,
            ArmReg::R6,
            ArmReg::R7,
            ArmReg::R8,
            ArmReg::R9,
            ArmReg::R10,
            ArmReg::R11,
            ArmReg::Sp,
        ] {
            assert_eq!(cpu.read_reg(reg).unwrap(), 0x76543210);
        }
        cpu.read_reg(ArmReg::R0).unwrap()
    }
    #[test]
    fn inflate_native_rect_handles_null_shrink_and_wrapping_without_adjacent_writes() {
        let mut cpu = cpu("InflateRect");
        assert_eq!(call(&mut cpu, 0, 1, 2), 0);
        cpu.write_mem(RECT - 4, &[0xa5; 4]).unwrap();
        for (rect, dx, dy) in [
            ([10, 20, 40, 60], 3, 4),
            ([10, 20, 40, 60], -50, -60),
            ([i32::MIN, i32::MAX, i32::MAX, i32::MIN], 1, -1),
            ([-5, -9, 5, 9], i32::MIN, i32::MAX),
        ] {
            store(&mut cpu, rect);
            assert_eq!(call(&mut cpu, RECT, dx, dy), 1);
            let expected = [
                rect[0].wrapping_sub(dx),
                rect[1].wrapping_sub(dy),
                rect[2].wrapping_add(dx),
                rect[3].wrapping_add(dy),
            ];
            let actual = cpu.read_mem(RECT, 16).unwrap();
            assert_eq!(
                actual,
                expected
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>()
            );
            assert_eq!(cpu.read_mem(RECT - 4, 4).unwrap(), [0xa5; 4]);
        }
        cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
        cpu.write_reg(ArmReg::R0, RECT).unwrap();
        cpu.write_reg(ArmReg::Lr, RETURN).unwrap();
        assert!(cpu.run_until_hook(SLOT, 0).is_err());
    }
    #[test]
    fn native_point_in_rect_matches_signed_half_open_boundaries_on_read_only_memory() {
        let mut cpu = cpu("PtInRect");
        assert_eq!(call(&mut cpu, 0, 0, 0), 0);
        for rect in [
            [-7, -11, 13, 17],
            [0, 0, 0, 0],
            [10, 10, -10, -10],
            [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
        ] {
            cpu.protect_region(0x3000, 0x1000, Prot::READ | Prot::WRITE)
                .unwrap();
            store(&mut cpu, rect);
            cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
            for x in [
                rect[0].wrapping_sub(1),
                rect[0],
                rect[2].wrapping_sub(1),
                rect[2],
                i32::MIN,
                i32::MAX,
            ] {
                for y in [
                    rect[1].wrapping_sub(1),
                    rect[1],
                    rect[3].wrapping_sub(1),
                    rect[3],
                    i32::MIN,
                    i32::MAX,
                ] {
                    let expected = x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3];
                    assert_eq!(
                        call(&mut cpu, RECT, x, y),
                        u32::from(expected),
                        "{rect:?}, ({x},{y})"
                    );
                }
            }
        }
    }
    #[test]
    fn rectangle_helpers_are_coredll_only_and_reject_unreachable_pools() {
        for name in ["InflateRect", "PtInRect"] {
            assert!(
                super::super::native_thunk_with_helper("other.dll", name, SLOT, 0x6000).is_none()
            );
            assert!(thunk(name, SLOT, 0x80000000).is_none());
            assert!(thunk(name, SLOT, u32::MAX).is_none());
        }
    }
}
