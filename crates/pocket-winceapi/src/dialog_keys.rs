//! WinCE dialog navigation. Gizmondo Play/Soft1/Soft2 already emit
//! Return/Tab/Escape; preserve those shared bindings and consult WM_GETDLGCODE.
use crate::CallCtx;
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{
    controls::ControlClass, DialogKeyFrame, DispatchOutcome, GuestCallFrame, KernelError,
    KernelState,
};

fn parent(kernel: &KernelState, hwnd: u32) -> Option<u32> {
    kernel
        .controls
        .get(hwnd)
        .map(|c| c.parent)
        .or_else(|| kernel.window_parents.get(&hwnd).copied())
}
fn belongs(kernel: &KernelState, mut hwnd: u32, dialog: u32) -> bool {
    for _ in 0..64 {
        if hwnd == dialog {
            return true;
        }
        let Some(next) = parent(kernel, hwnd) else {
            return false;
        };
        if next == hwnd {
            return false;
        }
        hwnd = next;
    }
    false
}
fn restore(ctx: &mut CallCtx<'_>, frame: GuestCallFrame) -> Result<(), KernelError> {
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(frame.args)
    {
        ctx.cpu.write_reg(r, v)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, frame.lr)?;
    Ok(())
}
fn invoke(
    ctx: &mut CallCtx<'_>,
    state: DialogKeyFrame,
    proc: u32,
    args: [u32; 4],
) -> Result<DispatchOutcome, KernelError> {
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(args)
    {
        ctx.cpu.write_reg(r, v)?;
    }
    ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
    ctx.kernel.dialog_key_frames.push(state);
    Ok(DispatchOutcome::JumpTo(proc))
}
fn wants_key(code: u32, key: u32) -> bool {
    code & 4 != 0 || (key == 9 && code & 2 != 0)
}
fn command_target(kernel: &KernelState, dialog: u32, key: u32) -> (u32, u32) {
    if key == 0x1b {
        return (2, 0);
    }
    let focus = kernel.controls.get(kernel.controls.focus);
    if let Some(c) = focus.filter(|c| {
        c.parent == dialog
            && c.class == ControlClass::Button
            && c.enabled
            && c.visible
            && c.style & 0xf <= 1
    }) {
        return (c.id, c.hwnd);
    }
    let default_id = kernel.window_extra.get(&(dialog, -1200)).copied();
    let button = kernel
        .controls
        .children_of(dialog)
        .into_iter()
        .filter_map(|h| kernel.controls.get(h))
        .find(|c| {
            c.class == ControlClass::Button
                && c.visible
                && c.enabled
                && default_id.map_or(c.style & 0xf == 1, |id| c.id == id)
        });
    button
        .map(|c| (c.id, c.hwnd))
        .unwrap_or((default_id.unwrap_or(1), 0))
}
fn process(
    ctx: &mut CallCtx<'_>,
    mut state: DialogKeyFrame,
    code: u32,
) -> Result<DispatchOutcome, KernelError> {
    log::debug!(
        "IsDialogMessage dialog=0x{:08x} target=0x{:08x} key=0x{:x} dlgcode=0x{:x}",
        state.dialog,
        state.target,
        state.key,
        code
    );
    if wants_key(code, state.key) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if state.key == 9 {
        let order: Vec<_> = ctx
            .kernel
            .controls
            .children_of(state.dialog)
            .into_iter()
            .filter(|h| {
                ctx.kernel.controls.get(*h).is_some_and(|c| {
                    c.visible && c.enabled && c.style & 0x10000 != 0 && c.is_focusable()
                })
            })
            .collect();
        if !order.is_empty() {
            let index = order.iter().position(|h| *h == ctx.kernel.controls.focus);
            let shift = ctx.kernel.pressed_keys[0x10]
                || ctx.kernel.pressed_keys[0xa0]
                || ctx.kernel.pressed_keys[0xa1];
            let next = match index {
                Some(i) if shift => (i + order.len() - 1) % order.len(),
                Some(i) => (i + 1) % order.len(),
                None if shift => order.len() - 1,
                None => 0,
            };
            ctx.kernel.controls.focus = order[next];
        }
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let (id, button) = command_target(ctx.kernel, state.dialog, state.key);
    log::debug!("dialog key command id={} button=0x{:08x}", id, button);
    let proc = ctx
        .kernel
        .window_procs
        .get(&state.dialog)
        .copied()
        .unwrap_or(0);
    if proc == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    state.stage = 1;
    invoke(ctx, state, proc, [state.dialog, 0x111, id, button])
}
pub(crate) fn is_dialog_message(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if ctx.kernel.dialog_key_frames.last().is_some_and(|s| {
        s.thread == ctx.kernel.current_thread && s.thunk == ctx.thunk.thunk_va && s.frame.sp == sp
    }) {
        let code = ctx.cpu.read_reg(ArmReg::R0)?;
        let state = ctx.kernel.dialog_key_frames.pop().unwrap();
        restore(ctx, state.frame)?;
        return if state.stage == 1 {
            Ok(DispatchOutcome::ReturnedR0(1))
        } else {
            process(ctx, state, code)
        };
    }
    let dialog = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    if dialog == 0 || ptr == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let msg = ctx.cpu.read_mem(ptr, 16)?;
    let field = |i| u32::from_le_bytes(msg[i..i + 4].try_into().unwrap());
    let target = field(0);
    let key = field(8);
    if field(4) != 0x100 || !matches!(key, 9 | 0xd | 0x1b) || !belongs(ctx.kernel, target, dialog) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let frame = GuestCallFrame {
        args: [dialog, ptr, ctx.arg_u32(2)?, ctx.arg_u32(3)?],
        sp,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    let state = DialogKeyFrame {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame,
        dialog,
        target,
        key,
        stage: 0,
    };
    if let Some(proc) = ctx
        .kernel
        .window_procs
        .get(&target)
        .copied()
        .filter(|p| *p != 0)
    {
        return invoke(ctx, state, proc, [target, 0x87, key, ptr]);
    }
    let code = ctx.kernel.controls.get(target).map_or(0, |c| {
        if c.class == ControlClass::Edit && c.style & 0x1004 == 0x1004 {
            4
        } else {
            0
        }
    });
    process(ctx, state, code)
}
#[cfg(test)]
mod tests {
    use super::wants_key;
    #[test]
    fn dialog_respects_control_key_requests() {
        assert!(wants_key(4, 0xd));
        assert!(wants_key(4, 0x1b));
        assert!(wants_key(2, 9));
        assert!(!wants_key(2, 0xd));
        assert!(!wants_key(0x81, 0xd));
    }
}
