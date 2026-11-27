//! Splitter command/notify forwarding and shared-scrollbar synchronization.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1931,
        "?OnHScroll@CSplitterWnd@@IAAXIIPAVCScrollBar@@@Z",
        horizontal,
    ),
    (
        2057,
        "?OnVScroll@CSplitterWnd@@IAAXIIPAVCScrollBar@@@Z",
        vertical,
    ),
    (1839, "?OnCommand@CSplitterWnd@@MAAHIJ@Z", command),
    (1965, "?OnNotify@CSplitterWnd@@MAAHIJPAJ@Z", notify),
];
fn command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, false)
}
fn notify(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, true)
}
fn forward(ctx: &mut CallCtx<'_>, notify: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let wp = f.arg(ctx, 1)?;
        let lp = f.arg(ctx, 2)?;
        let out = f.arg(ctx, 3)?;
        return f.call_service(
            ctx,
            1,
            if notify {
                super::window_notify::BASE
            } else {
                super::window_commands::WINDOW
            },
            &[o, wp, lp, out],
        );
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result != 0 {
                return f.finish(ctx, 1);
            }
            f.call_service(ctx, 2, super::hierarchy::PARENT, &[o])
        }
        2 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            let hwnd = word(ctx, result + 0x20)?;
            let wp = f.arg(ctx, 1)?;
            let lp = f.arg(ctx, 2)?;
            f.call_service(
                ctx,
                3,
                "SendMessageW",
                &[hwnd, if notify { 0x4e } else { 0x111 }, wp, lp],
            )
        }
        3 => {
            if notify {
                let out = f.arg(ctx, 3)?;
                put(ctx, out, result)?;
            }
            f.finish(ctx, if notify { 1 } else { result })
        }
        _ => Err(KernelError::Dispatch(
            "invalid splitter notification continuation".into(),
        )),
    }
}
fn horizontal(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll(ctx, false)
}
fn vertical(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll(ctx, true)
}
fn scroll(ctx: &mut CallCtx<'_>, vertical: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let control = f.arg(ctx, 3)?;
        if control == 0 {
            return f.call_service(ctx, 10, "__PocketHLE_MFC_WindowDefault", &[o]);
        }
        let hwnd = word(ctx, control + 0x20)?;
        f.set(ctx, 0, hwnd)?;
        return f.call_service(ctx, 1, "GetDlgCtrlID", &[hwnd]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            let index = (result & 0xffff).wrapping_sub(if vertical { 0xea10 } else { 0xea00 });
            let count = word(ctx, o + if vertical { 0x74 } else { 0x78 })?;
            if index >= count {
                return Err(KernelError::Dispatch(
                    "invalid shared splitter scrollbar ID".into(),
                ));
            }
            f.set(ctx, 1, index)?;
            let hwnd = f.get(ctx, 0)?;
            f.call_service(ctx, 2, "GetScrollPos", &[hwnd, 2])
        }
        2 => {
            f.set(ctx, 2, result)?;
            next(ctx, f, o, vertical)
        }
        3 => {
            if result == 0 {
                return Err(KernelError::Dispatch(
                    "shared-scrollbar pane is missing".into(),
                ));
            }
            let hwnd = word(ctx, result + 0x20)?;
            let wp = (f.arg(ctx, 1)? & 0xffff) | ((f.arg(ctx, 2)? & 0xffff) << 16);
            let scroll = f.get(ctx, 0)?;
            f.call_service(
                ctx,
                4,
                "SendMessageW",
                &[hwnd, if vertical { 0x115 } else { 0x114 }, wp, scroll],
            )
        }
        4 => {
            let i = f.get(ctx, 3)?;
            f.set(ctx, 3, i + 1)?;
            let count = word(ctx, o + if vertical { 0x78 } else { 0x74 })?;
            if i + 1 < count {
                let hwnd = f.get(ctx, 0)?;
                let pos = f.get(ctx, 2)?;
                return f.call_service(ctx, 5, "SetScrollPos", &[hwnd, 2, pos, 0]);
            }
            f.finish(ctx, 0)
        }
        5 => next(ctx, f, o, vertical),
        10 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "invalid splitter shared-scroll continuation".into(),
        )),
    }
}
fn next(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
    vertical: bool,
) -> Result<DispatchOutcome, KernelError> {
    let cross = f.get(ctx, 3)?;
    if cross >= word(ctx, o + if vertical { 0x78 } else { 0x74 })? {
        return f.finish(ctx, 0);
    }
    let index = f.get(ctx, 1)?;
    f.call_service(
        ctx,
        3,
        super::splitter_panes::GET,
        &[
            o,
            if vertical { index } else { cross },
            if vertical { cross } else { index },
        ],
    )
}
