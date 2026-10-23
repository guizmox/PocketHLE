//! SaveDC/RestoreDC preserve the complete modeled context, including clipping.
use crate::{CallCtx, WinCeDispatcher};
use pocket_kernel::{DispatchOutcome, KernelError};
pub(crate) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "SaveDC", save);
    d.register_handler("coredll.dll", "RestoreDC", restore);
}
fn save(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    if ctx.kernel.gdi.dc(dc).is_none() {
        crate::coredll::set_thread_error(ctx, 6);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let level = ctx.kernel.gdi.save_dc(dc).unwrap_or(0);
    if level == 0 {
        crate::coredll::set_thread_error(ctx, 8);
    }
    Ok(DispatchOutcome::ReturnedR0(level))
}
fn restore(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let level = ctx.arg_u32(1)? as i32;
    let known = ctx.kernel.gdi.dc(dc).is_some();
    let ok = ctx.kernel.gdi.restore_dc(dc, level);
    if !ok {
        crate::coredll::set_thread_error(ctx, if known { 87 } else { 6 });
    }
    Ok(DispatchOutcome::ReturnedR0(u32::from(ok)))
}
#[cfg(test)]
mod tests {
    use crate::bluetooth::tests::{call, setup};
    use pocket_kernel::DispatchOutcome;
    #[test]
    fn nested_levels_restore_selection_clipping_and_colors() {
        let (mut cpu, mut k, mut d) = setup();
        let dc = k.gdi.create_memory_dc();
        let other = k.gdi.create_memory_dc();
        let bitmap = k.gdi.create_compatible_bitmap(2, 2);
        k.gdi.select_into(dc, bitmap);
        k.gdi.dc_mut(dc).unwrap().clip_rects = Some(vec![[1, 2, 3, 4]]);
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "coredll.dll", "SaveDC", &[dc]),
            DispatchOutcome::ReturnedR0(1)
        );
        k.gdi.dc_mut(dc).unwrap().text_color = 0x123456;
        k.gdi.dc_mut(dc).unwrap().clip_rects = None;
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "coredll.dll", "SaveDC", &[dc]),
            DispatchOutcome::ReturnedR0(2)
        );
        k.gdi.dc_mut(dc).unwrap().text_color = 0xffffff;
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RestoreDC",
                &[dc, u32::MAX]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(k.gdi.dc(dc).unwrap().text_color, 0x123456);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RestoreDC",
                &[other, 1]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RestoreDC",
                &[dc, 1]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        let restored = k.gdi.dc(dc).unwrap();
        assert_eq!(restored.clip_rects, Some(vec![[1, 2, 3, 4]]));
        assert_eq!(restored.selected_bitmap, Some(bitmap));
        assert_eq!(restored.text_color, 0);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RestoreDC",
                &[dc, 1]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }
}
