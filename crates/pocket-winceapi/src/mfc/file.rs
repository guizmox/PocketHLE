//! CE3 ARM CFile: guest objects reference ordinary VFS descriptions.
use super::*;
use pocket_kernel::vfs::SeekKind;
const DATA: &str = "__PocketHLE_MFC_CFile_ClassData";
const METHODS: [&str; 20] = [
    "Runtime",
    "Delete",
    "Serialize",
    "AssertValid",
    "Dump",
    "Position",
    "Name",
    "Title",
    "Path",
    "SetPath",
    "Open",
    "Seek",
    "SetLength",
    "Length",
    "Read",
    "Write",
    "Abort",
    "Flush",
    "Close",
    "Buffer",
];
fn symbol(name: &str) -> String {
    format!("__PocketHLE_CFile_{name}")
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (97, "??0CFile@@QAA@XZ", construct as crate::Handler),
    (242, "??1CFile@@UAA@XZ", destruct),
    (854, "?Close@CFile@@UAAXXZ", close),
    (2063, "?Open@CFile@@UAAHPBGIPAVCFileException@@@Z", open),
    (2170, "?Rename@CFile@@SAXPBG0@Z", rename),
];

pub(super) fn register(d: &mut WinCeDispatcher) {
    let handlers: [crate::Handler; 20] = [
        runtime,
        deleting,
        unsupported,
        validate,
        unsupported,
        position,
        name,
        title,
        path,
        set_path,
        open,
        seek,
        set_length,
        length,
        read,
        write,
        abort,
        flush,
        close,
        buffer,
    ];
    for (name, handler) in METHODS.iter().zip(handlers) {
        d.register_handler("coredll.dll", &symbol(name), handler);
    }
    d.register_handler("coredll.dll", &symbol("BaseClass"), base_class);
    d.register_handler("coredll.dll", &symbol("RootClass"), root_class);
}
fn class_data(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let runtime = symbol("Runtime");
    let owner = ctx
        .kernel
        .dynamic_exports
        .iter()
        .find_map(|(h, m)| m.contains_key(&runtime).then_some(*h))
        .ok_or_else(|| KernelError::Loader("CFile virtual exports missing".into()))?;
    if let Some(p) = ctx.kernel.dynamic_exports[&owner].get(DATA) {
        return Ok(*p);
    }
    let exports = &ctx.kernel.dynamic_exports[&owner];
    let methods: Vec<_> = METHODS
        .iter()
        .map(|n| {
            exports
                .get(&symbol(n))
                .copied()
                .ok_or_else(|| KernelError::Loader(format!("CFile virtual method {n} missing")))
        })
        .collect::<Result<_, _>>()?;
    let base = *exports.get(&symbol("BaseClass")).unwrap();
    let root = *exports.get(&symbol("RootClass")).unwrap();
    // One DLL-lifetime allocation, rooted in the existing guest export map.
    // Heap bytes and this root already participate in native snapshots.
    let p = alloc(ctx, 160)?;
    let result = (|| {
        for (i, a) in methods.into_iter().enumerate() {
            put(ctx, p + i as u32 * 4, a)?;
        }
        ctx.cpu.write_mem(p + 128, b"CFile\0CObject\0")?;
        for (i, v) in [p + 128, 16, 0xffff, 0, base, 0].into_iter().enumerate() {
            put(ctx, p + 80 + i as u32 * 4, v)?;
        }
        for (i, v) in [p + 134, 4, 0xffff, 0, root, 0].into_iter().enumerate() {
            put(ctx, p + 104 + i as u32 * 4, v)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        ctx.kernel.heap.free(p);
        return Err(e);
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(DATA.into(), p);
    Ok(p)
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(o, 16, pocket_cpu::Prot::WRITE)?;
    let table = class_data(ctx)?;
    new_string(ctx, o + 12, &[])?;
    put(ctx, o, table)?;
    put(ctx, o + 4, u32::MAX)?;
    put(ctx, o + 8, 0)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(class_data(ctx)? + 80))
}
fn base_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(class_data(ctx)? + 104))
}
fn root_class(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn unsupported(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Err(KernelError::Loader(
        "CFile archive/debug serialization is not implemented".into(),
    ))
}
fn validate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let h = word(ctx, o + 4)?;
    if h != u32::MAX && !ctx.kernel.vfs.is_open(h) {
        return Err(KernelError::Loader("CFile contains a closed handle".into()));
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn handle(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let o = ctx.arg_u32(0)?;
    let h = word(ctx, o + 4)?;
    if !ctx.kernel.vfs.is_open(h) {
        return Err(KernelError::Loader(
            "CFile operation on a closed file".into(),
        ));
    }
    Ok(h)
}
fn io_error(operation: &str) -> KernelError {
    KernelError::Loader(format!("CFile::{operation} failed"))
}
fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, p, mode, error) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    );
    if word(ctx, o + 4)? != u32::MAX {
        return Err(KernelError::Loader("CFile::Open on an open object".into()));
    }
    ctx.cpu
        .check_guest_access(o, 16, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let text = strings::wide(ctx, p)?;
    let path = String::from_utf16_lossy(&text);
    let access = match mode & 3 {
        0 => 1,
        1 => 2,
        2 => 3,
        _ => return Err(io_error("Open mode")),
    };
    let share = match mode & 0x70 {
        0 | 0x10 => 0,
        0x20 => 1,
        0x30 => 2,
        0x40 => 3,
        _ => return Err(io_error("Open sharing mode")),
    };
    let disposition = if mode & 0x1000 != 0 {
        if mode & 0x2000 != 0 {
            4
        } else {
            2
        }
    } else {
        3
    };
    // Prepare the instance-owned name before acquiring a host handle.
    strings::replace(ctx, o + 12, &text)?;
    match ctx
        .kernel
        .vfs
        .open_file(&path, access, share, disposition, false, 0x80)
    {
        Ok(open) => {
            put(ctx, o + 4, open.handle)?;
            put(ctx, o + 8, 1)?;
            Ok(DispatchOutcome::ReturnedR0(1))
        }
        Err(code) => {
            crate::coredll::set_thread_error(ctx, code);
            if error != 0 {
                let cause = match code {
                    2 => 2,
                    3 => 3,
                    5 => 5,
                    32 => 11,
                    33 => 12,
                    112 => 13,
                    _ => 1,
                };
                put(ctx, error + 4, cause)?;
                put(ctx, error + 8, code)?;
                strings::replace(ctx, error + 12, &text)?;
            }
            Ok(DispatchOutcome::ReturnedR0(0))
        }
    }
}
fn close_impl(ctx: &mut CallCtx<'_>, o: u32) -> Result<(), KernelError> {
    let h = word(ctx, o + 4)?;
    if h != u32::MAX && !ctx.kernel.vfs.close(h) {
        return Err(io_error("Close"));
    }
    put(ctx, o + 4, u32::MAX)?;
    put(ctx, o + 8, 0)?;
    strings::replace(ctx, o + 12, &[])
}
fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    close_impl(ctx, o)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn abort(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let h = word(ctx, o + 4)?;
    if h != u32::MAX {
        ctx.kernel.vfs.close(h);
    }
    put(ctx, o + 4, u32::MAX)?;
    put(ctx, o + 8, 0)?;
    strings::replace(ctx, o + 12, &[])?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    if word(ctx, o + 8)? != 0 {
        close_impl(ctx, o)?;
    }
    release_string(ctx, o + 12)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    destruct(ctx)?;
    if flags & 1 != 0 {
        ctx.kernel.heap.free(o);
    }
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn position(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    let p = ctx
        .kernel
        .vfs
        .position(h)
        .ok_or_else(|| io_error("GetPosition"))?;
    Ok(DispatchOutcome::ReturnedR0(
        u32::try_from(p).map_err(|_| io_error("GetPosition overflow"))?,
    ))
}
fn length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    let n = ctx
        .kernel
        .vfs
        .size(h)
        .ok_or_else(|| io_error("GetLength"))?;
    Ok(DispatchOutcome::ReturnedR0(
        u32::try_from(n).map_err(|_| io_error("GetLength overflow"))?,
    ))
}
fn seek(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    let off = ctx.arg_u32(1)? as i32 as i64;
    let kind = match ctx.arg_u32(2)? {
        0 => SeekKind::Begin,
        1 => SeekKind::Current,
        2 => SeekKind::End,
        _ => return Err(io_error("Seek origin")),
    };
    let p = ctx
        .kernel
        .vfs
        .seek(h, off, kind)
        .ok_or_else(|| io_error("Seek"))?;
    Ok(DispatchOutcome::ReturnedR0(
        u32::try_from(p).map_err(|_| io_error("Seek overflow"))?,
    ))
}
fn set_length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    let n = ctx.arg_u32(1)?;
    ctx.kernel
        .vfs
        .seek(h, n as i64, SeekKind::Begin)
        .ok_or_else(|| io_error("SetLength"))?;
    let ok = ctx.kernel.vfs.set_end_of_file(h);
    if !ok {
        return Err(io_error("SetLength"));
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn flush(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    ctx.kernel.vfs.flush(h).map_err(|_| io_error("Flush"))?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn read(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    let dst = ctx.arg_u32(1)?;
    let n = ctx.arg_u32(2)?;
    ctx.cpu
        .check_guest_access(dst, n, pocket_cpu::Prot::WRITE)?;
    let mut buffer = vec![0; n.min(65536) as usize];
    let mut total = 0;
    while total < n {
        let count = (n - total).min(buffer.len() as u32) as usize;
        let got = ctx
            .kernel
            .vfs
            .read(h, &mut buffer[..count])
            .ok_or_else(|| io_error("Read"))?;
        ctx.cpu.write_mem(dst + total, &buffer[..got])?;
        total += got as u32;
        if got < count {
            break;
        }
    }
    Ok(DispatchOutcome::ReturnedR0(total))
}
fn write(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    let src = ctx.arg_u32(1)?;
    let n = ctx.arg_u32(2)?;
    ctx.cpu.check_guest_access(src, n, pocket_cpu::Prot::READ)?;
    let mut total = 0;
    while total < n {
        let count = (n - total).min(65536);
        let buffer = ctx.cpu.read_mem(src + total, count)?;
        if ctx.kernel.vfs.write(h, &buffer) != Some(count as usize) {
            return Err(io_error("Write"));
        }
        total += count;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn path(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(0)?;
    let o = ctx.arg_u32(1)?;
    let text = strings::value(ctx, o + 12)?;
    new_string(ctx, out, &text)?;
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn name_part(ctx: &mut CallCtx<'_>, title: bool) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(0)?;
    let o = ctx.arg_u32(1)?;
    let text = String::from_utf16_lossy(&strings::value(ctx, o + 12)?);
    let name = text.rsplit(['\\', '/']).next().unwrap_or("");
    let name = if title {
        name.rsplit_once('.').map_or(name, |p| p.0)
    } else {
        name
    };
    new_string(ctx, out, &name.encode_utf16().collect::<Vec<_>>())?;
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn name(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    name_part(ctx, false)
}
fn title(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    name_part(ctx, true)
}
fn set_path(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    let text = strings::wide(ctx, p)?;
    strings::replace(ctx, o + 12, &text)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn rename(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let a = ctx.arg_u32(0)?;
    let b = ctx.arg_u32(1)?;
    let a = String::from_utf16_lossy(&strings::wide(ctx, a)?);
    let b = String::from_utf16_lossy(&strings::wide(ctx, b)?);
    ctx.kernel
        .vfs
        .rename(&a, &b)
        .map_err(|_| io_error("Rename"))?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if ctx.arg_u32(1)? != 0 {
        return Err(KernelError::Loader(
            "CFile does not expose a direct buffer".into(),
        ));
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{Cpu, Prot};
    fn returned(out: DispatchOutcome) -> u32 {
        match out {
            DispatchOutcome::ReturnedR0(value) => value,
            _ => panic!("unexpected outcome"),
        }
    }
    #[test]
    fn file_reads_from_start_seeks_and_releases_its_owned_resources() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.bin"), [0, 0, 0, 87, 1, 2, 3, 4]).unwrap();
        kernel.vfs.mount("\\", dir.path());
        let exports = METHODS
            .iter()
            .chain(["BaseClass", "RootClass"].iter())
            .enumerate()
            .map(|(i, name)| (symbol(name), 0x70000100 + i as u32 * 32))
            .collect();
        kernel.dynamic_exports.insert(1, exports);
        let object = 0x2000;
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:97",
            &[object],
        );
        let class_free = kernel.heap.free_bytes();
        let path: Vec<u8> = "\\index.bin"
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x2100, &path).unwrap();
        assert_eq!(
            returned(call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "mfcce300.dll",
                "ord:2063",
                &[object, 0x2100, 0x22, 0]
            )),
            1
        );
        let h = cpu.read_u32_le(object + 4).unwrap();
        assert_eq!(kernel.vfs.position(h), Some(0));
        assert_eq!(
            returned(call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                &symbol("Read"),
                &[object, 0x2200, 4]
            )),
            4
        );
        assert_eq!(cpu.read_mem(0x2200, 4).unwrap(), [0, 0, 0, 87]);
        assert_eq!(
            returned(call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                &symbol("Length"),
                &[object]
            )),
            8
        );
        assert_eq!(kernel.vfs.position(h), Some(4));
        assert_eq!(
            returned(call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                &symbol("Seek"),
                &[object, 1, 0]
            )),
            1
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            &symbol("SetLength"),
            &[object, 3],
        );
        assert_eq!(kernel.vfs.position(h), Some(3));
        assert_eq!(kernel.vfs.size(h), Some(3));
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:242",
            &[object],
        );
        assert!(!kernel.vfs.is_open(h));
        // The class table has DLL lifetime; the instance's CString does not.
        assert!(kernel.heap.free_bytes() > class_free);
    }
}
