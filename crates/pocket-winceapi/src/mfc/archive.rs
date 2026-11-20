//! CE3 CArchive owns a guest buffer over an ordinary CFile/VFS description.
//! Buffer pointers remain guest-visible: compiled inline stream operators and
//! snapshots must observe exactly the same cursor as out-of-line Read/Write.
use super::*;
use pocket_kernel::vfs::SeekKind;

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (38, "??0CArchive@@QAA@PAVCFile@@IHPAX@Z", construct),
    (212, "??1CArchive@@QAA@XZ", destruct),
    (2135, "?Read@CArchive@@QAAIPAXI@Z", read),
    (2142, "?ReadCount@CArchive@@QAAKXZ", read_count),
    (2582, "?Write@CArchive@@QAAXPBXI@Z", write),
    (2588, "?WriteCount@CArchive@@QAAXK@Z", write_count),
];
const SIZE: u32 = 68;
const MODE: u32 = 20;
const USER_BUFFER: u32 = 24;
const CAPACITY: u32 = 28;
const FILE: u32 = 32;
const CURRENT: u32 = 36;
const END: u32 = 40;
const BASE: u32 = 44;

fn error(detail: &str) -> KernelError {
    KernelError::Loader(format!("MFC CArchive: {detail}"))
}
pub(super) fn loading(ctx: &mut CallCtx<'_>, archive: u32) -> Result<bool, KernelError> {
    Ok(word(ctx, archive + MODE)? & 1 != 0)
}
fn file_handle(ctx: &mut CallCtx<'_>, archive: u32) -> Result<u32, KernelError> {
    let file = word(ctx, archive + FILE)?;
    if file == 0 {
        return Err(error("closed archive"));
    }
    file::archive_handle(ctx, file)
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
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let archive = ctx.arg_u32(0)?;
    let file = ctx.arg_u32(1)?;
    let mode = ctx.arg_u32(2)?;
    let requested = ctx.arg_u32(3)? as i32;
    let supplied = ctx.arg_u32(4)?;
    ctx.cpu
        .check_guest_access(archive, SIZE, pocket_cpu::Prot::WRITE)?;
    file::archive_handle(ctx, file)?;
    let text = strings::value(ctx, file + 12)?;
    let capacity = requested.max(128) as u32;
    let user = supplied != 0 && requested >= 128;
    let base = if user {
        supplied
    } else {
        alloc(ctx, capacity)?
    };
    let result: Result<(), KernelError> = (|| {
        let end = base
            .checked_add(capacity)
            .ok_or_else(|| error("buffer address overflow"))?;
        ctx.cpu.check_guest_access(
            base,
            capacity,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )?;
        ctx.cpu.write_mem(archive, &[0; SIZE as usize])?;
        new_string(ctx, archive + 16, &text)?;
        for (offset, value) in [
            (4, 1),
            (12, u32::MAX),
            (MODE, mode),
            (USER_BUFFER, u32::from(user)),
            (CAPACITY, capacity),
            (FILE, file),
            (CURRENT, if mode & 1 != 0 { end } else { base }),
            (END, end),
            (BASE, base),
            (60, if mode & 1 != 0 { 64 } else { 16 }),
            (64, 137),
        ] {
            put(ctx, archive + offset, value)?;
        }
        Ok(())
    })();
    if result.is_err() && !user {
        ctx.kernel.heap.free(base);
    }
    result?;
    Ok(DispatchOutcome::ReturnedR0(archive))
}

pub(super) fn read_bytes(
    ctx: &mut CallCtx<'_>,
    archive: u32,
    dst: u32,
    count: u32,
) -> Result<u32, KernelError> {
    if !loading(ctx, archive)? {
        return Err(error("read on a storing archive"));
    }
    let handle = file_handle(ctx, archive)?;
    if count == 0 {
        return Ok(0);
    }
    ctx.cpu
        .check_guest_access(dst, count, pocket_cpu::Prot::WRITE)?;
    let mut copied = 0;
    while copied < count {
        let (base, current, end, capacity) = buffer(ctx, archive)?;
        let available = end - current;
        if available != 0 {
            let n = available.min(count - copied).min(65536);
            let bytes = ctx.cpu.read_mem(current, n)?;
            ctx.cpu.write_mem(dst + copied, &bytes)?;
            put(ctx, archive + CURRENT, current + n)?;
            copied += n;
            continue;
        }
        // Refill the same guest allocation; inline operators can consume it
        // after this call. VFS cursor includes unread bytes until Flush/close.
        let mut bytes = vec![0; capacity.min(65536) as usize];
        let n = ctx
            .kernel
            .vfs
            .read(handle, &mut bytes)
            .ok_or_else(|| error("file read failed"))? as u32;
        ctx.cpu.write_mem(base, &bytes[..n as usize])?;
        put(ctx, archive + CURRENT, base)?;
        put(ctx, archive + END, base + n)?;
        if n == 0 {
            break;
        }
    }
    Ok(copied)
}
pub(super) fn write_bytes(
    ctx: &mut CallCtx<'_>,
    archive: u32,
    src: u32,
    count: u32,
) -> Result<(), KernelError> {
    if loading(ctx, archive)? {
        return Err(error("write on a loading archive"));
    }
    file_handle(ctx, archive)?;
    if count == 0 {
        return Ok(());
    }
    ctx.cpu
        .check_guest_access(src, count, pocket_cpu::Prot::READ)?;
    let mut copied = 0;
    while copied < count {
        let (_, current, end, _) = buffer(ctx, archive)?;
        let available = end - current;
        if available == 0 {
            flush(ctx, archive)?;
            continue;
        }
        let n = available.min(count - copied).min(65536);
        let bytes = ctx.cpu.read_mem(src + copied, n)?;
        ctx.cpu.write_mem(current, &bytes)?;
        put(ctx, archive + CURRENT, current + n)?;
        copied += n;
    }
    Ok(())
}
fn flush(ctx: &mut CallCtx<'_>, archive: u32) -> Result<(), KernelError> {
    let handle = file_handle(ctx, archive)?;
    let (base, current, end, capacity) = buffer(ctx, archive)?;
    if loading(ctx, archive)? {
        ctx.kernel
            .vfs
            .seek(handle, -i64::from(end - current), SeekKind::Current)
            .ok_or_else(|| error("file rewind failed"))?;
        put(ctx, archive + CURRENT, base)?;
        put(ctx, archive + END, base)?;
    } else {
        let mut written = 0;
        while written < current - base {
            let n = (current - base - written).min(65536);
            let bytes = ctx.cpu.read_mem(base + written, n)?;
            if ctx.kernel.vfs.write(handle, &bytes) != Some(n as usize) {
                return Err(error("file write failed"));
            }
            written += n;
        }
        put(ctx, archive + CURRENT, base)?;
        put(ctx, archive + END, base + capacity)?;
    }
    Ok(())
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let archive = ctx.arg_u32(0)?;
    if word(ctx, archive + FILE)? != 0 && word(ctx, archive + MODE)? & 2 == 0 {
        flush(ctx, archive)?;
    }
    let base = word(ctx, archive + BASE)?;
    if word(ctx, archive + USER_BUFFER)? == 0 && base != 0 {
        ctx.kernel.heap.free(base);
    }
    for offset in [FILE, CURRENT, END, BASE] {
        put(ctx, archive + offset, 0)?;
    }
    release_string(ctx, archive + 16)?;
    Ok(DispatchOutcome::ReturnedR0(archive))
}
fn read(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (archive, dst, n) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    Ok(DispatchOutcome::ReturnedR0(read_bytes(
        ctx, archive, dst, n,
    )?))
}
fn write(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (archive, src, n) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    write_bytes(ctx, archive, src, n)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
pub(super) fn read_size(ctx: &mut CallCtx<'_>, archive: u32) -> Result<u32, KernelError> {
    // Use a temporary guest slot so the ordinary buffered read is the single
    // cursor implementation, including EOF and inline-operator interoperability.
    let slot = alloc(ctx, 4)?;
    let result = (|| {
        if read_bytes(ctx, archive, slot, 2)? != 2 {
            return Err(error("truncated count"));
        }
        let n = u32::from(ctx.cpu.read_u16_le(slot)?);
        if n != 0xffff {
            return Ok(n);
        }
        if read_bytes(ctx, archive, slot, 4)? != 4 {
            return Err(error("truncated wide count"));
        }
        word(ctx, slot)
    })();
    ctx.kernel.heap.free(slot);
    result
}
pub(super) fn write_size(ctx: &mut CallCtx<'_>, archive: u32, n: u32) -> Result<(), KernelError> {
    let slot = alloc(ctx, 6)?;
    let result = (|| {
        if n < 0xffff {
            ctx.cpu.write_mem(slot, &(n as u16).to_le_bytes())?;
            write_bytes(ctx, archive, slot, 2)
        } else {
            ctx.cpu.write_mem(slot, &[0xff, 0xff])?;
            put(ctx, slot + 2, n)?;
            write_bytes(ctx, archive, slot, 6)
        }
    })();
    ctx.kernel.heap.free(slot);
    result
}
fn read_count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let archive = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(read_size(ctx, archive)?))
}
fn write_count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (archive, n) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    write_size(ctx, archive, n)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{stub::StubCpu, Cpu, Prot};
    use pocket_kernel::KernelState;

    struct Fixture {
        cpu: StubCpu,
        kernel: KernelState,
        dispatcher: WinCeDispatcher,
        directory: tempfile::TempDir,
    }
    impl Fixture {
        fn new(bytes: &[u8]) -> Self {
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
        fn call(&mut self, name: &str, args: &[u32]) -> u32 {
            let DispatchOutcome::ReturnedR0(value) = call(
                &mut self.cpu,
                &mut self.kernel,
                &mut self.dispatcher,
                "mfcce300.dll",
                name,
                args,
            ) else {
                panic!("unexpected result");
            };
            value
        }
        fn word(&mut self, at: u32) -> u32 {
            self.cpu.read_u32_le(at).unwrap()
        }
        fn handle(&mut self) -> u32 {
            self.word(0x2004)
        }
        fn construct(&mut self, mode: u32, supplied: u32) {
            assert_eq!(
                self.call("ord:38", &[0x2300, 0x2000, mode, 128, supplied]),
                0x2300
            );
        }
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
        call(
            &mut f.cpu,
            &mut f.kernel,
            &mut f.dispatcher,
            "coredll.dll",
            SERIALIZE,
            &[0x2500, 0x2300],
        );
        f.call("ord:212", &[0x2300]);
        let stored = std::fs::read(f.directory.path().join("archive.bin")).unwrap();
        assert_eq!(&stored[..2], &300u16.to_le_bytes());
        assert_eq!(&stored[2..], data);
        let handle = f.handle();
        f.kernel.vfs.seek(handle, 0, SeekKind::Begin).unwrap();
        f.construct(1, 0);
        call(
            &mut f.cpu,
            &mut f.kernel,
            &mut f.dispatcher,
            "coredll.dll",
            SERIALIZE,
            &[0x2600, 0x2300],
        );
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
}
