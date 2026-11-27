//! Nonlocal returns retire abandoned continuations through their existing
//! owners. Older scopes and other threads must survive unchanged.
use crate::{guest_callbacks, CallCtx};
use pocket_kernel::{CreateStage, KernelError};
/// Validate abandoned continuation and borrowed-resource owners before loader
/// rollback starts invoking guest DllMain callbacks. Apply revalidates afterward.
pub(crate) fn preflight(ctx: &mut CallCtx<'_>, sp: u32) -> Result<(), KernelError> {
    crate::coredll::prepare_destroy_unwind(ctx, sp)?;
    guest_callbacks::prepare_unwind(ctx, sp)?;
    crate::coredll::paint_unwind::prepare(ctx, sp)?;
    crate::imgdecmp::prepare_unwind(ctx, sp)?;
    Ok(())
}
pub(crate) fn to_stack(ctx: &mut CallCtx<'_>, sp: u32) -> Result<(), KernelError> {
    let thread = ctx.kernel.current_thread;
    let younger = |t: usize, p: u32| t == thread && p <= sp;
    let k = &ctx.kernel;
    // DLL callbacks run their actual loader rollback before reaching this
    // synchronous retirement step; a direct caller must not bypass that owner.
    if k.module_attach_frames
        .iter()
        .any(|f| younger(f.thread, f.frame.sp))
    {
        return Err(KernelError::Dispatch(
            "DLL exceptional rollback is still pending".into(),
        ));
    }
    let windows = crate::coredll::prepare_destroy_unwind(ctx, sp)?;
    // Preflight every owner before freeing any callback or resource. Plans are
    // immediate host temporaries, not a second persistent snapshot registry.
    let callbacks = guest_callbacks::prepare_unwind(ctx, sp)?;
    let paints = crate::coredll::paint_unwind::prepare(ctx, sp)?;
    let images = crate::imgdecmp::prepare_unwind(ctx, sp)?;
    paints.flush(ctx)?;
    paints.apply(ctx);
    images.apply(ctx);
    callbacks.apply(ctx)?;
    crate::coredll::apply_destroy_unwind(ctx, windows);
    let k = &mut ctx.kernel;
    while k.dialog_frame.is_some_and(|f| f.sp <= sp) {
        if let Some((f, hwnd)) = k.dialog_frame_stack.pop() {
            k.dialog_frame = Some(f);
            k.dialog_frame_hwnd = hwnd;
        } else {
            k.dialog_frame = None;
            k.dialog_frame_hwnd = 0;
        }
    }
    while k.create_frame.is_some_and(|f| younger(f.thread, f.sp)) {
        crate::coredll::release_window_create_scratch(k);
        if let Some((f, stage, hwnd)) = k.create_frame_stack.pop() {
            k.create_frame = Some(f);
            k.create_stage = stage;
            k.create_frame_hwnd = hwnd;
        } else {
            k.create_frame = None;
            k.create_frame_hwnd = 0;
            k.create_stage = CreateStage::Idle;
        }
    }
    k.dialog_default_frames
        .retain(|(t, _, f)| !younger(*t, f.sp));
    k.control_notify_frames
        .retain(|f| !younger(f.thread, f.frame.sp));
    k.gesture_frames.retain(|(t, _, f, _)| !younger(*t, f.sp));
    k.dialog_key_frames
        .retain(|f| !younger(f.thread, f.frame.sp));
    k.accelerator_frames.retain(|(t, _, f)| !younger(*t, f.sp));
    k.notify_message_frames
        .retain(|f| !younger(f.thread, f.frame.sp));
    if let Some(frames) = k.message_frames.get_mut(&thread) {
        frames.retain(|f| f.sp > sp);
    }
    k.controls
        .capture_frames
        .retain(|(t, _, f, _)| !younger(*t, f.sp));
    let heap = &mut k.heap;
    k.controls.tab_notify_frames.retain(|f| {
        if younger(f.thread, f.frame.sp) {
            heap.free(f.scratch);
            false
        } else {
            true
        }
    });
    k.gdi.font_enum_frames.retain(|f| {
        if younger(f.thread, f.frame.sp) {
            heap.free(f.scratch);
            false
        } else {
            true
        }
    });
    k.gdi.owner_draw_frames.retain(|f| {
        if younger(f.thread, f.frame.sp) {
            heap.free(f.scratch);
            false
        } else {
            true
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_kernel::{GuestCallFrame, Thunk};
    fn frame(sp: u32) -> GuestCallFrame {
        GuestCallFrame {
            args: [0; 4],
            lr: 0x6100,
            sp,
        }
    }
    fn thunk() -> Thunk {
        Thunk {
            thunk_va: 0x70000000,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("longjmp".into()),
            friendly_name: None,
        }
    }
    #[test]
    fn nonlocal_return_keeps_older_scopes_other_threads_and_their_scratch_owners() {
        let (mut cpu, mut k, _) = crate::mfc::tests::framework_setup();
        k.dialog_frame = Some(frame(0x3000));
        k.dialog_frame_hwnd = 42;
        k.dialog_frame_stack.push((frame(0x5000), 41));
        k.dialog_default_frames = vec![
            (0, 0, frame(0x3000)),
            (0, 0, frame(0x5000)),
            (1, 0, frame(0x3000)),
        ];
        let dead = k.heap.alloc_cpp(32).unwrap();
        let live = k.heap.alloc_cpp(32).unwrap();
        for (thread, scratch) in [(0, dead), (1, live)] {
            k.gdi
                .owner_draw_frames
                .push(pocket_kernel::gdi::OwnerDrawFrame {
                    thread,
                    thunk: 0,
                    frame: frame(0x3000),
                    scratch,
                });
        }
        let t = thunk();
        to_stack(
            &mut CallCtx {
                cpu: &mut cpu,
                kernel: &mut k,
                thunk: &t,
            },
            0x4000,
        )
        .unwrap();
        assert_eq!(k.dialog_frame.unwrap().sp, 0x5000);
        assert_eq!(k.dialog_frame_hwnd, 41);
        assert_eq!(k.dialog_default_frames.len(), 2);
        assert_eq!(k.dialog_default_frames[1].0, 1);
        assert!(k.heap.msize(dead).is_none());
        assert!(k.heap.msize(live).is_some());
        assert_eq!(k.gdi.owner_draw_frames.len(), 1);
    }
    #[test]
    fn resource_transaction_rejection_happens_before_retiring_any_callback() {
        let (mut cpu, mut k, _) = crate::mfc::tests::framework_setup();
        k.dialog_frame = Some(frame(0x3000));
        k.dialog_frame_hwnd = 42;
        k.gdi
            .image_decode_frames
            .push(pocket_kernel::gdi::ImageDecodeFrame {
                thread: 0,
                thunk: 0,
                frame: frame(0x3000),
                info: [0; 15],
                bytes: vec![],
                completion: Some((0, 0xbad)),
            });
        let t = thunk();
        assert!(to_stack(
            &mut CallCtx {
                cpu: &mut cpu,
                kernel: &mut k,
                thunk: &t
            },
            0x4000
        )
        .is_err());
        assert_eq!(k.dialog_frame.unwrap().sp, 0x3000);
        assert_eq!(k.dialog_frame_hwnd, 42);
        assert_eq!(k.gdi.image_decode_frames.len(), 1);
    }
}

#[cfg(test)]
mod destroy_tests {
    use super::*;
    use pocket_kernel::{GuestCallFrame, Thunk, WindowDestroyFrame};
    #[test]
    fn interrupted_destroy_preserves_unfinished_hwnds_and_retires_only_completed_teardown() {
        let (mut cpu, mut k, _) = crate::mfc::tests::framework_setup();
        for hwnd in [11, 12, 13] {
            k.window_classes.insert(hwnd, "TEST".into());
        }
        let frame = GuestCallFrame {
            args: [0; 4],
            lr: 0x6100,
            sp: 0x3000,
        };
        k.window_destroy_frames.push(WindowDestroyFrame {
            thread: 0,
            thunk: 0,
            frame,
            windows: vec![11, 12],
            notifications: vec![(11, 0x82)].into(),
        });
        k.window_destroy_frames.push(WindowDestroyFrame {
            thread: 1,
            thunk: 0,
            frame,
            windows: vec![13],
            notifications: vec![].into(),
        });
        let thunk = Thunk {
            thunk_va: 0,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("longjmp".into()),
            friendly_name: None,
        };
        to_stack(
            &mut CallCtx {
                cpu: &mut cpu,
                kernel: &mut k,
                thunk: &thunk,
            },
            0x4000,
        )
        .unwrap();
        assert!(k.window_classes.contains_key(&11));
        assert!(!k.window_classes.contains_key(&12));
        assert!(k.window_classes.contains_key(&13));
        assert_eq!(k.window_destroy_frames.len(), 1);
        assert_eq!(k.window_destroy_frames[0].thread, 1);
    }
}
