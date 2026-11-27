//! Popup initialization updates each live command through actual CCmdUI routing.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[(
    1947,
    "?OnInitMenuPopup@CFrameWnd@@IAAXPAVCMenu@@IH@Z",
    popup,
)];
fn handle(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<u32, KernelError> {
    let menu = f.arg(ctx, 1)?;
    word(ctx, menu + 4)
}
fn count(ctx: &mut CallCtx<'_>, f: &Frame, phase: u32) -> Result<DispatchOutcome, KernelError> {
    let menu = handle(ctx, f)?;
    f.call_service(ctx, phase, "GetMenuItemCount", &[menu])
}
fn next(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let ui = f.address + 128;
    let index = word(ctx, ui + 8)?;
    if index >= word(ctx, ui + 0x20)? {
        return f.finish(ctx, 0);
    }
    let steps = f.get(ctx, 2)?;
    if steps >= 8192 {
        return Err(KernelError::Dispatch(
            "MFC popup update exceeds its bound".into(),
        ));
    }
    f.set(ctx, 2, steps + 1)?;
    let menu = handle(ctx, &f)?;
    f.call_service(ctx, 6, "GetMenuItemID", &[menu, index])
}
fn advance(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let ui = f.address + 128;
    let index = word(ctx, ui + 8)?.wrapping_add(1);
    put(ctx, ui + 8, index)?;
    next(ctx, f)
}
fn changed(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let n = f.get(ctx, 0)?;
    put(ctx, f.address + 128 + 0x20, n)?;
    advance(ctx, f)
}
fn update(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    automatic: bool,
) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    f.call_service(
        ctx,
        10,
        super::command_ui::UPDATE,
        &[f.address + 128, object, u32::from(automatic)],
    )
}
fn popup(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let ui = f.address + 128;
    if f.fresh {
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, super::pretranslation_frames::CANCEL, &[hwnd]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if f.arg(ctx, 3)? != 0 {
                return f.finish(ctx, 0);
            }
            let menu = f.arg(ctx, 1)?;
            ctx.cpu
                .check_guest_access(menu, 8, pocket_cpu::Prot::READ)?;
            super::command_ui::initialize(ctx, ui)?;
            put(ctx, ui + 0xc, menu)?;
            let thread = super::application::current_thread_state(ctx)?;
            let menu_handle = handle(ctx, &f)?;
            if word(ctx, thread + 0x5c)? == menu_handle {
                let menu = f.arg(ctx, 1)?;
                put(ctx, ui + 0x24, menu)?;
                return count(ctx, &f, 5);
            }
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 2, "GetMenu", &[hwnd])
        }
        2 => {
            if result == 0 {
                return count(ctx, &f, 5);
            }
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 3, super::pretranslation::ROOT, &[hwnd])
        }
        3 => {
            if result == 0 {
                return count(ctx, &f, 5);
            }
            f.call_service(ctx, 4, "GetMenu", &[result])
        }
        4 => {
            let child = handle(ctx, &f)?;
            if ctx
                .kernel
                .menu_items
                .get(&result)
                .is_some_and(|items| items.iter().any(|i| i.submenu == child))
            {
                let parent = super::menus::wrapper(ctx, result)?;
                put(ctx, ui + 0x24, parent)?;
            }
            count(ctx, &f, 5)
        }
        5 => {
            if result > 4096 {
                return Err(KernelError::Dispatch("Invalid MFC popup menu count".into()));
            }
            put(ctx, ui + 0x20, result)?;
            put(ctx, ui + 8, 0)?;
            next(ctx, f)
        }
        6 => {
            put(ctx, ui + 4, result)?;
            if result == 0 {
                return advance(ctx, f);
            }
            if result == u32::MAX {
                let menu = handle(ctx, &f)?;
                let index = word(ctx, ui + 8)?;
                return f.call_service(ctx, 7, "GetSubMenu", &[menu, index]);
            }
            put(ctx, ui + 0x10, 0)?;
            let automatic = word(ctx, object + 0x54)? != 0 && result < 0xf000;
            update(ctx, &f, automatic)
        }
        7 => {
            let submenu = super::menus::wrapper(ctx, result)?;
            put(ctx, ui + 0x10, submenu)?;
            if submenu == 0 {
                return advance(ctx, f);
            }
            f.call_service(ctx, 8, "GetMenuItemID", &[result, 0])
        }
        8 => {
            put(ctx, ui + 4, result)?;
            if result == 0 || result == u32::MAX {
                return advance(ctx, f);
            }
            update(ctx, &f, false)
        }
        10 => count(ctx, &f, 11),
        11 => {
            if result > 4096 {
                return Err(KernelError::Dispatch(
                    "Invalid mutated MFC popup menu count".into(),
                ));
            }
            f.set(ctx, 0, result)?;
            let old = word(ctx, ui + 0x20)?;
            if result < old {
                let index = word(ctx, ui + 8)?.wrapping_add(result).wrapping_sub(old);
                put(ctx, ui + 8, index)?;
                if index < result {
                    let menu = handle(ctx, &f)?;
                    return f.call_service(ctx, 12, "GetMenuItemID", &[menu, index]);
                }
            }
            changed(ctx, f)
        }
        12 => {
            if result == word(ctx, ui + 4)? {
                let index = word(ctx, ui + 8)?.wrapping_add(1);
                put(ctx, ui + 8, index)?;
                if index < f.get(ctx, 0)? {
                    let menu = handle(ctx, &f)?;
                    return f.call_service(ctx, 12, "GetMenuItemID", &[menu, index]);
                }
            }
            changed(ctx, f)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC popup continuation".into(),
        )),
    }
}
