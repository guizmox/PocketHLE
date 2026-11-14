//! Compose classic CE windows in their own client coordinates and z order.
//! CerfDemo's fullscreen background must stay behind its separate About window,
//! including when a repaint arrives from the background after the dialog.
use crate::{
    gdi::{Bitmap, Font, Surface},
    windowing::{self, Metrics},
    KernelState,
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

#[derive(Clone)]
struct Window {
    hwnd: u32,
    parent: Option<u32>,
    rect: (i32, i32, i32, i32),
    origin: (i32, i32),
    size: (i32, i32),
    metrics: Metrics,
    style: u32,
    title: String,
    menu: Vec<(u32, String)>,
    bitmap: Option<Bitmap>,
    nonclient: Option<Bitmap>,
    children: Vec<u32>,
    icon: Option<crate::imaging::Image>,
    caret: Option<crate::caret::Caret>,
}

impl KernelState {
    pub fn window_icon(&self, hwnd: u32) -> Option<&crate::imaging::Image> {
        let explicit = self.ce.window_icons.get(&hwnd).copied().unwrap_or_default();
        let class = self
            .window_classes
            .get(&hwnd)
            .map(|name| name.to_lowercase());
        let class_handle = |index| {
            class
                .as_ref()
                .and_then(|name| self.gdi.class_values.get(&(name.clone(), index)))
                .copied()
                .unwrap_or(0)
        };
        [
            explicit[0],
            explicit[1],
            class_handle(-34),
            class_handle(-14),
        ]
        .into_iter()
        .find_map(|handle| self.gdi.icons.get(&handle))
    }

    pub fn window_metrics(&self, hwnd: u32) -> Metrics {
        if !self.ce.windows.enabled
            || self.controls.panel(hwnd).is_some()
            || self.controls.get(hwnd).is_some()
        {
            return Metrics::default();
        }
        let (style, extended) = self.window_styles.get(&hwnd).copied().unwrap_or_default();
        Metrics::new(style, extended, self.window_menus.contains_key(&hwnd))
    }
    pub fn window_client_size(&self, hwnd: u32) -> Option<(i32, i32)> {
        if let Some(c) = self.controls.get(hwnd) {
            return Some((
                c.w,
                if c.class == crate::controls::ControlClass::ComboBox {
                    c.choice_closed_height()
                } else {
                    c.h
                },
            ));
        }
        if let Some(p) = self.controls.panel(hwnd) {
            let border = 2 * i32::from(p.border);
            return Some(((p.w - border).max(0), (p.h - border - p.caption_h()).max(0)));
        }
        if self.ce.windows.enabled
            && self
                .window_styles
                .get(&hwnd)
                .is_some_and(|s| s.0 & 0x2000_0000 != 0)
        {
            return Some((0, 0));
        }
        self.window_rects
            .get(&hwnd)
            .map(|r| self.window_metrics(hwnd).client_size(r.2, r.3))
    }
    pub fn native_window_screen_rect(&self, hwnd: u32) -> Option<(i32, i32, i32, i32)> {
        let mut r = *self.window_rects.get(&hwnd)?;
        if self
            .window_styles
            .get(&hwnd)
            .is_some_and(|s| s.0 & 0x4000_0000 != 0)
        {
            let origin = self
                .window_parents
                .get(&hwnd)
                .and_then(|p| self.window_client_origin(*p))?;
            r.0 = r.0.saturating_add(origin.0);
            r.1 = r.1.saturating_add(origin.1);
        }
        Some(r)
    }

    pub(crate) fn composite_classic_windows(&mut self) {
        self.refresh_control_visibility();
        let visible = |hwnd| self.window_tree_visible(hwnd, |h| h != 0);
        let mut windows = HashMap::new();
        for (&hwnd, &rect) in &self.window_rects {
            if self.controls.get(hwnd).is_some()
                || self.controls.panel(hwnd).is_some()
                || !visible(hwnd)
                || rect.2 <= 0
                || rect.3 <= 0
            {
                continue;
            }
            let Some(origin) = self.window_client_origin(hwnd) else {
                continue;
            };
            let Some(rect) = self.native_window_screen_rect(hwnd) else {
                continue;
            };
            let style = self.window_styles.get(&hwnd).map_or(0, |s| s.0);
            let metrics = self.window_metrics(hwnd);
            let bitmap = self
                .window_dcs
                .get(&hwnd)
                .and_then(|d| self.gdi.dc(*d))
                .and_then(|d| d.selected_bitmap)
                .and_then(|b| self.gdi.bitmap(b))
                .cloned();
            let nonclient = self
                .ce
                .windows
                .window_dcs
                .get(&hwnd)
                .and_then(|d| self.gdi.dc(*d))
                .and_then(|d| d.selected_bitmap)
                .and_then(|b| self.gdi.bitmap(b))
                .cloned();
            let menu = self
                .window_menus
                .get(&hwnd)
                .and_then(|m| self.menu_items.get(m))
                .map(|items| {
                    items
                        .iter()
                        .map(|item| (item.id, String::from_utf16_lossy(&item.text)))
                        .collect()
                })
                .unwrap_or_default();
            let children = self
                .window_parents
                .iter()
                .filter(|(child, parent)| {
                    **parent == hwnd
                        && self
                            .window_styles
                            .get(child)
                            .is_some_and(|s| s.0 & 0x4000_0000 != 0)
                })
                .map(|(&h, _)| h)
                .collect();
            windows.insert(
                hwnd,
                Window {
                    hwnd,
                    parent: (style & 0x4000_0000 != 0)
                        .then(|| self.window_parents.get(&hwnd).copied())
                        .flatten(),
                    rect,
                    origin,
                    size: metrics.client_size(rect.2, rect.3),
                    metrics,
                    style,
                    title: self.window_titles.get(&hwnd).cloned().unwrap_or_default(),
                    menu,
                    bitmap,
                    nonclient,
                    children,
                    icon: self.window_icon(hwnd).cloned(),
                    caret: self
                        .ce
                        .carets
                        .threads
                        .values()
                        .find(|caret| {
                            caret.hwnd == hwnd
                                && self.controls.focus == hwnd
                                && caret.shown(std::time::Instant::now(), self.ce.carets.blink_ms)
                        })
                        .cloned(),
                },
            );
        }
        let mut order: Vec<_> = self
            .ce
            .windows
            .z_order
            .iter()
            .copied()
            .filter(|h| windows.get(h).is_some_and(|v| v.style & 0x4000_0000 == 0))
            .collect();
        let mut extra: Vec<_> = windows
            .iter()
            .filter(|(h, w)| w.style & 0x4000_0000 == 0 && !order.contains(h))
            .map(|(&h, _)| h)
            .collect();
        extra.sort_unstable();
        order.extend(extra);
        order.sort_by_key(|hwnd| !self.is_desktop_window(*hwnd));
        let controls = self.controls.clone();
        let control_layers: HashMap<u32, Bitmap> = self
            .window_dcs
            .iter()
            .filter(|(h, _)| self.controls.get(**h).is_some() && visible(**h))
            .filter_map(|(&h, &dc)| {
                self.gdi
                    .dc(dc)
                    .and_then(|d| d.selected_bitmap)
                    .and_then(|b| self.gdi.bitmap(b))
                    .cloned()
                    .map(|b| (h, b))
            })
            .collect();
        if order.is_empty()
            && !controls.has_visible_content()
            && self.modal.is_none()
            && self.ce.windows.last_stamp.is_none()
        {
            return;
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.active_window.hash(&mut hash);
        self.ce.windows.pressed.hash(&mut hash);
        self.ce.windows.system_menu.hash(&mut hash);
        self.gdi.installed_fonts.len().hash(&mut hash);
        order.hash(&mut hash);
        let mut ids: Vec<_> = windows.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let w = &windows[&id];
            (w.parent, w.rect, w.origin, w.style, &w.title, &w.menu).hash(&mut hash);
            w.caret
                .as_ref()
                .map(|caret| (caret.position, caret.size, caret.gray, &caret.mask))
                .hash(&mut hash);
            if let Some(icon) = &w.icon {
                (
                    icon.width,
                    icon.height,
                    &icon.pixels,
                    &icon.alpha,
                    &icon.invert,
                )
                    .hash(&mut hash);
            }
            for b in [&w.bitmap, &w.nonclient].into_iter().flatten() {
                b.pixels.hash(&mut hash);
                b.painted.hash(&mut hash);
            }
        }
        // Controls retain more state than text/pressed (lists, tabs, edits and
        // focus); their full host state keeps all of those repaint triggers.
        for hwnd in controls.window_handles().filter(|&h| visible(h)) {
            format!("{:?}{:?}", controls.get(hwnd), controls.panel(hwnd)).hash(&mut hash);
            if let Some(control) = controls.get(hwnd) {
                for handle in control
                    .list_image_lists
                    .into_iter()
                    .chain([control.tab.image_list])
                {
                    if let Some(list) = self.gdi.image_lists.get(&handle) {
                        (handle, list.generation).hash(&mut hash);
                    }
                }
            }
            if let Some(b) = control_layers.get(&hwnd) {
                b.pixels.hash(&mut hash);
                b.painted.hash(&mut hash);
            }
        }
        controls.focus.hash(&mut hash);
        for hwnd in controls.window_handles().filter(|&h| visible(h)) {
            if let Some(bar) = self.gdi.shell_menus.bars.get(&hwnd) {
                format!("{bar:?}{:?}", self.menu_items.get(&bar.menu)).hash(&mut hash);
            }
        }
        format!("{:?}", self.modal).hash(&mut hash);
        let stamp = hash.finish();
        if self.ce.windows.last_stamp == Some(stamp) {
            return;
        }
        for hwnd in &order {
            let w = &windows[hwnd];
            log::debug!("CE window layer hwnd=0x{hwnd:08x} style=0x{:08x} rect={:?} client={:?}/{:?} title={:?}", w.style, w.rect, w.origin, w.size, w.title);
        }
        let mut caption_font = Font::new(-13);
        for (i, u) in "Tahoma".encode_utf16().enumerate() {
            caption_font.logfont[28 + i * 2..30 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        crate::text::bind_resource(&self.gdi.installed_fonts, &mut caption_font);
        let gdi = &self.gdi;
        let active = self.active_window;
        let pressed = self.ce.windows.pressed;
        let modal = self.modal.clone();
        let before_counter = self.framebuffer.frame_counter;
        let (width, height) = (self.framebuffer.width, self.framebuffer.height);
        {
            let Ok(frame) = self.framebuffer.cpu_mut() else {
                return;
            };
            let mut surface = Surface::Screen(frame);
            surface.fill_rect(
                0,
                0,
                width as i32,
                height as i32,
                crate::framebuffer::colorref_to_rgb565(windowing::system_color(1)),
            );
            for hwnd in order {
                render_window(
                    hwnd,
                    &windows,
                    &control_layers,
                    &controls,
                    gdi,
                    &self.menu_items,
                    &self.menus,
                    &caption_font,
                    active,
                    pressed,
                    &mut surface,
                    0,
                );
            }
            // Template dialogs retain the established dialog renderer and input
            // ABI. They form the topmost layer above ordinary CE application windows.
            for hwnd in controls.panel_handles() {
                if let Some(panel) = controls.panel(hwnd).filter(|p| p.visible) {
                    panel.render(&mut surface);
                    controls.render_owner_with_images(hwnd, &mut surface, gdi, |_, _| {});
                }
            }
            if let Some(hwnd) = self.ce.windows.system_menu {
                if let Some(window) = windows.get(&hwnd) {
                    windowing::draw_system_menu(
                        &mut surface,
                        window.rect,
                        window.style,
                        window.metrics,
                        &caption_font,
                    );
                }
            }
            if let Some(modal) = modal {
                modal.render(&mut surface);
            }
        }
        self.framebuffer.frame_counter = before_counter;
        self.framebuffer.mark_dirty();
        self.ce.windows.last_stamp = Some(stamp);
        for dc in self
            .window_dcs
            .values()
            .chain(self.ce.windows.window_dcs.values())
        {
            if let Some(b) = self.gdi.dc(*dc).and_then(|d| d.selected_bitmap) {
                if let Some(b) = self.gdi.bitmap_mut(b) {
                    b.published_generation = b.paint_generation;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_window(
    hwnd: u32,
    windows: &HashMap<u32, Window>,
    control_layers: &HashMap<u32, Bitmap>,
    controls: &crate::controls::Controls,
    gdi: &crate::gdi::GdiState,
    menus: &HashMap<u32, Vec<crate::MenuItem>>,
    states: &HashMap<u32, HashMap<u32, u32>>,
    font: &Font,
    active: u32,
    pressed: Option<(u32, u32)>,
    s: &mut Surface<'_>,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    let Some(w) = windows.get(&hwnd) else {
        return;
    };
    windowing::draw_window_frame(
        s,
        w.rect,
        w.style,
        w.metrics,
        &w.title,
        hwnd == active,
        pressed.filter(|p| p.0 == hwnd).map(|p| p.1),
        Some(font),
        w.icon.as_ref(),
    );
    if let Some(b) = &w.nonclient {
        s.blit_window_clipped(
            w.rect.0,
            w.rect.1,
            b,
            w.rect.2.max(0) as u32,
            w.rect.3.max(0) as u32,
        );
    }
    windowing::draw_caption_icon(
        s,
        w.rect,
        w.style,
        w.metrics,
        hwnd == active,
        w.icon.as_ref(),
    );
    if w.metrics.menu != 0 {
        let y = w.rect.1 + w.metrics.border + w.metrics.caption;
        let mut x = w.rect.0 + w.metrics.border + 4;
        s.fill_rect(
            x - 4,
            y,
            w.rect.2 - 2 * w.metrics.border,
            w.metrics.menu,
            crate::framebuffer::colorref_to_rgb565(windowing::system_color(4)),
        );
        for (_, text) in &w.menu {
            let layout = crate::text::layout(
                Some(font),
                &text.replace('&', "").encode_utf16().collect::<Vec<_>>(),
            );
            layout.draw(
                s,
                x,
                y + (w.metrics.menu - layout.height) / 2,
                0,
                [
                    x,
                    y,
                    w.rect.0 + w.rect.2 - w.metrics.border,
                    y + w.metrics.menu,
                ],
            );
            x += layout.width + 16;
        }
    }
    let before = s.pixels().to_vec();
    if let Some(b) = &w.bitmap {
        s.blit_window_clipped(
            w.origin.0,
            w.origin.1,
            b,
            w.size.0.max(0) as u32,
            w.size.1.max(0) as u32,
        );
    }
    if let Some(caret) = &w.caret {
        caret.render(s, w.origin);
    }
    let mut children = w.children.clone();
    children.sort_unstable();
    for child in children {
        render_window(
            child,
            windows,
            control_layers,
            controls,
            gdi,
            menus,
            states,
            font,
            active,
            pressed,
            s,
            depth + 1,
        );
    }
    controls.render_owner_tree_with_images(w.hwnd, s, gdi, |child, s| {
        if let Some(b) = control_layers.get(&child) {
            if let Some((x, y, cw, ch)) = controls.screen_rect(child) {
                s.blit_window_clipped(x, y, b, cw.max(0) as u32, ch.max(0) as u32);
            }
        }
        if gdi
            .shell_menus
            .bars
            .get(&child)
            .is_some_and(|b| b.bounds.is_some())
        {
            gdi.shell_menus.render(child, s, gdi, menus, states);
        }
        // CERF SampleApp hosts its native page windows inside a built-in tab.
        // Neither the native-only traversal nor the control-only traversal can
        // cross that boundary without visiting the page at its parent's layer.
        let mut hosted: Vec<_> = windows
            .values()
            .filter(|window| window.parent == Some(child))
            .map(|window| window.hwnd)
            .collect();
        hosted.sort_unstable();
        if !hosted.is_empty() {
            if let Some((x, y, cw, ch)) = controls.screen_rect(child) {
                let before = s.pixels().to_vec();
                for hwnd in hosted {
                    render_window(
                        hwnd,
                        windows,
                        control_layers,
                        controls,
                        gdi,
                        menus,
                        states,
                        font,
                        active,
                        pressed,
                        s,
                        depth + 1,
                    );
                }
                restore_outside_clip(
                    s,
                    &before,
                    [
                        x,
                        y,
                        x.saturating_add(cw.max(0)),
                        y.saturating_add(ch.max(0)),
                    ],
                );
            }
        }
    });
    // Children cannot paint over their parent's caption or outside its client.
    let clip = [
        w.origin.0,
        w.origin.1,
        w.origin.0.saturating_add(w.size.0),
        w.origin.1.saturating_add(w.size.1),
    ];
    restore_outside_clip(s, &before, clip);
}

fn restore_outside_clip(s: &mut Surface<'_>, before: &[u8], clip: [i32; 4]) {
    let (sw, sh) = s.dimensions();
    for y in 0..sh as i32 {
        let row = y as usize * sw as usize * 2;
        if y < clip[1] || y >= clip[3] {
            s.pixels_mut()[row..row + sw as usize * 2]
                .copy_from_slice(&before[row..row + sw as usize * 2]);
        } else {
            let l = clip[0].clamp(0, sw as i32) as usize * 2;
            let r = clip[2].clamp(0, sw as i32) as usize * 2;
            s.pixels_mut()[row..row + l].copy_from_slice(&before[row..row + l]);
            s.pixels_mut()[row + r..row + sw as usize * 2]
                .copy_from_slice(&before[row + r..row + sw as usize * 2]);
        }
    }
}
