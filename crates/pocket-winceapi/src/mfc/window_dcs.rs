//! Scoped client/paint DCs borrow shared window backing through the CDC owner.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};

pub(super) const PAINT_CONSTRUCT: &str = "__PocketHLE_MFC_ConstructPaintDC";
pub(super) const PAINT_DESTRUCT: &str = "__PocketHLE_MFC_DestructPaintDC";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", PAINT_CONSTRUCT, paint_construct);
    d.register_handler("coredll.dll", PAINT_DESTRUCT, paint_destruct);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (67, "??0CClientDC@@QAA@PAVCWnd@@@Z", client_construct),
    (136, "??0CPaintDC@@QAA@PAVCWnd@@@Z", paint_construct),
    (225, "??1CClientDC@@UAA@XZ", client_destruct),
    (271, "??1CPaintDC@@UAA@XZ", paint_destruct),
    (
        1449,
        "?GetRuntimeClass@CClientDC@@UBAPAUCRuntimeClass@@XZ",
        client_runtime,
    ),
    (
        1503,
        "?GetRuntimeClass@CPaintDC@@UBAPAUCRuntimeClass@@XZ",
        paint_runtime,
    ),
];
fn client_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CClientDC",
    )?))
}
fn paint_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CPaintDC",
    )?))
}
fn client_construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, false)
}
fn paint_construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, true)
}
fn construct(ctx: &mut CallCtx<'_>, paint: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        // The retail CPaintDC contains the full 64-byte PAINTSTRUCT, even
        // though the shared CE BeginPaint currently fills its public prefix.
        ctx.cpu
            .check_guest_access(o, if paint { 88 } else { 24 }, Prot::READ | Prot::WRITE)?;
        let window = f.arg(ctx, 1)?;
        let hwnd = if window == 0 && !paint {
            0
        } else {
            word(ctx, window + 0x20)?
        };
        super::device_contexts::construct_base(ctx, o)?;
        let table = super::classes::vtable(ctx, if paint { "CPaintDC" } else { "CClientDC" })?;
        put(ctx, o, table)?;
        put(ctx, o + 0x14, hwnd)?;
        return f.call_service(
            ctx,
            1,
            if paint { "BeginPaint" } else { "GetDC" },
            &[hwnd, o + 0x18],
        );
    }
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            f.set(ctx, 0, returned)?;
            f.call_service(ctx, 2, super::device_contexts::ATTACH, &[o, returned])
        }
        2 if returned != 0 => f.finish(ctx, o),
        2 => {
            let hwnd = word(ctx, o + 0x14)?;
            let arg = if paint { o + 0x18 } else { f.get(ctx, 0)? };
            f.call_service(
                ctx,
                3,
                if paint { "EndPaint" } else { "ReleaseDC" },
                &[hwnd, arg],
            )
        }
        3 => Err(KernelError::Dispatch(
            "MFC scoped DC could not attach the acquired window DC".into(),
        )),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC scoped DC construction continuation".into(),
        )),
    }
}
fn client_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, false, false)
}
fn paint_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, true, false)
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>, paint: bool) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, paint, true)
}
fn destruct(
    ctx: &mut CallCtx<'_>,
    paint: bool,
    deleting: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, if paint { 88 } else { 24 }, Prot::READ | Prot::WRITE)?;
        let free = deleting && f.arg(ctx, 1)? & 1 != 0;
        f.set(ctx, 0, u32::from(free))?;
        let table = super::classes::vtable(ctx, if paint { "CPaintDC" } else { "CClientDC" })?;
        put(ctx, o, table)?;
        if paint {
            let hwnd = word(ctx, o + 0x14)?;
            return f.call_service(ctx, 2, "EndPaint", &[hwnd, o + 0x18]);
        }
        return f.call_service(ctx, 1, super::device_contexts::DETACH, &[o]);
    }
    match f.phase(ctx)? {
        1 => {
            let hdc = ctx.cpu.read_reg(ArmReg::R0)?;
            let hwnd = word(ctx, o + 0x14)?;
            f.call_service(ctx, 3, "ReleaseDC", &[hwnd, hdc])
        }
        2 => f.call_service(ctx, 4, super::device_contexts::DETACH, &[o]),
        3 | 4 => f.call_service(ctx, 5, super::device_contexts::DESTRUCT, &[o]),
        5 => {
            if f.get(ctx, 0)? != 0 {
                ctx.kernel.heap.free(o);
            }
            f.finish(ctx, o)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC scoped DC destruction continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn scoped_window_dcs_detach_without_deleting_window_backing_or_caller_storage() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        k.window_classes.insert(7, "PaintTarget".into());
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        for (ctor, dtor, bytes) in [(67, 225, 24), (136, 271, 88)] {
            let mut warmed_free_bytes = None;
            for _ in 0..2 {
                cpu.write_mem(0x2400, &[0x5a; 92]).unwrap();
                cpu.write_arg(0, 0x2400).unwrap();
                cpu.write_arg(1, 0x2000).unwrap();
                assert_eq!(
                    drive(&mut cpu, &mut k, &mut d, entry(ctor), |pc, _| panic!(
                        "unexpected guest {pc:x}"
                    )),
                    0x2400
                );
                let dc = cpu.read_u32_le(0x2404).unwrap();
                assert_ne!(dc, 0);
                assert_eq!(cpu.read_u32_le(0x2408).unwrap(), dc);
                assert_eq!(
                    k.dynamic_exports[&1][&format!("__PocketHLE_MFC_CDC:0:{dc:08x}")],
                    0x2400
                );
                assert_eq!(cpu.read_mem(0x2400 + bytes, 4).unwrap(), [0x5a; 4]);
                cpu.write_arg(0, 0x2400).unwrap();
                drive(&mut cpu, &mut k, &mut d, entry(dtor), |pc, _| {
                    panic!("unexpected guest {pc:x}")
                });
                assert!(k.gdi.dc(dc).is_some());
                assert_eq!(cpu.read_mem(0x2404, 8).unwrap(), [0; 8]);
                assert!(
                    !k.dynamic_exports[&1].contains_key(&format!("__PocketHLE_MFC_CDC:0:{dc:08x}"))
                );
                if let Some(before) = warmed_free_bytes {
                    assert_eq!(k.heap.free_bytes(), before);
                }
                warmed_free_bytes = Some(k.heap.free_bytes());
            }
        }
    }
}
