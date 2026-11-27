//! Shared splitter scrollbars are ordinary USER children, never extra host UI.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const LAYOUT: &str = "__PocketHLE_MFC_SplitterScrollLayout";
pub(super) const STYLE: &str = "__PocketHLE_MFC_SplitterScrollStyle";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (984, "?CreateScrollBarCtrl@CSplitterWnd@@UAAHKI@Z", create),
    (2408, "?SetScrollStyle@CSplitterWnd@@QAAXK@Z", set_style),
    (1556, "?GetScrollStyle@CSplitterWnd@@QBAKXZ", get_style),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", STYLE, set_style);
    d.register_handler("coredll.dll", LAYOUT, layout);
}
fn get_style(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(style(ctx, o)?))
}
fn style(ctx: &mut CallCtx<'_>, o: u32) -> Result<u32, KernelError> {
    Ok(u32::from(word(ctx, o + 0x7c)? != 0) * 0x100000
        | u32::from(word(ctx, o + 0x80)? != 0) * 0x200000)
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let p = alloc(ctx, 20)?;
        f.own(ctx, p)?;
        let bytes: Vec<_> = "SCROLLBAR"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(p, &bytes)?;
        let style = f.arg(ctx, 1)? | 0x50000000;
        let id = f.arg(ctx, 2)?;
        let parent = word(ctx, o + 0x20)?;
        return f.call_service(
            ctx,
            1,
            "CreateWindowExW",
            &[
                0,
                p,
                0,
                style,
                0,
                0,
                1,
                1,
                parent,
                id,
                ctx.kernel.image_base,
                0,
            ],
        );
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, u32::from(result != 0))
}
fn child(ctx: &mut CallCtx<'_>, f: &Frame, o: u32) -> Result<(u32, u32, bool), KernelError> {
    let cols = word(ctx, o + 0x78)?;
    let rows = word(ctx, o + 0x74)?;
    let i = f.get(ctx, 0)?;
    if cols > 16 || rows > 16 {
        return Err(KernelError::Dispatch(
            "invalid splitter scrollbar counts".into(),
        ));
    }
    Ok(if i < cols {
        (0xea00 + i, 0, word(ctx, o + 0x7c)? != 0)
    } else if i < cols + rows {
        (0xea10 + i - cols, 1, word(ctx, o + 0x80)? != 0)
    } else {
        (0xea20, 0x08000000, style(ctx, o)? == 0x300000)
    })
}
fn next(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 0)? > word(ctx, o + 0x78)? + word(ctx, o + 0x74)? {
        return f.finish(ctx, 0);
    }
    let (id, _, _) = child(ctx, &f, o)?;
    let hwnd = word(ctx, o + 0x20)?;
    f.call_service(ctx, 1, "GetDlgItem", &[hwnd, id])
}
fn set_style(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let requested = f.arg(ctx, 1)? & 0x300000;
        if requested == style(ctx, o)? {
            return f.finish(ctx, 0);
        }
        put(ctx, o + 0x7c, u32::from(requested & 0x100000 != 0))?;
        put(ctx, o + 0x80, u32::from(requested & 0x200000 != 0))?;
        return next(ctx, f, o);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    let (id, orientation, visible) = child(ctx, &f, o)?;
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                if id == 0xea20 && !visible {
                    return f.finish(ctx, 0);
                }
                return f.call_virtual(ctx, 2, o, 50, &[orientation, id]);
            }
            if id == 0xea20 && !visible {
                return f.call_service(ctx, 3, "DestroyWindow", &[result]);
            }
            f.call_service(ctx, 3, "ShowWindow", &[result, if visible { 5 } else { 0 }])
        }
        2 => {
            if result == 0 {
                return Err(super::io_exceptions::fault("MFC CMemory", 0, 0, Vec::new()));
            }
            next(ctx, f, o)
        }
        3 => {
            let i = f.get(ctx, 0)?;
            f.set(ctx, 0, i + 1)?;
            next(ctx, f, o)
        }
        _ => Err(KernelError::Dispatch(
            "invalid splitter scrollbar continuation".into(),
        )),
    }
}

fn layout(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let total = word(ctx, o + 0x78)? + word(ctx, o + 0x74)? + 1;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        match f.phase(ctx)? {
            1 => {
                if result == 0 {
                    return Err(KernelError::Dispatch("missing splitter scrollbar".into()));
                }
                f.set(ctx, 1, result)?;
                return f.call_service(ctx, 2, "GetWindowLongW", &[result, (-16i32) as u32]);
            }
            2 => {
                let hwnd = f.get(ctx, 1)?;
                let width = word(ctx, f.address + 136)? as i32;
                let height = word(ctx, f.address + 140)? as i32;
                let style = if width > 1 && height > 1 {
                    result | 0x800000
                } else {
                    result & !0x800000
                };
                if style != result {
                    return f.call_service(
                        ctx,
                        3,
                        "SetWindowLongW",
                        &[hwnd, (-16i32) as u32, style],
                    );
                }
                return position(ctx, &f);
            }
            3 => return position(ctx, &f),
            4 => {
                let i = f.get(ctx, 0)?;
                f.set(ctx, 0, i + 1)?;
            }
            _ => {
                return Err(KernelError::Dispatch(
                    "invalid splitter scrollbar layout continuation".into(),
                ))
            }
        }
    }
    loop {
        let i = f.get(ctx, 0)?;
        if i >= total {
            return f.finish(ctx, 0);
        }
        let (id, _, visible) = child(ctx, &f, o)?;
        if !visible {
            f.set(ctx, 0, i + 1)?;
            continue;
        }
        let full = f.arg(ctx, 1)?;
        let inside = f.arg(ctx, 2)?;
        let cols = word(ctx, o + 0x78)?;
        let r = word(ctx, inside + 8)? as i32;
        let b = word(ctx, inside + 12)? as i32;
        let bx = word(ctx, o + 0x6c)? as i32;
        let by = word(ctx, o + 0x70)? as i32;
        let thick_x = (word(ctx, full + 8)? as i32)
            .wrapping_sub(bx)
            .wrapping_sub(1)
            .wrapping_sub(r);
        let thick_y = (word(ctx, full + 12)? as i32)
            .wrapping_sub(by)
            .wrapping_sub(1)
            .wrapping_sub(b);
        let (x, y, w, h) = if i == total - 1 {
            (r + 1, b + 1, thick_x, thick_y)
        } else {
            let horizontal = i < cols;
            let n = if horizontal { i } else { i - cols };
            let array = word(ctx, o + if horizontal { 0x84 } else { 0x88 })?;
            let gap = word(ctx, o + if horizontal { 0x64 } else { 0x68 })? as i32;
            let mut start = word(ctx, full + if horizontal { 0 } else { 4 })? as i32
                + if horizontal { bx } else { by };
            for j in 0..n {
                start = start
                    .wrapping_add(word(ctx, array + j * 12 + 8)? as i32)
                    .wrapping_add(gap);
            }
            let mut size = word(ctx, array + n * 12 + 8)? as i32;
            if n == 0
                && (if horizontal {
                    cols
                } else {
                    word(ctx, o + 0x74)?
                }) < word(ctx, o + if horizontal { 0x50 } else { 0x4c })?
            {
                let box_size = word(ctx, o + if horizontal { 0x54 } else { 0x58 })? as i32 + 1;
                start += box_size;
                size -= box_size;
            }
            if horizontal {
                (start, b + 1, size, thick_y)
            } else {
                (r + 1, start, thick_x, size)
            }
        };
        for (offset, value) in [(0, x), (4, y), (8, w), (12, h)] {
            put(ctx, f.address + 128 + offset, value as u32)?;
        }
        let hwnd = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 1, "GetDlgItem", &[hwnd, id]);
    }
}
fn position(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let h = f.get(ctx, 1)?;
    let mut args = vec![h, 0];
    for off in [0, 4, 8, 12] {
        args.push(word(ctx, f.address + 128 + off)?);
    }
    args.push(0x14);
    f.call_service(ctx, 4, "SetWindowPos", &args)
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn static_shared_scrollbars_have_native_ids_styles_visibility_and_destroy_with_splitter() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let run = |cpu: &mut dyn pocket_cpu::Cpu,
                   k: &mut pocket_kernel::KernelState,
                   d: &mut WinCeDispatcher,
                   n,
                   args: &[u32]| {
            let sp = cpu.read_reg(ArmReg::Sp).unwrap();
            let home = cpu.stack_arg_offset();
            for (i, &v) in args.iter().enumerate() {
                if i < 4 {
                    cpu.write_arg(i as u8, v).unwrap();
                } else {
                    cpu.write_mem(sp + home + (i as u32 - 4) * 4, &v.to_le_bytes())
                        .unwrap();
                }
            }
            drive(cpu, k, d, entry(n), |pc, _| {
                panic!("unexpected guest {pc:x}")
            })
        };
        run(&mut cpu, &mut k, &mut d, 206, &[0x2000]);
        k.window_classes.insert(7, "Parent".into());
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        run(&mut cpu, &mut k, &mut d, 169, &[0x2400]);
        assert_eq!(
            run(
                &mut cpu,
                &mut k,
                &mut d,
                985,
                &[0x2400, 0x2000, 2, 3, 0x50300000, 0xe900]
            ),
            1
        );
        assert_eq!(run(&mut cpu, &mut k, &mut d, 1556, &[0x2400]), 0x300000);
        let parent = cpu.read_u32_le(0x2420).unwrap();
        let bars: Vec<_> = k.controls.children_of(parent);
        assert_eq!(bars.len(), 6);
        for id in [0xea00, 0xea01, 0xea02, 0xea10, 0xea11, 0xea20] {
            assert!(
                bars.iter()
                    .any(|h| k.controls.get(*h).is_some_and(|c| c.id == id)),
                "missing {id:x}"
            );
        }
        run(&mut cpu, &mut k, &mut d, 2408, &[0x2400, 0x100000]);
        assert_eq!(run(&mut cpu, &mut k, &mut d, 1556, &[0x2400]), 0x100000);
        assert!(!k
            .controls
            .children_of(parent)
            .iter()
            .any(|h| k.controls.get(*h).is_some_and(|c| c.id == 0xea20)));
        for h in k.controls.children_of(parent) {
            if k.controls.get(h).unwrap().id >= 0xea10 {
                assert!(!k.controls.get(h).unwrap().visible);
            }
        }
        run(&mut cpu, &mut k, &mut d, 292, &[0x2400]);
        assert!(k.controls.children_of(parent).is_empty());
        assert!(!k.window_classes.contains_key(&parent));
    }
}
