//! Global-memory clipboard ownership shared by USER APIs and EDIT commands.
use super::*;
const TEXT: u32 = 1;
const UNICODE: u32 = 13;
fn opened(ctx: &CallCtx<'_>) -> bool {
    ctx.kernel.clipboard.opened_by == Some(ctx.kernel.current_thread)
}
fn denied(ctx: &mut CallCtx<'_>, code: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, code);
    Ok(DispatchOutcome::ReturnedR0(0))
}
pub(super) fn open_clipboard(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    if ctx.kernel.clipboard.opened_by.is_some() {
        return denied(ctx, 5);
    }
    if hwnd != 0 && !window_exists(ctx.kernel, hwnd) {
        return denied(ctx, 1400);
    }
    ctx.kernel.clipboard.opened_by = Some(ctx.kernel.current_thread);
    ctx.kernel.clipboard.owner = hwnd;
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(super) fn close_clipboard(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !opened(ctx) {
        return denied(ctx, 1418);
    }
    ctx.kernel.clipboard.opened_by = None;
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn clear(ctx: &mut CallCtx<'_>) {
    for (_, pointer) in std::mem::take(&mut ctx.kernel.clipboard.formats) {
        ctx.kernel.heap.free(pointer);
    }
}
pub(super) fn empty_clipboard(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !opened(ctx) {
        return denied(ctx, 1418);
    }
    clear(ctx);
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn available(ctx: &CallCtx<'_>, format: u32) -> bool {
    ctx.kernel.clipboard.formats.contains_key(&format)
        || matches!(format, TEXT | UNICODE)
            && ctx
                .kernel
                .clipboard
                .formats
                .keys()
                .any(|f| matches!(*f, TEXT | UNICODE))
}
pub(super) fn is_clipboard_format_available(
    ctx: &mut CallCtx<'_>,
) -> Result<DispatchOutcome, KernelError> {
    let format = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(available(
        ctx, format,
    ))))
}
fn buffer(ctx: &mut CallCtx<'_>, format: u32) -> Result<Option<Vec<u8>>, KernelError> {
    let Some(&pointer) = ctx.kernel.clipboard.formats.get(&format) else {
        return Ok(None);
    };
    let size =
        ctx.kernel.heap.msize(pointer).ok_or_else(|| {
            KernelError::Dispatch("Clipboard global-memory owner was freed".into())
        })?;
    if size > 0x2000000 {
        return Err(KernelError::Dispatch(
            "Clipboard text extent exceeds supported bounds".into(),
        ));
    }
    Ok(Some(ctx.cpu.read_mem(pointer, size)?))
}
pub(super) fn text(ctx: &mut CallCtx<'_>) -> Result<Option<String>, KernelError> {
    if let Some(bytes) = buffer(ctx, UNICODE)? {
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .take_while(|v| *v != 0)
            .collect();
        return Ok(Some(String::from_utf16_lossy(&units)));
    }
    if let Some(bytes) = buffer(ctx, TEXT)? {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        return Ok(Some(crate::crt_ansi::decode(&bytes[..end])));
    }
    Ok(None)
}
fn allocate(ctx: &mut CallCtx<'_>, bytes: &[u8]) -> Result<u32, KernelError> {
    let DispatchOutcome::ReturnedR0(pointer) = do_alloc(ctx, bytes.len() as u32)? else {
        unreachable!()
    };
    if pointer == 0 {
        set_thread_error(ctx, 8);
        return Ok(0);
    }
    if let Err(error) = ctx.cpu.write_mem(pointer, bytes) {
        ctx.kernel.heap.free(pointer);
        return Err(error.into());
    }
    Ok(pointer)
}
pub(super) fn get_clipboard_data(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !opened(ctx) {
        return denied(ctx, 1418);
    }
    let format = ctx.arg_u32(0)?;
    if let Some(&pointer) = ctx.kernel.clipboard.formats.get(&format) {
        return Ok(DispatchOutcome::ReturnedR0(pointer));
    }
    if matches!(format, TEXT | UNICODE) {
        if let Some(text) = text(ctx)? {
            let bytes = if format == UNICODE {
                text.encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect()
            } else {
                let mut bytes = crate::crt_ansi::encode(&text);
                bytes.push(0);
                bytes
            };
            let pointer = allocate(ctx, &bytes)?;
            if pointer == 0 {
                return Ok(DispatchOutcome::ReturnedR0(0));
            }
            ctx.kernel.clipboard.formats.insert(format, pointer);
            return Ok(DispatchOutcome::ReturnedR0(pointer));
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
pub(super) fn set_clipboard_data(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !opened(ctx) {
        return denied(ctx, 1418);
    }
    let format = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    if format == 0
        || pointer == 0
        || !ctx.kernel.heap.owns_global_block(pointer)
        || ctx
            .kernel
            .clipboard
            .formats
            .iter()
            .any(|(f, p)| *f != format && *p == pointer)
    {
        return denied(ctx, 87);
    }
    if let Some(old) = ctx
        .kernel
        .clipboard
        .formats
        .insert(format, pointer)
        .filter(|old| *old != pointer)
    {
        ctx.kernel.heap.free(old);
    }
    Ok(DispatchOutcome::ReturnedR0(pointer))
}
pub(super) fn copy_text(
    ctx: &mut CallCtx<'_>,
    owner: u32,
    text: &str,
) -> Result<bool, KernelError> {
    if ctx.kernel.clipboard.opened_by.is_some() {
        return Ok(false);
    }
    let bytes: Vec<_> = text
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let pointer = allocate(ctx, &bytes)?;
    if pointer == 0 {
        return Ok(false);
    }
    clear(ctx);
    ctx.kernel.clipboard.owner = owner;
    ctx.kernel.clipboard.formats.insert(UNICODE, pointer);
    Ok(true)
}
