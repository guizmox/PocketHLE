//! Static CE splitter geometry and painting delegate to live WinCE windows/GDI.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1873,
        "?OnDisplayChange@CSplitterWnd@@IAAXXZ",
        display_change,
    ),
    (2154, "?RecalcLayout@CSplitterWnd@@UAAXXZ", recalc),
    (
        1321,
        "?GetInsideRect@CSplitterWnd@@MBAXAAVCRect@@@Z",
        inside,
    ),
    (1406, "?GetPane@CSplitterWnd@@QBAPAVCWnd@@HH@Z", pane),
    (
        1099,
        "?DrawAllSplitBars@CSplitterWnd@@MAAXPAVCDC@@HH@Z",
        bars,
    ),
    (
        1879,
        "?OnDrawSplitter@CSplitterWnd@@MAAXPAVCDC@@W4ESplitType@1@ABVCRect@@@Z",
        draw,
    ),
    (1979, "?OnPaint@CSplitterWnd@@IAAXXZ", paint),
];
fn display_change(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, "IsWindow", &[hwnd]);
    }
    let result = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
    match f.phase(ctx)? {
        1 if result != 0 => {
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 2, "IsWindowVisible", &[hwnd])
        }
        2 if result != 0 => f.call_virtual(ctx, 3, object, 47, &[]),
        1..=3 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC splitter display-change continuation".into(),
        )),
    }
}
fn rect(ctx: &mut CallCtx<'_>, p: u32) -> Result<[i32; 4], KernelError> {
    Ok([
        word(ctx, p)? as i32,
        word(ctx, p + 4)? as i32,
        word(ctx, p + 8)? as i32,
        word(ctx, p + 12)? as i32,
    ])
}
fn put_rect(ctx: &mut CallCtx<'_>, p: u32, r: [i32; 4]) -> Result<(), KernelError> {
    for (i, v) in r.into_iter().enumerate() {
        put(ctx, p + i as u32 * 4, v as u32)?;
    }
    Ok(())
}
fn static_state(ctx: &mut CallCtx<'_>, o: u32) -> Result<(), KernelError> {
    ctx.cpu.check_guest_access(o, 208, Prot::READ)?;
    Ok(())
}
fn arrays(ctx: &mut CallCtx<'_>, o: u32) -> Result<[(u32, usize); 2], KernelError> {
    static_state(ctx, o)?;
    let mut result = [(0, 0); 2];
    for (index, (p_slot, max_slot, count_slot)) in [(0x84, 0x50, 0x78), (0x88, 0x4c, 0x74)]
        .into_iter()
        .enumerate()
    {
        let p = word(ctx, o + p_slot)?;
        let max = word(ctx, o + max_slot)?;
        let count = word(ctx, o + count_slot)?;
        if !(1..=16).contains(&max)
            || count == 0
            || count > max
            || ctx
                .kernel
                .heap
                .msize(p)
                .is_none_or(|bytes| bytes < max * 12)
        {
            return Err(KernelError::Dispatch(
                "Invalid owned MFC splitter layout array".into(),
            ));
        }
        ctx.cpu
            .check_guest_access(p, max * 12, Prot::READ | Prot::WRITE)?;
        result[index] = (p, count as usize);
    }
    Ok(result)
}
// CE's current sizes are distinct from ideal/minimum sizes. The final pane
// receives the remainder; collapsed panes and a clipped splitter retain their
// native border accounting rather than a proportional-size approximation.
fn sizes(min: &[i32], ideal: &mut [i32], available: i32, gap: i32) -> Vec<i32> {
    let count = min.len();
    let mut current = vec![0; count];
    for i in 0..count - 1 {
        if ideal[i] < min[i] {
            ideal[i] = 0;
        }
        current[i] = ideal[i];
    }
    current[count - 1] = i32::MAX;
    let mut remaining = available.max(0);
    for i in 0..count {
        if remaining == 0 {
            current[i] = 0;
        } else if remaining < min[i] && i != 0 {
            current[i] = 0;
            current[i - 1] = current[i - 1].wrapping_add(remaining).wrapping_add(1);
            remaining = 0;
        } else if current[i] != 0 || i == 0 {
            if remaining < current[i] {
                current[i] = remaining;
                remaining = 0;
            } else {
                remaining = remaining.wrapping_sub(current[i]);
            }
        }
        if i != count - 1 {
            if remaining > gap {
                remaining = remaining.wrapping_sub(gap);
            } else {
                current[i] = current[i].wrapping_add(remaining);
                if current[i] > gap.wrapping_sub(1) {
                    current[i] = current[i].wrapping_sub(gap).wrapping_add(1);
                }
                remaining = 0;
            }
        }
    }
    current
}
fn layout_axis(
    ctx: &mut CallCtx<'_>,
    p: u32,
    count: usize,
    available: i32,
    gap: i32,
) -> Result<(), KernelError> {
    let mut min = Vec::with_capacity(count);
    let mut ideal = Vec::with_capacity(count);
    for i in 0..count {
        min.push(word(ctx, p + i as u32 * 12)? as i32);
        ideal.push(word(ctx, p + i as u32 * 12 + 4)? as i32);
    }
    let current = sizes(&min, &mut ideal, available, gap);
    for i in 0..count {
        put(ctx, p + i as u32 * 12 + 4, ideal[i] as u32)?;
        put(ctx, p + i as u32 * 12 + 8, current[i] as u32)?;
    }
    Ok(())
}
fn current(ctx: &mut CallCtx<'_>, p: u32, count: usize) -> Result<Vec<i32>, KernelError> {
    (0..count)
        .map(|i| Ok(word(ctx, p + i as u32 * 12 + 8)? as i32))
        .collect()
}
fn offset(values: &[i32], index: usize, gap: i32) -> i32 {
    values[..index]
        .iter()
        .fold(0i32, |n, &v| n.wrapping_add(v).wrapping_add(gap))
}
fn pane(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let o = f.arg(ctx, 0)?;
        let row = f.arg(ctx, 1)?;
        let col = f.arg(ctx, 2)?;
        let hwnd = word(ctx, o + 0x20)?;
        let id = 0xe900u32
            .wrapping_add(row.wrapping_shl(4))
            .wrapping_add(col);
        return f.call_service(ctx, 1, "GetDlgItem", &[hwnd, id]);
    }
    let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
    let object = super::windows::wrapper(ctx, hwnd)?;
    f.finish(ctx, object)
}
fn inside(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let target = f.arg(ctx, 1)?;
    if f.fresh {
        static_state(ctx, o)?;
        ctx.cpu.check_guest_access(target, 16, Prot::WRITE)?;
        let h = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 1, "GetClientRect", &[h, target]);
    }
    let phase = f.phase(ctx)?;
    if phase == 1 {
        if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
            return Err(KernelError::Dispatch(
                "MFC splitter has no client rectangle".into(),
            ));
        }
        if word(ctx, o + 0x80)? != 0 {
            return f.call_service(ctx, 2, "GetSystemMetrics", &[2]);
        }
    }
    if phase == 2 {
        let width = ctx.cpu.read_reg(ArmReg::R0)?;
        f.set(ctx, 0, width.saturating_sub(1))?;
    }
    if phase != 3 && word(ctx, o + 0x7c)? != 0 {
        return f.call_service(ctx, 3, "GetSystemMetrics", &[3]);
    }
    let vertical = if phase == 3 {
        ctx.cpu.read_reg(ArmReg::R0)?.saturating_sub(1) as i32
    } else {
        0
    };
    let horizontal = f.get(ctx, 0)? as i32;
    let [l, t, r, b] = rect(ctx, target)?;
    let x = word(ctx, o + 0x6c)? as i32;
    let y = word(ctx, o + 0x70)? as i32;
    put_rect(
        ctx,
        target,
        [
            l.wrapping_add(x),
            t.wrapping_add(y),
            r.wrapping_sub(x).wrapping_sub(horizontal),
            b.wrapping_sub(y).wrapping_sub(vertical),
        ],
    )?;
    f.finish(ctx, 0)
}

fn next_pane(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let [(_, cols), (_, rows)] = arrays(ctx, o)?;
    let i = f.get(ctx, 0)? as usize;
    if i >= cols * rows {
        return f.call_service(
            ctx,
            10,
            super::splitter_scroll::LAYOUT,
            &[o, f.address + 128, f.address + 144],
        );
    }
    let hwnd = word(ctx, o + 0x20)?;
    let id = 0xe900 + ((i % rows) as u32 * 16) + (i / rows) as u32;
    f.call_service(ctx, 3, "GetDlgItem", &[hwnd, id])
}
fn target_rect(ctx: &mut CallCtx<'_>, f: &Frame, o: u32, edge: bool) -> Result<(), KernelError> {
    let [(cp, cols), (rp, rows)] = arrays(ctx, o)?;
    let i = f.get(ctx, 0)? as usize;
    if i >= cols * rows {
        return Err(KernelError::Dispatch(
            "MFC splitter changed pane counts during layout".into(),
        ));
    }
    let col = i / rows;
    let row = i % rows;
    let cs = current(ctx, cp, cols)?;
    let rs = current(ctx, rp, rows)?;
    let [left, top, _, _] = rect(ctx, f.address + 128)?;
    let x = left
        .wrapping_add(word(ctx, o + 0x6c)? as i32)
        .wrapping_add(offset(&cs, col, word(ctx, o + 0x64)? as i32));
    let y = top
        .wrapping_add(word(ctx, o + 0x70)? as i32)
        .wrapping_add(offset(&rs, row, word(ctx, o + 0x68)? as i32));
    let border = i32::from(edge); // CE3 AUX_DATA caches one logical pixel per border axis.
    put_rect(
        ctx,
        f.address + 160,
        [
            x.wrapping_sub(border),
            y.wrapping_sub(border),
            x.wrapping_add(cs[col]).wrapping_add(border),
            y.wrapping_add(rs[row]).wrapping_add(border),
        ],
    )
}
fn recalc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        static_state(ctx, o)?;
        let hwnd = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 1, "GetClientRect", &[hwnd, f.address + 128]);
    }
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if returned == 0 {
                return Err(KernelError::Dispatch(
                    "MFC splitter has no client rectangle".into(),
                ));
            }
            let target = virtual_function(ctx, o, 64)?;
            f.call_args(ctx, 2, target, &[o, f.address + 144])
        }
        2 => {
            let [(cp, cols), (rp, rows)] = arrays(ctx, o)?;
            let [l, t, r, b] = rect(ctx, f.address + 144)?;
            let cx = word(ctx, o + 0x64)? as i32;
            let cy = word(ctx, o + 0x68)? as i32;
            layout_axis(ctx, cp, cols, r.wrapping_sub(l), cx)?;
            layout_axis(ctx, rp, rows, b.wrapping_sub(t), cy)?;
            next_pane(ctx, f, o)
        }
        3 => {
            if returned == 0 {
                return Err(KernelError::Dispatch(
                    "MFC splitter pane window is missing".into(),
                ));
            }
            let pane = super::windows::wrapper(ctx, returned)?;
            f.set(ctx, 1, returned)?;
            f.set(ctx, 2, pane)?;
            f.call_service(ctx, 4, "GetWindowLongW", &[returned, (-20i32) as u32])
        }
        4 if returned & 0x200 == 0 => {
            let pane = f.get(ctx, 2)?;
            let class = super::classes::runtime(ctx, "CSplitterWnd")?;
            f.call_service(ctx, 5, super::class_queries::KIND, &[pane, class])
        }
        4 | 5 => {
            target_rect(ctx, &f, o, returned != 0)?;
            let h = f.get(ctx, 1)?;
            f.call_service(ctx, 6, "GetWindowRect", &[h, f.address + 176])
        }
        6 => {
            let h = word(ctx, o + 0x20)?;
            f.call_service(ctx, 7, "ScreenToClient", &[h, f.address + 176])
        }
        7 => {
            let h = word(ctx, o + 0x20)?;
            f.call_service(ctx, 8, "ScreenToClient", &[h, f.address + 184])
        }
        8 => {
            let r = rect(ctx, f.address + 160)?;
            if r != rect(ctx, f.address + 176)? {
                let h = f.get(ctx, 1)?;
                return f.call_service(
                    ctx,
                    9,
                    "SetWindowPos",
                    &[
                        h,
                        0,
                        r[0] as u32,
                        r[1] as u32,
                        r[2].wrapping_sub(r[0]) as u32,
                        r[3].wrapping_sub(r[1]) as u32,
                        0x14,
                    ],
                );
            }
            let i = f.get(ctx, 0)?;
            f.set(ctx, 0, i + 1)?;
            next_pane(ctx, f, o)
        }
        9 => {
            let i = f.get(ctx, 0)?;
            f.set(ctx, 0, i + 1)?;
            next_pane(ctx, f, o)
        }
        10 => {
            let [_, _, r, b] = rect(ctx, f.address + 144)?;
            f.call_virtual(ctx, 11, o, 68, &[0, r as u32, b as u32])
        }
        11 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC splitter layout continuation".into(),
        )),
    }
}
fn bars(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        static_state(ctx, o)?;
        let h = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 1, "GetClientRect", &[h, f.address + 128]);
    }
    if f.phase(ctx)? == 2 {
        let i = f.get(ctx, 0)?;
        f.set(ctx, 0, i + 1)?;
    }
    let [(cp, cols), (rp, rows)] = arrays(ctx, o)?;
    let cs = current(ctx, cp, cols)?;
    let rs = current(ctx, rp, rows)?;
    let [l, t, r, b] = rect(ctx, f.address + 128)?;
    let cx = word(ctx, o + 0x64)? as i32;
    let cy = word(ctx, o + 0x68)? as i32;
    let bx = word(ctx, o + 0x6c)? as i32;
    let by = word(ctx, o + 0x70)? as i32;
    let sx = word(ctx, o + 0x5c)? as i32;
    let sy = word(ctx, o + 0x60)? as i32;
    let wx = word(ctx, o + 0x54)? as i32;
    let wy = word(ctx, o + 0x58)? as i32;
    let maxx = f.arg(ctx, 2)? as i32;
    let maxy = f.arg(ctx, 3)? as i32;
    let cdc = f.arg(ctx, 1)?;
    loop {
        let i = f.get(ctx, 0)? as usize;
        if i >= cols - 1 + rows - 1 + cols * rows {
            return f.finish(ctx, 0);
        }
        let (kind, area) = if i < cols - 1 {
            let x = l
                .wrapping_add(bx)
                .wrapping_add(offset(&cs, i, sx.wrapping_mul(2).wrapping_add(wx)))
                .wrapping_add(cs[i])
                .wrapping_add(sx);
            if x > maxx {
                f.set(ctx, 0, (cols - 1) as u32)?;
                continue;
            }
            (1, [x, t, x.wrapping_add(wx), b])
        } else if i < cols + rows - 2 {
            let j = i - (cols - 1);
            let y = t
                .wrapping_add(by)
                .wrapping_add(offset(&rs, j, sy.wrapping_mul(2).wrapping_add(wy)))
                .wrapping_add(rs[j])
                .wrapping_add(sy);
            if y > maxy {
                f.set(ctx, 0, (cols + rows - 2) as u32)?;
                continue;
            }
            (1, [l, y, r, y.wrapping_add(wy)])
        } else {
            let j = i - (cols + rows - 2);
            let col = j / rows;
            let row = j % rows;
            let x = l.wrapping_add(offset(&cs, col, cx));
            let y = t.wrapping_add(offset(&rs, row, cy));
            (
                3,
                [
                    x,
                    y,
                    x.wrapping_add(cs[col]).wrapping_add(bx.wrapping_mul(2)),
                    y.wrapping_add(rs[row]).wrapping_add(by.wrapping_mul(2)),
                ],
            )
        };
        put_rect(ctx, f.address + 160, area)?;
        let target = virtual_function(ctx, o, 48)?;
        return f.call_args(ctx, 2, target, &[o, cdc, kind, f.address + 160]);
    }
}
fn draw(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::splitter_track_paint::draw(ctx)
}

fn paint(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        static_state(ctx, o)?;
        return f.call_service(
            ctx,
            1,
            super::window_dcs::PAINT_CONSTRUCT,
            &[f.address + 128, o],
        );
    }
    match f.phase(ctx)? {
        1 => {
            let target = virtual_function(ctx, o, 64)?;
            f.call_args(ctx, 2, target, &[o, f.address + 96])
        }
        2 => {
            let [_, _, r, b] = rect(ctx, f.address + 96)?;
            let target = virtual_function(ctx, o, 68)?;
            f.call_args(ctx, 3, target, &[o, f.address + 128, r as u32, b as u32])
        }
        3 => f.call_service(
            ctx,
            4,
            super::window_dcs::PAINT_DESTRUCT,
            &[f.address + 128],
        ),
        4 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC splitter paint continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_axis_layout_retains_ideals_and_border_accounting_when_panes_collapse() {
        for (min, mut ideal, available, expected) in [
            (vec![1, 32], vec![110, 158], 268, vec![110, 151]),
            (vec![50, 50], vec![100, 100], 30, vec![24, 0]),
            (vec![1, 32], vec![110, 158], 130, vec![124, 0]),
            (vec![50, 0], vec![20, 90], 200, vec![0, 193]),
            (vec![0], vec![42], 100, vec![100]),
            (vec![10, 10, 10], vec![50, 60, 70], 300, vec![50, 60, 176]),
        ] {
            assert_eq!(sizes(&min, &mut ideal, available, 7), expected);
            if min == [50, 0] {
                assert_eq!(ideal, [0, 90]);
            }
        }
    }
}

#[cfg(test)]
mod window_tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn pane_layout_uses_live_windows_and_avoids_repositioning_unchanged_rectangles() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        k.window_classes.insert(0xdead1000, "Parent".into());
        k.window_rects.insert(0xdead1000, (0, 0, 240, 320));
        cpu.write_mem(0x2020, &0xdead1000u32.to_le_bytes()).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:169",
            &[0x2400],
        );
        for (i, v) in [0x2400, 0x2000, 2, 1, 0x50800000, 0xe900]
            .into_iter()
            .enumerate()
        {
            if i < 4 {
                cpu.write_arg(i as u8, v).unwrap();
            } else {
                cpu.write_mem(0x4000 + (i as u32 - 4) * 4, &v.to_le_bytes())
                    .unwrap();
            }
        }
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(985), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            1
        );
        let splitter = cpu.read_u32_le(0x2420).unwrap();
        k.window_rects.insert(splitter, (0, 0, 240, 300));
        for (o, h, id, ex) in [
            (0x2800, 0xdead1001, 0xe900, 0x200),
            (0x2900, 0xdead1002, 0xe910, 0),
        ] {
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:206", &[o]);
            k.window_classes.insert(h, "Pane".into());
            k.window_rects.insert(h, (0, 0, 1, 1));
            k.window_parents.insert(h, splitter);
            k.window_ids.insert(h, id);
            k.window_styles.insert(h, (0x50000000, ex));
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:804",
                    &[o, h]
                )),
                1
            );
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2402",
            &[0x2400, 0, 110, 1],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2402",
            &[0x2400, 1, 158, 32],
        );
        for _ in 0..2 {
            cpu.write_arg(0, 0x2400).unwrap();
            drive(&mut cpu, &mut k, &mut d, entry(2154), |pc, _| {
                panic!("unexpected {pc:x}")
            });
            assert_eq!(k.window_rects[&0xdead1001], (0, 0, 240, 112));
            assert_eq!(k.window_rects[&0xdead1002], (1, 118, 238, 181));
            // A repeated layout should skip SetWindowPos and its callbacks.
            k.window_procs.insert(0xdead1001, 0x6600);
            k.window_procs.insert(0xdead1002, 0x6600);
        }
        assert!(k.gdi.pending_window_updates.contains_key(&splitter));
    }
}
