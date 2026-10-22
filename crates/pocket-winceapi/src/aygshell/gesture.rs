//! Cancellable PocketPC stylus/action hold recognition. Guest callbacks remain synchronous.
use crate::{coredll, CallCtx};
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{DispatchOutcome, GuestCallFrame, InputEvent, KernelError};

pub(super) fn recognize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    let thunk = ctx.thunk.thunk_va;
    let thread = ctx.kernel.current_thread;
    if ctx
        .kernel
        .gesture_frames
        .last()
        .is_some_and(|f| f.0 == thread && f.1 == thunk && f.2.sp == sp)
    {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        let (_, _, frame, storage) = ctx.kernel.gesture_frames.pop().unwrap();
        ctx.kernel.heap.free(storage);
        for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
            .into_iter()
            .zip(frame.args)
        {
            ctx.cpu.write_reg(reg, value)?;
        }
        ctx.cpu.write_reg(ArmReg::Lr, frame.lr)?;
        return Ok(DispatchOutcome::ReturnedR0(result));
    }
    let key = (thread, thunk, sp);
    let ptr = ctx.arg_u32(0)?;
    let Ok(info) = ctx.cpu.read_mem(ptr, 20) else {
        ctx.kernel.wait_deadlines.remove(&key);
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let word = |o| u32::from_le_bytes(info[o..o + 4].try_into().unwrap());
    let (hwnd, x, y, flags) = (word(4), word(8) as i32, word(12) as i32, word(16));
    if word(0) != 20 || !coredll::window_is_visible(ctx.kernel, hwnd) || flags & 0xb == 0 {
        ctx.kernel.wait_deadlines.remove(&key);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let hold_key = (thread, hwnd, x, y);
    let recognized = ctx.kernel.controls.recognized_holds.contains(&hold_key);
    let now = coredll::monotonic_ms();
    let deadline = *ctx
        .kernel
        .wait_deadlines
        .entry(key)
        .or_insert(now.saturating_add(if flags & 8 != 0 { 1500 } else { 800 }));
    let (ox, oy) = ctx.kernel.window_client_origin(hwnd).unwrap_or((0, 0));
    let action = ctx.kernel.pressed_keys[0x0d];
    // Leave cancelled input in the queue for the caller's normal message pump.
    let cancelled = ctx.kernel.pending_input.iter().any(|event| match event {
        InputEvent::PointerUp { .. } | InputEvent::PointerDown { .. } if !action => true,
        InputEvent::PointerMove { x: px, y: py } if !action => {
            (i32::from(*px) - ox - x).abs() > 8 || (i32::from(*py) - oy - y).abs() > 8
        }
        InputEvent::KeyUp { vk: 0x0d } if action => true,
        _ => false,
    });
    if (cancelled || (!action && !ctx.kernel.pressed_keys[1])) && !recognized {
        ctx.kernel.wait_deadlines.remove(&key);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if now < deadline && !recognized {
        std::thread::sleep(std::time::Duration::from_millis(1));
        return Ok(DispatchOutcome::JumpTo(thunk));
    }
    ctx.kernel.wait_deadlines.remove(&key);
    ctx.kernel.controls.recognized_holds.insert(hold_key);
    if flags & 1 != 0 {
        return Ok(DispatchOutcome::ReturnedR0(1000));
    }
    let receiver = if flags & 2 != 0 {
        ctx.kernel.window_parents.get(&hwnd).copied().unwrap_or(0)
    } else {
        hwnd
    };
    let Some(proc) = ctx
        .kernel
        .window_procs
        .get(&receiver)
        .copied()
        .filter(|p| *p != 0)
    else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let DispatchOutcome::ReturnedR0(storage) = coredll::do_alloc(ctx, 20)? else {
        unreachable!()
    };
    if storage == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let id = ctx
        .kernel
        .controls
        .get(hwnd)
        .map(|c| c.id)
        .or_else(|| ctx.kernel.window_ids.get(&hwnd).copied())
        .unwrap_or(0);
    ctx.cpu.write_mem(
        storage,
        &[hwnd, id, 1000, x as u32, y as u32]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    let frame = GuestCallFrame {
        args: [
            ctx.arg_u32(0)?,
            ctx.arg_u32(1)?,
            ctx.arg_u32(2)?,
            ctx.arg_u32(3)?,
        ],
        sp,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    ctx.kernel
        .gesture_frames
        .push((thread, thunk, frame, storage));
    ctx.cpu.write_reg(ArmReg::R0, receiver)?;
    ctx.cpu.write_reg(ArmReg::R1, 0x4e)?;
    ctx.cpu.write_reg(ArmReg::R2, id)?;
    ctx.cpu.write_reg(ArmReg::R3, storage)?;
    ctx.cpu.write_reg(ArmReg::Lr, thunk)?;
    Ok(DispatchOutcome::JumpTo(proc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn hold_waits_and_cancel_preserves_original_input() {
        let (mut cpu, mut k, mut d) = setup();
        k.pressed_keys[1] = true;
        k.window_classes.insert(123, "Custom".into());
        k.window_visibility.insert(123, true);
        k.window_rects.insert(123, (20, 30, 100, 100));
        cpu.write_mem(
            0x1000,
            &[20u32, 123, 5, 6, 1]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "ord:40",
                &[0x1000]
            ),
            DispatchOutcome::JumpTo(_)
        ));
        k.wait_deadlines.insert((0, 0x70000000, 0x4000), 0);
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "ord:40",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(1000)
        ));
        for event in [
            InputEvent::PointerUp { x: 25, y: 36 },
            InputEvent::PointerMove { x: 50, y: 36 },
        ] {
            // A fresh stroke, independent of the successfully recognized one above.
            k.controls.recognized_holds.clear();
            k.pending_input.push_back(event);
            assert!(matches!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "aygshell.dll",
                    "ord:40",
                    &[0x1000]
                ),
                DispatchOutcome::ReturnedR0(0)
            ));
            assert_eq!(k.pending_input.pop_front(), Some(event));
            assert!(k.wait_deadlines.is_empty());
        }
    }
    #[test]
    fn recognized_return_command_can_notify_parent_without_another_hold() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(
            0x50000000,
            0x10000,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )
        .unwrap();
        k.pressed_keys[1] = true;
        k.window_classes.insert(123, "Custom".into());
        k.window_visibility.insert(123, true);
        k.window_rects.insert(123, (0, 0, 100, 100));
        k.window_parents.insert(123, 124);
        k.window_procs.insert(124, 0x123400);
        cpu.write_mem(
            0x1000,
            &[20u32, 123, 5, 6, 1]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        k.wait_deadlines.insert((0, 0x70000000, 0x4000), 0);
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "ord:40",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(1000)
        ));
        // The MFC pattern reissues SHRGINFO with NOTIFYPARENT for the SAME stroke.
        // Even a release queued while the first call completed must not erase recognition.
        k.pending_input
            .push_back(InputEvent::PointerUp { x: 5, y: 6 });
        cpu.write_mem(0x1010, &2u32.to_le_bytes()).unwrap();
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "ord:40",
                &[0x1000]
            ),
            DispatchOutcome::JumpTo(0x123400)
        ));
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 124);
        assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 0x4e);
        let ptr = cpu.read_reg(ArmReg::R3).unwrap();
        assert_eq!(cpu.read_u32_le(ptr + 8).unwrap(), 1000);
        assert!(k.wait_deadlines.is_empty());
        assert_eq!(
            k.pending_input.pop_front(),
            Some(InputEvent::PointerUp { x: 5, y: 6 })
        );
    }

    #[test]
    fn released_stylus_never_starts_a_false_hold() {
        let (mut cpu, mut k, mut d) = setup();
        k.window_classes.insert(123, "Custom".into());
        k.window_visibility.insert(123, true);
        cpu.write_mem(
            0x1000,
            &[20u32, 123, 5, 6, 1]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "ord:40",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        ));
        assert!(k.wait_deadlines.is_empty());
        assert!(k.controls.recognized_holds.is_empty());
    }
}
