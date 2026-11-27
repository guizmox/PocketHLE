//! CE3 CArchive owns a guest buffer over an ordinary CFile/VFS description.
//! Buffer pointers remain guest-visible: compiled inline stream operators and
//! snapshots must observe exactly the same cursor as out-of-line Read/Write.
use super::*;
#[cfg(test)]
use pocket_kernel::vfs::SeekKind;
#[path = "archive_codec.rs"]
mod codec;
#[path = "archive_stream.rs"]
mod stream;
use codec::{read_count, read_string, write_count, write_string};
pub(super) use codec::{READ_COUNT, READ_STRING, WRITE_COUNT, WRITE_STRING};
use stream::{close, construct, destruct, fill_buffer, flush as flush_call, read, write};
pub(super) use stream::{READ, WRITE};

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (550, "?Abort@CArchive@@QAAXXZ", abort),
    (851, "?Close@CArchive@@QAAXXZ", close),
    (1166, "?Flush@CArchive@@QAAXXZ", flush_call),
    (38, "??0CArchive@@QAA@PAVCFile@@IHPAX@Z", construct),
    (212, "??1CArchive@@QAA@XZ", destruct),
    (2135, "?Read@CArchive@@QAAIPAXI@Z", read),
    (1150, "?FillBuffer@CArchive@@QAAXI@Z", fill_buffer),
    (2142, "?ReadCount@CArchive@@QAAKXZ", read_count),
    (335, "??5@YAAAVCArchive@@AAV0@AAVCString@@@Z", read_string),
    (338, "??6@YAAAVCArchive@@AAV0@ABVCString@@@Z", write_string),
    (2582, "?Write@CArchive@@QAAXPBXI@Z", write),
    (2588, "?WriteCount@CArchive@@QAAXK@Z", write_count),
];
pub(super) const CONSTRUCT: &str = "__PocketHLE_MFC_ArchiveConstruct";
pub(super) const DESTRUCT: &str = "__PocketHLE_MFC_ArchiveDestruct";
pub(super) const CLOSE: &str = "__PocketHLE_MFC_ArchiveClose";
pub(super) const ABORT: &str = "__PocketHLE_MFC_ArchiveAbort";
pub(super) fn register(d: &mut WinCeDispatcher) {
    stream::register(d);
    codec::register(d);
    for (name, handler) in [
        (CONSTRUCT, construct as crate::Handler),
        (DESTRUCT, destruct),
        (CLOSE, close),
        (ABORT, abort),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
const SIZE: u32 = 68;
const MODE: u32 = 20;
const USER_BUFFER: u32 = 24;
const CAPACITY: u32 = 28;
const FILE: u32 = 32;
const CURRENT: u32 = 36;
const END: u32 = 40;
const BASE: u32 = 44;

pub(super) fn error(detail: &str) -> KernelError {
    KernelError::Loader(format!("MFC CArchive: {detail}"))
}
pub(super) fn fault(ctx: &mut CallCtx<'_>, archive: u32, cause: u32) -> KernelError {
    match strings::value(ctx, archive + 16) {
        Ok(path) => super::io_exceptions::fault(super::io_exceptions::ARCHIVE, cause, 0, path),
        Err(e) => e,
    }
}
fn mutable_archive(ctx: &mut CallCtx<'_>, archive: u32) -> Result<(), KernelError> {
    ctx.cpu.check_guest_access(
        archive,
        SIZE,
        pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
    )?;
    Ok(())
}
pub(super) fn loading(ctx: &mut CallCtx<'_>, archive: u32) -> Result<bool, KernelError> {
    Ok(word(ctx, archive + MODE)? & 1 != 0)
}
fn buffer(ctx: &mut CallCtx<'_>, archive: u32) -> Result<(u32, u32, u32, u32), KernelError> {
    let base = word(ctx, archive + BASE)?;
    let capacity = word(ctx, archive + CAPACITY)?;
    let current = word(ctx, archive + CURRENT)?;
    let end = word(ctx, archive + END)?;
    let limit = base
        .checked_add(capacity)
        .ok_or_else(|| error("buffer address overflow"))?;
    if base == 0 || capacity < 128 || current < base || current > end || end > limit {
        return Err(error("invalid buffer pointers"));
    }
    Ok((base, current, end, capacity))
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::bluetooth::tests::setup;
    use pocket_cpu::{stub::StubCpu, Cpu, Prot};
    use pocket_kernel::KernelState;

    pub(in crate::mfc) struct Fixture {
        pub(in crate::mfc) cpu: StubCpu,
        pub(in crate::mfc) kernel: KernelState,
        pub(in crate::mfc) dispatcher: WinCeDispatcher,
        pub(in crate::mfc) directory: tempfile::TempDir,
    }
    impl Fixture {
        pub(in crate::mfc) fn new(bytes: &[u8]) -> Self {
            let (mut cpu, mut kernel, dispatcher) = setup();
            cpu.map_region(0x50000000, 0x200000, Prot::READ | Prot::WRITE)
                .unwrap();
            let directory = tempfile::tempdir().unwrap();
            std::fs::write(directory.path().join("archive.bin"), bytes).unwrap();
            kernel.vfs.mount("\\", directory.path());
            // CString is already a normal guest object; a base CFile is enough
            // for testing buffered archive I/O without a synthetic class table.
            let handle = kernel
                .vfs
                .open(
                    "\\archive.bin",
                    pocket_kernel::vfs::Access::ReadWrite,
                    false,
                )
                .unwrap();
            file::install_test_exports(&mut kernel);
            let mut fixture = Self {
                cpu,
                kernel,
                dispatcher,
                directory,
            };
            fixture.call("ord:97", &[0x2000]);
            fixture
                .cpu
                .write_mem(0x2004, &handle.to_le_bytes())
                .unwrap();
            fixture
        }
        pub(in crate::mfc) fn call(&mut self, name: &str, args: &[u32]) -> u32 {
            use super::super::window_dispatch::tests::{drive_with, entry};
            use pocket_cpu::regs::ArmReg;
            let sp = self.cpu.read_reg(ArmReg::Sp).unwrap();
            for (i, value) in args.iter().enumerate() {
                if i < 4 {
                    self.cpu.write_arg(i as u8, *value).unwrap();
                } else {
                    self.cpu
                        .write_mem(
                            sp + self.cpu.stack_arg_offset() + (i as u32 - 4) * 4,
                            &value.to_le_bytes(),
                        )
                        .unwrap();
                }
            }
            let start = if let Some(ordinal) = name.strip_prefix("ord:") {
                entry(ordinal.parse().unwrap())
            } else {
                pocket_kernel::Thunk {
                    thunk_va: 0x70000000,
                    iat_va: 0,
                    dll: "coredll.dll".into(),
                    binding: pocket_pe::ImportBinding::Name(name.into()),
                    friendly_name: None,
                }
            };
            drive_with(
                &mut self.cpu,
                &mut self.kernel,
                &mut self.dispatcher,
                start,
                8192,
                |pc, _, _, _| panic!("unexpected guest callback {pc:x}"),
            )
        }
        pub(in crate::mfc) fn word(&mut self, at: u32) -> u32 {
            self.cpu.read_u32_le(at).unwrap()
        }
        pub(in crate::mfc) fn handle(&mut self) -> u32 {
            self.word(0x2004)
        }
        pub(in crate::mfc) fn construct(&mut self, mode: u32, supplied: u32) {
            assert_eq!(
                self.call("ord:38", &[0x2300, 0x2000, mode, 128, supplied]),
                0x2300
            );
        }
    }
    fn failure(f: &mut Fixture, ordinal: u16, args: &[u32]) -> KernelError {
        use pocket_kernel::Dispatcher;
        for (i, value) in args.iter().enumerate() {
            f.cpu.write_arg(i as u8, *value).unwrap();
        }
        f.dispatcher
            .dispatch(
                &mut f.cpu,
                &super::super::window_dispatch::tests::entry(ordinal),
                &mut f.kernel,
            )
            .unwrap_err()
    }
    #[test]
    fn protected_archive_buffer_fails_before_refill_consumes_vfs_input() {
        let mut f = Fixture::new(&[1, 2, 3, 4]);
        f.construct(1, 0x3000);
        let handle = f.handle();
        let cursor = f.word(0x2300 + CURRENT);
        let free = f.kernel.heap.free_bytes();
        f.cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
        failure(&mut f, 2135, &[0x2300, 0x2800, 4]);
        assert_eq!(f.kernel.vfs.position(handle), Some(0));
        assert_eq!(f.word(0x2300 + CURRENT), cursor);
        assert_eq!(f.kernel.heap.free_bytes(), free);
        assert!(f.kernel.vfs.is_open(handle));
        f.cpu
            .protect_region(0x3000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        assert_eq!(f.call("ord:2135", &[0x2300, 0x2800, 4]), 4);
        assert_eq!(f.cpu.read_mem(0x2800, 4).unwrap(), [1, 2, 3, 4]);
    }
    #[test]
    fn string_input_checks_output_before_io_and_keeps_locked_storage_when_reusable() {
        let mut f = Fixture::new(&[0xff, 0xfe, 0xff, 1, 65, 0]);
        f.call("ord:181", &[0x3000]);
        f.call("ord:1233", &[0x3000, 8]);
        let locked = f.call("ord:1733", &[0x3000]);
        f.construct(1, 0);
        let handle = f.handle();
        let free = f.kernel.heap.free_bytes();
        f.cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
        failure(&mut f, 335, &[0x2300, 0x3000]);
        assert_eq!(f.kernel.vfs.position(handle), Some(0));
        assert_eq!(f.kernel.heap.free_bytes(), free);
        f.cpu
            .protect_region(0x3000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        assert_eq!(f.call("ord:335", &[0x2300, 0x3000]), 0x2300);
        assert_eq!(f.word(0x3000), locked);
        assert_eq!(f.word(locked - 12), u32::MAX);
        assert_eq!(f.word(locked - 8), 1);
        assert_eq!(f.cpu.read_mem(locked, 4).unwrap(), [65, 0, 0, 0]);
        assert_eq!(f.kernel.heap.free_bytes(), free);
    }
    #[test]
    fn archived_strings_decode_ansi_unicode_and_long_lengths_without_leaking_buffers() {
        for (prefix, text, expected) in [
            (vec![3], vec![b'A', 0xe9, b'Z'], vec![65u16, 233, 90]),
            (
                vec![0xff, 0xfe, 0xff, 3],
                vec![65, 0, 0x3d, 0xd8, 0x00, 0xde],
                vec![65, 0xd83d, 0xde00],
            ),
            (
                vec![0xff, 0xfe, 0xff, 0xff, 0xff, 0xff, 0, 1, 0, 0],
                vec![65, 0].repeat(256),
                vec![65; 256],
            ),
        ] {
            let mut bytes = prefix;
            bytes.extend_from_slice(&text);
            let mut f = Fixture::new(&bytes);
            let free = f.kernel.heap.free_bytes();
            f.call("ord:181", &[0x2500]);
            f.construct(1, 0);
            assert_eq!(f.call("ord:335", &[0x2300, 0x2500]), 0x2300);
            let pointer = f.word(0x2500);
            assert_eq!(f.word(pointer - 8), expected.len() as u32);
            assert_eq!(
                f.cpu.read_mem(pointer, expected.len() as u32 * 2).unwrap(),
                expected
                    .iter()
                    .flat_map(|ch| ch.to_le_bytes())
                    .collect::<Vec<_>>()
            );
            f.call("ord:212", &[0x2300]);
            let handle = f.handle();
            f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
            f.construct(0, 0);
            assert_eq!(f.call("ord:338", &[0x2300, 0x2500]), 0x2300);
            f.call("ord:212", &[0x2300]);
            let encoded = std::fs::read(f.directory.path().join("archive.bin")).unwrap();
            assert_eq!(&encoded[..3], &[0xff, 0xfe, 0xff]);
            f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
            f.construct(1, 0);
            assert_eq!(f.call("ord:335", &[0x2300, 0x2500]), 0x2300);
            f.call("ord:212", &[0x2300]);
            f.call("ord:297", &[0x2500]);
            assert_eq!(f.kernel.heap.free_bytes(), free);
        }
    }
    #[test]
    fn fill_buffer_preserves_unread_inline_data_and_keeps_the_real_file_cursor() {
        let bytes: Vec<u8> = (0..300).map(|i| i as u8).collect();
        let mut f = Fixture::new(&bytes);
        f.construct(1, 0);
        f.call("ord:2135", &[0x2300, 0x2800, 126]);
        let current = f.word(0x2300 + CURRENT);
        assert_eq!(f.cpu.read_mem(current, 2).unwrap(), bytes[126..128]);
        f.call("ord:1150", &[0x2300, 4]);
        let current = f.word(0x2300 + CURRENT);
        assert_eq!(f.cpu.read_mem(current, 8).unwrap(), bytes[126..134]);
        assert_eq!(f.word(0x2300 + END) - current, 128);
        let handle = f.handle();
        assert_eq!(f.kernel.vfs.position(handle), Some(254));
        // Consume four bytes inline; Read must start at the same cursor.
        f.cpu
            .write_mem(0x2300 + CURRENT, &(current + 4).to_le_bytes())
            .unwrap();
        f.call("ord:2135", &[0x2300, 0x2800, 3]);
        assert_eq!(f.cpu.read_mem(0x2800, 3).unwrap(), bytes[130..133]);
        f.call("ord:212", &[0x2300]);
        assert_eq!(f.kernel.vfs.position(handle), Some(133));
    }
    #[test]
    fn loading_shares_inline_cursor_rewinds_readahead_and_releases_owned_buffer() {
        let bytes: Vec<_> = (0..300).map(|i| i as u8).collect();
        let mut f = Fixture::new(&bytes);
        let free = f.kernel.heap.free_bytes();
        f.construct(1, 0);
        assert_eq!(f.call("ord:2135", &[0x2300, 0x2800, 3]), 3);
        assert_eq!(f.cpu.read_mem(0x2800, 3).unwrap(), bytes[..3]);
        let handle = f.handle();
        assert_eq!(f.kernel.vfs.position(handle), Some(128));
        let current = f.word(0x2300 + CURRENT);
        assert_eq!(f.cpu.read_mem(current, 1).unwrap(), [3]);
        // A compiled inline operator consumes one byte before the next Read.
        f.cpu
            .write_mem(0x2300 + CURRENT, &(current + 1).to_le_bytes())
            .unwrap();
        assert_eq!(f.call("ord:2135", &[0x2300, 0x2800, 126]), 126);
        assert_eq!(f.cpu.read_mem(0x2800, 126).unwrap(), bytes[4..130]);
        assert_eq!(f.kernel.vfs.position(handle), Some(256));
        f.call("ord:212", &[0x2300]);
        assert_eq!(f.kernel.vfs.position(handle), Some(130));
        assert!(f.kernel.vfs.is_open(handle));
        assert_eq!(f.kernel.heap.free_bytes(), free);
    }
    #[test]
    fn storing_flushes_and_count_encoding_round_trips_without_owning_caller_memory() {
        let mut f = Fixture::new(&[]);
        let free = f.kernel.heap.free_bytes();
        f.construct(0, 0x3000);
        for n in [0, 65534, 65535, u32::MAX] {
            f.call("ord:2588", &[0x2300, n]);
        }
        f.call("ord:212", &[0x2300]);
        assert_eq!(f.kernel.heap.free_bytes(), free);
        let expected = [
            0, 0, 254, 255, 255, 255, 255, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        assert_eq!(
            std::fs::read(f.directory.path().join("archive.bin")).unwrap(),
            expected
        );
        let handle = f.handle();
        f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
        f.construct(1, 0);
        for n in [0, 65534, 65535, u32::MAX] {
            assert_eq!(f.call("ord:2142", &[0x2300]), n);
        }
        assert_eq!(f.call("ord:2135", &[0x2300, 0x2800, 1]), 0);
        f.call("ord:212", &[0x2300]);
        assert_eq!(f.kernel.heap.free_bytes(), free);
        assert_eq!(f.cpu.read_mem(0x3000, 16).unwrap(), expected);
    }
    #[test]
    fn byte_array_virtual_serialize_round_trips_real_file_across_archive_buffers() {
        let mut f = Fixture::new(&[]);
        let free = f.kernel.heap.free_bytes();
        let data: Vec<_> = (0..300).map(|n| (n * 37) as u8).collect();
        f.cpu.write_mem(0x3000, &data).unwrap();
        for (offset, value) in [(4, 0x3000u32), (8, 300), (12, 300)] {
            f.cpu
                .write_mem(0x2500 + offset, &value.to_le_bytes())
                .unwrap();
        }
        f.construct(0, 0);
        f.call("__PocketHLE_MFC_Class:CByteArray:2", &[0x2500, 0x2300]);
        f.call("ord:212", &[0x2300]);
        let stored = std::fs::read(f.directory.path().join("archive.bin")).unwrap();
        assert_eq!(&stored[..2], &300u16.to_le_bytes());
        assert_eq!(&stored[2..], data);
        let handle = f.handle();
        f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
        f.construct(1, 0);
        f.call("__PocketHLE_MFC_Class:CByteArray:2", &[0x2600, 0x2300]);
        assert_eq!(f.word(0x2608), 300);
        let restored = f.word(0x2604);
        assert_eq!(f.cpu.read_mem(restored, 300).unwrap(), data);
        f.call("ord:212", &[0x2300]);
        f.kernel.heap.free(restored);
        assert_eq!(f.kernel.heap.free_bytes(), free);
    }

    #[test]
    fn multi_buffer_write_persists_every_byte_and_loading_returns_a_short_read_at_eof() {
        let bytes: Vec<_> = (0..700).map(|i| (i * 7) as u8).collect();
        let mut f = Fixture::new(&[]);
        f.cpu.write_mem(0x2800, &bytes).unwrap();
        f.construct(0, 0);
        f.call("ord:2582", &[0x2300, 0x2800, bytes.len() as u32]);
        f.call("ord:212", &[0x2300]);
        assert_eq!(
            std::fs::read(f.directory.path().join("archive.bin")).unwrap(),
            bytes
        );
        let handle = f.handle();
        f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
        f.construct(1, 0);
        assert_eq!(f.call("ord:2135", &[0x2300, 0x2800, 800]), 700);
        assert_eq!(f.cpu.read_mem(0x2800, 700).unwrap(), bytes);
        f.call("ord:212", &[0x2300]);
        assert_eq!(f.kernel.vfs.position(handle), Some(700));
    }
    #[test]
    fn word_array_growth_and_archive_round_trip_use_element_counts_not_byte_counts() {
        let mut f = Fixture::new(&[]);
        f.call("ord:207", &[0x2100]);
        let baseline = f.kernel.heap.free_bytes();
        f.call("ord:2311", &[0x2100, 2, 0x1234]);
        f.call("ord:2419", &[0x2100, 100, u32::MAX]);
        let data = f.word(0x2104);
        assert_eq!(f.cpu.read_mem(data, 6).unwrap(), [0, 0, 0, 0, 0x34, 0x12]);
        f.call("ord:2419", &[0x2100, 3, u32::MAX]);
        f.construct(0, 0);
        f.call("ord:2294", &[0x2100, 0x2300]);
        f.call("ord:212", &[0x2300]);
        assert_eq!(
            std::fs::read(f.directory.path().join("archive.bin")).unwrap(),
            [3, 0, 0, 0, 0, 0, 0x34, 0x12]
        );
        f.call("ord:2419", &[0x2100, 0, u32::MAX]);
        let handle = f.handle();
        f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
        f.construct(1, 0);
        f.call("ord:2294", &[0x2100, 0x2300]);
        f.call("ord:212", &[0x2300]);
        assert_eq!(f.word(0x2108), 3);
        let data = f.word(0x2104);
        assert_eq!(f.cpu.read_mem(data, 6).unwrap(), [0, 0, 0, 0, 0x34, 0x12]);
        f.call("ord:314", &[0x2100]);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
    }
}

fn abort(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let archive = ctx.arg_u32(0)?;
    mutable_archive(ctx, archive)?;
    super::archive_graph::release(ctx, archive)?;
    if word(ctx, archive + USER_BUFFER)? == 0 {
        let base = word(ctx, archive + BASE)?;
        if base != 0 {
            ctx.kernel.heap.free(base);
        }
        put(ctx, archive + BASE, 0)?;
        put(ctx, archive + CURRENT, 0)?;
    }
    put(ctx, archive + FILE, 0)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod container_tests {
    use super::tests::Fixture;
    use pocket_cpu::Cpu;
    use pocket_kernel::vfs::SeekKind;
    fn wide(f: &mut Fixture, p: u32, text: &str) {
        let bytes: Vec<_> = text
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        f.cpu.write_mem(p, &bytes).unwrap();
    }
    #[test]
    fn string_arrays_round_trip_unicode_elements_and_release_each_owned_string() {
        let mut f = Fixture::new(&[]);
        f.call("ord:182", &[0x2500]);
        let baseline = f.kernel.heap.free_bytes();
        let expected = ["alpha", "", "café 🚗"];
        for _ in 0..3 {
            for (i, text) in expected.iter().enumerate() {
                wide(&mut f, 0x3000, text);
                f.call("ord:2309", &[0x2500, i as u32, 0x3000]);
            }
            f.construct(0, 0);
            f.call("ord:2292", &[0x2500, 0x2300]);
            f.call("ord:212", &[0x2300]);
            f.call("ord:2417", &[0x2500, 0, u32::MAX]);
            let handle = f.handle();
            f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
            f.construct(1, 0);
            f.call("ord:2292", &[0x2500, 0x2300]);
            f.call("ord:212", &[0x2300]);
            assert_eq!(f.word(0x2508), 3);
            let data = f.word(0x2504);
            for (i, text) in expected.iter().enumerate() {
                let p = f.word(data + i as u32 * 4);
                let n = f.word(p - 8);
                let bytes = f.cpu.read_mem(p, n * 2).unwrap();
                let units: Vec<_> = bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                assert_eq!(String::from_utf16(&units).unwrap(), *text);
            }
            f.call("ord:2417", &[0x2500, 0, u32::MAX]);
            f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
            assert_eq!(f.kernel.heap.free_bytes(), baseline);
        }
        f.call("ord:298", &[0x2500]);
    }
    #[test]
    fn string_object_map_uses_archive_graph_and_releases_temporary_keys_on_every_load() {
        let mut f = Fixture::new(&[]);
        f.call("ord:123", &[0x2500, 2]);
        // The CE3 TRY context is process/thread metadata, not a serializer leak.
        f.call("ord:2649", &[]);
        let baseline = f.kernel.heap.free_bytes();
        for _ in 0..3 {
            for text in ["alpha", "café"] {
                wide(&mut f, 0x3000, text);
                let slot = f.call("ord:354", &[0x2500, 0x3000]);
                f.cpu.write_mem(slot, &0u32.to_le_bytes()).unwrap();
            }
            f.construct(0, 0);
            f.call("ord:2286", &[0x2500, 0x2300]);
            f.call("ord:212", &[0x2300]);
            let bytes = std::fs::read(f.directory.path().join("archive.bin")).unwrap();
            assert_eq!(&bytes[..2], &2u16.to_le_bytes());
            f.call("ord:2176", &[0x2500]);
            let handle = f.handle();
            f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
            f.construct(1, 0);
            f.call("ord:2286", &[0x2500, 0x2300]);
            f.call("ord:212", &[0x2300]);
            assert_eq!(f.word(0x250c), 2);
            for text in ["alpha", "café"] {
                wide(&mut f, 0x3000, text);
                f.cpu.write_mem(0x3100, &u32::MAX.to_le_bytes()).unwrap();
                assert_eq!(f.call("ord:1740", &[0x2500, 0x3000, 0x3100]), 1);
                assert_eq!(f.word(0x3100), 0);
            }
            f.call("ord:2176", &[0x2500]);
            f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
            assert_eq!(f.kernel.heap.free_bytes(), baseline);
        }
        f.call("ord:259", &[0x2500]);
    }
}
