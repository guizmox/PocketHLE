//! Pocket CMD's COPY reads source FILETIMEs and applies them to the open
//! destination. Set real backing metadata, shared by reopened/child handles.
use super::*;
use std::{
    fs::FileTimes,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const UNIX_OFFSET: u64 = 116_444_736_000_000_000;

fn to_file_time(time: std::io::Result<SystemTime>) -> u64 {
    let epoch = UNIX_EPOCH - Duration::from_secs(11_644_473_600);
    time.ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .and_then(|d| u64::try_from(d.as_nanos() / 100).ok())
        .unwrap_or(0)
}

pub(super) fn metadata_times(metadata: &std::fs::Metadata) -> [u64; 3] {
    [
        to_file_time(metadata.created()),
        to_file_time(metadata.accessed()),
        to_file_time(metadata.modified()),
    ]
}

fn from_file_time(time: u64) -> Result<SystemTime, u32> {
    if time > i64::MAX as u64 {
        return Err(87);
    }
    let delta = time.abs_diff(UNIX_OFFSET);
    let duration = Duration::new(delta / 10_000_000, (delta % 10_000_000) as u32 * 100);
    if time >= UNIX_OFFSET {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    }
    .ok_or(87)
}

impl Vfs {
    /// Creation/access/write timestamps. Zero means the store has no timestamp.
    pub fn file_times(&self, handle: u32) -> Option<[u64; 3]> {
        #[cfg(not(windows))]
        let state = self.shared.0.lock().ok()?;
        if let Some(open) = self.ram_handles.get(&handle) {
            return Some(open.lock().ok()?.file.lock().ok()?.times);
        }
        let open = self.handles.get(&handle)?.lock().ok()?;
        let metadata = open.file.metadata().ok()?;
        let times = metadata_times(&metadata);
        #[cfg(not(windows))]
        let times = {
            let mut times = times;
            let key = open
                .host_path
                .canonicalize()
                .ok()?
                .to_string_lossy()
                .to_ascii_lowercase();
            if let Some(time) = state.creation_times.get(&key) {
                times[0] = *time;
            }
            times
        };
        Some(times)
    }

    /// None and a zero FILETIME leave a field unchanged. Validate every field
    /// and write access before mutating any host or RAM metadata.
    pub fn set_file_times(&self, handle: u32, times: [Option<u64>; 3]) -> Result<(), u32> {
        let times = times.map(|t| t.filter(|t| *t != 0));
        let mut converted = [None; 3];
        for (slot, value) in converted.iter_mut().zip(times) {
            if let Some(value) = value {
                *slot = Some(from_file_time(value)?);
            }
        }
        #[allow(unused_mut)] // Birth-time fallback is writable only on non-Windows stores.
        let mut state = self.shared.0.lock().map_err(|_| 29u32)?;
        if let Some(open) = self.ram_handles.get(&handle) {
            let open = open.lock().map_err(|_| 29u32)?;
            if open.access == Access::Read || open.lease.as_ref().is_some_and(|l| l.access & 2 == 0)
            {
                return Err(5);
            }
            // Locate the shared RAM name, including an imported open description.
            let read_only = self.ram.as_ref().is_some_and(|ram| {
                ram.files.lock().unwrap().files.iter().any(|(key, entry)| {
                    Arc::ptr_eq(entry, &open.file)
                        && state
                            .attributes
                            .get(&format!("ram:{key}"))
                            .is_some_and(|a| a & 1 != 0)
                })
            });
            if read_only {
                return Err(5);
            }
            let mut file = open.file.lock().map_err(|_| 29u32)?;
            for (slot, value) in file.times.iter_mut().zip(times) {
                if let Some(value) = value {
                    *slot = value;
                }
            }
            return Ok(());
        }
        let open = self
            .handles
            .get(&handle)
            .ok_or(6u32)?
            .lock()
            .map_err(|_| 29u32)?;
        if open.access == Access::Read || open.lease.as_ref().is_some_and(|l| l.access & 2 == 0) {
            return Err(5);
        }
        let metadata = open.file.metadata().map_err(|e| io_error(&e))?;
        let key = open
            .host_path
            .canonicalize()
            .map_err(|e| io_error(&e))?
            .to_string_lossy()
            .to_ascii_lowercase();
        if metadata.permissions().readonly()
            || state.attributes.get(&key).is_some_and(|a| a & 1 != 0)
        {
            return Err(5);
        }
        if converted.iter().all(Option::is_none) {
            return Ok(());
        }
        let mut native = FileTimes::new();
        if let Some(time) = converted[1] {
            native = native.set_accessed(time);
        }
        if let Some(time) = converted[2] {
            native = native.set_modified(time);
        }
        #[cfg(windows)]
        if let Some(time) = converted[0] {
            use std::os::windows::fs::FileTimesExt;
            native = native.set_created(time);
        }
        if cfg!(windows) || converted[1].is_some() || converted[2].is_some() {
            open.file.set_times(native).map_err(|e| io_error(&e))?;
        }
        #[cfg(not(windows))]
        if let Some(time) = times[0] {
            state.creation_times.insert(key, time);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_set_times_preserve_metadata_in_reopened_and_child_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut vfs = Vfs::new();
        vfs.mount("\\", dir.path());
        let h = vfs
            .open_file("\\source.txt", 3, 3, 2, false, 0)
            .unwrap()
            .handle;
        vfs.write(h, b"CMD copy fixture").unwrap();
        let times = [
            UNIX_OFFSET + 10_000_000,
            UNIX_OFFSET + 20_000_000,
            UNIX_OFFSET + 30_000_000,
        ];
        vfs.set_file_times(h, times.map(Some)).unwrap();
        assert_eq!(vfs.file_times(h), Some(times));
        let object = vfs.export_handle(h).unwrap();
        let mut child = Vfs::new();
        child.mount("\\", dir.path());
        child.attach_shared_context(vfs.shared_context());
        child.import_handle(42, object);
        assert_eq!(child.file_times(42), Some(times));
        child
            .set_file_times(42, [None, Some(0), Some(times[2] + 10_000_000)])
            .unwrap();
        assert_eq!(vfs.file_times(h).unwrap()[2], times[2] + 10_000_000);
        child.close(42);
        vfs.close(h);
        assert_eq!(
            vfs.file_information("\\source.txt").unwrap().times[0],
            times[0]
        );
        vfs.copy_file("\\source.txt", "\\copy.txt", true).unwrap();
        let copied = vfs.file_information("\\copy.txt").unwrap().times;
        assert_eq!(copied[0], times[0]);
        assert_eq!(copied[2], times[2] + 10_000_000);
        vfs.rename("\\copy.txt", "\\renamed.txt").unwrap();
        assert_eq!(vfs.file_information("\\renamed.txt").unwrap().times, copied);
    }

    #[test]
    fn invalid_or_read_only_set_times_cannot_change_backing_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut vfs = Vfs::new();
        vfs.mount("\\", dir.path());
        let h = vfs
            .open_file("\\file.txt", 3, 3, 2, false, 0)
            .unwrap()
            .handle;
        let before = vfs.file_times(h).unwrap();
        assert_eq!(
            vfs.set_file_times(h, [Some(UNIX_OFFSET), None, Some(u64::MAX)]),
            Err(87)
        );
        assert_eq!(vfs.file_times(h).unwrap(), before);
        vfs.set_attributes("\\file.txt", 1).unwrap();
        assert_eq!(
            vfs.set_file_times(h, [None, None, Some(UNIX_OFFSET)]),
            Err(5)
        );
        assert_eq!(vfs.file_times(h).unwrap(), before);
        vfs.close(h);
        let h = vfs
            .open_file("\\file.txt", 1, 3, 3, false, 0)
            .unwrap()
            .handle;
        assert_eq!(vfs.set_file_times(h, [None; 3]), Err(5));
        assert_eq!(vfs.set_file_times(0, [None; 3]), Err(6));
    }

    #[test]
    fn setting_overlay_times_preserves_original_image_metadata() {
        let image = tempfile::tempdir().unwrap();
        let saved = tempfile::tempdir().unwrap();
        let original = image.path().join("copy.txt");
        std::fs::write(&original, b"original ROM bytes").unwrap();
        let before = metadata_times(&original.metadata().unwrap());
        let mut vfs = Vfs::new();
        vfs.mount_read_only("\\Windows\\", image.path());
        vfs.mount_save_dir("\\Windows\\", saved.path());
        let handle = vfs
            .open_file("\\Windows\\copy.txt", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        let time = UNIX_OFFSET + 40_000_000;
        vfs.set_file_times(handle, [Some(time), None, Some(time)])
            .unwrap();
        assert_eq!(vfs.file_times(handle).unwrap()[2], time);
        assert_eq!(metadata_times(&original.metadata().unwrap())[2], before[2]);
        assert_eq!(std::fs::read(original).unwrap(), b"original ROM bytes");
    }

    #[test]
    fn ram_timestamps_follow_shared_file_identity_and_rename() {
        let ram = crate::memory_division::MemoryDivision::default();
        let mut vfs = Vfs::new();
        vfs.attach_ram(Some(ram.clone()));
        let h = vfs
            .open_file("\\copy.txt", 3, 3, 2, false, 0)
            .unwrap()
            .handle;
        let mut child = Vfs::new();
        child.attach_ram(Some(ram));
        child.attach_shared_context(vfs.shared_context());
        child.import_handle(42, vfs.export_handle(h).unwrap());
        let times = [
            UNIX_OFFSET + 10_000_000,
            UNIX_OFFSET + 20_000_000,
            UNIX_OFFSET + 30_000_000,
        ];
        child.set_file_times(42, times.map(Some)).unwrap();
        assert_eq!(vfs.file_times(h), Some(times));
        assert_eq!(vfs.file_information("\\copy.txt").unwrap().times, times);
        vfs.close(h);
        child.close(42);
        vfs.rename("\\copy.txt", "\\renamed.txt").unwrap();
        assert_eq!(
            child.file_information("\\renamed.txt").unwrap().times,
            times
        );
    }
}
