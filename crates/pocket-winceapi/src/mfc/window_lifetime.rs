//! CE CWnd lifetime: real window callbacks precede detachment and PostNcDestroy.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const ON_DESTROY: &str = "__PocketHLE_MFC_OnDestroyWindow";
pub(super) const DESTROY: &str = "__PocketHLE_MFC_DestroyWindow";
pub(super) const DESTRUCT: &str = "__PocketHLE_MFC_DestructWindow";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1056, "?DestroyWindow@CWnd@@UAAHXZ", destroy),
    (1872, "?OnDestroy@CWnd@@IAAXXZ", on_destroy),
    (1960, "?OnNcDestroy@CWnd@@IAAXXZ", on_nc_destroy),
    (313, "??1CWnd@@UAA@XZ", destruct),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", ON_DESTROY, on_destroy);
    d.register_handler("coredll.dll", DESTROY, destroy);
    d.register_handler("coredll.dll", DESTRUCT, destruct);
}
fn destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let hwnd = word(ctx, o + 0x20)?;
        if hwnd == 0 {
            return f.finish(ctx, 0);
        }
        if word(ctx, o + 0x40)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC OLE site window destruction is not implemented".into(),
            ));
        }
        let owner = super::application::owner(ctx)?;
        let permanent = ctx.kernel.dynamic_exports[&owner]
            .get(&format!("__PocketHLE_MFC_Window:{hwnd:08x}"))
            == Some(&o);
        f.set(ctx, 0, u32::from(permanent))?;
        return f.call_service(ctx, 1, "DestroyWindow", &[hwnd]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    // A permanent object may already have deleted itself in PostNcDestroy.
    if f.get(ctx, 0)? == 0 {
        super::window_handles::detach_value(ctx, o)?;
    }
    f.finish(ctx, result)
}
fn on_destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        if word(ctx, o + 0x44)? != 0 {
            return f.call_service(ctx, 3, super::done_button::SHOW, &[o, 0]);
        }
    }
    if f.fresh || f.phase(ctx)? == 3 {
        let container = word(ctx, o + 0x3c)?;
        if container != 0 {
            let target = virtual_function(ctx, container, 1)?;
            return f.call_args(ctx, 1, target, &[container, 1]);
        }
    }
    if f.fresh || matches!(f.phase(ctx)?, 1 | 3) {
        put(ctx, o + 0x3c, 0)?;
        return f.call_service(ctx, 2, "__PocketHLE_MFC_WindowDefault", &[o]);
    }
    f.finish(ctx, 0)
}
fn on_nc_destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, 72, Prot::READ | Prot::WRITE)?;
        let thread = super::application::thread_object(ctx)?;
        if thread != 0 {
            if word(ctx, thread + 0x20)? == o {
                let module = super::application::state(ctx)?;
                let dll = ctx.cpu.read_mem(module + 0x14, 1)?[0] != 0;
                let app = word(ctx, module + 4)?;
                let can_exit = thread != app || word(ctx, module + 0x34)? == 0;
                put(ctx, thread + 0x20, 0)?;
                if !dll && can_exit {
                    return f.call_service(ctx, 10, super::frame_close::POST_QUIT, &[0]);
                }
            }
        }
        return nc_prepare(ctx, f, o);
    }
    match f.phase(ctx)? {
        10 => nc_prepare(ctx, f, o),
        1 => {
            put(ctx, o + 0x3c, 0)?;
            nc_default(ctx, f, o)
        }
        2 => {
            f.set(ctx, 1, result)?;
            f.call_service(ctx, 3, "__PocketHLE_MFC_WindowDefault", &[o])
        }
        3 => {
            let hwnd = f.get(ctx, 0)?;
            f.call_service(ctx, 4, "GetWindowLongW", &[hwnd, (-4i32) as u32])
        }
        4 => {
            if result != f.get(ctx, 1)? {
                return nc_finish(ctx, f, o);
            }
            let target = virtual_function(ctx, o, 32)?;
            f.call_args(ctx, 5, target, &[o])
        }
        5 => {
            let procedure = word(ctx, result)?;
            if procedure == 0 {
                return nc_finish(ctx, f, o);
            }
            let hwnd = f.get(ctx, 0)?;
            f.call_service(ctx, 6, "SetWindowLongW", &[hwnd, (-4i32) as u32, procedure])
        }
        6 => nc_finish(ctx, f, o),
        7 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC nonclient destruction continuation".into(),
        )),
    }
}
fn nc_prepare(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let thread = super::application::thread_object(ctx)?;
    if thread != 0 && word(ctx, thread + 0x24)? == o {
        put(ctx, thread + 0x24, 0)?;
    }
    put(ctx, o + 0x38, 0)?;
    let hwnd = word(ctx, o + 0x20)?;
    f.set(ctx, 0, hwnd)?;
    let container = word(ctx, o + 0x3c)?;
    if container != 0 {
        let target = virtual_function(ctx, container, 1)?;
        return f.call_args(ctx, 1, target, &[container, 1]);
    }
    nc_default(ctx, f, o)
}
fn nc_default(ctx: &mut CallCtx<'_>, f: Frame, _o: u32) -> Result<DispatchOutcome, KernelError> {
    let hwnd = f.get(ctx, 0)?;
    f.call_service(ctx, 2, "GetWindowLongW", &[hwnd, (-4i32) as u32])
}
fn nc_finish(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    super::window_handles::detach_value(ctx, o)?;
    let target = virtual_function(ctx, o, 41)?;
    f.call_args(ctx, 7, target, &[o])
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_impl(ctx, false)
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_impl(ctx, true)
}
fn destruct_impl(ctx: &mut CallCtx<'_>, deleting: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, 72, Prot::READ | Prot::WRITE)?;
        let flags = f.arg(ctx, 1)?;
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let table = super::classes::vtable(ctx, "CWnd")?;
        put(ctx, o, table)?;
        if word(ctx, o + 0x20)? != 0 {
            return f.call_service(ctx, 1, DESTROY, &[o]);
        }
    }
    if f.fresh || f.phase(ctx)? == 1 {
        let container = word(ctx, o + 0x3c)?;
        if container != 0 {
            let target = virtual_function(ctx, container, 1)?;
            return f.call_args(ctx, 2, target, &[container, 1]);
        }
    }
    if f.fresh || matches!(f.phase(ctx)?, 1 | 2) {
        put(ctx, o + 0x3c, 0)?;
        let site = word(ctx, o + 0x40)?;
        if site != 0 && word(ctx, site + 0x28)? == o {
            put(ctx, site + 0x28, 0)?;
        }
        return f.call_service(ctx, 3, super::command_targets::DESTRUCT, &[o]);
    }
    let free = f.get(ctx, 0)? != 0;
    if free {
        ctx.kernel.heap.free(o);
    }
    f.finish(ctx, o)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn ce_destroy_detaches_before_post_nc_override_and_never_reuses_the_deleted_object() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let table = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(table, 46 * 4).unwrap();
        cpu.write_mem(0x2600, &bytes).unwrap();
        cpu.write_mem(0x2600 + 41 * 4, &0x6100u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2000, &0x2600u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2200, &0x2800u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2800 + 24 * 4, &0x6200u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2804, &0x6300u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x203c, &0x2200u32.to_le_bytes()).unwrap();
        k.window_classes.insert(7, "Window".into());
        let base = k.dynamic_exports[&1]["DefWindowProcW"];
        k.window_procs.insert(7, base);
        for (i, v) in [0x2000, 7].into_iter().enumerate() {
            cpu.write_arg(i as u8, v).unwrap();
        }
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(2460), |pc, _| panic!(
                "unexpected guest {pc:x}"
            )),
            1
        );
        let mut post_calls = 0;
        let mut container_calls = Vec::new();
        cpu.write_arg(0, 0x2000).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1056), |pc, cpu| {
                if matches!(pc, 0x6200 | 0x6300) {
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                    assert_eq!(cpu.read_arg(1).unwrap(), u32::from(pc == 0x6300));
                    container_calls.push(pc);
                    if pc == 0x6300 {
                        cpu.write_mem(0x2200, &[0xff; 4]).unwrap();
                    }
                    return 0;
                }
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_u32_le(0x2020).unwrap(), 0);
                post_calls += 1;
                // PostNcDestroy may delete this. Poison all fields to catch later reads.
                cpu.write_mem(0x2000, &[0xff; 72]).unwrap();
                0
            }),
            1
        );
        assert_eq!(post_calls, 1);
        assert_eq!(container_calls, [0x6200, 0x6300]);
        assert!(!k.window_classes.contains_key(&7));
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|s| s.starts_with("__PocketHLE_MFC_Window:")
                || s.starts_with("__PocketHLE_MFC_Frame:")));
    }
    #[test]
    fn window_destructor_on_an_unbound_object_preserves_borrowed_memory_and_releases_no_caller_storage(
    ) {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        cpu.write_arg(0, 0x2000).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(313), |pc, _| panic!(
                "unexpected guest {pc:x}"
            )),
            0x2000
        );
        assert_eq!(cpu.read_u32_le(0x201c).unwrap(), 0);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1056",
                &[0x2000]
            )),
            0
        );
    }
}

pub(super) fn post_nc_delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    if object == 0 {
        return f.finish(ctx, 0);
    }
    let target = virtual_function(ctx, object, 1)?;
    f.call_args(ctx, 1, target, &[object, 1])
}
