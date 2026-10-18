//! Observed GMSHELLCLASS text-panel messages. Unknown commands fail rather
//! than claiming hardware/shell services that are not implemented.
use crate::CallCtx;
use pocket_kernel::{controls::ControlClass, DispatchOutcome, KernelError};
fn string(ctx: &mut CallCtx<'_>, ptr: u32) -> Result<String, KernelError> {
    if ptr == 0 {
        return Ok(String::new());
    }
    let mut units = Vec::new();
    for i in 0..128 {
        let raw = ctx.cpu.read_mem(ptr + i * 2, 2)?;
        let u = u16::from_le_bytes([raw[0], raw[1]]);
        if u == 0 {
            break;
        }
        units.push(u);
    }
    Ok(String::from_utf16_lossy(&units))
}
pub(crate) fn find(ctx: &mut CallCtx<'_>) -> Result<bool, KernelError> {
    let class = ctx.arg_u32(0)?;
    if class == 0 || class <= 0xffff {
        return Ok(false);
    }
    let class = string(ctx, class)?;
    if !class.eq_ignore_ascii_case("GMSHELLCLASS") {
        return Ok(false);
    }
    let title_ptr = ctx.arg_u32(1)?;
    let title = string(ctx, title_ptr)?;
    Ok(title.is_empty() || title.eq_ignore_ascii_case("GMSHELL"))
}
fn edit_target(ctx: &CallCtx<'_>) -> u32 {
    let focus = ctx.kernel.controls.focus;
    let Some(control) = ctx.kernel.controls.get(focus) else {
        return 0;
    };
    if control.class == ControlClass::Edit
        && control.enabled
        && crate::coredll::window_is_visible(ctx.kernel, focus)
    {
        return focus;
    }
    ctx.kernel
        .controls
        .children_of(control.parent)
        .into_iter()
        .find(|h| {
            ctx.kernel.controls.get(*h).is_some_and(|c| {
                c.class == ControlClass::Edit
                    && c.visible
                    && c.enabled
                    && crate::coredll::window_is_visible(ctx.kernel, *h)
            })
        })
        .unwrap_or(0)
}
fn post(ctx: &mut CallCtx<'_>, hwnd: u32, msg: u32, wp: u32, lp: u32) {
    if hwnd != 0 && ctx.kernel.posted_messages.len() < 256 {
        ctx.kernel.posted_messages.push_back((hwnd, msg, wp, lp));
    }
}
fn open(ctx: &mut CallCtx<'_>) -> bool {
    let target = edit_target(ctx);
    if target == 0 {
        return false;
    }
    ctx.kernel.shell_keyboard.target = target;
    ctx.kernel.shell_keyboard.visible = true;
    true
}
/// A show request may precede SetFocus during native page construction.
/// Resolve it when the application returns to its message pump, without
/// showing a text panel over list-only pages.
pub(crate) fn service(ctx: &mut CallCtx<'_>) {
    if !ctx.kernel.shell_keyboard.requested {
        return;
    }
    let target = edit_target(ctx);
    ctx.kernel.shell_keyboard.visible = target != 0 && ctx.kernel.modal.is_none();
    if target != 0 {
        ctx.kernel.shell_keyboard.target = target;
    }
}

pub(crate) fn message(
    ctx: &mut CallCtx<'_>,
    msg: u32,
    key: u32,
    lp: u32,
) -> Result<DispatchOutcome, KernelError> {
    let result = match msg {
        0x4a => {
            let raw = ctx.cpu.read_mem(lp, 12)?;
            let size = u32::from_le_bytes(raw[4..8].try_into().unwrap());
            let ptr = u32::from_le_bytes(raw[8..12].try_into().unwrap());
            if !(3..=256).contains(&size) || ptr == 0 {
                return Ok(DispatchOutcome::ReturnedR0(0));
            }
            let data = ctx.cpu.read_mem(ptr, size)?;
            let command = u16::from_le_bytes([data[0], data[1]]);
            log::debug!("GMSHELL COPYDATA command={command} bytes={data:?}");
            match command {
                0x90 | 0x190 => {
                    if data[2] != 0 {
                        ctx.kernel.shell_keyboard.requested = true;
                        service(ctx);
                        1
                    } else {
                        ctx.kernel.shell_keyboard.visible = false;
                        ctx.kernel.shell_keyboard.requested = false;
                        1
                    }
                }
                _ => 0,
            }
        }
        0x101 => 1,
        0x100 => {
            if !ctx.kernel.shell_keyboard.visible {
                if open(ctx) {
                    ctx.kernel.shell_keyboard.requested = true;
                    return Ok(DispatchOutcome::ReturnedR0(1));
                }
                return Ok(DispatchOutcome::ReturnedR0(0));
            }
            let focused = edit_target(ctx);
            if focused != 0 {
                ctx.kernel.shell_keyboard.target = focused;
            }
            let target = ctx.kernel.shell_keyboard.target;
            match key {
                0x09 => {
                    if let Some(control) = ctx.kernel.controls.get(target) {
                        let edits: Vec<_> = ctx
                            .kernel
                            .controls
                            .children_of(control.parent)
                            .into_iter()
                            .filter(|h| {
                                ctx.kernel.controls.get(*h).is_some_and(|c| {
                                    c.class == ControlClass::Edit
                                        && c.enabled
                                        && crate::coredll::window_is_visible(ctx.kernel, *h)
                                })
                            })
                            .collect();
                        if let Some(i) = edits.iter().position(|h| *h == target) {
                            let next = edits[(i + 1) % edits.len()];
                            ctx.kernel.controls.focus = next;
                            ctx.kernel.shell_keyboard.target = next;
                        }
                    }
                }
                0x25..=0x28 => ctx.kernel.shell_keyboard.navigate(key),
                0xd => {
                    let ch = ctx.kernel.shell_keyboard.character();
                    post(ctx, target, 0x102, ch, 1);
                }
                0x11 | 8 => post(ctx, target, 0x102, 8, 1),
                0x20 | 0x1b => {
                    ctx.kernel.shell_keyboard.visible = false;
                    ctx.kernel.shell_keyboard.requested = false;
                    // Returning from the shell's input panel to the application
                    // re-enters its pointer-input mode before the Done key.
                    let active = ctx
                        .kernel
                        .controls
                        .get(target)
                        .map(|c| c.parent)
                        .unwrap_or(ctx.kernel.active_window);
                    post(ctx, active, 0x200, 0, 0);
                }
                0x30..=0x39 | 0x41..=0x5a | 0xbd | 0xbe => post(
                    ctx,
                    target,
                    0x102,
                    match key {
                        0xbd => 45,
                        0xbe => 46,
                        _ => key,
                    },
                    1,
                ),
                _ => {}
            }
            1
        }
        _ => 0,
    };
    Ok(DispatchOutcome::ReturnedR0(result))
}
