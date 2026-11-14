//! The unmodified CE6 shell creates a typed system database for recycled files.
use super::*;
use crate::bluetooth::tests::{call, setup};
use pocket_cpu::{regs::ArmReg, stub::StubCpu, Cpu};
use pocket_kernel::KernelState;

const PATH: u32 = 0x1100;
const INFO: u32 = 0x1200;
const NAME: u32 = INFO + 8;
const SORT: u32 = INFO + 92;
const OID: u32 = 0x1400;
const GUID: u32 = 0x1000;
const PROPS: u32 = 0x1500;
const LABEL: u32 = 0x1700;
const BUFFER: u32 = 0x2000;
const TEXT_ID: u32 = 0x1001_001f;
const SIZE_ID: u32 = 0x1002_0013;
const TIME_ID: u32 = 0x1003_0040;
const BLOB_ID: u32 = 0x1004_0041;

fn invoke(
    cpu: &mut StubCpu,
    kernel: &mut KernelState,
    dispatcher: &mut WinCeDispatcher,
    name: &str,
    args: &[u32],
) -> u32 {
    let DispatchOutcome::ReturnedR0(result) =
        call(cpu, kernel, dispatcher, "coredll.dll", name, args)
    else {
        panic!("database operation yielded");
    };
    result
}
fn wide(cpu: &mut StubCpu, at: u32, value: &str) {
    cpu.write_mem(
        at,
        &value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
fn database(cpu: &mut StubCpu, name: &str, unique: bool) {
    let mut data = [0; 220];
    data[..4].copy_from_slice(&[1, 0, 1, 0]);
    data[4..8].copy_from_slice(&7u32.to_le_bytes());
    data[72..76].copy_from_slice(&0x7462_6372u32.to_le_bytes());
    data[92..96].copy_from_slice(&[1, 0, 1, 0]);
    data[96..98].copy_from_slice(&(if unique { 0x200u16 } else { 0 }).to_le_bytes());
    data[100..104].copy_from_slice(&TEXT_ID.to_le_bytes());
    data[112..116].copy_from_slice(&2u32.to_le_bytes());
    cpu.write_mem(INFO, &data).unwrap();
    wide(cpu, NAME, name);
}
fn properties(cpu: &mut StubCpu, label: &str, size: u32) {
    wide(cpu, LABEL, label);
    let values = [
        TEXT_ID,
        0,
        LABEL,
        0,
        SIZE_ID,
        0,
        size,
        0,
        TIME_ID,
        0,
        0x1234_5678,
        0x01da_1234,
        BLOB_ID,
        0,
        3,
        0x1800,
    ];
    cpu.write_mem(
        PROPS,
        &values
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    cpu.write_mem(0x1800, &[0, 128, 255]).unwrap();
}
fn mount_device(kernel: &mut KernelState) -> tempfile::TempDir {
    let device = tempfile::tempdir().unwrap();
    std::fs::create_dir(device.path().join("Windows")).unwrap();
    kernel.vfs.mount_filesystem("\\", device.path());
    device
}

#[test]
fn recycle_database_preserves_typed_records_and_cursor_after_buffer_negotiation() {
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    let device = mount_device(&mut kernel);
    database(&mut cpu, "Recycle Bin", true);
    macro_rules! api { ($name:literal, $($value:expr),* $(,)?) => { invoke(&mut cpu, &mut kernel, &mut dispatcher, $name, &[$($value),*]) }; }
    assert_eq!(
        api!("CeOpenDatabaseEx2", GUID, OID, NAME, SORT, 1, 0),
        INVALID_HANDLE
    );
    let db = api!("CeCreateDatabaseEx2", GUID, INFO);
    assert!(db > DATABASE_OID && db != INVALID_HANDLE);
    assert_eq!(api!("CeCreateDatabaseEx2", GUID, INFO), 0);
    let handle = api!("CeOpenDatabaseEx2", GUID, OID, NAME, SORT, 1, 0);
    assert_ne!(handle, INVALID_HANDLE);
    assert_eq!(cpu.read_u32_le(OID).unwrap(), db);
    properties(&mut cpu, "Zoé.txt", 17);
    let zoe = api!("CeWriteRecordProps", handle, 0, 4, PROPS);
    properties(&mut cpu, "Ana.txt", 42);
    let ana = api!("CeWriteRecordProps", handle, 0, 4, PROPS);
    assert!(zoe > db && ana > zoe);
    assert_eq!(api!("CeWriteRecordProps", handle, 0, 4, PROPS), 0);
    assert_eq!(kernel.thread_last_errors.get(&0), Some(&183));
    assert_eq!(api!("CeSeekDatabaseEx", handle, 2, 0, 0, 0), ana);
    cpu.write_mem(0x1900, &[0; 12]).unwrap();
    assert_eq!(
        api!("CeReadRecordProps", handle, 0, 0x1900, 0, 0x1904, 0x1908),
        0
    );
    let required = cpu.read_u32_le(0x1908).unwrap();
    assert!(required > 64);
    assert_eq!(cpu.read_u32_le(0x1904).unwrap(), 0);
    assert_eq!(kernel.cedb.generic.cursors[&handle].record, Some(ana));
    cpu.write_mem(0x1904, &BUFFER.to_le_bytes()).unwrap();
    assert_eq!(
        api!(
            "CeReadRecordPropsEx",
            handle,
            0,
            0x1900,
            0,
            0x1904,
            0x1908,
            0
        ),
        ana
    );
    assert_eq!(cpu.read_mem(0x1900, 2).unwrap(), 4u16.to_le_bytes());
    let text = cpu.read_u32_le(BUFFER + 8).unwrap();
    let expected: Vec<_> = "Ana.txt"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(cpu.read_mem(text, expected.len() as u32).unwrap(), expected);
    assert_eq!(cpu.read_u32_le(BUFFER + 16 + 8).unwrap(), 42);
    assert_eq!(
        cpu.read_mem(BUFFER + 32 + 8, 8).unwrap(),
        [0x78, 0x56, 0x34, 0x12, 0x34, 0x12, 0xda, 1]
    );
    let blob = cpu.read_u32_le(BUFFER + 48 + 12).unwrap();
    assert_eq!(cpu.read_mem(blob, 3).unwrap(), [0, 128, 255]);
    assert_eq!(kernel.cedb.generic.cursors[&handle].record, Some(zoe));
    assert_eq!(api!("CeDeleteDatabaseEx", GUID, db), 0);
    assert_eq!(api!("CloseHandle", handle), 1);
    assert_eq!(api!("CeFlushDBVol", GUID), 1);
    kernel.cedb.generic = Default::default();
    let reopened = api!("CeOpenDatabaseEx2", GUID, OID, 0, SORT, 0, 0);
    assert_ne!(reopened, INVALID_HANDLE);
    assert_eq!(api!("CeSeekDatabaseEx", reopened, 2, 0, 0, 0), ana);
    assert_eq!(api!("CeDeleteRecord", reopened, ana), 1);
    assert_eq!(api!("CeSeekDatabaseEx", reopened, 2, 0, 0, 0), zoe);
    assert_eq!(api!("CloseHandle", reopened), 1);
    assert_eq!(api!("CeDeleteDatabaseEx", GUID, db), 1);
    assert!(device
        .path()
        .join("Windows/PocketHLE-System.cedb.json")
        .is_file());
}

#[test]
fn database_mount_respects_guest_outputs_readonly_image_and_reopen() {
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    let image = tempfile::tempdir().unwrap();
    std::fs::write(image.path().join("untouched.txt"), b"original").unwrap();
    kernel.vfs.mount_read_only_filesystem("\\", image.path());
    let device = mount_device(&mut kernel);
    macro_rules! api { ($name:literal, $($value:expr),* $(,)?) => { invoke(&mut cpu, &mut kernel, &mut dispatcher, $name, &[$($value),*]) }; }
    wide(&mut cpu, PATH, "\\RecycleBin.vol");
    assert_eq!(api!("CeMountDBVol", 0x6000, PATH, 4), 0);
    assert!(!device.path().join("RecycleBin.vol").exists());
    assert_eq!(api!("CeMountDBVol", GUID, PATH, 4), 1);
    let first_guid = cpu.read_mem(GUID, 16).unwrap();
    assert_ne!(first_guid, [0; 16]);
    database(&mut cpu, "Files", false);
    let db = api!("CeCreateDatabaseEx2", GUID, INFO);
    assert_ne!(db, 0);
    let handle = api!("CeOpenDatabaseEx2", GUID, OID, NAME, SORT, 0, 0);
    properties(&mut cpu, "Document.txt", 9);
    let record = api!("CeWriteRecordProps", handle, 0, 4, PROPS);
    assert_ne!(record, 0);
    assert_eq!(api!("CeUnmountDBVol", GUID), 0);
    assert_eq!(api!("CloseHandle", handle), 1);
    assert_eq!(api!("CeUnmountDBVol", GUID), 1);
    assert_eq!(api!("CeMountDBVol", GUID, PATH, 3), 1);
    assert_eq!(cpu.read_mem(GUID, 16).unwrap(), first_guid);
    let handle = api!("CeOpenDatabaseEx2", GUID, OID, 0, SORT, 0, 0);
    assert_ne!(handle, INVALID_HANDLE);
    assert_eq!(api!("CeSeekDatabaseEx", handle, 2, 0, 0, 0), record);
    assert_eq!(api!("CloseHandle", handle), 1);
    assert_eq!(api!("CeUnmountDBVol", GUID), 1);
    assert!(!image.path().join("RecycleBin.vol").exists());
    assert_eq!(
        std::fs::read(image.path().join("untouched.txt")).unwrap(),
        b"original"
    );
    std::fs::write(device.path().join("corrupt.vol"), b"foreign database").unwrap();
    wide(&mut cpu, PATH, "\\corrupt.vol");
    assert_eq!(api!("CeMountDBVol", GUID, PATH, 3), 0);
    assert_eq!(kernel.thread_last_errors.get(&0), Some(&13));
    assert_eq!(
        std::fs::read(device.path().join("corrupt.vol")).unwrap(),
        b"foreign database"
    );
    // Keep the caller's stack intact after repeated six-argument API calls.
    assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
}
