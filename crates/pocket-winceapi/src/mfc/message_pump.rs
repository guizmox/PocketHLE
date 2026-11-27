//! Native MFC pump control flow delegates all queue/wait/dispatch work to WinCE.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
const SAVE_PROFILE: &str = "__PocketHLE_MFC_SaveStdProfileSettings";
const RUN: &str = "__PocketHLE_MFC_ThreadRun";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2234, "?Run@CWinApp@@UAAHXZ", app_run),
    (2235, "?Run@CWinThread@@UAAHXZ", run),
    (2117, "?PumpMessage@CWinThread@@UAAHXZ", pump),
    (
        1698,
        "?IsIdleMessage@CWinThread@@UAAHPAUtagMSG@@@Z",
        is_idle,
    ),
    (1146, "?ExitInstance@CWinApp@@UAAHXZ", app_exit),
    (
        2244,
        "?SaveStdProfileSettings@CWinApp@@IAAXXZ",
        save_profile,
    ),
    (1147, "?ExitInstance@CWinThread@@UAAHXZ", thread_exit),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", RUN, run);
    d.register_handler("coredll.dll", SAVE_PROFILE, save_profile);
}
fn app_run(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let app = frame.arg(ctx, 0)?;
    if frame.fresh {
        if word(ctx, app + 0x20)? == 0 {
            return frame.call_service(ctx, 1, "PostQuitMessage", &[0]);
        }
        return frame.call_service(ctx, 2, RUN, &[app]);
    }
    if frame.phase(ctx)? == 1 {
        return frame.call_service(ctx, 2, RUN, &[app]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    frame.finish(ctx, result)
}
fn peek(ctx: &mut CallCtx<'_>, frame: &Frame, phase: u32) -> Result<DispatchOutcome, KernelError> {
    let object = frame.arg(ctx, 0)?;
    frame.call_service(ctx, phase, "PeekMessageW", &[object + 0x34, 0, 0, 0, 0])
}
fn pump_virtual(ctx: &mut CallCtx<'_>, frame: &Frame) -> Result<DispatchOutcome, KernelError> {
    let object = frame.arg(ctx, 0)?;
    let target = virtual_function(ctx, object, 24)?;
    frame.call_args(ctx, 3, target, &[object])
}
fn run(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        frame.set(ctx, 0, 1)?;
        return peek(ctx, &frame, 1);
    }
    match frame.phase(ctx)? {
        1 if result == 0 && frame.get(ctx, 0)? != 0 => {
            let count = frame.get(ctx, 1)?;
            frame.set(ctx, 1, count.wrapping_add(1))?;
            let target = virtual_function(ctx, object, 25)?;
            frame.call_args(ctx, 2, target, &[object, count])
        }
        1 => pump_virtual(ctx, &frame),
        2 => {
            if result == 0 {
                frame.set(ctx, 0, 0)?;
                pump_virtual(ctx, &frame)
            } else {
                peek(ctx, &frame, 1)
            }
        }
        3 if result == 0 => {
            let target = virtual_function(ctx, object, 27)?;
            frame.call_args(ctx, 6, target, &[object])
        }
        3 => {
            let target = virtual_function(ctx, object, 26)?;
            frame.call_args(ctx, 4, target, &[object, object + 0x34])
        }
        4 => {
            if result != 0 {
                frame.set(ctx, 0, 1)?;
                frame.set(ctx, 1, 0)?;
            }
            peek(ctx, &frame, 5)
        }
        5 if result != 0 || frame.get(ctx, 0)? == 0 => pump_virtual(ctx, &frame),
        5 => peek(ctx, &frame, 1),
        6 => frame.finish(ctx, result),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC run-loop continuation".into(),
        )),
    }
}
fn pump(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let message = object + 0x34;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        return frame.call_service(ctx, 1, "GetMessageW", &[message, 0, 0, 0]);
    }
    match frame.phase(ctx)? {
        1 if result == 0 => frame.finish(ctx, 0),
        1 if word(ctx, message + 4)? == 0x36a => frame.finish(ctx, 1),
        1 => {
            let target = virtual_function(ctx, object, 23)?;
            frame.call_args(ctx, 2, target, &[object, message])
        }
        2 if result != 0 => frame.finish(ctx, 1),
        2 => frame.call_service(ctx, 3, "TranslateMessage", &[message]),
        3 => frame.call_service(ctx, 4, "DispatchMessageW", &[message]),
        4 => frame.finish(ctx, 1),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC message-pump continuation".into(),
        )),
    }
}
fn is_idle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let message = ctx.arg_u32(1)?;
    let kind = word(ctx, message + 4)?;
    if matches!(kind, 0xf | 0x118) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if kind == 0x200 {
        let point = ctx.cpu.read_mem(message + 20, 8)?;
        if ctx.cpu.read_mem(object + 0x5c, 8)? == point && word(ctx, object + 0x64)? == kind {
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        ctx.cpu.write_mem(object + 0x5c, &point)?;
        put(ctx, object + 0x64, kind)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn app_exit(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let app = f.arg(ctx, 0)?;
    if f.fresh {
        let command = word(ctx, app + 0xa8)?;
        let module = super::application::state(ctx)?;
        if (command == 0 || word(ctx, command + 0x10)? != 5)
            && ctx.cpu.read_mem(module + 0x14, 1)?[0] == 0
        {
            return f.call_service(ctx, 1, SAVE_PROFILE, &[app]);
        }
    }
    let code = word(ctx, app + 0x3c)?;
    f.finish(ctx, code)
}
fn save_profile(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let app = f.arg(ctx, 0)?;
        let recent = word(ctx, app + 0xa4)?;
        if recent != 0 {
            let target = virtual_function(ctx, recent, 4)?;
            return f.call_args(ctx, 1, target, &[recent]);
        }
    }
    f.finish(ctx, 0)
}

fn thread_exit(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(word(ctx, object + 0x3c)?))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn run_rearms_idle_after_real_virtual_message_classification_and_returns_exit_result() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:201",
            &[0x2000, 0],
        );
        let table = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(table, 39 * 4).unwrap();
        cpu.write_mem(0x2800, &bytes).unwrap();
        for (slot, target) in [(25, 0x6000u32), (24, 0x6100), (26, 0x6200), (27, 0x6300)] {
            cpu.write_mem(0x2800 + slot * 4, &target.to_le_bytes())
                .unwrap();
        }
        cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
        cpu.write_arg(0, 0x2000).unwrap();
        let available = k.heap.free_bytes();
        // This unit drives the thread virtuals, so it does not dispatch the
        // synthetic paint traffic. Schedule that unrelated traffic past the test.
        k.synthetic_paint_next_ms = u64::MAX;
        let mut calls = Vec::new();
        let mut pumped = 0;
        let value = drive(&mut cpu, &mut k, &mut d, entry(2235), |pc, cpu| {
            calls.push(pc);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
            match pc {
                0x6000 => {
                    assert_eq!(cpu.read_arg(1).unwrap(), 0);
                    0
                }
                0x6100 => {
                    pumped += 1;
                    u32::from(pumped == 1)
                }
                0x6200 => {
                    assert_eq!(cpu.read_arg(1).unwrap(), 0x2034);
                    1
                }
                0x6300 => 77,
                _ => panic!("Unexpected pump callback {pc:x}"),
            }
        });
        assert_eq!(value, 77);
        assert_eq!(calls, [0x6000, 0x6100, 0x6200, 0x6000, 0x6100, 0x6300]);
        assert_eq!(k.heap.free_bytes(), available);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
    #[test]
    fn idle_filter_ignores_paints_timers_and_repeated_mouse_points_without_losing_changes() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (message, x, expected) in [
            (0xfu32, 0u32, 0u32),
            (0x118, 0, 0),
            (0x200, 5, 1),
            (0x200, 5, 0),
            (0x200, 6, 1),
            (0x100, 6, 1),
        ] {
            cpu.write_mem(0x1804, &message.to_le_bytes()).unwrap();
            cpu.write_mem(0x1814, &x.to_le_bytes()).unwrap();
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:1698",
                    &[0x2000, 0x1800]
                )),
                expected
            );
        }
        cpu.write_mem(0x203c, &91u32.to_le_bytes()).unwrap();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1147",
                &[0x2000]
            )),
            91
        );
    }
}

#[cfg(test)]
mod app_exit_tests {
    use super::super::archive::tests::Fixture;
    use super::super::window_dispatch::tests::{drive_with, entry};
    use pocket_cpu::Cpu;
    #[test]
    fn application_exit_persists_the_real_recent_list_only_for_the_native_command_and_module_policy(
    ) {
        let mut f = Fixture::new(&[]);
        f.call("ord:201", &[0x2400, 0]);
        let module = f.call("ord:644", &[]);
        for (at, value) in [
            (0x24a8, 0x2a00u32),
            (0x24a4, 0x2c00),
            (0x2c00, 0x2d00),
            (0x2d10, 0x6100),
        ] {
            f.cpu.write_mem(at, &value.to_le_bytes()).unwrap();
        }
        let mut baseline = None;
        for _ in 0..32 {
            for (command, dll) in [(0u32, false), (5, false), (0, true), (5, true)] {
                f.cpu.write_mem(0x2a10, &command.to_le_bytes()).unwrap();
                f.cpu.write_mem(module + 0x14, &[u8::from(dll)]).unwrap();
                f.cpu.write_mem(0x243c, &37u32.to_le_bytes()).unwrap();
                f.cpu.write_arg(0, 0x2400).unwrap();
                let mut calls = 0;
                let result = drive_with(
                    &mut f.cpu,
                    &mut f.kernel,
                    &mut f.dispatcher,
                    entry(1146),
                    256,
                    |pc, cpu, _, _| {
                        assert_eq!(pc, 0x6100);
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2c00);
                        cpu.write_mem(0x243c, &41u32.to_le_bytes()).unwrap();
                        calls += 1;
                        0
                    },
                );
                let save = !dll && command != 5;
                assert_eq!(calls, u32::from(save));
                assert_eq!(result, if save { 41 } else { 37 });
                let free = f.kernel.heap.free_bytes();
                assert_eq!(*baseline.get_or_insert(free), free);
            }
        }
    }
}
