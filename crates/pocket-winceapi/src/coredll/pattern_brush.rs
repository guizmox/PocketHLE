//! Bitmap brushes own their pixels independently of the source HBITMAP.
use super::*;
pub(super) fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let Some(mut bitmap) = ctx.kernel.gdi.bitmap(handle).cloned() else {
        set_thread_error(ctx, 6);
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    if bitmap.dib_bits_va.is_some() && !bitmap.host_dirty {
        let (mut raw, mut pixels) = (Vec::new(), Vec::new());
        if !snapshot_dib_into(ctx.cpu, &bitmap, &mut raw, &mut pixels) {
            set_thread_error(ctx, 998);
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        if bitmap.bpp == 1 {
            // Retain bit identity even when both DIB palette colours are equal.
            for y in 0..bitmap.height {
                for x in 0..bitmap.width {
                    let row = if bitmap.dib_bottom_up {
                        bitmap.height - 1 - y
                    } else {
                        y
                    };
                    let bit =
                        raw[(row * bitmap.dib_row_stride + x / 8) as usize] & (0x80 >> (x % 8));
                    let out = if bit == 0 { 0u16 } else { 0xffff };
                    let i = ((y * bitmap.width + x) * 2) as usize;
                    pixels[i..i + 2].copy_from_slice(&out.to_le_bytes());
                }
            }
            bitmap.dib_palette = vec![0, 0xffff];
        }
        bitmap.pixels = pixels;
    }
    match ctx.kernel.gdi.create_pattern_brush(&bitmap) {
        Some(handle) => Ok(DispatchOutcome::ReturnedR0(handle)),
        None => {
            set_thread_error(ctx, 87);
            Ok(DispatchOutcome::ReturnedR0(0))
        }
    }
}
pub(super) fn set_origin(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hdc = ctx.arg_u32(0)?;
    let origin = (ctx.arg_u32(1)? as i32, ctx.arg_u32(2)? as i32);
    let output = ctx.arg_u32(3)?;
    let Some(dc) = ctx.kernel.gdi.dc(hdc) else {
        set_thread_error(ctx, 6);
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let previous = dc.brush_origin;
    if output != 0 {
        let bytes: Vec<_> = [previous.0, previous.1]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(output, &bytes)?;
    }
    ctx.kernel.gdi.dc_mut(hdc).unwrap().brush_origin = origin;
    Ok(DispatchOutcome::ReturnedR0(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    use pocket_kernel::gdi::{Bitmap, GDI_SCREEN_DC};
    #[test]
    fn copied_pattern_survives_source_deletion_and_fills_clipped_surface() {
        let (mut cpu, mut k, mut d) = setup();
        let mut bitmap = Bitmap::new(2, 1);
        bitmap.pixels = vec![0, 248, 31, 0];
        let bitmap = k.gdi.register_dib(bitmap);
        let DispatchOutcome::ReturnedR0(brush) = call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreatePatternBrush",
            &[bitmap],
        ) else {
            panic!()
        };
        assert_ne!(brush, 0);
        assert!(k.gdi.delete(bitmap));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SelectObject",
            &[GDI_SCREEN_DC, brush],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SetBrushOrgEx",
            &[GDI_SCREEN_DC, 1, 0, 0x2100],
        );
        assert_eq!(cpu.read_mem(0x2100, 8).unwrap(), [0; 8]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "PatBlt",
            &[GDI_SCREEN_DC, 0, 0, 4, 1, rop3::PATCOPY],
        );
        assert_eq!(
            &k.framebuffer.cpu().unwrap().pixels[..8],
            &[31, 0, 0, 248, 31, 0, 0, 248]
        );
    }
}
