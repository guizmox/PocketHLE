//! UpdateWindow is synchronous on CE: a visible invalid client paints before return.
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
        // CoPilot's mouse.dll hides its new child, calls UpdateWindow, then
        // installs GWL_USERDATA. Paint/erase must not enter that hidden child
        // (or a visible child of a hidden parent) before initialization.
        // Keep its invalid region for the next show; explicit SendMessage
        // painting retains its ordinary synchronous semantics.
        if !crate::coredll::window_is_visible(ctx.kernel, hwnd) {
            return f.finish(ctx, 1);
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
    #[test]
    fn hidden_child_update_waits_for_userdata_and_visible_parent_without_losing_damage() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (hwnd, parent, style) in [(7u32, 0u32, 0x10000000u32), (8, 7, 0x40000000)] {
            k.window_classes.insert(hwnd, "Test".into());
            k.window_procs.insert(hwnd, 0x6100);
            k.window_rects.insert(hwnd, (0, 0, 100, 100));
            k.window_styles.insert(hwnd, (style, 0));
            k.window_visibility.insert(hwnd, style & 0x10000000 != 0);
            if parent != 0 {
                k.window_parents.insert(hwnd, parent);
            }
        }
        crate::coredll::queue_window_paint(&mut k, 8);
        k.gdi.pending_window_erases.insert(8);
        k.posted_messages.push_back((7, 0x400, 5, 6));
        let damage = k.gdi.pending_window_updates[&8].clone();
        let available = k.heap.free_bytes();
        for (parent_visible, child_visible) in [(true, false), (false, true)] {
            for (hwnd, visible) in [(7, parent_visible), (8, child_visible)] {
                k.window_visibility.insert(hwnd, visible);
                let style = &mut k.window_styles.get_mut(&hwnd).unwrap().0;
                *style = (*style & !0x10000000) | if visible { 0x10000000 } else { 0 };
            }
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "UpdateWindow",
                    &[8]
                ),
                DispatchOutcome::ReturnedR0(1)
            );
            assert_eq!(k.gdi.pending_window_updates[&8], damage);
            assert!(k.gdi.pending_window_erases.contains(&8));
            assert_eq!(k.heap.free_bytes(), available);
        }
        // The real constructor stores its instance only after UpdateWindow.
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SetWindowLongW",
            &[8, (-21i32) as u32, 0x2000],
        );
        k.window_visibility.insert(7, true);
        k.window_styles.get_mut(&7).unwrap().0 |= 0x10000000;
        cpu.write_arg(0, 8).unwrap();
        let start = Thunk {
            thunk_va: k.dynamic_exports[&1]["UpdateWindow"],
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("UpdateWindow".into()),
            friendly_name: None,
        };
        let mut paints = 0;
        assert_eq!(
            drive_outcomes(&mut cpu, &mut k, &mut d, start, 128, |pc, cpu, k, d| {
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_arg(0).unwrap(), 8);
                assert_eq!(cpu.read_arg(1).unwrap(), 15);
                assert_eq!(k.window_userdata[&8], 0x2000);
                paints += 1;
                let saved_lr = cpu.read_reg(ArmReg::Lr).unwrap();
                cpu.write_arg(0, 8).unwrap();
                cpu.write_arg(1, 0).unwrap();
                let thunk = Thunk {
                    thunk_va: 0x70000000,
                    iat_va: 0,
                    dll: "coredll.dll".into(),
                    binding: pocket_pe::ImportBinding::Name("ValidateRect".into()),
                    friendly_name: None,
                };
                use pocket_kernel::Dispatcher;
                d.dispatch(cpu, &thunk, k).unwrap();
                cpu.write_reg(ArmReg::Lr, saved_lr).unwrap();
                DispatchOutcome::ReturnedR0(0)
            }),
            1
        );
        assert_eq!(paints, 1);
        assert!(k.posted_messages.contains(&(7, 0x400, 5, 6)));
        assert_eq!(k.heap.free_bytes(), available);
    }
}
