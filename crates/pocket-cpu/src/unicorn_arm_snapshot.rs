//! Portable register image for the ARM WinCE backend.

use super::*;
use serde::{Deserialize, Serialize};

const MODE_MASK: u32 = 0x1f;
const THUMB: u32 = 0x20;
const USER: u32 = 0x10;
// System shares the user bank and allows switching back to a privileged mode.
const BANKS: [u32; 6] = [0x1f, 0x11, 0x12, 0x13, 0x17, 0x1b];

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmRegisterSnapshot {
    shared: [u32; 8],
    banks: [Bank; BANKS.len()],
    cpsr: u32,
    pc: u32,
    d: [u64; 32],
    fpscr: u32,
    fpexc: u32,
    cpacr: u32,
    tpidruro: u32,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Bank {
    r8_to_lr: [u32; 7],
    spsr: Option<u32>,
}

impl ArmRegisterSnapshot {
    pub(super) fn validate(&self) -> Result<(), CpuError> {
        check_mode(self.cpsr)?;
        if self
            .banks
            .iter()
            .enumerate()
            .any(|(index, bank)| bank.spsr.is_some() != (index != 0))
            || self.pc & if self.cpsr & THUMB == 0 { 3 } else { 1 } != 0
        {
            return Err(CpuError::Backend("invalid ARM snapshot registers".into()));
        }

        Ok(())
    }
    pub(super) fn continuation(&self) -> u32 {
        self.pc | ((self.cpsr & THUMB) >> 5)
    }

    pub(super) fn set_continuation(&mut self, address: u32) {
        self.pc = address & !1;
        self.cpsr = (self.cpsr & !THUMB) | ((address & 1) << 5);
    }
}

impl UnicornCpu {
    /// Capture the architectural registers used by the flat ARM WinCE runtime.
    /// Unicorn contexts also contain host-owned implementation state. This
    /// register image can be loaded in a different emulator instance.
    pub fn capture_arm_registers(&mut self) -> Result<ArmRegisterSnapshot, CpuError> {
        self.require_arm_snapshot()?;
        let cpsr = self.snapshot_read(RegisterARM::CPSR)?;
        let pc = self.snapshot_read(RegisterARM::PC)?;
        check_mode(cpsr)?;

        // Inspect the banks without leaving the guest in a different mode if
        // an underlying register access fails.
        let banks = (|| {
            let mut banks = Vec::with_capacity(BANKS.len());
            for mode in BANKS {
                self.snapshot_write(RegisterARM::CPSR, (cpsr & !MODE_MASK) | mode)?;
                banks.push(Bank {
                    r8_to_lr: self.snapshot_read_gprs(RegisterARM::R8 as i32)?,
                    spsr: if mode == BANKS[0] {
                        None
                    } else {
                        Some(self.snapshot_read(RegisterARM::SPSR)?)
                    },
                });
            }
            Ok::<_, CpuError>(banks.try_into().expect("one record per register bank"))
        })();
        self.snapshot_write(RegisterARM::CPSR, cpsr)?;
        let banks = banks?;

        let mut d = [0; 32];
        for (index, value) in d.iter_mut().enumerate() {
            *value = self.snapshot_read(RegisterARM::D0 as i32 + index as i32)?;
        }
        Ok(ArmRegisterSnapshot {
            shared: self.snapshot_read_gprs(RegisterARM::R0 as i32)?,
            banks,
            cpsr,
            pc,
            d,
            fpscr: self.snapshot_read(RegisterARM::FPSCR)?,
            fpexc: self.snapshot_read(RegisterARM::FPEXC)?,
            cpacr: self.snapshot_read(RegisterARM::C1_C0_2)?,
            tpidruro: self.snapshot_read(RegisterARM::C13_C0_3)?,
        })
    }

    /// Restore into a fresh ARM backend whose guest mappings have already been
    /// rebuilt. A partially restored backend must be discarded on error.
    /// Returns the entry address with its Thumb tag for the next CPU slice.
    pub fn restore_arm_registers(
        &mut self,
        snapshot: &ArmRegisterSnapshot,
    ) -> Result<u32, CpuError> {
        self.require_arm_snapshot()?;
        snapshot.validate()?;

        for (mode, bank) in BANKS.into_iter().zip(&snapshot.banks) {
            self.snapshot_write(RegisterARM::CPSR, (snapshot.cpsr & !MODE_MASK) | mode)?;
            for (index, value) in bank.r8_to_lr.iter().enumerate() {
                self.snapshot_write(banked_register(RegisterARM::R8 as i32, index), *value)?;
            }
            if let Some(spsr) = bank.spsr {
                self.snapshot_write(RegisterARM::SPSR, spsr)?;
            }
        }
        self.snapshot_write(RegisterARM::CPSR, snapshot.cpsr)?;
        for (index, value) in snapshot.shared.iter().enumerate() {
            self.snapshot_write(RegisterARM::R0 as i32 + index as i32, *value)?;
        }
        self.snapshot_write(RegisterARM::C1_C0_2, snapshot.cpacr)?;
        self.snapshot_write(RegisterARM::C13_C0_3, snapshot.tpidruro)?;
        self.snapshot_write(RegisterARM::FPEXC, snapshot.fpexc)?;
        self.snapshot_write(RegisterARM::FPSCR, snapshot.fpscr)?;
        for (index, value) in snapshot.d.iter().enumerate() {
            self.uc
                .reg_write(RegisterARM::D0 as i32 + index as i32, *value)
                .map_err(|error| CpuError::Backend(format!("restore VFP d{index}: {error:?}")))?;
        }

        let entry = snapshot.pc | u32::from(snapshot.cpsr & THUMB != 0);
        self.snapshot_write(RegisterARM::PC, entry)?;
        Ok(entry)
    }

    fn require_arm_snapshot(&self) -> Result<(), CpuError> {
        if self.arch == Arch::Arm {
            Ok(())
        } else {
            Err(CpuError::Unsupported("ARM snapshots on a MIPS backend"))
        }
    }

    fn snapshot_read<T: TryFrom<u64>>(&self, register: impl Into<i32>) -> Result<T, CpuError> {
        let register = register.into();
        self.uc
            .reg_read(register)
            .map_err(|error| CpuError::Backend(format!("snapshot read r{register}: {error:?}")))?
            .try_into()
            .map_err(|_| CpuError::Backend(format!("invalid snapshot register r{register}")))
    }

    fn snapshot_write(&mut self, register: impl Into<i32>, value: u32) -> Result<(), CpuError> {
        let register = register.into();
        self.uc
            .reg_write(register, u64::from(value))
            .map_err(|error| CpuError::Backend(format!("snapshot write r{register}: {error:?}")))
    }

    fn snapshot_read_gprs<const N: usize>(&self, first: i32) -> Result<[u32; N], CpuError> {
        let mut values = [0; N];
        for (index, value) in values.iter_mut().enumerate() {
            *value = self.snapshot_read(banked_register(first, index))?;
        }
        Ok(values)
    }
}

fn banked_register(first: i32, index: usize) -> i32 {
    match first - RegisterARM::R0 as i32 + index as i32 {
        13 => RegisterARM::SP as i32,
        14 => RegisterARM::LR as i32,
        _ => first + index as i32,
    }
}

fn check_mode(cpsr: u32) -> Result<(), CpuError> {
    let mode = cpsr & MODE_MASK;
    if mode == USER || BANKS.contains(&mode) {
        Ok(())
    } else {
        Err(CpuError::Unsupported("ARM snapshot processor mode"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_resumes_thumb_code_in_a_new_unicorn_instance() {
        let mut cpu = UnicornCpu::new().unwrap();
        cpu.snapshot_write(RegisterARM::CPSR, 0x6000_00df).unwrap();
        cpu.snapshot_write(RegisterARM::SP, 0x9000).unwrap();
        cpu.snapshot_write(RegisterARM::CPSR, 0x6000_00d3).unwrap();
        cpu.snapshot_write(RegisterARM::SP, 0xa000).unwrap();
        cpu.snapshot_write(RegisterARM::SPSR, 0x10).unwrap();
        cpu.snapshot_write(RegisterARM::R0, 40).unwrap();
        cpu.snapshot_write(RegisterARM::PC, 0x1001).unwrap();
        cpu.uc
            .reg_write(RegisterARM::D31, 0xfedc_ba98_7654_3210)
            .unwrap();
        cpu.write_fpscr(0x0100_0000).unwrap();
        let before = cpu.capture_arm_registers().unwrap();
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0xa000);
        assert_eq!(cpu.read_reg(ArmReg::Cpsr).unwrap(), 0x6000_00f3);
        let disk = serde_json::to_vec(&before).unwrap();
        drop(cpu);

        let mut cpu = UnicornCpu::new().unwrap();
        cpu.map_region(0x1000, 0x1000, Prot::ALL).unwrap();
        cpu.write_mem(0x1000, &[0x02, 0x30, 0xfe, 0xe7]).unwrap(); // adds r0, #2
        let saved = serde_json::from_slice(&disk).unwrap();
        let pc = cpu.restore_arm_registers(&saved).unwrap();
        assert_eq!(pc, 0x1001);
        assert_eq!(cpu.capture_arm_registers().unwrap(), before);
        cpu.uc.emu_start(u64::from(pc), 0x1002, 0, 1).unwrap();
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 42);
    }
}
