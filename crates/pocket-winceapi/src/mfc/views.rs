//! CView owns its document association; painting uses the existing CPaintDC
//! and guest TRY owners. HWND/GDI resources are never mirrored in a view cache.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const ACTIVE: &str = "__PocketHLE_MFC_SetActiveView";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", ACTIVE, active);
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    super::windows::initialize_target(ctx, object, "CView", 76)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CView",
    )?))
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    delete(ctx, false)
}
pub(super) fn delete(
    ctx: &mut CallCtx<'_>,
    deleting: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let flags = f.arg(ctx, 1)?;
        if deleting && flags & !1 != 0 {
            return Err(KernelError::Dispatch(
                "MFC CView vector deletion is unsupported".into(),
            ));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let table = super::classes::vtable(ctx, "CView")?;
        put(ctx, object, table)?;
        let state = super::application::current_thread_state(ctx)?;
        if word(ctx, state + 0x128)? == object {
            put(ctx, state + 0x128, 0)?;
        }
        let document = word(ctx, object + 0x48)?;
        if document != 0 {
            return f.call_service(ctx, 1, super::documents::REMOVE, &[document, object]);
        }
    }
    if f.fresh || f.phase(ctx)? == 1 {
        return f.call_service(ctx, 2, super::window_lifetime::DESTRUCT, &[object]);
    }
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(object);
    }
    f.finish(ctx, object)
}

fn pre_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let cs = ctx.arg_u32(1)?;
    if word(ctx, cs + 40)? == 0 {
        let class = super::frame_loading::standard_class(ctx, true)?;
        put(ctx, cs + 40, class)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn on_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, "__PocketHLE_MFC_WindowDefault", &[object]);
    }
    if f.phase(ctx)? == 1 {
        if ctx.cpu.read_reg(ArmReg::R0)? == u32::MAX {
            return f.finish(ctx, u32::MAX);
        }
        let cs = f.arg(ctx, 1)?;
        let context = word(ctx, cs)?;
        if context != 0 {
            let document = word(ctx, context + 4)?;
            if document != 0 {
                return f.call_service(ctx, 2, super::documents::ADD, &[document, object]);
            }
        }
    }
    f.finish(ctx, 0)
}
fn on_destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::hierarchy::PARENT, &[object]);
    }
    if f.phase(ctx)? == 1 {
        let frame = ctx.cpu.read_reg(ArmReg::R0)?;
        if frame != 0 && word(ctx, frame + 0xac)? == object {
            return f.call_service(ctx, 2, ACTIVE, &[frame, 0, 1]);
        }
    }
    if f.phase(ctx)? <= 2 {
        return f.call_service(ctx, 3, super::window_lifetime::ON_DESTROY, &[object]);
    }
    f.finish(ctx, 0)
}
fn initial(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    let target = virtual_function(ctx, object, 53)?;
    f.call_args(ctx, 1, target, &[object, 0, 0, 0])
}
fn update(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 1, "InvalidateRect", &[hwnd, 0, 1])
}
fn activate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        if f.arg(ctx, 1)? == 0 {
            return f.finish(ctx, 0);
        }
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, "IsWindowVisible", &[hwnd]);
    }
    if f.phase(ctx)? == 1 && ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        return f.call_service(ctx, 2, super::window_position::FOCUS, &[object]);
    }
    f.finish(ctx, 0)
}
fn active(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let frame = f.arg(ctx, 0)?;
    let view = f.arg(ctx, 1)?;
    if f.fresh {
        let old = word(ctx, frame + 0xac)?;
        if old == view {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, old)?;
        put(ctx, frame + 0xac, 0)?;
        if old != 0 {
            let target = virtual_function(ctx, old, 51)?;
            return f.call_args(ctx, 1, target, &[old, 0, view, old]);
        }
    }
    if f.fresh || f.phase(ctx)? == 1 {
        // A deactivation override may have selected a different view itself.
        if word(ctx, frame + 0xac)? != 0 {
            return f.finish(ctx, 0);
        }
        put(ctx, frame + 0xac, view)?;
        if view != 0 && f.arg(ctx, 2)? != 0 {
            let old = f.get(ctx, 0)?;
            let target = virtual_function(ctx, view, 51)?;
            return f.call_args(ctx, 2, target, &[view, 1, view, old]);
        }
    }
    f.finish(ctx, 0)
}
fn command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let view = f.arg(ctx, 0)?;
    if f.fresh {
        let info = ctx.arg_u32(4)?;
        f.set(ctx, 0, info)?;
        let args = [view, f.arg(ctx, 1)?, f.arg(ctx, 2)?, f.arg(ctx, 3)?, info];
        return f.call_service(ctx, 1, super::command_routing::BASE, &args);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 1 {
        if result != 0 {
            return f.finish(ctx, 1);
        }
        let document = word(ctx, view + 0x48)?;
        if document == 0 {
            return f.finish(ctx, 0);
        }
        let state = super::application::current_thread_state(ctx)?;
        f.save_region(ctx, 128, state + 0x128, 4)?;
        put(ctx, state + 0x128, view)?;
        let target = virtual_function(ctx, document, 5)?;
        let args = [
            document,
            f.arg(ctx, 1)?,
            f.arg(ctx, 2)?,
            f.arg(ctx, 3)?,
            f.get(ctx, 0)?,
        ];
        return f.call_args(ctx, 2, target, &args);
    }
    f.finish(ctx, u32::from(result != 0))
}
fn paint(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let view = f.arg(ctx, 0)?;
    if f.fresh {
        let dc = alloc_cpp(ctx, 88)?;
        f.own(ctx, dc)?;
        f.set(ctx, 0, dc)?;
        return f.call_service(ctx, 1, super::window_dcs::PAINT_CONSTRUCT, &[dc, view]);
    }
    let dc = f.get(ctx, 0)?;
    let phase = f.phase(ctx)?;
    if (2..=3).contains(&phase) {
        let exception = word(ctx, f.address + 212)?;
        if exception != 0 || phase == 3 {
            f.set(ctx, 1, exception)?;
            // Hand ownership of an escaping exception back to the outer TRY;
            // CPaintDC must end painting before that nonlocal return.
            put(ctx, f.address + 212, 0)?;
            super::exception_links::unlink(ctx, f.address + 160)?;
            return f.call_service(ctx, 4, super::window_dcs::PAINT_DESTRUCT, &[dc]);
        }
    }
    match phase {
        1 => {
            super::exception_links::begin_catch(ctx, &f, 160)?;
            let target = virtual_function(ctx, view, 49)?;
            f.call_args(ctx, 2, target, &[view, dc, 0])
        }
        2 => {
            let target = virtual_function(ctx, view, 54)?;
            f.call_args(ctx, 3, target, &[view, dc])
        }
        4 => {
            let exception = f.get(ctx, 1)?;
            if exception != 0 {
                return f.call_service(ctx, 5, super::exception_throw::THROW, &[exception]);
            }
            f.finish(ctx, 0)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC view paint continuation".into(),
        )),
    }
}
fn empty_base(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(object, 76, Prot::READ)?;
    // Retail OnPrepareDC/OnActivateFrame are empty and base scrolling/selection
    // return FALSE. Derived methods are selected through the real guest vtable.
    Ok(DispatchOutcome::ReturnedR0(0))
}

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (200, "??0CView@@IAA@XZ", construct),
    (309, "??1CView@@UAA@XZ", destruct),
    (
        1538,
        "?GetRuntimeClass@CView@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (
        2097,
        "?PreCreateWindow@CView@@MAAHAAUtagCREATESTRUCTW@@@Z",
        pre_create,
    ),
    (
        1852,
        "?OnCreate@CView@@IAAHPAUtagCREATESTRUCTW@@@Z",
        on_create,
    ),
    (1871, "?OnDestroy@CView@@IAAXXZ", on_destroy),
    (
        2087,
        "?PostNcDestroy@CView@@MAAXXZ",
        super::window_lifetime::post_nc_delete,
    ),
    (1946, "?OnInitialUpdate@CView@@UAAXXZ", initial),
    (2037, "?OnUpdate@CView@@MAAXPAV1@JPAVCObject@@@Z", update),
    (1794, "?OnActivateView@CView@@MAAXHPAV1@0@Z", activate),
    (2301, "?SetActiveView@CFrameWnd@@QAAXPAVCView@@H@Z", active),
    (
        1835,
        "?OnCmdMsg@CView@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
        command,
    ),
    (1982, "?OnPaint@CView@@IAAXXZ", paint),
    (2761, "?OnPrepareDC@CView@@UAAXPAVCDC@@PBX@Z", empty_base),
    (
        1790,
        "?OnActivateFrame@CView@@MAAXIPAVCFrameWnd@@@Z",
        empty_base,
    ),
    (2003, "?OnScroll@CView@@UAAHIIH@Z", empty_base),
    (2005, "?OnScrollBy@CView@@UAAHVCSize@@H@Z", empty_base),
    (1704, "?IsSelected@CView@@UBAHPBVCObject@@@Z", empty_base),
];

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn view_paint_uses_a_live_paint_dc_and_releases_the_wrapper_and_try_on_each_frame() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:200",
            &[0x2000],
        );
        k.window_classes.insert(7, "PaintView".into());
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        let table = cpu.read_u32_le(0x2000).unwrap();
        let methods = cpu.read_mem(table, 55 * 4).unwrap();
        cpu.write_mem(0x2800, &methods).unwrap();
        cpu.write_mem(0x2800 + 54 * 4, &0x6100u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
        let mut freed = None;
        for _ in 0..3 {
            cpu.write_arg(0, 0x2000).unwrap();
            let mut drawn = 0;
            drive(&mut cpu, &mut k, &mut d, entry(1982), |pc, cpu| {
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                let dc = cpu.read_arg(1).unwrap();
                let handle = cpu.read_u32_le(dc + 4).unwrap();
                assert_ne!(handle, 0);
                assert_eq!(cpu.read_u32_le(dc + 8).unwrap(), handle);
                drawn += 1;
                0
            });
            assert_eq!(drawn, 1);
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|n| n.starts_with("__PocketHLE_MFC_CDC:0:")
                    || n.starts_with("__PocketHLE_MFC_Frame:")));
            if let Some(free) = freed {
                assert_eq!(k.heap.free_bytes(), free);
            }
            freed = Some(k.heap.free_bytes());
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        }
    }
}

#[cfg(test)]
mod exception_tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, drive_outcomes, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::Dispatcher;
    #[test]
    fn draw_exception_closes_the_paint_dc_before_propagating_to_the_window_boundary() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:200",
            &[0x2000],
        );
        cpu.write_arg(0, 0x2200).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(204), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        let mt = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:645",
            &[],
        ));
        cpu.write_mem(mt + 4, &0x2200u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        k.window_classes.insert(7, "PaintView".into());
        let table = cpu.read_u32_le(0x2000).unwrap();
        let methods = cpu.read_mem(table, 55 * 4).unwrap();
        cpu.write_mem(0x2800, &methods).unwrap();
        cpu.write_mem(0x2800 + 54 * 4, &0x6100u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
        let mut freed = None;
        for _ in 0..2 {
            for (i, v) in [0x2000u32, 7, 15, 0].into_iter().enumerate() {
                cpu.write_arg(i as u8, v).unwrap();
            }
            cpu.write_mem(0x4000, &0u32.to_le_bytes()).unwrap();
            let mut throws = 0;
            drive_outcomes(
                &mut cpu,
                &mut k,
                &mut d,
                entry(606),
                1024,
                |pc, cpu, k, d| {
                    assert_eq!(pc, 0x6100);
                    let dc = cpu.read_arg(1).unwrap();
                    assert_ne!(cpu.read_u32_le(dc + 4).unwrap(), 0);
                    throws += 1;
                    let mut throw = entry(711);
                    throw.thunk_va = 0x70000040;
                    d.dispatch(cpu, &throw, k).unwrap()
                },
            );
            assert_eq!(throws, 1);
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|n| n.starts_with("__PocketHLE_MFC_CDC:0:")
                    || n.starts_with("__PocketHLE_MFC_Frame:")));
            if let Some(free) = freed {
                assert_eq!(k.heap.free_bytes(), free);
            }
            freed = Some(k.heap.free_bytes());
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        }
    }
}
