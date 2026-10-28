//! Host-backed writable object-store namespace for the PocketPC profile.
//! Install/save/card mounts retain their existing, more-specific precedence.
use pocket_kernel::vfs::Vfs;
use std::path::Path;

pub(crate) fn mount(vfs: &mut Vfs, library_root: &Path) -> std::io::Result<()> {
    let root = library_root.join("object-store-pocketpc");
    std::fs::create_dir_all(&root)?;
    let canonical = root.canonicalize()?;
    // These are standard WinCE directories, not application resources. Never
    // manufacture files/registration data or replace a preexisting payload.
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
    vfs.mount("\\", &root);
    // A same-prefix writable Windows overlay also permits copy-on-write when
    // an installer mounts its bundled shared Windows files read-only.
    vfs.mount("\\Windows\\", root.join("Windows"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_kernel::vfs::{Access, SeekKind};
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
