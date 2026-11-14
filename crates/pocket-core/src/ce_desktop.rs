//! A shared CE desktop for independently running application processes.
//! BlankApp must cover only its own windows, leaving Portable Explorer and its
//! taskbar visible. No process's framebuffer is written into another process.
use crate::kernel::{framebuffer::PresentationStats, windowing, InputEvent, KernelState};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerKind {
    Desktop,
    Window,
    Topmost,
    Overlay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub id: u64,
    /// Logical CE screen coordinates: x, y, width, height.
    pub bounds: (i32, i32, i32, i32),
    pub kind: LayerKind,
}

#[derive(Debug, Clone)]
pub struct ProcessFrame {
    pub logical: [u32; 2],
    pub image: [u32; 2],
    /// Immutable, top-down RGBA; brightness has already been applied.
    pub rgba: Arc<[u8]>,
    pub layers: Vec<Layer>,
    pub text_input: bool,
    pub console_owner: Option<u32>,
    pub stats: PresentationStats,
}

impl ProcessFrame {
    pub fn capture(state: &mut KernelState) -> Result<Self, String> {
        let logical = [state.framebuffer.width, state.framebuffer.height];
        let frame = state.presentation_framebuffer()?;
        let image = frame.presentation_size();
        let mut rgba = Vec::new();
        frame.snapshot_presentation_rgba8888_into(&mut rgba);
        let mut order = state.ce.windows.z_order.clone();
        let mut extra: Vec<_> = state
            .window_rects
            .keys()
            .copied()
            .filter(|h| !order.contains(h))
            .collect();
        extra.sort_unstable();
        order.extend(extra);
        let mut layers = Vec::new();
        for hwnd in order {
            let (style, extended) = state.window_styles.get(&hwnd).copied().unwrap_or_default();
            if style & 0x4000_0000 != 0
                || state.controls.get(hwnd).is_some()
                || state.controls.panel(hwnd).is_some()
                || !state.window_tree_visible(hwnd, |_| false)
            {
                continue;
            }
            let Some(bounds) = state
                .native_window_screen_rect(hwnd)
                .filter(|r| r.2 > 0 && r.3 > 0)
            else {
                continue;
            };
            let class = state
                .window_classes
                .get(&hwnd)
                .map(String::as_str)
                .unwrap_or("");
            let kind = if state.ce.shell.desktop == Some(hwnd)
                || class.eq_ignore_ascii_case("PortableCEExplorerDesktop")
                || class.eq_ignore_ascii_case("DesktopExplorerWindow")
                || windowing::is_desktop_layer(style, bounds, (logical[0], logical[1]))
            {
                LayerKind::Desktop
            } else if state.ce.shell.taskbar.is_some_and(|(h, _)| h == hwnd)
                || extended & 8 != 0
                || style & 0x8000_0000 != 0
                    && style & windowing::WS_CAPTION == 0
                    && bounds.0 <= 0
                    && bounds.2 >= logical[0] as i32
                    && bounds.1 >= logical[1] as i32 - 2 * crate::kernel::shell_menu::HEIGHT
                    && bounds.3 <= 2 * crate::kernel::shell_menu::HEIGHT
            {
                LayerKind::Topmost
            } else {
                LayerKind::Window
            };
            layers.push(Layer {
                id: u64::from(hwnd),
                bounds,
                kind,
            });
        }
        for hwnd in state.controls.panel_handles() {
            if let Some(p) = state.controls.panel(hwnd).filter(|p| p.visible) {
                layers.push(Layer {
                    id: u64::from(hwnd),
                    bounds: (p.x, p.y, p.w, p.h),
                    kind: LayerKind::Window,
                });
            }
        }
        for popup in state.gdi.shell_menus.popup_layouts(
            logical[0] as i32,
            logical[1] as i32,
            &state.menu_items,
        ) {
            layers.push(Layer {
                id: (1 << 32) | u64::from(popup.menu),
                bounds: (popup.left, popup.top, popup.width, popup.height),
                kind: LayerKind::Overlay,
            });
        }
        if let Some(hwnd) = state.ce.windows.system_menu {
            if let Some(rect) = state.native_window_screen_rect(hwnd) {
                let r = windowing::system_menu_rect(rect, state.window_metrics(hwnd));
                layers.push(Layer {
                    id: 2 << 32,
                    bounds: (r[0], r[1], r[2] - r[0], r[3] - r[1]),
                    kind: LayerKind::Overlay,
                });
            }
        }
        if let Some(m) = &state.modal {
            layers.push(Layer {
                id: 3 << 32,
                bounds: (m.x, m.y, m.w, m.h),
                kind: LayerKind::Overlay,
            });
        }
        if state.shell_keyboard.visible {
            layers.push(Layer {
                id: 4 << 32,
                bounds: state
                    .shell_keyboard
                    .display_bounds(logical[0] as i32, logical[1] as i32),
                kind: LayerKind::Overlay,
            });
        }
        let console_owner = state.vfs.console_device().and_then(|device| {
            device
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .owner_process
        });
        // Windowless GAPI/GLES applications still own their whole display.
        // A classic process whose last window was destroyed owns no pixels.
        // Pocket CMD children share CON: and have no independent display.
        if layers.is_empty()
            && !state.ce.windows.enabled
            && (!state.console.inherited || state.framebuffer.presentation_stats.completed() != 0)
        {
            layers.push(Layer {
                id: 0,
                bounds: (0, 0, logical[0] as i32, logical[1] as i32),
                kind: LayerKind::Window,
            });
        }
        let text_input = state.accepts_text_input();
        Ok(Self {
            logical,
            image: [image.0, image.1],
            rgba: rgba.into(),
            layers,
            text_input,
            console_owner,
            stats: state.framebuffer.presentation_stats.clone(),
        })
    }
}

pub struct ComposedFrame {
    pub logical: [u32; 2],
    pub image: [u32; 2],
    pub rgba: Vec<u8>,
    pub text_input: bool,
    pub stats: PresentationStats,
    /// Changes only when displayed pixels or image dimensions change.
    pub image_revision: u64,
    /// Orders concurrent captures, focus changes and process teardown.
    pub revision: u64,
}

#[derive(Default)]
pub struct Desktop {
    frames: HashMap<u32, ProcessFrame>,
    order: Vec<(u32, u64)>,
    input: VecDeque<(u32, InputEvent)>,
    key_owners: HashMap<u16, u32>,
    pointer_owner: Option<u32>,
    focus: u32,
    revision: u64,
    composed_revision: u64,
    last_size: [u32; 2],
    last_pixels: Vec<u8>,
    image_revision: u64,
    images: PresentationStats,
}

impl Desktop {
    fn layer(&self, key: (u32, u64)) -> Option<&Layer> {
        self.frames
            .get(&key.0)?
            .layers
            .iter()
            .find(|l| l.id == key.1)
    }
    pub fn publish(&mut self, pid: u32, frame: ProcessFrame) -> Result<(), String> {
        if frame.logical.contains(&0)
            || frame.image.contains(&0)
            || (frame.image[0] as usize)
                .checked_mul(frame.image[1] as usize)
                .and_then(|n| n.checked_mul(4))
                != Some(frame.rgba.len())
        {
            return Err("invalid CE desktop image geometry".into());
        }
        if self.frames.values().any(|f| f.logical != frame.logical) {
            return Err("CE processes disagree on logical screen dimensions".into());
        }
        let local_order_changed = self.frames.get(&pid).is_some_and(|previous| {
            previous
                .layers
                .iter()
                .map(|l| (l.id, l.kind))
                .collect::<Vec<_>>()
                != frame
                    .layers
                    .iter()
                    .map(|l| (l.id, l.kind))
                    .collect::<Vec<_>>()
        });
        self.order
            .retain(|&(owner, id)| owner != pid || frame.layers.iter().any(|l| l.id == id));
        for layer in &frame.layers {
            if !self.order.contains(&(pid, layer.id)) {
                self.order.push((pid, layer.id));
            }
        }
        self.frames.insert(pid, frame);
        // Desktop and shell overlays keep their global rank when an ordinary
        // application gets focus. Clicking Start must not hide BlankApp.
        let ranks: HashMap<_, _> = self
            .order
            .iter()
            .filter_map(|&key| self.layer(key).map(|l| (key, l.kind)))
            .collect();
        self.order.sort_by_key(|key| ranks[key]);
        if local_order_changed {
            // Synchronize a guest SetWindowPos without raising all Explorer
            // windows every time its background repaints.
            for kind in [
                LayerKind::Desktop,
                LayerKind::Window,
                LayerKind::Topmost,
                LayerKind::Overlay,
            ] {
                let local: Vec<_> = self.frames[&pid]
                    .layers
                    .iter()
                    .filter(|l| l.kind == kind)
                    .map(|l| (pid, l.id))
                    .collect();
                let mut ordered = local.into_iter();
                for key in &mut self.order {
                    if key.0 == pid && ranks[key] == kind {
                        if let Some(next) = ordered.next() {
                            *key = next;
                        }
                    }
                }
            }
        }
        if self.focus == 0 {
            self.focus = pid;
        }
        // A console child shares its parent's CON: window. Its output must not
        // take keyboard focus away from the only process with that window.
        self.focus = self.visible_input_owner(self.focus);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn activate(&mut self, pid: u32) {
        let pid = self.visible_input_owner(pid);
        if self.focus != pid {
            self.focus = pid;
            self.revision = self.revision.wrapping_add(1);
        }
    }
    fn visible_input_owner(&self, pid: u32) -> u32 {
        self.frames
            .get(&pid)
            .filter(|frame| frame.layers.is_empty())
            .and_then(|frame| frame.console_owner)
            .filter(|owner| {
                self.frames
                    .get(owner)
                    .is_some_and(|frame| !frame.layers.is_empty())
            })
            .unwrap_or(pid)
    }
    pub fn focused_process(&self) -> u32 {
        self.focus
    }
    pub fn remove(&mut self, pid: u32, next: u32) {
        self.frames.remove(&pid);
        self.order.retain(|key| key.0 != pid);
        self.input.retain(|(owner, _)| *owner != pid);
        self.key_owners.retain(|_, owner| *owner != pid);
        if self.pointer_owner == Some(pid) {
            self.pointer_owner = None;
        }
        if self.focus == pid {
            self.focus = self.visible_input_owner(next);
        }
        self.revision = self.revision.wrapping_add(1);
    }
    fn raise(&mut self, key: (u32, u64)) {
        let Some(kind) = self.layer(key).map(|l| l.kind) else {
            return;
        };
        if kind == LayerKind::Desktop {
            return;
        }
        self.order.retain(|k| *k != key);
        let index = self
            .order
            .iter()
            .position(|&k| self.layer(k).is_some_and(|l| l.kind > kind))
            .unwrap_or(self.order.len());
        self.order.insert(index, key);
        self.revision = self.revision.wrapping_add(1);
    }
    fn at(&self, x: u16, y: u16) -> Option<(u32, u64)> {
        self.order.iter().rev().copied().find(|&key| {
            self.layer(key).is_some_and(|l| {
                let r = l.bounds;
                i32::from(x) >= r.0
                    && i32::from(y) >= r.1
                    && i32::from(x) < r.0.saturating_add(r.2)
                    && i32::from(y) < r.1.saturating_add(r.3)
            })
        })
    }
    /// Preserve the recipient of a drag and every held-key release across a
    /// focus change. Each process drains only its own addressed input.
    pub fn queue(&mut self, event: InputEvent, fallback: u32) -> u32 {
        let focused = if self.focus != 0 {
            self.focus
        } else {
            fallback
        };
        let owner = match event {
            InputEvent::PointerDown { x, y } => {
                let key = self.at(x, y);
                if let Some(key) = key {
                    self.raise(key);
                }
                let owner = key.map_or(focused, |k| k.0);
                self.pointer_owner = Some(owner);
                self.activate(owner);
                owner
            }
            InputEvent::PointerMove { x, y } => self
                .pointer_owner
                .or_else(|| self.at(x, y).map(|k| k.0))
                .unwrap_or(focused),
            InputEvent::PointerUp { .. } => self.pointer_owner.take().unwrap_or(focused),
            InputEvent::KeyDown { vk } => *self.key_owners.entry(vk).or_insert(focused),
            InputEvent::KeyUp { vk } => self.key_owners.remove(&vk).unwrap_or(focused),
            _ => focused,
        };
        self.input.push_back((owner, event));
        owner
    }
    pub fn drain(&mut self, pid: u32, pending: &mut VecDeque<InputEvent>) {
        self.input.retain(|(owner, event)| {
            if *owner == pid {
                pending.push_back(*event);
                false
            } else {
                true
            }
        });
    }
    pub fn compose(&mut self) -> Result<Option<ComposedFrame>, String> {
        if self.composed_revision == self.revision {
            return Ok(None);
        }
        self.composed_revision = self.revision;
        let Some(first) = self.frames.values().next() else {
            return Ok(None);
        };
        let logical = first.logical;
        let scale = self
            .frames
            .values()
            .map(|f| {
                f.image[0]
                    .div_ceil(logical[0])
                    .max(f.image[1].div_ceil(logical[1]))
            })
            .max()
            .unwrap_or(1)
            .max(1);
        let image = [
            logical[0]
                .checked_mul(scale)
                .ok_or("CE desktop size overflow")?,
            logical[1]
                .checked_mul(scale)
                .ok_or("CE desktop size overflow")?,
        ];
        let length = (image[0] as usize)
            .checked_mul(image[1] as usize)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n <= 256 * 1024 * 1024)
            .ok_or("CE desktop image is too large")?;
        let bg = crate::kernel::framebuffer::rgb565_to_rgba8888(
            crate::kernel::framebuffer::colorref_to_rgb565(windowing::system_color(1)),
        )
        .to_le_bytes();
        let mut rgba: Vec<u8> = (0..length / 4).flat_map(|_| bg).collect();
        for &key in &self.order {
            let frame = &self.frames[&key.0];
            let Some(layer) = self.layer(key) else {
                continue;
            };
            let (x, y, w, h) = layer.bounds;
            let left = (i64::from(x) * i64::from(scale)).clamp(0, i64::from(image[0])) as usize;
            let right = (i64::from(x.saturating_add(w)) * i64::from(scale))
                .clamp(0, i64::from(image[0])) as usize;
            let top = (i64::from(y) * i64::from(scale)).clamp(0, i64::from(image[1])) as usize;
            let bottom = (i64::from(y.saturating_add(h)) * i64::from(scale))
                .clamp(0, i64::from(image[1])) as usize;
            if left >= right || top >= bottom {
                continue;
            }
            for row in top..bottom {
                let dst = (row * image[0] as usize + left) * 4;
                if frame.image == image {
                    rgba[dst..dst + (right - left) * 4]
                        .copy_from_slice(&frame.rgba[dst..dst + (right - left) * 4]);
                } else {
                    let sy = row * frame.image[1] as usize / image[1] as usize;
                    for column in left..right {
                        let sx = column * frame.image[0] as usize / image[0] as usize;
                        let src = (sy * frame.image[0] as usize + sx) * 4;
                        let target = dst + (column - left) * 4;
                        rgba[target..target + 4].copy_from_slice(&frame.rgba[src..src + 4]);
                    }
                }
            }
        }
        if self.last_size != image || self.last_pixels != rgba {
            self.last_size = image;
            self.last_pixels.clone_from(&rgba);
            self.image_revision = self.image_revision.wrapping_add(1);
            self.images.record_image_update();
        }
        let focused = self.frames.get(&self.focus).unwrap_or(first);
        Ok(Some(ComposedFrame {
            logical,
            image,
            rgba,
            text_input: focused.text_input,
            stats: focused.stats.with_image_updates_from(&self.images),
            image_revision: self.image_revision,
            revision: self.revision,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(color: u8, layers: &[(u64, (i32, i32, i32, i32), LayerKind)]) -> ProcessFrame {
        ProcessFrame {
            logical: [8, 6],
            image: [8, 6],
            rgba: vec![color; 8 * 6 * 4].into(),
            layers: layers
                .iter()
                .map(|&(id, bounds, kind)| Layer { id, bounds, kind })
                .collect(),
            text_input: false,
            console_owner: None,
            stats: PresentationStats::default(),
        }
    }
    #[test]
    fn child_window_preserves_explorer_and_taskbar_and_exit_removes_its_pixels() {
        let mut desktop = Desktop::default();
        desktop
            .publish(
                1,
                frame(
                    20,
                    &[
                        (1, (0, 0, 8, 6), LayerKind::Window),
                        (2, (0, 5, 8, 1), LayerKind::Topmost),
                    ],
                ),
            )
            .unwrap();
        desktop
            .publish(2, frame(200, &[(1, (2, 1, 4, 5), LayerKind::Window)]))
            .unwrap();
        desktop.activate(2);
        let combined = desktop.compose().unwrap().unwrap();
        assert_eq!(combined.rgba[(2 * 8 + 3) * 4], 200);
        assert_eq!(combined.rgba[0], 20);
        assert_eq!(combined.rgba[(5 * 8 + 3) * 4], 20);
        desktop.remove(2, 1);
        assert!(desktop
            .compose()
            .unwrap()
            .unwrap()
            .rgba
            .iter()
            .all(|p| *p == 20));
    }
    #[test]
    fn start_and_background_repaints_do_not_raise_explorer_over_blankapp() {
        let mut d = Desktop::default();
        let explorer = frame(
            20,
            &[
                (1, (0, 0, 8, 6), LayerKind::Window),
                (2, (0, 5, 8, 1), LayerKind::Topmost),
            ],
        );
        d.publish(1, explorer.clone()).unwrap();
        d.publish(2, frame(200, &[(1, (2, 1, 4, 4), LayerKind::Window)]))
            .unwrap();
        assert_eq!(d.queue(InputEvent::PointerDown { x: 3, y: 5 }, 2), 1);
        d.queue(InputEvent::PointerUp { x: 3, y: 5 }, 1);
        d.publish(1, explorer).unwrap();
        assert_eq!(d.compose().unwrap().unwrap().rgba[(2 * 8 + 3) * 4], 200);
    }
    #[test]
    fn drag_and_key_releases_stay_with_their_original_process() {
        let mut d = Desktop::default();
        d.publish(1, frame(20, &[(1, (0, 0, 8, 6), LayerKind::Window)]))
            .unwrap();
        d.publish(2, frame(200, &[(1, (2, 1, 4, 4), LayerKind::Window)]))
            .unwrap();
        d.activate(2);
        assert_eq!(d.queue(InputEvent::KeyDown { vk: 65 }, 2), 2);
        assert_eq!(d.queue(InputEvent::PointerDown { x: 3, y: 2 }, 2), 2);
        assert_eq!(d.queue(InputEvent::PointerMove { x: 0, y: 0 }, 2), 2);
        assert_eq!(d.queue(InputEvent::PointerUp { x: 0, y: 0 }, 2), 2);
        assert_eq!(d.queue(InputEvent::PointerDown { x: 0, y: 0 }, 2), 1);
        assert_eq!(d.queue(InputEvent::KeyUp { vk: 65 }, 1), 2);
        let mut input = VecDeque::new();
        d.drain(2, &mut input);
        assert_eq!(input.len(), 5);
    }
    #[test]
    fn a_child_keyboard_owns_pixels_and_clicks_outside_its_dialog() {
        let mut desktop = Desktop::default();
        desktop
            .publish(1, frame(20, &[(1, (0, 0, 8, 6), LayerKind::Window)]))
            .unwrap();
        desktop
            .publish(
                2,
                frame(
                    200,
                    &[
                        (1, (2, 1, 4, 3), LayerKind::Window),
                        (4 << 32, (0, 3, 8, 3), LayerKind::Overlay),
                    ],
                ),
            )
            .unwrap();
        desktop.activate(2);
        assert_eq!(
            desktop.compose().unwrap().unwrap().rgba[(5 * 8 + 7) * 4],
            200
        );
        assert_eq!(desktop.queue(InputEvent::PointerDown { x: 7, y: 5 }, 1), 2);
        assert_eq!(desktop.queue(InputEvent::PointerUp { x: 7, y: 5 }, 1), 2);
        desktop
            .publish(2, frame(200, &[(1, (2, 1, 4, 3), LayerKind::Window)]))
            .unwrap();
        assert_eq!(desktop.focused_process(), 2);
        let combined = desktop.compose().unwrap().unwrap();
        assert_eq!(combined.rgba[(2 * 8 + 3) * 4], 200);
        assert_eq!(combined.rgba[(5 * 8 + 7) * 4], 20);
    }
    #[test]
    fn unchanged_repaint_keeps_image_identity_and_high_density_child_pixels() {
        let mut d = Desktop::default();
        let root = frame(20, &[(1, (0, 0, 8, 6), LayerKind::Window)]);
        d.publish(1, root.clone()).unwrap();
        let before = d.compose().unwrap().unwrap().image_revision;
        d.publish(1, root).unwrap();
        assert_eq!(d.compose().unwrap().unwrap().image_revision, before);
        let mut child = frame(200, &[(1, (2, 1, 4, 4), LayerKind::Window)]);
        child.image = [16, 12];
        child.rgba = vec![200; 16 * 12 * 4].into();
        d.publish(2, child).unwrap();
        let combined = d.compose().unwrap().unwrap();
        assert_eq!(combined.image, [16, 12]);
        assert_eq!(combined.rgba[(4 * 16 + 6) * 4], 200);
        assert_eq!(combined.rgba[0], 20);
    }
    #[test]
    fn windowless_cmd_child_keeps_console_pixels_text_focus_and_explorer_taskbar() {
        let mut desktop = Desktop::default();
        desktop
            .publish(
                1,
                frame(
                    20,
                    &[
                        (1, (0, 0, 8, 6), LayerKind::Desktop),
                        (2, (0, 5, 8, 1), LayerKind::Topmost),
                    ],
                ),
            )
            .unwrap();
        let mut console = frame(100, &[(1, (1, 1, 6, 4), LayerKind::Window)]);
        console.text_input = true;
        console.console_owner = Some(2);
        desktop.publish(2, console).unwrap();
        desktop.activate(2);
        let before = desktop.compose().unwrap().unwrap();
        let mut child = frame(0, &[]);
        child.console_owner = Some(2);
        desktop.publish(3, child).unwrap();
        desktop.activate(3);
        assert_eq!(desktop.focused_process(), 2);
        let combined = desktop.compose().unwrap().unwrap();
        assert!(combined.text_input);
        assert_eq!(combined.rgba, before.rgba);
        assert_eq!(combined.image_revision, before.image_revision);
        assert_eq!(desktop.queue(InputEvent::Text { character: 'Ж' }, 3), 2);
        let mut input = VecDeque::new();
        desktop.drain(2, &mut input);
        assert_eq!(input.pop_front(), Some(InputEvent::Text { character: 'Ж' }));
        assert_eq!(desktop.queue(InputEvent::PointerDown { x: 2, y: 5 }, 3), 1);
        assert_eq!(desktop.queue(InputEvent::PointerUp { x: 2, y: 5 }, 3), 1);
        desktop.activate(3);
        desktop.remove(3, 2);
        assert_eq!(desktop.focused_process(), 2);
        assert_eq!(desktop.compose().unwrap().unwrap().rgba, before.rgba);
    }
}
