//! Typed CEDB volumes for CE Explorer's recycle database. Contacts stay separate.
//! All volume I/O goes through the shared guest VFS, including private overlays.
use super::STORE_GATE;
use crate::vfs::{SeekKind, Vfs};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
};

pub const MAX_VOLUME_BYTES: usize = 4 * 1024 * 1024;
pub type Guid = [u8; 16];
pub type Properties = BTreeMap<u32, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    FileTime(u64),
    Text(Vec<u16>),
    Blob(Vec<u8>),
    Bool(u32),
    Double(u64),
}
impl Value {
    pub fn compare(&self, other: &Self, flags: u32) -> Ordering {
        match (self, other) {
            (Self::I16(a), Self::I16(b)) => a.cmp(b),
            (Self::U16(a), Self::U16(b)) => a.cmp(b),
            (Self::I32(a), Self::I32(b)) => a.cmp(b),
            (Self::U32(a), Self::U32(b)) | (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::FileTime(a), Self::FileTime(b)) => a.cmp(b),
            (Self::Double(a), Self::Double(b)) => f64::from_bits(*a).total_cmp(&f64::from_bits(*b)),
            (Self::Blob(a), Self::Blob(b)) => a.cmp(b),
            (Self::Text(a), Self::Text(b)) => {
                let (a, b) = (String::from_utf16_lossy(a), String::from_utf16_lossy(b));
                if flags & 2 != 0 {
                    a.to_lowercase().cmp(&b.to_lowercase())
                } else {
                    a.cmp(&b)
                }
            }
            _ => Ordering::Equal,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub key_flags: u16,
    pub properties: Vec<(u32, u32)>,
}
impl Sort {
    pub fn compare(&self, a: &Properties, b: &Properties) -> Ordering {
        for (id, flags) in &self.properties {
            let ordering = match (a.get(id), b.get(id)) {
                (Some(a), Some(b)) => {
                    let result = a.compare(b, *flags);
                    if flags & 1 != 0 {
                        result.reverse()
                    } else {
                        result
                    }
                }
                (None, None) => Ordering::Equal,
                (None, Some(_)) => {
                    if flags & 4 != 0 {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                }
                (Some(_), None) => {
                    if flags & 4 != 0 {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    }
                }
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Database {
    pub name: String,
    pub kind: u32,
    pub flags: u32,
    pub sorts: Vec<Sort>,
    pub modified: u64,
    pub records: BTreeMap<u32, Properties>,
}
impl Database {
    pub fn ordered(&self, sort: &Sort) -> Vec<u32> {
        let mut result: Vec<_> = self.records.keys().copied().collect();
        if !sort.properties.is_empty() {
            result.sort_by(|a, b| {
                sort.compare(&self.records[a], &self.records[b])
                    .then_with(|| a.cmp(b))
            });
        }
        result
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    magic: String,
    pub guid: Guid,
    pub next_oid: u32,
    pub databases: BTreeMap<u32, Database>,
}
impl Volume {
    fn new(guid: Guid) -> Self {
        Self {
            magic: "PocketHLE CEDB 1".into(),
            guid,
            next_oid: if guid == [0; 16] { 1 } else { 0 },
            databases: BTreeMap::new(),
        }
    }
    pub fn alloc_oid(&mut self) -> Option<u32> {
        self.next_oid = self.next_oid.checked_add(1)?;
        (self.next_oid != u32::MAX).then_some(self.next_oid)
    }
}
#[derive(Debug, Clone)]
pub struct Cursor {
    pub volume: Guid,
    pub database: u32,
    pub record: Option<u32>,
    pub autoincrement: bool,
    pub sort: Sort,
}
#[derive(Debug, Clone)]
pub struct Mount {
    pub path: String,
    pub references: u32,
}
#[derive(Default)]
pub struct Store {
    pub mounts: HashMap<Guid, Mount>,
    pub cursors: HashMap<u32, Cursor>,
    next_handle: u32,
}

fn read_file(vfs: &mut Vfs, path: &str) -> Result<Vec<u8>, u32> {
    let h = vfs.open_file(path, 1, 3, 3, false, 0)?.handle;
    let result = (|| {
        let size = vfs.size(h).ok_or(30u32)?;
        if size > MAX_VOLUME_BYTES as u64 {
            return Err(13);
        }
        let mut data = vec![0; size as usize];
        let mut offset = 0;
        while offset < data.len() {
            let count = vfs.read(h, &mut data[offset..]).ok_or(30u32)?;
            if count == 0 {
                return Err(30);
            }
            offset += count;
        }
        Ok(data)
    })();
    vfs.close(h);
    result
}
fn decode(data: &[u8], guid: Guid) -> Result<Volume, u32> {
    let volume: Volume = serde_json::from_slice(data).map_err(|_| 13u32)?;
    if volume.magic != "PocketHLE CEDB 1"
        || volume.guid != guid
        || volume.databases.len() > 4096
        || volume.databases.values().any(|db| {
            db.records.len() > 65535
                || db.sorts.len() > 4
                || db.sorts.iter().any(|s| s.properties.len() > 3)
        })
    {
        return Err(13);
    }
    Ok(volume)
}
fn save_file(vfs: &mut Vfs, path: &str, volume: &Volume) -> Result<(), u32> {
    let bytes = serde_json::to_vec(volume).map_err(|_| 13u32)?;
    if bytes.len() > MAX_VOLUME_BYTES {
        return Err(112);
    }
    let original = match read_file(vfs, path) {
        Ok(data) => data,
        Err(2 | 3) => Vec::new(),
        Err(error) => return Err(error),
    };
    let h = vfs.open_file(path, 3, 3, 4, false, 0)?.handle;
    let write = |vfs: &mut Vfs, bytes: &[u8]| -> Result<(), u32> {
        vfs.seek(h, 0, SeekKind::Begin).ok_or(29u32)?;
        if vfs.write(h, bytes) != Some(bytes.len()) {
            return Err(vfs.write_failure_error(h));
        }
        if !vfs.set_end_of_file(h) {
            return Err(vfs.write_failure_error(h));
        }
        vfs.flush(h).map_err(|_| 29u32)
    };
    let result = write(vfs, &bytes);
    if result.is_err() {
        let _ = write(vfs, &original);
    }
    vfs.close(h);
    result
}
pub fn modified_now() -> u64 {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    116_444_736_000_000_000 + (duration.as_nanos() / 100) as u64
}
fn path_guid(path: &str) -> Guid {
    let path = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\\")
        .to_lowercase();
    let hash = |seed: u64| {
        path.bytes().fold(seed, |value, byte| {
            (value ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
    };
    let mut guid = [0; 16];
    guid[..8].copy_from_slice(&hash(0xcbf29ce484222325).to_le_bytes());
    guid[8..].copy_from_slice(&hash(0x9e3779b97f4a7c15).to_le_bytes());
    guid[6] = (guid[6] & 15) | 0x40;
    guid[8] = (guid[8] & 63) | 0x80;
    guid
}
impl Store {
    fn path(&self, guid: Guid) -> Result<String, u32> {
        if guid == [0; 16] {
            Ok("\\Windows\\PocketHLE-System.cedb.json".into())
        } else {
            self.mounts.get(&guid).map(|m| m.path.clone()).ok_or(6)
        }
    }
    pub fn mount(&mut self, vfs: &mut Vfs, path: &str, flags: u32) -> Result<Guid, u32> {
        let _gate = STORE_GATE.lock().map_err(|_| 29u32)?;
        let mode = flags & 0xffff;
        if !(1..=5).contains(&mode) || flags & !0x10007 != 0 {
            return Err(87);
        }
        let guid = path_guid(path);
        if let Some(mount) = self.mounts.get_mut(&guid) {
            if mode == 1 {
                return Err(183);
            }
            if mode == 2 || mode == 5 {
                return Err(32);
            }
            mount.references = mount.references.checked_add(1).ok_or(8u32)?;
            return Ok(guid);
        }
        if self.mounts.len() >= 16 {
            return Err(8);
        }
        let existing = read_file(vfs, path);
        let volume = match (mode, existing) {
            (1, Ok(_)) => return Err(183),
            (2, _) => Volume::new(guid),
            (5, Err(error)) | (3, Err(error)) => return Err(error),
            (5, Ok(_)) => Volume::new(guid),
            (_, Ok(data)) => decode(&data, guid)?,
            (1 | 4, Err(2 | 3)) => Volume::new(guid),
            (_, Err(error)) => return Err(error),
        };
        if matches!(mode, 1 | 2 | 4 | 5) {
            save_file(vfs, path, &volume)?;
        }
        self.mounts.insert(
            guid,
            Mount {
                path: path.into(),
                references: 1,
            },
        );
        Ok(guid)
    }
    pub fn unmount(&mut self, guid: Guid) -> Result<(), u32> {
        if self.cursors.values().any(|c| c.volume == guid) {
            return Err(32);
        }
        let mount = self.mounts.get_mut(&guid).ok_or(6u32)?;
        mount.references -= 1;
        if mount.references == 0 {
            self.mounts.remove(&guid);
        }
        Ok(())
    }
    fn load(&self, vfs: &mut Vfs, guid: Guid) -> Result<Volume, u32> {
        let path = self.path(guid)?;
        match read_file(vfs, &path) {
            Ok(bytes) => decode(&bytes, guid),
            Err(2 | 3) if guid == [0; 16] => Ok(Volume::new(guid)),
            Err(error) => Err(error),
        }
    }
    pub fn snapshot(&self, vfs: &mut Vfs, guid: Guid) -> Result<Volume, u32> {
        let _gate = STORE_GATE.lock().map_err(|_| 29u32)?;
        self.load(vfs, guid)
    }
    pub fn update<T>(
        &mut self,
        vfs: &mut Vfs,
        guid: Guid,
        change: impl FnOnce(&mut Volume) -> Result<T, u32>,
    ) -> Result<T, u32> {
        let _gate = STORE_GATE.lock().map_err(|_| 29u32)?;
        let mut volume = self.load(vfs, guid)?;
        let result = change(&mut volume)?;
        save_file(vfs, &self.path(guid)?, &volume)?;
        Ok(result)
    }
    pub fn open(&mut self, cursor: Cursor) -> Result<u32, u32> {
        if self.cursors.len() >= 64 || self.next_handle >= 0x00ff_ffff {
            return Err(8);
        }
        self.next_handle += 1;
        let handle = 0xe300_0000 + self.next_handle;
        self.cursors.insert(handle, cursor);
        Ok(handle)
    }
    pub fn flush(&self, vfs: &mut Vfs, guid: Guid) -> Result<(), u32> {
        let _gate = STORE_GATE.lock().map_err(|_| 29u32)?;
        let path = self.path(guid)?;
        let h = vfs.open_file(&path, 3, 3, 3, false, 0)?.handle;
        let result = vfs.flush(h).map_err(|_| 29u32);
        vfs.close(h);
        result
    }
}
