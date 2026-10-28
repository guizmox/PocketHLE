//! Host-backed writable object-store namespace for the PocketPC profile.
//! Install/save/card mounts retain their existing, more-specific precedence.
use pocket_kernel::vfs::Vfs;
use std::path::Path;

pub(crate) fn mount(vfs: &mut Vfs, library_root: &Path) -> std::io::Result<()> {
    let root = library_root.join("object-store-pocketpc");
    std::fs::create_dir_all(&root)?;
    let canonical = root.canonicalize()?;
    // Standard WinCE directories; existing files and registration are preserved.
    // The explicitly requested hardware-presence marker is created separately.
    for directory in [
        "Windows",
        "Temp",
        "My Documents",
        "Program Files",
        "Windows/StartUp",
        "Windows/Start Menu",
        "Windows/Start Menu/Programs",
        "Windows/Start Menu/Programs/Games",
        "Windows/Fonts",
        "Windows/Desktop",
        "Windows/AppMgr",
        "Windows/Recent",
        "Windows/Favorites",
    ] {
        let path = root.join(directory);
        std::fs::create_dir_all(&path)?;
        if !path.canonicalize()?.starts_with(&canonical) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "PocketPC system directory escapes its object store",
            ));
        }
    }
    ensure_2700g_presence_marker(&root.join("Windows"))?;
    vfs.mount_filesystem("\\", &root);
    // A same-prefix writable Windows overlay also permits copy-on-write when
    // an installer mounts its bundled shared Windows files read-only.
    vfs.mount_filesystem("\\Windows\\", root.join("Windows"));
    Ok(())
}

/// Explicit compatibility marker requested by the user, not a codec or hardware
/// implementation. THPS2's startup fopen probe only checks whether this opens.
fn ensure_2700g_presence_marker(windows: &Path) -> std::io::Result<()> {
    use std::io::Write;
    const NAME: &str = "wmv9decoder2700G.dll";
    // Preserve genuine DLLs and any existing entry, including case variants
    // on case-sensitive hosts. Never open an existing file for writing.
    for entry in std::fs::read_dir(windows)? {
        if entry?
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(NAME)
        {
            return Ok(());
        }
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(windows.join(NAME))
    {
        Ok(mut file) => {
            file.write_all(b"PocketHLE hardware-presence marker. Not an executable codec.\r\n")
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_kernel::vfs::{Access, SeekKind};
    #[test]
    fn marker_is_created_once_and_existing_case_variant_is_preserved() {
        let library = tempfile::tempdir().unwrap();
        mount(&mut Vfs::new(), library.path()).unwrap();
        let windows = library.path().join("object-store-pocketpc/Windows");
        let marker = windows.join("wmv9decoder2700G.dll");
        assert!(std::fs::read(&marker).unwrap().starts_with(b"PocketHLE"));
        std::fs::remove_file(&marker).unwrap();
        let real = windows.join("WMV9DECODER2700G.DLL");
        std::fs::write(&real, b"existing original bytes").unwrap();
        mount(&mut Vfs::new(), library.path()).unwrap();
        assert_eq!(std::fs::read(&real).unwrap(), b"existing original bytes");
        assert_eq!(
            std::fs::read_dir(&windows)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("wmv9decoder2700g.dll"))
                .count(),
            1
        );
    }
    #[test]
    fn system_file_survives_new_vfs_without_changing_registered_bytes() {
        let library = tempfile::tempdir().unwrap();
        let path = "\\WINDOWS\\license-state.dat";
        let data = [0x31, 0x00, 0xff, 0x7e, 0x81];
        {
            let mut vfs = Vfs::new();
            assert!(vfs.open_file(path, 2, 0, 2, false, 0).is_err());
            mount(&mut vfs, library.path()).unwrap();
            let h = vfs.open_file(path, 2, 0, 2, false, 0).unwrap().handle;
            assert_eq!(vfs.write(h, &data), Some(data.len()));
            vfs.flush(h).unwrap();
            assert!(vfs.close(h));
        }
        let mut vfs = Vfs::new();
        mount(&mut vfs, library.path()).unwrap();
        let h = vfs
            .open_file("\\windows\\LICENSE-state.DAT", 1, 0, 3, false, 0)
            .unwrap()
            .handle;
        let mut bytes = [0; 5];
        assert_eq!(vfs.read(h, &mut bytes), Some(5));
        assert_eq!(bytes, data);
        assert!(vfs.close(h));
    }
    #[test]
    fn installed_assets_and_game_saves_keep_priority_and_windows_copy_on_write() {
        let library = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        std::fs::write(install.path().join("asset.dat"), b"installed").unwrap();
        let mut vfs = Vfs::new();
        mount(&mut vfs, library.path()).unwrap();
        vfs.mount_read_only("\\Program Files\\Example\\", install.path());
        vfs.mount_save_dir("\\Program Files\\Example\\", saves.path());
        let h = vfs
            .open_file("\\Program Files\\Example\\asset.dat", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        assert_eq!(vfs.write(h, b"saved!!!!"), Some(9));
        vfs.close(h);
        assert_eq!(
            std::fs::read(saves.path().join("asset.dat")).unwrap(),
            b"saved!!!!"
        );
        assert_eq!(
            std::fs::read(install.path().join("asset.dat")).unwrap(),
            b"installed"
        );
        assert!(!library
            .path()
            .join("object-store-pocketpc/Program Files/Example/asset.dat")
            .exists());
        vfs.mount_read_only("\\Windows\\", install.path());
        let h = vfs
            .open_file("\\Windows\\asset.dat", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        vfs.seek(h, 0, SeekKind::Begin).unwrap();
        vfs.write(h, b"updated!!");
        vfs.close(h);
        assert_eq!(
            std::fs::read(
                library
                    .path()
                    .join("object-store-pocketpc/Windows/asset.dat")
            )
            .unwrap(),
            b"updated!!"
        );
        assert_eq!(
            std::fs::read(install.path().join("asset.dat")).unwrap(),
            b"installed"
        );
        vfs.mount_read_only("\\SD Card\\", install.path());
        assert!(vfs
            .open("\\SD Card\\asset.dat", Access::Write, true)
            .is_none());
    }
    #[test]
    fn broad_device_root_does_not_hide_flat_install_resources() {
        let library = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        std::fs::write(install.path().join("default.pnx"), b"original resource").unwrap();
        let mut vfs = Vfs::new();
        mount(&mut vfs, library.path()).unwrap();
        // A same-named system file is outside this game's fallback namespace.
        std::fs::write(
            library
                .path()
                .join("object-store-pocketpc/Windows/default.pnx"),
            b"other",
        )
        .unwrap();
        vfs.mount_read_only("\\Program Files\\Example\\", install.path());
        vfs.mount_save_dir("\\Program Files\\Example\\", saves.path());
        let h = vfs
            .open_file(
                "\\Program Files\\Example\\data\\default.pnx",
                1,
                3,
                3,
                false,
                0,
            )
            .unwrap()
            .handle;
        let mut bytes = [0; 17];
        assert_eq!(vfs.read(h, &mut bytes), Some(17));
        assert_eq!(&bytes, b"original resource");
        vfs.close(h);
        // A real directory in either overlay forbids borrowing a missing leaf
        // from elsewhere in the installation, just as before the root mount.
        std::fs::create_dir(saves.path().join("data")).unwrap();
        assert!(vfs
            .open_file(
                "\\Program Files\\Example\\data\\default.pnx",
                1,
                3,
                3,
                false,
                0
            )
            .is_err());
    }

    #[test]
    #[cfg(unix)]
    fn system_directory_symlink_cannot_escape_storage() {
        let library = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = library.path().join("object-store-pocketpc");
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("Windows")).unwrap();
        let mut vfs = Vfs::new();
        assert!(mount(&mut vfs, library.path()).is_err());
        assert!(!outside.path().join("StartUp").exists());
        assert!(vfs
            .open("\\Windows\\new.dat", Access::Write, true)
            .is_none());
    }
}
