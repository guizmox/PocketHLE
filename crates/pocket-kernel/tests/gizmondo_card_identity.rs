//! An overlay changes file backing, not the removable medium's identity.
use pocket_kernel::vfs::{Access, Vfs};

#[test]
fn original_card_identity_survives_writable_overlay_and_volume_restore() {
    let card = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let serial = 0xb5f10053u32;
    let marker = 200045u32.wrapping_sub(serial).wrapping_sub(1);
    std::fs::write(card.path().join("GZGA200045"), marker.to_le_bytes()).unwrap();
    let mut vfs = Vfs::new();
    vfs.mount_read_only("\\SD Card\\", card.path());
    let original = vfs.open("\\SD Card\\Vol:", Access::Read, false).unwrap();
    assert_eq!(vfs.volume(original).unwrap().serial(), serial);
    vfs.mount_save_dir("\\SD Card\\", saves.path());
    let writable = vfs.open("\\SD Card\\Vol:", Access::Read, false).unwrap();
    assert_ne!(vfs.volume(writable).unwrap().serial(), serial);
    assert_eq!(vfs.volume_backing_serial(writable), Some(serial));
    assert!(!vfs.volume(writable).unwrap().read_only);
    let snapshot = vfs.capture_file_snapshot().unwrap();
    assert!(vfs.close(original));
    assert!(vfs.close(writable));
    vfs.restore_file_snapshot(snapshot).unwrap();
    assert_eq!(vfs.volume_backing_serial(writable), Some(serial));
    assert_eq!(vfs.volume_backing_serial(original), Some(serial));
    assert!(!vfs.volume(writable).unwrap().read_only);
    let file = vfs
        .open_file("\\SD Card\\options.dat", 2, 3, 2, false, 0)
        .unwrap();
    assert_eq!(vfs.write(file.handle, b"saved"), Some(5));
    assert!(vfs.close(file.handle));
    assert_eq!(
        std::fs::read(saves.path().join("options.dat")).unwrap(),
        b"saved"
    );
    assert!(!card.path().join("options.dat").exists());
}
