//! Apply an explicit game FPS ceiling at a completed native window repaint.
//!
//! A self-invalidating WM_PAINT loop (V-Rally) has no GX/EGL swap or Sleep to
//! pay the ceiling. Pace the paired transaction, not its individual drawing
//! calls. Arbitrary GDI repaint transactions remain image updates, not FPS.
use super::*;
use pocket_kernel::gdi::{DcSurface, PaintPresentation};

fn bitmap_revision(kernel: &KernelState, dc: u32) -> Option<(u32, u64)> {
    let handle = kernel.gdi.dc(dc)?.selected_bitmap?;
    Some((handle, kernel.gdi.bitmap(handle)?.paint_generation))
}

pub(super) fn begin(ctx: &mut CallCtx<'_>, hwnd: u32, dc: u32) {
    let key = (ctx.kernel.current_thread, hwnd);
    if !ctx
        .kernel
        .framebuffer
        .directdraw
        .frame_rate_limit
        .is_some_and(|fps| fps > 0)
    {
        ctx.kernel.gdi.paint_presentations.remove(&key);
        return;
    }
    let stamp = PaintPresentation {
        dc,
        bitmap_revision: bitmap_revision(ctx.kernel, dc),
        framebuffer_revision: ctx.kernel.framebuffer.frame_counter,
        completed_presentations: ctx.kernel.framebuffer.presentation_stats.completed(),
    };
    ctx.kernel.gdi.paint_presentations.insert(key, stamp);
}

fn changed_visible_destination(kernel: &KernelState, hwnd: u32, stamp: PaintPresentation) -> bool {
    if !window_is_visible(kernel, hwnd) {
        return false;
    }
    let Some(dc) = kernel.gdi.dc(stamp.dc) else {
        return false;
    };
    let (changed, width, height) = match dc.surface {
        DcSurface::Screen => (
            kernel.framebuffer.frame_counter != stamp.framebuffer_revision,
            kernel.framebuffer.width,
            kernel.framebuffer.height,
        ),
        DcSurface::Memory => {
            // A memory DC alone is not a display boundary. Only this window's
            // backing store will be composited by the matching EndPaint.
            if kernel.window_dcs.get(&hwnd) != Some(&stamp.dc) {
                return false;
            }
            let Some(bitmap) = dc.selected_bitmap.and_then(|h| kernel.gdi.bitmap(h)) else {
                return false;
            };
            (
                bitmap_revision(kernel, stamp.dc) != stamp.bitmap_revision
                    && bitmap.paint_generation != bitmap.published_generation,
                bitmap.width,
                bitmap.height,
            )
        }
    };
    let Some((x, y)) = kernel.window_client_origin(hwnd) else {
        return false;
    };
    changed
        && width > 0
        && height > 0
        && i64::from(x) < i64::from(kernel.framebuffer.width)
        && i64::from(y) < i64::from(kernel.framebuffer.height)
        && i64::from(x) + i64::from(width) > 0
        && i64::from(y) + i64::from(height) > 0
}

fn clear_pending_wait(ctx: &mut CallCtx<'_>) -> Result<(), KernelError> {
    if !ctx.kernel.framebuffer.directdraw.pending.is_empty() {
        let key = (
            ctx.kernel.current_thread,
            ctx.thunk.thunk_va,
            ctx.cpu.read_reg(ArmReg::Sp)?,
        );
        if ctx
            .kernel
            .framebuffer
            .directdraw
            .pending
            .remove(&key)
            .is_some()
        {
            ctx.kernel.wait_deadlines.remove(&key);
        }
    }
    Ok(())
}

pub(super) fn finish(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let key = (ctx.kernel.current_thread, hwnd);
    let Some(stamp) = ctx.kernel.gdi.paint_presentations.get(&key).copied() else {
        // DestroyWindow can remove a paint while this thunk is parked.
        clear_pending_wait(ctx)?;
        return Ok(None);
    };
    let capped = ctx
        .kernel
        .framebuffer
        .directdraw
        .frame_rate_limit
        .is_some_and(|fps| fps > 0);
    let already_presented =
        ctx.kernel.framebuffer.presentation_stats.completed() != stamp.completed_presentations;
    if capped && !already_presented && changed_visible_destination(ctx.kernel, hwnd, stamp) {
        if let Some(outcome) = crate::ddraw::wait_display(ctx, None)? {
            // A cooperative retry must retain both the paint anchor and the
            // existing display deadline. Neither publication nor a new paint
            // has completed yet.
            return Ok(Some(outcome));
        }
        ctx.kernel.framebuffer.directdraw.last_present = Some(std::time::Instant::now());
    }
    // Another thread may have completed a real swap while this paint waited,
    // or hidden/destroyed its window. Cancel only this EndPaint's old wait.
    clear_pending_wait(ctx)?;
    ctx.kernel.gdi.paint_presentations.remove(&key);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::{stub::StubCpu, Cpu, Prot};
    use pocket_kernel::{gdi::GdiState, Thunk};
    use pocket_pe::ImportBinding;
    use std::time::{Duration, Instant};

    fn fixture(screen_dc: bool) -> (StubCpu, KernelState, Thunk) {
        let mut cpu = StubCpu::new();
        cpu.map_region(0x1000, 0x4000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.write_reg(ArmReg::Sp, 0x4000).unwrap();
        cpu.write_reg(ArmReg::R0, FAKE_HWND).unwrap();
        cpu.write_reg(ArmReg::R1, 0x1100).unwrap();
        let mut kernel = crate::gx::tests::fresh_kernel();
        kernel.fb_mapped = true; // Keep unrelated shell repainting out of this transaction.
        kernel.window_rects.insert(FAKE_HWND, (0, 0, 240, 320));
        kernel.window_styles.insert(FAKE_HWND, (0x1000_0000, 0));
        kernel.window_visibility.insert(FAKE_HWND, true);
        let dc = if screen_dc {
            GDI_SCREEN_DC
        } else {
            let dc = kernel.gdi.create_memory_dc();
            let bitmap = kernel.gdi.create_compatible_bitmap(240, 320);
            kernel.gdi.bitmap_mut(bitmap).unwrap().painted = Some(vec![0; 240 * 320]);
            kernel.gdi.dc_mut(dc).unwrap().selected_bitmap = Some(bitmap);
            dc
        };
        kernel.window_dcs.insert(FAKE_HWND, dc);
        kernel.framebuffer.directdraw.frame_rate_limit = Some(20);
        (
            cpu,
            kernel,
            Thunk {
                thunk_va: 0x7000_0000,
                iat_va: 0,
                dll: "coredll.dll".into(),
                binding: ImportBinding::Name("EndPaint".into()),
                friendly_name: None,
            },
        )
    }

    fn paint(ctx: &mut CallCtx<'_>, screen_dc: bool) {
        if screen_dc {
            ctx.kernel.framebuffer.mark_dirty();
        } else {
            let dc = ctx.kernel.window_dcs[&FAKE_HWND];
            let bitmap = ctx.kernel.gdi.dc(dc).unwrap().selected_bitmap.unwrap();
            let bitmap = ctx.kernel.gdi.bitmap_mut(bitmap).unwrap();
            bitmap.pixels[..2].copy_from_slice(&0xf800u16.to_le_bytes());
            bitmap.painted.as_mut().unwrap()[0] = 1;
            bitmap.mark_painted();
        }
    }

    #[test]
    fn native_paint_cap_preserves_deadline_and_transaction_across_retries() {
        for screen_dc in [false, true] {
            let (mut cpu, mut kernel, thunk) = fixture(screen_dc);
            let anchor = Instant::now() + Duration::from_secs(60);
            kernel.framebuffer.directdraw.last_present = Some(anchor);
            let mut ctx = CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            };
            begin_paint(&mut ctx).unwrap();
            paint(&mut ctx, screen_dc);
            let before = ctx.kernel.framebuffer.frame_counter;
            let wait_key = (0, thunk.thunk_va, 0x4000);
            for _ in 0..2 {
                assert_eq!(
                    end_paint(&mut ctx).unwrap(),
                    DispatchOutcome::JumpTo(thunk.thunk_va)
                );
                assert_eq!(
                    ctx.kernel.framebuffer.directdraw.pending[&wait_key],
                    anchor + Duration::from_millis(50)
                );
                assert!(ctx
                    .kernel
                    .gdi
                    .paint_presentations
                    .contains_key(&(0, FAKE_HWND)));
                assert_eq!(ctx.kernel.framebuffer.frame_counter, before);
                assert_eq!(ctx.kernel.framebuffer.presentation_stats.completed(), 0);
            }
            // Complete the same cooperative wait without a wall-clock sleep.
            ctx.kernel
                .framebuffer
                .directdraw
                .pending
                .insert(wait_key, Instant::now());
            assert_eq!(end_paint(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
            assert!(ctx.kernel.gdi.paint_presentations.is_empty());
            assert!(ctx.kernel.framebuffer.directdraw.pending.is_empty());
            assert_eq!(ctx.kernel.framebuffer.presentation_stats.completed(), 0);
            let last = ctx.kernel.framebuffer.directdraw.last_present;
            assert_eq!(end_paint(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
            assert_eq!(ctx.kernel.framebuffer.directdraw.last_present, last);
        }
    }

    #[test]
    fn native_paint_empty_hidden_offscreen_and_memory_only_updates_do_not_wait() {
        for case in 0..5 {
            let (mut cpu, mut kernel, thunk) = fixture(false);
            let anchor = Instant::now() + Duration::from_secs(60);
            kernel.framebuffer.directdraw.last_present = Some(anchor);
            let mut ctx = CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            };
            begin_paint(&mut ctx).unwrap();
            match case {
                0 => {} // A matched but empty repaint is not a frame.
                1 => {
                    paint(&mut ctx, false);
                    ctx.kernel.window_visibility.insert(FAKE_HWND, false);
                }
                2 => {
                    paint(&mut ctx, false);
                    ctx.kernel
                        .window_rects
                        .insert(FAKE_HWND, (500, 500, 240, 320));
                }
                3 => {
                    let bitmap = ctx.kernel.gdi.create_compatible_bitmap(4, 4);
                    ctx.kernel.gdi.bitmap_mut(bitmap).unwrap().mark_painted();
                }
                _ => {
                    // Selecting a new empty backing store is not a drawing
                    // transaction and the compositor has nothing to publish.
                    let bitmap = ctx.kernel.gdi.create_compatible_bitmap(240, 320);
                    let dc = ctx.kernel.window_dcs[&FAKE_HWND];
                    ctx.kernel.gdi.select_into(dc, bitmap);
                }
            }
            assert_eq!(end_paint(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
            assert!(ctx.kernel.gdi.paint_presentations.is_empty());
            assert!(ctx.kernel.framebuffer.directdraw.pending.is_empty());
            assert_eq!(ctx.kernel.framebuffer.directdraw.last_present, Some(anchor));
        }
    }

    #[test]
    fn native_paint_does_not_add_pacing_without_an_explicit_cap_or_after_a_swap() {
        for cap in [None, Some(0), Some(20)] {
            let (mut cpu, mut kernel, thunk) = fixture(true);
            kernel.framebuffer.directdraw.frame_rate_limit = cap;
            let anchor = Instant::now() + Duration::from_secs(60);
            kernel.framebuffer.directdraw.last_present = Some(anchor);
            let mut ctx = CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            };
            begin_paint(&mut ctx).unwrap();
            paint(&mut ctx, true);
            if cap == Some(20) {
                // A real GX/EGL/DirectDraw presentation already paid its wait.
                ctx.kernel.framebuffer.presentation_stats.record_present();
            }
            assert_eq!(end_paint(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
            assert!(ctx.kernel.framebuffer.directdraw.pending.is_empty());
            assert!(ctx.kernel.gdi.paint_presentations.is_empty());
            assert_eq!(ctx.kernel.framebuffer.directdraw.last_present, Some(anchor));
            assert_eq!(
                ctx.kernel.framebuffer.presentation_stats.completed(),
                u64::from(cap == Some(20))
            );
        }
    }

    #[test]
    fn native_paint_wait_is_cancelled_when_another_swap_completes_or_window_is_destroyed() {
        for destroyed in [false, true] {
            let (mut cpu, mut kernel, thunk) = fixture(true);
            kernel.framebuffer.directdraw.last_present =
                Some(Instant::now() + Duration::from_secs(60));
            let mut ctx = CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            };
            begin_paint(&mut ctx).unwrap();
            paint(&mut ctx, true);
            assert_eq!(
                end_paint(&mut ctx).unwrap(),
                DispatchOutcome::JumpTo(thunk.thunk_va)
            );
            assert!(!ctx.kernel.framebuffer.directdraw.pending.is_empty());
            let wait_key = (0, thunk.thunk_va, 0x4000);
            ctx.kernel.wait_deadlines.insert(wait_key, 1);
            if destroyed {
                ctx.kernel.gdi.paint_presentations.remove(&(0, FAKE_HWND));
            } else {
                ctx.kernel.framebuffer.presentation_stats.record_present();
            }
            assert_eq!(end_paint(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(1));
            assert!(ctx.kernel.gdi.paint_presentations.is_empty());
            assert!(ctx.kernel.framebuffer.directdraw.pending.is_empty());
            assert!(!ctx.kernel.wait_deadlines.contains_key(&wait_key));
        }
    }

    #[test]
    fn native_paint_host_pacing_anchors_are_not_snapshot_state() {
        let mut gdi = GdiState::new();
        gdi.paint_presentations.insert(
            (0, FAKE_HWND),
            PaintPresentation {
                dc: GDI_SCREEN_DC,
                bitmap_revision: None,
                framebuffer_revision: 12,
                completed_presentations: 4,
            },
        );
        let saved = serde_json::to_vec(&gdi).unwrap();
        let restored: GdiState = serde_json::from_slice(&saved).unwrap();
        assert!(restored.paint_presentations.is_empty());
        assert!(gdi
            .capture_snapshot()
            .unwrap()
            .restore()
            .unwrap()
            .paint_presentations
            .is_empty());
    }
}
