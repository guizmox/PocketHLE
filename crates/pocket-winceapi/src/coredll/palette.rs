//! Logical palettes on the existing true-color display; no hardware palette is advertised.
use super::*;
use pocket_kernel::gdi::GdiObject;

fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}
pub(super) fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let ptr = ctx.arg_u32(0)?;
    let Ok(header) = ctx.cpu.read_mem(ptr, 4) else {
        return fail(ctx, 87);
    };
    let version = u16::from_le_bytes([header[0], header[1]]);
    let count = usize::from(u16::from_le_bytes([header[2], header[3]]));
    if version != 0x300 || count == 0 || count > 256 {
        return fail(ctx, 87);
    }
    let Some(data) = ptr.checked_add(4) else {
        return fail(ctx, 87);
    };
    let Ok(bytes) = ctx.cpu.read_mem(data, (count * 4) as u32) else {
        return fail(ctx, 87);
    };
    let entries = bytes
        .chunks_exact(4)
        .map(|e| [e[0], e[1], e[2], e[3]])
        .collect();
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel.gdi.create_palette(entries),
    ))
}
pub(super) fn get_entries(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let start = ctx.arg_u32(1)? as usize;
    let count = ctx.arg_u32(2)? as usize;
    let output = ctx.arg_u32(3)?;
    let Some(entries) = ctx.kernel.gdi.palette(handle) else {
        return fail(ctx, 6);
    };
    if output == 0 {
        return Ok(DispatchOutcome::ReturnedR0(entries.len() as u32));
    }
    let range = entries.get(start..).unwrap_or_default();
    let count = count.min(range.len());
    let bytes: Vec<u8> = range[..count].iter().flatten().copied().collect();
    if count != 0 && ctx.cpu.write_mem(output, &bytes).is_err() {
        return fail(ctx, 998);
    }
    Ok(DispatchOutcome::ReturnedR0(count as u32))
}
pub(super) fn set_entries(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let start = ctx.arg_u32(1)? as usize;
    let requested = ctx.arg_u32(2)? as usize;
    let input = ctx.arg_u32(3)?;
    let Some(entries) = ctx.kernel.gdi.palette(handle) else {
        return fail(ctx, 6);
    };
    if pocket_kernel::gdi::is_stock_handle(handle) {
        return fail(ctx, 87);
    }
    let count = requested.min(entries.len().saturating_sub(start));
    if count == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let Ok(bytes) = ctx.cpu.read_mem(input, (count * 4) as u32) else {
        return fail(ctx, 998);
    };
    if let Some(GdiObject::Palette(entries)) = ctx.kernel.gdi.get_mut(handle) {
        for (dst, src) in entries[start..start + count]
            .iter_mut()
            .zip(bytes.chunks_exact(4))
        {
            dst.copy_from_slice(src);
        }
    }
    Ok(DispatchOutcome::ReturnedR0(count as u32))
}
pub(super) fn select(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let handle = ctx.arg_u32(1)?;
    if ctx.kernel.gdi.palette(handle).is_none() {
        return fail(ctx, 6);
    }
    let Some(dc) = ctx.kernel.gdi.dc_mut(dc) else {
        return fail(ctx, 6);
    };
    let previous = std::mem::replace(&mut dc.selected_palette, handle);
    Ok(DispatchOutcome::ReturnedR0(previous))
}
pub(super) fn realize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    if ctx.kernel.gdi.dc(dc).is_none() {
        return fail(ctx, 6);
    }
    // RGB565 has no writable system palette: zero hardware entries change.
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// Resolve COLORREF against the selected logical palette, then report the
/// representable color of our true-color RGB565 device. This query must not
/// acquire pixel write authority or publish a frame.
pub(super) fn nearest_color(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hdc = ctx.arg_u32(0)?;
    let color = ctx.arg_u32(1)?;
    let Some(dc) = ctx.kernel.gdi.dc(hdc) else {
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    };
    let rgb = if color >> 24 == 1 {
        let Some(entry) = ctx
            .kernel
            .gdi
            .palette(dc.selected_palette)
            .and_then(|entries| entries.get((color & 0xffff) as usize))
        else {
            return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
        };
        u32::from(entry[0]) | (u32::from(entry[1]) << 8) | (u32::from(entry[2]) << 16)
    } else {
        // RGB and PALETTERGB both resolve directly on a non-palettized device.
        color & 0x00ff_ffff
    };
    let (r, g, b) = rgb565_to_888(colorref_to_rgb565(rgb));
    Ok(DispatchOutcome::ReturnedR0(
        u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coredll::tests::{dummy_thunk, fresh_kernel};
    use pocket_cpu::{stub::StubCpu, Cpu};
    #[test]
    fn nearest_color_resolves_selected_palette_and_rgb565_without_drawing() {
        let mut cpu = StubCpu::new();
        let mut kernel = fresh_kernel();
        let thunk = dummy_thunk();
        let memory_dc = kernel.gdi.create_memory_dc();
        let custom = kernel
            .gdi
            .create_palette(vec![[255, 0, 255, 0], [33, 66, 99, 0]]);
        kernel.gdi.dc_mut(memory_dc).unwrap().selected_palette = custom;
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let frame_counter = ctx.kernel.framebuffer.frame_counter;
        for (dc, input, expected) in [
            (GDI_SCREEN_DC, 0x0100_0006, 0x0084_8200),
            (memory_dc, 0x0100_0000, 0x00ff_00ff),
            (memory_dc, 0x0100_0001, 0x0063_4121),
            (GDI_SCREEN_DC, 0x0063_4221, 0x0063_4121),
            (GDI_SCREEN_DC, 0x0263_4221, 0x0063_4121),
            (GDI_SCREEN_DC, 0x00ff_ffff, 0x00ff_ffff),
            (memory_dc, 0x0100_0002, u32::MAX),
            (0, 0, u32::MAX),
        ] {
            ctx.cpu.write_reg(ArmReg::R0, dc).unwrap();
            ctx.cpu.write_reg(ArmReg::R1, input).unwrap();
            assert_eq!(
                nearest_color(&mut ctx).unwrap(),
                DispatchOutcome::ReturnedR0(expected)
            );
        }
        assert_eq!(ctx.kernel.framebuffer.frame_counter, frame_counter);
        assert_eq!(
            ctx.kernel.gdi.dc(memory_dc).unwrap().selected_palette,
            custom
        );
    }

    #[test]
    fn logical_palette_entries_are_owned_bounded_and_selected_per_dc() {
        let mut cpu = StubCpu::new();
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.write_mem(0x1000, &[0, 3, 2, 0, 255, 0, 0, 1, 0, 255, 0, 4])
            .unwrap();
        let mut kernel = fresh_kernel();
        let thunk = dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        ctx.cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        let DispatchOutcome::ReturnedR0(handle) = create(&mut ctx).unwrap() else {
            panic!()
        };
        assert_eq!(
            ctx.kernel.gdi.palette(handle).unwrap(),
            &[[255, 0, 0, 1], [0, 255, 0, 4]]
        );
        ctx.cpu.write_reg(ArmReg::R0, handle).unwrap();
        ctx.cpu.write_reg(ArmReg::R1, 1).unwrap();
        ctx.cpu.write_reg(ArmReg::R2, u32::MAX).unwrap();
        ctx.cpu.write_reg(ArmReg::R3, 0x1100).unwrap();
        assert_eq!(
            get_entries(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(ctx.cpu.read_mem(0x1100, 4).unwrap(), [0, 255, 0, 4]);
        let dc = ctx.kernel.gdi.create_memory_dc();
        ctx.cpu.write_reg(ArmReg::R0, dc).unwrap();
        ctx.cpu.write_reg(ArmReg::R1, handle).unwrap();
        let stock = pocket_kernel::gdi::STOCK_DEFAULT_PALETTE;
        assert_eq!(
            select(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(stock)
        );
        assert!(!ctx.kernel.gdi.delete(handle));
        ctx.cpu.write_reg(ArmReg::R1, stock).unwrap();
        select(&mut ctx).unwrap();
        assert!(ctx.kernel.gdi.delete(handle));
        ctx.cpu.write_reg(ArmReg::R0, handle).unwrap();
        assert_eq!(
            get_entries(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(ctx.kernel.thread_last_errors.get(&0), Some(&6));
        ctx.cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        ctx.cpu.write_mem(0x1000, &[0, 3, 1, 1]).unwrap();
        assert_eq!(create(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
    }
}
