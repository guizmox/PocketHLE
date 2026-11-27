//! One continuation-based stream for ordinary and guest-derived CFile objects.
//! Every virtual operation resumes once; guest buffers/cursors remain the ABI
//! authority, including direct-buffer files and snapshots taken inside Read.
use super::super::callbacks::Frame;
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(in crate::mfc) const READ: &str = "__PocketHLE_MFC_ArchiveRead";
pub(in crate::mfc) const WRITE: &str = "__PocketHLE_MFC_ArchiveWrite";
pub(in crate::mfc) const FLUSH: &str = "__PocketHLE_MFC_ArchiveFlush";
const REFILL: &str = "__PocketHLE_MFC_ArchiveRefill";
pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        (READ, read as crate::Handler),
        (WRITE, write),
        (FLUSH, flush),
        (REFILL, refill),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn file(ctx: &mut CallCtx<'_>, a: u32) -> Result<u32, KernelError> {
    let file = word(ctx, a + FILE)?;
    if file == 0 {
        return Err(error("closed archive"));
    }
    ctx.cpu.check_guest_access(file, 4, Prot::READ)?;
    Ok(file)
}
fn pointers(ctx: &mut CallCtx<'_>, a: u32) -> Result<(u32, u32, u32, u32), KernelError> {
    if word(ctx, a + 8)? == 0 {
        return buffer(ctx, a);
    }
    let (base, cur, end, capacity) = (
        word(ctx, a + BASE)?,
        word(ctx, a + CURRENT)?,
        word(ctx, a + END)?,
        word(ctx, a + CAPACITY)?,
    );
    if cur < base || end < cur || end - base > capacity {
        return Err(error("invalid direct buffer"));
    }
    Ok((base, cur, end, capacity))
}
pub(super) fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let file = f.arg(ctx, 1)?;
    let mode = f.arg(ctx, 2)?;
    if f.fresh {
        let requested = f.arg(ctx, 3)? as i32;
        let supplied = ctx.arg_u32(4)?;
        ctx.cpu.check_guest_access(a, SIZE, Prot::WRITE)?;
        if supplied != 0 && requested >= 128 {
            ctx.cpu
                .check_guest_access(supplied, requested as u32, Prot::READ | Prot::WRITE)?;
        }
        ctx.cpu.write_mem(a, &[0; SIZE as usize])?;
        for (offset, value) in [
            (4, 1),
            (12, u32::MAX),
            (MODE, mode),
            (USER_BUFFER, 1),
            (CAPACITY, requested.max(128) as u32),
            (FILE, file),
            (BASE, if requested >= 128 { supplied } else { 0 }),
            (60, if mode & 1 != 0 { 64 } else { 16 }),
            (64, 137),
        ] {
            put(ctx, a + offset, value)?;
        }
        // Hidden CString result is constructed by the actual virtual method.
        return f.call_virtual(ctx, 1, file, 8, &[a + 16]);
    }
    if f.phase(ctx)? == 1 && word(ctx, a + BASE)? == 0 {
        return f.call_virtual(ctx, 2, file, 19, &[3, 0, 0, 0]);
    }
    let mut base = word(ctx, a + BASE)?;
    let capacity = word(ctx, a + CAPACITY)?;
    let direct = f.phase(ctx)? == 2 && ctx.cpu.read_reg(ArmReg::R0)? != 0;
    if direct {
        put(ctx, a + 8, 1)?;
    } else if base == 0 {
        base = alloc(ctx, capacity)?;
        put(ctx, a + BASE, base)?;
        put(ctx, a + USER_BUFFER, 0)?;
    }
    let end = if direct {
        0
    } else {
        base.checked_add(capacity)
            .ok_or_else(|| error("buffer address overflow"))?
    };
    put(ctx, a + END, end)?;
    put(ctx, a + CURRENT, if mode & 1 != 0 { end } else { base })?;
    f.finish(ctx, a)
}
pub(super) fn read(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let dst = f.arg(ctx, 1)?;
    let count = f.arg(ctx, 2)?;
    if f.fresh {
        let checked = (|| {
            mutable_archive(ctx, a)?;
            if !loading(ctx, a)? {
                return Err(fault(ctx, a, 2));
            }
            file(ctx, a)?;
            ctx.cpu.check_guest_access(dst, count, Prot::WRITE)?;
            if word(ctx, a + 8)? == 0 {
                let (base, _, _, capacity) = pointers(ctx, a)?;
                ctx.cpu
                    .check_guest_access(base, capacity, Prot::READ | Prot::WRITE)?;
            }
            Ok::<_, KernelError>(())
        })();
        if let Err(error) = checked {
            f.finish(ctx, 0)?;
            return Err(error);
        }
    }
    let mut copied = f.get(ctx, 0)?;
    if !f.fresh && ctx.cpu.read_reg(ArmReg::R0)? == 0 {
        return f.finish(ctx, copied);
    }
    while copied < count {
        let (_, cur, end, _) = pointers(ctx, a)?;
        if end == cur {
            f.set(ctx, 0, copied)?;
            return f.call_service(ctx, 1, REFILL, &[a, 0]);
        }
        let n = (end - cur).min(count - copied).min(65536);
        let bytes = ctx.cpu.read_mem(cur, n)?;
        ctx.cpu.write_mem(dst + copied, &bytes)?;
        put(ctx, a + CURRENT, cur + n)?;
        copied += n;
    }
    f.finish(ctx, copied)
}
pub(super) fn write(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let src = f.arg(ctx, 1)?;
    let count = f.arg(ctx, 2)?;
    if f.fresh {
        let checked = (|| {
            mutable_archive(ctx, a)?;
            if loading(ctx, a)? {
                return Err(fault(ctx, a, 4));
            }
            file(ctx, a)?;
            ctx.cpu.check_guest_access(src, count, Prot::READ)?;
            if word(ctx, a + 8)? == 0 {
                let (base, _, _, capacity) = pointers(ctx, a)?;
                ctx.cpu
                    .check_guest_access(base, capacity, Prot::READ | Prot::WRITE)?;
            }
            Ok::<_, KernelError>(())
        })();
        if let Err(error) = checked {
            f.finish(ctx, 0)?;
            return Err(error);
        }
    }
    let mut copied = f.get(ctx, 0)?;
    while copied < count {
        let (_, cur, end, _) = pointers(ctx, a)?;
        if cur == end {
            f.set(ctx, 0, copied)?;
            return f.call_service(ctx, 1, FLUSH, &[a]);
        }
        let n = (end - cur).min(count - copied).min(65536);
        let bytes = ctx.cpu.read_mem(src + copied, n)?;
        ctx.cpu.write_mem(cur, &bytes)?;
        put(ctx, a + CURRENT, cur + n)?;
        copied += n;
    }
    f.finish(ctx, 0)
}
pub(super) fn fill_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    if f.fresh {
        let (_, cur, end, capacity) = pointers(ctx, a)?;
        let required = (end - cur)
            .checked_add(f.arg(ctx, 1)?)
            .filter(|n| *n <= capacity)
            .ok_or_else(|| error("fill exceeds archive buffer capacity"))?;
        return f.call_service(ctx, 1, REFILL, &[a, required]);
    }
    f.finish(ctx, 0)
}
fn refill(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let required = f.arg(ctx, 1)?;
    mutable_archive(ctx, a)?;
    if !loading(ctx, a)? {
        return Err(fault(ctx, a, 2));
    }
    let file = file(ctx, a)?;
    // GetBufferPtr writes the new range while CURRENT still denotes the old
    // range; publish its matching cursor before validating the complete tuple.
    if !f.fresh && f.phase(ctx)? == 2 && word(ctx, a + 8)? != 0 {
        let base = word(ctx, a + BASE)?;
        put(ctx, a + CURRENT, base)?;
    }
    let (base, cur, end, capacity) = pointers(ctx, a)?;
    if required > capacity {
        return Err(error("fill exceeds archive buffer capacity"));
    }
    if word(ctx, a + 8)? != 0 {
        if f.fresh && end > cur {
            return f.call_virtual(ctx, 1, file, 11, &[(cur as i64 - end as i64) as u32, 1]);
        }
        if f.fresh || f.phase(ctx)? == 1 {
            return f.call_virtual(ctx, 2, file, 19, &[0, capacity, a + BASE, a + END]);
        }
        let base = word(ctx, a + BASE)?;
        let end = word(ctx, a + END)?;
        if end < base || end - base > capacity {
            return Err(error("invalid direct read buffer"));
        }
        ctx.cpu.check_guest_access(base, end - base, Prot::READ)?;
        put(ctx, a + CURRENT, base)?;
        if end - base < required {
            return Err(fault(ctx, a, 3));
        }
        return f.finish(ctx, end - base);
    }
    ctx.cpu
        .check_guest_access(base, capacity, Prot::READ | Prot::WRITE)?;
    let mut available = if f.fresh {
        let unread = end - cur;
        let bytes = ctx.cpu.read_mem(cur, unread)?;
        ctx.cpu.write_mem(base, &bytes)?;
        put(ctx, a + CURRENT, base)?;
        put(ctx, a + END, base + unread)?;
        unread
    } else {
        end - base
    };
    if !f.fresh {
        let n = ctx.cpu.read_reg(ArmReg::R0)?;
        let requested = f.get(ctx, 0)?;
        if n > requested {
            return Err(error("file Read exceeded its supplied buffer"));
        }
        available += n;
        put(ctx, a + END, base + available)?;
        if n == 0 || available >= required {
            if available < required {
                return Err(fault(ctx, a, 3));
            }
            return f.finish(ctx, available);
        }
    }
    if available == capacity {
        return f.finish(ctx, available);
    }
    let n = (capacity - available).min(65536);
    f.set(ctx, 0, n)?;
    f.call_virtual(ctx, 3, file, 14, &[base + available, n])
}
pub(super) fn flush(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    mutable_archive(ctx, a)?;
    let file = file(ctx, a)?;
    // GetBufferPtr writes the new range while CURRENT still denotes the old
    // range; publish its matching cursor before validating the complete tuple.
    if !f.fresh && f.phase(ctx)? == 2 && word(ctx, a + 8)? != 0 {
        let base = word(ctx, a + BASE)?;
        put(ctx, a + CURRENT, base)?;
    }
    let (base, cur, end, capacity) = pointers(ctx, a)?;
    if loading(ctx, a)? {
        if f.fresh && cur != end {
            return f.call_virtual(ctx, 1, file, 11, &[(cur as i64 - end as i64) as u32, 1]);
        }
        put(ctx, a + CURRENT, base)?;
        put(ctx, a + END, base)?;
    } else if word(ctx, a + 8)? != 0 {
        if f.fresh && cur != base {
            return f.call_virtual(ctx, 1, file, 19, &[2, cur - base, 0, 0]);
        }
        if f.fresh || f.phase(ctx)? == 1 {
            return f.call_virtual(ctx, 2, file, 19, &[1, capacity, a + BASE, a + END]);
        }
        let new_base = word(ctx, a + BASE)?;
        put(ctx, a + CURRENT, new_base)?;
        let (base, _, end, _) = pointers(ctx, a)?;
        if end == base {
            return Err(error("file returned an empty direct write buffer"));
        }
        ctx.cpu.check_guest_access(base, end - base, Prot::WRITE)?;
        put(ctx, a + CURRENT, base)?;
    } else {
        if f.fresh && cur != base {
            ctx.cpu.check_guest_access(base, cur - base, Prot::READ)?;
            return f.call_virtual(ctx, 1, file, 15, &[base, cur - base]);
        }
        put(ctx, a + CURRENT, base)?;
        put(ctx, a + END, base + capacity)?;
    }
    f.finish(ctx, 0)
}
pub(super) fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    mutable_archive(ctx, a)?;
    if f.fresh && word(ctx, a + FILE)? != 0 && word(ctx, a + MODE)? & 2 == 0 {
        return f.call_service(ctx, 1, FLUSH, &[a]);
    }
    super::archive_graph::release(ctx, a)?;
    let base = word(ctx, a + BASE)?;
    if word(ctx, a + USER_BUFFER)? == 0 && base != 0 {
        ctx.kernel.heap.free(base);
    }
    for off in [FILE, CURRENT, END, BASE] {
        put(ctx, a + off, 0)?;
    }
    release_string(ctx, a + 16)?;
    f.finish(ctx, a)
}
pub(super) fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, FLUSH, &[a]);
    }
    put(ctx, a + FILE, 0)?;
    f.finish(ctx, 0)
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;
    use crate::mfc::window_dispatch::tests::{drive_with, entry};
    use pocket_cpu::Cpu;
    fn args(f: &mut Fixture, values: &[u32]) {
        for (i, &v) in values.iter().enumerate() {
            f.cpu.write_arg(i as u8, v).unwrap();
        }
    }
    fn derived(f: &mut Fixture, slots: &[(u32, u32)]) {
        let v = f.word(0x2000);
        let bytes = f.cpu.read_mem(v, 80).unwrap();
        f.cpu.write_mem(0x2100, &bytes).unwrap();
        for &(slot, pc) in slots {
            f.cpu
                .write_mem(0x2100 + slot * 4, &pc.to_le_bytes())
                .unwrap();
        }
        f.cpu.write_mem(0x2000, &0x2100u32.to_le_bytes()).unwrap();
    }
    #[test]
    fn derived_file_short_reads_and_writes_use_real_virtual_methods_once_per_transfer() {
        let data: Vec<u8> = (0..511).map(|i| (i * 37) as u8).collect();
        let mut f = Fixture::new(&[]);
        derived(&mut f, &[(14, 0x6100), (15, 0x6200), (11, 0x6300)]);
        let handle = f.handle();
        let baseline = f.kernel.heap.free_bytes();
        for _ in 0..3 {
            f.cpu.write_mem(0x3000, &data).unwrap();
            f.construct(0, 0);
            args(&mut f, &[0x2300, 0x3000, 511]);
            let mut writes = 0;
            drive_with(
                &mut f.cpu,
                &mut f.kernel,
                &mut f.dispatcher,
                entry(2582),
                2048,
                |pc, cpu, k, _| {
                    assert_eq!(pc, 0x6200);
                    writes += 1;
                    let p = cpu.read_arg(1).unwrap();
                    let n = cpu.read_arg(2).unwrap();
                    let bytes = cpu.read_mem(p, n).unwrap();
                    assert_eq!(k.vfs.write(handle, &bytes), Some(n as usize));
                    0
                },
            );
            // Three full buffers have reached CFile; closing commits the tail.
            assert_eq!(writes, 3);
            args(&mut f, &[0x2300]);
            drive_with(
                &mut f.cpu,
                &mut f.kernel,
                &mut f.dispatcher,
                entry(851),
                128,
                |pc, cpu, k, _| {
                    assert_eq!(pc, 0x6200);
                    let p = cpu.read_arg(1).unwrap();
                    let n = cpu.read_arg(2).unwrap();
                    let bytes = cpu.read_mem(p, n).unwrap();
                    assert_eq!(k.vfs.write(handle, &bytes), Some(n as usize));
                    0
                },
            );
            f.call("ord:212", &[0x2300]);
            assert_eq!(
                std::fs::read(f.directory.path().join("archive.bin")).unwrap(),
                data
            );
            f.kernel
                .vfs
                .seek(handle, 0, pocket_kernel::vfs::SeekKind::Begin)
                .unwrap();
            f.construct(1, 0);
            args(&mut f, &[0x2300, 0x3500, 600]);
            let mut reads = 0;
            assert_eq!(
                drive_with(
                    &mut f.cpu,
                    &mut f.kernel,
                    &mut f.dispatcher,
                    entry(2135),
                    4096,
                    |pc, cpu, k, _| {
                        assert_eq!(pc, 0x6100);
                        reads += 1;
                        let p = cpu.read_arg(1).unwrap();
                        let n = cpu.read_arg(2).unwrap().min(17);
                        let mut bytes = vec![0; n as usize];
                        let n = k.vfs.read(handle, &mut bytes).unwrap();
                        cpu.write_mem(p, &bytes[..n]).unwrap();
                        n as u32
                    }
                ),
                511
            );
            assert_eq!(reads, 32); // 31 nonempty short reads, then genuine EOF.
            assert_eq!(f.cpu.read_mem(0x3500, 511).unwrap(), data);
            f.call("ord:212", &[0x2300]);
            f.kernel
                .vfs
                .seek(handle, 0, pocket_kernel::vfs::SeekKind::Begin)
                .unwrap();
            assert_eq!(f.kernel.heap.free_bytes(), baseline);
        }
    }
    #[test]
    fn direct_file_buffer_is_borrowed_and_unread_bytes_are_rewound_through_virtual_seek() {
        let mut f = Fixture::new(b"abcdefghijklmnopqrstuvwxyz");
        derived(&mut f, &[(19, 0x6100), (11, 0x6200)]);
        let data = b"abcdefghijklmnopqrstuvwxyz";
        f.cpu.write_mem(0x3000, data).unwrap();
        let sp = f.cpu.read_reg(ArmReg::Sp).unwrap();
        f.cpu
            .write_mem(sp + f.cpu.stack_arg_offset(), &0u32.to_le_bytes())
            .unwrap();
        args(&mut f, &[0x2300, 0x2000, 1, 128]);
        assert_eq!(
            drive_with(
                &mut f.cpu,
                &mut f.kernel,
                &mut f.dispatcher,
                entry(38),
                128,
                |pc, cpu, _, _| {
                    assert_eq!(pc, 0x6100);
                    assert_eq!(cpu.read_arg(1).unwrap(), 3);
                    1
                }
            ),
            0x2300
        );
        assert_eq!(f.word(0x2308), 1);
        let baseline = f.kernel.heap.free_bytes();
        args(&mut f, &[0x2300, 0x3500, 5]);
        assert_eq!(
            drive_with(
                &mut f.cpu,
                &mut f.kernel,
                &mut f.dispatcher,
                entry(2135),
                128,
                |pc, cpu, _, _| {
                    assert_eq!(pc, 0x6100);
                    assert_eq!(cpu.read_arg(1).unwrap(), 0);
                    let start = cpu.read_arg(3).unwrap();
                    let sp = cpu.read_reg(ArmReg::Sp).unwrap();
                    let end = cpu.read_u32_le(sp + cpu.stack_arg_offset()).unwrap();
                    cpu.write_mem(start, &0x3000u32.to_le_bytes()).unwrap();
                    cpu.write_mem(end, &0x301au32.to_le_bytes()).unwrap();
                    26
                }
            ),
            5
        );
        assert_eq!(f.cpu.read_mem(0x3500, 5).unwrap(), b"abcde");
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
        args(&mut f, &[0x2300]);
        let mut seeks = 0;
        drive_with(
            &mut f.cpu,
            &mut f.kernel,
            &mut f.dispatcher,
            entry(851),
            128,
            |pc, cpu, _, _| {
                assert_eq!(pc, 0x6200);
                seeks += 1;
                assert_eq!(cpu.read_arg(1).unwrap() as i32, -21);
                assert_eq!(cpu.read_arg(2).unwrap(), 1);
                5
            },
        );
        assert_eq!(seeks, 1);
        f.call("ord:212", &[0x2300]);
        assert_eq!(f.cpu.read_mem(0x3000, 26).unwrap(), data);
    }
}
