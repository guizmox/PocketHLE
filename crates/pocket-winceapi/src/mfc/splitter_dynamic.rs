//! Row/column changes share one axis engine, native pane IDs and USER owners.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        926,
        "?Create@CSplitterWnd@@QAAHPAVCWnd@@HHUtagSIZE@@PAUCCreateContext@@KI@Z",
        create,
    ),
    (2452, "?SplitColumn@CSplitterWnd@@UAAHH@Z", split_column),
    (2453, "?SplitRow@CSplitterWnd@@UAAHH@Z", split_row),
    (1032, "?DeleteColumn@CSplitterWnd@@UAAXH@Z", delete_column),
    (1042, "?DeleteRow@CSplitterWnd@@UAAXH@Z", delete_row),
    (
        373,
        "?_AfxCanSplitRowCol@@YAHPAUCRowColInfo@CSplitterWnd@@HH@Z",
        can_split,
    ),
];
#[derive(Clone, Copy)]
struct Axis {
    count: u32,
    max: u32,
    array: u32,
    cross_count: u32,
    cross_array: u32,
    border: u32,
    thickness: u32,
    scroll: u32,
    scroll_id: u32,
    row: bool,
}
impl Axis {
    fn new(row: bool) -> Self {
        if row {
            Self {
                count: 0x74,
                max: 0x4c,
                array: 0x88,
                cross_count: 0x78,
                cross_array: 0x84,
                border: 0x70,
                thickness: 0x58,
                scroll: 0x80,
                scroll_id: 0xea10,
                row,
            }
        } else {
            Self {
                count: 0x78,
                max: 0x50,
                array: 0x84,
                cross_count: 0x74,
                cross_array: 0x88,
                border: 0x6c,
                thickness: 0x54,
                scroll: 0x7c,
                scroll_id: 0xea00,
                row,
            }
        }
    }
    fn pane(self, axis: u32, cross: u32) -> (u32, u32) {
        if self.row {
            (axis, cross)
        } else {
            (cross, axis)
        }
    }
}
fn can_split_at(ctx: &mut CallCtx<'_>, at: u32, first: i32, gap: i32) -> Result<i32, KernelError> {
    let minimum = word(ctx, at)? as i32;
    let current = word(ctx, at + 8)? as i32;
    let next_min = word(ctx, at + 12)? as i32;
    let second = current.wrapping_sub(first).wrapping_sub(gap);
    Ok(if first < minimum || second < next_min {
        -1
    } else {
        second
    })
}
fn can_split(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let at = ctx.arg_u32(0)?;
    let first = ctx.arg_u32(1)? as i32;
    let gap = ctx.arg_u32(2)? as i32;
    Ok(DispatchOutcome::ReturnedR0(
        can_split_at(ctx, at, first, gap)? as u32,
    ))
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let rows = f.arg(ctx, 2)?;
        let cols = f.arg(ctx, 3)?;
        if !(1..=2).contains(&rows) || !(1..=2).contains(&cols) || word(ctx, o + 0x20)? != 0 {
            return f.finish(ctx, 0);
        }
        let cx = ctx.arg_u32(4)?;
        let cy = ctx.arg_u32(5)?;
        let context = ctx.arg_u32(6)?;
        let style = ctx.arg_u32(7)?;
        let id = ctx.arg_u32(8)?;
        ctx.cpu
            .check_guest_access(context, 20, pocket_cpu::Prot::READ)?;
        f.set(ctx, 0, cx)?;
        f.set(ctx, 1, cy)?;
        f.set(ctx, 2, context)?;
        for (off, v) in [(0x4c, rows), (0x50, cols), (0x74, 1), (0x78, 1)] {
            put(ctx, o + off, v)?;
        }
        let parent = f.arg(ctx, 1)?;
        return f.call_service(
            ctx,
            1,
            super::splitter_windows::COMMON,
            &[o, parent, cx, cy, style, id],
        );
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            let context = f.get(ctx, 2)?;
            let class = word(ctx, context)?;
            put(ctx, o + 0x48, class)?;
            let cx = f.get(ctx, 0)?;
            let cy = f.get(ctx, 1)?;
            f.call_virtual(ctx, 2, o, 46, &[0, 0, class, cx, cy, context])
        }
        2 => {
            if result == 0 {
                return f.call_virtual(ctx, 3, o, 23, &[]);
            }
            let cp = word(ctx, o + 0x84)?;
            let rp = word(ctx, o + 0x88)?;
            let cx = f.get(ctx, 0)?;
            let cy = f.get(ctx, 1)?;
            put(ctx, cp + 4, cx)?;
            put(ctx, rp + 4, cy)?;
            f.finish(ctx, 1)
        }
        3 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid dynamic splitter creation continuation".into(),
        )),
    }
}
fn split_row(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    split(ctx, Axis::new(true))
}
fn split_column(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    split(ctx, Axis::new(false))
}
fn split(ctx: &mut CallCtx<'_>, axis: Axis) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let n = word(ctx, o + axis.count)?;
        let max = word(ctx, o + axis.max)?;
        if n == 0 || n >= max || max > 16 {
            return f.finish(ctx, 0);
        }
        let p = word(ctx, o + axis.array)?;
        let first = (f.arg(ctx, 1)? as i32).wrapping_sub(word(ctx, o + axis.border)? as i32);
        let gap = word(ctx, o + axis.thickness)? as i32;
        let second = can_split_at(ctx, p + (n - 1) * 12, first, gap)?;
        if second < 0 {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, n)?;
        f.set(ctx, 1, first as u32)?;
        f.set(ctx, 2, second as u32)?;
        if word(ctx, o + axis.scroll)? != 0 {
            return f.call_virtual(ctx, 1, o, 50, &[u32::from(axis.row), axis.scroll_id + n]);
        }
        return begin_split(ctx, f, o, axis);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            begin_split(ctx, f, o, axis)
        }
        2 => {
            if result == 0 {
                return rollback_split(ctx, f, o, axis);
            }
            let i = f.get(ctx, 3)?;
            f.set(ctx, 3, i + 1)?;
            next_split(ctx, f, o, axis)
        }
        3 => f.finish(ctx, 1),
        4 => rollback_split(ctx, f, o, axis),
        5 => {
            if result != 0 {
                return f.call_service(ctx, 6, "DestroyWindow", &[result]);
            }
            abort_split(ctx, f, o, axis)
        }
        6 => abort_split(ctx, f, o, axis),
        _ => Err(KernelError::Dispatch(
            "invalid splitter split continuation".into(),
        )),
    }
}
fn begin_split(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    let n = f.get(ctx, 0)?;
    put(ctx, o + axis.count, n + 1)?;
    next_split(ctx, f, o, axis)
}
fn next_split(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    let i = f.get(ctx, 3)?;
    let n = f.get(ctx, 0)?;
    if i >= word(ctx, o + axis.cross_count)? {
        let p = word(ctx, o + axis.array)?;
        let first = f.get(ctx, 1)?;
        let second = f.get(ctx, 2)?;
        put(ctx, p + (n - 1) * 12 + 4, first)?;
        put(ctx, p + n * 12 + 4, second)?;
        return f.call_virtual(ctx, 3, o, 47, &[]);
    }
    let cp = word(ctx, o + axis.cross_array)?;
    let cross = word(ctx, cp + i * 12 + 8)?;
    let second = f.get(ctx, 2)?;
    let (row, col) = axis.pane(n, i);
    let class = word(ctx, o + 0x48)?;
    let (cx, cy) = if axis.row {
        (cross, second)
    } else {
        (second, cross)
    };
    f.call_virtual(ctx, 2, o, 46, &[row, col, class, cx, cy, 0])
}
fn rollback_split(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    let i = f.get(ctx, 3)?;
    let n = f.get(ctx, 0)?;
    if i != 0 {
        f.set(ctx, 3, i - 1)?;
        let (row, col) = axis.pane(n, i - 1);
        return f.call_virtual(ctx, 4, o, 51, &[row, col]);
    }
    if word(ctx, o + axis.scroll)? != 0 {
        let hwnd = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 5, "GetDlgItem", &[hwnd, axis.scroll_id + n]);
    }
    abort_split(ctx, f, o, axis)
}
fn abort_split(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    let n = f.get(ctx, 0)?;
    put(ctx, o + axis.count, n)?;
    f.finish(ctx, 0)
}
fn delete_row(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    delete(ctx, Axis::new(true))
}
fn delete_column(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    delete(ctx, Axis::new(false))
}
fn delete(ctx: &mut CallCtx<'_>, axis: Axis) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let removed = f.arg(ctx, 1)?;
    if f.fresh {
        let n = word(ctx, o + axis.count)?;
        if n <= 1 || n > 16 || removed >= n {
            return Err(KernelError::Dispatch(
                "invalid splitter deletion index".into(),
            ));
        }
        f.set(ctx, 0, n)?;
        return f.call_virtual(ctx, 1, o, 56, &[f.address + 128, f.address + 132]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result != 0 {
                let row = word(ctx, f.address + 128)?;
                let col = word(ctx, f.address + 132)?;
                if (if axis.row { row } else { col }) == removed {
                    let n = f.get(ctx, 0)?;
                    let next = (removed + 1) % n;
                    let (row, col) = if axis.row { (next, col) } else { (row, next) };
                    return f.call_virtual(ctx, 2, o, 57, &[row, col, 0]);
                }
            }
            begin_delete(ctx, f, o, axis)
        }
        2 => begin_delete(ctx, f, o, axis),
        3 => {
            f.set(ctx, 2, removed + 1)?;
            shift_next(ctx, f, o, axis)
        }
        4 => {
            if result == 0 {
                return Err(KernelError::Dispatch(
                    "missing splitter pane during compaction".into(),
                ));
            }
            let j = f.get(ctx, 2)?;
            let cross = f.get(ctx, 1)?;
            let (row, col) = axis.pane(j - 1, cross);
            let hwnd = word(ctx, result + 0x20)?;
            f.call_service(
                ctx,
                5,
                "SetWindowLongW",
                &[
                    hwnd,
                    (-12i32) as u32,
                    super::splitter_panes::pane_id(row, col),
                ],
            )
        }
        5 => {
            let j = f.get(ctx, 2)?;
            f.set(ctx, 2, j + 1)?;
            shift_next(ctx, f, o, axis)
        }
        6 => {
            if result != 0 {
                f.set(ctx, 4, result)?;
            }
            next_delete(ctx, f, o, axis)
        }
        7 => {
            if result == 0 {
                return Err(KernelError::Dispatch(
                    "missing shared splitter scrollbar during compaction".into(),
                ));
            }
            let j = f.get(ctx, 2)?;
            f.call_service(
                ctx,
                8,
                "SetWindowLongW",
                &[result, (-12i32) as u32, axis.scroll_id + j - 1],
            )
        }
        8 => {
            let j = f.get(ctx, 2)?;
            f.set(ctx, 2, j + 1)?;
            shift_scroll(ctx, f, o, axis)
        }
        9 => finish_delete(ctx, f, o, axis),
        10 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter deletion continuation".into(),
        )),
    }
}
fn begin_delete(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    if word(ctx, o + axis.scroll)? != 0 {
        let hwnd = word(ctx, o + 0x20)?;
        let removed = f.arg(ctx, 1)?;
        return f.call_service(ctx, 6, "GetDlgItem", &[hwnd, axis.scroll_id + removed]);
    }
    next_delete(ctx, f, o, axis)
}
fn next_delete(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    let cross = f.get(ctx, 1)?;
    if cross >= word(ctx, o + axis.cross_count)? {
        let removed = f.arg(ctx, 1)?;
        f.set(ctx, 2, removed + 1)?;
        return shift_scroll(ctx, f, o, axis);
    }
    let removed = f.arg(ctx, 1)?;
    let (row, col) = axis.pane(removed, cross);
    f.call_virtual(ctx, 3, o, 51, &[row, col])
}
fn shift_next(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    let j = f.get(ctx, 2)?;
    if j >= f.get(ctx, 0)? {
        let i = f.get(ctx, 1)?;
        f.set(ctx, 1, i + 1)?;
        return next_delete(ctx, f, o, axis);
    }
    let i = f.get(ctx, 1)?;
    let (row, col) = axis.pane(j, i);
    f.call_service(ctx, 4, super::splitter_panes::GET, &[o, row, col])
}
fn shift_scroll(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    let j = f.get(ctx, 2)?;
    if word(ctx, o + axis.scroll)? != 0 && j < f.get(ctx, 0)? {
        let hwnd = word(ctx, o + 0x20)?;
        return f.call_service(ctx, 7, "GetDlgItem", &[hwnd, axis.scroll_id + j]);
    }
    let doomed = f.get(ctx, 4)?;
    if doomed != 0 {
        return f.call_service(ctx, 9, "DestroyWindow", &[doomed]);
    }
    finish_delete(ctx, f, o, axis)
}
fn finish_delete(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    axis: Axis,
) -> Result<DispatchOutcome, KernelError> {
    let n = f.get(ctx, 0)?;
    put(ctx, o + axis.count, n - 1)?;
    f.call_virtual(ctx, 10, o, 47, &[])
}
