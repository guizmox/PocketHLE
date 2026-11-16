//! Snapshot packing for CE section objects, views and their VFS leases.

use super::*;
use crate::vfs::OpenFile;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

#[derive(Serialize, Deserialize)]
pub struct MappingSnapshot {
    sections: Vec<SavedSection>,
    views: Vec<SavedView>,
    handles: Vec<(u32, u32)>,
    names: Vec<(String, u32)>,
    section_files: HashMap<u32, u32>,
    private_files: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
struct SavedSection {
    key: u32,
    size: u32,
    writable: bool,
    file: Option<u32>,
    bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct SavedView {
    base: u32,
    key: u32,
    offset: u32,
    len: u32,
    prot: u32,
}

/// Validated data to apply after the rest of the session has been restored.
#[derive(Default)]
pub struct MappingContents(Vec<(Arc<FileMapping>, Vec<u8>)>);

impl MappingContents {
    pub fn install(self) -> Result<(), String> {
        for (object, bytes) in self.0 {
            // The new CPU has not started. Writable bounds were checked when
            // the section was rebuilt.
            unsafe { object.memory.write(0, &bytes) }.map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

impl MappedViews {
    pub fn snapshot_ranges(&self) -> Vec<(u32, u32)> {
        self.views
            .iter()
            .map(|(&base, view)| (base, view.mapped_len))
            .collect()
    }

    pub(crate) fn capture_snapshot(
        &self,
        vfs: &crate::vfs::Vfs,
        table: &crate::handles::HandleTable,
        private: &mut HashMap<u32, Arc<Mutex<OpenFile>>>,
    ) -> Result<Option<MappingSnapshot>, String> {
        let (bindings, names) = table.snapshot_mapping_bindings()?;
        if bindings.is_empty() && self.views.is_empty() {
            return Ok(None);
        }
        let mut objects = BTreeMap::new();
        for object in bindings
            .iter()
            .map(|(_, object)| object)
            .chain(self.views.values().map(|view| &view.object))
        {
            objects.entry(object.key).or_insert(object);
        }
        let mut budget = 256 * 1024 * 1024usize;
        let mut sections = Vec::with_capacity(objects.len());
        for (&key, object) in &objects {
            let file = object
                ._file
                .as_ref()
                .map(|file| vfs.snapshot_mapping_file(file, private))
                .transpose()?;
            // Also retain immutable backing bytes for exact verification.
            budget = budget
                .checked_sub(object.size as usize)
                .ok_or("mapped snapshot data exceeds 256 MiB")?;
            let mut bytes = vec![0; object.size as usize];
            // Capture runs at a serialized guest checkpoint.
            unsafe { object.memory.read(0, &mut bytes) }.map_err(|error| error.to_string())?;
            sections.push(SavedSection {
                key,
                size: object.size,
                writable: object.writable,
                file,
                bytes,
            });
        }
        Ok(Some(MappingSnapshot {
            sections,
            views: self
                .views
                .iter()
                .map(|(&base, view)| SavedView {
                    base,
                    key: view.object.key,
                    offset: view.offset,
                    len: view.len,
                    prot: view.prot.bits(),
                })
                .collect(),
            handles: bindings
                .into_iter()
                .map(|(id, object)| (id, object.key))
                .collect(),
            names,
            section_files: vfs.snapshot_section_files(),
            private_files: private.keys().copied().collect(),
        }))
    }
}

impl MappingSnapshot {
    pub(crate) fn file_handles(&self) -> HashSet<u32> {
        self.sections
            .iter()
            .filter_map(|section| section.file)
            .collect()
    }

    pub fn restore(
        self,
        state: &mut crate::KernelState,
        cpu: &mut dyn Cpu,
    ) -> Result<MappingContents, String> {
        let private_backings = self.file_handles();
        let mut objects = HashMap::new();
        let mut contents = MappingContents::default();
        for section in self.sections {
            if section.size == 0
                || section.bytes.len() != section.size as usize
                || objects.contains_key(&section.key)
            {
                return Err("invalid saved section backing".into());
            }
            let object = if let Some(file) = section.file {
                if state
                    .vfs
                    .size(file)
                    .is_none_or(|len| len < u64::from(section.size))
                {
                    return Err("saved mapping backing is truncated".into());
                }
                let backing = state
                    .vfs
                    .mapping_backing(file, section.size, section.writable)
                    .map_err(|error| {
                        format!("cannot reopen mapping backing: WinCE error {error}")
                    })?;
                FileMapping::file(section.key, section.size, section.writable, backing)
            } else {
                if !section.writable {
                    return Err("invalid anonymous section protection".into());
                }
                FileMapping::anonymous(section.key, section.size, state.memory_division.as_ref())
                    .map_err(|error| {
                        format!("cannot recreate anonymous section: WinCE error {error}")
                    })?
            };
            if section.writable {
                contents.0.push((object.clone(), section.bytes));
            } else {
                let mut bytes = vec![0; section.size as usize];
                unsafe { object.memory.read(0, &mut bytes) }.map_err(|error| error.to_string())?;
                if bytes != section.bytes {
                    return Err("read-only mapped file changed since checkpoint".into());
                }
            }
            objects.insert(section.key, object);
        }
        for view in self.views {
            let object = objects
                .get(&view.key)
                .ok_or("missing saved mapped view backing")?;
            let mapped_len = rounded_len(view.len).ok_or("invalid saved mapped view length")?;
            let prot = Prot::from_bits(view.prot).ok_or("invalid saved mapped view protection")?;
            if !matches!(prot, Prot::READ) && prot != (Prot::READ | Prot::WRITE)
                || prot.contains(Prot::WRITE) && !object.writable
                || view.base < VIEW_BASE
                || view.base % 65536 != 0
                || view
                    .base
                    .checked_add(mapped_len)
                    .is_none_or(|end| end > VIEW_END)
                || view.offset % 4096 != 0
                || view
                    .offset
                    .checked_add(view.len)
                    .is_none_or(|end| end > object.size)
            {
                return Err("invalid saved mapped view".into());
            }
            let region = SharedRegion::new(object.memory.clone(), object.clone());
            // The CPU is stopped and retains the same section as the view.
            unsafe { cpu.map_shared_region(view.base, mapped_len, prot, region, view.offset) }
                .map_err(|error| error.to_string())?;
            state.mapped_views.views.insert(
                view.base,
                View {
                    object: object.clone(),
                    offset: view.offset,
                    len: view.len,
                    mapped_len,
                    prot,
                },
            );
        }
        if self.section_files.iter().any(|(&section, &file)| {
            !self.handles.iter().any(|&(handle, _)| handle == section)
                || !state.vfs.is_for_mapping(file)
        }) {
            return Err("invalid saved section-owned file".into());
        }
        state
            .object_handles
            .restore_mapping_bindings(&objects, &self.handles, self.names)?;
        let mut private = HashSet::new();
        for file in self.private_files {
            if self.section_files.values().any(|&id| id == file)
                || !private.insert(file)
                || !private_backings.contains(&file)
                || !state.vfs.close(file)
            {
                return Err("invalid private snapshot file".into());
            }
        }
        state.vfs.restore_section_files(self.section_files);
        Ok(contents)
    }
}
