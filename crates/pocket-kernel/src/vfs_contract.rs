//! File contracts shared by all CPUs of one emulated device.
use super::*;
use std::sync::Weak;
#[derive(Clone, Default)]
pub struct VfsShared(pub(super) Arc<Mutex<Shared>>);
#[derive(Default)]
pub(super) struct Shared {
    pub(super) leases: HashMap<String, Vec<Weak<Lease>>>,
    // Shared by processes/duplicate handles, under the same gate as quota checks.
    // Rescan at opens, namespace changes and explicit space queries; byte writes
    // adjust the exact logical length instead of walking Flash Disk every time.
    volume_usage: HashMap<PathBuf, u64>,
    #[cfg(test)]
    volume_scans: usize,
    attributes: HashMap<String, u32>,
    bluetooth: crate::bluetooth::Service,
    camera: crate::camera::Service,
    gps: crate::gps::Service,
    vibration: crate::vibration::Service,
}
impl Shared {
    fn volume_used(&mut self, root: &Path) -> Result<u64, u32> {
        if let Some(&bytes) = self.volume_usage.get(root) {
            return Ok(bytes);
        }
        let bytes = used(root)?;
        #[cfg(test)]
        {
            self.volume_scans += 1;
        }
        self.volume_usage.insert(root.to_path_buf(), bytes);
        Ok(bytes)
    }
}
#[derive(Debug)]
pub(super) struct Lease {
    pub access: u32,
    pub(super) mapped: std::sync::atomic::AtomicUsize,
    share: u32,
    pub(super) volume: Option<(PathBuf, u64)>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct OpenResult {
    pub handle: u32,
    pub existed: bool,
}
fn io_error(e: &std::io::Error) -> u32 {
    match e.kind() {
        std::io::ErrorKind::NotFound => 2,
        std::io::ErrorKind::PermissionDenied => 5,
        std::io::ErrorKind::AlreadyExists => 183,
        std::io::ErrorKind::DirectoryNotEmpty => 145,
        std::io::ErrorKind::InvalidInput => 87,
        _ => 29,
    }
}
fn used(root: &Path) -> Result<u64, u32> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(root).map_err(|e| io_error(&e))? {
        let e = entry.map_err(|e| io_error(&e))?;
        let m = e.path().symlink_metadata().map_err(|e| io_error(&e))?;
        if m.is_symlink() {
            continue;
        }
        total = total.saturating_add(if m.is_dir() {
            used(&e.path())?
        } else {
            m.len()
        });
    }
    Ok(total)
}
fn busy(state: &mut Shared, key: &str) -> bool {
    state.leases.retain(|_, v| {
        v.retain(|l| l.strong_count() != 0);
        !v.is_empty()
    });
    state.leases.get(key).is_some_and(|v| !v.is_empty())
}
impl Vfs {
    /// Copy through guest file contracts, including sharing and volume quota.
    pub fn copy_file(&mut self, from: &str, to: &str, fail_if_exists: bool) -> Result<(), u32> {
        let source = self.open_file(from, 1, 1, 3, false, 0)?.handle;
        let result = (|| {
            let size = self.size(source).ok_or(30u32)?;
            let dest_key = self.file_key(to, true)?;
            let source_key = if let Some(file) = self.handles.get(&source) {
                file.lock()
                    .map_err(|_| 30u32)?
                    .host_path
                    .canonicalize()
                    .map_err(|e| io_error(&e))?
                    .to_string_lossy()
                    .to_ascii_lowercase()
            } else {
                self.file_key(from, false)?
            };
            if source_key == dest_key {
                return Err(87);
            }
            if fail_if_exists && self.attributes(to).is_ok() {
                return Err(80);
            }
            if !self.is_ram_path(to) {
                let (host, mount) = self.exact_path(to, true)?;
                if let Some((root, total)) = Self::quota(mount) {
                    let old = host.metadata().map(|m| m.len()).unwrap_or(0);
                    if size.saturating_sub(old) > total.saturating_sub(used(&root)?) {
                        return Err(112);
                    }
                }
            }
            let attrs = self.attributes(from)? & 0x27;
            let dest = self
                .open_file(to, 2, 0, if fail_if_exists { 1 } else { 2 }, false, 0)?
                .handle;
            let copied = (|| {
                let mut buffer = [0u8; 16384];
                loop {
                    let count = self.read(source, &mut buffer).ok_or(30u32)?;
                    if count == 0 {
                        break;
                    }
                    let mut written = 0;
                    while written < count {
                        let n = self.write(dest, &buffer[written..count]).ok_or(112u32)?;
                        if n == 0 {
                            return Err(29);
                        }
                        written += n;
                    }
                }
                self.flush(dest).map_err(|e| io_error(&e))
            })();
            self.close(dest);
            if copied.is_err() && fail_if_exists {
                let _ = self.delete(to);
            }
            copied?;
            self.set_attributes(to, attrs)
        })();
        self.close(source);
        result
    }
    pub fn shared_context(&self) -> VfsShared {
        self.shared.clone()
    }
    pub fn attach_shared_context(&mut self, shared: VfsShared) {
        self.bluetooth.service = shared.0.lock().unwrap().bluetooth.clone();
        self.shared = shared;
    }
    pub fn vibration_service(&self) -> crate::vibration::Service {
        self.shared.0.lock().unwrap().vibration.clone()
    }
    pub fn set_vibration_service(&self, service: crate::vibration::Service) {
        self.shared.0.lock().unwrap().vibration = service;
    }
    pub fn gps_service(&self) -> crate::gps::Service {
        self.shared.0.lock().unwrap().gps.clone()
    }
    pub fn set_gps_service(&self, service: crate::gps::Service) {
        self.shared.0.lock().unwrap().gps = service;
    }
    pub fn camera_service(&self) -> crate::camera::Service {
        self.shared.0.lock().unwrap().camera.clone()
    }
    pub fn set_camera_service(&self, service: crate::camera::Service) {
        self.shared.0.lock().unwrap().camera = service;
    }
    pub(super) fn bluetooth_service(&self) -> crate::bluetooth::Service {
        self.shared.0.lock().unwrap().bluetooth.clone()
    }
    pub(super) fn set_shared_bluetooth_service(&self, service: crate::bluetooth::Service) {
        self.shared.0.lock().unwrap().bluetooth = service;
    }
    fn exact_path(&self, path: &str, write: bool) -> Result<(PathBuf, &Mount), u32> {
        let normal = self.normalise_guest_path(path);
        let mounts = self.matching_mounts(&normal);
        let first = *mounts.first().ok_or(3u32)?;
        // A more specific read-only mount cannot be bypassed through a broader writable root.
        let mount = if write {
            mounts
                .into_iter()
                .find(|m| m.prefix == first.prefix && !m.read_only)
                .ok_or(5u32)?
        } else {
            first
        };
        // Case folding is a lookup key, never the spelling of a newly created file.
        let spelling = self.spelled_guest_path(path);
        let host = self.host_path_for_mount(mount, &spelling).ok_or(5u32)?;
        let root = mount.host_dir.canonicalize().map_err(|e| io_error(&e))?;
        let ancestor = host
            .ancestors()
            .find(|p| p.exists())
            .ok_or(3u32)?
            .canonicalize()
            .map_err(|e| io_error(&e))?;
        if !ancestor.starts_with(root) {
            return Err(5);
        }
        Ok((host, mount))
    }
    fn file_key(&self, path: &str, write: bool) -> Result<String, u32> {
        if self.is_ram_path(path) {
            return self.ram_key(path).map(|k| format!("ram:{k}")).ok_or(87);
        }
        let (host, _) = self.exact_path(path, write)?;
        let resolved = if host.exists() {
            host.canonicalize().map_err(|e| io_error(&e))?
        } else {
            let parent = host
                .parent()
                .ok_or(3u32)?
                .canonicalize()
                .map_err(|_| 3u32)?;
            parent.join(host.file_name().ok_or(87u32)?)
        };
        Ok(resolved.to_string_lossy().to_ascii_lowercase())
    }
    fn quota(mount: &Mount) -> Option<(PathBuf, u64)> {
        (mount.prefix == "/flash disk/").then(|| {
            (
                mount
                    .host_dir
                    .canonicalize()
                    .unwrap_or_else(|_| mount.host_dir.clone()),
                32 * 1024 * 1024,
            )
        })
    }
    /// Legacy internal callers retain their create-parent convenience; guest APIs use open_file.
    pub fn open(&mut self, path: &str, access: Access, create: bool) -> Option<u32> {
        if create && !self.is_ram_path(path) {
            if let Ok((host, _)) = self.exact_path(path, true) {
                if let Some(p) = host.parent() {
                    std::fs::create_dir_all(p).ok();
                }
            }
        }
        let bits = match access {
            Access::Read => 1,
            Access::Write => 2,
            Access::ReadWrite => 3,
        };
        let disposition = if create {
            if access == Access::Write {
                2
            } else {
                4
            }
        } else {
            3
        };
        self.open_file(path, bits, 3, disposition, false, 0)
            .ok()
            .map(|r| r.handle)
    }
    pub fn open_file(
        &mut self,
        path: &str,
        access: u32,
        share: u32,
        disposition: u32,
        append: bool,
        attributes: u32,
    ) -> Result<OpenResult, u32> {
        if path.is_empty() || access > 3 || share & !3 != 0 || !(1..=5).contains(&disposition) {
            return Err(87);
        }
        let normal = self.normalise_guest_path(path);
        if normal.rsplit('/').next() == Some("bth1:") {
            if disposition != 3 {
                return Err(87);
            }
            let mut state = self.shared.0.lock().unwrap();
            let key = "bluetooth:BTH1";
            busy(&mut state, key);
            if state.leases.get(key).is_some_and(|leases| {
                leases
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|l| access & !l.share != 0 || l.access & !share != 0)
            }) {
                return Err(32);
            }
            let lease = Arc::new(Lease {
                mapped: Default::default(),
                access,
                share,
                volume: None,
            });
            let handle = self.next_handle;
            self.next_handle = self.next_handle.checked_add(1).ok_or(8u32)?;
            self.bluetooth_driver_handles.insert(
                handle,
                Arc::new(BluetoothDriverOpen {
                    service: state.bluetooth.clone(),
                    access,
                    _lease: lease.clone(),
                }),
            );
            state
                .leases
                .entry(key.into())
                .or_default()
                .push(Arc::downgrade(&lease));
            return Ok(OpenResult {
                handle,
                existed: true,
            });
        }
        if normal.rsplit('/').next() == Some("vib1:") {
            if disposition != 3 {
                return Err(87);
            }
            let mut state = self.shared.0.lock().unwrap();
            let key = "vibration:VIB1";
            busy(&mut state, key);
            if state.leases.get(key).is_some_and(|leases| {
                leases
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|l| access & !l.share != 0 || l.access & !share != 0)
            }) {
                return Err(32);
            }
            let service = state.vibration.clone();
            let lease = Arc::new(Lease {
                mapped: Default::default(),
                access,
                share,
                volume: None,
            });
            let handle = self.next_handle;
            self.next_handle = self.next_handle.checked_add(1).ok_or(8u32)?;
            self.vibration_handles.insert(
                handle,
                Arc::new(VibrationOpen {
                    service,
                    access,
                    _lease: lease.clone(),
                }),
            );
            state
                .leases
                .entry(key.into())
                .or_default()
                .push(Arc::downgrade(&lease));
            return Ok(OpenResult {
                handle,
                existed: true,
            });
        }
        if normal.rsplit('/').next() == Some("gps1:") {
            if disposition != 3 {
                return Err(87);
            }
            let mut state = self.shared.0.lock().unwrap();
            let key = "gps:GPS1";
            busy(&mut state, key);
            if state.leases.get(key).is_some_and(|leases| {
                leases.iter().filter_map(Weak::upgrade).any(|l| {
                    access & !l.share != 0
                        || l.access & !share != 0
                        || (access & 2 != 0 && l.access & 2 != 0)
                })
            }) {
                return Err(32);
            }
            let device = state.gps.open()?;
            let lease = Arc::new(Lease {
                mapped: Default::default(),
                access,
                share,
                volume: None,
            });
            let handle = self.next_handle;
            self.next_handle = self.next_handle.checked_add(1).ok_or(8u32)?;
            self.gps_handles.insert(
                handle,
                Arc::new(GpsOpen {
                    device,
                    access,
                    _lease: lease.clone(),
                }),
            );
            state
                .leases
                .entry(key.into())
                .or_default()
                .push(Arc::downgrade(&lease));
            return Ok(OpenResult {
                handle,
                existed: true,
            });
        }
        if normal.rsplit('/').next() == Some("cam1:") {
            if disposition != 3 {
                return Err(87);
            }
            let mut state = self.shared.0.lock().unwrap();
            let key = "camera:CAM1";
            busy(&mut state, key);
            if state.leases.get(key).is_some_and(|leases| {
                leases
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|l| access & !l.share != 0 || l.access & !share != 0)
            }) {
                return Err(32);
            }
            let device = state.camera.open()?;
            let lease = Arc::new(Lease {
                mapped: Default::default(),
                access,
                share,
                volume: None,
            });
            let handle = self.next_handle;
            self.next_handle = self.next_handle.checked_add(1).ok_or(8u32)?;
            self.camera_handles.insert(
                handle,
                Arc::new(CameraOpen {
                    device,
                    access,
                    _lease: lease.clone(),
                }),
            );
            state
                .leases
                .entry(key.into())
                .or_default()
                .push(Arc::downgrade(&lease));
            return Ok(OpenResult {
                handle,
                existed: true,
            });
        }

        if let Some(index) = normal
            .rsplit('/')
            .next()
            .and_then(|s| s.strip_prefix("com"))
            .and_then(|s| s.strip_suffix(':'))
            .and_then(|s| s.parse::<u32>().ok())
        {
            if disposition != 3 {
                return Err(87);
            }
            let (registration, port) = self.bluetooth.service.registered_port(index).ok_or(2u32)?;
            let key = format!("bluetooth:COM{index}:{registration}");
            let mut state = self.shared.0.lock().unwrap();
            busy(&mut state, &key);
            if state.leases.get(&key).is_some_and(|leases| {
                leases
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|l| access & !l.share != 0 || l.access & !share != 0)
            }) {
                return Err(32);
            }
            let lease = Arc::new(Lease {
                mapped: Default::default(),
                access,
                share,
                volume: None,
            });
            let handle = self.next_handle;
            self.next_handle = self.next_handle.checked_add(1).ok_or(8u32)?;
            self.bluetooth_handles.insert(
                handle,
                Arc::new(BluetoothOpen {
                    port,
                    access,
                    _lease: lease.clone(),
                }),
            );
            state
                .leases
                .entry(key)
                .or_default()
                .push(Arc::downgrade(&lease));
            return Ok(OpenResult {
                handle,
                existed: true,
            });
        }
        let device = normal
            .rsplit('/')
            .next()
            .is_some_and(|p| p == "mas1:" || p == "reg1:" || p == "vol:");
        if device {
            if disposition != 3 {
                return Err(87);
            }
            let a = match access {
                2 => Access::Write,
                3 => Access::ReadWrite,
                _ => Access::Read,
            };
            return self
                .open_legacy(path, a, false)
                .map(|handle| OpenResult {
                    handle,
                    existed: true,
                })
                .ok_or(2);
        }
        if disposition == 5 && access & 2 == 0 {
            return Err(87);
        }
        let mut host = None;
        let mut volume = None;
        let mut read_only = false;
        let key = if self.is_ram_path(path) {
            self.file_key(path, false)?
        } else {
            let (exact, mount) = self.exact_path(
                path,
                access & 2 != 0 || disposition == 1 || disposition == 2,
            )?;
            volume = Self::quota(mount);
            read_only = mount.read_only;
            // Preserve the validated wrapped-install fallback for reads only.
            let mut target = if access == 1 && disposition == 3 {
                self.resolve(path).unwrap_or(exact)
            } else {
                exact
            };
            let root = mount.host_dir.canonicalize().map_err(|e| io_error(&e))?;
            let ancestor = target
                .ancestors()
                .find(|p| p.exists())
                .ok_or(3u32)?
                .canonicalize()
                .map_err(|e| io_error(&e))?;
            if !ancestor.starts_with(&root)
                && !(access == 1
                    && disposition == 3
                    && self.matching_mounts(path).iter().any(|m| {
                        m.host_dir
                            .canonicalize()
                            .is_ok_and(|root| ancestor.starts_with(root))
                    }))
            {
                return Err(5);
            }
            // Copy-on-write: OPEN_ALWAYS/OPEN_EXISTING with write access
            // must preserve an installed file from the read-only lower layer.
            // Creating an empty overlay file instead silently destroys the
            // guest's view of archives and databases opened read/write.
            let normal = self.normalise_guest_path(path);
            let lower = if !read_only && !target.exists() {
                self.matching_mounts(&normal)
                    .into_iter()
                    .filter(|m| m.read_only && m.prefix == mount.prefix)
                    .find_map(|m| {
                        let candidate = self.host_path_for_mount(m, &normal)?;
                        candidate.is_file().then(|| (candidate, &m.host_dir))
                    })
            } else {
                None
            };
            if lower.is_some() && disposition == 1 {
                return Err(80);
            }
            // Opening an existing file for writing must not rename it. The
            // lookup key is case folded, but directory enumeration preserves
            // the installed spelling when materializing a writable copy.
            if let Some((source, source_root)) = &lower {
                let relative = source.strip_prefix(source_root).map_err(|_| 5u32)?;
                let spelling = format!(
                    "{}{}",
                    mount.prefix,
                    relative.to_string_lossy().replace('\\', "/")
                );
                target = self.host_path_for_mount(mount, &spelling).ok_or(5u32)?;
            }
            // Materialize only directories which already exist in the merged
            // guest filesystem. An actually missing parent still fails.
            if !read_only && (matches!(disposition, 1 | 2 | 4) || lower.is_some()) {
                if let Some(parent) = target.parent().filter(|p| !p.exists()) {
                    if let Some((guest_parent, _)) = normal.rsplit_once('/') {
                        let exists = self.matching_mounts(guest_parent).iter().any(|m| {
                            self.host_path_for_mount(m, guest_parent)
                                .is_some_and(|p| p.is_dir())
                        });
                        if exists {
                            std::fs::create_dir_all(parent).map_err(|e| io_error(&e))?;
                        }
                    }
                }
            }
            if let Some((source, source_root)) = lower {
                let source = source.canonicalize().map_err(|e| io_error(&e))?;
                let source_root = source_root.canonicalize().map_err(|e| io_error(&e))?;
                if !source.starts_with(source_root) {
                    return Err(5);
                }
                let source_key = source.to_string_lossy().to_ascii_lowercase();
                let mut state = self.shared.0.lock().unwrap();
                busy(&mut state, &source_key);
                if state.leases.get(&source_key).is_some_and(|leases| {
                    leases
                        .iter()
                        .filter_map(Weak::upgrade)
                        .any(|l| access & !l.share != 0 || l.access & !share != 0)
                }) {
                    return Err(32);
                }
                if access & 2 != 0
                    && state
                        .attributes
                        .get(&source_key)
                        .is_some_and(|a| a & 1 != 0)
                {
                    return Err(5);
                }
                if matches!(disposition, 2 | 5)
                    && state.leases.get(&source_key).is_some_and(|leases| {
                        leases
                            .iter()
                            .filter_map(Weak::upgrade)
                            .any(|l| l.mapped.load(std::sync::atomic::Ordering::Relaxed) != 0)
                    })
                {
                    return Err(1224);
                }
                std::fs::copy(source, &target).map_err(|e| io_error(&e))?;
            }
            let k = if target.exists() {
                target.canonicalize().map_err(|e| io_error(&e))?
            } else {
                target
                    .parent()
                    .ok_or(3u32)?
                    .canonicalize()
                    .map_err(|_| 3u32)?
                    .join(target.file_name().ok_or(87u32)?)
            }
            .to_string_lossy()
            .to_ascii_lowercase();
            host = Some(target);
            k
        };
        let shared = self.shared.clone();
        let mut state = shared.0.lock().unwrap();
        busy(&mut state, &key);
        if state.leases.get(&key).is_some_and(|leases| {
            leases
                .iter()
                .filter_map(Weak::upgrade)
                .any(|l| access & !l.share != 0 || l.access & !share != 0)
        }) {
            return Err(32);
        }
        if (disposition == 2 || disposition == 5)
            && state.leases.get(&key).is_some_and(|leases| {
                leases
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|l| l.mapped.load(std::sync::atomic::Ordering::Relaxed) != 0)
            })
        {
            return Err(1224); // ERROR_USER_MAPPED_FILE
        }
        let existed = if let Some(p) = &host {
            p.exists()
        } else {
            self.ram_file_size(path).is_some()
        };
        if !existed && read_only {
            return Err(5);
        }
        if disposition == 1 && existed {
            return Err(80);
        }
        if (disposition == 3 || disposition == 5) && !existed {
            return Err(2);
        }
        if access & 2 != 0 && state.attributes.get(&key).is_some_and(|a| a & 1 != 0) {
            return Err(5);
        }
        let a = match access {
            2 => Access::Write,
            3 => Access::ReadWrite,
            _ => Access::Read,
        };
        let lease = Arc::new(Lease {
            mapped: Default::default(),
            access,
            share,
            volume,
        });
        let h = if let Some(path) = host {
            if path.is_dir() {
                return Err(5);
            }
            let mut options = OpenOptions::new();
            options
                .read(access & 1 != 0 || access == 0)
                .write(access & 2 != 0);
            match disposition {
                1 => {
                    options.create_new(true);
                }
                2 => {
                    options.create(true).truncate(true);
                }
                4 => {
                    options.create(true);
                }
                5 => {
                    options.truncate(true);
                }
                _ => {}
            }
            // An open is a reconciliation boundary, including host-side edits
            // between uses and truncation performed by the disposition below.
            state.volume_usage.clear();
            // Creating a read-only handle still creates the file, without granting guest write access.
            if access & 2 == 0
                && !existed
                && (disposition == 1 || disposition == 2 || disposition == 4)
            {
                let mut create = OpenOptions::new();
                create.write(true).create_new(true);
                create.open(&path).map_err(|e| io_error(&e))?;
                options.create(false).create_new(false).truncate(false);
            }
            if disposition == 2 && existed && access & 2 == 0 {
                return Err(5);
            }
            let file = options.open(&path).map_err(|e| io_error(&e))?;
            let handle = self.next_handle;
            self.next_handle = self.next_handle.checked_add(1).ok_or(8u32)?;
            self.handles.insert(
                handle,
                Arc::new(Mutex::new(OpenFile {
                    host_path: path,
                    access: a,
                    file,
                    text_mode: false,
                    lease: Some(lease.clone()),
                    append,
                    for_mapping: false,
                })),
            );
            handle
        } else {
            let ram = self.ram.as_ref().unwrap().clone();
            let rk = self.ram_key(path).ok_or(87u32)?;
            let store = ram.files.lock().unwrap();
            if store.dirs.contains_key(&rk) {
                return Err(5);
            }
            let parent = rk.rsplit_once('/').map(|p| p.0).unwrap_or("");
            if !parent.is_empty() && !store.dirs.contains_key(parent) {
                return Err(3);
            }
            drop(store);
            // open_ram already implements charging and transactional allocation.
            let h = self
                .open_ram(path, if !existed { Access::ReadWrite } else { a }, !existed)
                .ok_or(112u32)?;
            let open = self.ram_handles.get(&h).unwrap().clone();
            let mut f = open.lock().unwrap();
            f.access = a;
            f.lease = Some(lease.clone());
            f.append = append;
            if (disposition == 2 || disposition == 5) && !f.file.lock().unwrap().set_len(0) {
                drop(f);
                self.close(h);
                return Err(112);
            }
            h
        };
        if !existed {
            state
                .attributes
                .insert(key.clone(), (attributes & 0x27) | 0x20);
        }
        state
            .leases
            .entry(key)
            .or_default()
            .push(Arc::downgrade(&lease));
        if append {
            self.seek(h, 0, SeekKind::End);
        }
        Ok(OpenResult { handle: h, existed })
    }
    pub(super) fn lease(&self, h: u32) -> Option<Arc<Lease>> {
        if let Some(f) = self.handles.get(&h) {
            return f.lock().ok()?.lease.clone();
        }
        self.ram_handles.get(&h)?.lock().ok()?.lease.clone()
    }
    pub(super) fn growth_allowed(&self, h: u32, size: u64, old: u64, state: &mut Shared) -> bool {
        let Some(l) = self.lease(h) else {
            return true;
        };
        if l.access & 2 == 0 {
            return false;
        }
        if size > old && self.mapping_pinned(&l, state) {
            return false;
        }
        let Some((root, total)) = &l.volume else {
            return true;
        };
        size <= old
            || state
                .volume_used(root)
                .is_ok_and(|u| size - old <= total.saturating_sub(u))
    }
    pub(super) fn account_resize(&self, h: u32, old: u64, new: u64, state: &mut Shared) {
        if let Some(lease) = self.lease(h) {
            if let Some((root, _)) = &lease.volume {
                // Another alias may expose a nested/parent host volume. Its
                // cached total must be reconciled before its next quota check.
                state
                    .volume_usage
                    .retain(|cached_root, _| cached_root == root);
                if let Some(used) = state.volume_usage.get_mut(root) {
                    *used = used.saturating_sub(old).saturating_add(new);
                }
                return;
            }
        }
        // A writable non-quota mount can alias files below Flash Disk too.
        // RAM/device writes cannot change any host volume and need no rescan.
        if self.handles.contains_key(&h) {
            state.volume_usage.clear();
        }
    }
    pub fn write(&mut self, h: u32, data: &[u8]) -> Option<usize> {
        let shared = self.shared.clone();
        let mut state = shared.0.lock().unwrap();
        let old = self.size(h).unwrap_or(0);
        let pos = if let Some(f) = self.handles.get(&h) {
            let mut f = f.lock().ok()?;
            if f.append {
                old
            } else {
                f.file.stream_position().ok()?
            }
        } else {
            0
        };
        let end = pos.checked_add(data.len() as u64)?;
        if !self.growth_allowed(h, end, old, &mut state) {
            return None;
        }
        let result = self.write_raw(h, data);
        if let Some(n) = result {
            // Partial writes consume only bytes actually added. Overwrites do
            // not spend quota; seeking beyond EOF charges the intervening gap.
            if n != 0 {
                self.account_resize(h, old, old.max(pos + n as u64), &mut state);
            }
        } else {
            // A host I/O error may have changed the file: reconcile next time.
            state.volume_usage.clear();
        }
        result
    }

    pub fn set_end_of_file(&mut self, h: u32) -> bool {
        let shared = self.shared.clone();
        let mut state = shared.0.lock().unwrap();
        let old = self.size(h).unwrap_or(0);
        let size = if let Some(f) = self.handles.get(&h) {
            let Ok(mut f) = f.lock() else {
                return false;
            };
            let Ok(p) = f.file.stream_position() else {
                return false;
            };
            p
        } else {
            0
        };
        if size != old
            && self
                .lease(h)
                .is_some_and(|l| self.mapping_pinned(&l, &state))
        {
            return false;
        }
        if !self.growth_allowed(h, size, old, &mut state) {
            return false;
        }
        if !self.set_end_of_file_raw(h) {
            state.volume_usage.clear();
            return false;
        }
        self.account_resize(h, old, size, &mut state);
        true
    }
    pub fn disk_space(&self, path: &str) -> Result<(u64, u64), u32> {
        if self.is_ram_path(path) {
            if path != "\\" && path != "/" && self.list_dir(path).is_none() {
                return Err(3);
            }
            let ram = self.ram.as_ref().unwrap();
            let s = ram.snapshot();
            return Ok((
                s.store_pages as u64 * ram.page_size() as u64,
                s.store_free() as u64 * ram.page_size() as u64,
            ));
        }
        let (p, m) = self.exact_path(path, false)?;
        if !p.is_dir() {
            return Err(3);
        }
        let mut state = self.shared.0.lock().unwrap();
        let occupied = used(&m.host_dir)?;
        state.volume_usage.clear();
        let root = m
            .host_dir
            .canonicalize()
            .unwrap_or_else(|_| m.host_dir.clone());
        state.volume_usage.insert(root, occupied);
        let total = if let Some((_, total)) = Self::quota(m) {
            total
        } else {
            occupied
                .max(64 * 1024 * 1024)
                .checked_next_power_of_two()
                .unwrap_or(u64::MAX)
        };
        Ok((total, total.saturating_sub(occupied)))
    }
    pub fn attributes(&self, path: &str) -> Result<u32, u32> {
        if self.is_ram_path(path) {
            let key = self.file_key(path, false)?;
            if self.ram_file_size(path).is_some() {
                return Ok(self
                    .shared
                    .0
                    .lock()
                    .unwrap()
                    .attributes
                    .get(&key)
                    .copied()
                    .unwrap_or(0x20));
            }
            return self.list_dir(path).is_some().then_some(0x10).ok_or(2);
        }
        let (exact, m) = self.exact_path(path, false)?;
        let p = self.resolve(path).unwrap_or(exact);
        let meta = p.metadata().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound && !p.parent().is_some_and(Path::is_dir) {
                3
            } else {
                io_error(&e)
            }
        })?;
        let key = p
            .canonicalize()
            .map_err(|e| io_error(&e))?
            .to_string_lossy()
            .to_ascii_lowercase();
        let flags = self
            .shared
            .0
            .lock()
            .unwrap()
            .attributes
            .get(&key)
            .copied()
            .unwrap_or(if meta.is_dir() { 0x10 } else { 0x80 });
        // WinCE marks the root directory of a removable card TEMPORARY.
        // Files and subdirectories on that volume retain their own attributes.
        let card_root = self.mounts.iter().any(|mount| {
            mount.storage_card
                && mount.prefix.trim_end_matches('/')
                    == self.normalise_guest_path(path).trim_end_matches('/')
        });
        Ok(flags
            | if card_root && meta.is_dir() { 0x100 } else { 0 }
            | if m.read_only || meta.permissions().readonly() {
                1
            } else {
                0
            }
            | if meta.is_dir() { 0x10 } else { 0 })
    }
    pub fn set_attributes(&self, path: &str, flags: u32) -> Result<(), u32> {
        if flags & !(0x27 | 0x80) != 0 {
            return Err(87);
        }
        self.attributes(path)?;
        let key = self.file_key(path, true)?;
        self.shared.0.lock().unwrap().attributes.insert(
            key,
            if flags & 0x27 == 0 {
                0x80
            } else {
                flags & 0x27
            },
        );
        Ok(())
    }
    pub fn create_directory(&self, path: &str) -> Result<(), u32> {
        if self.attributes(path).is_ok() {
            return Err(183);
        }
        if !self.is_ram_path(path) {
            let (p, _) = self.exact_path(path, true)?;
            if !p.parent().is_some_and(Path::is_dir) {
                return Err(3);
            }
        }
        if self.create_dir_raw(path) {
            Ok(())
        } else {
            Err(if self.is_ram_path(path) { 112 } else { 5 })
        }
    }
    pub fn create_dir(&self, path: &str) -> bool {
        self.create_directory(path).is_ok()
    }
    pub fn delete(&self, path: &str) -> Result<(), u32> {
        let attrs = self.attributes(path)?;
        if attrs & 0x11 != 0 {
            return Err(5);
        }
        let key = self.file_key(path, true)?;
        let shared = self.shared.clone();
        let mut s = shared.0.lock().unwrap();
        if busy(&mut s, &key) {
            return Err(32);
        }
        s.volume_usage.clear();
        if !self.delete_file_raw(path) {
            return Err(5);
        }
        s.attributes.remove(&key);
        Ok(())
    }
    pub fn delete_file(&self, path: &str) -> bool {
        self.delete(path).is_ok()
    }
    pub fn remove_directory(&self, path: &str) -> Result<(), u32> {
        if self.attributes(path)? & 0x10 == 0 {
            return Err(267);
        }
        if self.list_dir(path).is_some_and(|v| !v.is_empty()) {
            return Err(145);
        }
        if self.is_ram_path(path) {
            return self.remove_ram_dir(path).then_some(()).ok_or(5);
        }
        let (p, m) = self.exact_path(path, true)?;
        if p == m.host_dir {
            return Err(5);
        }
        std::fs::remove_dir(p).map_err(|e| io_error(&e))
    }
    pub fn rename(&self, from: &str, to: &str) -> Result<(), u32> {
        self.attributes(from)?;
        if self.attributes(to).is_ok() {
            return Err(183);
        }
        let source = self.file_key(from, true)?;
        let dest = self.file_key(to, true)?;
        let shared = self.shared.clone();
        let mut s = shared.0.lock().unwrap();
        if busy(&mut s, &source)
            || s.leases.iter().any(|(key, v)| {
                key.starts_with(&format!("{source}/")) && v.iter().any(|l| l.strong_count() != 0)
            })
        {
            return Err(32);
        }
        s.volume_usage.clear();
        if !self.move_file_raw(from, to) {
            return Err(5);
        }
        if let Some(a) = s.attributes.remove(&source) {
            s.attributes.insert(dest, a);
        }
        Ok(())
    }
    pub fn move_file(&self, from: &str, to: &str) -> bool {
        self.rename(from, to).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_files_keep_requested_case_and_old_overlays_recover_without_losing_bytes() {
        let lower = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        std::fs::write(lower.path().join("Vehicle.DAT"), b"original").unwrap();
        std::fs::write(saves.path().join("vehicle.dat"), b"registered").unwrap();
        let mut v = Vfs::new();
        v.mount_read_only("\\Game\\", lower.path());
        v.mount_save_dir("\\Game\\", saves.path());
        assert_eq!(
            std::fs::read(saves.path().join("Vehicle.DAT")).unwrap(),
            b"registered"
        );
        assert_eq!(v.list_dir("\\GAME").unwrap()[0].0, "Vehicle.DAT");
        let h = v
            .open_file("\\GAME\\NewFile.DAT", 2, 3, 2, false, 0)
            .unwrap()
            .handle;
        v.close(h);
        assert!(saves.path().join("NewFile.DAT").exists());
        assert_eq!(
            std::fs::read(lower.path().join("Vehicle.DAT")).unwrap(),
            b"original"
        );
    }

    #[test]
    #[cfg(unix)]
    fn overlay_case_recovery_preserves_ambiguous_files_and_does_not_follow_links() {
        let lower = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::fs::write(lower.path().join("Vehicle.DAT"), b"asset").unwrap();
        std::fs::write(saves.path().join("vehicle.dat"), b"one").unwrap();
        std::fs::write(saves.path().join("VEHICLE.DAT"), b"two").unwrap();
        std::fs::write(external.path().join("vehicle.dat"), b"external").unwrap();
        std::os::unix::fs::symlink(external.path(), saves.path().join("link")).unwrap();
        let mut v = Vfs::new();
        v.mount_read_only("\\Game\\", lower.path());
        v.mount_save_dir("\\Game\\", saves.path());
        assert_eq!(
            std::fs::read(saves.path().join("vehicle.dat")).unwrap(),
            b"one"
        );
        assert_eq!(
            std::fs::read(saves.path().join("VEHICLE.DAT")).unwrap(),
            b"two"
        );
        assert!(external.path().join("vehicle.dat").exists());
        assert!(!external.path().join("Vehicle.DAT").exists());
    }

    #[test]
    fn writable_copy_preserves_installed_spelling_and_case_insensitive_lookup() {
        let lower = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        std::fs::create_dir(lower.path().join("Models")).unwrap();
        std::fs::write(lower.path().join("Models/Vehicle.DAT"), b"original").unwrap();
        let mut v = Vfs::new();
        v.mount_read_only("\\Game\\", lower.path());
        v.mount_save_dir("\\Game\\", saves.path());
        let path = "\\game\\models\\vehicle.dat";
        let h = v.open_file(path, 3, 3, 3, false, 0).unwrap().handle;
        assert_eq!(v.write(h, b"modified"), Some(8));
        v.close(h);
        assert_eq!(
            std::fs::read(saves.path().join("Models/Vehicle.DAT")).unwrap(),
            b"modified"
        );
        let entries = v.list_dir("\\GAME\\MODELS").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "Vehicle.DAT");
        let h = v
            .open_file("\\Game\\Models\\VEHICLE.dat", 1, 3, 3, false, 0)
            .unwrap()
            .handle;
        let mut bytes = [0; 8];
        assert_eq!(v.read(h, &mut bytes), Some(8));
        assert_eq!(&bytes, b"modified");
        v.close(h);
        assert_eq!(
            std::fs::read(lower.path().join("Models/Vehicle.DAT")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn creates_files_in_existing_lower_layer_directories_without_inventing_guest_parents() {
        let lower = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(lower.path().join("resources/scenes")).unwrap();
        std::fs::write(lower.path().join("resources/scenes/original"), b"asset").unwrap();
        let mut v = Vfs::new();
        v.mount_read_only("\\Game\\", lower.path());
        v.mount_save_dir("\\Game\\", saves.path());
        let path = "\\Game\\resources\\scenes\\preview.jpg";
        let h = v.open_file(path, 2, 3, 2, false, 0).unwrap().handle;
        assert_eq!(v.write(h, b"preview"), Some(7));
        v.close(h);
        assert_eq!(
            std::fs::read(saves.path().join("resources/scenes/preview.jpg")).unwrap(),
            b"preview"
        );
        assert!(!lower.path().join("resources/scenes/preview.jpg").exists());
        let h = v.open_file(path, 1, 3, 3, false, 0).unwrap().handle;
        let mut bytes = [0; 7];
        assert_eq!(v.read(h, &mut bytes), Some(7));
        assert_eq!(&bytes, b"preview");
        v.close(h);
        assert_eq!(
            v.open_file("\\Game\\missing\\preview.jpg", 2, 3, 2, false, 0),
            Err(3)
        );
        assert!(!saves.path().join("missing").exists());
    }

    #[test]
    fn writable_overlay_preserves_existing_assets_for_open_always_and_overrides_reads() {
        let lower = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(lower.path().join("maps")).unwrap();
        std::fs::write(lower.path().join("maps/level.gz"), b"original").unwrap();
        let mut v = Vfs::new();
        v.mount_read_only("\\Game\\", lower.path());
        v.mount_save_dir("\\Game\\", saves.path());
        let path = "\\Game\\maps\\level.gz";
        assert_eq!(v.open_file(path, 3, 3, 1, false, 0), Err(80));
        let open = v.open_file(path, 3, 3, 4, false, 0).unwrap();
        assert!(open.existed);
        let mut bytes = [0; 8];
        assert_eq!(v.read(open.handle, &mut bytes), Some(8));
        assert_eq!(&bytes, b"original");
        v.seek(open.handle, 0, SeekKind::Begin);
        assert_eq!(v.write(open.handle, b"modified"), Some(8));
        v.close(open.handle);
        let h = v.open_file(path, 1, 3, 3, false, 0).unwrap().handle;
        assert_eq!(v.read(h, &mut bytes), Some(8));
        assert_eq!(&bytes, b"modified");
        v.close(h);
        assert_eq!(
            std::fs::read(lower.path().join("maps/level.gz")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(saves.path().join("maps/level.gz")).unwrap(),
            b"modified"
        );
    }

    #[test]
    fn copying_an_installed_file_into_the_overlay_respects_existing_share_denials() {
        let lower = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        std::fs::write(lower.path().join("data"), b"asset").unwrap();
        let mut v = Vfs::new();
        v.mount_read_only("\\Game\\", lower.path());
        v.mount_save_dir("\\Game\\", saves.path());
        let h = v
            .open_file("\\Game\\data", 1, 0, 3, false, 0)
            .unwrap()
            .handle;
        assert_eq!(v.open_file("\\Game\\data", 3, 3, 4, false, 0), Err(32));
        assert!(!saves.path().join("data").exists());
        v.close(h);
        let open = v.open_file("\\Game\\data", 3, 3, 3, false, 0).unwrap();
        assert!(open.existed);
        v.close(open.handle);
    }

    fn mounted(dir: &Path) -> Vfs {
        let mut v = Vfs::new();
        v.mount_save_dir("\\Flash Disk\\", dir);
        v
    }
    #[test]
    fn copy_preserves_data_and_enforces_same_file_sharing_and_existing_destination() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("source"), vec![0x5a; 40000]).unwrap();
        let mut v = mounted(d.path());
        let from = "\\Flash Disk\\source";
        let to = "\\Flash Disk\\copy";
        v.copy_file(from, to, true).unwrap();
        assert_eq!(
            std::fs::read(d.path().join("copy")).unwrap(),
            vec![0x5a; 40000]
        );
        assert_eq!(v.copy_file(from, to, true), Err(80));
        assert_eq!(v.copy_file(from, from, false), Err(87));
        let h = v.open_file(to, 1, 1, 3, false, 0).unwrap().handle;
        assert_eq!(v.copy_file(from, to, false), Err(32));
        v.close(h);
        std::fs::write(d.path().join("source"), b"short").unwrap();
        v.copy_file(from, to, false).unwrap();
        assert_eq!(std::fs::read(d.path().join("copy")).unwrap(), b"short");
        assert!(v.open_handles().is_empty());
    }

    #[test]
    fn copy_between_ram_and_flash_and_quota_failure_preserves_old_destination() {
        let d = tempfile::tempdir().unwrap();
        let mut v = mounted(d.path());
        v.attach_ram(Some(crate::memory_division::MemoryDivision::default()));
        v.create_directory("/copytest").unwrap();
        let h = v
            .open_file("/copytest/source", 3, 3, 1, false, 0)
            .unwrap()
            .handle;
        assert_eq!(v.write(h, b"ram data"), Some(8));
        v.close(h);
        v.copy_file("/copytest/source", "\\Flash Disk\\save", true)
            .unwrap();
        v.copy_file("\\Flash Disk\\save", "/copytest/back", true)
            .unwrap();
        let h = v
            .open_file("/copytest/back", 1, 3, 3, false, 0)
            .unwrap()
            .handle;
        let mut b = [0; 8];
        assert_eq!(v.read(h, &mut b), Some(8));
        assert_eq!(&b, b"ram data");
        v.close(h);
        let large = std::fs::File::create(d.path().join("large")).unwrap();
        large.set_len(32 * 1024 * 1024).unwrap();
        assert_eq!(
            v.copy_file("\\Flash Disk\\large", "\\Flash Disk\\save", false),
            Err(112)
        );
        assert_eq!(std::fs::read(d.path().join("save")).unwrap(), b"ram data");
        assert!(v.open_handles().is_empty());
    }

    #[test]
    fn diagnostic_seeded_file_operations_match_byte_model_on_flash_and_ram() {
        for ram in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut v = mounted(dir.path());
            let path = if ram {
                v = Vfs::new();
                v.attach_ram(Some(crate::memory_division::MemoryDivision::default()));
                assert!(v.create_dir("/test"));
                "/test/model"
            } else {
                "\\Flash Disk\\model"
            };
            let h = v.open_file(path, 3, 3, 1, false, 0).unwrap().handle;
            let mut expected = Vec::<u8>::new();
            let mut position = 0usize;
            let mut seed = 0x9c0a1c11u32;
            for step in 0..1024 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                match (seed >> 24) % 4 {
                    0 => {
                        let target = ((seed >> 8) as usize) % 32769;
                        assert_eq!(
                            v.seek(h, target as i64, SeekKind::Begin),
                            Some(target as u64)
                        );
                        position = target;
                    }
                    1 => {
                        let len = [0, 1, 4095, 4096, 4097][(seed >> 16) as usize % 5];
                        let bytes: Vec<u8> =
                            (0..len).map(|i| (seed as usize + i * 13) as u8).collect();
                        assert_eq!(v.write(h, &bytes), Some(len), "step {step}");
                        if len != 0 {
                            expected.resize(expected.len().max(position + len), 0);
                            expected[position..position + len].copy_from_slice(&bytes);
                            position += len;
                        }
                    }
                    2 => {
                        let mut bytes = [0xcc; 4097];
                        let n = expected.len().saturating_sub(position).min(bytes.len());
                        assert_eq!(v.read(h, &mut bytes), Some(n), "step {step}");
                        if n != 0 {
                            assert_eq!(&bytes[..n], &expected[position..position + n]);
                        }
                        assert!(bytes[n..].iter().all(|b| *b == 0xcc));
                        position += n;
                    }
                    _ => {
                        assert!(v.set_end_of_file(h));
                        expected.resize(position, 0);
                    }
                }
                assert_eq!(v.size(h), Some(expected.len() as u64), "step {step}");
                assert_eq!(v.seek(h, 0, SeekKind::Current), Some(position as u64));
                if step % 31 == 0 {
                    v.flush(h).unwrap();
                }
            }
            assert!(v.close(h));
            let h = v.open_file(path, 1, 3, 3, false, 0).unwrap().handle;
            let mut actual = vec![0; expected.len() + 1];
            assert_eq!(v.read(h, &mut actual), Some(expected.len()));
            assert_eq!(&actual[..expected.len()], expected);
            assert!(v.close(h));
            assert!(v.delete_file(path));
        }
    }

    #[test]
    fn creation_dispositions_preserve_or_truncate_on_host_and_ram() {
        let d = tempfile::tempdir().unwrap();
        for ram in [false, true] {
            let mut v = mounted(d.path());
            if ram {
                v = Vfs::new();
                v.attach_ram(Some(crate::memory_division::MemoryDivision::default()));
                v.create_dir("/test");
            }
            let path = if ram {
                "/test/save"
            } else {
                "\\Flash Disk\\save"
            };
            let h = v.open_file(path, 3, 3, 1, false, 0).unwrap();
            assert!(!h.existed);
            assert_eq!(v.write(h.handle, b"SAVE"), Some(4));
            v.close(h.handle);
            assert_eq!(v.open_file(path, 2, 3, 1, false, 0), Err(80));
            let h = v.open_file(path, 2, 3, 4, false, 0).unwrap();
            assert!(h.existed);
            assert_eq!(v.size(h.handle), Some(4));
            v.close(h.handle);
            let h = v.open_file(path, 3, 3, 2, false, 0).unwrap();
            assert_eq!(v.size(h.handle), Some(0));
            assert_eq!(v.write(h.handle, b"NEW"), Some(3));
            v.close(h.handle);
            let h = v.open_file(path, 2, 3, 5, false, 0).unwrap();
            assert_eq!(v.size(h.handle), Some(0));
            v.close(h.handle);
            assert!(v.delete_file(path));
            assert_eq!(v.open_file(path, 3, 3, 5, false, 0), Err(2));
        }
    }
    #[test]
    fn share_restrictions_survive_duplicates_and_cross_process_close() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("save"), b"data").unwrap();
        let mut a = mounted(d.path());
        let mut b = mounted(d.path());
        b.attach_shared_context(a.shared_context());
        let h = a
            .open_file("\\Flash Disk\\save", 1, 0, 3, false, 0)
            .unwrap()
            .handle;
        assert_eq!(
            b.open_file("\\Flash Disk\\save", 1, 3, 3, false, 0),
            Err(32)
        );
        b.import_handle(99, a.export_handle(h).unwrap());
        a.close(h);
        assert_eq!(a.delete("\\Flash Disk\\save"), Err(32));
        b.close(99);
        let h = a
            .open_file("\\Flash Disk\\save", 1, 1, 3, false, 0)
            .unwrap()
            .handle;
        assert_eq!(
            b.open_file("\\Flash Disk\\save", 2, 3, 3, false, 0),
            Err(32)
        );
        assert_eq!(
            b.open_file("\\Flash Disk\\save", 1, 0, 3, false, 0),
            Err(32)
        );
        let h2 = b
            .open_file("\\Flash Disk\\save", 1, 1, 3, false, 0)
            .unwrap()
            .handle;
        a.close(h);
        b.close(h2);
        assert!(a.delete_file("\\Flash Disk\\save"));
    }
    #[test]
    fn append_mode_always_writes_at_end_after_seek() {
        let d = tempfile::tempdir().unwrap();
        let mut v = mounted(d.path());
        let h = v
            .open_file("\\Flash Disk\\save", 3, 3, 4, true, 0)
            .unwrap()
            .handle;
        assert_eq!(v.write(h, b"ONE"), Some(3));
        v.seek(h, 0, SeekKind::Begin);
        assert_eq!(v.write(h, b"TWO"), Some(3));
        v.seek(h, 0, SeekKind::Begin);
        let mut buf = [0; 6];
        assert_eq!(v.read(h, &mut buf), Some(6));
        assert_eq!(&buf, b"ONETWO");
    }
    #[test]
    fn flash_quota_small_writes_scan_once_and_charge_shared_writers() {
        let d = tempfile::tempdir().unwrap();
        let mut v = mounted(d.path());
        let a = v
            .open_file("\\Flash Disk\\a", 3, 3, 1, false, 0)
            .unwrap()
            .handle;
        let mut other = mounted(d.path());
        other.attach_shared_context(v.shared_context());
        let b = other
            .open_file("\\Flash Disk\\b", 3, 3, 1, false, 0)
            .unwrap()
            .handle;
        for _ in 0..2000 {
            assert_eq!(v.write(a, b"12345678"), Some(8));
        }
        assert_eq!(v.shared.0.lock().unwrap().volume_scans, 1);
        assert_eq!(
            v.disk_space("\\Flash Disk\\").unwrap().1,
            32 * 1024 * 1024 - 16000
        );
        // The second writer consumes the remaining space, including sparse gaps.
        other.seek(b, (32 * 1024 * 1024 - 16000) as i64, SeekKind::Begin);
        assert!(other.set_end_of_file(b));
        assert_eq!(v.write(a, b"X"), None);
        v.seek(a, 0, SeekKind::Begin);
        assert_eq!(v.write(a, b"overwrite"), Some(9));
        assert!(v.set_end_of_file(a)); // Truncation refunds space immediately.
        assert_eq!(other.write(b, b"refunded"), Some(8));
        v.close(a);
        v.delete("\\Flash Disk\\a").unwrap();
        assert_eq!(v.disk_space("\\Flash Disk\\").unwrap().1, 15992);
    }
    #[test]
    fn flash_quota_observes_writes_through_another_mount_alias() {
        let d = tempfile::tempdir().unwrap();
        let mut v = mounted(d.path());
        v.mount_save_dir("/alias/", d.path());
        let a = v
            .open_file("/flash disk/a", 3, 3, 1, false, 0)
            .unwrap()
            .handle;
        let b = v.open_file("/alias/b", 3, 3, 1, false, 0).unwrap().handle;
        assert_eq!(v.write(a, b"A"), Some(1));
        v.seek(b, 32 * 1024 * 1024 - 1, SeekKind::Begin);
        assert!(v.set_end_of_file(b));
        assert_eq!(v.write(a, b"B"), None);
        assert_eq!(v.size(a), Some(1));
    }
    #[test]
    fn flash_quota_refunds_truncation_and_refuses_overflow_atomically() {
        let d = tempfile::tempdir().unwrap();
        let mut v = mounted(d.path());
        let before = v.disk_space("\\Flash Disk\\").unwrap();
        assert_eq!(before, (32 * 1024 * 1024, 32 * 1024 * 1024));
        let h = v
            .open_file("\\Flash Disk\\large", 3, 3, 1, false, 0)
            .unwrap()
            .handle;
        v.seek(h, before.1 as i64 + 1, SeekKind::Begin);
        assert!(!v.set_end_of_file(h));
        assert_eq!(v.size(h), Some(0));
        v.seek(h, before.1 as i64, SeekKind::Begin);
        assert!(v.set_end_of_file(h));
        assert_eq!(v.disk_space("\\Flash Disk\\").unwrap().1, 0);
        assert_eq!(v.write(h, b"X"), None);
        assert_eq!(v.size(h), Some(before.1));
        v.seek(h, 0, SeekKind::Begin);
        assert!(v.set_end_of_file(h));
        assert_eq!(v.disk_space("\\Flash Disk\\").unwrap(), before);
    }
    #[test]
    fn directory_removal_and_rename_do_not_succeed_fictitiously() {
        let d = tempfile::tempdir().unwrap();
        let mut v = mounted(d.path());
        assert_eq!(v.create_directory("\\Flash Disk\\missing\\child"), Err(3));
        v.create_directory("\\Flash Disk\\scratch").unwrap();
        assert_eq!(v.create_directory("\\Flash Disk\\scratch"), Err(183));
        let h = v
            .open_file("\\Flash Disk\\scratch\\a", 2, 3, 1, false, 0)
            .unwrap()
            .handle;
        assert_eq!(v.remove_directory("\\Flash Disk\\scratch"), Err(145));
        assert_eq!(
            v.rename("\\Flash Disk\\scratch\\a", "\\Flash Disk\\scratch\\b"),
            Err(32)
        );
        v.close(h);
        std::fs::write(d.path().join("scratch/b"), b"DEST").unwrap();
        assert_eq!(
            v.rename("\\Flash Disk\\scratch\\a", "\\Flash Disk\\scratch\\b"),
            Err(183)
        );
        assert_eq!(std::fs::read(d.path().join("scratch/b")).unwrap(), b"DEST");
        v.delete("\\Flash Disk\\scratch\\a").unwrap();
        v.delete("\\Flash Disk\\scratch\\b").unwrap();
        v.remove_directory("\\Flash Disk\\scratch").unwrap();
        assert!(!d.path().join("scratch").exists());
    }
    #[test]
    fn protected_mount_cannot_be_written_through_broader_root() {
        let broad = tempfile::tempdir().unwrap();
        let card = tempfile::tempdir().unwrap();
        let mut v = Vfs::new();
        v.mount_save_dir("\\", broad.path());
        v.mount_read_only("\\SD Card\\", card.path());
        assert_eq!(v.open_file("\\SD Card\\new", 2, 3, 2, false, 0), Err(5));
        assert_eq!(v.create_directory("\\SD Card\\dir"), Err(5));
    }
    #[test]
    fn readonly_attributes_and_query_access_are_enforced() {
        let d = tempfile::tempdir().unwrap();
        let mut v = mounted(d.path());
        let path = "\\Flash Disk\\save";
        let h = v.open_file(path, 3, 3, 1, false, 0).unwrap().handle;
        v.close(h);
        v.set_attributes(path, 3).unwrap();
        assert_eq!(v.attributes(path).unwrap() & 3, 3);
        assert_eq!(v.open_file(path, 2, 3, 3, false, 0), Err(5));
        assert_eq!(v.delete(path), Err(5));
        let h = v.open_file(path, 0, 3, 3, false, 0).unwrap().handle;
        assert_eq!(v.read(h, &mut [0; 1]), None);
        assert_eq!(v.write(h, b"X"), None);
        v.close(h);
        v.set_attributes(path, 0x80).unwrap();
        v.delete(path).unwrap();
    }
    #[test]
    fn negative_seek_preserves_position_on_host() {
        let d = tempfile::tempdir().unwrap();
        let mut v = mounted(d.path());
        let h = v
            .open_file("\\Flash Disk\\save", 3, 3, 1, false, 0)
            .unwrap()
            .handle;
        v.seek(h, 10, SeekKind::Begin);
        assert_eq!(v.seek(h, -1, SeekKind::Begin), None);
        assert_eq!(v.seek(h, 0, SeekKind::Current), Some(10));
    }
}
