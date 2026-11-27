//! Recursive MFC notifications use WinCE's live child/sibling order and dispatch.
use super::callbacks::Frame;
use super::*;
const SEND: &str = "__PocketHLE_MFC_SendDescendants";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[(
    2277,
    "?SendMessageToDescendants@CWnd@@SAXPAUHWND__@@IIJHH@Z",
    send,
)];
pub(super) const TARGET: &str = SEND;
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", SEND, send);
}
fn depth_key(ctx: &CallCtx<'_>) -> String {
    format!(
        "__PocketHLE_MFC_DescendantsDepth:{}",
        ctx.kernel.current_thread
    )
}
fn finish(ctx: &mut CallCtx<'_>, frame: Frame) -> Result<DispatchOutcome, KernelError> {
    let previous = frame.get(ctx, 4)?;
    let owner = super::application::owner(ctx)?;
    let key = depth_key(ctx);
    if previous == 0 {
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .remove(&key);
    } else {
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .insert(key, previous);
    }
    frame.finish(ctx, 0)
}
fn after_child(ctx: &mut CallCtx<'_>, frame: Frame) -> Result<DispatchOutcome, KernelError> {
    let child = frame.get(ctx, 2)?;
    if frame.get(ctx, 0)? != 0 {
        let args = [
            child,
            frame.arg(ctx, 1)?,
            frame.arg(ctx, 2)?,
            frame.arg(ctx, 3)?,
            1,
            frame.get(ctx, 1)?,
        ];
        frame.call_service(ctx, 3, SEND, &args)
    } else {
        frame.call_service(ctx, 1, "GetWindow", &[child, 2])
    }
}
fn send(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if frame.fresh {
        let deep = ctx.arg_u32(4)?;
        let permanent_only = ctx.arg_u32(5)?;
        let owner = super::application::owner(ctx)?;
        let key = depth_key(ctx);
        let previous = ctx.kernel.dynamic_exports[&owner]
            .get(&key)
            .copied()
            .unwrap_or(0);
        if previous >= 128 {
            return Err(KernelError::Dispatch(
                "MFC descendant notification exceeds its depth bound".into(),
            ));
        }
        frame.set(ctx, 0, deep)?;
        frame.set(ctx, 1, permanent_only)?;
        frame.set(ctx, 4, previous)?;
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .insert(key, previous + 1);
        let hwnd = frame.arg(ctx, 0)?;
        return frame.call_service(ctx, 1, "GetWindow", &[hwnd, 5]);
    }
    match frame.phase(ctx)? {
        1 => {
            let child = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
            if child == 0 {
                return finish(ctx, frame);
            }
            let count = frame.get(ctx, 3)?;
            if count >= 4096 {
                return Err(KernelError::Dispatch(
                    "MFC descendant sibling traversal exceeds its bound".into(),
                ));
            }
            frame.set(ctx, 3, count + 1)?;
            frame.set(ctx, 2, child)?;
            let owner = super::application::owner(ctx)?;
            if frame.get(ctx, 1)? == 0
                || ctx.kernel.dynamic_exports[&owner]
                    .contains_key(&format!("__PocketHLE_MFC_Window:{child:08x}"))
            {
                let args = [
                    child,
                    frame.arg(ctx, 1)?,
                    frame.arg(ctx, 2)?,
                    frame.arg(ctx, 3)?,
                ];
                frame.call_service(ctx, 2, "SendMessageW", &args)
            } else {
                after_child(ctx, frame)
            }
        }
        2 => after_child(ctx, frame),
        3 => {
            let child = frame.get(ctx, 2)?;
            frame.call_service(ctx, 1, "GetWindow", &[child, 2])
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC descendant continuation".into(),
        )),
    }
}

// Search has no guest callbacks. Reuse WinCE's ordering, including mixed
// native-control and custom HWND children, rather than a second tree registry.
pub(super) fn find_permanent(ctx: &CallCtx<'_>, parent: u32, id: u32) -> Result<u32, KernelError> {
    fn find(
        ctx: &CallCtx<'_>,
        parent: u32,
        id: u32,
        depth: u32,
        budget: &mut usize,
    ) -> Result<u32, KernelError> {
        if depth >= 128 || *budget == 0 {
            return Err(KernelError::Dispatch(
                "MFC descendant search exceeds its bound".into(),
            ));
        }
        let owner = super::application::owner(ctx)?;
        let mut children = Vec::new();
        let mut child = crate::coredll::related_window(ctx.kernel, parent, 5);
        while child != 0 {
            if *budget == 0 {
                return Err(KernelError::Dispatch(
                    "MFC descendant search exceeds its bound".into(),
                ));
            }
            *budget -= 1;
            children.push(child);
            child = crate::coredll::related_window(ctx.kernel, child, 2);
        }
        // A matching immediate child is searched first, recursively before its
        // own permanent wrapper, matching CWnd::GetDescendantWindow(TRUE).
        let matching = children.iter().copied().find(|hwnd| {
            ctx.kernel
                .controls
                .get(*hwnd)
                .map(|c| c.id)
                .or_else(|| ctx.kernel.window_ids.get(hwnd).copied())
                == Some(id)
        });
        if let Some(hwnd) = matching {
            if crate::coredll::related_window(ctx.kernel, hwnd, 5) != 0 {
                let nested = find(ctx, hwnd, id, depth + 1, budget)?;
                if nested != 0 {
                    return Ok(nested);
                }
            }
            if let Some(&object) = ctx.kernel.dynamic_exports[&owner]
                .get(&format!("__PocketHLE_MFC_Window:{hwnd:08x}"))
            {
                return Ok(object);
            }
        }
        for hwnd in children {
            if Some(hwnd) != matching && crate::coredll::related_window(ctx.kernel, hwnd, 5) != 0 {
                let nested = find(ctx, hwnd, id, depth + 1, budget)?;
                if nested != 0 {
                    return Ok(nested);
                }
            }
        }
        Ok(0)
    }
    find(ctx, parent, id, 0, &mut 4096)
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn permanent_only_broadcast_still_visits_nested_children_and_releases_each_frame() {
        for (deep, only, expected) in [
            (1u32, 1u32, vec![0xdead1002, 0xdead1003]),
            (0, 1, vec![0xdead1003]),
            (1, 0, vec![0xdead1001, 0xdead1002, 0xdead1003]),
        ] {
            let (mut cpu, mut k, mut d) = framework_setup();
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:644", &[]);
            for (hwnd, parent) in [
                (0xdead1001, 0xdead1000),
                (0xdead1002, 0xdead1001),
                (0xdead1003, 0xdead1000),
            ] {
                k.window_classes.insert(hwnd, "NotifyTest".into());
                k.window_parents.insert(hwnd, parent);
                k.window_styles.insert(hwnd, (0x40000000, 0));
                k.window_procs.insert(hwnd, 0x6000);
            }
            for hwnd in [0xdead1002u32, 0xdead1003] {
                k.dynamic_exports
                    .get_mut(&1)
                    .unwrap()
                    .insert(format!("__PocketHLE_MFC_Window:{hwnd:08x}"), 0x2000);
            }
            for (i, value) in [0xdead1000, 0x364, 0xabc, 0xdef, deep, only]
                .into_iter()
                .enumerate()
            {
                if i < 4 {
                    cpu.write_arg(i as u8, value).unwrap();
                } else {
                    cpu.write_mem(0x4000 + (i as u32 - 4) * 4, &value.to_le_bytes())
                        .unwrap();
                }
            }
            let baseline = k.heap.free_bytes();
            let mut notifications = Vec::new();
            drive(&mut cpu, &mut k, &mut d, entry(2277), |pc, cpu| {
                assert_eq!(pc, 0x6000);
                notifications.push(cpu.read_arg(0).unwrap());
                assert_eq!(cpu.read_arg(1).unwrap(), 0x364);
                assert_eq!(cpu.read_arg(2).unwrap(), 0xabc);
                assert_eq!(cpu.read_arg(3).unwrap(), 0xdef);
                0
            });
            assert_eq!(notifications, expected);
            assert_eq!(k.heap.free_bytes(), baseline);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")
                    || key.starts_with("__PocketHLE_MFC_DescendantsDepth:")));
        }
    }
}
