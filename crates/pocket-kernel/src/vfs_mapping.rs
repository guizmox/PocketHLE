//! Mapping-aware file growth, leases and RAM object-store aliases.
use super::*;
use pocket_cpu::shared_memory::SharedMemory;
use std::sync::{atomic::Ordering, Weak};

pub struct MappedFile {
    pub memory: Arc<SharedMemory>,
    _file: VfsObject,
    _pin: MappingPin,
}
struct MappingPin(Arc<contracts::Lease>);
impl Drop for MappingPin {
    fn drop(&mut self) {
        self.0.mapped.fetch_sub(1, Ordering::Relaxed);
    }
}
impl Vfs {
    pub fn mark_for_mapping(&mut self, handle: u32) {
        if let Some(f) = self.handles.get(&handle) {
            f.lock().unwrap().for_mapping = true;
        }
        if let Some(f) = self.ram_handles.get(&handle) {
            f.lock().unwrap().for_mapping = true;
        }
    }
    pub fn is_for_mapping(&self, handle: u32) -> bool {
        self.handles
            .get(&handle)
            .is_some_and(|f| f.lock().unwrap().for_mapping)
            || self
                .ram_handles
                .get(&handle)
                .is_some_and(|f| f.lock().unwrap().for_mapping)
    }
    pub(super) fn mapping_pinned(
        &self,
        lease: &Arc<contracts::Lease>,
        state: &contracts::Shared,
    ) -> bool {
        // Each open description has its own lease, while duplicates share one.
        // Find the canonical file group rather than checking only this handle.
        state.leases.values().any(|leases| {
            leases
                .iter()
                .filter_map(Weak::upgrade)
                .any(|l| Arc::ptr_eq(&l, lease))
                && leases
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|l| l.mapped.load(Ordering::Relaxed) != 0)
        })
    }
    /// Create stable backing without changing the shared file cursor. Growth
    /// reserves volume/object-store quota before exposing any view.
    pub fn mapping_backing(
        &mut self,
        handle: u32,
        len: u32,
        writable: bool,
    ) -> Result<MappedFile, u32> {
        if len == 0 {
            return Err(87);
        }
        let object = self.export_handle(handle).ok_or(6u32)?;
        let lease = self.lease(handle).ok_or(6u32)?;
        if lease.access & 1 == 0 || (writable && lease.access & 2 == 0) {
            return Err(5);
        }
        let shared = self.shared.clone();
        let mut state = shared.0.lock().unwrap();
        let old = self.size(handle).ok_or(6u32)?;
        let new_len = old.max(u64::from(len));
        if new_len != old && !self.growth_allowed(handle, new_len, old, &mut state) {
            return Err(if self.mapping_pinned(&lease, &state) {
                1224
            } else if lease.access & 2 == 0 {
                5
            } else {
                112
            });
        }
        let memory = if let Some(open) = self.handles.get(&handle) {
            let open = open.lock().unwrap();
            if new_len != old {
                open.file.set_len(new_len).map_err(|_| 112u32)?;
            }
            match SharedMemory::file(&open.file, len, writable) {
                Ok(memory) => memory,
                Err(_) => {
                    if new_len != old {
                        let _ = open.file.set_len(old);
                    }
                    return Err(8);
                }
            }
        } else if let Some(open) = self.ram_handles.get(&handle) {
            let open = open.lock().unwrap();
            let mut file = open.file.lock().unwrap();
            if new_len != old && !file.set_len(new_len as usize) {
                return Err(112);
            }
            if let RamData::Shared(memory) = &file.data {
                memory.clone()
            } else {
                let memory = match SharedMemory::anonymous(new_len as u32) {
                    Ok(m) => m,
                    Err(_) => {
                        if new_len != old {
                            file.set_len(old as usize);
                        }
                        return Err(8);
                    }
                };
                let RamData::Bytes(bytes) = &file.data else {
                    unreachable!()
                };
                unsafe {
                    memory.write(0, bytes).map_err(|_| 8u32)?;
                }
                file.data = RamData::Shared(memory.clone());
                memory
            }
        } else {
            return Err(6);
        };
        self.account_resize(handle, old, new_len, &mut state);
        lease.mapped.fetch_add(1, Ordering::Relaxed);
        Ok(MappedFile {
            memory,
            _file: object,
            _pin: MappingPin(lease),
        })
    }
}
