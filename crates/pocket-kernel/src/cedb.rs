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

#[derive(Default)]
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
