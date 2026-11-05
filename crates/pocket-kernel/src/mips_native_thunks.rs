//! Register-only MIPS runtime helpers, executed without an HLE round trip.
//!
//! The backend already executes MIPS32. These stubs implement the WinCE
//! software-runtime ABI; they do not change instructions in guest images.

pub(super) fn native_thunk_for(dll: &str, name: &str) -> Option<[u32; 8]> {
    if !dll.eq_ignore_ascii_case("coredll.dll") || name != "__ll_mul" {
        return None;
    }
    // (a0 + a1*2^32)*(a2 + a3*2^32), modulo 2^64, returned in v0:v1.
    // Capture both halves of the low-word product before MUL clobbers HI/LO.
    // t0/t1 and HI/LO are caller-saved; arguments, saved registers and SP stay
    // untouched. The final addition occupies JR RA's mandatory delay slot.
    Some([
        0x0086_0019, // multu a0,a2
        0x0000_1012, // mflo v0
        0x0000_1810, // mfhi v1
        0x7087_4002, // mul t0,a0,a3
        0x70a6_4802, // mul t1,a1,a2
        0x0068_1821, // addu v1,v1,t0
        0x03e0_0008, // jr ra
        0x0069_1821, // addu v1,v1,t1 (delay slot)
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_matching_runtime_export_uses_the_mips_stub() {
        assert!(native_thunk_for("COREDLL.dll", "__ll_mul").is_some());
        assert!(native_thunk_for("other.dll", "__ll_mul").is_none());
        assert!(native_thunk_for("coredll.dll", "__ll_div").is_none());
    }

    #[cfg(feature = "unicorn-tests")]
    #[test]
    fn multiply_executes_with_exact_wrapping_result_and_preserves_the_abi() {
        use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Arch, Cpu, Prot};
        let mut cpu = UnicornCpu::new_for_arch(Arch::Mips).unwrap();
        cpu.map_region(0x1000, 0x2000, Prot::ALL).unwrap();
        let words = native_thunk_for("coredll.dll", "__ll_mul").unwrap();
        cpu.write_mem(0x1100, &crate::native_thunks::thunk_bytes(&words))
            .unwrap();
        cpu.write_mem(
            0x1000,
            // jal 0x1100; nop; move gp,v1 (expose the high return word); break.
            &crate::native_thunks::thunk_bytes(&[
                0x0c00_0440,
                0,
                0x0060_e021,
                0x0000_000d,
                0,
                0,
                0,
                0,
            ]),
        )
        .unwrap();
        cpu.add_code_hook(0x100c).unwrap();
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut cases = vec![
            (0, 0),
            (0, u64::MAX),
            (u64::MAX, 2),
            ((-255i64) as u64, 65536),
            (0x100000001, 0x100000001),
            (u64::MAX, u64::MAX),
            (1 << 63, 2),
        ];
        for _ in 0..256 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let a = seed;
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            cases.push((a, seed));
        }
        for (a, b) in cases {
            for (r, v) in [
                (ArmReg::R0, a as u32),
                (ArmReg::R1, (a >> 32) as u32),
                (ArmReg::R2, b as u32),
                (ArmReg::R3, (b >> 32) as u32),
                (ArmReg::Sp, 0x2ff0),
                (ArmReg::R4, 0x13579bdf),
            ] {
                cpu.write_reg(r, v).unwrap();
            }
            cpu.run_until_hook(0x1000, 1000).unwrap();
            let product = a.wrapping_mul(b);
            assert_eq!(cpu.read_return().unwrap(), product as u32);
            assert_eq!(cpu.read_reg(ArmReg::R12).unwrap(), (product >> 32) as u32);
            assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), a as u32);
            assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), (a >> 32) as u32);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x2ff0);
            assert_eq!(cpu.read_reg(ArmReg::R4).unwrap(), 0x13579bdf);
            assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1008);
            assert_eq!(cpu.read_reg(ArmReg::Pc).unwrap(), 0x100c);
        }
    }
}
