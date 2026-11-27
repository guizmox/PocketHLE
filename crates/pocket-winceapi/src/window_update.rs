//! UpdateWindow is synchronous on CE: an invalid client is painted before return.
//! Use the existing USER send/continuation owners rather than a second message pump.
use crate::guest_callbacks::{owner, service, Frame, WINCE};
use crate::CallCtx;
use pocket_kernel::{DispatchOutcome, KernelError};
pub(crate) fn update(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let owner = owner(ctx, WINCE.continue_name)?;
    let f = Frame::enter(ctx, WINCE, owner)?;
    if f.fresh {
        let hwnd = f.arg(ctx, 0)?;
        if !crate::coredll::window_exists(ctx.kernel, hwnd) {
            crate::coredll::set_thread_error(ctx, 1400);
            return f.finish(ctx, 0);
        }
        if ctx
            .kernel
            .gdi
            .pending_window_updates
            .get(&hwnd)
            .is_some_and(|v| !v.is_empty())
        {
            // Keep the update region until the actual BeginPaint/DefWindowProc
            // consumes it. Sending does not validate a handler that failed to paint.
            let target = service(ctx, "SendMessageW")?;
            return f.call_args(ctx, 1, target, &[hwnd, 15, 0, 0]);
        }
    }
    f.finish(ctx, 1)
}

#[cfg(test)]
mod tests {
    use crate::bluetooth::tests::call;
    use crate::mfc::drive_test_callbacks as drive_outcomes;
    use crate::mfc::tests::framework_setup;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::{DispatchOutcome, Thunk};
    #[test]
    fn update_window_sends_before_return_preserves_other_messages_and_skips_valid_clients() {
        let (mut cpu, mut k, mut d) = framework_setup();
        k.window_procs.insert(7, 0x6100);
        k.window_classes.insert(7, "Test".into());
        k.window_rects.insert(7, (0, 0, 100, 100));
        k.window_visibility.insert(7, true);
        crate::coredll::queue_window_paint(&mut k, 7);
        k.posted_messages.push_back((7, 0x400, 5, 6));
        let start = Thunk {
            thunk_va: k.dynamic_exports[&1]["UpdateWindow"],
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("UpdateWindow".into()),
            friendly_name: None,
        };
        for expected in [1, 0] {
            cpu.write_reg(ArmReg::R0, 7).unwrap();
            let mut calls = 0;
            assert_eq!(
                drive_outcomes(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    start.clone(),
                    128,
                    |pc, cpu, k, d| {
                        assert_eq!(pc, 0x6100);
                        calls += 1;
                        assert_eq!(cpu.read_arg(0).unwrap(), 7);
                        assert_eq!(cpu.read_arg(1).unwrap(), 15);
                        assert!(k.gdi.pending_window_updates.contains_key(&7));
                        // Real USER validation consumes the region; the API must not
                        // perform that validation before entering the guest procedure.
                        let saved_lr = cpu.read_reg(ArmReg::Lr).unwrap();
                        for (i, v) in [7, 0].into_iter().enumerate() {
                            cpu.write_arg(i as u8, v).unwrap();
                        }
                        let thunk = Thunk {
                            thunk_va: 0x70000000,
                            iat_va: 0,
                            dll: "coredll.dll".into(),
                            binding: pocket_pe::ImportBinding::Name("ValidateRect".into()),
                            friendly_name: None,
                        };
                        use pocket_kernel::Dispatcher;
                        assert_eq!(
                            d.dispatch(cpu, &thunk, k).unwrap(),
                            DispatchOutcome::ReturnedR0(1)
                        );
                        cpu.write_reg(ArmReg::Lr, saved_lr).unwrap();
                        DispatchOutcome::ReturnedR0(0xdeadbeef)
                    }
                ),
                1
            );
            assert_eq!(calls, expected);
            assert!(k.posted_messages.contains(&(7, 0x400, 5, 6)));
            assert!(!k.gdi.pending_window_updates.contains_key(&7));
        }
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "UpdateWindow",
                &[0x12345678]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "coredll.dll", "GetLastError", &[]),
            DispatchOutcome::ReturnedR0(1400)
        );
    }
}
