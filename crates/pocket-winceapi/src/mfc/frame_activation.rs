//! Default frame activation keeps shell state, frame flags and view notifications.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
const SHELL: &str = "__PocketHLE_MFC_ShellActivate";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] =
    &[(1788, "?OnActivate@CFrameWnd@@IAAXIPAVCWnd@@H@Z", activate)];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", SHELL, crate::aygshell::activation::handle);
}
fn get_style(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    f.call_service(ctx, 3, super::activation::STYLE, &[object])
}
fn default(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    f.call_service(ctx, 11, "__PocketHLE_MFC_WindowDefault", &[object])
}
fn view_frame(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let state = f.arg(ctx, 1)?;
    let view = f.get(ctx, 2)?;
    let target = virtual_function(ctx, view, 52)?;
    f.call_args(ctx, 10, target, &[view, state, object])
}
fn notify_view(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    view: u32,
) -> Result<DispatchOutcome, KernelError> {
    if view == 0 {
        return default(ctx, f);
    }
    f.set(ctx, 2, view)?;
    if f.arg(ctx, 1)? != 0 && f.arg(ctx, 3)? == 0 {
        let target = virtual_function(ctx, view, 51)?;
        f.call_args(ctx, 9, target, &[view, 1, view, view])
    } else {
        view_frame(ctx, f)
    }
}
fn set_flags(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    active: bool,
) -> Result<DispatchOutcome, KernelError> {
    let root = f.get(ctx, 0)?;
    let flags = word(ctx, root + 0x2c)?;
    put(
        ctx,
        root + 0x2c,
        (flags & !0x20) | if active { 0x20 } else { 0 },
    )?;
    let object = f.arg(ctx, 0)?;
    let view = word(ctx, object + 0xac)?;
    if view != 0 {
        return notify_view(ctx, f, view);
    }
    let target = virtual_function(ctx, object, 48)?;
    f.call_args(ctx, 8, target, &[object])
}
fn target_frame(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    root: u32,
) -> Result<DispatchOutcome, KernelError> {
    if root == 0 {
        return Err(KernelError::Dispatch(
            "MFC activation has no top-level frame".into(),
        ));
    }
    f.set(ctx, 0, root)?;
    let object = f.arg(ctx, 0)?;
    let other = if f.arg(ctx, 1)? != 0 {
        object
    } else {
        f.arg(ctx, 2)?
    };
    f.set(ctx, 1, other)?;
    if root == other {
        return set_flags(ctx, f, true);
    }
    f.call_service(ctx, 5, super::hierarchy::TOP, &[other])
}
fn activate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        if word(ctx, object + 0xc8)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC automatic Done-button management is not implemented".into(),
            ));
        }
        return f.call_service(ctx, 1, super::hierarchy::TOP, &[object]);
    }
    match f.phase(ctx)? {
        1 if value == object => {
            let thread = super::application::current_thread_state(ctx)?;
            let wp = word(ctx, thread + 0x44)?;
            let lp = word(ctx, thread + 0x48)?;
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 2, SHELL, &[hwnd, wp, lp, object + 0xcc, 0])
        }
        1 | 2 => get_style(ctx, &f),
        3 if value & 0x40000000 != 0 => target_frame(ctx, &f, object),
        3 => f.call_service(ctx, 4, super::hierarchy::TOP, &[object]),
        4 => target_frame(ctx, &f, value),
        5 => {
            if value != f.get(ctx, 0)? {
                return set_flags(ctx, &f, false);
            }
            let other = f.get(ctx, 1)?;
            let hwnd = word(ctx, other + 0x20)?;
            f.call_service(ctx, 6, "SendMessageW", &[hwnd, 0x36d, 0x40, 0])
        }
        6 => set_flags(ctx, &f, value != 0),
        8 => {
            if value == 0 {
                return Err(KernelError::Dispatch(
                    "MFC GetActiveFrame returned NULL".into(),
                ));
            }
            let view = word(ctx, value + 0xac)?;
            notify_view(ctx, &f, view)
        }
        9 => view_frame(ctx, &f),
        10 => default(ctx, &f),
        11 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame activation continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn frame_activation_calls_view_hooks_in_order_and_restores_default_message() {
        for (state, minimized, expected) in [
            (1u32, 0u32, vec![0x6000, 0x6100, 0x6200]),
            (0, 0, vec![0x6100, 0x6200]),
            (1, 1, vec![0x6100, 0x6200]),
        ] {
            let (mut cpu, mut k, mut d) = framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:103",
                &[0x2000],
            );
            let thread = result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:651",
                &[],
            ));
            let original = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(original, 58 * 4).unwrap();
            cpu.write_mem(0x2600, &bytes).unwrap();
            for (at, value) in [
                (0x2000, 0x2600u32),
                (0x2020, 0xdead1001),
                (0x20ac, 0x2400),
                (0x2400, 0x2800),
                (0x28cc, 0x6000),
                (0x28d0, 0x6100),
                (0x26a0, 0x6200),
                (thread + 0x40, 6),
                (thread + 0x44, state | (minimized << 16)),
                (thread + 0x48, 0),
            ] {
                cpu.write_mem(at, &value.to_le_bytes()).unwrap();
            }
            k.window_classes.insert(0xdead1001, "Frame".into());
            k.window_styles.insert(0xdead1001, (0x90000000, 0));
            k.window_visibility.insert(0xdead1001, true);
            k.dynamic_exports
                .get_mut(&1)
                .unwrap()
                .insert("__PocketHLE_MFC_Window:dead1001".into(), 0x2000);
            let free = k.heap.free_bytes();
            for (i, value) in [0x2000, state, 0, minimized].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            let mut calls = Vec::new();
            drive(&mut cpu, &mut k, &mut d, entry(1788), |pc, cpu| {
                calls.push(pc);
                match pc {
                    0x6000 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2400);
                        assert_eq!(cpu.read_arg(1).unwrap(), 1);
                        assert_eq!(cpu.read_arg(2).unwrap(), 0x2400);
                        assert_eq!(cpu.read_arg(3).unwrap(), 0x2400);
                    }
                    0x6100 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2400);
                        assert_eq!(cpu.read_arg(1).unwrap(), state);
                        assert_eq!(cpu.read_arg(2).unwrap(), 0x2000);
                    }
                    0x6200 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                        assert_eq!(cpu.read_arg(1).unwrap(), 6);
                        assert_eq!(cpu.read_arg(2).unwrap(), state | (minimized << 16));
                    }
                    _ => panic!("unexpected {pc:x}"),
                }
                0
            });
            assert_eq!(calls, expected);
            assert_eq!(
                cpu.read_u32_le(0x202c).unwrap() & 0x20,
                if state == 0 { 0 } else { 0x20 }
            );
            assert_eq!(k.heap.free_bytes(), free);
        }
    }
}
