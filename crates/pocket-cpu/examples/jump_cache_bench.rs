//! Synthetic ARM calls/returns whose targets collide in the original jump cache.
//! Run twice with POCKETHLE_JUMP_CACHE_BITS=12 / 16 to compare source builds.
#[cfg(feature = "unicorn")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};
    use std::{collections::HashSet, time::Instant};
    fn index(pc: u32, bits: u32) -> u32 {
        let page_bits = bits / 2;
        let page_size = 1 << page_bits;
        let mixed = pc ^ (pc >> (12 - page_bits));
        ((mixed >> (12 - page_bits)) & ((1 << bits) - page_size)) | (mixed & (page_size - 1))
    }
    fn branch(from: u32, to: u32, link: bool) -> u32 {
        let opcode = if link { 0xeb00_0000 } else { 0xea00_0000 };
        opcode | (((i64::from(to) - i64::from(from) - 8) / 4) as u32 & 0x00ff_ffff)
    }
    let iterations: u32 = std::env::args()
        .nth(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(100_000);
    if iterations == 0 || iterations > u32::MAX / 16 {
        return Err("iterations must be between 1 and u32::MAX / 16".into());
    }
    let mut destinations = Vec::new();
    let mut expanded_indices = HashSet::new();
    for pc in (0x20000..0x210000).step_by(8) {
        if index(pc, 12) == index(0x20000, 12) && expanded_indices.insert(index(pc, 16)) {
            destinations.push(pc);
            if destinations.len() == 16 {
                break;
            }
        }
    }
    assert_eq!(destinations.len(), 16);
    let mut cpu = UnicornCpu::new()?;
    cpu.map_region(0x10000, 0x200000, Prot::READ | Prot::WRITE | Prot::EXEC)?;
    for &target in &destinations {
        cpu.write_mem(target, &[0x01, 0x00, 0x80, 0xe2, 0x1e, 0xff, 0x2f, 0xe1])?;
        // add r0,#1; bx lr
    }
    let mut program = Vec::new();
    for (i, &target) in destinations.iter().enumerate() {
        program.extend_from_slice(&branch(0x10000 + i as u32 * 4, target, true).to_le_bytes());
    }
    program.extend_from_slice(&0xe254_4001u32.to_le_bytes()); // subs r4,r4,#1
    let back = 0x10000 + program.len() as u32;
    program.extend_from_slice(
        &(branch(back, 0x10000, false) & 0x0fff_ffff | 0x1000_0000).to_le_bytes(),
    );
    let done = 0x10000 + program.len() as u32;
    program.extend_from_slice(&0xe1a0_0000u32.to_le_bytes());
    cpu.write_mem(0x10000, &program)?;
    cpu.add_code_hook(done)?;
    for sample in 0..5 {
        cpu.write_reg(ArmReg::R0, 0)?;
        cpu.write_reg(ArmReg::R4, iterations)?;
        let started = Instant::now();
        assert_eq!(cpu.run_until_hook(0x10000, 0)?, StopReason::Hook(done));
        let elapsed = started.elapsed();
        assert_eq!(cpu.read_reg(ArmReg::R0)?, iterations * 16);
        assert_eq!(cpu.read_reg(ArmReg::R4)?, 0);
        println!(
            "sample={sample} calls={} elapsed_ms={:.3}",
            iterations * 16,
            elapsed.as_secs_f64() * 1000.0
        );
    }
    Ok(())
}
#[cfg(not(feature = "unicorn"))]
fn main() {
    eprintln!("Enable --features unicorn to run this benchmark");
}
