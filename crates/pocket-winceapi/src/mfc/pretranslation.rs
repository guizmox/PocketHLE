//! CE message pretranslation preserves real virtual overrides and shared HWNDs.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const MAIN: &str = "__PocketHLE_MFC_MainWindow";
const WALK: &str = "__PocketHLE_MFC_WalkPreTranslateTree";
pub(super) const ROOT: &str = "__PocketHLE_MFC_TopLevelHwnd";
pub(super) const PARENT: &str = "__PocketHLE_MFC_ParentOwner";
const THREAD: &str = "__PocketHLE_MFC_DispatchThreadMessage";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2110,
        "?PreTranslateMessage@CWinThread@@UAAHPAUtagMSG@@@Z",
        thread_translate,
    ),
    (
        2111,
        "?PreTranslateMessage@CWnd@@UAAHPAUtagMSG@@@Z",
        window_translate,
    ),
    (
        2503,
        "?WalkPreTranslateTree@CWnd@@SAHPAUHWND__@@PAUtagMSG@@@Z",
        walk,
    ),
    (2633, "?AfxGetMainWnd@@YAPAVCWnd@@XZ", main_window),
    (1345, "?GetMainWnd@CWinThread@@UAAPAVCWnd@@XZ", get_main),
    (
        1582,
        "?GetTopLevelParent@CWnd@@QBAPAV1@XZ",
        top_level_parent,
    ),
    (
        647,
        "?AfxGetParentOwner@@YAPAUHWND__@@PAU1@@Z",
        parent_owner,
    ),
    (
        1069,
        "?DispatchThreadMessageEx@CWinThread@@IAAHPAUtagMSG@@@Z",
        dispatch_thread,
    ),
    (
        1068,
        "?DispatchThreadMessage@CWinThread@@IAAXPAUtagMSG@@@Z",
        dispatch_thread_void,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        (MAIN, main_window as crate::Handler),
        (WALK, walk),
        (ROOT, top_level_hwnd),
        (PARENT, parent_owner),
        (THREAD, dispatch_thread),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn window_translate(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // The verified retail CE base method is exactly FALSE. Derived virtuals are
    // dispatched separately; this is not a replacement for their processing.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn main_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let value = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, value);
    }
    let thread = super::application::thread_object(ctx)?;
    if thread == 0 {
        return f.finish(ctx, 0);
    }
    let target = virtual_function(ctx, thread, 29)?;
    f.call_args(ctx, 1, target, &[thread])
}
fn get_main(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
        let object = super::windows::wrapper(ctx, hwnd)?;
        return f.finish(ctx, object);
    }
    let thread = f.arg(ctx, 0)?;
    let active = word(ctx, thread + 0x24)?;
    if active != 0 {
        return f.finish(ctx, active);
    }
    let main = word(ctx, thread + 0x20)?;
    if main != 0 {
        return f.finish(ctx, main);
    }
    f.call_service(ctx, 1, "GetActiveWindow", &[])
}
fn after_window(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let hwnd = f.get(ctx, 0)?;
    if hwnd == f.arg(ctx, 0)? {
        return f.finish(ctx, 0);
    }
    f.call_service(ctx, 1, "GetParent", &[hwnd])
}
fn walk_step(ctx: &mut CallCtx<'_>, f: Frame, hwnd: u32) -> Result<DispatchOutcome, KernelError> {
    if hwnd == 0 {
        return f.finish(ctx, 0);
    }
    let count = f.get(ctx, 1)?;
    if count >= 128 {
        return Err(KernelError::Dispatch(
            "MFC pretranslation parent chain exceeds its bound".into(),
        ));
    }
    f.set(ctx, 1, count + 1)?;
    f.set(ctx, 0, hwnd)?;
    if let Some(object) = super::windows::permanent(ctx, hwnd)? {
        let message = f.arg(ctx, 1)?;
        let target = virtual_function(ctx, object, 36)?;
        f.call_args(ctx, 2, target, &[object, message])
    } else {
        after_window(ctx, f)
    }
}
fn walk(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let message = f.arg(ctx, 1)?;
        let hwnd = word(ctx, message)?;
        return walk_step(ctx, f, hwnd);
    }
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => walk_step(ctx, f, value),
        2 if value != 0 => f.finish(ctx, value),
        2 => after_window(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC pretranslation walk continuation".into(),
        )),
    }
}
fn thread_translate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let message = f.arg(ctx, 1)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        if word(ctx, message)? == 0 {
            let thread = f.arg(ctx, 0)?;
            return f.call_service(ctx, 1, THREAD, &[thread, message]);
        }
        return f.call_service(ctx, 2, MAIN, &[]);
    }
    match f.phase(ctx)? {
        1 if value != 0 => f.finish(ctx, 1),
        1 => f.call_service(ctx, 2, MAIN, &[]),
        2 => {
            f.set(ctx, 0, value)?;
            let hwnd = if value == 0 {
                0
            } else {
                word(ctx, value + 0x20)?
            };
            f.call_service(ctx, 3, WALK, &[hwnd, message])
        }
        3 if value != 0 => f.finish(ctx, 1),
        3 if f.get(ctx, 0)? == 0 => f.finish(ctx, 0),
        3 => {
            let hwnd = word(ctx, message)?;
            f.call_service(ctx, 4, ROOT, &[hwnd])
        }
        4 => {
            let main = f.get(ctx, 0)?;
            if word(ctx, main + 0x20)? == value {
                return f.finish(ctx, 0);
            }
            let target = virtual_function(ctx, main, 36)?;
            f.call_args(ctx, 5, target, &[main, message])
        }
        5 => f.finish(ctx, value),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC thread pretranslation continuation".into(),
        )),
    }
}
fn parent_owner(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let hwnd = f.arg(ctx, 0)?;
        if hwnd == 0 || hwnd == crate::coredll::FAKE_DESKTOP_HWND {
            return f.finish(ctx, 0);
        }
        if let Some(object) = super::windows::permanent(ctx, hwnd)? {
            let owner = word(ctx, object + 0x28)?;
            if owner != 0 {
                return f.finish(ctx, owner);
            }
            return f.call_service(ctx, 2, "GetParent", &[hwnd]);
        }
        return f.call_service(ctx, 1, "GetWindowLongW", &[hwnd, (-16i32) as u32]);
    }
    match f.phase(ctx)? {
        1 => {
            let hwnd = f.arg(ctx, 0)?;
            if value & 0x40000000 != 0 {
                f.call_service(ctx, 2, "GetParent", &[hwnd])
            } else {
                f.call_service(ctx, 2, "GetWindow", &[hwnd, 4])
            }
        }
        // The shared shell-less provider exposes a desktop boundary for a
        // top-level HWND. It is not an MFC logical owner or another app window.
        2 => f.finish(
            ctx,
            if value == crate::coredll::FAKE_DESKTOP_HWND {
                0
            } else {
                value
            },
        ),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC parent-owner continuation".into(),
        )),
    }
}
fn top_level_hwnd(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let value = if f.fresh {
        f.arg(ctx, 0)?
    } else {
        ctx.cpu.read_reg(ArmReg::R0)?
    };
    if value == 0 {
        let root = f.get(ctx, 0)?;
        return f.finish(ctx, root);
    }
    let count = f.get(ctx, 1)?;
    if count >= 128 {
        return Err(KernelError::Dispatch(
            "MFC top-level parent chain exceeds its bound".into(),
        ));
    }
    f.set(ctx, 0, value)?;
    f.set(ctx, 1, count + 1)?;
    f.call_service(ctx, 1, PARENT, &[value])
}
fn top_level_parent(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
        let object = super::windows::wrapper(ctx, hwnd)?;
        return f.finish(ctx, object);
    }
    let object = f.arg(ctx, 0)?;
    let hwnd = if object == 0 {
        0
    } else {
        word(ctx, object + 0x20)?
    };
    f.call_service(ctx, 1, ROOT, &[hwnd])
}
fn dispatch_thread(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let target = virtual_function(ctx, object, 12)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    if f.phase(ctx)? == 3 {
        return f.finish(ctx, 1);
    }
    let map = ctx.cpu.read_reg(ArmReg::R0)?;
    if map == 0 {
        return f.finish(ctx, 0);
    }
    let count = f.get(ctx, 1)?;
    if count >= 32 {
        return Err(KernelError::Dispatch(
            "MFC thread message-map inheritance exceeds its bound".into(),
        ));
    }
    for i in 0..count {
        if word(ctx, f.address + 96 + i * 4)? == map {
            return Err(KernelError::Dispatch(
                "Cyclic MFC thread message-map inheritance".into(),
            ));
        }
    }
    put(ctx, f.address + 96 + count * 4, map)?;
    f.set(ctx, 1, count + 1)?;
    let message = f.arg(ctx, 1)?;
    let kind = word(ctx, message + 4)?;
    let entries = word(ctx, map + 4)?;
    if let Some(fields) = super::message_maps::find_window_entry(ctx, entries, kind)? {
        let wp = word(ctx, message + 8)?;
        let lp = word(ctx, message + 12)?;
        f.call_args(ctx, 3, fields[5], &[object, wp, lp])
    } else {
        let base = word(ctx, map)?;
        f.call_args(ctx, 2, base, &[])
    }
}
fn dispatch_thread_void(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    let message = f.arg(ctx, 1)?;
    f.call_service(ctx, 1, THREAD, &[object, message])
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn tree_walk_skips_unbound_children_honors_stop_and_preserves_virtual_result() {
        for (stop, handled, expected, result) in [
            (0xdead1000u32, false, vec![0x6200, 0x6100], 0u32),
            (0xdead1000, true, vec![0x6200], 9),
            (0xdead1001, false, vec![0x6200], 0),
        ] {
            let (mut cpu, mut k, mut d) = framework_setup();
            for (object, table, hwnd, procedure) in [
                (0x2000u32, 0x2600u32, 0xdead1000u32, 0x6100u32),
                (0x2200, 0x2800, 0xdead1001, 0x6200),
            ] {
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:206",
                    &[object],
                );
                let original = cpu.read_u32_le(object).unwrap();
                let bytes = cpu.read_mem(original, 46 * 4).unwrap();
                cpu.write_mem(table, &bytes).unwrap();
                cpu.write_mem(table + 36 * 4, &procedure.to_le_bytes())
                    .unwrap();
                cpu.write_mem(object, &table.to_le_bytes()).unwrap();
                cpu.write_mem(object + 0x20, &hwnd.to_le_bytes()).unwrap();
                k.dynamic_exports
                    .get_mut(&1)
                    .unwrap()
                    .insert(format!("__PocketHLE_MFC_Window:{hwnd:08x}"), object);
            }
            k.window_parents.insert(0xdead1002, 0xdead1001);
            k.window_parents.insert(0xdead1001, 0xdead1000);
            cpu.write_mem(0x1800, &0xdead1002u32.to_le_bytes()).unwrap();
            cpu.write_arg(0, stop).unwrap();
            cpu.write_arg(1, 0x1800).unwrap();
            let available = k.heap.free_bytes();
            let mut calls = Vec::new();
            let value = drive(&mut cpu, &mut k, &mut d, entry(2503), |pc, cpu| {
                calls.push(pc);
                assert_eq!(cpu.read_arg(1).unwrap(), 0x1800);
                assert_eq!(
                    cpu.read_arg(0).unwrap(),
                    if pc == 0x6200 { 0x2200 } else { 0x2000 }
                );
                if handled && pc == 0x6200 {
                    9
                } else {
                    0
                }
            });
            assert_eq!(calls, expected);
            assert_eq!(value, result);
            assert_eq!(k.heap.free_bytes(), available);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        }
    }
    #[test]
    fn thread_message_maps_handle_registered_ids_and_ignore_void_callback_registers() {
        for (kind, matched) in [(0x8001u32, true), (0xc123, true), (0x8002, false)] {
            let (mut cpu, mut k, mut d) = framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:201",
                &[0x2000, 0],
            );
            let original = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(original, 39 * 4).unwrap();
            cpu.write_mem(0x2600, &bytes).unwrap();
            cpu.write_mem(0x2600 + 12 * 4, &0x6000u32.to_le_bytes())
                .unwrap();
            cpu.write_mem(0x2000, &0x2600u32.to_le_bytes()).unwrap();
            for (at, value) in [
                (0x1800, 0u32),
                (0x1804, kind),
                (0x1808, 55),
                (0x180c, 66),
                (0x1900, kind),
                (0x3000, 0x6100),
                (0x3004, 0x3100),
            ] {
                cpu.write_mem(at, &value.to_le_bytes()).unwrap();
            }
            let fields: [u32; 6] = [
                if kind >= 0xc000 { 0xc000 } else { 0x8001 },
                0,
                0,
                0,
                if kind >= 0xc000 { 0x1900 } else { 14 },
                0x6200,
            ];
            for (i, value) in fields.into_iter().enumerate() {
                cpu.write_mem(0x3100 + i as u32 * 4, &value.to_le_bytes())
                    .unwrap();
            }
            cpu.write_arg(0, 0x2000).unwrap();
            cpu.write_arg(1, 0x1800).unwrap();
            let available = k.heap.free_bytes();
            let mut handlers = 0;
            let value = drive(&mut cpu, &mut k, &mut d, entry(1069), |pc, cpu| match pc {
                0x6000 => 0x3000,
                0x6100 => 0,
                0x6200 => {
                    handlers += 1;
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                    assert_eq!(cpu.read_arg(1).unwrap(), 55);
                    assert_eq!(cpu.read_arg(2).unwrap(), 66);
                    0xdeadbeef
                }
                _ => panic!("Unexpected thread handler {pc:x}"),
            });
            assert_eq!(value, u32::from(matched));
            assert_eq!(handlers, u32::from(matched));
            assert_eq!(k.heap.free_bytes(), available);
        }
    }
    #[test]
    fn main_window_resolution_calls_the_actual_thread_virtual() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:201",
            &[0x2000, 0],
        );
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:645", &[]);
        let original = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(original, 39 * 4).unwrap();
        cpu.write_mem(0x2600, &bytes).unwrap();
        cpu.write_mem(0x2600 + 29 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2000, &0x2600u32.to_le_bytes()).unwrap();
        let available = k.heap.free_bytes();
        let value = drive(&mut cpu, &mut k, &mut d, entry(2633), |pc, cpu| {
            assert_eq!(pc, 0x6000);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
            0x2200
        });
        assert_eq!(value, 0x2200);
        assert_eq!(k.heap.free_bytes(), available);
    }
}
