//! Windows CE file/anonymous section objects; not VirtualAlloc substitutes.
use super::*;
use pocket_kernel::file_mapping::FileMapping;

pub(super) fn register(d: &mut WinCeDispatcher) {
    let dll = "coredll.dll";
    for name in ["CreateFileForMapping", "CreateFileForMappingW"] {
        d.register_handler(dll, name, create_file_for_mapping);
    }
    for name in ["CreateFileMapping", "CreateFileMappingW"] {
        d.register_handler(dll, name, create_file_mapping);
    }
    d.register_handler(dll, "MapViewOfFile", map_view);
    d.register_handler(dll, "UnmapViewOfFile", unmap_view);
    d.register_handler(dll, "FlushViewOfFile", flush_view);
}
fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn create_file_for_mapping(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let result = create_file_w(ctx)?;
    if let DispatchOutcome::ReturnedR0(handle) = result {
        if ctx.kernel.vfs.is_open(handle) {
            ctx.kernel.vfs.mark_for_mapping(handle);
        }
    }
    Ok(result)
}
fn create_file_mapping(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let file = ctx.arg_u32(0)?;
    let consumed = ctx.kernel.vfs.is_for_mapping(file);
    let result = create_mapping_inner(ctx, file);
    // CE transfers CreateFileForMapping ownership even when section creation
    // fails. Ordinary CreateFile handles retain their independent lifetime.
    if consumed {
        close_owned_handle(ctx.kernel, file);
    }
    result
}
fn create_mapping_inner(ctx: &mut CallCtx<'_>, file: u32) -> Result<DispatchOutcome, KernelError> {
    let attributes = ctx.arg_u32(1)?;
    let flags = ctx.arg_u32(2)?;
    let high = ctx.arg_u32(3)?;
    let low = ctx.arg_u32(4)?;
    let name_ptr = ctx.arg_u32(5)?;
    if attributes != 0 || flags & !0x08000006 != 0 || !matches!(flags & !0x08000000, 2 | 4) {
        return fail(ctx, ERROR_INVALID_PARAMETER);
    }
    let writable = flags & !0x08000000 == 4;
    let name = if name_ptr == 0 {
        None
    } else {
        let mut units = Vec::new();
        let mut terminated = false;
        for index in 0..260u32 {
            let Some(address) = name_ptr.checked_add(index * 2) else {
                return fail(ctx, ERROR_INVALID_PARAMETER);
            };
            if ctx.cpu.check_guest_access(address, 2, Prot::READ).is_err() {
                return fail(ctx, ERROR_INVALID_PARAMETER);
            }
            let unit = ctx.cpu.read_u16_le(address)?;
            if unit == 0 {
                terminated = true;
                break;
            }
            units.push(unit);
        }
        if !terminated {
            return fail(ctx, ERROR_INVALID_PARAMETER);
        }
        Some(String::from_utf16_lossy(&units))
    };
    if let Some(object) = name
        .as_ref()
        .and_then(|n| ctx.kernel.object_handles.named_mapping(n))
    {
        if writable && !object.writable {
            return fail(ctx, 5);
        }
        let Some(handle) = ctx
            .kernel
            .object_handles
            .duplicate(HandleObject::Mapping(object.key))
        else {
            return fail(ctx, 8);
        };
        set_thread_error(ctx, 183);
        return Ok(DispatchOutcome::ReturnedR0(handle));
    }
    if high != 0 {
        return fail(ctx, ERROR_INVALID_PARAMETER);
    }
    let size = if low == 0 && file != INVALID_HANDLE_VALUE {
        match ctx
            .kernel
            .vfs
            .size(file)
            .and_then(|n| u32::try_from(n).ok())
        {
            Some(n) => n,
            None => return fail(ctx, ERROR_INVALID_HANDLE),
        }
    } else {
        low
    };
    if pocket_cpu::shared_memory::rounded_len(size).is_none() {
        return fail(ctx, ERROR_INVALID_PARAMETER);
    }
    let Some(handle) = ctx.kernel.object_handles.reserve() else {
        return fail(ctx, 8);
    };
    let object = if file == INVALID_HANDLE_VALUE {
        if !writable {
            return fail(ctx, ERROR_INVALID_PARAMETER);
        }
        FileMapping::anonymous(handle, size, ctx.kernel.memory_division.as_ref())
    } else {
        ctx.kernel
            .vfs
            .mapping_backing(file, size, writable)
            .map(|backing| FileMapping::file(handle, size, writable, backing))
    };
    match object {
        Ok(object) => {
            ctx.kernel
                .object_handles
                .insert_mapping(handle, name, object);
            set_thread_error(ctx, 0);
            Ok(DispatchOutcome::ReturnedR0(handle))
        }
        Err(error) => fail(ctx, error),
    }
}
fn map_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let access = ctx.arg_u32(1)?;
    let high = ctx.arg_u32(2)?;
    let offset = ctx.arg_u32(3)?;
    let len = ctx.arg_u32(4)?;
    if high != 0 {
        return fail(ctx, ERROR_INVALID_PARAMETER);
    }
    let Some(object) = ctx.kernel.object_handles.mapping(handle) else {
        return fail(ctx, ERROR_INVALID_HANDLE);
    };
    // The dispatcher and guest CPU slices share the device execution gate.
    let result = unsafe {
        ctx.kernel
            .mapped_views
            .map(ctx.cpu, object, access, offset, len)
    };
    match result {
        Ok(base) => Ok(DispatchOutcome::ReturnedR0(base)),
        Err(error) => fail(ctx, error),
    }
}
fn unmap_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let base = ctx.arg_u32(0)?;
    match ctx.kernel.mapped_views.unmap(ctx.cpu, base) {
        Ok(()) => Ok(DispatchOutcome::ReturnedR0(1)),
        Err(error) => fail(ctx, error),
    }
}
fn flush_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let base = ctx.arg_u32(0)?;
    let len = ctx.arg_u32(1)?;
    match ctx.kernel.mapped_views.flush(base, len) {
        Ok(()) => Ok(DispatchOutcome::ReturnedR0(1)),
        Err(error) => fail(ctx, error),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{fresh_kernel, scanf_cpu, thunk_at};
    use super::*;
    use pocket_cpu::Cpu;
    fn invoke(
        ctx: &mut CallCtx<'_>,
        args: &[u32],
        handler: fn(&mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError>,
    ) -> u32 {
        for (index, &value) in args.iter().enumerate() {
            if index < 4 {
                ctx.cpu
                    .write_reg(
                        [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3][index],
                        value,
                    )
                    .unwrap();
            } else {
                ctx.cpu
                    .write_mem(0x9000 + ((index - 4) * 4) as u32, &value.to_le_bytes())
                    .unwrap();
            }
        }
        match handler(ctx).unwrap() {
            DispatchOutcome::ReturnedR0(v) => v,
            other => panic!("unexpected {other:?}"),
        }
    }
    #[test]
    fn mapping_api_transfers_special_file_handles_and_reports_mapped_virtual_memory() {
        let dir = tempfile::tempdir().unwrap();
        let mut cpu = scanf_cpu();
        let mut kernel = fresh_kernel();
        kernel.vfs.mount_save_dir("\\SD Card\\", dir.path());
        let path: Vec<_> = "\\SD Card\\mapping"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &path).unwrap();
        let thunk = thunk_at(0x70000100);
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let file = invoke(
            &mut ctx,
            &[0x1000, 0xc0000000, 0, 0, 2, 0, 0],
            create_file_for_mapping,
        );
        assert_ne!(file, INVALID_HANDLE_VALUE);
        let section = invoke(
            &mut ctx,
            &[file, 0, 0x08000004, 0, 4097, 0],
            create_file_mapping,
        );
        assert_ne!(section, 0);
        assert!(!ctx.kernel.vfs.is_open(file));
        let base = invoke(&mut ctx, &[section, 0x000f001f, 0, 0, 0], map_view);
        assert_ne!(base, 0);
        assert!(close_owned_handle(ctx.kernel, section));
        ctx.cpu.write_mem(base + 4093, b"tail").unwrap();
        assert_eq!(invoke(&mut ctx, &[base, 0x1800, 28], virtual_query), 28);
        assert_eq!(ctx.cpu.read_u32_le(0x1818).unwrap(), 0x40000);
        assert_eq!(ctx.cpu.read_u32_le(0x180c).unwrap(), 8192);
        assert_eq!(invoke(&mut ctx, &[base, 0, 0x8000], virtual_free), 0);
        assert_eq!(invoke(&mut ctx, &[base + 4093, 4], flush_view), 1);
        assert_eq!(invoke(&mut ctx, &[base + 1], unmap_view), 0);
        assert_eq!(invoke(&mut ctx, &[base], unmap_view), 1);
        let data = std::fs::read(dir.path().join("mapping")).unwrap();
        assert_eq!(data.len(), 4097);
        assert_eq!(&data[4093..], b"tail");
    }
    #[test]
    fn failed_mapping_consumes_only_create_file_for_mapping_handles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"data").unwrap();
        let mut cpu = scanf_cpu();
        let mut kernel = fresh_kernel();
        kernel.vfs.mount_read_only("\\SD Card\\", dir.path());
        let thunk = thunk_at(0x70000100);
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        for special in [false, true] {
            let h = ctx
                .kernel
                .vfs
                .open_file("\\SD Card\\a", 1, 3, 3, false, 0)
                .unwrap()
                .handle;
            if special {
                ctx.kernel.vfs.mark_for_mapping(h);
            }
            assert_eq!(
                invoke(&mut ctx, &[h, 0, 4, 0, 4, 0], create_file_mapping),
                0
            );
            assert_eq!(ctx.kernel.thread_last_errors.get(&0), Some(&5));
            assert_eq!(ctx.kernel.vfs.is_open(h), !special);
            if !special {
                ctx.kernel.vfs.close(h);
            }
        }
        assert_eq!(
            invoke(
                &mut ctx,
                &[INVALID_HANDLE_VALUE, 0, 4, 1, 4096, 0],
                create_file_mapping
            ),
            0
        );
        assert_eq!(ctx.kernel.thread_last_errors.get(&0), Some(&87));
    }
}
