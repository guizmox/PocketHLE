//! Original Explorer file-copy packets and CERF SampleApp's text clipboard.
//! Data is shared through owned snapshots; every returned HLOCAL is CPU-local.
use super::*;
use std::collections::HashSet;

pub(super) fn register(d: &mut crate::WinCeDispatcher) {
    for (name, handler) in [
        ("OpenClipboard", open as crate::Handler),
        ("CloseClipboard", close),
        ("EmptyClipboard", empty),
        ("IsClipboardFormatAvailable", available),
        ("GetClipboardData", get),
        ("GetClipboardDataAlloc", get_alloc),
        ("SetClipboardData", set),
        ("EnumClipboardFormats", enumerate),
        ("CountClipboardFormats", count),
        ("GetClipboardOwner", owner),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}

fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn retire_stale_handles(ctx: &mut CallCtx<'_>) {
    let revisions = ctx
        .kernel
        .object_handles
        .with_clipboard(|c, _| c.revisions());
    let mut retired = HashSet::new();
    ctx.kernel
        .ce
        .clipboard
        .entries
        .retain(|format, (revision, pointer)| {
            let live = revisions.get(format) == Some(revision)
                && ctx.kernel.heap.msize(*pointer).is_some();
            if !live {
                retired.insert(*pointer);
            }
            live
        });
    for pointer in retired {
        if ctx.kernel.heap.msize(pointer).is_some()
            && !ctx
                .kernel
                .ce
                .clipboard
                .entries
                .values()
                .any(|(_, p)| *p == pointer)
        {
            do_free(ctx, pointer);
        }
    }
}

fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    if hwnd != 0
        && !ctx.kernel.window_classes.contains_key(&hwnd)
        && ctx.kernel.controls.get(hwnd).is_none()
    {
        return fail(ctx, 1400);
    }
    let thread = ctx.kernel.current_thread;
    match ctx
        .kernel
        .object_handles
        .with_clipboard(|c, pid| c.open(pid, thread, hwnd))
    {
        Ok(()) => {
            retire_stale_handles(ctx);
            Ok(DispatchOutcome::ReturnedR0(1))
        }
        Err(error) => fail(ctx, error),
    }
}

fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let thread = ctx.kernel.current_thread;
    match ctx
        .kernel
        .object_handles
        .with_clipboard(|c, pid| c.close(pid, thread))
    {
        Ok(()) => {
            retire_stale_handles(ctx);
            Ok(DispatchOutcome::ReturnedR0(1))
        }
        Err(error) => fail(ctx, error),
    }
}

fn empty(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let thread = ctx.kernel.current_thread;
    match ctx
        .kernel
        .object_handles
        .with_clipboard(|c, pid| c.empty(pid, thread))
    {
        Ok(()) => {
            retire_stale_handles(ctx);
            Ok(DispatchOutcome::ReturnedR0(1))
        }
        Err(error) => fail(ctx, error),
    }
}

fn set(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let format = ctx.arg_u32(0)?;
    let handle = ctx.arg_u32(1)?;
    if format == 0 || format > 0xffff {
        return fail(ctx, 87);
    }
    // Delayed rendering and GDI-object formats require separate owner callbacks;
    // they must not be mistaken for HLOCAL pointers or reported as stored data.
    if handle == 0 || matches!(format, 2 | 3 | 9 | 14) {
        return fail(ctx, 50);
    }
    let Some(size) = ctx.kernel.heap.msize(handle).filter(|size| *size > 0) else {
        return fail(ctx, 6);
    };
    if ctx
        .cpu
        .check_guest_access(handle, size, Prot::READ)
        .is_err()
    {
        return fail(ctx, 998);
    }
    let bytes = ctx.cpu.read_mem(handle, size)?;
    let thread = ctx.kernel.current_thread;
    let revision = match ctx
        .kernel
        .object_handles
        .with_clipboard(|c, pid| c.set(pid, thread, format, bytes))
    {
        Ok(revision) => revision,
        Err(error) => return fail(ctx, error),
    };
    if let Some((_, old)) = ctx
        .kernel
        .ce
        .clipboard
        .entries
        .insert(format, (revision, handle))
    {
        if old != handle
            && ctx.kernel.heap.msize(old).is_some()
            && !ctx
                .kernel
                .ce
                .clipboard
                .entries
                .values()
                .any(|(_, p)| *p == old)
        {
            do_free(ctx, old);
        }
    }
    retire_stale_handles(ctx);
    Ok(DispatchOutcome::ReturnedR0(handle))
}

fn get(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    read(ctx, false)
}
fn get_alloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    read(ctx, true)
}

fn read(ctx: &mut CallCtx<'_>, owned: bool) -> Result<DispatchOutcome, KernelError> {
    let format = ctx.arg_u32(0)?;
    if format == 0 || format > 0xffff {
        return fail(ctx, 87);
    }
    let thread = ctx.kernel.current_thread;
    let data = match ctx
        .kernel
        .object_handles
        .with_clipboard(|c, pid| c.get(pid, thread, format))
    {
        Ok(Some(data)) => data,
        Ok(None) => return fail(ctx, 0),
        Err(error) => return fail(ctx, error),
    };
    retire_stale_handles(ctx);
    if !owned {
        if let Some(&(_, pointer)) = ctx.kernel.ce.clipboard.entries.get(&format) {
            return Ok(DispatchOutcome::ReturnedR0(pointer));
        }
    }
    let size = u32::try_from(data.bytes.len())
        .map_err(|_| KernelError::Loader("clipboard data too large".into()))?;
    let allocation = do_alloc(ctx, size)?;
    let DispatchOutcome::ReturnedR0(pointer) = allocation else {
        return Ok(allocation);
    };
    if pointer == 0 {
        return Ok(allocation);
    }
    if let Err(error) = ctx.cpu.write_mem(pointer, &data.bytes) {
        do_free(ctx, pointer);
        return Err(error.into());
    }
    if !owned {
        ctx.kernel
            .ce
            .clipboard
            .entries
            .insert(format, (data.revision, pointer));
    }
    Ok(DispatchOutcome::ReturnedR0(pointer))
}

fn available(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let format = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(
        ctx.kernel
            .object_handles
            .with_clipboard(|c, _| c.available(format)),
    )))
}
fn enumerate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let previous = ctx.arg_u32(0)?;
    let thread = ctx.kernel.current_thread;
    match ctx
        .kernel
        .object_handles
        .with_clipboard(|c, pid| c.next(pid, thread, previous))
    {
        Ok(format) => {
            set_thread_error(ctx, 0);
            Ok(DispatchOutcome::ReturnedR0(format))
        }
        Err(error) => fail(ctx, error),
    }
}
fn count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel.object_handles.with_clipboard(|c, _| c.count()),
    ))
}
fn owner(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel.object_handles.with_clipboard(|c, _| c.owner()),
    ))
}

#[cfg(test)]
mod tests;
