//! CE frame layout shares live WinCE geometry and synchronous size notifications.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
const REPOSITION: &str = "__PocketHLE_MFC_RepositionBars";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2022, "?OnSize@CFrameWnd@@IAAXIHH@Z", on_size),
    (2153, "?RecalcLayout@CFrameWnd@@UAAXH@Z", recalc),
    (
        2225,
        "?RepositionBars@CWnd@@QAAXIIIIPAUtagRECT@@PBU2@H@Z",
        reposition,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", REPOSITION, reposition);
}
fn recalc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    if !frame.fresh {
        put(ctx, object + 0xc4, 0)?;
        return frame.finish(ctx, 0);
    }
    if word(ctx, object + 0xc4)? != 0 {
        return frame.finish(ctx, 0);
    }
    if word(ctx, object + 0x40)? != 0 {
        return Err(KernelError::Dispatch(
            "MFC OLE frame layout is not implemented".into(),
        ));
    }
    let hwnd = word(ctx, object + 0x20)?;
    let style = ctx
        .kernel
        .window_styles
        .get(&hwnd)
        .map(|s| s.0)
        .unwrap_or(0);
    if style & 0x2000 != 0 {
        return Err(KernelError::Dispatch(
            "MFC snap-to-bars frame layout is not implemented".into(),
        ));
    }
    put(ctx, object + 0xc4, 1)?;
    let flags = word(ctx, object + 0xe4)?;
    put(ctx, object + 0xe4, flags & !12)?;
    frame.call_service(
        ctx,
        1,
        REPOSITION,
        &[object, 0, 0xffff, 0xe900, 2, object + 0x70, 0, 1],
    )
}
fn next(ctx: &mut CallCtx<'_>, frame: &Frame) -> Result<DispatchOutcome, KernelError> {
    let child = frame.get(ctx, 7)?;
    frame.call_service(ctx, 2, "GetWindow", &[child, 2])
}
fn complete(ctx: &mut CallCtx<'_>, frame: Frame) -> Result<DispatchOutcome, KernelError> {
    let layout = frame.address + 128;
    let rect = layout + 4;
    let flags = frame.get(ctx, 3)?;
    if flags == 1 {
        let out = frame.get(ctx, 4)?;
        if frame.get(ctx, 6)? != 0 {
            let bytes = ctx.cpu.read_mem(rect, 16)?;
            ctx.cpu.write_mem(out, &bytes)?;
        } else {
            let cx = word(ctx, layout + 20)?;
            let cy = word(ctx, layout + 24)?;
            for (i, value) in [0, 0, cx, cy].into_iter().enumerate() {
                put(ctx, out + i as u32 * 4, value)?;
            }
        }
        return frame.finish(ctx, 0);
    }
    let child = frame.get(ctx, 8)?;
    if frame.get(ctx, 2)? == 0 || child == 0 {
        return frame.finish(ctx, 0);
    }
    let object = super::windows::wrapper(ctx, child)?;
    if flags == 2 {
        let border = frame.get(ctx, 4)?;
        for (i, add) in [true, true, false, false].into_iter().enumerate() {
            let at = rect + i as u32 * 4;
            let value = word(ctx, at)?;
            let margin = word(ctx, border + i as u32 * 4)?;
            put(
                ctx,
                at,
                if add {
                    value.wrapping_add(margin)
                } else {
                    value.wrapping_sub(margin)
                },
            )?;
        }
    }
    let target = virtual_function(ctx, object, 25)?;
    frame.call_args(ctx, 4, target, &[object, rect, 0])
}
fn reposition(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let layout = frame.address + 128;
    let rect = layout + 4;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        for i in 0..7 {
            let value = ctx.arg_u32(i + 1)?;
            frame.set(ctx, i as u32, value)?;
        }
        let flags = frame.get(ctx, 3)?;
        if flags > 2 {
            return Err(KernelError::Dispatch(
                "Invalid MFC RepositionBars flags".into(),
            ));
        }
        ctx.cpu.write_mem(layout, &[0; 32])?;
        put(ctx, layout, u32::from(flags != 1))?;
        let stretch = frame.get(ctx, 6)?;
        put(ctx, layout + 28, stretch)?;
        let input = frame.get(ctx, 5)?;
        if input == 0 {
            let hwnd = word(ctx, object + 0x20)?;
            return frame.call_service(ctx, 1, "GetClientRect", &[hwnd, rect]);
        }
        let bytes = ctx.cpu.read_mem(input, 16)?;
        ctx.cpu.write_mem(rect, &bytes)?;
        let hwnd = word(ctx, object + 0x20)?;
        return frame.call_service(ctx, 2, "GetWindow", &[hwnd, 5]);
    }
    match frame.phase(ctx)? {
        1 => {
            let hwnd = word(ctx, object + 0x20)?;
            frame.call_service(ctx, 2, "GetWindow", &[hwnd, 5])
        }
        2 => {
            if result == 0 {
                return complete(ctx, frame);
            }
            let count = frame.get(ctx, 9)?;
            if count >= 4096 {
                return Err(KernelError::Dispatch(
                    "MFC bar traversal exceeds its bound".into(),
                ));
            }
            frame.set(ctx, 9, count + 1)?;
            frame.set(ctx, 7, result)?;
            let id = ctx
                .kernel
                .controls
                .get(result)
                .map(|c| c.id)
                .or_else(|| ctx.kernel.window_ids.get(&result).copied())
                .unwrap_or(0)
                & 0xffff;
            if id == frame.get(ctx, 2)? {
                frame.set(ctx, 8, result)?;
                return next(ctx, &frame);
            }
            let owner = super::application::owner(ctx)?;
            let permanent = ctx.kernel.dynamic_exports[&owner]
                .contains_key(&format!("__PocketHLE_MFC_Window:{result:08x}"));
            if id == 0xe40a && permanent {
                return Err(KernelError::Dispatch(
                    "MFC rebar layout is not implemented".into(),
                ));
            }
            if id >= frame.get(ctx, 0)? && id <= frame.get(ctx, 1)? && permanent {
                frame.call_service(ctx, 3, "SendMessageW", &[result, 0x361, 0, layout])
            } else {
                next(ctx, &frame)
            }
        }
        3 => next(ctx, &frame),
        4 => {
            // A query-only layout has hDWP == NULL. The CE helper intentionally
            // refrains from moving the leftover pane in that case.
            if word(ctx, layout)? == 0 {
                return frame.finish(ctx, 0);
            }
            let x = word(ctx, rect)?;
            let y = word(ctx, rect + 4)?;
            let width = word(ctx, rect + 8)?.wrapping_sub(x);
            let height = word(ctx, rect + 12)?.wrapping_sub(y);
            let child = frame.get(ctx, 8)?;
            // Both WinCE geometry paths store child rectangles in parent-client
            // coordinates. Preserve the native helper's unchanged-rect guard;
            // SetWindowPos would otherwise enqueue a redundant WM_SIZE.
            let current = ctx
                .kernel
                .window_rects
                .get(&child)
                .copied()
                .or_else(|| ctx.kernel.controls.get(child).map(|c| (c.x, c.y, c.w, c.h)));
            if current == Some((x as i32, y as i32, width as i32, height as i32)) {
                return frame.finish(ctx, 0);
            }
            frame.call_service(
                ctx,
                5,
                "SetWindowPos",
                &[child, 0, x, y, width, height, 0x14],
            )
        }
        5 => frame.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC bar-layout continuation".into(),
        )),
    }
}

fn on_size(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    if frame.fresh {
        let target = service(ctx, "__PocketHLE_MFC_WindowDefault")?;
        return frame.call_args(ctx, 1, target, &[object]);
    }
    match frame.phase(ctx)? {
        1 if frame.arg(ctx, 1)? != 1 => {
            let target = virtual_function(ctx, object, 50)?;
            frame.call_args(ctx, 2, target, &[object, 1])
        }
        1 | 2 => frame.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame OnSize continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn size_parent_query_and_real_layout_preserve_shared_rectangles_and_callback_ownership() {
        for (flags, stretch) in [(0u32, 1u32), (1, 1), (1, 0), (2, 1)] {
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
            let table = cpu.read_u32_le(0x2200).unwrap();
            let bytes = cpu.read_mem(table, 46 * 4).unwrap();
            cpu.write_mem(0x2800, &bytes).unwrap();
            cpu.write_mem(0x2800 + 25 * 4, &0x6100u32.to_le_bytes())
                .unwrap();
            cpu.write_mem(0x2200, &0x2800u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2020, &0xdead1000u32.to_le_bytes()).unwrap();
            k.window_classes.insert(0xdead1000, "LayoutTest".into());
            k.window_rects.insert(0xdead1000, (0, 0, 240, 320));
            for (child, id, object) in [(0xdead1001, 0xe800, 0x2000), (0xdead1002, 0xe900, 0x2200)]
            {
                k.window_classes.insert(child, "LayoutTest".into());
                k.window_styles.insert(child, (0x50000000, 0));
                k.window_parents.insert(child, 0xdead1000);
                k.window_ids.insert(child, id);
                k.window_rects.insert(child, (0, 0, 240, 320));
                k.dynamic_exports
                    .get_mut(&1)
                    .unwrap()
                    .insert(format!("__PocketHLE_MFC_Window:{child:08x}"), object);
            }
            k.window_procs.insert(0xdead1001, 0x6000);
            for (i, value) in [2u32, 3, 4, 5].into_iter().enumerate() {
                cpu.write_mem(0x1800 + i as u32 * 4, &value.to_le_bytes())
                    .unwrap();
            }
            for (i, value) in [0x2000, 0, 0xffff, 0xe900, flags, 0x1800, 0, stretch]
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
            let mut adjusts = 0;
            drive(&mut cpu, &mut k, &mut d, entry(2225), |pc, cpu| {
                match pc {
                    0x6000 => {
                        assert_eq!(cpu.read_arg(1).unwrap(), 0x361);
                        let layout = cpu.read_arg(3).unwrap();
                        assert_eq!(cpu.read_u32_le(layout).unwrap(), u32::from(flags != 1));
                        cpu.write_mem(layout + 8, &20u32.to_le_bytes()).unwrap();
                        cpu.write_mem(layout + 24, &20u32.to_le_bytes()).unwrap();
                    }
                    0x6100 => {
                        adjusts += 1;
                        assert_ne!(flags, 1);
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                        assert_eq!(cpu.read_arg(2).unwrap(), 0);
                    }
                    _ => panic!("Unexpected layout callback {pc:x}"),
                }
                0
            });
            assert_eq!(adjusts, usize::from(flags != 1));
            assert_eq!(
                k.window_rects[&0xdead1002],
                match flags {
                    1 => (0, 0, 240, 320),
                    2 => (2, 23, 234, 292),
                    _ => (0, 20, 240, 300),
                }
            );
            if flags == 1 {
                let expected = if stretch != 0 {
                    [0u32, 20, 240, 320]
                } else {
                    [0, 0, 0, 20]
                };
                for (i, value) in expected.into_iter().enumerate() {
                    assert_eq!(cpu.read_u32_le(0x1800 + i as u32 * 4).unwrap(), value);
                }
            }
            assert_eq!(k.heap.free_bytes(), baseline);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        }
    }
}
