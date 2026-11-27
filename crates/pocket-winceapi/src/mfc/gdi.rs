//! CGdiObject ownership operations verified against the CE3 ARM runtime.
use super::handle_maps::Kind;
use super::*;
pub(super) const ATTACH: &str = "__PocketHLE_MFC_Gdi_Attach";
pub(super) const CLEAR_TEMP: &str = "__PocketHLE_MFC_Gdi_ClearTemporary";
pub(super) const FROM_HANDLE: &str = "__PocketHLE_MFC_Gdi_FromHandle";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", ATTACH, attach);
    d.register_handler("coredll.dll", CLEAR_TEMP, clear_temporary);
    d.register_handler("coredll.dll", FROM_HANDLE, from_handle);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (43, "??0CBrush@@QAA@K@Z", solid_brush),
    (104, "??0CGdiObject@@QAA@XZ", construct),
    (1061, "?Detach@CGdiObject@@QAAPAXXZ", detach),
    (
        1476,
        "?GetRuntimeClass@CGdiObject@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (1203, "?FromHandle@CGdiObject@@SAPAV1@PAX@Z", from_handle),
    (1045, "?DeleteTempMap@CGdiObject@@SAXXZ", clear_temporary),
    (
        2509,
        "?wce_CreateFont@@YAPAUHFONT__@@HHHHHEEEEEEEEPBG@Z",
        create_font,
    ),
    (2510, "?wce_CreatePen@@YAPAUHPEN__@@HHK@Z", create_pen),
    (
        800,
        "?Attach@CGdiObject@@QAAHPAX@Z",
        attach as crate::Handler,
    ),
    (1040, "?DeleteObject@CGdiObject@@QAAHXZ", delete),
];

fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(o, 8, pocket_cpu::Prot::WRITE)?;
    let table = super::classes::vtable(ctx, "CGdiObject")?;
    put(ctx, o, table)?;
    put(ctx, o + 4, 0)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CGdiObject",
    )?))
}
fn from_handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(
        super::handle_maps::from_handle(ctx, Kind::Gdi, handle)?,
    ))
}
fn clear_temporary(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::handle_maps::clear_temporary(ctx, Kind::Gdi)
}
fn attach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let handle = ctx.arg_u32(1)?;
    ctx.cpu
        .check_guest_access(object, 8, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let owner = super::application::owner(ctx)?;
    let key = Kind::Gdi.root(ctx, handle);
    if handle == 0
        || word(ctx, object + 4)? != 0
        || ctx.kernel.gdi.get(handle).is_none()
        || ctx.kernel.dynamic_exports[&owner].contains_key(&key)
    {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    put(ctx, object + 4, handle)?;
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key, object);
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn detach_value(ctx: &mut CallCtx<'_>, object: u32) -> Result<u32, KernelError> {
    ctx.cpu
        .check_guest_access(object, 8, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let handle = word(ctx, object + 4)?;
    let owner = super::application::owner(ctx)?;
    let key = Kind::Gdi.root(ctx, handle);
    if ctx.kernel.dynamic_exports[&owner].get(&key) == Some(&object) {
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .remove(&key);
    }
    put(ctx, object + 4, 0)?;
    Ok(handle)
}
fn detach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(detach_value(ctx, object)?))
}
fn delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let handle = detach_value(ctx, object)?;
    if handle == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    ctx.cpu.write_arg(0, handle)?;
    crate::coredll::delete_object(ctx)
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    if flags & 2 != 0 {
        return Err(KernelError::Dispatch(
            "MFC CGdiObject vector deletion is not implemented".into(),
        ));
    }
    let table = super::classes::vtable(ctx, "CGdiObject")?;
    put(ctx, object, table)?;
    delete(ctx)?;
    let table = super::classes::vtable(ctx, "CObject")?;
    put(ctx, object, table)?;
    if flags & 1 != 0 {
        ctx.kernel.heap.free(object);
    }
    Ok(DispatchOutcome::ReturnedR0(object))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn attach_and_delete_track_the_real_gdi_handle_and_detach_the_object() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        let bitmap = k.gdi.create_compatible_bitmap(2, 2);
        let DispatchOutcome::ReturnedR0(result) = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:800",
            &[0x2000, bitmap],
        ) else {
            panic!()
        };
        assert_eq!(result, 1);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), bitmap);
        let DispatchOutcome::ReturnedR0(result) = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1040",
            &[0x2000],
        ) else {
            panic!()
        };
        assert_eq!(result, 1);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 0);
        assert!(k.gdi.bitmap(bitmap).is_none());
    }
}

fn create_font(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let mut font = [0u8; 92];
    for i in 0..5 {
        let value = ctx.arg_u32(i)?;
        font[i as usize * 4..i as usize * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    for i in 0..8 {
        font[20 + i as usize] = ctx.arg_u32(5 + i)? as u8;
    }
    let face = ctx.arg_u32(13)?;
    if face != 0 {
        for (i, c) in crate::coredll::read_wstr(ctx, face, 32)?
            .into_iter()
            .enumerate()
        {
            font[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
    }
    Ok(DispatchOutcome::ReturnedR0(
        crate::coredll::register_gdi_font(ctx.kernel, font),
    ))
}
fn create_pen(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let style = ctx.arg_u32(0)?;
    let width = ctx.arg_u32(1)?;
    let color = ctx.arg_u32(2)?;
    Ok(DispatchOutcome::ReturnedR0(
        crate::coredll::register_gdi_pen(
            ctx.kernel,
            if style == 2 { 1 } else { style },
            width.max(1),
            color,
        ),
    ))
}

#[cfg(test)]
mod constructor_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn ce_font_and_pen_wrappers_create_owned_gdi_objects_and_release_guest_scratch() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let baseline = k.heap.free_bytes();
        let face: Vec<_> = "Tahoma"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &face).unwrap();
        let font = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2509",
            &[
                (-17i32) as u32,
                0,
                0,
                0,
                700,
                1,
                1,
                0,
                1,
                0,
                0,
                5,
                0x22,
                0x1000,
            ],
        ));
        let Some(pocket_kernel::gdi::GdiObject::Font(f)) = k.gdi.get(font) else {
            panic!("A real GDI font must be registered")
        };
        assert_eq!(f.height, -17);
        assert_eq!(
            u32::from_le_bytes(f.logfont[16..20].try_into().unwrap()),
            700
        );
        assert_eq!(&f.logfont[20..28], &[1, 1, 0, 1, 0, 0, 5, 0x22]);
        assert_eq!(&f.logfont[28..28 + face.len()], face);
        assert_eq!(k.heap.free_bytes(), baseline);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:800",
                &[0x2000, font]
            )),
            1
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1040",
                &[0x2000]
            )),
            1
        );
        assert!(k.gdi.get(font).is_none());
        let pen = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2510",
            &[0, 0, 0x00112233],
        ));
        assert_eq!(k.gdi.pen(pen).unwrap().width, 1);
        assert_eq!(k.gdi.pen(pen).unwrap().color, 0x00112233);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:800",
                &[0x2000, pen]
            )),
            1
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1040",
                &[0x2000]
            )),
            1
        );
        assert!(k.gdi.get(pen).is_none());
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

fn solid_brush(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::Frame;
    use pocket_cpu::regs::ArmReg;
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(object, 8, pocket_cpu::Prot::WRITE)?;
        let table = super::classes::vtable(ctx, "CBrush")?;
        put(ctx, object, table)?;
        put(ctx, object + 4, 0)?;
        let color = f.arg(ctx, 1)?;
        return f.call_service(ctx, 1, "CreateSolidBrush", &[color]);
    }
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 if value == 0 => f.call_service(ctx, 4, super::exception_throw::RESOURCE, &[]),
        1 => {
            f.set(ctx, 0, value)?;
            f.call_service(ctx, 2, ATTACH, &[object, value])
        }
        2 if value != 0 => f.finish(ctx, object),
        2 => {
            let handle = f.get(ctx, 0)?;
            f.call_service(ctx, 3, "DeleteObject", &[handle])
        }
        3 => f.call_service(ctx, 4, super::exception_throw::RESOURCE, &[]),
        _ => Err(KernelError::Dispatch(
            "Invalid solid-brush constructor continuation".into(),
        )),
    }
}
