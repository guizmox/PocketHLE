//! Idle processing dispatches real UI updates and document-template virtuals.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
const THREAD: &str = "__PocketHLE_MFC_ThreadIdle";
const UNLOCK: &str = "__PocketHLE_MFC_UnlockTempMaps";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1935, "?OnIdle@CWinApp@@UAAHJ@Z", app_idle),
    (1938, "?OnIdleUpdateCmdUI@CFrameWnd@@IAAXXZ", frame_idle),
    (667, "?AfxLockTempMaps@@YAXXZ", lock_maps),
    (771, "?AfxUnlockTempMaps@@YAHH@Z", unlock_maps),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", THREAD, thread_idle);
    d.register_handler("coredll.dll", UNLOCK, unlock_maps);
}
pub(super) fn thread_handler(name: &str) -> Option<crate::Handler> {
    (name == "?OnIdle@CWinThread@@UAAHJ@Z").then_some(thread_idle as crate::Handler)
}
fn lock_maps(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let at = super::application::module_thread(ctx)? + 0x10;
    let next = word(ctx, at)?
        .checked_add(1)
        .ok_or_else(|| KernelError::Dispatch("MFC temporary-map lock overflow".into()))?;
    put(ctx, at, next)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn cleanup_maps(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let state = super::application::module_thread(ctx)?;
    // Permanent wrappers belong to the shared handle owners. Temporary native
    // handle maps are not yet constructed by HLE: never silently discard one.
    for offset in [0x20, 0x1c, 0x18, 0x14, 0x24] {
        if word(ctx, state + offset)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC temporary handle-map cleanup is not implemented".into(),
            ));
        }
    }
    // Preserve native GDI-before-DC temporary-map destruction order.
    f.call_service(ctx, 2, super::gdi::CLEAR_TEMP, &[])
}
fn unlock_maps(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        if f.phase(ctx)? == 2 {
            return f.call_service(ctx, 3, super::device_contexts::CLEAR_TEMP, &[]);
        }
        if f.phase(ctx)? == 3 {
            return f.call_service(ctx, 5, super::menus::CLEAR_TEMP, &[]);
        }
        if f.phase(ctx)? == 5 {
            return f.call_service(ctx, 4, super::window_handles::CLEAR_TEMP, &[]);
        }
        if f.phase(ctx)? == 4 {
            let state = super::application::module_thread(ctx)?;
            let count = word(ctx, state + 0x10)?;
            return f.finish(ctx, u32::from(count != 0));
        }
        return cleanup_maps(ctx, f);
    }
    let state = super::application::module_thread(ctx)?;
    let count = word(ctx, state + 0x10)?;
    if count == 0 {
        return f.finish(ctx, 0);
    }
    put(ctx, state + 0x10, count - 1)?;
    let delete = f.arg(ctx, 0)?;
    if count != 1 || delete == 0 {
        return f.finish(ctx, u32::from(count != 1));
    }
    if delete != u32::MAX {
        let thread = super::application::thread_object(ctx)?;
        if thread != 0 {
            let terminate = word(ctx, thread + 0x58)?;
            if terminate != 0 {
                return f.call_args(ctx, 1, terminate, &[0, 0]);
            }
        }
    }
    cleanup_maps(ctx, f)
}
fn next_frame(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    loop {
        let current = f.get(ctx, 1)?;
        let next = if current == 0 {
            let state = super::application::module_thread(ctx)?;
            word(ctx, state + 8)?
        } else {
            word(ctx, current + 0x6c)?
        };
        if next == 0 {
            let count = f.arg(ctx, 1)? as i32;
            return f.finish(ctx, u32::from(count < 0));
        }
        let visited = f.get(ctx, 2)?;
        if visited >= 4096 {
            return Err(KernelError::Dispatch(
                "MFC idle frame chain exceeds its bound".into(),
            ));
        }
        f.set(ctx, 2, visited + 1)?;
        f.set(ctx, 1, next)?;
        if next == f.get(ctx, 0)? || word(ctx, next + 0x20)? == 0 {
            continue;
        }
        let hwnd = word(ctx, next + 0x20)?;
        if word(ctx, next + 0x9c)? == 0 {
            return f.call_service(ctx, 3, "ShowWindow", &[hwnd, 0]);
        }
        return f.call_service(ctx, 4, "IsWindowVisible", &[hwnd]);
    }
}
fn update(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    object: u32,
    phase: u32,
) -> Result<DispatchOutcome, KernelError> {
    f.set(ctx, 3, object)?;
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(
        ctx,
        phase,
        "__PocketHLE_MFC_WindowProc",
        &[hwnd, 0x363, 1, 0],
    )
}
fn show_delayed(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.get(ctx, 1)?;
    let show = word(ctx, object + 0x9c)? as i32;
    if show > 0 {
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 7, "ShowWindow", &[hwnd, show as u32]);
    }
    put(ctx, object + 0x9c, u32::MAX)?;
    next_frame(ctx, f)
}
fn thread_idle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        if f.arg(ctx, 1)? as i32 > 0 {
            let state = super::application::module_thread(ctx)?;
            if word(ctx, state + 0x10)? == 0 {
                lock_maps(ctx)?;
                return f.call_service(ctx, 8, UNLOCK, &[1]);
            }
            return f.finish(ctx, 0);
        }
        let object = f.arg(ctx, 0)?;
        let main = word(ctx, object + 0x20)?;
        f.set(ctx, 0, main)?;
        if main == 0 || word(ctx, main + 0x20)? == 0 {
            return next_frame(ctx, f);
        }
        let hwnd = word(ctx, main + 0x20)?;
        return f.call_service(ctx, 1, "IsWindowVisible", &[hwnd]);
    }
    match f.phase(ctx)? {
        1 if result != 0 => {
            let main = f.get(ctx, 0)?;
            update(ctx, &f, main, 2)
        }
        1 => next_frame(ctx, f),
        2 | 5 => {
            let object = f.get(ctx, 3)?;
            let hwnd = word(ctx, object + 0x20)?;
            let phase = if f.phase(ctx)? == 2 { 9 } else { 6 };
            f.call_service(
                ctx,
                phase,
                super::descendants::TARGET,
                &[hwnd, 0x363, 1, 0, 1, 1],
            )
        }
        3 => {
            let object = f.get(ctx, 1)?;
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 4, "IsWindowVisible", &[hwnd])
        }
        4 => {
            let object = f.get(ctx, 1)?;
            if result != 0 || word(ctx, object + 0x9c)? as i32 >= 0 {
                update(ctx, &f, object, 5)
            } else {
                show_delayed(ctx, f)
            }
        }
        6 => show_delayed(ctx, f),
        7 => {
            let object = f.get(ctx, 1)?;
            put(ctx, object + 0x9c, u32::MAX)?;
            next_frame(ctx, f)
        }
        8 => {
            let count = f.arg(ctx, 1)? as i32;
            f.finish(ctx, u32::from(count < 0))
        }
        9 => next_frame(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC thread idle continuation".into(),
        )),
    }
}
fn next_template(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if word(ctx, f.address + 96)? == 0 {
        let count = f.arg(ctx, 1)? as i32;
        return f.finish(ctx, u32::from(count < 1));
    }
    let count = f.get(ctx, 0)?;
    if count >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC idle document-template traversal exceeds its bound".into(),
        ));
    }
    f.set(ctx, 0, count + 1)?;
    let object = f.arg(ctx, 0)?;
    let manager = word(ctx, object + 0x80)?;
    let target = virtual_function(ctx, manager, 7)?;
    f.call_args(ctx, 3, target, &[manager, f.address + 96])
}
fn app_idle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    let object = f.arg(ctx, 0)?;
    let count = f.arg(ctx, 1)? as i32;
    if f.fresh {
        if count <= 1 {
            return f.call_service(ctx, 1, THREAD, &[object, count as u32]);
        }
        return f.finish(ctx, 0);
    }
    match f.phase(ctx)? {
        1 if count <= 0 => {
            let manager = word(ctx, object + 0x80)?;
            if manager == 0 {
                return f.finish(ctx, 1);
            }
            let target = virtual_function(ctx, manager, 6)?;
            f.call_args(ctx, 2, target, &[manager])
        }
        1 => f.finish(ctx, 0),
        2 => {
            put(ctx, f.address + 96, result)?;
            next_template(ctx, f)
        }
        3 => {
            let target = virtual_function(ctx, result, 35)?;
            f.call_args(ctx, 4, target, &[result])
        }
        4 => next_template(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC application idle continuation".into(),
        )),
    }
}
fn frame_idle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let phase = if f.fresh { 0 } else { f.phase(ctx)? };
    let flags = word(ctx, object + 0xe4)?;
    if phase == 0 && flags & 1 != 0 {
        let target = virtual_function(ctx, object, 56)?;
        let notify = word(ctx, object + 0xbc)?;
        return f.call_args(ctx, 1, target, &[object, notify]);
    }
    if phase <= 1 && flags & 2 != 0 {
        let target = virtual_function(ctx, object, 55)?;
        return f.call_args(ctx, 2, target, &[object, 1]);
    }
    if phase <= 2 && flags & 8 != 0 {
        let target = virtual_function(ctx, object, 50)?;
        return f.call_args(ctx, 3, target, &[object, flags & 4]);
    }
    if phase == 3 {
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 4, "UpdateWindow", &[hwnd]);
    }
    put(ctx, object + 0xe4, 0)?;
    f.finish(ctx, 0)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn application_idle_calls_each_document_template_and_releases_continuations() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:201",
            &[0x2000, 0],
        );
        for (at, value) in [
            (0x2080, 0x2400u32),
            (0x2400, 0x2600),
            (0x2618, 0x6000),
            (0x261c, 0x6100),
            (0x2800, 0x2900),
            (0x298c, 0x6200),
        ] {
            cpu.write_mem(at, &value.to_le_bytes()).unwrap();
        }
        // Initialize the lazily owned per-thread structures before counting.
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:645", &[]);
        let free = k.heap.free_bytes();
        for (count, expected_result, expected_calls) in [(0u32, 1, 5), (1, 0, 0), (2, 0, 0)] {
            cpu.write_arg(0, 0x2000).unwrap();
            cpu.write_arg(1, count).unwrap();
            let mut calls = 0;
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(1935), |pc, cpu| {
                    calls += 1;
                    match pc {
                        0x6000 => 2,
                        0x6100 => {
                            let position = cpu.read_arg(1).unwrap();
                            let old = cpu.read_u32_le(position).unwrap();
                            cpu.write_mem(position, &(old - 1).to_le_bytes()).unwrap();
                            0x2800
                        }
                        0x6200 => {
                            assert_eq!(cpu.read_arg(0).unwrap(), 0x2800);
                            123
                        }
                        _ => panic!("unexpected guest {pc:x}"),
                    }
                }),
                expected_result
            );
            assert_eq!(calls, expected_calls);
            assert_eq!(k.heap.free_bytes(), free);
        }
    }
    #[test]
    fn temp_map_locks_nest_and_frame_idle_preserves_virtual_order() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for _ in 0..2 {
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:667", &[]);
        }
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:771",
                &[1]
            )),
            1
        );
        cpu.write_arg(0, 1).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(771), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            0
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:103",
            &[0x2000],
        );
        let table = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(table, 58 * 4).unwrap();
        cpu.write_mem(0x2600, &bytes).unwrap();
        for (at, value) in [
            (0x2000, 0x2600u32),
            (0x26e0, 0x6000),
            (0x26dc, 0x6100),
            (0x20bc, 77),
            (0x20e4, 3),
        ] {
            cpu.write_mem(at, &value.to_le_bytes()).unwrap();
        }
        cpu.write_arg(0, 0x2000).unwrap();
        let mut calls = Vec::new();
        drive(&mut cpu, &mut k, &mut d, entry(1938), |pc, cpu| {
            calls.push(pc);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
            assert_eq!(cpu.read_arg(1).unwrap(), if pc == 0x6000 { 77 } else { 1 });
            0
        });
        assert_eq!(calls, [0x6000, 0x6100]);
        assert_eq!(cpu.read_u32_le(0x20e4).unwrap(), 0);
    }
}
