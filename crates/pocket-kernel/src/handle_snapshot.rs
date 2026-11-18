//! Guest handles for a single WinCE process and its scheduler domain.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct HandleSnapshot {
    coordinated: bool,
    pid: u32,
    next: u32,
    next_pid: u32,
    next_tid: u32,
    threads: HashMap<usize, ThreadControl>,
    updates: HashMap<usize, ThreadUpdate>,
    entries: HashMap<u32, Option<HandleObject>>,
    names: HashMap<String, HandleObject>,
    process_image: Option<(String, u32)>,
    toolhelp: HashMap<u32, ToolhelpSnapshot>,
    exits: Vec<(Option<usize>, u32)>,
    file_objects: HashSet<HandleObject>,
}

impl HandleTable {
    /// Capture under the device execution gate. Capturing several guest
    /// processes requires a shared checkpoint of the whole device.
    pub fn capture_snapshot(&self) -> Result<HandleSnapshot, String> {
        self.capture_process_handles_with_mappings(false, false)
    }

    pub(crate) fn capture_process_handles_with_mappings(
        &self,
        mappings: bool,
        coordinated: bool,
    ) -> Result<HandleSnapshot, String> {
        let domain = self.domain.lock().map_err(|_| "handle domain poisoned")?;
        let pid = self.pid;
        if !coordinated
            && (domain.active.len() != 1
                || !domain.active.contains(&pid)
                || (!mappings && domain.mapping_refs.keys().any(|&(owner, _)| owner == pid))
                || domain.imports.keys().any(|&(owner, _)| owner == pid)
                || domain.held_start.contains(&pid)
                || domain.process_kills.contains_key(&pid)
                || domain.deferred_process_exits.contains(&pid))
        {
            return Err("handle snapshot contains a live child process or mapping".into());
        }
        let objects: HashSet<_> = domain
            .entries
            .iter()
            .filter(|&(&(owner, _), _)| owner == pid)
            .filter_map(|(_, &object)| object)
            .collect();
        let snapshot = HandleSnapshot {
            coordinated,
            pid,
            next: domain.next,
            next_pid: domain.next_pid,
            next_tid: domain.next_tid,
            threads: domain
                .threads
                .iter()
                .filter_map(|(&(owner, index), control)| {
                    (owner == pid).then_some((index, *control))
                })
                .collect(),
            updates: domain
                .updates
                .iter()
                .filter_map(|(&(owner, index), update)| (owner == pid).then_some((index, *update)))
                .collect(),
            entries: domain
                .entries
                .iter()
                .filter_map(|(&(owner, id), entry)| {
                    (owner == pid).then(|| {
                        (
                            id,
                            entry.map(|object| {
                                if coordinated {
                                    object
                                } else {
                                    self.local(object)
                                }
                            }),
                        )
                    })
                })
                .collect(),
            names: domain
                .names
                .iter()
                .filter(|(_, object)| objects.contains(object))
                .map(|(name, &object)| {
                    (
                        name.clone(),
                        if coordinated {
                            object
                        } else {
                            self.local(object)
                        },
                    )
                })
                .collect(),
            process_image: domain.process_images.get(&pid).cloned(),
            toolhelp: domain
                .toolhelp_snapshots
                .iter()
                .filter_map(|(&(owner, id), toolhelp)| {
                    (owner == pid).then(|| (id, toolhelp.clone()))
                })
                .collect(),
            exits: domain
                .exits
                .iter()
                .filter_map(|(&(owner, index), &exit)| (owner == pid).then_some((index, exit)))
                .collect(),
            file_objects: if coordinated {
                HashSet::new()
            } else {
                domain
                    .files
                    .keys()
                    .filter(|object| objects.contains(object))
                    .map(|&object| self.local(object))
                    .collect()
            },
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl HandleSnapshot {
    pub(crate) fn is_device_process(&self) -> bool {
        self.coordinated
    }
    pub fn process_id(&self) -> u32 {
        self.pid
    }
    pub(crate) fn owned_objects(&self) -> impl Iterator<Item = HandleObject> + '_ {
        self.entries.values().copied().flatten()
    }

    /// Restore into a new domain, with each exported file description linked
    /// back to the VFS object reconstructed at its original identifier.
    pub fn restore(
        self,
        vfs: &crate::vfs::Vfs,
        audio: &crate::audio::AudioEngine,
    ) -> Result<HandleTable, String> {
        self.validate()?;
        let mut domain = Domain {
            next: self.next,
            next_pid: self.next_pid,
            next_tid: self.next_tid,
            active: [self.pid].into(),
            threads: self
                .threads
                .into_iter()
                .map(|(index, control)| ((self.pid, index), control))
                .collect(),
            updates: self
                .updates
                .into_iter()
                .map(|(index, update)| ((self.pid, index), update))
                .collect(),
            process_images: self
                .process_image
                .into_iter()
                .map(|image| (self.pid, image))
                .collect(),
            toolhelp_snapshots: self
                .toolhelp
                .into_iter()
                .map(|(id, toolhelp)| ((self.pid, id), toolhelp))
                .collect(),
            exits: self
                .exits
                .into_iter()
                .map(|(index, exit)| ((self.pid, index), exit))
                .collect(),
            ..Default::default()
        };
        let table = HandleTable {
            pid: self.pid,
            domain: Default::default(),
        };
        for (id, object) in self.entries {
            let value = object.map(|object| {
                if self.coordinated {
                    object
                } else {
                    table.canonical(object, &domain)
                }
            });
            domain.entries.insert((self.pid, id), value);
        }
        for (name, object) in self.names {
            let value = if self.coordinated {
                object
            } else {
                table.canonical(object, &domain)
            };
            domain.names.insert(name, value);
        }
        for object in self.file_objects {
            let file_id = match object {
                HandleObject::File(id) | HandleObject::Device(id) => id,
                _ => return Err("invalid saved exported file handle".into()),
            };
            let mut file = vfs
                .export_handle(file_id)
                .ok_or("missing VFS file description")?;
            if vfs.is_mp3_decoder(file_id) {
                let key = vfs.mp3_stream_key(file_id);
                file = file.with_mas(key, audio.export_mas_stream(key));
            }
            let value = table.canonical(object, &domain);
            domain.files.insert(value, file);
        }
        *table.domain.lock().map_err(|_| "handle domain poisoned")? = domain;
        Ok(table)
    }

    fn validate(&self) -> Result<(), String> {
        let supported = |object: HandleObject| {
            self.coordinated
                || matches!(
                    object,
                    HandleObject::Thread(_)
                        | HandleObject::CurrentProcess
                        | HandleObject::Event(_)
                        | HandleObject::Semaphore(_)
                        | HandleObject::Mutex(_)
                        | HandleObject::File(_)
                        | HandleObject::Device(_)
                        | HandleObject::Mapping(_)
                )
        };
        if self.pid == 0
            || self.next_pid <= self.pid
            || self.next == 0
            || self
                .entries
                .values()
                .copied()
                .flatten()
                .any(|object| !supported(object))
            || self
                .names
                .values()
                .copied()
                .any(|object| !supported(object))
            || self
                .file_objects
                .iter()
                .any(|object| !matches!(object, HandleObject::File(_) | HandleObject::Device(_)))
        {
            return Err("unsupported saved handle object".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_snapshot_keeps_aliases_tombstones_and_named_objects() {
        let mut table = HandleTable::default();
        let object = HandleObject::Event(62);
        let a = table.duplicate(object).unwrap();
        let b = table.duplicate(object).unwrap();
        table.name("a test event".into(), object);
        assert_eq!(table.close(a), Some((object, false)));

        let bytes = serde_json::to_vec(&table.capture_snapshot().unwrap()).unwrap();
        let snapshot: HandleSnapshot = serde_json::from_slice(&bytes).unwrap();
        let mut restored = snapshot
            .restore(&crate::vfs::Vfs::new(), &crate::audio::AudioEngine::new())
            .unwrap();
        assert!(restored.is_closed(a));
        assert_eq!(restored.get(b), Some(object));
        assert_eq!(restored.named("a test event"), Some(object));
        assert_eq!(restored.reserve().unwrap(), b + 1);
        assert_eq!(restored.close(b), Some((object, true)));
        assert_eq!(restored.named("a test event"), None);
    }
}
