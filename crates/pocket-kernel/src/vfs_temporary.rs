//! Snapshot ownership for names allocated by GetTempFileName, independent of
//! open handles. A document's mirror can be closed or renamed after capture;
//! only these allocated names may be recreated during cold reconstruction.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Default, Serialize, Deserialize)]
pub struct TemporaryFiles(Vec<TemporaryFile>);
#[derive(Serialize, Deserialize)]
struct TemporaryFile {
    mount_index: usize,
    prefix: String,
    relative: PathBuf,
    #[serde(with = "serde_bytes")]
    contents: Vec<u8>,
}
impl Vfs {
    pub(super) fn is_temporary_path(&self, path: &Path) -> Result<bool, String> {
        let names = self
            .shared
            .0
            .lock()
            .map_err(|_| "VFS device lock poisoned")?
            .temporary_files
            .clone();
        for name in names {
            if !self.is_ram_path(&name) {
                if let Ok((candidate, _)) = self.exact_path(&name, true) {
                    if candidate.canonicalize().is_ok_and(|p| p == path) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    pub(crate) fn capture_temporary_files(&self) -> Result<TemporaryFiles, String> {
        let names = self
            .shared
            .0
            .lock()
            .map_err(|_| "VFS device lock poisoned")?
            .temporary_files
            .clone();
        let mut files = Vec::new();
        for name in names {
            // RAM files already have an owned namespace and contents record.
            if self.is_ram_path(&name) {
                continue;
            }
            let (path, mount) = self
                .exact_path(&name, true)
                .map_err(|code| format!("cannot capture temporary file: WinCE error {code}"))?;
            let root = mount.host_dir.canonicalize().map_err(|e| e.to_string())?;
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| "temporary file escaped mount")?
                .to_path_buf();
            let input = std::fs::File::open(path).map_err(|e| e.to_string())?;
            if input.metadata().map_err(|e| e.to_string())?.len() > 256 * 1024 * 1024 {
                return Err("temporary snapshot file exceeds 256 MiB".into());
            }
            let mut contents = Vec::new();
            input
                .take(256 * 1024 * 1024 + 1)
                .read_to_end(&mut contents)
                .map_err(|e| e.to_string())?;
            if contents.len() > 256 * 1024 * 1024 {
                return Err("temporary file changed during capture".into());
            }
            let mount_index = self
                .mounts
                .iter()
                .position(|m| std::ptr::eq(m, mount))
                .unwrap();
            files.push(TemporaryFile {
                mount_index,
                prefix: mount.prefix.clone(),
                relative,
                contents,
            });
        }
        files.sort_by(|a, b| (a.mount_index, &a.relative).cmp(&(b.mount_index, &b.relative)));
        Ok(TemporaryFiles(files))
    }
    pub(crate) fn prepare_temporary_files(
        &mut self,
        saved: &TemporaryFiles,
    ) -> Result<SnapshotFileCommit, String> {
        let mut names = HashSet::new();
        let mut paths = Vec::new();
        for record in &saved.0 {
            if record.relative.as_os_str().is_empty()
                || !record
                    .relative
                    .components()
                    .all(|p| matches!(p, Component::Normal(_)))
                || record.contents.len() > 256 * 1024 * 1024
            {
                return Err("invalid saved temporary file".into());
            }
            let mount = self
                .mounts
                .get(record.mount_index)
                .filter(|m| m.prefix == record.prefix && !m.read_only)
                .ok_or("temporary file mount is missing or read-only")?;
            let root = mount.host_dir.canonicalize().map_err(|e| e.to_string())?;
            let path = root.join(&record.relative);
            let parent = path
                .parent()
                .ok_or("invalid temporary file parent")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !parent.starts_with(&root)
                || path
                    .symlink_metadata()
                    .is_ok_and(|m| m.is_symlink() || !m.is_file())
            {
                return Err("saved temporary file escaped mount".into());
            }
            let name = format!(
                "{}{}",
                mount.prefix,
                record.relative.to_string_lossy().replace('/', "\\")
            );
            if !names.insert(name.to_ascii_lowercase()) {
                return Err("duplicate saved temporary name".into());
            }
            paths.push((path, record.contents.clone(), self.full_guest_path(&name)));
        }
        // Validate every name before any mutation; the existing file transaction
        // owns backups and removes newly created names if any later stage fails.
        for (path, contents, _) in &paths {
            self.queue_snapshot_metadata_file(path.clone(), contents.clone());
        }
        let mut transaction = self.install_snapshot_files()?;
        let mut state = self
            .shared
            .0
            .lock()
            .map_err(|_| "VFS device lock poisoned")?;
        let mut added = HashSet::new();
        for (_, _, name) in paths {
            if !state
                .temporary_files
                .iter()
                .any(|old| old.eq_ignore_ascii_case(&name))
            {
                state.temporary_files.insert(name.clone());
                added.insert(name);
            }
        }
        drop(state);
        transaction.track_temporary_names(self.shared.clone(), added);
        Ok(transaction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renamed_closed_temporary_is_recreated_and_abort_rolls_back_only_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let mount = || {
            let mut v = Vfs::new();
            v.mount("\\Data\\", dir.path());
            v
        };
        let mut source = mount();
        let (name, _) = source.temporary_file("\\Data", "MFC", 0).unwrap();
        let handle = source.open_file(&name, 3, 0, 3, false, 0).unwrap().handle;
        assert_eq!(source.write(handle, b"saved"), Some(5));
        let names = source.capture_temporary_files().unwrap();
        let files = source.capture_file_snapshot().unwrap();
        assert!(source.close(handle));
        source.rename(&name, "\\Data\\document.dat").unwrap();
        assert!(source.capture_temporary_files().unwrap().0.is_empty());
        drop(source);
        let mut restored = mount();
        {
            let guard = restored.prepare_temporary_files(&names).unwrap();
            restored.restore_file_snapshot(files).unwrap();
            let mut buf = [0; 5];
            restored
                .seek(handle, 0, super::super::SeekKind::Begin)
                .unwrap();
            assert_eq!(restored.read(handle, &mut buf), Some(5));
            assert_eq!(&buf, b"saved");
            restored.close(handle);
            drop(guard);
        }
        assert!(!restored.resolve(&name).unwrap().exists());
        assert!(restored.capture_temporary_files().unwrap().0.is_empty());
        assert_eq!(
            std::fs::read(dir.path().join("document.dat")).unwrap(),
            b"saved"
        );
        let guard = restored.prepare_temporary_files(&names).unwrap();
        guard.commit();
        assert_eq!(
            std::fs::read(restored.resolve(&name).unwrap()).unwrap(),
            b"saved"
        );
        restored.delete(&name.to_ascii_lowercase()).unwrap();
        assert!(restored.capture_temporary_files().unwrap().0.is_empty());
    }
    #[test]
    fn explicit_unique_does_not_create_or_own_a_name_and_prefixes_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = Vfs::new();
        v.mount("\\", dir.path());
        let (name, unique) = v.temporary_file("\\", "abcdef", 0x12345678).unwrap();
        assert_eq!(name, "\\abc5678.TMP");
        assert_eq!(unique, 0x12345678);
        assert!(!dir.path().join("abc5678.TMP").exists());
        assert!(v.capture_temporary_files().unwrap().0.is_empty());
        assert_eq!(v.temporary_file("\\", "a/b", 0), Err(123));
    }
    #[test]
    fn rollback_restores_existing_temp_contents_and_rejects_parent_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let mut v = Vfs::new();
        v.mount("\\", dir.path());
        let (name, _) = v.temporary_file("\\", "MFC", 0).unwrap();
        let saved = v.capture_temporary_files().unwrap();
        let path = v.resolve(&name).unwrap();
        std::fs::write(&path, b"new contents").unwrap();
        {
            let _guard = v.prepare_temporary_files(&saved).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"");
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"new contents");
        assert_eq!(v.capture_temporary_files().unwrap().0.len(), 1);
        let bad = TemporaryFiles(vec![TemporaryFile {
            mount_index: 0,
            prefix: "\\".into(),
            relative: "../escaped.tmp".into(),
            contents: vec![],
        }]);
        assert!(v.prepare_temporary_files(&bad).is_err());
        assert!(!outside.path().join("escaped.tmp").exists());
    }
}
