//! Pending byte ranges, populated before their first real CPU/HLE access.
#[cfg(feature = "unicorn")]
use crate::CpuError;
#[cfg(feature = "unicorn")]
use std::{collections::BTreeMap, sync::Arc};

/// Immutable bytes for a deferred memory publication. Must not access the CPU
/// recursively; failure leaves the publication pending and retryable.
pub trait DeferredMemorySource: std::fmt::Debug + Send + Sync {
    fn materialize(&self) -> Result<Vec<u8>, String>;
}
#[cfg(feature = "unicorn")]
#[derive(Clone)]
pub(crate) struct Region {
    pub base: u32,
    pub size: u32,
    pub source: Arc<dyn DeferredMemorySource>,
}
#[cfg(feature = "unicorn")]
#[derive(Default)]
pub(crate) struct PendingMemory {
    regions: BTreeMap<u32, Region>,
}
#[cfg(feature = "unicorn")]
impl PendingMemory {
    pub fn validate(&self, base: u32, size: u32) -> Result<(), CpuError> {
        let end = u64::from(base) + u64::from(size);
        if base % 4096 != 0 || size == 0 || size > 64 * 1024 * 1024 || end > 0x1_0000_0000 {
            return Err(CpuError::BadMemory { va: base, size });
        }
        if self
            .overlapping(base, size)
            .iter()
            .any(|r| r.base != base || r.size != size)
        {
            return Err(CpuError::BadMemory { va: base, size });
        }
        Ok(())
    }
    pub fn insert(&mut self, base: u32, size: u32, source: Arc<dyn DeferredMemorySource>) {
        self.regions.insert(base, Region { base, size, source });
    }
    pub fn overlapping(&self, base: u32, size: u32) -> Vec<Region> {
        let end = u64::from(base) + u64::from(size);
        if size == 0 {
            return Vec::new();
        }
        self.regions
            .values()
            .filter(|r| {
                u64::from(r.base) < end && u64::from(r.base) + u64::from(r.size) > u64::from(base)
            })
            .cloned()
            .collect()
    }
    pub fn range(&self, address: u32) -> Option<(u32, u32)> {
        self.regions
            .range(..=address)
            .next_back()
            .filter(|(_, r)| u64::from(address) < u64::from(r.base) + u64::from(r.size))
            .map(|(_, r)| (r.base, r.size))
    }
    pub fn pending(&self, address: u32) -> bool {
        self.regions
            .range(..=address)
            .next_back()
            .is_some_and(|(_, r)| u64::from(address) < u64::from(r.base) + u64::from(r.size))
    }
    pub fn remove(&mut self, base: u32) {
        self.regions.remove(&base);
    }
    pub fn discard(&mut self, base: u32, size: u32) -> Result<(), CpuError> {
        let regions = self.overlapping(base, size);
        if regions.iter().any(|r| {
            u64::from(r.base) < u64::from(base)
                || u64::from(r.base) + u64::from(r.size) > u64::from(base) + u64::from(size)
        }) {
            return Err(CpuError::BadMemory { va: base, size });
        }
        for r in regions {
            self.remove(r.base);
        }
        Ok(())
    }
}
