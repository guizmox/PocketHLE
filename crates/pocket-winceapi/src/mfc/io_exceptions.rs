//! Structured guest I/O failures enter the same CE3 TRY engine as AfxThrow.
//! The dispatcher consumes the transient fault before executing guest code.
use super::*;
use pocket_cpu::Prot;
pub(super) const ARCHIVE: &str = "MFC CArchive";
pub(super) const FILE: &str = "MFC CFile";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (705, "?AfxThrowArchiveException@@YAXHPBG@Z", throw_archive),
    (706, "?AfxThrowFileException@@YAXHJPBG@Z", throw_file),
    (
        1282,
        "?GetErrorMessage@CArchiveException@@UAAHPAGIPAI@Z",
        archive_message,
    ),
    (
        1284,
        "?GetErrorMessage@CFileException@@UAAHPAGIPAI@Z",
        file_message,
    ),
    (
        1438,
        "?GetRuntimeClass@CArchiveException@@UBAPAUCRuntimeClass@@XZ",
        archive_runtime,
    ),
    (
        1471,
        "?GetRuntimeClass@CFileException@@UBAPAUCRuntimeClass@@XZ",
        file_runtime,
    ),
    (
        2072,
        "?OsErrorToException@CFileException@@SAHJ@Z",
        os_cause_call,
    ),
    (2461, "?ThrowOsError@CFileException@@SAXJPBG@Z", throw_os),
];
pub(super) fn fault(
    runtime: &'static str,
    cause: u32,
    os_error: u32,
    path: Vec<u16>,
) -> KernelError {
    KernelError::RuntimeException {
        runtime,
        cause,
        os_error,
        path,
    }
}
fn filename(ctx: &mut CallCtx<'_>, pointer: u32) -> Result<Vec<u16>, KernelError> {
    if pointer == 0 {
        Ok(Vec::new())
    } else {
        strings::wide(ctx, pointer)
    }
}
fn throw_archive(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let cause = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    Err(fault(ARCHIVE, cause, 0, filename(ctx, p)?))
}
fn throw_file(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let cause = ctx.arg_u32(0)?;
    let os = ctx.arg_u32(1)?;
    let p = ctx.arg_u32(2)?;
    Err(fault(FILE, cause, os, filename(ctx, p)?))
}
fn throw_os(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let os = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    if os == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    Err(fault(FILE, os_cause(os), os, filename(ctx, p)?))
}
fn os_cause_call(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(os_cause(ctx.arg_u32(0)?)))
}
// Exhaustive non-generic mappings from the native CE3 pure conversion routine.
// Unknown (including out-of-range DWORD) errors retain generic == 1.
pub(super) fn os_cause(os: u32) -> u32 {
    match os {
        0 => 0,
        2 | 6 | 18 | 107 => 2,
        3 | 15 | 17 | 34 | 52 | 53 | 55 | 67 | 70 | 85 | 111 | 123 | 124 | 125 | 144 | 154
        | 161 | 206 | 208 | 267 => 3,
        4 | 36 | 68 | 113 => 4,
        5 | 12 | 19 | 29 | 54 | 58 | 64 | 65 | 71 | 80 | 82 | 86 | 170 | 183 | 999 => 5,
        11 | 26 | 60 | 66 | 114 | 182 | 191 | 193 => 6,
        16 | 145 => 7,
        62 => 8,
        24 | 25 | 27 | 30 | 131 | 132 => 9,
        20 | 21 | 22 | 23 | 57 | 59 | 88 | 117 | 995 | 996 | 997 => 10,
        32 => 11,
        33 | 108 | 167 => 12,
        39 | 112 => 13,
        38 => 14,
        _ => 1,
    }
}
pub(super) fn initialize_file(ctx: &mut CallCtx<'_>, p: u32) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(p, 20, Prot::READ | Prot::WRITE)?;
    let table = super::classes::vtable(ctx, "CFileException")?;
    ctx.cpu.write_mem(p, &[0; 20])?;
    new_string(ctx, p + 16, &[])?;
    put(ctx, p, table)?;
    put(ctx, p + 4, 1)?;
    put(ctx, p + 12, u32::MAX)
}
pub(super) fn raise(
    ctx: &mut CallCtx<'_>,
    runtime: &str,
    cause: u32,
    os: u32,
    path: &[u16],
) -> Result<DispatchOutcome, KernelError> {
    if runtime == "MFC CMemory" {
        return Ok(DispatchOutcome::JumpTo(super::callbacks::service(
            ctx,
            super::exception_throw::MEMORY,
        )?));
    }
    let (class, size, offset) = match runtime {
        ARCHIVE => ("CArchiveException", 16, 12),
        FILE => ("CFileException", 20, 16),
        _ => {
            return Err(KernelError::Dispatch(format!(
                "Unknown guest runtime exception provider {runtime}"
            )))
        }
    };
    let table = super::classes::vtable(ctx, class)?;
    let target = super::callbacks::service(ctx, super::exception_throw::THROW)?;
    let p = alloc_cpp(ctx, size)?;
    let result = (|| {
        ctx.cpu.write_mem(p, &vec![0; size as usize])?;
        new_string(ctx, p + offset, path)?;
        put(ctx, p, table)?;
        put(ctx, p + 4, 1)?;
        put(ctx, p + 8, cause)?;
        if runtime == FILE {
            put(ctx, p + 12, os)?;
        }
        Ok::<_, KernelError>(())
    })();
    if let Err(e) = result {
        if word(ctx, p + offset).unwrap_or(0) != 0 {
            let _ = release_string(ctx, p + offset);
        }
        ctx.kernel.heap.free(p);
        return Err(e);
    }
    // Tail-transfer: the throw does not return to the failed operation. Its
    // real guest TRY jump retires that operation's younger continuation owners.
    ctx.cpu.write_arg(0, p)?;
    Ok(DispatchOutcome::JumpTo(target))
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>, file: bool) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    if flags & !1 != 0 {
        return Err(KernelError::Dispatch(
            "MFC I/O exception vector deletion".into(),
        ));
    }
    ctx.cpu
        .check_guest_access(p, if file { 20 } else { 16 }, Prot::READ | Prot::WRITE)?;
    release_string(ctx, p + if file { 16 } else { 12 })?;
    if flags & 1 != 0 {
        ctx.kernel.heap.free(p);
    }
    Ok(DispatchOutcome::ReturnedR0(p))
}
fn archive_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    runtime(ctx, "CArchiveException")
}
fn file_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    runtime(ctx, "CFileException")
}
fn runtime(ctx: &mut CallCtx<'_>, name: &str) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, name,
    )?))
}
fn archive_message(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    message(ctx, false)
}
fn file_message(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    message(ctx, true)
}
fn message(ctx: &mut CallCtx<'_>, file: bool) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let help = ctx.arg_u32(3)?;
    let cause = word(ctx, p + 8)?;
    let id = if file { 0xf1a0u32 } else { 0xf1b0u32 }.wrapping_add(cause);
    if help != 0 {
        ctx.cpu.check_guest_access(help, 4, Prot::WRITE)?;
    }
    if count != 0 {
        ctx.cpu.check_guest_access(
            out,
            count
                .checked_mul(2)
                .ok_or_else(|| KernelError::Dispatch("Exception text size overflow".into()))?,
            Prot::WRITE,
        )?;
    }
    let name = strings::value(ctx, p + if file { 16 } else { 12 })?;
    let name = if name.is_empty() {
        super::resources::framework_string(ctx, 0xf006)?
    } else {
        name
    };
    let format = super::resources::framework_string(ctx, id)?;
    let text = String::from_utf16_lossy(&format).replace("%1", &String::from_utf16_lossy(&name));
    if help != 0 {
        put(ctx, help, id)?;
    }
    if count != 0 {
        let bytes: Vec<_> = text
            .encode_utf16()
            .take(count.saturating_sub(1) as usize)
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(out, &bytes)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn owned_file_and_archive_faults_are_caught_by_real_guest_try_and_cleanup_their_filename() {
        for (ordinal, kind, cause, offset, os) in
            [(705, ARCHIVE, 7, 12, 0), (706, FILE, 13, 16, 112)]
        {
            let (mut cpu, mut k, mut d) = framework_setup();
            cpu.write_mem(
                0x1000,
                &"\\Data\\state.bin"
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            let ts = result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:651",
                &[],
            ));
            let mut free = None;
            for _ in 0..3 {
                cpu.write_reg(ArmReg::Sp, 0x3ff0).unwrap();
                cpu.write_reg(ArmReg::Lr, 0x12345678).unwrap();
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:33",
                    &[0x2400],
                );
                call(&mut cpu, &mut k, &mut d, "coredll.dll", "setjmp", &[0x2400]);
                cpu.write_reg(ArmReg::Sp, 0x3f00).unwrap();
                let args = if kind == FILE {
                    vec![cause, os, 0x1000]
                } else {
                    vec![cause, 0x1000]
                };
                for (i, v) in args.into_iter().enumerate() {
                    cpu.write_arg(i as u8, v).unwrap();
                }
                assert_eq!(
                    drive(&mut cpu, &mut k, &mut d, entry(ordinal), |pc, _| panic!(
                        "unexpected guest {pc:x}"
                    )),
                    1
                );
                let exception = cpu.read_u32_le(0x2434).unwrap();
                assert_ne!(exception, 0);
                assert_eq!(cpu.read_u32_le(exception + 4).unwrap(), 1);
                assert_eq!(cpu.read_u32_le(exception + 8).unwrap(), cause);
                if kind == FILE {
                    assert_eq!(cpu.read_u32_le(exception + 12).unwrap(), os);
                }
                assert_ne!(cpu.read_u32_le(exception + offset).unwrap(), 0);
                for (i, v) in [exception, 0x2800, 80, 0x2900].into_iter().enumerate() {
                    cpu.write_arg(i as u8, v).unwrap();
                }
                assert_eq!(
                    drive(
                        &mut cpu,
                        &mut k,
                        &mut d,
                        entry(if kind == FILE { 1284 } else { 1282 }),
                        |_, _| panic!()
                    ),
                    1
                );
                assert_eq!(
                    cpu.read_u32_le(0x2900).unwrap(),
                    if kind == FILE { 0xf1a0 } else { 0xf1b0 } + cause
                );
                assert_eq!(
                    drive(&mut cpu, &mut k, &mut d, entry(768), |pc, _| panic!(
                        "unexpected cleanup {pc:x}"
                    )),
                    0
                );
                assert_eq!(cpu.read_u32_le(ts + 0x10).unwrap(), 0);
                assert!(k.heap.msize(exception).is_none());
                if let Some(n) = free {
                    assert_eq!(k.heap.free_bytes(), n);
                }
                free = Some(k.heap.free_bytes());
            }
        }
    }
}
