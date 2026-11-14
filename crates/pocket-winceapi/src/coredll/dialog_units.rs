//! Read-only dialog geometry used by CE6 Remote Desktop's options pages.
use super::*;

pub(super) fn register(d: &mut WinCeDispatcher, dll: &str) {
    d.register_handler(dll, "MapDialogRect", map);
    d.register_handler(dll, "GetDialogBaseUnits", base_units);
}

fn base_units(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dlu = crate::dlgtemplate::Dlu::default();
    Ok(DispatchOutcome::ReturnedR0(
        (dlu.base_y as u32) << 16 | dlu.base_x as u32,
    ))
}

fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn map(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    let Some([base_x, base_y]) = ctx.kernel.ce.window_aux.dialog_units.get(&hwnd).copied() else {
        return fail(ctx, 1400);
    };
    if p == 0
        || ctx
            .cpu
            .check_guest_access(p, 16, Prot::READ | Prot::WRITE)
            .is_err()
    {
        return fail(ctx, 87);
    }
    let bytes = ctx.cpu.read_mem(p, 16)?;
    let mut mapped = [0u8; 16];
    for (index, pair) in bytes.chunks_exact(4).enumerate() {
        let value = i64::from(i32::from_le_bytes(pair.try_into().unwrap()));
        let (unit, divisor) = if index % 2 == 0 {
            (base_x, 4i64)
        } else {
            (base_y, 8i64)
        };
        let product = value * i64::from(unit);
        // MapDialogRect uses MulDiv rounding, including signed coordinates.
        let rounded = (product + product.signum() * (divisor / 2)) / divisor;
        let Ok(result) = i32::try_from(rounded) else {
            return fail(ctx, 534);
        };
        mapped[index * 4..index * 4 + 4].copy_from_slice(&result.to_le_bytes());
    }
    ctx.cpu.write_mem(p, &mapped)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn dialog_mapping_uses_its_own_font_units_signed_rounding_and_never_paints() {
        let (mut cpu, mut k, mut d) = setup();
        for (hwnd, units) in [(0x111, [7, 14]), (0x112, [10, 20])] {
            k.ce.window_aux.dialog_units.insert(hwnd, units);
        }
        let frame = k.framebuffer.frame_counter;
        for (hwnd, expected) in [(0x111, [-2i32, -2, 9, 9]), (0x112, [-3, -3, 13, 13])] {
            let bytes: Vec<_> = [-1i32, -1, 5, 5]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect();
            cpu.write_mem(0x1800, &bytes).unwrap();
            k.thread_last_errors.insert(0, 99);
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "MapDialogRect",
                    &[hwnd, 0x1800]
                ),
                DispatchOutcome::ReturnedR0(1)
            );
            assert_eq!(
                cpu.read_mem(0x1800, 16).unwrap(),
                expected
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(k.thread_last_errors[&0], 99);
        }
        assert_eq!(k.framebuffer.frame_counter, frame);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetDialogBaseUnits",
                &[]
            ),
            DispatchOutcome::ReturnedR0(0x000e_0007)
        );
    }

    #[test]
    fn invalid_dialogs_rectangles_and_overflow_leave_the_output_unchanged() {
        let (mut cpu, mut k, mut d) = setup();
        k.ce.window_aux.dialog_units.insert(0x111, [7, 14]);
        cpu.write_mem(0x1800, &[0xa5; 16]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "MapDialogRect",
                &[0xbaad, 0x1800]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.thread_last_errors[&0], 1400);
        assert_eq!(cpu.read_mem(0x1800, 16).unwrap(), vec![0xa5; 16]);
        cpu.map_region(0x8000, 0x1000, Prot::READ).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "MapDialogRect",
                &[0x111, 0x8000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.thread_last_errors[&0], 87);
        cpu.write_mem(0x1800, &i32::MAX.to_le_bytes()).unwrap();
        let before = cpu.read_mem(0x1800, 16).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "MapDialogRect",
                &[0x111, 0x1800]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.thread_last_errors[&0], 534);
        assert_eq!(cpu.read_mem(0x1800, 16).unwrap(), before);
        k.discard_window_metadata(&0x111);
        assert!(!k.ce.window_aux.dialog_units.contains_key(&0x111));
    }
}
