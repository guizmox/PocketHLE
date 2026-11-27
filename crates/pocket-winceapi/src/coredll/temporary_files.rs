//! Temporary names use VFS CREATE_NEW, shared leases and normal mount policy.
use super::*;
pub(super) fn get(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let directory = ctx.arg_u32(0)?;
    let prefix = ctx.arg_u32(1)?;
    let unique = ctx.arg_u32(2)?;
    let out = ctx.arg_u32(3)?;
    ctx.cpu
        .check_guest_access(out, 520, pocket_cpu::Prot::WRITE)?;
    let directory = String::from_utf16_lossy(&read_wstr(ctx, directory, 260)?);
    let prefix = String::from_utf16_lossy(&read_wstr(ctx, prefix, 260)?);
    match ctx.kernel.vfs.temporary_file(&directory, &prefix, unique) {
        Ok((path, number)) => {
            let data: Vec<_> = path
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            ctx.cpu.write_mem(out, &data)?;
            Ok(DispatchOutcome::ReturnedR0(number))
        }
        Err(code) => {
            set_thread_error(ctx, code);
            Ok(DispatchOutcome::ReturnedR0(0))
        }
    }
}
