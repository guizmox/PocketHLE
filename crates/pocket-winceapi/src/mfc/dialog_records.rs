//! CE3 DLGINIT walks unaligned records and sends real control messages.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
use pocket_pe::ResourceKey;

pub(super) const DIRECT: &str = "__PocketHLE_MFC_ExecuteDialogInit";
pub(super) const EXECUTE: &str = "__PocketHLE_MFC_ExecuteDialogInitBlock";
const MAX_BYTES: u32 = 4 * 1024 * 1024;
const MAX_RECORDS: u32 = 4096;
const CURSOR: u32 = 0;
const END: u32 = 1;
const COUNT: u32 = 2;
const BUFFER: u32 = 3;
const MESSAGE: u32 = 4;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1142, "?ExecuteDlgInit@CWnd@@QAAHPAX@Z", direct),
    (1143, "?ExecuteDlgInit@CWnd@@QAAHPBG@Z", resource),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", EXECUTE, checked);
    d.register_handler("coredll.dll", DIRECT, direct);
}
pub(super) fn resource_data(ctx: &mut CallCtx<'_>, raw: u32) -> Result<(u32, u32), KernelError> {
    if raw == 0 {
        return Ok((0, 0));
    }
    let name = crate::coredll::read_wide_resource_key(ctx, raw)?;
    let module = super::resources::module_for(ctx, &name, &ResourceKey::Id(240))?;
    let Some((entry, base)) =
        crate::coredll::lookup_resource_in_module(ctx.kernel, module, &ResourceKey::Id(240), &name)
    else {
        return Ok((0, 0));
    };
    let address = base
        .checked_add(entry.data_rva)
        .ok_or_else(|| KernelError::Dispatch("MFC DLGINIT resource overflow".into()))?;
    Ok((address, entry.size))
}
fn resource(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    let object = f.arg(ctx, 0)?;
    let raw = f.arg(ctx, 1)?;
    let (data, size) = resource_data(ctx, raw)?;
    if data == 0 {
        return f.call_service(ctx, 1, DIRECT, &[object, 0]);
    }
    f.call_service(ctx, 1, EXECUTE, &[object, data, size])
}
fn direct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    execute(ctx, None)
}
fn checked(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let size = ctx.arg_u32(2)?;
    execute(ctx, Some(size))
}
fn read(ctx: &mut CallCtx<'_>, address: u32, size: u32, end: u32) -> Result<Vec<u8>, KernelError> {
    if address.checked_add(size).is_none_or(|next| next > end) {
        return Err(KernelError::Dispatch(
            "MFC DLGINIT record exceeds its block".into(),
        ));
    }
    Ok(ctx.cpu.read_mem(address, size)?)
}
fn execute(ctx: &mut CallCtx<'_>, size: Option<u32>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let data = f.arg(ctx, 1)?;
        let bytes = size.unwrap_or(MAX_BYTES);
        if bytes > MAX_BYTES {
            return Err(KernelError::Dispatch(
                "MFC DLGINIT block exceeds its bound".into(),
            ));
        }
        let end = data
            .checked_add(bytes)
            .ok_or_else(|| KernelError::Dispatch("MFC DLGINIT address overflow".into()))?;
        f.set(ctx, CURSOR, data)?;
        f.set(ctx, END, end)?;
    } else {
        match f.phase(ctx)? {
            2 => {
                let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
                let buffer = f.get(ctx, BUFFER)?;
                let message = f.get(ctx, MESSAGE)?;
                return f.call_service(ctx, 3, "SendMessageW", &[hwnd, message, 0, buffer]);
            }
            3 => {
                let result = ctx.cpu.read_reg(ArmReg::R0)?;
                let buffer = f.get(ctx, BUFFER)?;
                f.disown(ctx)?;
                ctx.kernel.heap.free(buffer);
                f.set(ctx, BUFFER, 0)?;
                if result == u32::MAX {
                    return f.finish(ctx, 0);
                }
            }
            1 => return f.finish(ctx, 1),
            _ => {
                return Err(KernelError::Dispatch(
                    "Invalid MFC DLGINIT continuation".into(),
                ))
            }
        }
    }
    loop {
        let cursor = f.get(ctx, CURSOR)?;
        let end = f.get(ctx, END)?;
        let id = if cursor == 0 {
            0
        } else {
            u16::from_le_bytes(read(ctx, cursor, 2, end)?.try_into().unwrap())
        };
        if id == 0 {
            let object = f.arg(ctx, 0)?;
            let hwnd = word(ctx, object + 0x20)?;
            return f.call_service(
                ctx,
                1,
                super::descendants::TARGET,
                &[hwnd, 0x364, 0, 0, 0, 0],
            );
        }
        let count = f.get(ctx, COUNT)?;
        if count >= MAX_RECORDS {
            return Err(KernelError::Dispatch(
                "MFC DLGINIT record count exceeds its bound".into(),
            ));
        }
        f.set(ctx, COUNT, count + 1)?;
        let header = read(ctx, cursor, 8, end)?;
        let raw = u16::from_le_bytes(header[2..4].try_into().unwrap());
        let bytes = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let data = cursor + 8; // checked with the whole header above
        let next = data
            .checked_add(bytes)
            .filter(|next| *next <= end)
            .ok_or_else(|| KernelError::Dispatch("MFC DLGINIT payload exceeds its block".into()))?;
        f.set(ctx, CURSOR, next)?;
        let message = match raw {
            0x401 => 0x180,
            0x403 => 0x143,
            0x1234 => 0x403,
            other => other,
        };
        // The retail CE3 runtime skips unrecognized record kinds, including
        // its 0x403 OLE record. Do not turn them into arbitrary SendMessage calls.
        if !matches!(message, 0x180 | 0x143) {
            continue;
        }
        let payload = read(ctx, data, bytes, end)?;
        if bytes < 2 || bytes % 2 != 0 || !payload.chunks_exact(2).any(|unit| unit == [0, 0]) {
            return Err(KernelError::Dispatch(
                "MFC DLGINIT string is not terminated UTF-16".into(),
            ));
        }
        let buffer = alloc_cpp(ctx, bytes)?;
        if let Err(error) = ctx.cpu.write_mem(buffer, &payload) {
            ctx.kernel.heap.free(buffer);
            return Err(error.into());
        }
        f.own(ctx, buffer)?;
        f.set(ctx, BUFFER, buffer)?;
        f.set(ctx, MESSAGE, u32::from(message))?;
        let object = f.arg(ctx, 0)?;
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 2, "GetDlgItem", &[hwnd, u32::from(id)]);
    }
}
