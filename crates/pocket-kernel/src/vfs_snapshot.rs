//! Persistent host file descriptions for the WinCE VFS.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::time::SystemTime;
#[path = "vfs_ram_snapshot.rs"]
mod ram;
use ram::RamSnapshot;

#[derive(Serialize, Deserialize)]
pub struct VfsFileSnapshot {
    descriptions: Vec<FileDescription>,
    identities: Vec<(u32, SnapshotObjectId)>,
    hidden_handles: Vec<u32>,
    camera_waits: Vec<((usize, u32, u32), std::time::Duration)>,
    volumes: Vec<VolumeDescription>,
    ram: Option<RamSnapshot>,
    registration: HashSet<u32>,
    bluetooth: crate::bluetooth::StateSnapshot,
    devices: Vec<DeviceDescription>,
    mas_decoders: Vec<MasDecoderSnapshot>,
    vibration: crate::vibration::Settings,
    default_dir: String,
    stdio_modes: [bool; 3],
    next_handle: u32,
}

#[derive(Serialize, Deserialize)]
struct VolumeDescription {
    handle: u32,
    mount_index: usize,
    prefix: String,
    read_only: bool,
}

#[derive(Serialize, Deserialize)]
struct DeviceDescription {
    kind: DeviceKind,
    handles: Vec<u32>,
    access: u32,
    share: u32,
}

#[derive(Clone, Serialize, Deserialize)]
enum DeviceKind {
    Bluetooth,
    BluetoothPort {
        index: u32,
        registration: u32,
        port: Arc<crate::bluetooth::PortSnapshot>,
    },
    Vibration,
    Gps {
        fixed: bool,
    },
    Camera(crate::camera::DeviceSnapshot),
    Accelerometer(AccelerometerStream),
}

#[derive(Serialize, Deserialize)]
struct MasDecoderSnapshot {
    handles: Vec<u32>,
    overlap: [Vec<f32>; 2],
    synthesis: Vec<f32>,
    header: [u8; 4],
    reservoir: Vec<u8>,
    reservoir_len: usize,
    stream_key: u32,
    callback_event: u32,
    pending: VecDeque<(u64, u64)>,
    completed_samples: u64,
    completed_bytes: u64,
    played_bytes: u64,
    bytes_seen: u64,
    encoded: Vec<u8>,
    decoded_offset: usize,
    pcm: Vec<i16>,
    sample_rate: u32,
    channels: u16,
    started: bool,
    paused: bool,
    volume: u32,
}

impl MasDecoderSnapshot {
    fn capture(handles: Vec<u32>, decoder: &Mp3DecoderState) -> Result<Self, String> {
        let native = decoder
            .decoder
            .layer3_state()
            .ok_or("invalid MAS decoder state")?;
        Ok(Self {
            handles,
            overlap: native.overlap.map(|values| values.to_vec()),
            synthesis: native.synthesis.to_vec(),
            header: native.header,
            reservoir: native.reservoir.to_vec(),
            reservoir_len: native.reservoir_len,
            stream_key: decoder.stream_key,
            callback_event: decoder.callback_event,
            pending: decoder.pending.clone(),
            completed_samples: decoder.completed_samples,
            completed_bytes: decoder.completed_bytes,
            played_bytes: decoder.played_bytes,
            bytes_seen: decoder.bytes_seen,
            encoded: decoder.encoded.clone(),
            decoded_offset: decoder.decoded_offset,
            pcm: decoder.pcm.clone(),
            sample_rate: decoder.sample_rate,
            channels: decoder.channels,
            started: decoder.started,
            paused: decoder.paused,
            volume: decoder.volume,
        })
    }

    fn restore(self) -> Result<Mp3DecoderState, String> {
        if self.decoded_offset > self.encoded.len()
            || (self.header[0] != 0 && mp3_frame_bytes(&self.header).is_none())
            || !(0xd3000000..u32::MAX).contains(&self.stream_key)
        {
            return Err("invalid saved MAS decoder state".into());
        }
        let mut previous = (self.completed_samples, self.completed_bytes);
        for &(samples, bytes) in &self.pending {
            if samples < previous.0 || bytes < previous.1 || bytes > self.bytes_seen {
                return Err("invalid saved MAS buffer completion".into());
            }
            previous = (samples, bytes);
        }
        let [left, right] = self.overlap;
        let invalid = |_| "invalid saved MAS decoder state";
        let decoder = RawDecoder::from_layer3_state(rmp3::Layer3State {
            overlap: [
                left.try_into().map_err(invalid)?,
                right.try_into().map_err(invalid)?,
            ],
            synthesis: self.synthesis.try_into().map_err(invalid)?,
            header: self.header,
            reservoir: self
                .reservoir
                .try_into()
                .map_err(|_| "invalid saved MAS reservoir")?,
            reservoir_len: self.reservoir_len,
        })
        .ok_or("invalid saved MAS decoder state")?;
        Ok(Mp3DecoderState {
            decoder,
            stream_key: self.stream_key,
            callback_event: self.callback_event,
            pending: self.pending,
            completed_samples: self.completed_samples,
            completed_bytes: self.completed_bytes,
            played_bytes: self.played_bytes,
            bytes_seen: self.bytes_seen,
            encoded: self.encoded,
            decoded_offset: self.decoded_offset,
            pcm: self.pcm,
            sample_rate: self.sample_rate,
            channels: self.channels,
            started: self.started,
            paused: self.paused,
            volume: self.volume,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct FileDescription {
    identity: u64,
    handles: Vec<u32>,
    mount_index: usize,
    mount_prefix: String,
    relative: PathBuf,
    length: u64,
    modified: Option<SystemTime>,
    cursor: u64,
    access: u32,
    share: u32,
    append: bool,
    text_mode: bool,
    for_mapping: bool,
    #[serde(with = "serde_bytes")]
    contents: Option<Vec<u8>>,
}

impl VfsFileSnapshot {
    pub(crate) fn prime_device_restore(
        &self,
        vfs: &Vfs,
        domain: &Arc<Mutex<SnapshotRestoreDomain>>,
    ) -> Result<(), String> {
        let mut paths = Vec::new();
        for record in &self.descriptions {
            if record.contents.is_none() {
                continue;
            }
            let mount = vfs
                .mounts
                .get(record.mount_index)
                .filter(|mount| mount.prefix == record.mount_prefix)
                .ok_or("saved VFS mount is missing")?;
            if !record
                .relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            {
                return Err("invalid saved file path".into());
            }
            let root = mount.host_dir.canonicalize().map_err(io_message)?;
            let path = root
                .join(&record.relative)
                .canonicalize()
                .map_err(io_message)?;
            if !path.starts_with(&root) || !path.is_file() {
                return Err("saved file escaped its mount".into());
            }
            paths.push(path);
        }
        domain
            .lock()
            .map_err(|_| "VFS restore domain poisoned")?
            .rewound_paths
            .extend(paths);
        Ok(())
    }
}

impl Vfs {
    /// Save a single process's regular file handles. Their backing files must
    /// remain mounted. Description identity preserves duplicate-handle cursors.
    pub fn capture_file_snapshot(&self) -> Result<VfsFileSnapshot, String> {
        self.capture_file_snapshot_with_mappings(
            &HashSet::new(),
            &HashMap::new(),
            false,
            std::time::Instant::now(),
        )
    }

    pub(crate) fn capture_file_snapshot_with_mappings(
        &self,
        mapping_files: &HashSet<u32>,
        private: &HashMap<u32, VfsObject>,
        coordinated: bool,
        at: std::time::Instant,
    ) -> Result<VfsFileSnapshot, String> {
        for (present, kind) in [(
            mapping_files.is_empty() && !self.section_files.is_empty(),
            "file mapping",
        )] {
            if present {
                return Err(format!("checkpoint contains an unsupported {kind}"));
            }
        }

        let mut decoders = self.decoders.clone();
        let mut bluetooth_driver_handles = self.bluetooth_driver_handles.clone();
        let mut bluetooth_handles = self.bluetooth_handles.clone();
        let mut vibration_handles = self.vibration_handles.clone();
        let mut gps_handles = self.gps_handles.clone();
        let mut camera_handles = self.camera_handles.clone();
        let mut htc_sensor_handles = self.htc_sensor_handles.clone();
        let mut volumes = self.volumes.clone();
        let mut registration = self.registration.clone();
        for (&handle, object) in private {
            match &object.0 {
                VfsObjectInner::Decoder(v) => {
                    decoders.insert(handle, v.clone());
                }
                VfsObjectInner::BluetoothDriver(v) => {
                    bluetooth_driver_handles.insert(handle, v.clone());
                }
                VfsObjectInner::Vibration(v) => {
                    vibration_handles.insert(handle, v.clone());
                }
                VfsObjectInner::Gps(v) => {
                    gps_handles.insert(handle, v.clone());
                }
                VfsObjectInner::Camera(v) => {
                    camera_handles.insert(handle, v.clone());
                }
                VfsObjectInner::HtcSensor(v) => {
                    htc_sensor_handles.insert(handle, *v);
                }
                VfsObjectInner::Volume(v) => {
                    volumes.insert(handle, v.clone());
                }
                VfsObjectInner::Registration => {
                    registration.insert(handle);
                }
                VfsObjectInner::Bluetooth(v) => {
                    bluetooth_handles.insert(handle, v.clone());
                }
                VfsObjectInner::File(_) | VfsObjectInner::Ram(_) => {}
            }
        }
        let mut decoder_aliases = HashMap::<_, Vec<_>>::new();
        for (&id, decoder) in &decoders {
            decoder_aliases
                .entry(Arc::as_ptr(decoder))
                .or_default()
                .push(id);
        }
        let mas_decoders = decoder_aliases
            .into_values()
            .map(|handles| {
                let decoder = decoders[&handles[0]]
                    .lock()
                    .map_err(|_| "MAS decoder lock poisoned")?;
                MasDecoderSnapshot::capture(handles, &decoder)
            })
            .collect::<Result<_, _>>()?;

        let mut devices = HashMap::new();
        let mut devices_by_lease = |id, lease: &Arc<Lease>, kind| {
            let record = devices
                .entry(Arc::as_ptr(lease))
                .or_insert_with(|| DeviceDescription {
                    kind,
                    handles: Vec::new(),
                    access: lease.access,
                    share: lease.share,
                });
            record.handles.push(id);
        };
        for (&id, open) in &bluetooth_driver_handles {
            devices_by_lease(id, &open._lease, DeviceKind::Bluetooth);
        }
        for (&id, open) in &bluetooth_handles {
            devices_by_lease(
                id,
                &open._lease,
                DeviceKind::BluetoothPort {
                    index: open.index,
                    registration: open.registration,
                    port: open.port.capture_snapshot(at)?,
                },
            );
        }
        for (&id, open) in &vibration_handles {
            devices_by_lease(id, &open._lease, DeviceKind::Vibration);
        }
        for (&id, open) in &gps_handles {
            let device = open.device.lock().map_err(|_| "GPS device lock poisoned")?;
            devices_by_lease(
                id,
                &open._lease,
                DeviceKind::Gps {
                    fixed: device.has_had_fix(),
                },
            );
        }
        for (&id, open) in &camera_handles {
            let device = open
                .device
                .lock()
                .map_err(|_| "camera device lock poisoned")?;
            devices_by_lease(
                id,
                &open._lease,
                DeviceKind::Camera(device.capture_snapshot(at)),
            );
        }
        let mut ids: HashMap<*const Mutex<OpenFile>, Vec<u32>> = HashMap::new();
        let private_files = private
            .iter()
            .filter_map(|(&id, object)| {
                if let VfsObjectInner::File(file) = &object.0 {
                    Some((id, file.clone()))
                } else {
                    None
                }
            })
            .collect::<HashMap<_, _>>();
        for (&handle, description) in self.handles.iter().chain(&private_files) {
            ids.entry(Arc::as_ptr(description))
                .or_default()
                .push(handle);
        }
        let descriptions = ids
            .into_values()
            .map(|mut handles| {
                handles.sort_unstable();
                let description = self
                    .handles
                    .get(&handles[0])
                    .or_else(|| private_files.get(&handles[0]))
                    .ok_or("missing snapshot file description")?;
                let identity = Arc::as_ptr(description) as usize as u64;
                let mut file = description.lock().map_err(|_| "VFS file lock poisoned")?;
                let lease = file.lease.as_ref().ok_or("missing WinCE file lease")?;
                let pinned = lease.mapped.load(std::sync::atomic::Ordering::Relaxed) != 0;
                if pinned && !coordinated && !handles.iter().any(|id| mapping_files.contains(id)) {
                    return Err("checkpoint contains a mapped file".into());
                }

                let path = file.host_path.canonicalize().map_err(io_message)?;
                let (mount_index, relative) = self
                    .mounts
                    .iter()
                    .enumerate()
                    .filter_map(|(index, mount)| {
                        let root = mount.host_dir.canonicalize().ok()?;
                        let relative = path.strip_prefix(&root).ok()?;
                        let distance = root.components().count();
                        Some((index, relative.to_path_buf(), distance))
                    })
                    .max_by_key(|&(index, _, distance)| (distance, std::cmp::Reverse(index)))
                    .map(|(index, relative, _)| (index, relative))
                    .ok_or("open file is outside saved mounts")?;
                let meta = file.file.metadata().map_err(io_message)?;
                let contents = if lease.access & 2 != 0 && !pinned {
                    let size =
                        usize::try_from(meta.len()).map_err(|_| "snapshot file is too large")?;
                    if size > 256 * 1024 * 1024 {
                        return Err("snapshot writable file exceeds 256 MiB".into());
                    }
                    let mut reader = std::fs::File::open(&file.host_path).map_err(io_message)?;
                    let mut bytes = vec![0; size];
                    reader.read_exact(&mut bytes).map_err(io_message)?;
                    Some(bytes)
                } else {
                    None
                };
                let mut saved = FileDescription {
                    identity,
                    handles,
                    mount_index,
                    mount_prefix: self.mounts[mount_index].prefix.clone(),
                    relative,
                    length: meta.len(),
                    modified: meta.modified().ok(),
                    cursor: 0,
                    access: lease.access,
                    share: lease.share,
                    append: file.append,
                    text_mode: file.text_mode,
                    for_mapping: file.for_mapping,
                    contents,
                };
                saved.cursor = if let Some(reader) = file.stdio_reader.as_mut() {
                    reader.stream_position()
                } else {
                    file.file.stream_position()
                }
                .map_err(io_message)?;
                Ok(saved)
            })
            .collect::<Result<Vec<_>, String>>()?;

        let volumes = volumes
            .iter()
            .map(|(&handle, volume)| {
                let mount_index = self
                    .mounts
                    .iter()
                    .position(|mount| {
                        mount.prefix == volume.prefix
                            && mount.host_dir == volume.host_dir
                            && mount.read_only == volume.read_only
                    })
                    .ok_or("saved volume is outside mounted storage")?;
                Ok(VolumeDescription {
                    handle,
                    mount_index,
                    prefix: volume.prefix.clone(),
                    read_only: volume.read_only,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let identities = self
            .open_handles()
            .into_iter()
            .map(|handle| {
                self.export_handle(handle)
                    .map(|object| (handle, object.snapshot_identity()))
                    .ok_or_else(|| "unregistered VFS snapshot handle".to_string())
            })
            .chain(
                private
                    .iter()
                    .map(|(&handle, object)| Ok((handle, object.snapshot_identity()))),
            )
            .collect::<Result<Vec<_>, _>>()?;
        Ok(VfsFileSnapshot {
            camera_waits: self
                .camera_deadlines
                .iter()
                .map(|(&key, &deadline)| (key, deadline.saturating_duration_since(at)))
                .collect(),
            hidden_handles: private.keys().copied().collect(),
            identities,
            volumes,
            ram: self.capture_ram_snapshot(private)?,
            descriptions,
            registration: registration.clone(),
            bluetooth: self.bluetooth.capture_snapshot(at)?,
            devices: devices
                .into_values()
                .chain(
                    htc_sensor_handles
                        .iter()
                        .map(|(&handle, &stream)| DeviceDescription {
                            kind: DeviceKind::Accelerometer(stream),
                            handles: vec![handle],
                            access: 0,
                            share: 0,
                        }),
                )
                .collect(),
            mas_decoders,
            vibration: self.vibration_service().settings(),
            default_dir: self.default_dir.clone(),
            stdio_modes: self.stdio_text_modes,
            next_handle: self.next_handle,
        })
    }

    /// Reopen without creating or truncating any host file. The destination
    /// has fresh handles and mounts configured by normal game startup.
    pub fn restore_file_snapshot(&mut self, snapshot: VfsFileSnapshot) -> Result<(), String> {
        self.restore_file_snapshot_with_mappings(snapshot, &HashSet::new())?;
        self.install_snapshot_files()?.commit();
        Ok(())
    }

    pub(crate) fn restore_file_snapshot_with_mappings(
        &mut self,
        snapshot: VfsFileSnapshot,
        mapping_files: &HashSet<u32>,
    ) -> Result<(), String> {
        let serial = self
            .snapshot_restore_domain
            .as_ref()
            .map(|registry| {
                registry
                    .lock()
                    .map_err(|_| "VFS restore domain poisoned")
                    .map(|registry| registry.restore_gate.clone())
            })
            .transpose()?;
        let _serial = serial
            .as_ref()
            .map(|gate| gate.lock().map_err(|_| "VFS reconstruction gate poisoned"))
            .transpose()?;
        if !self.open_handles().is_empty()
            || snapshot.next_handle < HANDLE_BASE
            || snapshot.next_handle == INVALID_HANDLE_VALUE
        {
            return Err("VFS restore requires an empty handle namespace".into());
        }
        let mut identities = HashMap::new();
        for (handle, identity) in snapshot.identities {
            if identities.insert(handle, identity).is_some() {
                return Err("duplicate saved VFS identity binding".into());
            }
        }
        let mut reused = Vec::new();
        self.restore_ram_snapshot(snapshot.ram)?;
        let mut opened = HashMap::new();
        let mut enrolled = BTreeMap::<String, Vec<Arc<Lease>>>::new();
        let mut ids = snapshot.registration.clone();
        for &id in self.ram_handles.keys() {
            if !ids.insert(id) {
                return Err("duplicate saved RAM handle".into());
            }
        }
        let mut volumes = HashMap::new();
        for record in snapshot.volumes {
            let mount = self
                .mounts
                .get(record.mount_index)
                .filter(|mount| {
                    mount.prefix == record.prefix && mount.read_only == record.read_only
                })
                .ok_or("saved volume mount is missing")?;
            if record.handle < HANDLE_BASE
                || record.handle == INVALID_HANDLE_VALUE
                || !ids.insert(record.handle)
                || !mount.host_dir.is_dir()
            {
                return Err("invalid saved volume handle".into());
            }
            volumes.insert(
                record.handle,
                OpenVolume {
                    prefix: mount.prefix.clone(),
                    host_dir: mount.host_dir.clone(),
                    read_only: mount.read_only,
                },
            );
        }
        // Independent descriptions may read a file which another description
        // has open for writing. Validate them against the captured backing,
        // rather than against its post-checkpoint host contents.
        let rewound: HashSet<_> = snapshot
            .descriptions
            .iter()
            .filter(|record| record.contents.is_some())
            .map(|record| (record.mount_index, record.relative.clone(), record.length))
            .collect();
        for record in snapshot.descriptions {
            if record.identity == 0
                || record.handles.is_empty()
                || record
                    .handles
                    .iter()
                    .any(|&id| id == 0 || id == INVALID_HANDLE_VALUE || !ids.insert(id))
                || record.access > 3
                || record.share & !3 != 0
                || record.relative.as_os_str().is_empty()
                || !record
                    .relative
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)))
            {
                return Err("invalid saved file description".into());
            }
            if record
                .handles
                .iter()
                .any(|id| identities.get(id) != Some(&SnapshotObjectId::Shared(record.identity)))
            {
                return Err("inconsistent saved file identity binding".into());
            }
            let mount = self
                .mounts
                .get(record.mount_index)
                .filter(|mount| mount.prefix == record.mount_prefix)
                .ok_or("saved VFS mount is missing")?;
            if mount.read_only && record.access & 2 != 0 {
                return Err("saved file is now on a read-only mount".into());
            }
            let root = mount.host_dir.canonicalize().map_err(io_message)?;
            let mut rewinds_backing =
                rewound.contains(&(record.mount_index, record.relative.clone(), record.length));
            let path = root
                .join(record.relative)
                .canonicalize()
                .map_err(io_message)?;
            if !path.starts_with(&root) || !path.is_file() {
                return Err("saved file is not contained by its mount".into());
            }

            if let Some(domain) = &self.snapshot_restore_domain {
                rewinds_backing |= domain
                    .lock()
                    .map_err(|_| "VFS restore domain poisoned")?
                    .rewound_paths
                    .contains(&path);
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
                let VfsObjectInner::File(description) = object.0 else {
                    return Err("saved VFS object identity changed type".into());
                };
                let mut open = description.lock().map_err(|_| "VFS file lock poisoned")?;
                let lease = open.lease.as_ref().ok_or("restored file has no lease")?;
                if open.host_path != path
                    || lease.access != record.access
                    || lease.share != record.share
                    || open.append != record.append
                    || open.text_mode != record.text_mode
                    || open.file.stream_position().map_err(io_message)? != record.cursor
                {
                    return Err("inconsistent shared file description".into());
                }
                drop(open);
                for id in record.handles {
                    opened.insert(id, description.clone());
                }
                continue;
            }
            let key = path.to_string_lossy().to_ascii_lowercase();
            let lease = Arc::new(Lease {
                access: record.access,
                share: record.share,
                mapped: Default::default(),
                volume: Self::quota(mount),
            });
            let mut file = OpenOptions::new()
                .read(record.access & 1 != 0 || record.access == 0)
                .write(record.access & 2 != 0)
                .open(&path)
                .map_err(io_message)?;
            let meta = file.metadata().map_err(io_message)?;
            let mapped = record.handles.iter().any(|id| mapping_files.contains(id));
            if record
                .contents
                .as_ref()
                .is_some_and(|bytes| record.access & 2 == 0 || bytes.len() as u64 != record.length)
            {
                return Err("invalid writable snapshot file".into());
            }
            // Owned temporary names were transactionally recreated before
            // opening descriptions. Their contents are the captured version,
            // even for a read-only alias whose old host timestamp has vanished.
            let temporary = self.is_temporary_path(&path)?;
            if record.contents.is_none()
                && !temporary
                && !rewinds_backing
                && (meta.len() != record.length
                    || (!mapped && meta.modified().ok() != record.modified))
            {
                return Err(format!(
                    "saved file changed since checkpoint: {}",
                    path.display()
                ));
            }
            if let Some(bytes) = record.contents {
                self.snapshot_files.0.push((path.clone(), bytes, false));
            }
            file.seek(SeekFrom::Start(record.cursor))
                .map_err(io_message)?;
            let description = Arc::new(Mutex::new(OpenFile {
                host_path: path,
                access: match record.access {
                    2 => Access::Write,
                    3 => Access::ReadWrite,
                    _ => Access::Read,
                },
                file,
                lease: Some(lease.clone()),
                append: record.append,
                text_mode: record.text_mode,
                stdio_reader: None,
                for_mapping: record.for_mapping,
            }));
            if let Some(registry) = &self.snapshot_restore_domain {
                registry
                    .lock()
                    .map_err(|_| "VFS restore domain poisoned")?
                    .objects
                    .insert(
                        SnapshotObjectId::Shared(record.identity),
                        VfsObject(VfsObjectInner::File(description.clone()), None),
                    );
            }
            for id in record.handles {
                opened.insert(id, description.clone());
            }
            enrolled.entry(key).or_default().push(lease);
        }

        let mut decoders = HashMap::new();
        let mut stream_keys = HashSet::new();
        for mut record in snapshot.mas_decoders {
            if record.handles.is_empty()
                || record
                    .handles
                    .iter()
                    .any(|&id| id == 0 || id == INVALID_HANDLE_VALUE || !ids.insert(id))
                || !stream_keys.insert(record.stream_key)
            {
                return Err("invalid saved MAS handle".into());
            }
            let identity = identities
                .get(&record.handles[0])
                .ok_or("missing MAS identity binding")?;
            let restored = self
                .snapshot_restore_domain
                .as_ref()
                .map(|registry| {
                    registry
                        .lock()
                        .map_err(|_| "VFS restore domain poisoned")
                        .map(|registry| registry.object(identity))
                })
                .transpose()?
                .flatten();
            if let Some(object) = restored {
                let VfsObjectInner::Decoder(decoder) = &object.0 else {
                    return Err("MAS object changed type".into());
                };
                if decoder
                    .lock()
                    .map_err(|_| "MAS decoder lock poisoned")?
                    .stream_key
                    != record.stream_key
                {
                    return Err("shared MAS stream changed identity".into());
                }
                reused.extend(record.handles.into_iter().map(|id| (id, object.clone())));
                continue;
            }
            let handles = std::mem::take(&mut record.handles);
            let decoder = Arc::new(Mutex::new(record.restore()?));
            for id in handles {
                decoders.insert(id, decoder.clone());
            }
        }

        self.bluetooth.restore_snapshot(snapshot.bluetooth)?;
        let mut drivers = HashMap::new();
        let mut bluetooth_handles = HashMap::new();
        let mut vibration_handles = HashMap::new();
        let mut gps_handles = HashMap::new();
        let mut camera_handles = HashMap::new();
        let mut accelerometers = HashMap::new();
        for record in snapshot.devices {
            if record.handles.is_empty()
                || record.access > 3
                || record.share & !3 != 0
                || record
                    .handles
                    .iter()
                    .any(|&id| id == 0 || id == INVALID_HANDLE_VALUE || !ids.insert(id))
            {
                return Err("invalid saved VFS device description".into());
            }
            let identity = identities
                .get(&record.handles[0])
                .ok_or("missing device identity binding")?;
            let restored = self
                .snapshot_restore_domain
                .as_ref()
                .map(|registry| {
                    registry
                        .lock()
                        .map_err(|_| "VFS restore domain poisoned")
                        .map(|registry| registry.object(identity))
                })
                .transpose()?
                .flatten();
            if let Some(object) = restored {
                let contract = match (&record.kind, &object.0) {
                    (DeviceKind::Bluetooth, VfsObjectInner::BluetoothDriver(open)) => {
                        Some((open._lease.access, open._lease.share))
                    }
                    (
                        DeviceKind::BluetoothPort {
                            index,
                            registration,
                            ..
                        },
                        VfsObjectInner::Bluetooth(open),
                    ) if *index == open.index && *registration == open.registration => {
                        Some((open._lease.access, open._lease.share))
                    }
                    (DeviceKind::Vibration, VfsObjectInner::Vibration(open)) => {
                        Some((open._lease.access, open._lease.share))
                    }
                    (DeviceKind::Gps { .. }, VfsObjectInner::Gps(open)) => {
                        Some((open._lease.access, open._lease.share))
                    }
                    (DeviceKind::Camera(_), VfsObjectInner::Camera(open)) => {
                        Some((open._lease.access, open._lease.share))
                    }
                    (DeviceKind::Accelerometer(a), VfsObjectInner::HtcSensor(b)) if a == b => {
                        Some((0, 0))
                    }
                    _ => None,
                };
                if contract != Some((record.access, record.share)) {
                    return Err("inconsistent shared device description".into());
                }
                reused.extend(record.handles.into_iter().map(|id| (id, object.clone())));
                continue;
            }
            let lease = || {
                Arc::new(Lease {
                    access: record.access,
                    share: record.share,
                    mapped: Default::default(),
                    volume: None,
                })
            };
            let (key, lease) = match record.kind {
                DeviceKind::Bluetooth => {
                    let lease = lease();
                    let driver = Arc::new(BluetoothDriverOpen {
                        service: self.bluetooth_service(),
                        access: record.access,
                        _lease: lease.clone(),
                    });
                    for id in record.handles {
                        drivers.insert(id, driver.clone());
                    }
                    (BLUETOOTH_DRIVER_LEASE.to_owned(), lease)
                }
                DeviceKind::BluetoothPort {
                    index,
                    registration,
                    port,
                } => {
                    if !(1..=9).contains(&index) || registration < 0xb7000000 {
                        return Err("invalid saved Bluetooth port description".into());
                    }
                    let lease = lease();
                    let open = Arc::new(BluetoothOpen {
                        index,
                        registration,
                        port: self.bluetooth.service.restore_port(port)?,
                        access: record.access,
                        _lease: lease.clone(),
                    });
                    for id in record.handles {
                        bluetooth_handles.insert(id, open.clone());
                    }
                    (format!("bluetooth:COM{index}:{registration}"), lease)
                }
                DeviceKind::Vibration => {
                    let lease = lease();
                    let driver = Arc::new(VibrationOpen {
                        service: self.vibration_service(),
                        access: record.access,
                        _lease: lease.clone(),
                    });
                    for id in record.handles {
                        vibration_handles.insert(id, driver.clone());
                    }
                    (VIBRATION_DRIVER_LEASE.to_owned(), lease)
                }
                DeviceKind::Gps { fixed } => {
                    let lease = lease();
                    let device = self
                        .gps_service()
                        .open()
                        .map_err(|error| format!("cannot reopen GPS1: WinCE error {error}"))?;
                    device
                        .lock()
                        .map_err(|_| "GPS device lock poisoned")?
                        .restore_fix_status(fixed);
                    let open = Arc::new(GpsOpen {
                        device,
                        access: record.access,
                        _lease: lease.clone(),
                    });
                    for id in record.handles {
                        gps_handles.insert(id, open.clone());
                    }
                    (GPS_DRIVER_LEASE.to_owned(), lease)
                }
                DeviceKind::Camera(saved) => {
                    let lease = lease();
                    let device = self
                        .camera_service()
                        .open()
                        .map_err(|error| format!("cannot reopen CAM1: WinCE error {error}"))?;
                    device
                        .lock()
                        .map_err(|_| "camera device lock poisoned")?
                        .restore_snapshot(saved)
                        .map_err(|error| format!("cannot restore camera: WinCE error {error}"))?;
                    let open = Arc::new(CameraOpen {
                        device,
                        access: record.access,
                        _lease: lease.clone(),
                    });
                    for id in record.handles {
                        camera_handles.insert(id, open.clone());
                    }
                    (CAMERA_DRIVER_LEASE.to_owned(), lease)
                }
                DeviceKind::Accelerometer(stream) => {
                    if record.access != 0 || record.share != 0 {
                        return Err("invalid saved accelerometer handle".into());
                    }
                    accelerometers.extend(record.handles.into_iter().map(|id| (id, stream)));
                    continue;
                }
            };
            enrolled.entry(key.into()).or_default().push(lease);
        }

        let mut shared = self
            .shared
            .0
            .lock()
            .map_err(|_| "VFS device lock poisoned")?;
        for (key, leases) in &enrolled {
            busy(&mut shared, key);
            let conflict = |a: &Lease, b: &Lease| {
                a.access & !b.share != 0
                    || b.access & !a.share != 0
                    || (key == GPS_DRIVER_LEASE && a.access & b.access & 2 != 0)
            };
            for (index, lease) in leases.iter().enumerate() {
                if leases[..index].iter().any(|other| conflict(lease, other))
                    || shared
                        .leases
                        .get(key)
                        .into_iter()
                        .flatten()
                        .filter_map(Weak::upgrade)
                        .any(|other| conflict(lease, &other))
                {
                    return Err("saved file share contract conflicts with an open handle".into());
                }
            }
        }

        self.volumes = volumes;
        shared
            .vibration
            .restore_settings(snapshot.vibration)
            .map_err(|error| format!("cannot restore vibration settings: WinCE error {error}"))?;
        for (key, leases) in enrolled {
            shared
                .leases
                .entry(key)
                .or_default()
                .extend(leases.iter().map(Arc::downgrade));
        }
        if let Some(max_key) = stream_keys.into_iter().max() {
            NEXT_MAS_STREAM.fetch_max(max_key + 1, std::sync::atomic::Ordering::Relaxed);
        }
        self.decoders = decoders;
        self.handles = opened;
        self.bluetooth_driver_handles = drivers;
        self.bluetooth_handles = bluetooth_handles;
        self.vibration_handles = vibration_handles;
        self.gps_handles = gps_handles;
        self.camera_handles = camera_handles;
        self.htc_sensor_handles = accelerometers;
        self.registration = snapshot.registration;
        self.default_dir = snapshot.default_dir;
        self.stdio_text_modes = snapshot.stdio_modes;
        self.next_handle = snapshot.next_handle;
        self.snapshot_hidden_handles = snapshot.hidden_handles;
        let mut waits = HashMap::new();
        for (key, delay) in snapshot.camera_waits {
            if delay > std::time::Duration::from_millis(u32::MAX as u64) || key.1 == 0 || key.2 == 0
            {
                return Err("invalid saved camera wait".into());
            }
            let deadline = std::time::Instant::now()
                .checked_add(delay)
                .ok_or("invalid saved camera deadline")?;
            if waits.insert(key, deadline).is_some() {
                return Err("duplicate saved camera wait".into());
            }
        }
        self.camera_deadlines = waits;

        drop(shared);
        for (handle, object) in reused {
            if !self.import_handle(handle, object) {
                return Err("could not attach shared VFS object".into());
            }
        }
        if let Some(registry) = &self.snapshot_restore_domain {
            let mut registry = registry.lock().map_err(|_| "VFS restore domain poisoned")?;
            for (handle, identity) in identities {
                let object = self
                    .export_handle(handle)
                    .ok_or("saved VFS identity has no handle")?;
                if let Some(existing) = registry.objects.get(&identity) {
                    if existing.snapshot_identity() != object.snapshot_identity() {
                        return Err("VFS identity was reconstructed twice".into());
                    }
                } else {
                    registry.objects.insert(identity, object);
                }
            }
        }
        Ok(())
    }
}

fn io_message(error: std::io::Error) -> String {
    format!("VFS checkpoint file I/O: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_preserves_mas_alias_and_partial_frame() {
        let mut vfs = Vfs::new();
        let decoder = vfs.open("MAS1:", Access::ReadWrite, false).unwrap();
        let alias = decoder + 16;
        assert!(vfs.duplicate_file(decoder, alias));
        let mut frame = [0u8; 417];
        frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0]);
        let data = frame.repeat(4);
        let cut = 417 + 47;
        vfs.feed_mp3_decoder_data(decoder, &data[..cut]).unwrap();

        let mut restored = Vfs::new();
        restored
            .restore_file_snapshot(vfs.capture_file_snapshot().unwrap())
            .unwrap();
        assert_eq!(restored.mp3_stream_key(alias), vfs.mp3_stream_key(decoder));
        assert_eq!(restored.mp3_decoder_handles().len(), 1);
        vfs.feed_mp3_decoder_data(decoder, &data[cut..]).unwrap();
        restored.feed_mp3_decoder_data(alias, &data[cut..]).unwrap();
        assert_eq!(
            vfs.take_mp3_decoder_pcm(decoder),
            restored.take_mp3_decoder_pcm(decoder)
        );
    }

    #[test]
    fn snapshot_preserves_both_accelerometer_streams_and_aliases() {
        let mut source = Vfs::new();
        let sen0 = source.open_htc_sensor(AccelerometerStream::Sen0).unwrap();
        let acc1 = source.open_htc_sensor(AccelerometerStream::Acc1).unwrap();
        let alias = acc1 + 16;
        assert!(source.duplicate_file(acc1, alias));

        let mut restored = Vfs::new();
        restored
            .restore_file_snapshot(source.capture_file_snapshot().unwrap())
            .unwrap();
        assert_eq!(
            restored.htc_sensor_stream(sen0),
            Some(AccelerometerStream::Sen0)
        );
        assert_eq!(
            restored.htc_sensor_stream(acc1),
            Some(AccelerometerStream::Acc1)
        );
        assert!(restored.close(acc1));
        assert_eq!(
            restored.htc_sensor_stream(alias),
            Some(AccelerometerStream::Acc1)
        );
    }

    #[test]
    fn writable_snapshot_rewinds_cursor_and_keeps_aliases() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("log");
        std::fs::write(&path, b"before").unwrap();
        let mut source = Vfs::new();
        source.mount_save_dir(r"\Game\", directory.path());
        let file = source
            .open_file(r"\Game\log", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        let alias = file + 20;
        assert!(source.duplicate_file(file, alias));
        source.seek(file, 3, SeekKind::Begin);
        let saved = source.capture_file_snapshot().unwrap();
        source.seek(file, 0, SeekKind::End);
        source.write(alias, b" later").unwrap();
        drop(source);

        let mut restored = Vfs::new();
        restored.mount_save_dir(r"\Game\", directory.path());
        restored.restore_file_snapshot(saved).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"before");
        let mut bytes = [0; 3];
        assert_eq!(restored.read(alias, &mut bytes), Some(3));
        assert_eq!(&bytes, b"ore");
        assert_eq!(restored.seek(file, 0, SeekKind::Current), Some(6));
    }

    #[test]
    fn writable_snapshot_restores_an_independent_reader_of_the_same_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shared");
        std::fs::write(&path, b"before").unwrap();
        let mut source = Vfs::new();
        source.mount_save_dir(r"\Game\", directory.path());
        let writer = source
            .open_file(r"\Game\shared", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        let reader = source
            .open_file(r"\Game\shared", 1, 3, 3, false, 0)
            .unwrap()
            .handle;
        let saved = source.capture_file_snapshot().unwrap();
        source.write(writer, b"modified later").unwrap();
        drop(source);
        let mut restored = Vfs::new();
        restored.mount_save_dir(r"\Game\", directory.path());
        restored.restore_file_snapshot(saved).unwrap();
        let mut bytes = [0; 6];
        assert_eq!(restored.read(reader, &mut bytes), Some(6));
        assert_eq!(&bytes, b"before");
    }

    #[test]
    fn append_snapshot_restores_saved_end_and_append_semantics() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("log");
        std::fs::write(&path, b"before").unwrap();
        let mut source = Vfs::new();
        source.mount_save_dir(r"\Game\", directory.path());
        let file = source
            .open_file(r"\Game\log", 2, 3, 3, true, 0)
            .unwrap()
            .handle;
        let saved = source.capture_file_snapshot().unwrap();
        source.write(file, b" later").unwrap();
        drop(source);
        let mut restored = Vfs::new();
        restored.mount_save_dir(r"\Game\", directory.path());
        restored.restore_file_snapshot(saved).unwrap();
        restored.seek(file, 0, SeekKind::Begin);
        restored.write(file, b" resumed").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"before resumed");
    }

    #[test]
    fn writable_snapshot_validation_failure_does_not_rewind_host_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("log");
        std::fs::write(&path, b"before").unwrap();
        let mut source = Vfs::new();
        source.mount_save_dir(r"\Game\", directory.path());
        source.open_file(r"\Game\log", 3, 3, 3, false, 0).unwrap();
        let mut saved = source.capture_file_snapshot().unwrap();
        saved.next_handle = 0;
        std::fs::write(&path, b"later").unwrap();
        let mut restored = Vfs::new();
        restored.mount_save_dir(r"\Game\", directory.path());
        assert!(restored.restore_file_snapshot(saved).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"later");
    }

    #[test]
    fn pending_file_install_rolls_back_until_the_session_commits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config");
        std::fs::write(&path, b"checkpoint").unwrap();
        let mut source = Vfs::new();
        source.mount_save_dir(r"\Game\", directory.path());
        source
            .open_file(r"\Game\config", 3, 3, 3, false, 0)
            .unwrap();
        let saved = source.capture_file_snapshot().unwrap();
        drop(source);
        std::fs::write(&path, b"newer data after F5").unwrap();
        let mut restored = Vfs::new();
        restored.mount_save_dir(r"\Game\", directory.path());
        restored
            .restore_file_snapshot_with_mappings(saved, &HashSet::new())
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"newer data after F5");
        {
            let _uncommitted = restored.install_snapshot_files().unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"checkpoint");
        }
        assert_eq!(std::fs::read(path).unwrap(), b"newer data after F5");
    }

    #[test]
    fn device_checkpoint_reuses_transferred_disk_and_ram_descriptions() {
        use crate::memory_division::MemoryDivision;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("disk"), b"disk bytes").unwrap();
        let ram = MemoryDivision::new(128 * 4096, 64).unwrap();
        let mut parent = Vfs::new();
        parent.attach_ram(Some(ram.clone()));
        parent.mount_save_dir(r"\Disk\", directory.path());
        let disk = parent
            .open_file(r"\Disk\disk", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        assert!(parent.create_dir(r"\RAM"));
        let file = parent
            .open_file(r"\RAM\file", 3, 3, 2, false, 0)
            .unwrap()
            .handle;
        parent.write(file, b"RAM bytes").unwrap();
        parent.seek(file, 0, SeekKind::Begin);
        let mut child = Vfs::new();
        child.attach_ram(Some(ram.clone()));
        child.attach_shared_context(parent.shared_context());
        child.mount_save_dir(r"\Disk\", directory.path());
        let disk_alias = disk + 100;
        let ram_alias = file + 100;
        assert!(child.import_handle(disk_alias, parent.export_handle(disk).unwrap()));
        assert!(child.import_handle(ram_alias, parent.export_handle(file).unwrap()));
        let parent_save = parent.capture_file_snapshot().unwrap();
        let child_save = child.capture_file_snapshot().unwrap();
        let used = ram.snapshot().store_used;
        drop(parent);
        drop(child);
        drop(ram);
        let ram = MemoryDivision::new(128 * 4096, 64).unwrap();
        let domain = Arc::new(Mutex::new(SnapshotRestoreDomain::default()));
        let mut parent = Vfs::new();
        parent.attach_ram(Some(ram.clone()));
        parent.mount_save_dir(r"\Disk\", directory.path());
        parent.attach_snapshot_restore_domain(Some(domain.clone()));
        let mut child = Vfs::new();
        child.attach_ram(Some(ram.clone()));
        child.mount_save_dir(r"\Disk\", directory.path());
        child.attach_shared_context(parent.shared_context());
        child.attach_snapshot_restore_domain(Some(domain));
        parent.restore_file_snapshot(parent_save).unwrap();
        child.restore_file_snapshot(child_save).unwrap();
        assert_eq!(ram.snapshot().store_used, used);
        let mut data = [0; 4];
        child.read(disk_alias, &mut data).unwrap();
        assert_eq!(&data, b"disk");
        assert_eq!(parent.seek(disk, 0, SeekKind::Current), Some(4));
        child.write(ram_alias, b"new").unwrap();
        assert_eq!(parent.seek(file, 0, SeekKind::Current), Some(3));
        parent.seek(file, 0, SeekKind::Begin);
        parent.read(file, &mut data).unwrap();
        assert_eq!(&data, b"new ");
        assert!(parent.close(file));
        assert!(child.close(ram_alias));
        let reopened = parent
            .open_file(r"\RAM\file", 1, 3, 3, false, 0)
            .unwrap()
            .handle;
        parent.read(reopened, &mut data).unwrap();
        assert_eq!(&data, b"new ");
    }

    #[test]
    fn ram_snapshot_preserves_store_aliases_cursor_and_quota() {
        use crate::memory_division::MemoryDivision;
        let division = MemoryDivision::new(128 * 4096, 64).unwrap();
        let mut source = Vfs::new();
        source.attach_ram(Some(division.clone()));
        assert!(source.create_dir(r"\Data"));
        let file = source
            .open_file(r"\Data\object", 3, 3, 2, false, 0)
            .unwrap()
            .handle;
        source.write(file, &vec![0x37; 4097]).unwrap();
        source.seek(file, 2, SeekKind::Begin);
        let alias = file + 20;
        assert!(source.duplicate_file(file, alias));
        let saved = source.capture_file_snapshot().unwrap();
        let used = division.snapshot().store_used;
        source.close(file);
        source.close(alias);
        let new_division = MemoryDivision::new(128 * 4096, 64).unwrap();
        let mut restored = Vfs::new();
        restored.attach_ram(Some(new_division.clone()));
        restored.restore_file_snapshot(saved).unwrap();
        assert_eq!(new_division.snapshot().store_used, used);
        restored.write(alias, b"RAM").unwrap();
        assert_eq!(restored.seek(file, 0, SeekKind::Current), Some(5));
        restored.seek(file, 0, SeekKind::Begin);
        let mut bytes = [0; 5];
        restored.read(file, &mut bytes).unwrap();
        assert_eq!(&bytes, &[0x37, 0x37, b'R', b'A', b'M']);
        assert_eq!(restored.ram_file_size(r"\Data\object"), Some(4097));
    }

    struct TestCamera;
    impl crate::camera::Backend for TestCamera {
        fn start(&self) -> crate::camera::Result<Box<dyn crate::camera::Capture>> {
            Ok(Box::new(Self))
        }
    }
    impl crate::camera::Capture for TestCamera {
        fn latest(&mut self) -> crate::camera::Result<Option<Arc<crate::camera::Frame>>> {
            Ok(None)
        }
    }

    #[test]
    fn snapshot_reopens_gps_and_camera_using_destination_services() {
        fn configure(vfs: &Vfs, position: (f64, f64)) {
            let gps = vfs.gps_service();
            gps.set_allowed(true);
            gps.set_fixed_position(Some(position)).unwrap();
            let camera = crate::camera::Service::with_backend(Arc::new(TestCamera));
            camera.set_allowed(true);
            vfs.set_camera_service(camera);
        }

        let mut source = Vfs::new();
        configure(&source, (48., 2.));
        let gps = source.open_file("GPS1:", 3, 3, 3, false, 0).unwrap().handle;
        let camera = source.open_file("CAM1:", 3, 3, 3, false, 0).unwrap().handle;
        let camera_alias = camera + 16;
        assert!(source.duplicate_file(camera, camera_alias));
        let source_gps = source.gps_open(gps).unwrap();
        source_gps.device.lock().unwrap().packet().unwrap();
        {
            let open = source.camera_open(camera).unwrap();
            let mut device = open.device.lock().unwrap();
            device.set_format([640, 480, 192, 148]).unwrap();
            device.start().unwrap();
            device.consumed(42);
        }

        let mut restored = Vfs::new();
        configure(&restored, (50., 4.));
        restored
            .restore_file_snapshot(source.capture_file_snapshot().unwrap())
            .unwrap();
        assert_eq!(restored.open_file("GPS1:", 2, 3, 3, false, 0), Err(32));
        let restored_gps = restored.gps_open(gps).unwrap();
        assert!(restored_gps.device.lock().unwrap().has_had_fix());
        let packet = restored_gps.device.lock().unwrap().packet().unwrap();
        assert_eq!(
            i32::from_le_bytes(packet[21..25].try_into().unwrap()),
            500_000_000
        );
        assert_eq!(
            i32::from_le_bytes(packet[25..29].try_into().unwrap()),
            40_000_000
        );
        let first = restored.camera_open(camera).unwrap();
        let duplicate = restored.camera_open(camera_alias).unwrap();
        assert!(Arc::ptr_eq(&first, &duplicate));
        let mut device = first.device.lock().unwrap();
        assert_eq!(device.format, [640, 480, 192, 148]);
        assert_eq!(device.frame_count, 1);
        assert!(device.latest().unwrap().is_none());
    }

    #[test]
    fn snapshot_rebases_volume_handles_and_pending_camera_deadlines() {
        let old = tempfile::tempdir().unwrap();
        let current = tempfile::tempdir().unwrap();
        let mut source = Vfs::new();
        source.mount_save_dir(r"\Data\", old.path());
        let volume = source
            .open_file(r"\Data\Vol:", 0, 3, 3, false, 0)
            .unwrap()
            .handle;
        let alias = volume + 16;
        assert!(source.duplicate_file(volume, alias));
        let key = (2, 0x70000020, 0x100080);
        source.camera_deadlines.insert(
            key,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        );
        let saved = source.capture_file_snapshot().unwrap();
        let mut restored = Vfs::new();
        restored.mount_save_dir(r"\Data\", current.path());
        restored.restore_file_snapshot(saved).unwrap();
        for handle in [volume, alias] {
            assert_eq!(restored.volumes[&handle].host_dir, current.path());
            assert!(!restored.volumes[&handle].read_only);
        }
        let remaining =
            restored.camera_deadlines[&key].saturating_duration_since(std::time::Instant::now());
        assert!(remaining > std::time::Duration::from_secs(4));
        assert!(remaining <= std::time::Duration::from_secs(5));
        assert!(restored.close(volume));
        assert!(restored.volumes.contains_key(&alias));
    }

    #[test]
    fn snapshot_reopens_vibration_driver_with_current_settings() {
        let mut source = Vfs::new();
        let driver = source.open_file("VIB1:", 2, 0, 3, false, 0).unwrap().handle;
        let duplicate = driver + 16;
        assert!(source.duplicate_file(driver, duplicate));
        let settings = crate::vibration::Settings {
            mode: 2,
            total: 1000,
            on: 500,
            off: 500,
            meta_on: 1,
            meta_off: 0,
        };
        source.vibration_service().set(settings).unwrap();

        let snapshot = source.capture_file_snapshot().unwrap();
        let mut restored = Vfs::new();
        restored.restore_file_snapshot(snapshot).unwrap();
        let first = restored.vibration_open(driver).unwrap();
        let second = restored.vibration_open(duplicate).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.access, 2);
        assert_eq!(first.service.settings(), settings);
        assert_eq!(restored.open_file("VIB1:", 2, 0, 3, false, 0), Err(32));
    }

    #[test]
    fn snapshot_reopens_bluetooth_driver_and_preserves_its_exclusive_lease() {
        let mut source = Vfs::new();
        let driver = source.open_file("BTH1:", 0, 0, 3, false, 0).unwrap().handle;
        let duplicate = driver + 16;
        assert!(source.duplicate_file(driver, duplicate));
        source.bluetooth.startups = 1;
        let snapshot = source.capture_file_snapshot().unwrap();

        let mut restored = Vfs::new();
        restored.restore_file_snapshot(snapshot).unwrap();
        let first = restored.bluetooth_driver_open(driver).unwrap();
        let second = restored.bluetooth_driver_open(duplicate).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(restored.bluetooth.startups, 1);
        assert!(!first.service.is_enabled());
        drop(first);
        drop(second);
        assert!(restored.close(driver));
        assert_eq!(restored.open_file("BTH1:", 1, 3, 3, false, 0), Err(32));
        assert!(restored.close(duplicate));
        assert!(restored.open_file("BTH1:", 1, 3, 3, false, 0).is_ok());
    }

    #[test]
    fn snapshot_reopens_under_new_mount_and_preserves_crt_alias_cursor() {
        let original = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let path = original.path().join("data.dat");
        std::fs::write(&path, b"123456").unwrap();
        std::fs::copy(&path, target.path().join("data.dat")).unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_read_only(r"\SD Card\", original.path());
        let input = vfs
            .open_file(r"\SD Card\data.dat", 1, 1, 3, false, 0)
            .unwrap()
            .handle;
        assert!(vfs.duplicate_file(input, input + 16));
        let mut buf = [0; 2];
        assert_eq!(vfs.read_stdio(input, &mut buf), Some(2));
        assert_eq!(&buf, b"12");
        assert!(vfs.has_stdio_read_ahead(input));

        let mut saved = vfs.capture_file_snapshot().unwrap();
        assert!(vfs.has_stdio_read_ahead(input));
        // Host copies need not retain the modification timestamp. For this
        // fixture use the same version explicitly on the relocated mount.
        saved.descriptions[0].modified = target
            .path()
            .join("data.dat")
            .metadata()
            .unwrap()
            .modified()
            .ok();
        let serialized = serde_json::to_vec(&saved).unwrap();
        drop(vfs);

        let mut restored = Vfs::new();
        restored.mount_read_only(r"\SD Card\", target.path());
        restored
            .restore_file_snapshot(serde_json::from_slice(&serialized).unwrap())
            .unwrap();
        assert_eq!(restored.read(input + 16, &mut buf), Some(2));
        assert_eq!(&buf, b"34");
        assert_eq!(restored.read(input, &mut buf), Some(2));
        assert_eq!(&buf, b"56");
    }

    #[test]
    fn stale_file_snapshot_leaves_target_namespace_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("save.bin");
        std::fs::write(&file, b"initial").unwrap();
        let mut original = Vfs::new();
        original.mount(r"\Flash Disk\", dir.path());
        original
            .open_file(r"\Flash Disk\save.bin", 1, 3, 3, false, 0)
            .unwrap();
        let snapshot = original.capture_file_snapshot().unwrap();
        drop(original);

        std::fs::write(&file, b"new content").unwrap();
        let mut restored = Vfs::new();
        restored.mount(r"\Flash Disk\", dir.path());
        assert!(restored.restore_file_snapshot(snapshot).is_err());
        assert!(restored.open_handles().is_empty());
        assert_eq!(std::fs::read(file).unwrap(), b"new content");
    }
}

/// Writes remain deferred until CPU, graphics and all resource validation finish.
#[derive(Default)]
pub struct SnapshotFileWrites(pub(crate) Vec<(PathBuf, Vec<u8>, bool)>);

pub struct SnapshotFileCommit {
    backups: Vec<(std::fs::File, Vec<u8>)>,
    applied: usize,
    created: Vec<PathBuf>,
    temporary_names: Option<(VfsShared, HashSet<String>)>,
}

impl SnapshotFileCommit {
    pub(super) fn track_temporary_names(&mut self, shared: VfsShared, names: HashSet<String>) {
        self.temporary_names = Some((shared, names));
    }
    pub fn commit(mut self) {
        self.applied = 0;
        self.backups.clear();
        self.created.clear();
        self.temporary_names = None;
    }
}

impl Drop for SnapshotFileCommit {
    fn drop(&mut self) {
        if let Some((shared, names)) = self.temporary_names.take() {
            match shared.0.lock() {
                Ok(mut state) => state.temporary_files.retain(|name| !names.contains(name)),
                Err(_) => log::error!("snapshot temporary-name rollback lock poisoned"),
            }
        }
        for (file, bytes) in self.backups.iter_mut().take(self.applied).rev() {
            if let Err(error) = replace_file_bytes(file, bytes) {
                log::error!("snapshot file rollback failed: {error}");
            }
        }
        // Windows will not unlink a newly created database while its backup
        // file handle is open. Close the handles before removing new entries.
        self.backups.clear();
        for path in self.created.drain(..).rev() {
            if let Err(error) = std::fs::remove_file(&path) {
                log::error!("snapshot file creation rollback failed: {error}");
            }
        }
    }
}

fn replace_file_bytes(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.write_all(bytes)?;
    file.set_len(bytes.len() as u64)?;
    file.sync_data()
}

impl Vfs {
    /// Internal subsystem backing, resolved from the current launch rather than
    /// from a serialized native path. Installation shares the normal rollback.
    pub(crate) fn queue_snapshot_metadata_file(&mut self, path: PathBuf, bytes: Vec<u8>) {
        self.snapshot_files.0.push((path, bytes, true));
    }
    pub fn install_snapshot_files(&mut self) -> Result<SnapshotFileCommit, String> {
        let writes = std::mem::take(&mut self.snapshot_files);
        let mut transaction = SnapshotFileCommit {
            backups: Vec::new(),
            applied: 0,
            created: Vec::new(),
            temporary_names: None,
        };
        let mut prepared = BTreeMap::new();
        for (path, bytes, create) in writes.0 {
            match prepared.entry(path) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert((bytes, create));
                }
                std::collections::btree_map::Entry::Occupied(entry) if entry.get().0 != bytes => {
                    return Err("inconsistent snapshot contents for one file".into());
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().1 &= create;
                }
            }
        }
        // Read backups before the first mutation so open/read failures cannot
        // leave a partially applied restore.
        let mut pending = Vec::new();
        for (path, (bytes, create)) in prepared {
            let mut file = match OpenOptions::new().read(true).write(true).open(&path) {
                Ok(file) => file,
                Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                    let file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(io_message)?;
                    transaction.created.push(path);
                    file
                }
                Err(error) => return Err(io_message(error)),
            };
            let mut backup = Vec::new();
            file.read_to_end(&mut backup).map_err(io_message)?;
            transaction.backups.push((file, backup));
            pending.push(bytes);
        }
        for ((file, _), bytes) in transaction.backups.iter_mut().zip(pending) {
            transaction.applied += 1;
            replace_file_bytes(file, &bytes).map_err(io_message)?;
        }
        self.shared
            .0
            .lock()
            .map_err(|_| "VFS device lock poisoned")?
            .volume_usage
            .clear();
        Ok(transaction)
    }
}
