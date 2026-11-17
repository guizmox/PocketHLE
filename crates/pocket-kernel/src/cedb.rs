//! Guest-local CEDB contacts and open cursors.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    sync::Mutex,
};

const MAX_STORE_BYTES: u64 = 4 * 1024 * 1024;
static STORE_GATE: Mutex<()> = Mutex::new(());

pub type Properties = BTreeMap<u32, Vec<u16>>;

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Contacts {
    pub next_oid: u32,
    pub sort: u32,
    #[serde(default)]
    pub last_modified: u64,
    pub records: BTreeMap<u32, Properties>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Cursor {
    pub oid: Option<u32>,
    pub autoincrement: bool,
}

#[derive(Default)]
pub struct ContactStore {
    pub path: Option<PathBuf>,
    pub cursors: HashMap<u32, Cursor>,
    pub next_handle: u32,
    memory: Contacts,
}

impl ContactStore {
    fn load(&self) -> io::Result<Contacts> {
        let Some(path) = &self.path else {
            return Ok(self.memory.clone());
        };
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Contacts::default()),
            Err(error) => return Err(error),
        };
        let mut data = Vec::new();
        file.take(MAX_STORE_BYTES + 1).read_to_end(&mut data)?;
        if data.len() as u64 > MAX_STORE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CEDB contacts too large",
            ));
        }
        serde_json::from_slice(&data).map_err(io::Error::other)
    }

    pub fn snapshot(&self) -> io::Result<Contacts> {
        let _guard = STORE_GATE.lock().unwrap();
        self.load()
    }

    pub fn update<T>(
        &mut self,
        change: impl FnOnce(&mut Contacts) -> Option<T>,
    ) -> io::Result<Option<T>> {
        let _guard = STORE_GATE.lock().unwrap();
        let mut contacts = self.load()?;
        let Some(result) = change(&mut contacts) else {
            return Ok(None);
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        contacts.last_modified = 116_444_736_000_000_000 + (now.as_nanos() / 100) as u64;
        if let Some(path) = &self.path {
            let data = serde_json::to_vec(&contacts).map_err(io::Error::other)?;
            if data.len() as u64 > MAX_STORE_BYTES {
                return Err(io::Error::other("CEDB contacts too large"));
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let temp = path.with_extension(format!("tmp-{}", std::process::id()));
            let write = (|| {
                let mut file = fs::File::create(&temp)?;
                file.write_all(&data)?;
                file.sync_all()?;
                drop(file);
                fs::rename(&temp, path)
            })();
            if write.is_err() {
                let _ = fs::remove_file(temp);
            }
            write?;
        } else {
            self.memory = contacts;
        }
        Ok(Some(result))
    }
}

impl Contacts {
    pub fn ordered_oids(&self) -> Vec<u32> {
        let mut ids: Vec<_> = self.records.keys().copied().collect();
        if self.sort != 0 {
            ids.sort_by_cached_key(|oid| {
                self.records[oid]
                    .get(&self.sort)
                    .map(|value| String::from_utf16_lossy(value).to_lowercase())
                    .unwrap_or_default()
            });
        }
        ids
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ContactSnapshot {
    persistent: bool,
    next_handle: u32,
    cursors: HashMap<u32, Cursor>,
    contents: Option<Contacts>,
}
impl ContactStore {
    pub(crate) fn capture_checkpoint(&self) -> Result<ContactSnapshot, String> {
        let saved = ContactSnapshot {
            persistent: self.path.is_some(),
            next_handle: self.next_handle,
            cursors: self.cursors.clone(),
            contents: if self.path.is_none() || !self.cursors.is_empty() {
                Some(self.snapshot().map_err(|error| error.to_string())?)
            } else {
                None
            },
        };
        saved.validate()?;
        Ok(saved)
    }
}
impl ContactSnapshot {
    fn validate(&self) -> Result<(), String> {
        if self.next_handle > 0x00ff_ffff
            || self.cursors.len() > 64
            || self
                .cursors
                .keys()
                .any(|&handle| handle <= 0xe2000000 || handle - 0xe2000000 > self.next_handle)
            || !self.cursors.is_empty() && self.contents.is_none()
            || !self.persistent && self.contents.is_none()
        {
            return Err("invalid saved WinCE database cursors".into());
        }
        if let Some(contents) = &self.contents {
            if serde_json::to_vec(contents)
                .map_err(|error| error.to_string())?
                .len() as u64
                > MAX_STORE_BYTES
                || contents
                    .records
                    .keys()
                    .any(|&oid| oid <= 1 || oid > contents.next_oid)
            {
                return Err("invalid saved WinCE database contents".into());
            }
        }
        Ok(())
    }
    pub(crate) fn restore(
        self,
        store: &mut ContactStore,
        vfs: &mut crate::vfs::Vfs,
    ) -> Result<(), String> {
        self.validate()?;
        if self.persistent != store.path.is_some() {
            return Err("saved WinCE database storage differs from the current launch".into());
        }
        if let Some(contents) = self.contents {
            if let Some(path) = &store.path {
                let bytes = serde_json::to_vec(&contents).map_err(|error| error.to_string())?;
                vfs.queue_snapshot_metadata_file(path.clone(), bytes);
            } else {
                store.memory = contents;
            }
        }
        store.cursors = self.cursors;
        store.next_handle = self.next_handle;
        Ok(())
    }
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    fn open_cursor(store: &mut ContactStore) {
        store.next_handle = 1;
        store.cursors.insert(
            0xe2000001,
            Cursor {
                oid: Some(2),
                autoincrement: true,
            },
        );
    }
    fn put(store: &mut ContactStore, value: u16) {
        store
            .update(|contacts| {
                contacts.next_oid = 2;
                contacts
                    .records
                    .insert(2, BTreeMap::from([(0x4001001f, vec![value, 0])]));
                Some(())
            })
            .unwrap()
            .unwrap();
    }
    #[test]
    fn database_cursor_cold_restore_uses_current_path_and_defers_writes() {
        let source_dir = tempfile::tempdir().unwrap();
        let destination_dir = tempfile::tempdir().unwrap();
        let mut source = ContactStore {
            path: Some(source_dir.path().join("contacts.json")),
            ..Default::default()
        };
        put(&mut source, 65);
        open_cursor(&mut source);
        let saved = source.capture_checkpoint().unwrap();
        let mut destination = ContactStore {
            path: Some(destination_dir.path().join("contacts.json")),
            ..Default::default()
        };
        put(&mut destination, 66);
        let mut vfs = crate::vfs::Vfs::new();
        saved.restore(&mut destination, &mut vfs).unwrap();
        assert_eq!(
            destination.snapshot().unwrap().records[&2][&0x4001001f][0],
            66
        );
        assert!(destination.cursors[&0xe2000001].autoincrement);
        {
            let _rolled_back = vfs.install_snapshot_files().unwrap();
        }
        assert_eq!(
            destination.snapshot().unwrap().records[&2][&0x4001001f][0],
            66
        );
        source
            .capture_checkpoint()
            .unwrap()
            .restore(&mut destination, &mut vfs)
            .unwrap();
        vfs.install_snapshot_files().unwrap().commit();
        assert_eq!(
            destination.snapshot().unwrap().records[&2][&0x4001001f][0],
            65
        );
        assert_eq!(
            destination.cursors.remove(&0xe2000001).unwrap().oid,
            Some(2)
        );
        put(&mut destination, 67);
        assert_eq!(
            destination.snapshot().unwrap().records[&2][&0x4001001f][0],
            67
        );
        assert_eq!(source.snapshot().unwrap().records[&2][&0x4001001f][0], 65);
    }
    #[test]
    fn database_initial_file_creation_is_rolled_back_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("contacts.json");
        let mut source = ContactStore {
            path: Some(path.clone()),
            ..Default::default()
        };
        open_cursor(&mut source);
        let saved = source.capture_checkpoint().unwrap();
        let mut destination = ContactStore {
            path: Some(path.clone()),
            ..Default::default()
        };
        let mut vfs = crate::vfs::Vfs::new();
        saved.restore(&mut destination, &mut vfs).unwrap();
        assert!(!path.exists());
        {
            let _transaction = vfs.install_snapshot_files().unwrap();
            assert!(path.is_file());
        }
        assert!(!path.exists());
        source
            .capture_checkpoint()
            .unwrap()
            .restore(&mut destination, &mut vfs)
            .unwrap();
        vfs.install_snapshot_files().unwrap().commit();
        assert!(path.is_file());
    }
    #[test]
    fn database_without_live_cursor_does_not_rewind_closed_persistent_saves() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("contacts.json");
        let mut store = ContactStore {
            path: Some(path),
            ..Default::default()
        };
        put(&mut store, 65);
        let saved = store.capture_checkpoint().unwrap();
        put(&mut store, 66);
        let mut vfs = crate::vfs::Vfs::new();
        saved.restore(&mut store, &mut vfs).unwrap();
        vfs.install_snapshot_files().unwrap().commit();
        assert_eq!(store.snapshot().unwrap().records[&2][&0x4001001f][0], 66);
    }
}
