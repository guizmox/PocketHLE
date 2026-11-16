//! Persistent host file descriptions for the WinCE VFS.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::time::SystemTime;

#[derive(Serialize, Deserialize)]
pub struct VfsFileSnapshot {
    descriptions: Vec<FileDescription>,
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
struct DeviceDescription {
    kind: DeviceKind,
    handles: Vec<u32>,
    access: u32,
    share: u32,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum DeviceKind {
    Bluetooth,
    Vibration,
    Gps { fixed: bool },
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
}

impl Vfs {
    /// Save a single process's regular file handles. Their backing files must
    /// remain mounted. Description identity preserves duplicate-handle cursors.
    pub fn capture_file_snapshot(&self) -> Result<VfsFileSnapshot, String> {
        for (present, kind) in [
            (!self.section_files.is_empty(), "file mapping"),
            (!self.ram_handles.is_empty(), "RAM file handle"),
            (!self.volumes.is_empty(), "Vol: handle"),
            (
                !self.camera_deadlines.is_empty(),
                "in-flight camera request",
            ),
            (
                !self.bluetooth_handles.is_empty(),
                "Bluetooth COM port handle",
            ),
        ] {
            if present {
                return Err(format!("checkpoint contains an unsupported {kind}"));
            }
        }

        let mut decoder_aliases = HashMap::<_, Vec<_>>::new();
        for (&id, decoder) in &self.decoders {
            decoder_aliases
                .entry(Arc::as_ptr(decoder))
                .or_default()
                .push(id);
        }
        let mas_decoders = decoder_aliases
            .into_values()
            .map(|handles| {
                let decoder = self.decoders[&handles[0]]
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
        for (&id, open) in &self.bluetooth_driver_handles {
            devices_by_lease(id, &open._lease, DeviceKind::Bluetooth);
        }
        for (&id, open) in &self.vibration_handles {
            devices_by_lease(id, &open._lease, DeviceKind::Vibration);
        }
        for (&id, open) in &self.gps_handles {
            let device = open.device.lock().map_err(|_| "GPS device lock poisoned")?;
            devices_by_lease(
                id,
                &open._lease,
                DeviceKind::Gps {
                    fixed: device.has_had_fix(),
                },
            );
        }
        for (&id, open) in &self.camera_handles {
            let device = open
                .device
                .lock()
                .map_err(|_| "camera device lock poisoned")?;
            devices_by_lease(
                id,
                &open._lease,
                DeviceKind::Camera(device.capture_snapshot()),
            );
        }
        let mut ids: HashMap<*const Mutex<OpenFile>, Vec<u32>> = HashMap::new();
        for (&handle, description) in &self.handles {
            ids.entry(Arc::as_ptr(description))
                .or_default()
                .push(handle);
        }
        let descriptions = ids
            .into_values()
            .map(|mut handles| {
                handles.sort_unstable();
                let mut file = self.handles[&handles[0]]
                    .lock()
                    .map_err(|_| "VFS file lock poisoned")?;
                let lease = file.lease.as_ref().ok_or("missing WinCE file lease")?;
                if file.for_mapping || lease.mapped.load(std::sync::atomic::Ordering::Relaxed) != 0
                {
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
                let mut saved = FileDescription {
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

        Ok(VfsFileSnapshot {
            descriptions,
            registration: self.registration.clone(),
            bluetooth: self.bluetooth.capture_snapshot()?,
            devices: devices
                .into_values()
                .chain(
                    self.htc_sensor_handles
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
        if !self.open_handles().is_empty()
            || snapshot.next_handle < HANDLE_BASE
            || snapshot.next_handle == INVALID_HANDLE_VALUE
        {
            return Err("VFS restore requires an empty handle namespace".into());
        }
        let mut opened = HashMap::new();
        let mut enrolled = BTreeMap::<String, Vec<Arc<Lease>>>::new();
        let mut ids = snapshot.registration.clone();
        for record in snapshot.descriptions {
            if record.handles.is_empty()
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
            let mount = self
                .mounts
                .get(record.mount_index)
                .filter(|mount| mount.prefix == record.mount_prefix)
                .ok_or("saved VFS mount is missing")?;
            if mount.read_only && record.access & 2 != 0 {
                return Err("saved file is now on a read-only mount".into());
            }
            let root = mount.host_dir.canonicalize().map_err(io_message)?;
            let path = root
                .join(record.relative)
                .canonicalize()
                .map_err(io_message)?;
            if !path.starts_with(&root) || !path.is_file() {
                return Err("saved file is not contained by its mount".into());
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
            if meta.len() != record.length || meta.modified().ok() != record.modified {
                return Err(format!(
                    "saved file changed since checkpoint: {}",
                    path.display()
                ));
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
                for_mapping: false,
            }));
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
            let handles = std::mem::take(&mut record.handles);
            let decoder = Arc::new(Mutex::new(record.restore()?));
            for id in handles {
                decoders.insert(id, decoder.clone());
            }
        }

        let mut drivers = HashMap::new();
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
                    (BLUETOOTH_DRIVER_LEASE, lease)
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
                    (VIBRATION_DRIVER_LEASE, lease)
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
                    (GPS_DRIVER_LEASE, lease)
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
                    (CAMERA_DRIVER_LEASE, lease)
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
        self.bluetooth.restore_snapshot(snapshot.bluetooth)?;
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
        self.vibration_handles = vibration_handles;
        self.gps_handles = gps_handles;
        self.camera_handles = camera_handles;
        self.htc_sensor_handles = accelerometers;
        self.registration = snapshot.registration;
        self.default_dir = snapshot.default_dir;
        self.stdio_text_modes = snapshot.stdio_modes;
        self.next_handle = snapshot.next_handle;
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
            .open_file(r"\Flash Disk\save.bin", 3, 3, 3, false, 0)
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
