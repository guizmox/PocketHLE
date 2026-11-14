use super::*;
use pocket_kernel::caret::Caret;

pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("CreateCaret", create as crate::Handler),
        ("DestroyCaret", destroy),
        ("ShowCaret", show),
        ("HideCaret", hide),
        ("SetCaretPos", set_position),
        ("GetCaretPos", get_position),
        ("SetCaretBlinkTime", set_blink),
        ("GetCaretBlinkTime", get_blink),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}

fn failure(ctx: &mut CallCtx<'_>, code: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, code);
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let bitmap = ctx.arg_u32(1)?;
    let width = ctx.arg_u32(2)? as i32;
    let height = ctx.arg_u32(3)? as i32;
    if !ctx.kernel.window_classes.contains_key(&hwnd) && ctx.kernel.controls.get(hwnd).is_none()
        || window_thread(ctx.kernel, hwnd) != ctx.kernel.current_thread
    {
        return failure(ctx, 1400);
    }
    let (size, mask) = if bitmap > 1 {
        let Some(bitmap) = ctx.kernel.gdi.bitmap(bitmap) else {
            return failure(ctx, 6);
        };
        if bitmap.width == 0 || bitmap.height == 0 || bitmap.width > 4096 || bitmap.height > 4096 {
            return failure(ctx, 87);
        }
        (
            (bitmap.width, bitmap.height),
            Some(
                bitmap
                    .pixels
                    .chunks_exact(2)
                    .map(|pixel| u16::from_le_bytes([pixel[0], pixel[1]]) ^ u16::MAX)
                    .collect(),
            ),
        )
    } else {
        if width < 0 || height < 0 || width > 4096 || height > 4096 {
            return failure(ctx, 87);
        }
        ((width.max(1) as u32, height.max(1) as u32), None)
    };
    ctx.kernel.ce.carets.threads.insert(
        ctx.kernel.current_thread,
        Caret {
            hwnd,
            position: (0, 0),
            size,
            hidden: 1,
            gray: bitmap == 1,
            mask,
            epoch: std::time::Instant::now(),
        },
    );
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if ctx
        .kernel
        .ce
        .carets
        .threads
        .remove(&ctx.kernel.current_thread)
        .is_none()
    {
        return failure(ctx, 1400);
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn change_visibility(ctx: &mut CallCtx<'_>, show: bool) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let Some(caret) = ctx
        .kernel
        .ce
        .carets
        .threads
        .get_mut(&ctx.kernel.current_thread)
    else {
        return failure(ctx, 1400);
    };
    if hwnd != 0 && hwnd != caret.hwnd {
        return failure(ctx, 1400);
    }
    caret.hidden = if show {
        caret.hidden.saturating_sub(1)
    } else {
        caret.hidden.saturating_add(1)
    };
    caret.epoch = std::time::Instant::now();
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn show(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    change_visibility(ctx, true)
}
fn hide(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    change_visibility(ctx, false)
}

fn set_position(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let position = (ctx.arg_u32(0)? as i32, ctx.arg_u32(1)? as i32);
    let Some(caret) = ctx
        .kernel
        .ce
        .carets
        .threads
        .get_mut(&ctx.kernel.current_thread)
    else {
        return failure(ctx, 1400);
    };
    caret.position = position;
    caret.epoch = std::time::Instant::now();
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn get_position(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(0)?;
    if output == 0 || ctx.cpu.check_guest_access(output, 8, Prot::WRITE).is_err() {
        return failure(ctx, 87);
    }
    let Some(caret) = ctx.kernel.ce.carets.threads.get(&ctx.kernel.current_thread) else {
        return failure(ctx, 1400);
    };
    ctx.cpu.write_mem(output, &caret.position.0.to_le_bytes())?;
    ctx.cpu
        .write_mem(output + 4, &caret.position.1.to_le_bytes())?;
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn set_blink(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let value = ctx.arg_u32(0)?;
    if value == 0 {
        return failure(ctx, 87);
    }
    ctx.kernel.ce.carets.blink_ms = value;
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn get_blink(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(ctx.kernel.ce.carets.blink_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_caret_has_balanced_visibility_thread_ownership_and_readonly_queries() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        kernel.window_classes.insert(10, "RichEdit20W".into());
        kernel.window_threads.insert(10, 0);
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        for (reg, value) in [10, 0, 2, 16].into_iter().enumerate() {
            ctx.cpu.write_arg(reg as u8, value).unwrap();
        }
        assert_eq!(create(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
        assert_eq!(ctx.kernel.ce.carets.threads[&0].hidden, 1);
        ctx.cpu.write_arg(0, 10).unwrap();
        show(&mut ctx).unwrap();
        hide(&mut ctx).unwrap();
        hide(&mut ctx).unwrap();
        show(&mut ctx).unwrap();
        assert_eq!(ctx.kernel.ce.carets.threads[&0].hidden, 1);
        show(&mut ctx).unwrap();
        assert!(ctx.kernel.ce.carets.threads[&0].shown(std::time::Instant::now(), u32::MAX));
        ctx.cpu.write_arg(0, (-3i32) as u32).unwrap();
        ctx.cpu.write_arg(1, 7).unwrap();
        set_position(&mut ctx).unwrap();
        ctx.cpu.write_arg(0, 0x1100).unwrap();
        let before = ctx.kernel.framebuffer.frame_counter;
        assert_eq!(
            get_position(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            ctx.cpu.read_mem(0x1100, 8).unwrap(),
            [-3i32, 7]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            get_blink(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(500)
        );
        assert_eq!(ctx.kernel.framebuffer.frame_counter, before);
        ctx.kernel.current_thread = 1;
        ctx.cpu.write_arg(0, 10).unwrap();
        assert_eq!(hide(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.kernel.ce.carets.threads[&0].hidden, 0);
        ctx.kernel.current_thread = 0;
        assert_eq!(destroy(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
        assert!(ctx.kernel.ce.carets.threads.is_empty());
    }
}
