//! CWnd scroll helpers delegate to the virtual bar and existing WinCE owner.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2570, "?wce_ShowScrollBar@@YAHPAUHWND__@@HH@Z", show_bar),
    (
        1548,
        "?GetScrollBarCtrl@CWnd@@UBAPAVCScrollBar@@H@Z",
        no_bar,
    ),
    (1553, "?GetScrollPos@CWnd@@QBAHH@Z", get_pos),
    (1555, "?GetScrollRange@CWnd@@QBAXHPAH0@Z", get_range),
    (2405, "?SetScrollPos@CWnd@@QAAHHHH@Z", set_pos),
    (2406, "?SetScrollRange@CWnd@@QAAXHHHH@Z", set_range),
];
// The retail base virtual returns NULL; derived CView/guest implementations
// supply a shared bar through the same vtable slot (26, byte offset 0x68).
fn no_bar(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}
#[derive(Clone, Copy)]
enum Operation {
    GetPos,
    GetRange,
    SetPos,
    SetRange,
}
fn get_pos(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll(ctx, Operation::GetPos)
}
fn get_range(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll(ctx, Operation::GetRange)
}
fn set_pos(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll(ctx, Operation::SetPos)
}
fn set_range(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll(ctx, Operation::SetRange)
}
fn scroll(ctx: &mut CallCtx<'_>, op: Operation) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        // Saved register arguments 4/5 are LR/SP; a fifth ABI argument must be
        // captured from the actual incoming stack before scheduling a callback.
        if matches!(op, Operation::SetRange) {
            let redraw = ctx.arg_u32(4)?;
            f.set(ctx, 0, redraw)?;
        }
        let bar = f.arg(ctx, 1)?;
        return f.call_virtual(ctx, 1, object, 26, &[bar]);
    }
    match f.phase(ctx)? {
        1 => {
            let supplied = ctx.cpu.read_reg(ArmReg::R0)?;
            let (hwnd, bar) = if supplied == 0 {
                (word(ctx, object + 0x20)?, f.arg(ctx, 1)?)
            } else {
                (word(ctx, supplied + 0x20)?, 2)
            };
            let mut args = vec![hwnd, bar];
            let name = match op {
                Operation::GetPos => "GetScrollPos",
                Operation::GetRange => {
                    args.extend([f.arg(ctx, 2)?, f.arg(ctx, 3)?]);
                    "GetScrollRange"
                }
                Operation::SetPos => {
                    args.extend([f.arg(ctx, 2)?, f.arg(ctx, 3)?]);
                    "SetScrollPos"
                }
                Operation::SetRange => {
                    args.extend([f.arg(ctx, 2)?, f.arg(ctx, 3)?, f.get(ctx, 0)?]);
                    "SetScrollRange"
                }
            };
            f.call_service(ctx, 2, name, &args)
        }
        2 => {
            let value = ctx.cpu.read_reg(ArmReg::R0)?;
            f.finish(
                ctx,
                if matches!(op, Operation::GetRange | Operation::SetRange) {
                    0
                } else {
                    value
                },
            )
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC scroll-helper continuation".into(),
        )),
    }
}

// The CE3 helper changes GWL_STYLE, unlike Desktop ShowScrollBar. Retail
// treats zero as horizontal and every other nBar as vertical (no SB_BOTH).
fn show_bar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let hwnd = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, "GetWindowLongW", &[hwnd, (-16i32) as u32]);
    }
    match f.phase(ctx)? {
        1 => {
            let old = ctx.cpu.read_reg(ArmReg::R0)?;
            let bit = if f.arg(ctx, 1)? == 0 {
                0x00100000
            } else {
                0x00200000
            };
            let style = if f.arg(ctx, 2)? != 0 {
                old | bit
            } else {
                old & !bit
            };
            f.call_service(ctx, 2, "SetWindowLongW", &[hwnd, (-16i32) as u32, style])
        }
        2 => {
            let previous = ctx.cpu.read_reg(ArmReg::R0)?;
            f.finish(ctx, u32::from(previous != 0))
        }
        _ => Err(KernelError::Dispatch(
            "Invalid CE scroll-style continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::archive::tests::Fixture;
    use super::super::window_dispatch::tests::{drive_with, entry};
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn ce_scroll_style_helper_uses_retail_masks_and_the_shared_window_style() {
        let mut f = Fixture::new(&[]);
        let h = f.kernel.controls.create(
            7,
            pocket_kernel::controls::ControlClass::Edit,
            12,
            "".into(),
            0x50000004,
            0,
            0,
            80,
            20,
        );
        for (bar, show, expected) in [
            (0, 1, 0x50100004),
            (1, 1, 0x50300004),
            (0, 0, 0x50200004),
            (3, 0, 0x50000004),
        ] {
            assert_eq!(f.call("ord:2570", &[h, bar, show]), 1);
            assert_eq!(f.kernel.controls.get(h).unwrap().style, expected);
        }
    }
    #[test]
    fn ranges_positions_and_owned_bar_virtuals_share_user_state_and_release_frames() {
        let mut f = Fixture::new(&[]);
        f.call("ord:206", &[0x2400]);
        f.cpu
            .write_mem(0x2420, &0xdead0001u32.to_le_bytes())
            .unwrap();
        f.kernel.window_styles.insert(0xdead0001, (0x00300000, 0));
        // Initialize the per-thread callback root before measuring repeated use.
        f.call("ord:2406", &[0x2400, 1, (-20i32) as u32, 90, 1]);
        let baseline = f.kernel.heap.free_bytes();
        for _ in 0..128 {
            f.call("ord:2405", &[0x2400, 1, 30, 1]);
            assert_eq!(f.call("ord:1553", &[0x2400, 1]), 30);
            f.call("ord:1555", &[0x2400, 1, 0x2800, 0x2804]);
            assert_eq!(f.word(0x2800), (-20i32) as u32);
            assert_eq!(f.word(0x2804), 90);
            assert_eq!(f.kernel.heap.free_bytes(), baseline);
        }
        assert_eq!(f.kernel.scroll_info[&(0xdead0001, 1)][3], 30);
    }
    fn checked_range(
        ctx: &mut crate::CallCtx<'_>,
    ) -> Result<pocket_kernel::DispatchOutcome, pocket_kernel::KernelError> {
        for (i, value) in [0xdead1234, 2, 5, 80, 0xabcdef].into_iter().enumerate() {
            assert_eq!(ctx.arg_u32(i as u8)?, value);
        }
        Ok(pocket_kernel::DispatchOutcome::ReturnedR0(1))
    }
    #[test]
    fn derived_bar_and_fifth_argument_survive_a_checkpoint_and_clobbered_caller_stack() {
        let mut f = Fixture::new(&[]);
        f.cpu.write_mem(0x2400, &0x2800u32.to_le_bytes()).unwrap();
        f.cpu
            .write_mem(0x2800 + 26 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        f.cpu
            .write_mem(0x3020, &0xdead1234u32.to_le_bytes())
            .unwrap();
        for (i, value) in [0x2400, 1, 5, 80].into_iter().enumerate() {
            f.cpu.write_arg(i as u8, value).unwrap();
        }
        f.cpu.write_mem(0x4000, &0xabcdefu32.to_le_bytes()).unwrap();
        f.dispatcher
            .register_handler("coredll.dll", "SetScrollRange", checked_range);
        let baseline = f.kernel.heap.free_bytes();
        let mut calls = 0;
        let directory = f.directory.path().to_path_buf();
        let result = drive_with(
            &mut f.cpu,
            &mut f.kernel,
            &mut f.dispatcher,
            entry(2406),
            128,
            |pc, cpu, kernel, _| {
                calls += 1;
                assert_eq!(pc, 0x6000);
                assert_eq!(cpu.read_arg(0).unwrap(), 0x2400);
                assert_eq!(cpu.read_arg(1).unwrap(), 1);
                let saved = kernel.capture_snapshot().unwrap();
                let (_, mut restored, _) = super::super::tests::framework_setup();
                restored.vfs.mount("\\", &directory);
                saved.restore(&mut restored).unwrap();
                *kernel = restored;
                cpu.write_mem(0x4000, &0xbad0u32.to_le_bytes()).unwrap();
                0x3000
            },
        );
        assert_eq!(result, 0);
        assert_eq!(calls, 1);
        assert_eq!(f.cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
    }
}
