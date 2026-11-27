//! The WinCE allocator's host-side bookkeeping. Guest memory is restored by
//! the CPU backend, including live payloads and any guest-written headers.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct HeapSnapshot {
    base: u32,
    size: u32,
    regions: Vec<(u32, u32)>,
    free: Vec<(u32, u32)>,
    free_may_touch: bool,
    live: HashMap<u32, LiveAllocation>,
    page_refs: HashMap<u32, u32>,
    resident: Vec<(u32, u32)>,
    virtual_blocks: HashMap<u32, VirtualBlock>,
}

impl Heap {
    pub fn capture_snapshot(&self) -> HeapSnapshot {
        HeapSnapshot {
            base: self.base,
            size: self.size,
            regions: self.regions.clone(),
            free: self.free.clone(),
            free_may_touch: self.free_may_touch,
            live: self.live.clone(),
            page_refs: self.page_refs.clone(),
            resident: self.resident.clone(),
            virtual_blocks: self.virtual_blocks.clone(),
        }
    }
}

impl HeapSnapshot {
    pub(crate) fn owns_global_block(&self, address: u32) -> bool {
        self.live
            .get(&address)
            .is_some_and(|allocation| !allocation.cpp)
    }
    pub(crate) fn has_allocation(&self, address: u32, size: u32) -> bool {
        self.live
            .get(&address)
            .is_some_and(|allocation| allocation.size == size && !allocation.cpp)
    }
    /// Restore before CPU image pages so both consumers charge the same
    /// physical device. A failed charge leaves the device unchanged.
    pub fn restore(
        self,
        ram: Option<memory_division::MemoryDivision>,
    ) -> Result<Heap, KernelError> {
        self.validate()?;
        let mut heap = Heap {
            base: self.base,
            size: self.size,
            regions: self.regions,
            free: self.free,
            free_may_touch: self.free_may_touch,
            live: self.live,
            ram: None,
            image_memory: Default::default(),
            page_refs: self.page_refs,
            resident: self.resident,
            virtual_blocks: self.virtual_blocks,
        };
        if !heap.attach_ram(ram) {
            return Err(KernelError::Loader(
                "insufficient RAM for saved heap".into(),
            ));
        }
        Ok(heap)
    }

    fn validate(&self) -> Result<(), KernelError> {
        let invalid = || KernelError::Loader("invalid saved heap layout".into());
        if self.regions.first() != Some(&(self.base, self.size)) {
            return Err(invalid());
        }
        let mut regions = self.regions.clone();
        regions.sort_unstable();
        if !disjoint(&regions) {
            return Err(invalid());
        }

        let mut arenas: Vec<(u32, u32)> = Vec::new();
        for (start, size) in regions {
            if let Some((base, end)) = arenas.last_mut() {
                debug_assert!(*base <= *end);
                if *end == start {
                    *end = start + size;
                    continue;
                }
            }
            arenas.push((start, start + size));
        }
        let within = |start: u32, size: u32| {
            let index = arenas.partition_point(|region| region.0 <= start);
            index.checked_sub(1).is_some_and(|index| {
                start
                    .checked_add(size)
                    .is_some_and(|end| end <= arenas[index].1)
            })
        };
        if self.free.windows(2).any(|pair| pair[0].0 >= pair[1].0)
            || self.free.iter().any(|&(start, size)| !within(start, size))
        {
            return Err(invalid());
        }

        // A virtual reservation and a heap allocation cannot share bytes,
        // even when several allocations legitimately share a physical page.
        let mut occupied = self.free.clone();
        for (&address, allocation) in &self.live {
            let start = address.checked_sub(HEAP_HEADER_BYTES).ok_or_else(invalid)?;
            let size = allocation
                .size
                .max(1)
                .checked_add(HEAP_ALIGN - 1)
                .ok_or_else(invalid)?
                & !(HEAP_ALIGN - 1);
            let size = size.checked_add(HEAP_HEADER_BYTES).ok_or_else(invalid)?;
            if !within(start, size) {
                return Err(invalid());
            }
            occupied.push((start, size));
        }
        for (&base, block) in &self.virtual_blocks {
            let end = base.checked_add(block.size).ok_or_else(invalid)?;
            let pages = base / 4096..end / 4096;
            if base % 4096 != 0
                || block.size == 0
                || block.size % 4096 != 0
                || !block.committed.iter().all(|page| pages.contains(page))
                || !block
                    .protection
                    .keys()
                    .all(|page| block.committed.contains(page))
            {
                return Err(invalid());
            }
            if let Some(backing) = &block.backing {
                if base < HOST_VIRTUAL_BASE
                    || end > HOST_VIRTUAL_END
                    || backing.len() != block.committed.len()
                    || !backing.iter().all(|(&page, &backing)| {
                        block.committed.contains(&page)
                            && backing
                                .checked_mul(4096)
                                .is_some_and(|address| within(address, 4096))
                    })
                {
                    return Err(invalid());
                }
                occupied.extend(backing.values().map(|&page| (page * 4096, 4096)));
            } else if !within(base, block.size) {
                return Err(invalid());
            }
            occupied.push((base, block.size));
        }
        occupied.sort_unstable();
        if !disjoint(&occupied)
            || self
                .page_refs
                .iter()
                .any(|(&page, &count)| page > u32::MAX / 4096 || count == 0)
            || self
                .resident
                .iter()
                .any(|&(start, size)| size != 0 && Heap::page_range(start, size).is_none())
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn disjoint(ranges: &[(u32, u32)]) -> bool {
    let mut end = 0;
    for &(start, size) in ranges {
        if start < end || size == 0 {
            return false;
        }
        let Some(next) = start.checked_add(size) else {
            return false;
        };
        end = next;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_snapshot_restores_allocation_order_and_ram_accounting() {
        let ram = memory_division::MemoryDivision::new(32 * 4096, 8).unwrap();
        let mut heap = Heap::new(0x0010_0000, 0x0004_0000);
        assert!(heap.attach_ram(Some(ram.clone())));
        let allocation = heap.alloc_cpp(5000).unwrap();
        let next = heap.alloc(700).unwrap();
        let block = heap.virtual_alloc(0, 4096, true, true).unwrap();
        heap.free(next);
        let charged = ram.snapshot().program_used;
        let data = serde_json::to_vec(&heap.capture_snapshot()).unwrap();
        drop(heap);
        assert_eq!(ram.snapshot().program_used, 0);

        let decoded: HeapSnapshot = serde_json::from_slice(&data).unwrap();
        let mut restored = decoded.restore(Some(ram.clone())).unwrap();
        assert_eq!(ram.snapshot().program_used, charged);
        assert_eq!(restored.msize(allocation), Some(5000));
        restored.free_crt(allocation);
        assert_eq!(restored.msize(allocation), Some(5000));
        assert_eq!(restored.alloc(700), Some(next));
        assert!(restored.virtual_free(block, 0, true));
        restored.free(allocation);
        restored.free(next);
        assert_eq!(ram.snapshot().program_used, 0);
    }

    #[test]
    fn failed_heap_restore_leaves_the_device_ram_unchanged() {
        let mut heap = Heap::new(0x0010_0000, 0x0004_0000);
        heap.alloc(12000).unwrap();
        let ram = memory_division::MemoryDivision::new(10 * 4096, 8).unwrap();
        assert!(heap.capture_snapshot().restore(Some(ram.clone())).is_err());
        assert_eq!(ram.snapshot().program_used, 0);
    }

    #[test]
    fn heap_snapshot_restores_independent_virtual_backing() {
        let mut heap = Heap::new(0x0010_0000, 0x0004_0000);
        let free = heap.free_bytes();
        let address = heap.virtual_alloc(0, 8192, true, true).unwrap();
        assert!(heap.external_virtual_block(address));
        let mut restored = heap.capture_snapshot().restore(None).unwrap();
        assert_eq!(restored.free_bytes(), free - 8192);
        assert!(restored.virtual_free(address, 0, true));
        assert_eq!(restored.free_bytes(), free);
    }
}
