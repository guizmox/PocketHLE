//! Materialize declared legacy CAB files outside the application's install
//! directory into the writable guest filesystem. Never replace an existing
//! file: it may contain user data or a device registration written by the app.
use anyhow::{ensure, Context, Result};
use pocket_cab::WinCeInstallHeader;
use pocket_kernel::vfs::Vfs;
use std::path::{Component, Path};

fn under(path: &str, directory: &str) -> bool {
    let path = path.replace('/', "\\").to_ascii_lowercase();
    let directory = directory.replace('/', "\\").to_ascii_lowercase();
    let directory = directory.trim_end_matches('\\');
    path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|tail| tail.starts_with('\\'))
}

pub(super) fn complete(vfs: &mut Vfs, root: &Path, install_dir: &str) -> Result<usize> {
    let canonical_root = root
        .canonicalize()
        .context("Resolving CAB extraction directory")?;
    let mut headers = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && (entry.file_name().eq_ignore_ascii_case("_setup.xml")
                || entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("000")))
        {
            ensure!(
                entry.metadata()?.len() <= 8 * 1024 * 1024,
                "CAB install header too large"
            );
            headers.push(entry.path());
        }
    }
    headers.sort();
    ensure!(headers.len() <= 16, "Too many CAB install headers");
    let mut installed = 0;
    for path in headers {
        if path
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("_setup.xml"))
        {
            let script = pocket_cab::WinCeSetupScript::parse_bytes(&std::fs::read(path)?);
            install_directories(vfs, &script.install_dirs, install_dir)?;
            continue;
        }
        let header = WinCeInstallHeader::parse_file(&path)?;
        if !header.structured {
            continue; // Never infer system-file destinations from string scans.
        }
        install_directories(vfs, &header.directories, install_dir)?;
        for file in &header.files {
            if under(&file.destination, install_dir) {
                continue; // Already exposed by the application's install mount.
            }
            let destination = file.destination.replace('/', "\\");
            ensure!(
                destination.starts_with('\\')
                    && !destination.contains('\0')
                    && destination
                        .split('\\')
                        .all(|part| part != "." && part != ".."),
                "Invalid CAB system-file destination"
            );
            let Some(source) = header
                .host_path(root, &file.destination)
                .filter(|p| p.is_file())
            else {
                continue; // Partial imports may omit optional files.
            };
            let canonical_source = source.canonicalize()?;
            ensure!(
                canonical_source.starts_with(&canonical_root),
                "CAB payload escapes extraction directory"
            );
            let relative = source.strip_prefix(root)?;
            let parts: Vec<_> = relative
                .components()
                .map(|part| match part {
                    Component::Normal(name) => name.to_str().context("Invalid CAB filename"),
                    _ => anyhow::bail!("Invalid CAB payload path"),
                })
                .collect::<Result<_>>()?;
            let guest_source = format!(
                "{}\\{}",
                install_dir.trim_end_matches(['\\', '/']),
                parts.join("\\")
            );
            ensure!(
                vfs.resolve(&guest_source)
                    .and_then(|p| p.canonicalize().ok())
                    .as_ref()
                    == Some(&canonical_source),
                "CAB payload is not mounted at its install path"
            );
            if vfs
                .resolve(&destination)
                .and_then(|path| path.canonicalize().ok())
                .as_ref()
                == Some(&canonical_source)
            {
                continue; // An install mount already exposes this file.
            }
            match vfs.copy_file(&guest_source, &destination, true) {
                Ok(()) => installed += 1,
                Err(80 | 183) => {} // Existing guest data always wins, on every launch.
                Err(error) => {
                    anyhow::bail!("Cannot install CAB file {destination}: WinCE error {error}")
                }
            }
        }
    }
    Ok(installed)
}

fn install_directories(vfs: &Vfs, directories: &[String], install_dir: &str) -> Result<()> {
    // DIRS also contains empty application folders (LeoFS's Stn). Create
    // them through the mounted writable overlay, never the source package.
    let mut directories = directories.to_vec();
    directories.sort_by_key(|path| (path.matches('\\').count(), path.to_ascii_lowercase()));
    for directory in directories.iter().filter(|p| under(p, install_dir)) {
        ensure!(
            directory.starts_with('\\')
                && !directory.contains(['\0', '/', ':'])
                && directory.split('\\').all(|p| p != "." && p != ".."),
            "Invalid CAB directory"
        );
        let mut path = install_dir.trim_end_matches('\\').to_string();
        let suffix = directory
            .get(path.len()..)
            .context("Invalid CAB directory prefix")?;
        for part in suffix.split('\\').filter(|p| !p.is_empty()) {
            path.push('\\');
            path.push_str(part);
            match vfs.attributes(&path) {
                Ok(attrs) if attrs & 0x10 != 0 => continue,
                Ok(_) => anyhow::bail!("CAB directory is occupied by a file: {path}"),
                Err(2 | 3) => {}
                Err(error) => {
                    anyhow::bail!("Cannot inspect CAB directory {path}: WinCE error {error}")
                }
            }
            vfs.create_directory(&path).map_err(|error| {
                anyhow::anyhow!("Cannot install CAB directory {path}: WinCE error {error}")
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header() -> Vec<u8> {
        let mut strings = Vec::new();
        for (id, value) in [(1u16, "\\Program Files\\Example"), (2, "\\Windows")] {
            strings.extend_from_slice(&id.to_le_bytes());
            strings.extend_from_slice(&((value.len() + 1) as u16).to_le_bytes());
            strings.extend_from_slice(value.as_bytes());
            strings.push(0);
        }
        let mut dirs = Vec::new();
        for id in [1u16, 2] {
            for value in [id, 4, id, 0] {
                dirs.extend_from_slice(&value.to_le_bytes());
            }
        }
        let mut files = Vec::new();
        for (id, directory, name) in [(1u16, 1u16, "example.exe"), (2, 2, "state.dat")] {
            for value in [id, directory, id] {
                files.extend_from_slice(&value.to_le_bytes());
            }
            files.extend_from_slice(&0u32.to_le_bytes());
            files.extend_from_slice(&((name.len() + 1) as u16).to_le_bytes());
            files.extend_from_slice(name.as_bytes());
            files.push(0);
        }
        let dirs_off = 100 + strings.len();
        let files_off = dirs_off + dirs.len();
        let end = files_off + files.len();
        let mut bytes = vec![0; 100];
        bytes[..4].copy_from_slice(b"MSCE");
        bytes[8..12].copy_from_slice(&(end as u32).to_le_bytes());
        bytes[20..24].copy_from_slice(&2577u32.to_le_bytes());
        for offset in [48, 50, 52] {
            bytes[offset..offset + 2].copy_from_slice(&2u16.to_le_bytes());
        }
        for (offset, value) in [
            (60, 100),
            (64, dirs_off),
            (68, files_off),
            (72, end),
            (76, end),
            (80, end),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
        bytes.extend(strings);
        bytes.extend(dirs);
        bytes.extend(files);
        bytes
    }
    #[test]
    fn declared_system_file_is_installed_once_and_user_bytes_survive() {
        let package = tempfile::tempdir().unwrap();
        let device = tempfile::tempdir().unwrap();
        std::fs::create_dir(device.path().join("Windows")).unwrap();
        std::fs::write(package.path().join("example.000"), header()).unwrap();
        std::fs::write(package.path().join("example.exe"), b"executable").unwrap();
        std::fs::write(package.path().join("state.dat"), b"initial bytes").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_read_only("\\Program Files\\Example\\", package.path());
        vfs.mount_filesystem("\\", device.path());
        vfs.mount_filesystem("\\Windows\\", device.path().join("Windows"));
        assert_eq!(
            complete(&mut vfs, package.path(), "\\Program Files\\Example\\").unwrap(),
            1
        );
        let installed = vfs.resolve("\\Windows\\state.dat").unwrap();
        assert_eq!(std::fs::read(&installed).unwrap(), b"initial bytes");
        assert!(!device
            .path()
            .join("Program Files/Example/example.exe")
            .exists());
        std::fs::write(&installed, b"user registration").unwrap();
        assert_eq!(
            complete(&mut vfs, package.path(), "\\Program Files\\Example\\").unwrap(),
            0
        );
        assert_eq!(std::fs::read(&installed).unwrap(), b"user registration");
        assert_eq!(
            std::fs::read(package.path().join("state.dat")).unwrap(),
            b"initial bytes"
        );
    }
    #[test]
    fn empty_install_directories_are_repaired_in_the_save_overlay_without_replacing_user_data() {
        let package = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_read_only(r"\Program Files\Example\", package.path());
        vfs.mount_save_dir(r"\Program Files\Example\", saves.path());
        let directories = vec![
            r"\Program Files\Example\Stn\nested\".into(),
            r"\Program Files\Example\".into(),
            r"\Program Files\Example2\other\".into(),
        ];
        install_directories(&vfs, &directories, r"\Program Files\Example\").unwrap();
        let folder = vfs.resolve(r"\Program Files\Example\Stn").unwrap();
        let nested = vfs.resolve(r"\Program Files\Example\Stn\nested").unwrap();
        assert!(folder.starts_with(saves.path()));
        assert!(nested.is_dir());
        assert_eq!(std::fs::read_dir(package.path()).unwrap().count(), 0);
        std::fs::write(folder.join("user.stn"), b"pilot").unwrap();
        for _ in 0..16 {
            install_directories(&vfs, &directories, r"\Program Files\Example\").unwrap();
        }
        assert_eq!(std::fs::read(folder.join("user.stn")).unwrap(), b"pilot");
        assert_eq!(std::fs::read_dir(saves.path()).unwrap().count(), 1);
        assert!(install_directories(
            &vfs,
            &[r"\Program Files\Example\..\escape".into()],
            r"\Program Files\Example\"
        )
        .is_err());
    }

    #[test]
    fn install_directory_test_observes_component_boundaries() {
        assert!(under(
            "\\PROGRAM FILES\\Example\\data.bin",
            "\\Program Files\\Example\\"
        ));
        assert!(!under(
            "\\Program Files\\Example2\\data.bin",
            "\\Program Files\\Example"
        ));
        assert!(!under("\\Windows\\shared.dat", "\\Program Files\\Example"));
    }
}
