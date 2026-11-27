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
    (
        1469,
        "?GetRuntimeClass@CFile@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (1418, "?GetPosition@CFile@@UBAKXZ", position),
    (1292, "?GetFileName@CFile@@UBA?AVCString@@XZ", name),
    (1296, "?GetFileTitle@CFile@@UBA?AVCString@@XZ", title),
    (1294, "?GetFilePath@CFile@@UBA?AVCString@@XZ", path),
    (2346, "?SetFilePath@CFile@@UAAXPBG@Z", set_path),
    (2251, "?Seek@CFile@@UAAJJI@Z", seek),
    (2368, "?SetLength@CFile@@UAAXK@Z", set_length),
    (1340, "?GetLength@CFile@@UBAKXZ", length),
    (2584, "?Write@CFile@@UAAXPBXI@Z", write),
    (551, "?Abort@CFile@@UAAXXZ", abort),
    (1168, "?Flush@CFile@@UAAXXZ", flush),
    (1231, "?GetBufferPtr@CFile@@UAAIIIPAPAX0@Z", buffer),
    (97, "??0CFile@@QAA@XZ", construct as crate::Handler),
    (96, "??0CFile@@QAA@PBGI@Z", construct_path),
    (242, "??1CFile@@UAA@XZ", destruct),
    (2137, "?Read@CFile@@UAAIPAXI@Z", read),
    (854, "?Close@CFile@@UAAXXZ", close),
    (2063, "?Open@CFile@@UAAHPBGIPAVCFileException@@@Z", open),
    (2220, "?Rename@CFile@@SAXPBG0@Z", rename),
    (2170, "?Remove@CFile@@SAXPBG@Z", remove_file),
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
    for (name, handler) in [
        ("__PocketHLE_MFC_FileConstruct", construct as crate::Handler),
        ("__PocketHLE_MFC_FileOpen", open),
        ("__PocketHLE_MFC_FileDestruct", destruct),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
pub(super) fn class_data(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
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
    // One DLL-lifetime allocation, rooted in the existing guest export map.
    // Heap bytes and this root already participate in native snapshots.
    let p = alloc(ctx, 112)?;
    let result = (|| {
        for (i, a) in methods.into_iter().enumerate() {
            put(ctx, p + i as u32 * 4, a)?;
        }
        ctx.cpu.write_mem(p + 104, b"CFile\0")?;
        for (i, v) in [p + 104, 16, 0xffff, 0, base, 0].into_iter().enumerate() {
            put(ctx, p + 80 + i as u32 * 4, v)?;
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
#[cfg(test)]
pub(super) fn install_test_exports(kernel: &mut pocket_kernel::KernelState) {
    use pocket_kernel::Dispatcher;
    let exports = WinCeDispatcher::new()
        .dynamic_names("coredll.dll")
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n, 0x70010000 + i as u32 * 32))
        .collect();
    kernel.dynamic_exports.insert(1, exports);
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
fn construct_path(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let filename = ctx.arg_u32(1)?;
    let mode = ctx.arg_u32(2)?;
    construct(ctx)?;
    // The retail constructor calls the base Open implementation, not a
    // derived virtual, and throws CFileException when opening fails.
    let opened = (|| {
        ctx.cpu.write_arg(0, object)?;
        ctx.cpu.write_arg(1, filename)?;
        ctx.cpu.write_arg(2, mode)?;
        ctx.cpu.write_arg(3, 0)?;
        open(ctx)
    })();
    let error = match opened {
        Ok(DispatchOutcome::ReturnedR0(value)) if value != 0 => {
            return Ok(DispatchOutcome::ReturnedR0(object));
        }
        Ok(DispatchOutcome::ReturnedR0(0)) => {
            let os = ctx
                .kernel
                .thread_last_errors
                .get(&ctx.kernel.current_thread)
                .copied()
                .unwrap_or(0);
            failure(ctx, object, os)
        }
        Err(error) => error,
        _ => KernelError::Dispatch("Unexpected CFile constructor Open continuation".into()),
    };
    // A failed constructor has no live CFile owner. Capture the exception's
    // path before retiring its initialized CString and any acquired handle.
    let handle = word(ctx, object + 4)?;
    if handle != u32::MAX {
        ctx.kernel.vfs.close(handle);
        put(ctx, object + 4, u32::MAX)?;
        put(ctx, object + 8, 0)?;
    }
    release_string(ctx, object + 12)?;
    put(ctx, object + 12, 0)?;
    Err(error)
}
pub(super) fn base_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CObject",
    )?))
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
        return Err(failure(ctx, o, 6));
    }
    Ok(h)
}
pub(super) fn failure(ctx: &mut CallCtx<'_>, file: u32, os: u32) -> KernelError {
    match strings::value(ctx, file + 12) {
        Ok(path) => super::io_exceptions::fault(
            super::io_exceptions::FILE,
            if os == u32::MAX {
                1
            } else {
                super::io_exceptions::os_cause(os)
            },
            os,
            path,
        ),
        Err(error) => error,
    }
}
fn failure_for_call(ctx: &mut CallCtx<'_>) -> KernelError {
    match ctx.arg_u32(0) {
        Ok(file) => failure(ctx, file, u32::MAX),
        Err(error) => error,
    }
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
    if error != 0 {
        ctx.cpu
            .check_guest_access(error, 20, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
        strings::validate_assignment_target(ctx, error + 16)?;
    }
    let text = strings::wide(ctx, p)?;
    let path = ctx
        .kernel
        .vfs
        .full_guest_path(&String::from_utf16_lossy(&text));
    let text: Vec<_> = path.encode_utf16().collect();
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
            log::debug!("CFile::Open({path:?}, mode=0x{mode:x}) failed: WinCE error {code}");
            crate::coredll::set_thread_error(ctx, code);
            if error != 0 {
                let cause = super::io_exceptions::os_cause(code);
                put(ctx, error + 8, cause)?;
                put(ctx, error + 12, code)?;
                strings::replace(ctx, error + 16, &text)?;
            }
            Ok(DispatchOutcome::ReturnedR0(0))
        }
    }
}
pub(super) fn close_impl(ctx: &mut CallCtx<'_>, o: u32) -> Result<(), KernelError> {
    let h = word(ctx, o + 4)?;
    if h != u32::MAX && !ctx.kernel.vfs.close(h) {
        return Err(failure(ctx, o, 6));
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
pub(super) fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
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
        .ok_or_else(|| failure_for_call(ctx))?;
    Ok(DispatchOutcome::ReturnedR0(
        u32::try_from(p).map_err(|_| failure_for_call(ctx))?,
    ))
}
fn length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    let n = ctx
        .kernel
        .vfs
        .size(h)
        .ok_or_else(|| failure_for_call(ctx))?;
    Ok(DispatchOutcome::ReturnedR0(
        u32::try_from(n).map_err(|_| failure_for_call(ctx))?,
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
        .ok_or_else(|| failure_for_call(ctx))?;
    Ok(DispatchOutcome::ReturnedR0(
        u32::try_from(p).map_err(|_| failure_for_call(ctx))?,
    ))
}
fn set_length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    let n = ctx.arg_u32(1)?;
    ctx.kernel
        .vfs
        .seek(h, n as i64, SeekKind::Begin)
        .ok_or_else(|| failure_for_call(ctx))?;
    let ok = ctx.kernel.vfs.set_end_of_file(h);
    if !ok {
        return Err(failure_for_call(ctx));
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn flush(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = handle(ctx)?;
    ctx.kernel.vfs.flush(h).map_err(|_| failure_for_call(ctx))?;
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
            .ok_or_else(|| failure_for_call(ctx))?;
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
            return Err(failure_for_call(ctx));
        }
        total += count;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn path(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let text = strings::value(ctx, o + 12)?;
    new_string(ctx, out, &text)?;
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn name_part(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let text = String::from_utf16_lossy(&strings::value(ctx, o + 12)?);
    let name = text.rsplit(['\\', '/']).next().unwrap_or("");
    new_string(ctx, out, &name.encode_utf16().collect::<Vec<_>>())?;
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn name(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    name_part(ctx)
}
fn title(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    name_part(ctx)
}
fn set_path(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    let text = strings::wide(ctx, p)?;
    strings::replace(ctx, o + 12, &text)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn remove_file(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let path = ctx.arg_u32(0)?;
    let path = String::from_utf16_lossy(&strings::wide(ctx, path)?);
    ctx.kernel.vfs.delete(&path).map_err(|os| {
        super::io_exceptions::fault(
            super::io_exceptions::FILE,
            super::io_exceptions::os_cause(os),
            os,
            path.encode_utf16().collect(),
        )
    })?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn rename(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let a = ctx.arg_u32(0)?;
    let b = ctx.arg_u32(1)?;
    let a = String::from_utf16_lossy(&strings::wide(ctx, a)?);
    let b = String::from_utf16_lossy(&strings::wide(ctx, b)?);
    ctx.kernel.vfs.rename(&a, &b).map_err(|os| {
        super::io_exceptions::fault(
            super::io_exceptions::FILE,
            super::io_exceptions::os_cause(os),
            os,
            a.encode_utf16().collect(),
        )
    })?;
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
    fn filename_constructor_opens_real_vfs_data_and_destructor_releases_ownership() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), b"data").unwrap();
        kernel.vfs.mount("\\", dir.path());
        install_test_exports(&mut kernel);
        let bytes: Vec<_> = "\\data.bin"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x2100, &bytes).unwrap();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:97",
            &[0x2000],
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:242",
            &[0x2000],
        );
        let baseline = kernel.heap.free_bytes();
        let handles = kernel.vfs.open_handles();
        for _ in 0..32 {
            assert_eq!(
                returned(call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "mfcce300.dll",
                    "ord:96",
                    &[0x2000, 0x2100, 0x40]
                )),
                0x2000
            );
            let handle = cpu.read_u32_le(0x2004).unwrap();
            assert_eq!(cpu.read_u32_le(0x2008).unwrap(), 1);
            assert_eq!(kernel.vfs.position(handle), Some(0));
            assert_eq!(
                returned(call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "mfcce300.dll",
                    "ord:2137",
                    &[0x2000, 0x2300, 4]
                )),
                4
            );
            assert_eq!(cpu.read_mem(0x2300, 4).unwrap(), b"data");
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "mfcce300.dll",
                "ord:242",
                &[0x2000],
            );
            assert!(!kernel.vfs.is_open(handle));
            assert_eq!(kernel.vfs.open_handles(), handles);
            assert_eq!(kernel.heap.free_bytes(), baseline);
        }
    }

    #[test]
    fn failed_filename_constructor_preserves_typed_error_and_retires_its_initialized_string() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        kernel.vfs.mount("\\", dir.path());
        install_test_exports(&mut kernel);
        let bytes: Vec<_> = "\\missing.bin"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x2100, &bytes).unwrap();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:97",
            &[0x2000],
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:242",
            &[0x2000],
        );
        let baseline = kernel.heap.free_bytes();
        let handles = kernel.vfs.open_handles();
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x70000000,
            iat_va: 0,
            dll: "mfcce300.dll".into(),
            binding: pocket_pe::ImportBinding::Ordinal(96),
            friendly_name: None,
        };
        for _ in 0..32 {
            for (i, value) in [0x2000, 0x2100, 0x40].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            let mut ctx = CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            };
            let error = construct_path(&mut ctx).unwrap_err();
            match error {
                KernelError::RuntimeException {
                    runtime,
                    cause,
                    os_error,
                    path,
                } => {
                    assert_eq!(runtime, super::super::io_exceptions::FILE);
                    assert_eq!((cause, os_error), (2, 2));
                    assert_eq!(String::from_utf16(&path).unwrap(), "\\missing.bin");
                }
                other => panic!("Unexpected constructor error: {other:?}"),
            }
            assert_eq!(cpu.read_u32_le(0x2004).unwrap(), u32::MAX);
            assert_eq!(cpu.read_u32_le(0x200c).unwrap(), 0);
            assert_eq!(kernel.vfs.open_handles(), handles);
            assert_eq!(kernel.heap.free_bytes(), baseline);
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
        install_test_exports(&mut kernel);
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
        let path_out = 0x2400;
        assert_eq!(
            returned(call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                &symbol("Path"),
                &[object, path_out]
            )),
            path_out
        );
        let text = cpu.read_u32_le(path_out).unwrap();
        assert_eq!(cpu.read_mem(text, path.len() as u32).unwrap(), path);
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:297",
            &[path_out],
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

#[cfg(test)]
mod operation_ordinal_tests {
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn retail_remove_and_rename_ordinals_operate_on_the_distinct_real_vfs_files() {
        let (mut cpu, mut k, mut d) = setup();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("old.bin"), b"keep data").unwrap();
        std::fs::write(directory.path().join("remove.bin"), b"delete data").unwrap();
        k.vfs.mount("\\", directory.path());
        for (address, path) in [
            (0x1100, r"\old.bin"),
            (0x1200, r"\renamed.bin"),
            (0x1300, r"\remove.bin"),
        ] {
            let bytes: Vec<_> = path
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            cpu.write_mem(address, &bytes).unwrap();
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2220",
            &[0x1100, 0x1200],
        );
        assert!(!directory.path().join("old.bin").exists());
        assert_eq!(
            std::fs::read(directory.path().join("renamed.bin")).unwrap(),
            b"keep data"
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2170",
            &[0x1300],
        );
        assert!(!directory.path().join("remove.bin").exists());
        assert!(directory.path().join("renamed.bin").is_file());
    }
}

fn buffer(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Native base CFile supports no direct-buffer commands.
    Ok(DispatchOutcome::ReturnedR0(0))
}
