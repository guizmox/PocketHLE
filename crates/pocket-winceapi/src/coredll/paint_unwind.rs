//! Exceptional cleanup for paint callbacks. Prepare validates all owners before
//! the nonlocal return retires any frame; completed guest drawing is retained.
use super::*;
use pocket_kernel::gdi::{ControlColorFrame, ListPaintFrame, PaintEraseFrame};
pub(crate) struct UnwindPlan {
    lists: Vec<ListPaintFrame>,
    colors: Vec<ControlColorFrame>,
    erases: Vec<PaintEraseFrame>,
    thread: usize,
    sp: u32,
}
pub(crate) fn prepare(ctx: &CallCtx<'_>, sp: u32) -> Result<UnwindPlan, KernelError> {
    let thread = ctx.kernel.current_thread;
    let lists: Vec<_> = ctx
        .kernel
        .gdi
        .list_paint_frames
        .iter()
        .filter(|f| f.thread == thread && f.frame.sp <= sp)
        .cloned()
        .collect();
    let colors: Vec<_> = ctx
        .kernel
        .gdi
        .control_color_frames
        .iter()
        .filter(|f| f.thread == thread && f.frame.sp <= sp)
        .cloned()
        .collect();
    let erases: Vec<_> = ctx
        .kernel
        .gdi
        .paint_erase_frames
        .iter()
        .filter(|f| f.thread == thread && f.frame.sp <= sp)
        .cloned()
        .collect();
    let mut scratch = std::collections::HashSet::new();
    let mut dcs = std::collections::HashSet::new();
    for f in &lists {
        if ctx.kernel.heap.msize(f.scratch).is_none()
            || !scratch.insert(f.scratch)
            || ctx.kernel.gdi.dc(f.dc).is_none()
        {
            return Err(KernelError::Dispatch(
                "Invalid list-paint exceptional owner".into(),
            ));
        }
    }
    for f in &colors {
        if ctx.kernel.gdi.dc(f.dc).is_none() || !dcs.insert(f.dc) {
            return Err(KernelError::Dispatch(
                "Invalid control-color exceptional owner".into(),
            ));
        }
    }
    Ok(UnwindPlan {
        lists,
        colors,
        erases,
        thread,
        sp,
    })
}
impl UnwindPlan {
    pub(crate) fn flush(&self, ctx: &mut CallCtx<'_>) -> Result<(), KernelError> {
        // Do not manufacture the rest of a custom draw after its callback
        // threw. Preserve real writes already made to borrowed DIB backing.
        for f in &self.lists {
            sync_dst_dib_to_guest(ctx, f.dc)?;
        }
        Ok(())
    }
    pub(crate) fn apply(self, ctx: &mut CallCtx<'_>) {
        for f in self.lists {
            ctx.kernel.heap.free(f.scratch);
        }
        for f in self.colors {
            ctx.kernel.gdi.delete(f.dc);
        }
        for f in self.erases {
            let hwnd = f.frame.args[0];
            ctx.kernel
                .gdi
                .paint_presentations
                .remove(&(self.thread, hwnd));
            if window_exists(ctx.kernel, hwnd) {
                if let Some(rect) = f.update_rect {
                    let updates = ctx
                        .kernel
                        .gdi
                        .pending_window_updates
                        .entry(hwnd)
                        .or_default();
                    if !updates.contains(&rect) {
                        updates.push(rect);
                    }
                    queue_generated_paint(ctx.kernel, hwnd, false);
                } else {
                    queue_window_paint(ctx.kernel, hwnd);
                }
                ctx.kernel.gdi.pending_window_erases.insert(hwnd);
            }
        }
        let keep = |thread: usize, sp: u32| thread != self.thread || sp > self.sp;
        ctx.kernel
            .gdi
            .list_paint_frames
            .retain(|f| keep(f.thread, f.frame.sp));
        ctx.kernel
            .gdi
            .control_color_frames
            .retain(|f| keep(f.thread, f.frame.sp));
        ctx.kernel
            .gdi
            .paint_erase_frames
            .retain(|f| keep(f.thread, f.frame.sp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(sp: u32, hwnd: u32) -> pocket_kernel::GuestCallFrame {
        pocket_kernel::GuestCallFrame {
            sp,
            args: [hwnd, 15, 0, 0],
            lr: 0x6100,
        }
    }
    #[test]
    fn exceptional_cleanup_releases_only_owned_resources_of_younger_current_thread_callbacks() {
        let (mut cpu, mut k, _) = crate::mfc::tests::framework_setup();
        k.window_classes.insert(7, "Target".into());
        let borrowed = k.gdi.create_memory_dc();
        let owned = k.gdi.create_memory_dc();
        let other_owned = k.gdi.create_memory_dc();
        let scratch = k.heap.alloc_cpp(80).unwrap();
        k.gdi.list_paint_frames.push(ListPaintFrame {
            thread: 0,
            thunk: 0,
            frame: frame(0x3000, 7),
            hwnd: 7,
            dc: borrowed,
            scratch,
            stage: 0,
            item: 0,
            flags: 0,
            item_flags: 0,
        });
        for (thread, dc) in [(0, owned), (1, other_owned)] {
            k.gdi.control_color_frames.push(ControlColorFrame {
                thread,
                thunk: 0,
                frame: frame(0x3000, 7),
                hwnd: 7,
                dc,
                dialog: false,
            });
        }
        k.gdi.paint_erase_frames.push(PaintEraseFrame {
            thread: 0,
            thunk: 0,
            frame: frame(0x3000, 7),
            update_rect: Some([1, 2, 30, 40]),
        });
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x70000000,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("longjmp".into()),
            friendly_name: None,
        };
        crate::callback_unwind::to_stack(
            &mut CallCtx {
                cpu: &mut cpu,
                kernel: &mut k,
                thunk: &thunk,
            },
            0x4000,
        )
        .unwrap();
        assert!(k.heap.msize(scratch).is_none());
        assert!(k.gdi.dc(borrowed).is_some());
        assert!(k.gdi.dc(owned).is_none());
        assert!(k.gdi.dc(other_owned).is_some());
        assert!(k.gdi.list_paint_frames.is_empty());
        assert!(k.gdi.paint_erase_frames.is_empty());
        assert_eq!(k.gdi.control_color_frames.len(), 1);
        assert_eq!(k.gdi.pending_window_updates[&7], [[1, 2, 30, 40]]);
        assert!(k.gdi.pending_window_erases.contains(&7));
    }
}
