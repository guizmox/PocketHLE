//! Serialized device-wide handle namespace; native backing objects are resolved
//! from the common VFS/section reconstruction caches after every CPU is loaded.
use super::*;
use crate::vfs::{SnapshotObjectId, SnapshotRestoreDomain};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct DeviceHandleSnapshot {
    next_tid: u32,
    threads: HashMap<(u32, usize), ThreadControl>,
    updates: HashMap<(u32, usize), ThreadUpdate>,
    process_kills: HashMap<u32, u32>,
    held_start: HashSet<u32>,
    entries: HashMap<(u32, u32), Option<HandleObject>>,
    names: HashMap<String, HandleObject>,
    next: u32,
    next_pid: u32,
    active: HashSet<u32>,
    process_images: HashMap<u32, (String, u32)>,
    toolhelp: HashMap<(u32, u32), ToolhelpSnapshot>,
    exits: HashMap<(u32, Option<usize>), u32>,
    deferred_process_exits: HashSet<u32>,
    child_ids: HashMap<u32, u32>,
    files: Vec<(HandleObject, SnapshotObjectId)>,
    imports: Vec<((u32, u32), SnapshotObjectId)>,
    mapping_refs: Vec<((u32, u32), u32)>,
    mapping_names: Vec<(String, u32)>,
}
impl HandleTable {
    pub fn checkpoint_process_ids(&self) -> Vec<u32> {
        let domain = self.domain.lock().unwrap_or_else(|e| e.into_inner());
        let mut ids: Vec<_> = domain.active.iter().copied().collect();
        ids.sort_unstable();
        ids
    }
    pub fn capture_device_handles(&self) -> Result<DeviceHandleSnapshot, String> {
        let domain = self.domain.lock().map_err(|_| "handle domain poisoned")?;
        let snapshot = DeviceHandleSnapshot {
            next_tid: domain.next_tid,
            threads: domain.threads.clone(),
            updates: domain.updates.clone(),
            process_kills: domain.process_kills.clone(),
            held_start: domain.held_start.clone(),
            entries: domain.entries.clone(),
            names: domain.names.clone(),
            next: domain.next,
            next_pid: domain.next_pid,
            active: domain.active.clone(),
            process_images: domain.process_images.clone(),
            toolhelp: domain.toolhelp_snapshots.clone(),
            exits: domain.exits.clone(),
            deferred_process_exits: domain.deferred_process_exits.clone(),
            child_ids: domain.child_ids.clone(),
            files: domain
                .files
                .iter()
                .map(|(&object, file)| (object, file.snapshot_identity()))
                .collect(),
            imports: domain
                .imports
                .iter()
                .map(|(&slot, file)| (slot, file.snapshot_identity()))
                .collect(),
            mapping_refs: domain
                .mapping_refs
                .iter()
                .map(|(&slot, mapping)| (slot, mapping.key))
                .collect(),
            mapping_names: domain
                .mapping_names
                .iter()
                .filter_map(|(name, object)| {
                    object.upgrade().map(|object| (name.clone(), object.key))
                })
                .collect(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    pub fn checkpoint_backing_objects(&self) -> Vec<VfsObject> {
        let domain = self.domain.lock().unwrap_or_else(|e| e.into_inner());
        domain
            .files
            .values()
            .chain(domain.imports.values())
            .cloned()
            .collect()
    }
    pub fn checkpoint_owned_backings(&self) -> Vec<VfsObject> {
        let domain = self.domain.lock().unwrap_or_else(|e| e.into_inner());
        let first = domain.active.iter().min().copied();
        domain
            .files
            .iter()
            .filter_map(|(object, file)| {
                let owner = match object {
                    HandleObject::RemoteFile(owner, _) | HandleObject::RemoteDevice(owner, _) => {
                        *owner
                    }
                    _ => return None,
                };
                (owner == self.pid || (!domain.active.contains(&owner) && first == Some(self.pid)))
                    .then(|| file.clone())
            })
            .collect()
    }
    pub fn for_process(&self, pid: u32) -> Option<Self> {
        self.domain
            .lock()
            .ok()?
            .active
            .contains(&pid)
            .then(|| Self::from_domain(self.domain.clone(), pid))
    }
}
impl DeviceHandleSnapshot {
    pub fn child_process_id(&self, parent: u32, handle: u32) -> Option<u32> {
        let child = match self.entries.get(&(parent, handle))? {
            Some(HandleObject::RemoteProcess(pid)) => *pid,
            Some(HandleObject::ChildProcess(key)) => *self.child_ids.get(key)?,
            None => *self.child_ids.get(&handle)?, // closed alias, live wait
            _ => return None,
        };
        self.active.contains(&child).then_some(child)
    }
    pub fn process_ids(&self) -> Vec<u32> {
        let mut ids: Vec<_> = self.active.iter().copied().collect();
        ids.sort_unstable();
        ids
    }
    fn validate(&self) -> Result<(), String> {
        if self.active.is_empty()
            || self.active.len() > 128
            || self.active.contains(&0)
            || self.next == 0
            || self.active.iter().any(|&pid| pid >= self.next_pid)
            || self.entries.keys().any(|&(pid, handle)| {
                pid == 0 || pid >= self.next_pid || handle == 0 || handle == u32::MAX
            })
            || self.threads.iter().any(|(&(pid, _), thread)| {
                pid == 0 || pid >= self.next_pid || thread.tid == 0 || thread.tid >= self.next_tid
            })
            || self.held_start.iter().any(|pid| !self.active.contains(pid))
            || self
                .process_kills
                .keys()
                .any(|pid| !self.active.contains(pid))
        {
            return Err("invalid saved device handle domain".into());
        }
        for &(slot, key) in &self.mapping_refs {
            if self.entries.get(&slot) != Some(&Some(HandleObject::Mapping(key))) {
                return Err("invalid shared section handle".into());
            }
        }
        Ok(())
    }
    pub fn restore_namespace(&self) -> Result<HandleTable, String> {
        self.validate()?;
        let pid = *self.active.iter().min().ok_or("empty saved device")?;
        let domain = Domain {
            next_tid: self.next_tid,
            threads: self.threads.clone(),
            updates: self.updates.clone(),
            process_kills: self.process_kills.clone(),
            held_start: self.held_start.clone(),
            entries: self.entries.clone(),
            names: self.names.clone(),
            next: self.next,
            next_pid: self.next_pid,
            active: self.active.clone(),
            process_images: self.process_images.clone(),
            toolhelp_snapshots: self.toolhelp.clone(),
            exits: self.exits.clone(),
            deferred_process_exits: self.deferred_process_exits.clone(),
            child_ids: self.child_ids.clone(),
            ..Default::default()
        };
        Ok(HandleTable::from_domain(Arc::new(Mutex::new(domain)), pid))
    }
    pub fn bind_backings(
        self,
        table: &HandleTable,
        files: &SnapshotRestoreDomain,
        mappings: &HashMap<u32, Arc<crate::file_mapping::FileMapping>>,
    ) -> Result<(), String> {
        let ready = self.restore(files, mappings)?;
        let mut source = ready
            .domain
            .lock()
            .map_err(|_| "restored handle domain poisoned")?;
        let mut target = table
            .domain
            .lock()
            .map_err(|_| "device handle domain poisoned")?;
        let gate = target.api_gate.clone();
        let file_changes = target.file_changes.wrapping_add(1);
        *target = std::mem::take(&mut *source);
        target.api_gate = gate;
        // Metadata may already have been observed by any process table clone.
        // Replacing its VFS backings invalidates all those local polling hints.
        target.file_changes = file_changes;
        Ok(())
    }
    pub fn restore(
        self,
        files: &SnapshotRestoreDomain,
        mappings: &HashMap<u32, Arc<crate::file_mapping::FileMapping>>,
    ) -> Result<HandleTable, String> {
        self.validate()?;
        let pid = *self.active.iter().min().ok_or("empty saved device")?;
        let mut domain = Domain {
            next_tid: self.next_tid,
            threads: self.threads,
            updates: self.updates,
            process_kills: self.process_kills,
            held_start: self.held_start,
            entries: self.entries,
            names: self.names,
            next: self.next,
            next_pid: self.next_pid,
            active: self.active,
            process_images: self.process_images,
            toolhelp_snapshots: self.toolhelp,
            exits: self.exits,
            deferred_process_exits: self.deferred_process_exits,
            child_ids: self.child_ids,
            ..Default::default()
        };
        for (object, identity) in self.files {
            if !matches!(
                object,
                HandleObject::RemoteFile(..) | HandleObject::RemoteDevice(..)
            ) || domain
                .files
                .insert(
                    object,
                    files
                        .object(&identity)
                        .ok_or("missing shared VFS backing")?,
                )
                .is_some()
            {
                return Err("invalid saved shared file object".into());
            }
        }
        for (slot, identity) in self.imports {
            if domain
                .imports
                .insert(
                    slot,
                    files
                        .object(&identity)
                        .ok_or("missing transferred VFS backing")?,
                )
                .is_some()
            {
                return Err("duplicate transferred handle".into());
            }
        }
        domain.mappings = mappings
            .iter()
            .map(|(&key, object)| (key, Arc::downgrade(object)))
            .collect();
        for (name, key) in self.mapping_names {
            let object = mappings.get(&key).ok_or("missing named shared section")?;
            if domain
                .mapping_names
                .insert(name, Arc::downgrade(object))
                .is_some()
            {
                return Err("duplicate shared section name".into());
            }
        }
        for (slot, key) in self.mapping_refs {
            let object = mappings
                .get(&key)
                .ok_or("missing transferred shared section")?;
            if domain.mapping_refs.insert(slot, object.clone()).is_some() {
                return Err("duplicate shared section handle".into());
            }
        }
        if domain.entries.iter().any(|(slot, object)| {
            matches!(object, Some(HandleObject::Mapping(_)))
                && !domain.mapping_refs.contains_key(slot)
        }) {
            return Err("shared section handle is missing its object".into());
        }
        Ok(HandleTable::from_domain(Arc::new(Mutex::new(domain)), pid))
    }
}
