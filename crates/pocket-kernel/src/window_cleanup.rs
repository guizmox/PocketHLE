//! Window surfaces and input queues cannot outlive their owning UI thread.
//! Portable Explorer ends its folder thread without DestroyWindow on the frame.
use crate::KernelState;
use std::collections::HashSet;

impl KernelState {
    pub(crate) fn reclaim_finished_windows(&mut self) {
        let mut finished: HashSet<_> = self
            .threads
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.finished.then_some(i + 1))
            .collect();
        if self.main_thread.exit_code.is_some() {
            finished.insert(0);
        }
        let mut windows: Vec<_> = self
            .window_threads
            .iter()
            .filter_map(|(&hwnd, thread)| finished.contains(thread).then_some(hwnd))
            .collect();
        let mut i = 0;
        while i < windows.len() && windows.len() < 4096 {
            let hwnd = windows[i];
            let children = self
                .controls
                .children_of(hwnd)
                .into_iter()
                .chain(
                    self.window_parents
                        .iter()
                        .filter_map(|(&child, &parent)| (parent == hwnd).then_some(child)),
                )
                .collect::<Vec<_>>();
            for child in children {
                if !windows.contains(&child) {
                    windows.push(child);
                }
            }
            i += 1;
        }
        let changed = !windows.is_empty();
        // Release children before their parent's control panel drops them.
        for hwnd in windows.into_iter().rev() {
            self.discard_one_window_metadata(&hwnd);
        }
        let heap = &mut self.heap;
        self.gdi.list_paint_frames.retain(|frame| {
            if finished.contains(&frame.thread) {
                heap.free(frame.scratch);
                false
            } else {
                true
            }
        });
        self.gdi.owner_draw_frames.retain(|frame| {
            if finished.contains(&frame.thread) {
                heap.free(frame.scratch);
                false
            } else {
                true
            }
        });
        let mut dcs = Vec::new();
        self.gdi.control_color_frames.retain(|frame| {
            if finished.contains(&frame.thread) {
                dcs.push(frame.dc);
                false
            } else {
                true
            }
        });
        for dc in dcs {
            self.gdi.delete(dc);
        }
        self.window_destroy_frames
            .retain(|frame| !finished.contains(&frame.thread));
        self.control_notify_frames
            .retain(|frame| !finished.contains(&frame.thread));
        if changed && self.ce.windows.enabled {
            self.composite_controls();
        }
    }

    pub fn discard_window_metadata(&mut self, hwnd: &u32) {
        let mut windows = vec![*hwnd];
        let mut seen = HashSet::from([*hwnd]);
        let mut index = 0;
        while index < windows.len() {
            let parent = windows[index];
            for child in self.controls.children_of(parent).into_iter().chain(
                self.window_parents
                    .iter()
                    .filter_map(|(&child, &owner)| (owner == parent).then_some(child)),
            ) {
                if seen.insert(child) {
                    windows.push(child);
                }
            }
            index += 1;
        }
        for child in windows.into_iter().rev() {
            self.discard_one_window_metadata(&child);
        }
    }

    fn discard_one_window_metadata(&mut self, hwnd: &u32) {
        let directdraw = &mut self.framebuffer.directdraw;
        if directdraw
            .fullscreen_owner
            .is_some_and(|(_, owner)| owner == *hwnd)
        {
            directdraw.fullscreen_owner = None;
        }
        if self.console.view.as_ref().is_some_and(|v| v.hwnd == *hwnd) {
            self.release_console_window();
        }
        self.ce
            .window_aux
            .properties
            .retain(|(window, _), _| window != hwnd);
        self.ce.window_aux.dialog_units.remove(hwnd);
        self.ce
            .carets
            .threads
            .retain(|_, caret| caret.hwnd != *hwnd);
        if self.ce.shell.desktop == Some(*hwnd) {
            self.ce.shell.desktop = None;
        }
        if self.ce.shell.taskbar.is_some_and(|(h, _)| h == *hwnd) {
            self.ce.shell.taskbar = None;
        }
        if self
            .controls
            .file_dialog
            .as_ref()
            .is_some_and(|d| d.hwnd == *hwnd)
        {
            let dialog = self.controls.file_dialog.take().unwrap();
            self.gdi.image_lists.remove(&dialog.image_list);
            self.shell_keyboard.visible = false;
            self.controls.focus = dialog.previous_focus;
        }
        if let Some(control) = self.controls.get(*hwnd) {
            let bitmaps: Vec<_> = control.toolbar.owned_bitmaps.iter().copied().collect();
            for bitmap in bitmaps {
                self.gdi.delete(bitmap);
            }
        }
        if let Some(rebar) = self.gdi.rebars.remove(hwnd) {
            if rebar.notification != 0 {
                self.heap.free(rebar.notification);
            }
        }
        self.gdi.command_adornments.remove(hwnd);
        self.ce.windows.z_order.retain(|h| h != hwnd);
        self.ce.windows.restored.remove(hwnd);
        if self.ce.windows.pressed.is_some_and(|p| p.0 == *hwnd) {
            self.ce.windows.pressed = None;
        }
        if self.ce.windows.drag.is_some_and(|p| p.hwnd == *hwnd) {
            self.ce.windows.drag = None;
        }
        if self.ce.windows.system_menu == Some(*hwnd) {
            self.ce.windows.system_menu = None;
        }
        for dc in [
            self.ce.windows.window_dcs.remove(hwnd),
            self.window_dcs.remove(hwnd),
        ]
        .into_iter()
        .flatten()
        {
            let bitmap = self.gdi.dc(dc).and_then(|d| d.selected_bitmap);
            self.gdi.delete(dc);
            if let Some(bitmap) = bitmap {
                self.gdi.delete(bitmap);
            }
        }
        self.controls
            .pointer_capture
            .retain(|_, owner| owner != hwnd);
        self.controls.destroy(*hwnd);
        self.controls.destroy_children_of(*hwnd);
        self.timers.retain(|key, _| key.1 != *hwnd);
        self.posted_messages.retain(|msg| msg.0 != *hwnd);
        self.gdi.generated_window_paints.remove(hwnd);
        self.window_rects.remove(hwnd);
        self.window_menus.remove(hwnd);
        self.notification_icons
            .retain(|(owner, _), _| owner != hwnd);
        self.gdi.pending_window_erases.remove(hwnd);
        self.gdi.owner_draw_states.remove(hwnd);
        self.gdi.list_paint_stamps.remove(hwnd);
        self.gdi.dialog_backgrounds.remove(hwnd);
        if self.pending_message.is_some_and(|m| m.0 == *hwnd) {
            self.pending_message = None;
        }
        self.gdi.shell_menus.bars.remove(hwnd);
        self.window_classes.remove(hwnd);
        self.window_titles.remove(hwnd);
        self.window_ids.remove(hwnd);
        self.scroll_info.retain(|(h, _), _| h != hwnd);
        if let Some(ptr) = self.window_extra.get(&(*hwnd, -1001)).copied() {
            self.heap.free(ptr);
        }
        self.window_extra.retain(|(h, _), _| h != hwnd);
        self.window_styles.remove(hwnd);
        self.window_threads.remove(hwnd);
        self.ce.window_icons.remove(hwnd);
        self.window_input_contexts.remove(hwnd);
        self.window_parents.remove(hwnd);
        self.window_visibility.remove(hwnd);
        self.window_procs.remove(hwnd);
        self.window_userdata.remove(hwnd);
        if self.active_window == *hwnd {
            self.active_window = if self.ce.windows.enabled {
                self.ce
                    .windows
                    .z_order
                    .iter()
                    .rev()
                    .copied()
                    .find(|h| self.window_tree_visible(*h, |h| h != 0))
                    .unwrap_or(0xdead0001)
            } else {
                0xdead0001
            };
        }
    }
}
