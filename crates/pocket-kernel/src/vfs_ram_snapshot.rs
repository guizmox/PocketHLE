//! RAM object-store identity, data and process-owned open descriptions.
use super::*;

#[derive(Serialize, Deserialize)]
pub(super) struct RamSnapshot {
    files: Vec<SavedRamFile>,
    names: HashMap<String, usize>,
    dirs: Vec<String>,
    descriptions: Vec<SavedRamOpen>,
}

#[derive(Serialize, Deserialize)]
struct SavedRamFile {
    identity: u64,
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
    name_bytes: u64,
}

#[derive(Serialize, Deserialize)]
struct SavedRamOpen {
    identity: u64,
    handles: Vec<u32>,
    file: usize,
    cursor: u64,
    access: u32,
    share: u32,
    lease_key: Option<String>,
    append: bool,
    text_mode: bool,
    for_mapping: bool,
}

impl Vfs {
    pub(super) fn capture_ram_snapshot(
        &self,
        private: &HashMap<u32, VfsObject>,
    ) -> Result<Option<RamSnapshot>, String> {
        let Some(ram) = &self.ram else {
            if !self.ram_handles.is_empty() {
                return Err("RAM files have no device memory division".into());
            }
            return Ok(None);
        };
        let store = ram.files.lock().map_err(|_| "RAM object store poisoned")?;
        let mut objects = Vec::new();
        let mut identities = HashMap::new();
        let mut register = |file: &Arc<Mutex<RamFile>>| {
            let identity = Arc::as_ptr(file);
            *identities.entry(identity).or_insert_with(|| {
                let id = objects.len();
                objects.push(file.clone());
                id
            })
        };
        let names = store
            .files
            .iter()
            .map(|(name, file)| (name.clone(), register(file)))
            .collect();
        let mut descriptions: HashMap<_, (Arc<Mutex<OpenRamFile>>, Vec<u32>)> = HashMap::new();
        let private_ram = private
            .iter()
            .filter_map(|(&id, object)| {
                if let VfsObjectInner::Ram(file) = &object.0 {
                    Some((id, file.clone()))
                } else {
                    None
                }
            })
            .collect::<HashMap<_, _>>();
        for (&handle, open) in self.ram_handles.iter().chain(&private_ram) {
            descriptions
                .entry(Arc::as_ptr(open))
                .or_insert_with(|| (open.clone(), Vec::new()))
                .1
                .push(handle);
        }
        let descriptions = descriptions
            .into_values()
            .map(|(open, mut handles)| {
                handles.sort_unstable();
                let identity = Arc::as_ptr(&open) as usize as u64;
                let open = open.lock().map_err(|_| "RAM file description poisoned")?;
                let access = match open.access {
                    Access::Read => 1,
                    Access::Write => 2,
                    Access::ReadWrite => 3,
                };
                let (access, share, leased) =
                    open.lease.as_ref().map_or((access, 0, false), |lease| {
                        (lease.access, lease.share, true)
                    });
                let lease_key = if leased {
                    self.shared
                        .0
                        .lock()
                        .map_err(|_| "VFS device lock poisoned")?
                        .leases
                        .iter()
                        .find(|(_, leases)| {
                            leases
                                .iter()
                                .filter_map(std::sync::Weak::upgrade)
                                .any(|lease| {
                                    open.lease
                                        .as_ref()
                                        .is_some_and(|current| Arc::ptr_eq(current, &lease))
                                })
                        })
                        .map(|(key, _)| key.clone())
                } else {
                    None
                };
                if leased && lease_key.is_none() {
                    return Err("RAM file lease has no namespace key".into());
                }
                Ok(SavedRamOpen {
                    identity,
                    handles,
                    file: register(&open.file),
                    cursor: open.position,
                    access,
                    share,
                    lease_key,
                    append: open.append,
                    text_mode: open.text_mode,
                    for_mapping: open.for_mapping,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        drop(register);
        let files = objects
            .into_iter()
            .map(|object| {
                let identity = Arc::as_ptr(&object) as usize as u64;
                let file = object.lock().map_err(|_| "RAM file poisoned")?;
                let mut bytes = vec![0; file.data.len()];
                if !file.data.read(0, &mut bytes) {
                    return Err("cannot read RAM snapshot file".into());
                }
                Ok(SavedRamFile {
                    identity,
                    bytes,
                    name_bytes: file.name_bytes,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some(RamSnapshot {
            files,
            names,
            dirs: store.dirs.keys().cloned().collect(),
            descriptions,
        }))
    }

    pub(super) fn restore_ram_snapshot(
        &mut self,
        saved: Option<RamSnapshot>,
    ) -> Result<(), String> {
        let Some(saved) = saved else {
            if self.ram.is_some() {
                return Err("missing RAM object store snapshot".into());
            }
            return Ok(());
        };
        let ram = self
            .ram
            .as_ref()
            .ok_or("saved RAM store requires a device memory division")?
            .clone();
        let valid_name = |name: &str| {
            self.ram_key(name)
                .is_some_and(|key| key == name && !key.is_empty())
        };
        if saved
            .names
            .iter()
            .any(|(name, &id)| !valid_name(name) || id >= saved.files.len())
            || saved.dirs.iter().any(|name| !valid_name(name))
        {
            return Err("invalid saved RAM object-store namespace".into());
        }

        let mut named = HashSet::new();
        for (name, &id) in &saved.names {
            let expected = (name.encode_utf16().count() as u64 + 1) * 2;
            if !named.insert(id) || saved.files[id].name_bytes != expected {
                return Err("invalid RAM file namespace charge".into());
            }
        }

        let first_store = self
            .snapshot_restore_domain
            .as_ref()
            .map(|registry| {
                registry
                    .lock()
                    .map_err(|_| "VFS restore domain poisoned")
                    .map(|registry| !registry.ram_store_loaded)
            })
            .transpose()?
            .unwrap_or(true);
        if first_store {
            let previous =
                std::mem::take(&mut *ram.files.lock().map_err(|_| "RAM object store poisoned")?);
            drop(previous);
        }
        let mut files = Vec::new();
        for file in saved.files {
            if file.identity == 0 {
                return Err("invalid RAM backing identity".into());
            }
            let restored = self
                .snapshot_restore_domain
                .as_ref()
                .map(|registry| {
                    registry
                        .lock()
                        .map_err(|_| "VFS restore domain poisoned")
                        .map(|registry| registry.ram_files.get(&file.identity).cloned())
                })
                .transpose()?
                .flatten();
            if let Some(backing) = restored {
                let saved = backing.lock().map_err(|_| "RAM file poisoned")?;
                let mut bytes = vec![0; saved.data.len()];
                if saved.name_bytes != file.name_bytes
                    || !saved.data.read(0, &mut bytes)
                    || bytes != file.bytes
                {
                    return Err("inconsistent shared RAM backing".into());
                }
                drop(saved);
                files.push(backing);
                continue;
            }
            let pages = crate::memory_division::pages(
                (file.bytes.len() as u64)
                    .checked_add(file.name_bytes)
                    .ok_or("invalid RAM file charge")?,
            );
            let charge = ram
                .store_charge(pages)
                .ok_or("insufficient object-store RAM for snapshot")?;
            let backing = Arc::new(Mutex::new(RamFile {
                data: RamData::Bytes(file.bytes),
                charge,
                name_bytes: file.name_bytes,
            }));
            if let Some(registry) = &self.snapshot_restore_domain {
                registry
                    .lock()
                    .map_err(|_| "VFS restore domain poisoned")?
                    .ram_files
                    .insert(file.identity, backing.clone());
            }
            files.push(backing);
        }
        let mut store = RamStore::default();
        for name in saved.dirs {
            if !first_store {
                if !ram
                    .files
                    .lock()
                    .map_err(|_| "RAM object store poisoned")?
                    .dirs
                    .contains_key(&name)
                {
                    return Err("shared RAM directory is missing".into());
                }
                continue;
            }
            let charge = ram
                .store_charge(crate::memory_division::pages(
                    (name.encode_utf16().count() as u64 + 1) * 2,
                ))
                .ok_or("insufficient object-store RAM for snapshot directory")?;
            if store.dirs.insert(name, charge).is_some() {
                return Err("duplicate saved RAM directory".into());
            }
        }
        for (name, id) in saved.names {
            if store.dirs.contains_key(&name) {
                return Err("RAM file conflicts with directory".into());
            }
            if first_store {
                store.files.insert(name, files[id].clone());
            } else if !ram
                .files
                .lock()
                .map_err(|_| "RAM object store poisoned")?
                .files
                .get(&name)
                .is_some_and(|file| Arc::ptr_eq(file, &files[id]))
            {
                return Err("shared RAM file name changed identity".into());
            }
        }
        let mut handles = HashMap::new();
        let mut leases = Vec::new();
        for record in saved.descriptions {
            if record.identity == 0
                || record.file >= files.len()
                || record.handles.is_empty()
                || record.access > 3
                || record.share > 3
            {
                return Err("invalid saved RAM file description".into());
            }
            let restored = self
                .snapshot_restore_domain
                .as_ref()
                .map(|registry| {
                    registry
                        .lock()
                        .map_err(|_| "VFS restore domain poisoned")
                        .map(|registry| registry.object(&SnapshotObjectId::Shared(record.identity)))
                })
                .transpose()?
                .flatten();
            if let Some(object) = restored {
                let VfsObjectInner::Ram(description) = object.0 else {
                    return Err("shared RAM description changed type".into());
                };
                let open = description
                    .lock()
                    .map_err(|_| "RAM file description poisoned")?;
                if !Arc::ptr_eq(&open.file, &files[record.file])
                    || open.position != record.cursor
                    || open.append != record.append
                    || open.text_mode != record.text_mode
                {
                    return Err("inconsistent shared RAM description".into());
                }
                drop(open);
                for handle in record.handles {
                    if handle < HANDLE_BASE
                        || handle == INVALID_HANDLE_VALUE
                        || handles.insert(handle, description.clone()).is_some()
                    {
                        return Err("invalid shared RAM handle".into());
                    }
                }
                continue;
            }
            let lease = record.lease_key.as_ref().map(|_| {
                Arc::new(Lease {
                    access: record.access,
                    share: record.share,
                    mapped: Default::default(),
                    volume: None,
                })
            });
            if let Some(lease) = &lease {
                let name = record
                    .lease_key
                    .clone()
                    .ok_or("missing RAM file lease key")?;
                if !name.starts_with("ram:") {
                    return Err("invalid RAM file lease key".into());
                }
                leases.push((name, lease.clone()));
            }
            let description = Arc::new(Mutex::new(OpenRamFile {
                file: files[record.file].clone(),
                position: record.cursor,
                access: match record.access {
                    2 => Access::Write,
                    3 => Access::ReadWrite,
                    _ => Access::Read,
                },
                lease,
                append: record.append,
                text_mode: record.text_mode,
                for_mapping: record.for_mapping,
            }));
            if let Some(registry) = &self.snapshot_restore_domain {
                registry
                    .lock()
                    .map_err(|_| "VFS restore domain poisoned")?
                    .objects
                    .insert(
                        SnapshotObjectId::Shared(record.identity),
                        VfsObject(VfsObjectInner::Ram(description.clone()), None),
                    );
            }
            for handle in record.handles {
                if handle < HANDLE_BASE
                    || handle == INVALID_HANDLE_VALUE
                    || handles.insert(handle, description.clone()).is_some()
                {
                    return Err("invalid saved RAM handle".into());
                }
            }
        }
        let mut shared = self
            .shared
            .0
            .lock()
            .map_err(|_| "VFS device lock poisoned")?;
        for (index, (key, lease)) in leases.iter().enumerate() {
            busy(&mut shared, key);
            let conflicts = |other: &Lease| {
                lease.access & !other.share != 0 || other.access & !lease.share != 0
            };
            if leases[..index]
                .iter()
                .any(|(other_key, other)| other_key == key && conflicts(other))
                || shared
                    .leases
                    .get(key)
                    .into_iter()
                    .flatten()
                    .filter_map(Weak::upgrade)
                    .any(|other| conflicts(&other))
            {
                return Err("saved RAM share contract conflicts with an open handle".into());
            }
        }
        for (key, lease) in leases {
            shared
                .leases
                .entry(key)
                .or_default()
                .push(Arc::downgrade(&lease));
        }
        if first_store {
            *ram.files.lock().map_err(|_| "RAM object store poisoned")? = store;
        }
        if let Some(registry) = &self.snapshot_restore_domain {
            registry
                .lock()
                .map_err(|_| "VFS restore domain poisoned")?
                .ram_store_loaded = true;
        }
        self.ram_handles = handles;
        Ok(())
    }
}
