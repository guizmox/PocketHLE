//! CMirrorFile stages replacement data in an ordinary VFS temporary file.
use super::callbacks::{service, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
const TABLE: &str = "__PocketHLE_MFC_MirrorFileTable";
const CONSTRUCT: &str = "__PocketHLE_MFC_MirrorFileConstruct";
const DELETE: &str = "__PocketHLE_MFC_MirrorFileDelete";
pub(super) const NEW: &str = CONSTRUCT;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2064,
        "?Open@CMirrorFile@@UAAHPBGIPAVCFileException@@@Z",
        open,
    ),
    (861, "?Close@CMirrorFile@@UAAXXZ", close),
    (554, "?Abort@CMirrorFile@@UAAXXZ", abort),
    (
        1575,
        "?GetTempName@CMirrorFile@@SA?AVCString@@PBGH@Z",
        temp_name,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        (CONSTRUCT, construct as crate::Handler),
        (DELETE, delete),
        ("__PocketHLE_MFC_MirrorFileOpen", open),
        ("__PocketHLE_MFC_MirrorFileClose", close),
        ("__PocketHLE_MFC_MirrorFileAbort", abort),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn table(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let (base, fresh) = super::application::allocation(ctx, TABLE, 80)?;
    if fresh {
        let parent = super::file::class_data(ctx)?;
        let bytes = ctx.cpu.read_mem(parent, 80)?;
        ctx.cpu.write_mem(base, &bytes)?;
        for (slot, name) in [
            (1, DELETE),
            (10, "__PocketHLE_MFC_MirrorFileOpen"),
            (16, "__PocketHLE_MFC_MirrorFileAbort"),
            (18, "__PocketHLE_MFC_MirrorFileClose"),
        ] {
            let target = service(ctx, name)?;
            put(ctx, base + slot * 4, target)?;
        }
    }
    Ok(base)
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, "__PocketHLE_MFC_FileConstruct", &[o]);
    }
    new_string(ctx, o + 16, &[])?;
    let v = table(ctx)?;
    put(ctx, o, v)?;
    f.finish(ctx, o)
}
fn parent(path: &str) -> &str {
    path.rsplit_once('\\')
        .map(|(p, _)| if p.is_empty() { "\\" } else { p })
        .unwrap_or("\\")
}
fn temp_name(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(out, 4, pocket_cpu::Prot::WRITE)?;
    let path = ctx.arg_u32(1)?;
    let keep = ctx.arg_u32(2)? != 0;
    let input = String::from_utf16_lossy(&strings::wide(ctx, path)?);
    let path = ctx.kernel.vfs.full_guest_path(&input);
    let (temp, _) = ctx
        .kernel
        .vfs
        .temporary_file(parent(&path), "~MF", 0)
        .map_err(|os| {
            super::io_exceptions::fault(
                super::io_exceptions::FILE,
                super::io_exceptions::os_cause(os),
                os,
                path.encode_utf16().collect(),
            )
        })?;
    if !keep {
        ctx.kernel.vfs.delete(&temp).map_err(|os| {
            super::io_exceptions::fault(
                super::io_exceptions::FILE,
                super::io_exceptions::os_cause(os),
                os,
                temp.encode_utf16().collect(),
            )
        })?;
    }
    if let Err(error) = new_string(ctx, out, &temp.encode_utf16().collect::<Vec<_>>()) {
        if keep {
            // No CString owner was published: retire the newly created VFS file.
            let _ = ctx.kernel.vfs.delete(&temp);
        }
        return Err(error);
    }
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let pointer = f.arg(ctx, 1)?;
        let units = strings::wide(ctx, pointer)?;
        let path = ctx
            .kernel
            .vfs
            .full_guest_path(&String::from_utf16_lossy(&units));
        let canonical =
            super::application::duplicate_wide(ctx, &path.encode_utf16().collect::<Vec<_>>())?;
        f.own(ctx, canonical)?;
        f.set(ctx, 0, canonical)?;
        strings::replace(ctx, o + 16, &[])?;
        let mode = f.arg(ctx, 2)?;
        let error = f.arg(ctx, 3)?;
        if mode & 0x1000 != 0
            && ctx
                .kernel
                .vfs
                .attributes(&path)
                .is_ok_and(|attrs| attrs & 0x11 == 0)
        {
            let (temp, _) = match ctx.kernel.vfs.temporary_file(parent(&path), "~MF", 0) {
                Ok(value) => value,
                Err(_) => {
                    return f.call_service(
                        ctx,
                        2,
                        "__PocketHLE_MFC_FileOpen",
                        &[o, canonical, mode, error],
                    )
                }
            };
            if let Err(error) =
                strings::replace(ctx, o + 16, &temp.encode_utf16().collect::<Vec<_>>())
            {
                let _ = ctx.kernel.vfs.delete(&temp);
                return Err(error);
            }
            let p = word(ctx, o + 16)?;
            return f.call_service(ctx, 1, "__PocketHLE_MFC_FileOpen", &[o, p, mode, error]);
        }
        return f.call_service(
            ctx,
            2,
            "__PocketHLE_MFC_FileOpen",
            &[o, canonical, mode, error],
        );
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 1 {
        if result != 0 {
            let pointer = f.get(ctx, 0)?;
            let units = strings::wide(ctx, pointer)?;
            strings::replace(ctx, o + 12, &units)?;
            return f.finish(ctx, result);
        }
        let temp = String::from_utf16_lossy(&strings::value(ctx, o + 16)?);
        let _ = ctx.kernel.vfs.delete(&temp);
        strings::replace(ctx, o + 16, &[])?;
        let path = f.get(ctx, 0)?;
        let mode = f.arg(ctx, 2)?;
        let error = f.arg(ctx, 3)?;
        return f.call_service(ctx, 2, "__PocketHLE_MFC_FileOpen", &[o, path, mode, error]);
    }
    f.finish(ctx, result)
}
fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let destination = String::from_utf16_lossy(&strings::value(ctx, o + 12)?);
    let temporary = String::from_utf16_lossy(&strings::value(ctx, o + 16)?);
    super::file::close_impl(ctx, o)?;
    if !temporary.is_empty() {
        ctx.kernel
            .vfs
            .delete(&destination)
            .and_then(|_| ctx.kernel.vfs.rename(&temporary, &destination))
            .map_err(|os| {
                super::io_exceptions::fault(
                    super::io_exceptions::FILE,
                    super::io_exceptions::os_cause(os),
                    os,
                    destination.encode_utf16().collect(),
                )
            })?;
        strings::replace(ctx, o + 16, &[])?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn abort(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let handle = word(ctx, o + 4)?;
    if handle != u32::MAX {
        ctx.kernel.vfs.close(handle);
    }
    put(ctx, o + 4, u32::MAX)?;
    put(ctx, o + 8, 0)?;
    strings::replace(ctx, o + 12, &[])?;
    let temporary = String::from_utf16_lossy(&strings::value(ctx, o + 16)?);
    if !temporary.is_empty() {
        ctx.kernel.vfs.delete(&temporary).map_err(|os| {
            super::io_exceptions::fault(
                super::io_exceptions::FILE,
                super::io_exceptions::os_cause(os),
                os,
                temporary.encode_utf16().collect(),
            )
        })?;
        strings::replace(ctx, o + 16, &[])?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    if flags & !1 != 0 {
        return Err(KernelError::Dispatch(
            "MFC mirror-file vector deletion".into(),
        ));
    }
    release_string(ctx, o + 16)?;
    super::file::destruct(ctx)?;
    if flags & 1 != 0 {
        ctx.kernel.heap.free(o);
    }
    Ok(DispatchOutcome::ReturnedR0(o))
}

#[cfg(test)]
mod tests {
    use super::super::{archive::tests::Fixture, window_dispatch::tests::entry};
    use super::*;
    use pocket_cpu::Cpu;
    #[test]
    fn temporary_name_oom_rolls_back_vfs_file_and_readonly_failure_keeps_output() {
        let mut f = Fixture::new(&[]);
        f.cpu
            .write_mem(
                0x2800,
                &"\\state.bin\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        f.cpu.write_mem(0x2900, &0xabcdefu32.to_le_bytes()).unwrap();
        let baseline = f.kernel.heap.free_bytes();
        let mut pressure = Vec::new();
        while let Some(p) = f.kernel.heap.alloc(1) {
            pressure.push(p);
        }
        for (i, value) in [0x2900, 0x2800, 1].into_iter().enumerate() {
            f.cpu.write_arg(i as u8, value).unwrap();
        }
        let thunk = entry(1575);
        let mut ctx = CallCtx {
            cpu: &mut f.cpu,
            kernel: &mut f.kernel,
            thunk: &thunk,
        };
        assert!(temp_name(&mut ctx).is_err());
        assert_eq!(ctx.cpu.read_u32_le(0x2900).unwrap(), 0xabcdef);
        assert_eq!(std::fs::read_dir(f.directory.path()).unwrap().count(), 1);
        for p in pressure {
            ctx.kernel.heap.free(p);
        }
        assert_eq!(ctx.kernel.heap.free_bytes(), baseline);
        let readonly = tempfile::tempdir().unwrap();
        ctx.kernel.vfs.mount_read_only("\\Locked", readonly.path());
        ctx.cpu
            .write_mem(
                0x2800,
                &"\\Locked\\state.bin\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert!(temp_name(&mut ctx).is_err());
        assert_eq!(ctx.cpu.read_u32_le(0x2900).unwrap(), 0xabcdef);
        assert_eq!(std::fs::read_dir(f.directory.path()).unwrap().count(), 1);
        assert_eq!(ctx.kernel.heap.free_bytes(), baseline);
        assert_eq!(std::fs::read_dir(readonly.path()).unwrap().count(), 0);
    }
}
