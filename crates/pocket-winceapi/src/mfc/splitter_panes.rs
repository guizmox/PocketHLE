//! Splitter panes reuse runtime factories, live HWND ancestry and frame views.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const GET: &str = "__PocketHLE_MFC_SplitterGetPane";
pub(super) const CHILD: &str = "__PocketHLE_MFC_SplitterChildPane";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        988,
        "?CreateView@CSplitterWnd@@UAAHHHPAUCRuntimeClass@@UtagSIZE@@PAUCCreateContext@@@Z",
        create_view,
    ),
    (1051, "?DeleteView@CSplitterWnd@@UAAXHH@Z", delete_view),
    (
        1215,
        "?GetActivePane@CSplitterWnd@@UAAPAVCWnd@@PAH0@Z",
        active,
    ),
    (
        2300,
        "?SetActivePane@CSplitterWnd@@UAAXHHPAVCWnd@@@Z",
        set_active,
    ),
    (
        1685,
        "?IsChildPane@CSplitterWnd@@QAAHPAVCWnd@@PAH1@Z",
        is_child,
    ),
    (835, "?CanActivateNext@CSplitterWnd@@UAAHH@Z", can_next),
    (559, "?ActivateNext@CSplitterWnd@@UAAXH@Z", activate_next),
    (
        1244,
        "?GetColumnInfo@CSplitterWnd@@QBAXHAAH0@Z",
        column_info,
    ),
    (1437, "?GetRowInfo@CSplitterWnd@@QBAXHAAH0@Z", row_info),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", GET, get);
    d.register_handler("coredll.dll", CHILD, is_child);
}
fn get(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::handler_named("?GetPane@CSplitterWnd@@QBAPAVCWnd@@HH@Z").unwrap()(ctx)
}
pub(super) fn pane_id(row: u32, col: u32) -> u32 {
    0xe900 + row * 16 + col
}
fn info(ctx: &mut CallCtx<'_>, slot: u32, countslot: u32) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let i = ctx.arg_u32(1)?;
    let size = ctx.arg_u32(2)?;
    let min = ctx.arg_u32(3)?;
    let count = word(ctx, o + countslot)?;
    if i >= count || count > 16 {
        return Err(KernelError::Dispatch("invalid splitter info index".into()));
    }
    let at = word(ctx, o + slot)? + i * 12;
    let minimum = word(ctx, at)?;
    let current = word(ctx, at + 8)?;
    ctx.cpu.check_guest_access(size, 4, Prot::WRITE)?;
    ctx.cpu.check_guest_access(min, 4, Prot::WRITE)?;
    put(ctx, size, current)?;
    put(ctx, min, minimum)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn column_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    info(ctx, 0x84, 0x78)
}
fn row_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    info(ctx, 0x88, 0x74)
}
fn child_result(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    id: Option<u32>,
) -> Result<DispatchOutcome, KernelError> {
    for (i, v) in [
        (2, id.map_or(u32::MAX, |id| (id - 0xe900) >> 4)),
        (3, id.map_or(u32::MAX, |id| id & 15)),
    ] {
        let p = f.arg(ctx, i)?;
        if p != 0 {
            put(ctx, p, v)?;
        }
    }
    f.finish(ctx, u32::from(id.is_some()))
}
fn is_child(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let pane = f.arg(ctx, 1)?;
    if f.fresh {
        for i in [2, 3] {
            let p = f.arg(ctx, i)?;
            if p != 0 {
                ctx.cpu.check_guest_access(p, 4, Prot::WRITE)?;
            }
        }
        if pane == 0 {
            return child_result(ctx, f, None);
        }
        let parent = word(ctx, o + 0x20)?;
        let hwnd = word(ctx, pane + 0x20)?;
        return f.call_service(ctx, 1, "IsChild", &[parent, hwnd]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 1 {
        if result == 0 {
            return child_result(ctx, f, None);
        }
        let hwnd = word(ctx, pane + 0x20)?;
        return f.call_service(ctx, 2, "GetDlgCtrlID", &[hwnd]);
    }
    let id = result & 0xffff;
    child_result(ctx, f, (0xe900..=0xe9ff).contains(&id).then_some(id))
}
fn active(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::hierarchy::PARENT, &[o]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            let view = if result == 0 {
                0
            } else {
                word(ctx, result + 0xac)?
            };
            if view == 0 {
                return f.call_service(ctx, 2, "GetFocus", &[]);
            }
            f.set(ctx, 0, view)?;
            let row = f.arg(ctx, 1)?;
            let col = f.arg(ctx, 2)?;
            f.call_service(ctx, 3, CHILD, &[o, view, row, col])
        }
        2 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            let view = super::windows::wrapper(ctx, result)?;
            f.set(ctx, 0, view)?;
            let row = f.arg(ctx, 1)?;
            let col = f.arg(ctx, 2)?;
            f.call_service(ctx, 3, CHILD, &[o, view, row, col])
        }
        3 => {
            let view = if result == 0 { 0 } else { f.get(ctx, 0)? };
            f.finish(ctx, view)
        }
        _ => Err(KernelError::Dispatch(
            "invalid active splitter pane continuation".into(),
        )),
    }
}
fn set_active(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let pane = f.arg(ctx, 3)?;
        if pane != 0 {
            f.set(ctx, 0, pane)?;
            return kind(ctx, &f, pane);
        }
        let row = f.arg(ctx, 1)?;
        let col = f.arg(ctx, 2)?;
        return f.call_service(ctx, 1, GET, &[o, row, col]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 0, result)?;
            kind(ctx, &f, result)
        }
        2 => {
            let pane = f.get(ctx, 0)?;
            if result != 0 {
                return f.call_service(ctx, 3, super::hierarchy::PARENT, &[o]);
            }
            let hwnd = word(ctx, pane + 0x20)?;
            f.call_service(ctx, 4, "SetFocus", &[hwnd])
        }
        3 => {
            let pane = f.get(ctx, 0)?;
            if result == 0 {
                return Err(KernelError::Dispatch(
                    "splitter view has no parent frame".into(),
                ));
            }
            f.call_service(ctx, 4, super::views::ACTIVE, &[result, pane, 1])
        }
        4 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid SetActivePane continuation".into(),
        )),
    }
}
fn kind(ctx: &mut CallCtx<'_>, f: &Frame, p: u32) -> Result<DispatchOutcome, KernelError> {
    let class = super::classes::runtime(ctx, "CView")?;
    f.call_service(ctx, 2, super::class_queries::KIND, &[p, class])
}
fn can_next(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_virtual(ctx, 1, o, 56, &[0, 0]);
    }
    let found = ctx.cpu.read_reg(ArmReg::R0)? != 0;
    let more = word(ctx, o + 0x74)? > 1 || word(ctx, o + 0x78)? > 1;
    f.finish(ctx, u32::from(found && more))
}
fn activate_next(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_virtual(ctx, 1, o, 56, &[f.address + 128, f.address + 132]);
    }
    if f.phase(ctx)? == 1 {
        if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
            return f.finish(ctx, 0);
        }
        let rows = word(ctx, o + 0x74)?;
        let cols = word(ctx, o + 0x78)?;
        let row = word(ctx, f.address + 128)?;
        let col = word(ctx, f.address + 132)?;
        if rows == 0 || cols == 0 || rows > 16 || cols > 16 || row >= rows || col >= cols {
            return Err(KernelError::Dispatch(
                "invalid active splitter pane position".into(),
            ));
        }
        let n = row * cols + col;
        let n = if f.arg(ctx, 1)? != 0 {
            (n + rows * cols - 1) % (rows * cols)
        } else {
            (n + 1) % (rows * cols)
        };
        return f.call_virtual(ctx, 2, o, 57, &[n / cols, n % cols, 0]);
    }
    f.finish(ctx, 0)
}
fn create_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let row = f.arg(ctx, 1)?;
    let col = f.arg(ctx, 2)?;
    if f.fresh {
        let cx = ctx.arg_u32(4)?;
        let cy = ctx.arg_u32(5)?;
        let context = ctx.arg_u32(6)?;
        if row >= word(ctx, o + 0x74)? || col >= word(ctx, o + 0x78)? {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, cx)?;
        f.set(ctx, 1, cy)?;
        f.set(ctx, 2, context)?;
        let cp = word(ctx, o + 0x84)?;
        let rp = word(ctx, o + 0x88)?;
        put(ctx, cp + col * 12 + 4, cx)?;
        put(ctx, rp + row * 12 + 4, cy)?;
        if context == 0 {
            ctx.cpu.write_mem(f.address + 192, &[0; 20])?;
            return f.call_virtual(ctx, 1, o, 56, &[0, 0]);
        }
        return factory(ctx, &f);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result != 0 {
                f.set(ctx, 4, result)?;
                let class = super::classes::runtime(ctx, "CView")?;
                return f.call_service(ctx, 2, super::class_queries::KIND, &[result, class]);
            }
            factory(ctx, &f)
        }
        2 => {
            if result != 0 {
                let last = f.get(ctx, 4)?;
                let doc = word(ctx, last + 0x48)?;
                put(ctx, f.address + 204, last)?;
                put(ctx, f.address + 196, doc)?;
                if doc != 0 {
                    let template = word(ctx, doc + 0x28)?;
                    put(ctx, f.address + 200, template)?;
                }
            }
            factory(ctx, &f)
        }
        3 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 3, result)?;
            let cx = f.get(ctx, 0)?;
            let cy = f.get(ctx, 1)?;
            for (off, val) in [(0, 0), (4, 0), (8, cx), (12, cy)] {
                put(ctx, f.address + 128 + off, val)?;
            }
            let context = if f.get(ctx, 2)? == 0 {
                f.address + 192
            } else {
                f.get(ctx, 2)?
            };
            f.call_virtual(
                ctx,
                4,
                result,
                22,
                &[
                    0,
                    0,
                    0x50000000,
                    f.address + 128,
                    o,
                    pane_id(row, col),
                    context,
                ],
            )
        }
        4 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            if f.get(ctx, 2)? == 0 {
                let view = f.get(ctx, 3)?;
                let hwnd = word(ctx, view + 0x20)?;
                return f.call_service(ctx, 5, "SendMessageW", &[hwnd, 0x364, 0, 0]);
            }
            f.finish(ctx, 1)
        }
        5 => f.finish(ctx, 1),
        _ => Err(KernelError::Dispatch(
            "invalid splitter view creation continuation".into(),
        )),
    }
}
fn factory(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let class = f.arg(ctx, 3)?;
    f.call_service(ctx, 3, super::class_queries::CREATE, &[class])
}
fn delete_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let row = f.arg(ctx, 1)?;
        let col = f.arg(ctx, 2)?;
        return f.call_service(ctx, 1, GET, &[o, row, col]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 0, result)?;
            f.call_virtual(ctx, 2, o, 56, &[0, 0])
        }
        2 => {
            if result == f.get(ctx, 0)? {
                return f.call_virtual(ctx, 3, o, 59, &[0]);
            }
            destroy_pane(ctx, &f)
        }
        3 => destroy_pane(ctx, &f),
        4 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter pane deletion continuation".into(),
        )),
    }
}
fn destroy_pane(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let pane = f.get(ctx, 0)?;
    f.call_virtual(ctx, 4, pane, 23, &[])
}
