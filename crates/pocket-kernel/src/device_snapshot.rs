//! Common WinCE objects for a coherent checkpoint of several stopped processes.
use super::*;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Serialize, Deserialize)]
pub struct DeviceSnapshot {
    handles: handles::DeviceHandleSnapshot,
    mas: std::collections::BTreeMap<u32, audio::MasSnapshot>,
    events: HashMap<u32, EventObject>,
    semaphores: HashMap<u32, SemaphoreObject>,
    mutexes: HashMap<u32, MutexObject>,
}
impl DeviceSnapshot {
    pub fn include_process_audio(
        &mut self,
        streams: std::collections::BTreeMap<u32, audio::MasSnapshot>,
    ) {
        self.mas.extend(streams);
    }
    pub fn shared_audio_keys(&self) -> std::collections::BTreeSet<u32> {
        self.mas.keys().copied().collect()
    }
    pub fn child_process_id(&self, parent: u32, handle: u32) -> Option<u32> {
        self.handles.child_process_id(parent, handle)
    }
    pub fn process_ids(&self) -> Vec<u32> {
        self.handles.process_ids()
    }
}
impl KernelState {
    /// All participants are parked before the owning process calls this.
    pub fn capture_device_objects(&self) -> Result<DeviceSnapshot, String> {
        let mut mas = std::collections::BTreeMap::new();
        for object in self.object_handles.checkpoint_backing_objects() {
            if let Some((key, playback)) = object.mas() {
                mas.entry(key).or_insert(playback.capture_checkpoint()?);
            }
        }
        Ok(DeviceSnapshot {
            mas,
            handles: self.object_handles.capture_device_handles()?,
            events: self.events.copy_objects(),
            semaphores: self.semaphores.copy_objects(),
            mutexes: self.mutexes.copy_objects(),
        })
    }
}

pub struct DeviceRestoreDomain {
    pub files: Arc<Mutex<vfs::SnapshotRestoreDomain>>,
    pub sections: Mutex<file_mapping::MappingRestoreDomain>,
    pub serial: Mutex<()>,
    pub mas: std::collections::BTreeMap<u32, audio::MasPlayback>,
    snapshot: Mutex<Option<handles::DeviceHandleSnapshot>>,
    table: handles::HandleTable,
    processes: Vec<u32>,
    memory: Option<memory_division::MemoryDivision>,
    vfs_shared: vfs::VfsShared,
    registry: Arc<Mutex<registry::RegistryStore>>,
    events: shared_objects::SharedObjects<EventObject>,
    semaphores: shared_objects::SharedObjects<SemaphoreObject>,
    mutexes: shared_objects::SharedObjects<MutexObject>,
}
impl DeviceRestoreDomain {
    pub fn new(snapshot: DeviceSnapshot, reference: &KernelState) -> Result<Self, String> {
        if snapshot
            .semaphores
            .values()
            .any(|object| object.count > object.max_count)
            || snapshot
                .mutexes
                .values()
                .any(|object| (object.owner.is_none()) != (object.depth == 0))
        {
            return Err("invalid saved shared synchronization object".into());
        }
        let processes = snapshot.process_ids();
        let table = snapshot.handles.restore_namespace()?;
        let mas = snapshot
            .mas
            .into_iter()
            .map(|(key, stream)| stream.restore().map(|stream| (key, stream)))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            mas,
            files: Arc::new(Mutex::new(vfs::SnapshotRestoreDomain::default())),
            sections: Mutex::default(),
            serial: Mutex::new(()),
            snapshot: Mutex::new(Some(snapshot.handles)),
            table,
            processes,
            memory: reference.memory_division.clone(),
            vfs_shared: reference.vfs.shared_context(),
            registry: reference.registry.store.clone(),
            events: shared_objects::SharedObjects::from_objects(snapshot.events),
            semaphores: shared_objects::SharedObjects::from_objects(snapshot.semaphores),
            mutexes: shared_objects::SharedObjects::from_objects(snapshot.mutexes),
        })
    }
    pub fn process_table(&self, pid: u32) -> Option<handles::HandleTable> {
        self.table.for_process(pid)
    }
    pub fn process_ids(&self) -> &[u32] {
        &self.processes
    }
    /// The caller has released its bootstrap CPU first. Reconstructed heaps and
    /// image pages are subsequently charged to this one common RAM division.
    pub fn prepare_process(&self, state: &mut KernelState) {
        state.memory_division = self.memory.clone();
        state.vfs.attach_ram(self.memory.clone());
        state.vfs.attach_shared_context(self.vfs_shared.clone());
        state
            .vfs
            .attach_snapshot_restore_domain(Some(self.files.clone()));
        state.registry.store = self.registry.clone();
    }
    pub fn finish_objects(&self) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .lock()
            .map_err(|_| "device restore domain poisoned")?
            .take()
            .ok_or("device objects already reconstructed")?;
        let mappings = self
            .sections
            .lock()
            .map_err(|_| "section restore domain poisoned")?
            .objects();
        let mut files = self
            .files
            .lock()
            .map_err(|_| "VFS restore domain poisoned")?;
        files.attach_mas_transports(&self.mas)?;
        snapshot.bind_backings(&self.table, &files, &mappings)
    }
    pub fn adopt_process(&self, pid: u32, state: &mut KernelState) -> Result<(), String> {
        let table = self
            .table
            .for_process(pid)
            .ok_or("saved process is missing from device")?;
        state.attach_handle_context(ProcessHandleContext {
            display_turns: state.async_keys.display_turns,
            table,
            vfs_shared: self.vfs_shared.clone(),
            registry_store: self.registry.clone(),
            events: self.events.clone(),
            semaphores: self.semaphores.clone(),
            mutexes: self.mutexes.clone(),
        });
        state.vfs.finish_snapshot_restore()?;
        Ok(())
    }
    pub fn release_reconstruction_references(&self) -> Result<(), String> {
        self.files
            .lock()
            .map_err(|_| "VFS restore domain poisoned")?
            .release_objects();
        self.sections
            .lock()
            .map_err(|_| "section restore domain poisoned")?
            .release_objects();
        Ok(())
    }
}
