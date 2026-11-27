//! CE3 command/update routing follows actual guest maps and virtual targets.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const BASE: &str = "__PocketHLE_MFC_CommandTargetMessage";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1830,
        "?OnCmdMsg@CDialog@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
        dialog,
    ),
    (
        1829,
        "?OnCmdMsg@CCmdTarget@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
        base,
    ),
    (
        1833,
        "?OnCmdMsg@CFrameWnd@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
        frame,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", BASE, base);
}
fn base(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let id = f.arg(ctx, 1)?;
    let extra = f.arg(ctx, 3)?;
    if f.fresh {
        let info = ctx.arg_u32(4)?;
        if info != 0 {
            ctx.cpu.check_guest_access(info, 8, Prot::WRITE)?;
        }
        f.set(ctx, 0, info)?;
        let code = f.arg(ctx, 2)?;
        if code == u32::MAX - 1 {
            return Err(KernelError::Dispatch(
                "MFC OLE command routing is not implemented".into(),
            ));
        }
        let (message, code) = if code == u32::MAX {
            (0x111, code)
        } else {
            (
                if code >> 16 == 0 { 0x111 } else { code >> 16 },
                code & 0xffff,
            )
        };
        f.set(ctx, 2, code)?;
        f.set(ctx, 3, message)?;
        let target = virtual_function(ctx, object, 12)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 3 {
        let signature = f.get(ctx, 4)?;
        let value = match signature {
            2 | 35 | 39 | 41 | 47 => returned,
            44 | 45 => {
                let continuing = word(ctx, extra + 0x1c)?;
                put(ctx, extra + 0x1c, 0)?;
                u32::from(continuing == 0)
            }
            _ => 1,
        };
        return f.finish(ctx, value);
    }
    if !matches!(f.phase(ctx)?, 1 | 2) {
        return Err(KernelError::Dispatch(
            "Invalid MFC command-map continuation".into(),
        ));
    }
    let map = returned;
    if map == 0 {
        return f.finish(ctx, 0);
    }
    ctx.cpu.check_guest_access(map, 8, Prot::READ)?;
    let count = f.get(ctx, 1)?;
    if count >= 32 {
        return Err(KernelError::Dispatch(
            "MFC command-map inheritance exceeds its bound".into(),
        ));
    }
    for i in 0..count {
        if word(ctx, f.address + 96 + 4 * i)? == map {
            return Err(KernelError::Dispatch(
                "Cyclic MFC command-map inheritance".into(),
            ));
        }
    }
    put(ctx, f.address + 96 + 4 * count, map)?;
    f.set(ctx, 1, count + 1)?;
    let entries = word(ctx, map + 4)?;
    let message = f.get(ctx, 3)?;
    let code = f.get(ctx, 2)?;
    let Some(fields) = super::message_maps::find_command_entry(ctx, entries, message, code, id)?
    else {
        let getter = word(ctx, map)?;
        return f.call_args(ctx, 2, getter, &[]);
    };
    let info = f.get(ctx, 0)?;
    if info != 0 {
        put(ctx, info, object)?;
        put(ctx, info + 4, fields[5])?;
        return f.finish(ctx, 1);
    }
    let signature = fields[4];
    let args = match signature {
        2 | 13 | 35 => vec![object, id],
        12 => vec![object],
        38 | 39 => vec![object, word(ctx, extra + 4)?, word(ctx, extra)?],
        40 | 41 => vec![object, id, word(ctx, extra + 4)?, word(ctx, extra)?],
        44 | 46 | 47 => vec![object, extra],
        45 => vec![object, extra, id],
        _ => {
            return Err(KernelError::Dispatch(format!(
                "MFC command signature {signature} is not implemented"
            )))
        }
    };
    if matches!(signature, 44 | 45) {
        ctx.cpu
            .check_guest_access(extra, 40, Prot::READ | Prot::WRITE)?;
    }
    f.set(ctx, 4, signature)?;
    f.call_args(ctx, 3, fields[5], &args)
}
fn finish_frame(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    value: u32,
) -> Result<DispatchOutcome, KernelError> {
    let state = f.get(ctx, 0)?;
    let previous = f.get(ctx, 1)?;
    put(ctx, state + 0x12c, previous)?;
    f.finish(ctx, value)
}
fn route(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    target: u32,
    phase: u32,
) -> Result<DispatchOutcome, KernelError> {
    let function = virtual_function(ctx, target, 5)?;
    let args = [
        target,
        f.arg(ctx, 1)?,
        f.arg(ctx, 2)?,
        f.arg(ctx, 3)?,
        f.get(ctx, 2)?,
    ];
    f.call_args(ctx, phase, function, &args)
}
fn frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let info = ctx.arg_u32(4)?;
        if info != 0 {
            ctx.cpu.check_guest_access(info, 8, Prot::WRITE)?;
        }
        f.set(ctx, 2, info)?;
        let state = super::application::current_thread_state(ctx)?;
        f.set(ctx, 0, state)?;
        let previous = word(ctx, state + 0x12c)?;
        f.set(ctx, 1, previous)?;
        put(ctx, state + 0x12c, object)?;
        let view = word(ctx, object + 0xac)?;
        if view != 0 {
            return route(ctx, &f, view, 1);
        }
    } else {
        if !matches!(f.phase(ctx)?, 1..=3) {
            return Err(KernelError::Dispatch(
                "Invalid MFC frame command continuation".into(),
            ));
        }
        let returned = ctx.cpu.read_reg(ArmReg::R0)?;
        if returned != 0 {
            return finish_frame(ctx, f, 1);
        }
        if f.phase(ctx)? == 3 {
            return finish_frame(ctx, f, 0);
        }
        if f.phase(ctx)? == 2 {
            let module = super::application::state(ctx)?;
            let app = word(ctx, module + 4)?;
            if app == 0 {
                return finish_frame(ctx, f, 0);
            }
            return route(ctx, &f, app, 3);
        }
        if f.phase(ctx)? != 1 {
            return Err(KernelError::Dispatch(
                "Invalid MFC frame command continuation".into(),
            ));
        }
    }
    let target = service(ctx, BASE)?;
    let args = [
        object,
        f.arg(ctx, 1)?,
        f.arg(ctx, 2)?,
        f.arg(ctx, 3)?,
        f.get(ctx, 2)?,
    ];
    f.call_args(ctx, 2, target, &args)
}

fn dialog(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let id = f.arg(ctx, 1)?;
    let code = f.arg(ctx, 2)?;
    let extra = f.arg(ctx, 3)?;
    if f.fresh {
        let info = ctx.arg_u32(4)?;
        f.set(ctx, 0, info)?;
        let target = service(ctx, BASE)?;
        return f.call_args(ctx, 1, target, &[object, id, code, extra, info]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    let info = f.get(ctx, 0)?;
    match f.phase(ctx)? {
        1 if result != 0 => f.finish(ctx, 1),
        1 if (code != 0 && code != u32::MAX) || !(0x8000..0xf000).contains(&id) => f.finish(ctx, 0),
        1 => {
            let hwnd = word(ctx, object + 0x20)?;
            let target = service(ctx, "GetParent")?;
            f.call_args(ctx, 2, target, &[hwnd])
        }
        2 if result != 0 => {
            let parent = super::windows::wrapper(ctx, result)?;
            let target = virtual_function(ctx, parent, 5)?;
            f.call_args(ctx, 3, target, &[parent, id, code, extra, info])
        }
        2 | 3 => {
            if f.phase(ctx)? == 3 && result != 0 {
                return f.finish(ctx, 1);
            }
            let module = super::application::state(ctx)?;
            let app = word(ctx, module + 4)?;
            if app == 0 {
                return f.finish(ctx, 0);
            }
            let target = virtual_function(ctx, app, 5)?;
            f.call_args(ctx, 4, target, &[app, id, code, extra, info])
        }
        4 => f.finish(ctx, u32::from(result != 0)),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC dialog command continuation".into(),
        )),
    }
}
