//! Guest memory state independent of Unicorn's native allocation addresses.

use super::*;
use crate::image_pages::ImagePageBudget;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const PAGE: u32 = 4096;
// ARMv5/v6 tiny pages are 1 KiB (qemu/target/arm/cpu-param.h).
// TLB fetch promotions use that granularity, independently of our 4 KiB
// allocation/image pages. Chix snapshots contain entries such as 0x11400.
const ARM_TLB_PAGE: u64 = 1024;
const ADDRESS_LIMIT: u64 = 1 << 32;

#[derive(Serialize, Deserialize)]
pub struct ArmMemorySnapshot {
    mapped: Vec<Mapping>,
    images: Vec<Image>,
    pending: Vec<(u32, Vec<u8>)>,
    virtual_tlb: Option<VirtualTlb>,
}

#[derive(Serialize, Deserialize)]
struct Mapping {
    base: u32,
    permissions: u32,
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Image {
    base: u32,
    permissions: u32,
    resident: bool,
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct VirtualTlb {
    regions: Vec<(u64, u64, bool)>,
    promoted_exec: Vec<u64>,
    protected: Vec<(u64, u64, u32)>,
}

#[derive(Debug)]
struct DeferredBytes(Vec<u8>);

impl crate::DeferredMemorySource for DeferredBytes {
    fn materialize(&self) -> Result<Vec<u8>, String> {
        Ok(self.0.clone())
    }
}

impl UnicornCpu {
    /// Capture only guest memory at a dispatch boundary under the device
    /// execution gate. Any shared mappings must first be supported by a
    /// checkpoint of the underlying WinCE section objects and their aliases.
    pub fn capture_arm_memory(&self) -> Result<ArmMemorySnapshot, CpuError> {
        self.capture_arm_memory_with_mappings(&[])
    }

    pub(crate) fn capture_arm_memory_with_mappings(
        &self,
        mappings: &[(u32, u32)],
    ) -> Result<ArmMemorySnapshot, CpuError> {
        if self.arch != Arch::Arm
            || self.shared_regions.len() != mappings.len()
            || mappings.iter().any(|&(base, size)| {
                self.shared_regions
                    .get(&base)
                    .is_none_or(|(mapped, _)| *mapped != size)
            })
        {
            return Err(CpuError::Unsupported(
                "snapshot of this guest address space",
            ));
        }

        let mut mapped = Vec::new();
        let mut regions = self.uc.mem_regions().map_err(memory_snapshot_error)?;
        regions.sort_unstable_by_key(|region| region.begin);
        for region in regions {
            let base = u32::try_from(region.begin).map_err(memory_snapshot_error)?;
            let size =
                usize::try_from(region.end - region.begin + 1).map_err(memory_snapshot_error)?;
            if self.shared_regions.contains_key(&base) {
                if self.shared_regions[&base].0 as usize != size {
                    return Err(memory_snapshot_error("inconsistent shared region"));
                }
                continue;
            }
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(size)
                .map_err(memory_snapshot_error)?;
            bytes.resize(size, 0);
            self.uc
                .mem_read(region.begin, &mut bytes)
                .map_err(memory_snapshot_error)?;
            mapped.push(Mapping {
                base,
                permissions: region.perms as u32,
                bytes,
            });
        }
        let images = self
            .images
            .borrow()
            .pages
            .iter()
            .map(|(&base, image)| Image {
                base,
                permissions: image.prot.bits(),
                resident: image.resident,
                bytes: image.bytes.clone(),
            })
            .collect();
        let pending = self
            .deferred_memory
            .borrow()
            .regions()
            .map(|region| {
                let bytes = region.source.materialize().map_err(memory_snapshot_error)?;
                if bytes.len() != region.size as usize {
                    return Err(memory_snapshot_error("deferred memory length mismatch"));
                }
                Ok((region.base, bytes))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let virtual_tlb = self.guest_map.as_ref().map(|map| {
            let map = map.borrow();
            let mut map = super::GuestMap {
                regions: map.regions.clone(),
                promoted_exec: map.promoted_exec.clone(),
                protected: map.protected.clone(),
            };
            for &(base, size) in mappings {
                map.remove(u64::from(base), u64::from(size));
            }
            VirtualTlb {
                regions: map.regions,
                promoted_exec: map.promoted_exec,
                protected: map
                    .protected
                    .into_iter()
                    .map(|(lo, hi, prot)| (lo, hi, prot.bits()))
                    .collect(),
            }
        });
        Ok(ArmMemorySnapshot {
            mapped,
            images,
            pending,
            virtual_tlb,
        })
    }

    /// Rebuild in an empty ARM backend. The kernel supplies the shared image
    /// budget it will use for future page-ins. Drop the backend on an error.
    /// HLE thunk hooks must be regenerated from the process loader state.
    pub fn restore_arm_memory(
        &mut self,
        snapshot: ArmMemorySnapshot,
        budget: Arc<dyn ImagePageBudget>,
    ) -> Result<(), CpuError> {
        if self.arch != Arch::Arm
            || !self.shared_regions.is_empty()
            || !self.images.borrow().pages.is_empty()
            || !self
                .uc
                .mem_regions()
                .map_err(memory_snapshot_error)?
                .is_empty()
            || self.guest_map.is_some() != snapshot.virtual_tlb.is_some()
        {
            return Err(CpuError::Unsupported(
                "snapshot needs a matching empty ARM backend",
            ));
        }
        snapshot.validate()?;

        for mapping in snapshot.mapped {
            let base = u64::from(mapping.base);
            self.uc
                .mem_map(
                    base,
                    mapping.bytes.len() as u64,
                    map_prot(permissions(mapping.permissions)?),
                )
                .map_err(memory_snapshot_error)?;
            self.uc
                .mem_write(base, &mapping.bytes)
                .map_err(memory_snapshot_error)?;
        }
        let mut images = self.images.borrow_mut();
        for image in snapshot.images {
            if image.resident && !budget.acquire() {
                return Err(CpuError::ImageOutOfMemory { va: image.base });
            }
            images.pages.insert(
                image.base,
                crate::image_pages::ImagePage {
                    bytes: image.bytes,
                    prot: permissions(image.permissions)?,
                    resident: image.resident,
                    budget: budget.clone(),
                },
            );
        }
        drop(images);
        let mut pending = self.deferred_memory.borrow_mut();
        for (base, bytes) in snapshot.pending {
            pending.insert(base, bytes.len() as u32, Arc::new(DeferredBytes(bytes)));
        }
        drop(pending);
        if let (Some(map), Some(saved)) = (&self.guest_map, snapshot.virtual_tlb) {
            let mut map = map.borrow_mut();
            map.regions = saved.regions;
            map.promoted_exec = saved.promoted_exec;
            map.protected = saved
                .protected
                .into_iter()
                .map(|(lo, hi, bits)| Ok((lo, hi, permissions(bits)?)))
                .collect::<Result<_, CpuError>>()?;
        }
        self.uc.ctl_flush_tlb().map_err(memory_snapshot_error)
    }
}

impl ArmMemorySnapshot {
    pub(super) fn validate(&self) -> Result<(), CpuError> {
        let mut previous_end = 0;
        for mapping in &self.mapped {
            permissions(mapping.permissions)?;
            let base = u64::from(mapping.base);
            let end = base + mapping.bytes.len() as u64;
            if base < previous_end || !valid_page_range(base, end) {
                return Err(memory_snapshot_error("invalid mapped range"));
            }
            previous_end = end;
        }
        let contains = |base: u32, length: usize| {
            let index = self.mapped.partition_point(|m| m.base <= base);
            index.checked_sub(1).is_some_and(|index| {
                let mapping = &self.mapped[index];
                u64::from(base - mapping.base) + length as u64 <= mapping.bytes.len() as u64
            })
        };

        let mut previous = None;
        for image in &self.images {
            permissions(image.permissions)?;
            if previous.is_some_and(|base| base >= image.base)
                || !image.base.is_multiple_of(PAGE)
                || image.resident != contains(image.base, PAGE as usize)
                || image.bytes.len() > PAGE as usize
                || (image.resident && !image.bytes.is_empty())
            {
                return Err(memory_snapshot_error("invalid image page"));
            }
            previous = Some(image.base);
        }

        let mut previous_end = 0;
        for (address, bytes) in &self.pending {
            let size = u32::try_from(bytes.len()).map_err(memory_snapshot_error)?;
            let base = u64::from(*address);
            let end = base + u64::from(size);
            if base < previous_end
                || !crate::deferred_memory::PendingMemory::valid_range(*address, size)
                || !contains(*address, bytes.len())
                || self.virtual_tlb.is_none()
            {
                return Err(memory_snapshot_error("invalid deferred range"));
            }
            previous_end = end;
        }
        if let Some(tlb) = &self.virtual_tlb {
            if tlb.regions.windows(2).any(|pair| pair[0].1 > pair[1].0)
                || tlb
                    .regions
                    .iter()
                    .any(|&(lo, hi, _)| !valid_page_range(lo, hi))
                || tlb.promoted_exec.windows(2).any(|pair| pair[0] >= pair[1])
                || tlb
                    .promoted_exec
                    .iter()
                    .any(|&base| base >= ADDRESS_LIMIT || base % ARM_TLB_PAGE != 0)
            {
                return Err(memory_snapshot_error("invalid virtual TLB mapping"));
            }
            for &(lo, hi, bits) in &tlb.protected {
                permissions(bits)?;
                if !valid_page_range(lo, hi) {
                    return Err(memory_snapshot_error("invalid virtual TLB protection"));
                }
            }
        }
        Ok(())
    }
}

fn valid_page_range(base: u64, end: u64) -> bool {
    end <= ADDRESS_LIMIT && end > base && base % u64::from(PAGE) == 0 && end % u64::from(PAGE) == 0
}

fn permissions(bits: u32) -> Result<Prot, CpuError> {
    Prot::from_bits(bits).ok_or_else(|| memory_snapshot_error("invalid memory permissions"))
}

fn memory_snapshot_error(error: impl std::fmt::Debug) -> CpuError {
    CpuError::Backend(format!("ARM memory snapshot: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_pages::tests::Budget;

    #[test]
    fn memory_snapshot_restores_executable_arm_tiny_pages() {
        let mut cpu = UnicornCpu::new().unwrap();
        cpu.map_region(0x1000, 2 * PAGE, Prot::ALL).unwrap();
        cpu.add_code_hook(0x2000).unwrap();
        cpu.write_reg(ArmReg::Lr, 0x2000).unwrap();
        for address in [0x1000, 0x1400, 0x1800, 0x1c00] {
            cpu.write_mem(address, &[7, 0, 0xa0, 0xe3, 0x1e, 0xff, 0x2f, 0xe1])
                .unwrap(); // mov r0, #7; bx lr
            assert_eq!(
                cpu.run_until_hook(address, 100).unwrap(),
                StopReason::Hook(0x2000)
            );
        }
        let saved = cpu.capture_arm_memory().unwrap();
        let tlb = saved
            .virtual_tlb
            .as_ref()
            .expect("ARM virtual TLB required");
        assert!(tlb.promoted_exec.contains(&0x1400));
        assert!(tlb.promoted_exec.contains(&0x1800));
        assert!(tlb.promoted_exec.contains(&0x1c00));
        saved.validate().unwrap();
        let bytes = serde_json::to_vec(&saved).unwrap();
        drop(cpu);
        let saved = serde_json::from_slice(&bytes).unwrap();
        let mut restored = UnicornCpu::new().unwrap();
        restored.restore_arm_memory(saved, Budget::new(0)).unwrap();
        restored.add_code_hook(0x2000).unwrap();
        restored.write_reg(ArmReg::Lr, 0x2000).unwrap();
        for address in [0x1000, 0x1400, 0x1800, 0x1c00] {
            restored.write_reg(ArmReg::R0, 0).unwrap();
            assert_eq!(
                restored.run_until_hook(address, 100).unwrap(),
                StopReason::Hook(0x2000)
            );
            assert_eq!(restored.read_reg(ArmReg::R0).unwrap(), 7);
        }
        let mut invalid = restored.capture_arm_memory().unwrap();
        invalid.virtual_tlb.as_mut().unwrap().promoted_exec = vec![0x1401];
        assert!(invalid.validate().is_err());
        invalid.virtual_tlb.as_mut().unwrap().promoted_exec = vec![0x1400, 0x1400];
        assert!(invalid.validate().is_err());
        invalid.virtual_tlb.as_mut().unwrap().promoted_exec = vec![ADDRESS_LIMIT];
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn memory_snapshot_restores_deferred_rgb565_frame_without_padding() {
        const BASE: u32 = 0x80000;
        const SIZE: u32 = 320 * 240 * 2;
        let mut source = UnicornCpu::new().unwrap();
        source
            .map_region(BASE, SIZE.div_ceil(PAGE) * PAGE, Prot::READ | Prot::WRITE)
            .unwrap();
        source.write_mem(BASE + SIZE, &[0x5c]).unwrap();
        let deferred = source
            .defer_memory(
                BASE,
                SIZE,
                Arc::new(DeferredBytes(vec![0xa7; SIZE as usize])),
            )
            .unwrap();
        assert!(
            deferred,
            "ARM virtual TLB should support deferring an RGB565 frame"
        );
        let snapshot = source.capture_arm_memory().unwrap();

        let mut restored = UnicornCpu::new().unwrap();
        restored
            .restore_arm_memory(snapshot, Budget::new(0))
            .unwrap();
        assert_eq!(restored.deferred_memory_range(BASE), Some((BASE, SIZE)));
        assert_eq!(restored.read_u8(BASE + SIZE).unwrap(), 0x5c);
        assert_eq!(restored.read_u8(BASE + SIZE - 1).unwrap(), 0xa7);
        assert!(!restored.deferred_memory_pending(BASE));
    }

    #[test]
    fn memory_snapshot_preserves_permissions_lazy_images_and_deferred_bytes() {
        let mut cpu = UnicornCpu::new().unwrap();
        let source_budget = Budget::new(2);
        cpu.map_image_region(
            0x1000,
            2 * PAGE,
            Prot::READ | Prot::EXEC,
            vec![5; PAGE as usize + 1],
            source_budget.clone(),
        )
        .unwrap();
        assert_eq!(cpu.read_u8(0x1000).unwrap(), 5);
        assert_eq!(source_budget.count(), 1);

        cpu.map_region(0x4000, 2 * PAGE, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.write_mem(0x4000, &[0x42]).unwrap();
        cpu.protect_region(0x4000, PAGE, Prot::READ).unwrap();
        let deferred = cpu
            .defer_memory(
                0x5000,
                PAGE,
                Arc::new(DeferredBytes(vec![16; PAGE as usize])),
            )
            .unwrap();
        let snapshot = cpu.capture_arm_memory().unwrap();
        assert_eq!(source_budget.count(), 1);
        assert_eq!(cpu.deferred_memory_pending(0x5000), deferred);
        drop(cpu);

        let mut cpu = UnicornCpu::new().unwrap();
        let budget = Budget::new(2);
        cpu.restore_arm_memory(snapshot, budget.clone()).unwrap();
        assert_eq!(budget.count(), 1);
        assert_eq!(cpu.read_u8(0x4000).unwrap(), 0x42);
        assert!(cpu.check_guest_access(0x4000, 1, Prot::WRITE).is_err());
        assert_eq!(cpu.read_u8(0x2000).unwrap(), 5);
        assert_eq!(budget.count(), 2);
        assert_eq!(cpu.deferred_memory_pending(0x5000), deferred);
        if deferred {
            assert_eq!(cpu.read_u8(0x5000).unwrap(), 16);
            assert!(!cpu.deferred_memory_pending(0x5000));
        }
    }
}
