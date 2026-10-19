//! WinCE asks the parent for a control's brush and DC text attributes.
use super::*;
use pocket_kernel::gdi::ControlColorFrame;

pub(super) fn resume(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if !ctx.kernel.gdi.control_color_frames.last().is_some_and(|f| {
        f.thread == ctx.kernel.current_thread && f.thunk == ctx.thunk.thunk_va && f.frame.sp == sp
    }) {
        return Ok(None);
    }
    let f = ctx.kernel.gdi.control_color_frames.pop().unwrap();
    let brush = ctx.cpu.read_reg(ArmReg::R0)?;
    let color = class_background_color(ctx, brush);
    let text_color = ctx.kernel.gdi.dc(f.dc).map_or(0, |d| d.text_color);
    if f.dialog {
        if let Some(color) = color {
            let old = ctx.kernel.gdi.dialog_backgrounds.insert(f.hwnd, color);
            if old != Some(color) || ctx.kernel.gdi.pending_window_erases.contains(&f.hwnd) {
                let dc = window_dc(ctx, f.hwnd);
                let size = ctx.kernel.window_rects.get(&f.hwnd).map(|r| (r.2, r.3));
                if let Some((w, h)) = size {
                    if let Some(mut surface) = surface_for_dc(ctx.kernel, dc) {
                        surface.fill_rect(0, 0, w, h, colorref_to_rgb565(color));
                    }
                }
            }
            ctx.kernel.gdi.pending_window_erases.remove(&f.hwnd);
        } else if f.frame.args[1] == WM_ERASEBKGND {
            // An unhandled colour request must retain the ordinary class
            // background erasure, rather than suppressing it for other apps.
            let dc = window_dc(ctx, f.hwnd);
            if dc == GDI_SCREEN_DC {
                erase_window_background(ctx);
            } else {
                erase_child_background(ctx, f.hwnd, dc)?;
            }
        }
    } else if let Some(c) = ctx.kernel.controls.get_mut(f.hwnd) {
        // Native push buttons keep their system face; check/radio labels
        // and statics use the parent's text/background attributes.
        if c.class != ControlClass::Button || (2..=9).contains(&(c.style & 0xf)) {
            c.text_color = text_color;
            c.background_color = color;
        }
    }
    ctx.kernel.gdi.delete(f.dc);
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(f.frame.args)
    {
        ctx.cpu.write_reg(r, v)?;
    }
    ctx.cpu.write_reg(ArmReg::Lr, f.frame.lr)?;
    ctx.cpu.write_reg(ArmReg::Sp, f.frame.sp)?;
    ctx.kernel.framebuffer.mark_dirty();
    Ok(Some(DispatchOutcome::ReturnedR0(u32::from(
        f.frame.args[1] == WM_ERASEBKGND && color.is_some(),
    ))))
}

pub(super) fn paint(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    dialog: bool,
) -> Result<DispatchOutcome, KernelError> {
    let (parent, message, font) = if dialog {
        (hwnd, 0x136, STOCK_SYSTEM_FONT)
    } else {
        let c = ctx.kernel.controls.get(hwnd).unwrap();
        let msg = match c.class {
            ControlClass::Edit if c.enabled && c.style & 0x800 == 0 => 0x133,
            ControlClass::ListBox => 0x134,
            ControlClass::Button => 0x135,
            _ => 0x138,
        };
        (c.parent, msg, c.font)
    };
    let proc = ctx.kernel.window_procs.get(&parent).copied().unwrap_or(0);
    if proc == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let frame = GuestCallFrame {
        args: [
            ctx.arg_u32(0)?,
            ctx.arg_u32(1)?,
            ctx.arg_u32(2)?,
            ctx.arg_u32(3)?,
        ],
        sp: ctx.cpu.read_reg(ArmReg::Sp)?,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    let dc = ctx.kernel.gdi.create_memory_dc();
    ctx.kernel.gdi.dc_mut(dc).unwrap().selected_font = font;
    ctx.kernel.gdi.control_color_frames.push(ControlColorFrame {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame,
        hwnd,
        dc,
        dialog,
    });
    ctx.cpu.write_reg(ArmReg::R0, parent)?;
    ctx.cpu.write_reg(ArmReg::R1, message)?;
    ctx.cpu.write_reg(ArmReg::R2, dc)?;
    ctx.cpu.write_reg(ArmReg::R3, hwnd)?;
    ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
    Ok(DispatchOutcome::JumpTo(proc & !1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::{stub::StubCpu, Cpu};
    #[test]
    fn navigator_skin_static_uses_native_text_color_and_brush() {
        let mut cpu = StubCpu::new();
        let mut kernel = super::super::tests::fresh_kernel();
        let hwnd = kernel.controls.create(
            FAKE_HWND,
            ControlClass::Static,
            1557,
            "Welcome".into(),
            0x50000000,
            0,
            0,
            100,
            20,
        );
        kernel.window_procs.insert(FAKE_HWND, 0x6000);
        let brush = kernel.gdi.create_solid_brush(0x00efe2d6);
        let thunk = super::super::tests::dummy_thunk();
        for (r, v) in [
            (ArmReg::R0, hwnd),
            (ArmReg::R1, WM_PAINT),
            (ArmReg::R2, 0),
            (ArmReg::R3, 0),
            (ArmReg::Sp, 0x1800),
            (ArmReg::Lr, 0x8000),
        ] {
            cpu.write_reg(r, v).unwrap();
        }
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        assert_eq!(
            paint(&mut ctx, hwnd, false).unwrap(),
            DispatchOutcome::JumpTo(0x6000)
        );
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), 0x138);
        let dc = ctx.cpu.read_reg(ArmReg::R2).unwrap();
        ctx.kernel.gdi.dc_mut(dc).unwrap().text_color = 0x99;
        ctx.cpu.write_reg(ArmReg::R0, brush).unwrap();
        assert_eq!(
            resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::ReturnedR0(0))
        );
        let c = ctx.kernel.controls.get(hwnd).unwrap();
        assert_eq!(c.text_color, 0x99);
        assert_eq!(c.background_color, Some(0x00efe2d6));
        assert!(ctx.kernel.gdi.dc(dc).is_none());
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x8000);
    }
}
