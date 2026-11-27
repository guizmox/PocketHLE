//! Permanent CWnd associations borrow shared HWNDs; subclassing preserves callbacks.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const CLEAR_TEMP: &str = "__PocketHLE_MFC_CWnd_ClearTemporary";
pub(super) const SUBCLASS: &str = "__PocketHLE_MFC_SubclassWindow";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1207, "?FromHandle@CWnd@@SAPAV1@PAUHWND__@@@Z", from_handle),
    (1048, "?DeleteTempMap@CWnd@@SAXXZ", clear_temporary),
    (804, "?Attach@CWnd@@QAAHPAUHWND__@@@Z", attach),
    (1065, "?Detach@CWnd@@QAAPAUHWND__@@XZ", detach),
    (2460, "?SubclassWindow@CWnd@@QAAHPAUHWND__@@@Z", subclass),
    (2491, "?UnsubclassWindow@CWnd@@QAAPAUHWND__@@XZ", unsubclass),
];
fn from_handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(super::windows::wrapper(
        ctx, hwnd,
    )?))
}
fn clear_temporary(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::handle_maps::clear_temporary(ctx, super::handle_maps::Kind::Wnd)
}
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", CLEAR_TEMP, clear_temporary);
    d.register_handler("coredll.dll", SUBCLASS, subclass);
}
pub(super) fn thread_key(ctx: &CallCtx<'_>, hwnd: u32) -> String {
    format!(
        "__PocketHLE_MFC_ThreadWindow:{}:{hwnd:08x}",
        ctx.kernel.current_thread
    )
}
fn association_key(hwnd: u32) -> String {
    format!("__PocketHLE_MFC_Window:{hwnd:08x}")
}
pub(super) fn associate(
    ctx: &mut CallCtx<'_>,
    object: u32,
    hwnd: u32,
) -> Result<bool, KernelError> {
    ctx.cpu
        .check_guest_access(object, 72, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let owner = super::application::owner(ctx)?;
    let key = association_key(hwnd);
    if hwnd == 0
        || word(ctx, object + 0x20)? != 0
        || ctx.kernel.dynamic_exports[&owner].contains_key(&key)
    {
        return Ok(false);
    }
    if !crate::coredll::window_exists(ctx.kernel, hwnd) {
        return Ok(false);
    }
    let parent = ctx
        .kernel
        .controls
        .get(hwnd)
        .map(|c| c.parent)
        .or_else(|| ctx.kernel.window_parents.get(&hwnd).copied())
        .unwrap_or(0);
    if word(ctx, object + 0x40)? == 0 {
        if let Some(&parent_object) =
            ctx.kernel.dynamic_exports[&owner].get(&association_key(parent))
        {
            if word(ctx, parent_object + 0x3c)? != 0 {
                return Err(KernelError::Dispatch(
                    "MFC OLE control-site attachment is not implemented".into(),
                ));
            }
        }
    }
    put(ctx, object + 0x20, hwnd)?;
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key, object);
    Ok(true)
}
fn attach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let hwnd = ctx.arg_u32(1)?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(associate(
        ctx, object, hwnd,
    )?)))
}
pub(super) fn detach_value(ctx: &mut CallCtx<'_>, object: u32) -> Result<u32, KernelError> {
    ctx.cpu
        .check_guest_access(object, 72, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let owner = super::application::owner(ctx)?;
    let hwnd = word(ctx, object + 0x20)?;
    let scoped = thread_key(ctx, hwnd);
    let key = if ctx.kernel.dynamic_exports[&owner].get(&scoped) == Some(&object) {
        scoped
    } else {
        association_key(hwnd)
    };
    if let Some(&other) = ctx.kernel.dynamic_exports[&owner].get(&key) {
        if other != object {
            return Err(KernelError::Dispatch(
                "MFC CWnd association belongs to another object".into(),
            ));
        }
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .remove(&key);
    }
    put(ctx, object + 0x20, 0)?;
    put(ctx, object + 0x40, 0)?;
    Ok(hwnd)
}
fn detach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(detach_value(ctx, object)?))
}
fn subclass(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let hwnd = f.arg(ctx, 1)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        if !associate(ctx, object, hwnd)? {
            return f.finish(ctx, 0);
        }
        let target = virtual_function(ctx, object, 21)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    match f.phase(ctx)? {
        1 => {
            let target = virtual_function(ctx, object, 32)?;
            f.call_args(ctx, 2, target, &[object])
        }
        2 => {
            ctx.cpu.check_guest_access(
                result,
                4,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )?;
            f.set(ctx, 0, result)?;
            let procedure = super::windows::procedure(ctx)?;
            let target = service(ctx, "SetWindowLongW")?;
            f.call_args(ctx, 3, target, &[hwnd, (-4i32) as u32, procedure])
        }
        3 => {
            let slot = f.get(ctx, 0)?;
            if word(ctx, slot)? == 0 {
                put(ctx, slot, result)?;
            }
            f.finish(ctx, 1)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC subclass continuation".into(),
        )),
    }
}
pub(super) fn unsubclass(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let target = virtual_function(ctx, object, 32)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    match f.phase(ctx)? {
        1 => {
            let slot = ctx.cpu.read_reg(ArmReg::R0)?;
            ctx.cpu.check_guest_access(
                slot,
                4,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )?;
            f.set(ctx, 0, slot)?;
            let old = word(ctx, slot)?;
            let hwnd = word(ctx, object + 0x20)?;
            let target = service(ctx, "SetWindowLongW")?;
            f.call_args(ctx, 2, target, &[hwnd, (-4i32) as u32, old])
        }
        2 => {
            let slot = f.get(ctx, 0)?;
            put(ctx, slot, 0)?;
            let hwnd = detach_value(ctx, object)?;
            f.finish(ctx, hwnd)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC unsubclass continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn attachment_does_not_reinitialize_an_existing_borrowed_control_site() {
        use pocket_kernel::Dispatcher;
        let (mut cpu, mut k, mut d) = framework_setup();
        for object in [0x2000, 0x2200] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[object],
            );
        }
        for hwnd in [0xdead1000, 0xdead1001] {
            k.window_classes.insert(hwnd, "Example".into());
        }
        k.window_parents.insert(0xdead1001, 0xdead1000);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:804",
            &[0x2000, 0xdead1000],
        );
        cpu.write_mem(0x203c, &0x2600u32.to_le_bytes()).unwrap();
        cpu.write_arg(0, 0x2200).unwrap();
        cpu.write_arg(1, 0xdead1001).unwrap();
        assert!(d
            .dispatch(&mut cpu, &entry(804), &mut k)
            .unwrap_err()
            .to_string()
            .contains("control-site attachment"));
        assert_eq!(cpu.read_u32_le(0x2220).unwrap(), 0);
        cpu.write_mem(0x2240, &0x2700u32.to_le_bytes()).unwrap();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:804",
                &[0x2200, 0xdead1001]
            )),
            1
        );
        assert_eq!(cpu.read_u32_le(0x2240).unwrap(), 0x2700);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1065",
            &[0x2200],
        );
        assert_eq!(cpu.read_u32_le(0x2240).unwrap(), 0);
    }
    #[test]
    fn subclass_round_trip_preserves_original_proc_and_borrows_the_live_hwnd() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(original, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &bytes).unwrap();
        cpu.write_mem(0x2454, &0x6000u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        k.window_classes.insert(0xdead1001, "Example".into());
        k.window_procs.insert(0xdead1001, 0x6100);
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 0xdead1001).unwrap();
        let free = k.heap.free_bytes();
        let mut callbacks = 0;
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(2460), |pc, cpu| {
                assert_eq!(pc, 0x6000);
                assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                assert_eq!(cpu.read_u32_le(0x2020).unwrap(), 0xdead1001);
                callbacks += 1;
                123
            }),
            1
        );
        assert_eq!(callbacks, 1);
        assert_eq!(cpu.read_u32_le(0x2030).unwrap(), 0x6100);
        assert_eq!(
            k.window_procs[&0xdead1001],
            k.dynamic_exports[&1]["__PocketHLE_MFC_WindowProc"]
        );
        assert_eq!(
            k.dynamic_exports[&1]["__PocketHLE_MFC_Window:dead1001"],
            0x2000
        );
        cpu.write_arg(0, 0x2000).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(2491), |pc, _| panic!(
                "unexpected callback {pc:x}"
            )),
            0xdead1001
        );
        assert_eq!(cpu.read_u32_le(0x2020).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2030).unwrap(), 0);
        assert_eq!(k.window_procs[&0xdead1001], 0x6100);
        assert!(k.window_classes.contains_key(&0xdead1001));
        assert!(!k.dynamic_exports[&1].contains_key("__PocketHLE_MFC_Window:dead1001"));
        assert_eq!(k.heap.free_bytes(), free);
    }
    #[test]
    fn duplicate_attachment_preserves_the_original_owner_and_detach_does_not_destroy_window() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for object in [0x2000, 0x2200] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[object],
            );
        }
        k.window_classes.insert(0xdead1001, "Example".into());
        for (object, hwnd, expected) in [
            (0x2000, 0xdead1001, 1),
            (0x2200, 0xdead1001, 0),
            (0x2200, 0x12345678, 0),
        ] {
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:804",
                    &[object, hwnd]
                )),
                expected
            );
        }
        assert_eq!(cpu.read_u32_le(0x2220).unwrap(), 0);
        assert_eq!(
            k.dynamic_exports[&1]["__PocketHLE_MFC_Window:dead1001"],
            0x2000
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1065",
                &[0x2000]
            )),
            0xdead1001
        );
        assert!(k.window_classes.contains_key(&0xdead1001));
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:804",
                &[0x2200, 0xdead1001]
            )),
            1
        );
    }
}
