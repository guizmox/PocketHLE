//! A tiny in-memory Windows CE registry.
//!
//! Pocket PC games keep more than preferences in `HKLM` / `HKCU`: the
//! CAB installer writes the paths a title later reads back to find its
//! own data. Astraware Bejeweled, for example, refuses to start (it
//! calls `ExitProcess(0x42)`) unless
//! `HKLM\SOFTWARE\Apps\Astraware Bejeweled\SaveDir` exists, because
//! that is where its `_setup.xml` told the installer to put saves.
//!
//! The store is deliberately simple:
//!
//! * Keys are canonical strings such as
//!   `HKLM\SOFTWARE\Apps\Astraware Bejeweled`, compared
//!   case-insensitively (WinCE registry keys are case-insensitive).
//! * Values are `REG_SZ`, `REG_DWORD` or `REG_BINARY`.
//! * `RegOpenKeyEx` hands out integer handles that map back to the
//!   canonical path, so `RegQueryValueEx` can resolve them without the
//!   guest holding a pointer into host memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Value types we model — the subset Pocket PC titles actually use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryValue {
    /// `REG_SZ` (stored as UTF-8, handed to the guest as UTF-16).
    Sz(String),
    /// `REG_DWORD`.
    Dword(u32),
    /// `REG_BINARY`.
    Binary(Vec<u8>),
}

impl RegistryValue {
    /// The `REG_*` type code reported through `RegQueryValueEx`.
    pub fn type_code(&self) -> u32 {
        match self {
            RegistryValue::Sz(_) => 1,
            RegistryValue::Binary(_) => 3,
            RegistryValue::Dword(_) => 4,
        }
    }

    /// The value encoded exactly as the guest expects to receive it.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            RegistryValue::Sz(text) => text
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
            RegistryValue::Dword(value) => value.to_le_bytes().to_vec(),
            RegistryValue::Binary(bytes) => bytes.clone(),
        }
    }
}

/// First handle handed out by [`Registry::open`]. Chosen to be
/// obviously not a predefined `HKEY_*` constant (`0x8000_000n`) and not
/// to collide with the VFS or GDI fake-handle ranges.
const HANDLE_BASE: u32 = 0xDEAD_9100;

#[derive(Debug, Default)]
pub(crate) struct RegistryStore {
    keys: HashMap<String, HashMap<String, RegistryValue>>,
    display: HashMap<String, String>,
    charge: Option<crate::memory_division::StoreCharge>,
}
#[derive(Debug, Default)]
pub struct Registry {
    store: Arc<Mutex<RegistryStore>>,
    /// Handles belong to a process; the values belong to the device.
    handles: HashMap<u32, String>,
    next_handle: u32,
}

/// Map a predefined `HKEY_*` constant to its canonical prefix.
fn root_prefix(root: u32) -> Option<&'static str> {
    match root {
        0x8000_0000 => Some("HKCR"),
        0x8000_0001 => Some("HKCU"),
        0x8000_0002 => Some("HKLM"),
        0x8000_0003 => Some("HKU"),
        _ => None,
    }
}

/// Normalise the textual form of a key path: single backslashes, no
/// leading or trailing separator, `HKEY_LOCAL_MACHINE` style prefixes
/// folded to the short form used internally.
pub fn canonical_key(path: &str) -> String {
    let mut parts: Vec<&str> = path
        .split('\\')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if let Some(first) = parts.first_mut() {
        *first = match first.to_ascii_uppercase().as_str() {
            "HKEY_CLASSES_ROOT" => "HKCR",
            "HKEY_CURRENT_USER" => "HKCU",
            "HKEY_LOCAL_MACHINE" => "HKLM",
            "HKEY_USERS" => "HKU",
            _ => *first,
        };
    }
    parts.join("\\")
}

impl Registry {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(RegistryStore::default())),
            handles: HashMap::new(),
            next_handle: HANDLE_BASE,
        }
    }

    /// A registry pre-populated the way a Pocket PC device would be.
    ///
    /// `HKCU\ControlPanel\Owner` exists on every device (the "Owner
    /// Information" control panel), and games read the owner name to
    /// personalise menus. MetalStrike additionally reads its own
    /// licence pair, which earlier work established as `1739` / `0`.
    /// The `Drivers\BuiltIn` entries describe hardware the emulated
    /// device claims to have; a title that polls for a missing driver
    /// tends to give up and exit rather than fall back.
    pub fn with_device_defaults() -> Self {
        let mut reg = Self::new();
        reg.set_value(
            r"HKCU\ControlPanel\Owner",
            "Owner",
            RegistryValue::Sz("Argon".to_string()),
        );
        reg.set_value(
            r"HKLM\SOFTWARE\Greatelsoft.Com\MetalStrike",
            "SN-Key1",
            RegistryValue::Dword(1739),
        );
        reg.set_value(
            r"HKLM\SOFTWARE\Greatelsoft.Com\MetalStrike",
            "SN-Key2",
            RegistryValue::Dword(0),
        );
        // The g-sensor. Tilt-controlled titles find the accelerometer by
        // opening its driver key rather than by asking a sensor API, so
        // a device without this key reads as a device without a sensor —
        // Xtrakt polls for it (`RegOpenKeyEx`, `Sleep(10)`, retry) and
        // shuts down when the retries run out.
        //
        // `poll_delay` is the sampling interval in milliseconds and
        // `resolution` the counts-per-g the driver reports; both are the
        // values shipped on the HTC handsets these games targeted.
        reg.set_value(
            r"HKLM\Drivers\BuiltIn\Accelerometer",
            "poll_delay",
            RegistryValue::Dword(20),
        );
        reg.set_value(
            r"HKLM\Drivers\BuiltIn\Accelerometer",
            "resolution",
            RegistryValue::Dword(1000),
        );
        // Which way up the driver currently believes the device is.
        // `0` is the unrotated portrait orientation, which is what an
        // emulated screen always is.
        reg.set_value(
            r"HKLM\Drivers\BuiltIn\Accelerometer",
            "current_rotation",
            RegistryValue::Dword(0),
        );
        // The companion screen-rotation service. A game that rotates
        // itself reads `OverrideCounter` to suppress the shell's own
        // auto-rotation while it is in the foreground; zero means
        // nothing is currently overriding it.
        reg.set_value(
            r"HKLM\Services\MultiService\mods\Rotation",
            "OverrideCounter",
            RegistryValue::Dword(0),
        );
        reg
    }

    /// Resolve the `(root, subkey)` pair a `Reg*` call was given.
    ///
    /// `root` is either a predefined `HKEY_*` constant or a handle we
    /// previously returned from [`Registry::open`], which is how games
    /// walk down a tree one level at a time.
    pub fn resolve(&self, root: u32, subkey: &str) -> Option<String> {
        let base = match root_prefix(root) {
            Some(prefix) => prefix.to_string(),
            None => self.handles.get(&root)?.clone(),
        };
        let sub = canonical_key(subkey);
        if sub.is_empty() {
            Some(canonical_key(&base))
        } else {
            Some(canonical_key(&format!("{base}\\{sub}")))
        }
    }

    pub fn contains_key(&self, path: &str) -> bool {
        self.store.lock().unwrap_or_else(|e| e.into_inner()).keys
            .contains_key(&canonical_key(path).to_ascii_lowercase())
    }

    fn stored_pages(keys: &HashMap<String, HashMap<String, RegistryValue>>) -> u32 {
        let mut bytes = 0u64;
        for (key, values) in keys {
            bytes += (key.encode_utf16().count() as u64 + 1) * 2;
            for (name, value) in values {
                bytes += (name.encode_utf16().count() as u64 + 1) * 2 + 4 + value.to_bytes().len() as u64;
            }
        }
        crate::memory_division::pages(bytes)
    }
    pub fn attach_ram(&mut self, ram: Option<&crate::memory_division::MemoryDivision>) -> bool {
        let Some(ram) = ram else {
            let current = self.store.lock().unwrap_or_else(|e| e.into_inner());
            let store = RegistryStore { keys: current.keys.clone(), display: current.display.clone(), charge: None };
            drop(current);
            self.store = Arc::new(Mutex::new(store));
            return true;
        };
        if Arc::ptr_eq(&self.store, &ram.registry) { return true; }
        let current = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut target = ram.registry.lock().unwrap_or_else(|e| e.into_inner());
        let mut keys = target.keys.clone();
        // A child's boot defaults must not overwrite the launcher's live
        // registry. Explicit frontend settings are written after attachment.
        for (key, values) in &current.keys {
            let dest = keys.entry(key.clone()).or_default();
            for (name, value) in values { dest.entry(name.clone()).or_insert_with(|| value.clone()); }
        }
        let pages = Self::stored_pages(&keys);
        if let Some(charge) = &mut target.charge {
            if !charge.resize(pages) { return false; }
        } else {
            let Some(charge) = ram.store_charge(pages) else { return false; };
            target.charge = Some(charge);
        }
        target.keys = keys;
        for (key, name) in &current.display { target.display.entry(key.clone()).or_insert_with(|| name.clone()); }
        drop(target);
        drop(current);
        self.store = ram.registry.clone();
        true
    }
    pub fn create_key(&mut self, path: &str) -> bool {
        let canonical = canonical_key(path);
        let lower = canonical.to_ascii_lowercase();
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        if store.keys.contains_key(&lower) { return true; }
        let mut keys = store.keys.clone();
        keys.insert(lower.clone(), HashMap::new());
        if store.charge.as_mut().is_some_and(|charge| !charge.resize(Self::stored_pages(&keys))) { return false; }
        store.display.entry(lower).or_insert(canonical);
        store.keys = keys;
        true
    }
    pub fn set_value(&mut self, path: &str, name: &str, value: RegistryValue) -> bool {
        let canonical = canonical_key(path);
        let lower = canonical.to_ascii_lowercase();
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut keys = store.keys.clone();
        keys.entry(lower.clone()).or_default().insert(name.to_ascii_lowercase(), value);
        if store.charge.as_mut().is_some_and(|charge| !charge.resize(Self::stored_pages(&keys))) { return false; }
        store.display.entry(lower).or_insert(canonical);
        store.keys = keys;
        true
    }
    pub fn value(&self, path: &str, name: &str) -> Option<RegistryValue> {
        self.store.lock().unwrap_or_else(|e| e.into_inner()).keys
            .get(&canonical_key(path).to_ascii_lowercase())?.get(&name.to_ascii_lowercase()).cloned()
    }
    pub fn delete_value(&mut self, path: &str, name: &str) -> bool {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let removed = store.keys.get_mut(&canonical_key(path).to_ascii_lowercase())
            .and_then(|values| values.remove(&name.to_ascii_lowercase())).is_some();
        if removed {
            let pages = Self::stored_pages(&store.keys);
            if let Some(charge) = &mut store.charge { charge.resize(pages); }
        }
        removed
    }

    /// Hand out a handle for an existing key. Returns `None` when the
    /// key was never created, so `RegOpenKeyEx` can report
    /// `ERROR_FILE_NOT_FOUND` the way a real device does.
    pub fn open(&mut self, path: &str) -> Option<u32> {
        let canonical = canonical_key(path);
        let lower = canonical.to_ascii_lowercase();
        if !self.store.lock().unwrap_or_else(|e| e.into_inner()).keys.contains_key(&lower) {
            return None;
        }
        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        self.handles.insert(handle, lower);
        Some(handle)
    }

    /// Create the key if needed, then hand out a handle for it.
    pub fn create_and_open(&mut self, path: &str) -> u32 {
        if !self.create_key(path) { return 0; }
        self.open(path).unwrap_or(0)
    }

    pub fn path_for(&self, handle: u32) -> Option<String> {
        if let Some(prefix) = root_prefix(handle) {
            return Some(prefix.to_string());
        }
        self.handles.get(&handle).cloned()
    }

    pub fn close(&mut self, handle: u32) {
        self.handles.remove(&handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_division_registry_changes_obey_store_capacity() {
        let ram = crate::memory_division::MemoryDivision::new(64 * 4096, 8).unwrap();
        let mut registry = Registry::new();
        assert!(registry.attach_ram(Some(&ram)));
        assert!(registry.set_value("HKCU\\Test", "value", RegistryValue::Binary(vec![9; 4096])));
        let used = ram.snapshot().store_used;
        assert!(!registry.set_value("HKCU\\Test", "value", RegistryValue::Binary(vec![7; 8 * 4096])));
        assert_eq!(ram.snapshot().store_used, used);
        assert_eq!(registry.value("HKCU\\Test", "value"), Some(RegistryValue::Binary(vec![9; 4096])));
        assert!(registry.delete_value("HKCU\\Test", "value"));
        assert!(ram.snapshot().store_used < used);
        let mut child = Registry::new();
        assert!(child.attach_ram(Some(&ram)));
        assert_eq!(ram.snapshot().store_used, 1);
        assert!(child.set_value("HKCU\\Test", "shared", RegistryValue::Dword(42)));
        assert_eq!(registry.value("HKCU\\Test", "shared"), Some(RegistryValue::Dword(42)));
        drop(registry);
        assert_eq!(ram.snapshot().store_used, 1); // object store outlives a process
    }

    #[test]
    fn canonicalises_paths_and_roots() {
        assert_eq!(
            canonical_key(r"HKEY_LOCAL_MACHINE\SOFTWARE\Apps\"),
            r"HKLM\SOFTWARE\Apps"
        );
        assert_eq!(
            canonical_key(r"\ControlPanel\Owner\"),
            r"ControlPanel\Owner"
        );
    }

    #[test]
    fn stores_and_reads_values_case_insensitively() {
        let mut reg = Registry::new();
        reg.set_value(
            r"HKLM\SOFTWARE\Apps\Astraware Bejeweled",
            "SaveDir",
            RegistryValue::Sz(r"\My Documents\My Saved Games\Bejeweled".to_string()),
        );
        assert_eq!(
            reg.value(r"hklm\software\apps\astraware bejeweled", "savedir"),
            Some(RegistryValue::Sz(
                r"\My Documents\My Saved Games\Bejeweled".to_string()
            ))
        );
    }

    #[test]
    fn open_only_succeeds_for_existing_keys() {
        let mut reg = Registry::new();
        assert!(reg.open(r"HKLM\SOFTWARE\Nope").is_none());
        reg.create_key(r"HKLM\SOFTWARE\Yes");
        let handle = reg.open(r"HKLM\SOFTWARE\Yes").expect("key exists");
        assert_eq!(reg.path_for(handle).as_deref(), Some(r"hklm\software\yes"));
        // A handle can be used as the root of a relative open.
        reg.create_key(r"HKLM\SOFTWARE\Yes\Child");
        assert_eq!(
            reg.resolve(handle, "Child").as_deref(),
            Some(r"hklm\software\yes\Child")
        );
        reg.close(handle);
        assert!(reg.path_for(handle).is_none());
    }

    #[test]
    fn dword_and_string_encodings_match_win32() {
        assert_eq!(RegistryValue::Dword(1739).type_code(), 4);
        assert_eq!(RegistryValue::Dword(1739).to_bytes(), 1739u32.to_le_bytes());
        let sz = RegistryValue::Sz("AB".to_string());
        assert_eq!(sz.type_code(), 1);
        assert_eq!(sz.to_bytes(), vec![b'A', 0, b'B', 0, 0, 0]);
    }
}
