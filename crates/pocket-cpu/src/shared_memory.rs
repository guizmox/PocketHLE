//! Stable host-paged backing for coherent CE mapped views.
//! No Rust slices refer to bytes that guest CPUs may modify. File-backed
//! paging/writeback is delegated to the host OS; anonymous objects are charged
//! to the emulated device by the kernel owner, not by this backend module.
use crate::{CpuError, Prot};
use memmap2::{MmapOptions, MmapRaw};
use std::{any::Any, fs::File, io, sync::Arc};

pub struct SharedMemory {
    raw: MmapRaw,
    len: u32,
    writable: bool,
    file_backed: bool,
}
impl std::fmt::Debug for SharedMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedMemory")
            .field("len", &self.len)
            .field("writable", &self.writable)
            .field("file_backed", &self.file_backed)
            .finish()
    }
}
impl SharedMemory {
    pub fn anonymous(len: u32) -> io::Result<Arc<Self>> {
        let rounded =
            rounded_len(len).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let raw = MmapOptions::new().len(rounded as usize).map_anon()?.into();
        Ok(Arc::new(Self {
            raw,
            len,
            writable: true,
            file_backed: false,
        }))
    }
    /// The caller must retain the file's VFS mapping pin. The final page is
    /// host-mapped, but bytes past logical EOF are never included in a flush.
    pub fn file(file: &File, len: u32, writable: bool) -> io::Result<Arc<Self>> {
        if rounded_len(len).is_none() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut options = MmapOptions::new();
        options.len(len as usize);
        let raw = if writable {
            options.map_raw(file)?
        } else {
            options.map_raw_read_only(file)?
        };
        Ok(Arc::new(Self {
            raw,
            len,
            writable,
            file_backed: true,
        }))
    }
    pub fn len(&self) -> u32 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn writable(&self) -> bool {
        self.writable
    }
    pub fn mapped_len(&self) -> u32 {
        rounded_len(self.len).expect("validated mapping length")
    }
    pub(crate) fn pointer(&self, offset: u32) -> *mut u8 {
        self.raw.as_mut_ptr().wrapping_add(offset as usize)
    }
    /// # Safety
    /// No CPU or other thread may access these bytes during the copy.
    pub unsafe fn read(&self, offset: u32, out: &mut [u8]) -> Result<(), CpuError> {
        self.check(offset, out.len(), false)?;
        unsafe {
            std::ptr::copy_nonoverlapping(self.pointer(offset), out.as_mut_ptr(), out.len());
        }
        Ok(())
    }
    /// # Safety
    /// No CPU or other thread may access these bytes during the copy.
    pub unsafe fn write(&self, offset: u32, input: &[u8]) -> Result<(), CpuError> {
        self.check(offset, input.len(), true)?;
        unsafe {
            std::ptr::copy_nonoverlapping(input.as_ptr(), self.pointer(offset), input.len());
        }
        Ok(())
    }
    fn check(&self, offset: u32, len: usize, write: bool) -> Result<(), CpuError> {
        if (write && !self.writable)
            || u64::from(offset) + len as u64 > u64::from(self.mapped_len())
        {
            return Err(CpuError::BadMemory {
                va: offset,
                size: len as u32,
            });
        }
        Ok(())
    }
    pub fn flush(&self, offset: u32, len: u32) -> io::Result<()> {
        if u64::from(offset) + u64::from(len) > u64::from(self.len) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if !self.file_backed || !self.writable || len == 0 {
            return Ok(());
        }
        self.raw.flush_range(offset as usize, len as usize)
    }
}
pub fn rounded_len(len: u32) -> Option<u32> {
    (len != 0)
        .then_some(len)?
        .checked_add(4095)
        .map(|n| n & !4095)
}
/// Keeps the kernel section object (and its file lease/RAM charge) alive even
/// if the process namespace drops before its CPU backend is destroyed.
pub struct SharedRegion {
    pub memory: Arc<SharedMemory>,
    _owner: Arc<dyn Any + Send + Sync>,
}
impl std::fmt::Debug for SharedRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.memory.fmt(f)
    }
}
impl SharedRegion {
    pub fn new<T: Any + Send + Sync>(memory: Arc<SharedMemory>, owner: Arc<T>) -> Arc<Self> {
        Arc::new(Self {
            memory,
            _owner: owner,
        })
    }
    pub fn validate(&self, va: u32, size: u32, offset: u32, prot: Prot) -> Result<(), CpuError> {
        if size == 0
            || va % 4096 != 0
            || size % 4096 != 0
            || offset % 4096 != 0
            || va.checked_add(size).is_none()
            || u64::from(offset) + u64::from(size) > u64::from(self.memory.mapped_len())
            || (prot.contains(Prot::WRITE) && !self.memory.writable())
        {
            return Err(CpuError::BadMemory { va, size });
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "unicorn"))]
mod tests {
    use super::*;
    use crate::{regs::ArmReg, unicorn::UnicornCpu, Cpu, StopReason};
    #[test]
    fn native_arm_stores_are_immediately_visible_through_shared_aliases() {
        let memory = SharedMemory::anonymous(4096).unwrap();
        let region = SharedRegion::new(memory.clone(), Arc::new(()));
        let mut cpu = UnicornCpu::new().unwrap();
        cpu.map_region(0x1000, 4096, Prot::ALL).unwrap();
        unsafe {
            cpu.map_shared_region(
                0x60000000,
                4096,
                Prot::READ | Prot::WRITE,
                region.clone(),
                0,
            )
            .unwrap();
            cpu.map_shared_region(0x60010000, 4096, Prot::READ, region.clone(), 0)
                .unwrap();
        }
        let code: Vec<_> = [0xe5810000u32, 0xe5932000, 0xe12fff1e]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &code).unwrap();
        cpu.add_code_hook(0x1100).unwrap();
        for (reg, value) in [
            (ArmReg::R0, 42),
            (ArmReg::R1, 0x60000000),
            (ArmReg::R3, 0x60010000),
            (ArmReg::Lr, 0x1100),
        ] {
            cpu.write_reg(reg, value).unwrap();
        }
        assert_eq!(
            cpu.run_until_hook(0x1000, 100).unwrap(),
            StopReason::Hook(0x1100)
        );
        assert_eq!(cpu.read_reg(ArmReg::R2).unwrap(), 42);
        let mut other = UnicornCpu::new().unwrap();
        unsafe {
            other
                .map_shared_region(0x60000000, 4096, Prot::READ | Prot::WRITE, region, 0)
                .unwrap();
        }
        assert_eq!(other.read_u32_le(0x60000000).unwrap(), 42);
        other.write_mem(0x60000000, &77u32.to_le_bytes()).unwrap();
        assert_eq!(cpu.read_u32_le(0x60010000).unwrap(), 77);
        cpu.write_reg(ArmReg::R1, 0x60010000).unwrap();
        assert!(cpu.run_until_hook(0x1000, 100).is_err());
        assert_eq!(other.read_u32_le(0x60000000).unwrap(), 77);
        cpu.unmap_region(0x60000000, 4096).unwrap();
        cpu.unmap_region(0x60010000, 4096).unwrap();
        assert!(cpu.read_mem(0x60000000, 1).is_err());
        assert_eq!(other.read_u32_le(0x60000000).unwrap(), 77);
    }
}

#[cfg(all(test, feature = "unicorn"))]
mod mode_tests {
    use crate::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};
    #[test]
    fn kernel_mode_changes_privilege_without_changing_the_application_stack_or_flags() {
        let mut cpu = UnicornCpu::new().unwrap();
        cpu.map_region(0x1000, 4096, Prot::ALL).unwrap();
        let code: Vec<_> = [0xe10f0000u32, 0xee101f10, 0xe12fff1e]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &code).unwrap();
        cpu.add_code_hook(0x1100).unwrap();
        cpu.write_reg(ArmReg::Sp, 0x45678000).unwrap();
        cpu.write_reg(ArmReg::Lr, 0x1100).unwrap();
        let flags = cpu.read_reg(ArmReg::Cpsr).unwrap() & !0x1f;
        assert!(cpu.set_kernel_mode(true).unwrap());
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x45678000);
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1100);
        assert_eq!(cpu.read_reg(ArmReg::Cpsr).unwrap(), flags | 0x1f);
        assert_eq!(
            cpu.run_until_hook(0x1000, 100).unwrap(),
            StopReason::Hook(0x1100)
        );
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap() & 31, 31);
        assert_ne!(cpu.read_reg(ArmReg::R1).unwrap(), 0);
        assert!(cpu.set_kernel_mode(false).unwrap());
        assert!(!cpu.set_kernel_mode(false).unwrap());
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x45678000);
        assert_eq!(cpu.read_reg(ArmReg::Cpsr).unwrap() & 31, 16);
        assert!(cpu.run_until_hook(0x1000, 100).is_err());
        assert!(!cpu.set_kernel_mode(true).unwrap());
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1100);
    }
}
