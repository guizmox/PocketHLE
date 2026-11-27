//! CDC adapters use the same live GDI state as direct WinCE drawing calls.
use super::callbacks::{service, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1767, "?MoveTo@CDC@@QAA?AVCPoint@@HH@Z", move_to),
    (1715, "?LineTo@CDC@@QAAHHH@Z", line_to),
    (
        2270,
        "?SelectStockObject@CDC@@UAAPAVCGdiObject@@H@Z",
        select_stock,
    ),
    (
        2264,
        "?SelectGdiObject@CDC@@KAPAVCGdiObject@@PAUHDC__@@PAX@Z",
        select_gdi,
    ),
    (
        2266,
        "?SelectObject@CDC@@QAAPAVCBrush@@PAV2@@Z",
        select_wrapped,
    ),
    (
        2267,
        "?SelectObject@CDC@@QAAPAVCPen@@PAV2@@Z",
        select_wrapped,
    ),
    (
        2268,
        "?SelectObject@CDC@@UAAPAVCFont@@PAV2@@Z",
        select_wrapped,
    ),
    (1114, "?DrawTextW@CDC@@UAAHPBGHPAUtagRECT@@I@Z", draw_text),
    (
        1148,
        "?ExtTextOutW@CDC@@UAAHHHIPBUtagRECT@@PBGIPAH@Z",
        ext_text,
    ),
    (2315, "?SetBkColor@CDC@@UAAKK@Z", background_color),
    (2316, "?SetBkMode@CDC@@QAAHH@Z", background_mode),
    (2434, "?SetTextColor@CDC@@UAAKK@Z", text_color),
];
fn move_to(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // MSVC CE ARM places the hidden CPoint result after this, not before it.
    let o = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let x = ctx.arg_u32(2)?;
    let y = ctx.arg_u32(3)?;
    ctx.cpu
        .check_guest_access(o, 20, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    ctx.cpu
        .check_guest_access(out, 8, pocket_cpu::Prot::WRITE)?;
    let old = ctx.cpu.read_mem(o + 12, 8)?;
    put(ctx, o + 12, x)?;
    put(ctx, o + 16, y)?;
    ctx.cpu.write_mem(out, &old)?;
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn line_to(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let x = f.arg(ctx, 1)?;
    let y = f.arg(ctx, 2)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, 20, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
        let dc = word(ctx, o + 4)?;
        for (i, value) in [word(ctx, o + 12)?, word(ctx, o + 16)?, x, y]
            .into_iter()
            .enumerate()
        {
            put(ctx, f.address + 128 + i as u32 * 4, value)?;
        }
        let target = service(ctx, "Polyline")?;
        return f.call_args(ctx, 1, target, &[dc, f.address + 128, 2]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if result != 0 {
        put(ctx, o + 12, x)?;
        put(ctx, o + 16, y)?;
    }
    f.finish(ctx, result)
}
fn select_stock(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    select(ctx, true)
}
fn select_wrapped(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    select(ctx, false)
}
fn select_gdi(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let args = [f.arg(ctx, 0)?, f.arg(ctx, 1)?];
        let target = service(ctx, "SelectObject")?;
        return f.call_args(ctx, 1, target, &args);
    }
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 1 {
        let target = service(ctx, super::gdi::FROM_HANDLE)?;
        return f.call_args(ctx, 2, target, &[value]);
    }
    f.finish(ctx, value)
}
fn select(ctx: &mut CallCtx<'_>, stock: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let arg = f.arg(ctx, 1)?;
        if stock {
            let target = service(ctx, "GetStockObject")?;
            return f.call_args(ctx, 1, target, &[arg]);
        }
        let handle = if arg == 0 { 0 } else { word(ctx, arg + 4)? };
        f.set(ctx, 0, handle)?;
    } else {
        let value = ctx.cpu.read_reg(ArmReg::R0)?;
        match f.phase(ctx)? {
            1 => f.set(ctx, 0, value)?,
            2 | 3 => f.set(ctx, 1, value)?,
            4 => return f.finish(ctx, value),
            _ => {
                return Err(KernelError::Dispatch(
                    "Invalid MFC GDI selection continuation".into(),
                ))
            }
        }
    }
    let phase = if f.fresh { 0 } else { f.phase(ctx)? };
    let output = word(ctx, o + 4)?;
    let attributes = word(ctx, o + 8)?;
    let handle = f.get(ctx, 0)?;
    if phase <= 1 && output != attributes {
        let target = service(ctx, "SelectObject")?;
        return f.call_args(ctx, 2, target, &[output, handle]);
    }
    if phase <= 2 && attributes != 0 {
        let target = service(ctx, "SelectObject")?;
        return f.call_args(ctx, 3, target, &[attributes, handle]);
    }
    let previous = f.get(ctx, 1)?;
    let target = service(ctx, super::gdi::FROM_HANDLE)?;
    f.call_args(ctx, 4, target, &[previous])
}
fn background_color(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set(ctx, "SetBkColor", u32::MAX)
}
fn background_mode(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set(ctx, "SetBkMode", 0)
}
fn text_color(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set(ctx, "SetTextColor", u32::MAX)
}
fn set(ctx: &mut CallCtx<'_>, name: &str, failure: u32) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let setting = f.arg(ctx, 1)?;
    let output = word(ctx, object + 4)?;
    let attributes = word(ctx, object + 8)?;
    let previous = if f.fresh {
        failure
    } else {
        ctx.cpu.read_reg(ArmReg::R0)?
    };
    if f.fresh && output != attributes {
        let target = service(ctx, name)?;
        return f.call_args(ctx, 1, target, &[output, setting]);
    }
    if (f.fresh || f.phase(ctx)? == 1) && attributes != 0 {
        let target = service(ctx, name)?;
        return f.call_args(ctx, 2, target, &[attributes, setting]);
    }
    f.finish(ctx, previous)
}

fn draw_text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    output(ctx, "DrawTextW", 5)
}
fn ext_text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    output(ctx, "ExtTextOutW", 8)
}
fn output(ctx: &mut CallCtx<'_>, name: &str, count: u8) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    let object = f.arg(ctx, 0)?;
    let mut args = vec![word(ctx, object + 4)?];
    for i in 1..count {
        args.push(ctx.arg_u32(i)?);
    }
    let target = service(ctx, name)?;
    f.call_args(ctx, 1, target, &args)
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn draw_text_forwards_the_stack_flags_to_the_shared_gdi_measurement() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:76",
            &[0x2000],
        );
        let output = k.gdi.create_memory_dc();
        cpu.write_mem(0x2004, &output.to_le_bytes()).unwrap();
        cpu.write_mem(0x1800, &[65, 0, 0, 0]).unwrap();
        for (i, value) in [0u32, 0, 100, 100].into_iter().enumerate() {
            cpu.write_mem(0x1900 + i as u32 * 4, &value.to_le_bytes())
                .unwrap();
        }
        for (i, value) in [0x2000, 0x1800, 1, 0x1900].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        cpu.write_mem(0x4000, &0x400u32.to_le_bytes()).unwrap();
        let free = k.heap.free_bytes();
        let height = drive(&mut cpu, &mut k, &mut d, entry(1114), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert!(height > 0 && height < 100);
        assert_eq!(cpu.read_u32_le(0x190c).unwrap(), height);
        assert!(cpu.read_u32_le(0x1908).unwrap() > 0);
        assert_eq!(k.heap.free_bytes(), free);
    }
    #[test]
    fn setters_update_both_distinct_dcs_and_return_the_attribute_previous_value() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:76",
            &[0x2000],
        );
        let output = k.gdi.create_memory_dc();
        let attributes = k.gdi.create_memory_dc();
        cpu.write_mem(0x2004, &output.to_le_bytes()).unwrap();
        cpu.write_mem(0x2008, &attributes.to_le_bytes()).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SetTextColor",
            &[output, 0x123456],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SetTextColor",
            &[attributes, 0x654321],
        );
        let free = k.heap.free_bytes();
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 0xabcdef).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(2434), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            0x654321
        );
        assert_eq!(k.gdi.dc(output).unwrap().text_color, 0xabcdef);
        assert_eq!(k.gdi.dc(attributes).unwrap().text_color, 0xabcdef);
        assert_eq!(k.heap.free_bytes(), free);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn stock_selection_updates_both_dcs_and_returns_the_previous_attribute_object() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:76",
            &[0x2000],
        );
        let output = k.gdi.create_memory_dc();
        let attributes = k.gdi.create_memory_dc();
        let first = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreatePen",
            &[0, 1, 0x0000ff],
        ));
        let second = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreatePen",
            &[0, 1, 0xff0000],
        ));
        k.gdi.select_into(output, first);
        k.gdi.select_into(attributes, second);
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, output).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(799), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2312",
            &[0x2000, attributes],
        );
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 7).unwrap();
        let previous = drive(&mut cpu, &mut k, &mut d, entry(2270), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert_eq!(cpu.read_u32_le(previous + 4).unwrap(), second);
        assert_eq!(
            k.gdi.dc(output).unwrap().selected_pen,
            pocket_kernel::gdi::STOCK_BLACK_PEN
        );
        assert_eq!(
            k.gdi.dc(attributes).unwrap().selected_pen,
            pocket_kernel::gdi::STOCK_BLACK_PEN
        );
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, previous).unwrap();
        let stock = drive(&mut cpu, &mut k, &mut d, entry(2267), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert_eq!(
            cpu.read_u32_le(stock + 4).unwrap(),
            pocket_kernel::gdi::STOCK_BLACK_PEN
        );
        assert_eq!(k.gdi.dc(output).unwrap().selected_pen, second);
        assert_eq!(k.gdi.dc(attributes).unwrap().selected_pen, second);
        drive(&mut cpu, &mut k, &mut d, entry(1045), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert!(k.gdi.get(second).is_some());
        assert!(k.gdi.get(pocket_kernel::gdi::STOCK_BLACK_PEN).is_some());
    }
}

#[cfg(test)]
mod line_tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn ce_point_return_and_logical_current_position_drive_real_polyline_pixels() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:76",
            &[0x2000],
        );
        let dc = k.gdi.create_memory_dc();
        let bitmap = k.gdi.create_compatible_bitmap(10, 10);
        k.gdi.select_into(dc, bitmap);
        k.gdi.select_into(dc, pocket_kernel::gdi::STOCK_WHITE_PEN);
        k.gdi.dc_mut(dc).unwrap().current_position = (9, 9);
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, dc).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(799), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1767",
                &[0x2000, 0x2200, 1, 1]
            )),
            0x2200
        );
        assert_eq!(cpu.read_mem(0x2200, 8).unwrap(), [0; 8]);
        assert_eq!(k.gdi.dc(dc).unwrap().current_position, (9, 9));
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 5).unwrap();
        cpu.write_arg(2, 1).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1715), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            1
        );
        assert_eq!(&k.gdi.bitmap(bitmap).unwrap().pixels[24..26], &[255, 255]);
        assert_eq!(cpu.read_u32_le(0x200c).unwrap(), 5);
        assert_eq!(cpu.read_u32_le(0x2010).unwrap(), 1);
        assert_eq!(k.gdi.dc(dc).unwrap().current_position, (9, 9));
        // A rejected Polyline must not advance the MFC logical position.
        cpu.write_mem(0x2004, &0x1234u32.to_le_bytes()).unwrap();
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 7).unwrap();
        cpu.write_arg(2, 3).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1715), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            0
        );
        assert_eq!(cpu.read_u32_le(0x200c).unwrap(), 5);
    }
}
