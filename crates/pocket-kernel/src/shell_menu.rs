//! Pocket PC shell command bars. Resource ownership and commands stay with the guest.
use crate::{
    gdi::{GdiState, Surface, STOCK_SYSTEM_FONT},
    text, InputEvent, MenuItem,
};
use std::collections::HashMap;
pub const TRACKING_HANDLE: u32 = 0xfffffff0;
#[derive(Debug, Clone, Copy)]
pub enum MenuCallback {
    Initialize,
    Measure { menu: u32, index: usize },
    Draw { dc: u32, saved: u32 },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupLayout {
    pub menu: u32,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub selected: usize,
    pub rows: Vec<(usize, [i32; 4])>,
}
#[derive(Debug)]
pub struct MenuPaint {
    pub layout: PopupLayout,
    pub items: Vec<MenuItem>,
    pub states: Vec<u32>,
    pub dc: u32,
    pub bitmap: u32,
    pub next: usize,
    pub ready: bool,
}
#[derive(Debug)]
pub struct TrackingMenu {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub owner: u32,
    pub flags: u32,
    pub point: (i32, i32),
    pub result: Option<u32>,
    pub callback: Option<MenuCallback>,
    pub scratch: u32,
    /// Every visible cascade keeps its own guest-drawn pixels. Explorer's
    /// Start menu must remain visible while Programs or Settings is open.
    pub paints: Vec<MenuPaint>,
}

pub const HEIGHT: i32 = 26;
#[derive(Debug, Clone)]
pub struct Bar {
    pub owner: u32,
    pub menu: u32,
    pub sip: bool,
    /// CE command-bar adornment. It sends WM_CLOSE to the bar's owner and
    /// reserves no caption/client inset (the original Control Panel layout).
    pub close_button: bool,
    pub background: u16,
    /// Classic CE command bars have a window rectangle; Mobile bars use the
    /// existing bottom strip when this is None.
    pub bounds: Option<[i32; 4]>,
}
impl Bar {
    pub fn close_rect(&self, bounds: [i32; 4]) -> Option<[i32; 4]> {
        if !self.close_button || self.bounds.is_none() {
            return None;
        }
        let size = (bounds[3] - bounds[1] - 6)
            .min(bounds[2] - bounds[0] - 6)
            .min(18);
        if size <= 0 {
            return None;
        }
        let right = bounds[2] - 3;
        let top = bounds[1] + (bounds[3] - bounds[1] - size) / 2;
        Some([right - size, top, right, top + size])
    }
}
#[derive(Debug, Clone)]
struct Popup {
    menu: u32,
    selected: usize,
    first: usize,
}
#[derive(Debug, Default)]
pub struct ShellMenus {
    pub bars: HashMap<u32, Bar>,
    pub tracking: Option<TrackingMenu>,
    pub taskbar_hidden: bool,
    pub gx_fullscreen: bool,
    pub taskbar_captured: bool,
    pub start_icon_hidden: bool,
    pub sip_button_hidden: bool,
    pub measurements: HashMap<(u32, usize), (u32, u32)>,
    active: u32,
    column: usize,
    path: Vec<Popup>,
    captured: bool,
    close_capture: Option<u32>,
}
#[derive(Debug, PartialEq)]
pub enum Action {
    Consumed,
    Command(u32, u32),
    Close(u32),
    Sip,
    Open(u32, u32),
}
fn label(item: &MenuItem) -> Vec<u16> {
    item.text
        .iter()
        .copied()
        .take_while(|c| *c != 9)
        .filter(|c| *c != 38)
        .collect()
}
fn enabled(item: &MenuItem, menu: u32, states: &HashMap<u32, HashMap<u32, u32>>) -> bool {
    item.kind & 0x800 == 0
        && states
            .get(&menu)
            .and_then(|m| m.get(&item.id))
            .copied()
            .unwrap_or(0)
            & 3
            == 0
}
impl ShellMenus {
    pub fn render_taskbar(
        &self,
        surface: &mut Surface<'_>,
        caption: &str,
        dialog: Option<&crate::controls::DialogPanel>,
    ) {
        let (width, height) = surface.dimensions();
        let width = width as i32;
        for y in 0..HEIGHT {
            let shade = (5 + y / 5) as u16;
            surface.fill_rect(
                0,
                y,
                width,
                1,
                (shade << 11) | ((shade + 7) << 5) | (shade + 12),
            );
        }
        // Decode the original application icon once, then scale at presentation.
        static ICON: std::sync::OnceLock<Option<(u32, u32, Vec<u8>)>> = std::sync::OnceLock::new();
        if let Some((iw, ih, bytes)) = ICON.get_or_init(|| {
            let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
                "../assets/pockethle_logo.png"
            )));
            let mut reader = decoder.read_info().ok()?;
            let mut bytes = vec![0; reader.output_buffer_size()];
            let info = reader.next_frame(&mut bytes).ok()?;
            if info.color_type != png::ColorType::Rgba {
                return None;
            }
            bytes.truncate(info.buffer_size());
            Some((info.width, info.height, bytes))
        }) {
            for y in 0..20u32 {
                for x in 0..20u32 {
                    let i = (((y * ih / 20) * iw + x * iw / 20) * 4) as usize;
                    let alpha = u32::from(bytes[i + 3]);
                    if alpha == 0 {
                        continue;
                    }
                    let px = 5 + x as i32;
                    let py = (HEIGHT - 20) / 2 + y as i32;
                    if px >= width || py >= height as i32 {
                        continue;
                    }
                    let offset = (py as usize * width as usize + px as usize) * 2;
                    let old = u16::from_le_bytes(
                        surface.pixels()[offset..offset + 2].try_into().unwrap(),
                    );
                    let bg = [
                        u32::from(old >> 11) * 255 / 31,
                        u32::from((old >> 5) & 63) * 255 / 63,
                        u32::from(old & 31) * 255 / 31,
                    ];
                    let c: [u8; 3] = std::array::from_fn(|n| {
                        ((u32::from(bytes[i + n]) * alpha + bg[n] * (255 - alpha)) / 255) as u8
                    });
                    surface.put_pixel(px, py, crate::framebuffer::pack_rgb565(c[0], c[1], c[2]));
                }
            }
        }
        let title_font = crate::gdi::Font::new(-12);
        let title = text::layout(
            Some(&title_font),
            &caption.encode_utf16().collect::<Vec<_>>(),
        );
        let title_right = dialog
            .and_then(|panel| panel.help_box().or_else(|| panel.ok_box()))
            .map_or(width - 5, |(left, ..)| left - 4);
        let title_clip = [30, 0, title_right.max(30), HEIGHT];
        title.draw(surface, 30, (HEIGHT - title.height) / 2, 0xffff, title_clip);
        if let Some(dialog) = dialog {
            dialog.render_caption_buttons(surface);
        }
        surface.fill_rect(0, HEIGHT - 1, width, 1, 0x3d5b);
    }

    pub fn close(&mut self) {
        if let Some(t) = self.tracking.as_mut() {
            t.result.get_or_insert(0);
        }
        self.path.clear();
        self.active = 0;
        self.close_capture = None;
    }
    pub fn begin_tracking(&mut self, tracking: TrackingMenu, menu: u32) {
        self.close();
        self.measurements.clear();
        self.bars.insert(
            TRACKING_HANDLE,
            Bar {
                owner: tracking.owner,
                menu: 0,
                sip: false,
                close_button: false,
                background: 0xffff,
                bounds: None,
            },
        );
        self.tracking = Some(tracking);
        self.active = TRACKING_HANDLE;
        self.captured = false;
        self.path.push(Popup {
            menu,
            selected: 0,
            first: 0,
        });
    }
    pub fn active_popup(&self) -> Option<(u32, usize, usize)> {
        self.path.last().map(|p| (p.menu, p.selected, p.first))
    }
    pub fn active_handle(&self) -> Option<u32> {
        (!self.path.is_empty()).then_some(self.active)
    }
    pub fn popup_layout(
        &self,
        width: i32,
        height: i32,
        menus: &HashMap<u32, Vec<MenuItem>>,
    ) -> Option<PopupLayout> {
        self.popup_layouts(width, height, menus).pop()
    }
    pub fn popup_layouts(
        &self,
        width: i32,
        height: i32,
        menus: &HashMap<u32, Vec<MenuItem>>,
    ) -> Vec<PopupLayout> {
        let mut layouts: Vec<PopupLayout> = Vec::new();
        for (depth, p) in self.path.iter().enumerate() {
            let Some(items) = menus.get(&p.menu) else {
                break;
            };
            let border = if self.tracking.is_some() { 2 } else { 0 };
            let available =
                (height - if self.tracking.is_some() { 0 } else { HEIGHT } - border * 2).max(1);
            let custom = self.tracking.is_some() && items.iter().any(|item| item.kind & 0x100 != 0);
            let w = if custom {
                (items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        self.measurements.get(&(p.menu, i)).map_or_else(
                            || (label(item).len() as i32 * 6 + 32).max(64),
                            |m| m.0 as i32,
                        )
                    })
                    .max()
                    .unwrap_or(64)
                    + border * 2)
                    .clamp(1, width.max(1))
            } else if self.tracking.is_some()
                || self
                    .bars
                    .get(&self.active)
                    .is_some_and(|b| b.bounds.is_some())
            {
                (items
                    .iter()
                    .map(|item| text::layout(None, &label(item)).width + 32)
                    .max()
                    .unwrap_or(64)
                    + border * 2)
                    .clamp(1, width.max(1))
            } else {
                width.max(1)
            };
            let mut rows = Vec::new();
            let mut used = 0;
            for (index, _) in items.iter().enumerate().skip(p.first) {
                let row_height = if custom {
                    self.measurements
                        .get(&(p.menu, index))
                        .map_or(20, |m| m.1 as i32)
                        .clamp(1, available)
                } else {
                    20
                };
                if used + row_height > available && !rows.is_empty() {
                    break;
                }
                rows.push((
                    index,
                    [
                        border,
                        border + used,
                        w - border,
                        border + used + row_height,
                    ],
                ));
                used += row_height;
            }
            let total_height = used + border * 2;
            let (left, top) = if depth > 0 {
                let parent = layouts.last().unwrap();
                let row_top = parent
                    .rows
                    .iter()
                    .find(|(index, _)| *index == parent.selected)
                    .map_or(parent.top, |(_, row)| row[1]);
                let right = parent.left + parent.width - border;
                let x = if right + w <= width {
                    right
                } else {
                    parent.left - w + border
                };
                (
                    x.clamp(0, (width - w).max(0)),
                    row_top.clamp(0, (height - total_height).max(0)),
                )
            } else if let Some(t) = &self.tracking {
                let mut x = t.point.0;
                let mut y = t.point.1;
                if t.flags & 8 != 0 {
                    x -= w;
                } else if t.flags & 4 != 0 {
                    x -= w / 2;
                }
                if t.flags & 0x20 != 0 {
                    y -= total_height;
                } else if t.flags & 0x10 != 0 {
                    y -= total_height / 2;
                }
                (
                    x.clamp(0, (width - w).max(0)),
                    y.clamp(0, (height - total_height).max(0)),
                )
            } else {
                if let Some(r) = self.bars.get(&self.active).and_then(|b| b.bounds) {
                    let offset = self
                        .titles(&self.bars[&self.active], menus, r[2] - r[0])
                        .get(self.column)
                        .map_or(0, |t| t.0);
                    (
                        (r[0] + offset).clamp(0, (width - w).max(0)),
                        r[3].min((height - total_height).max(0)).max(0),
                    )
                } else {
                    (0, height - HEIGHT - total_height)
                }
            };
            for (_, r) in &mut rows {
                r[0] += left;
                r[2] += left;
                r[1] += top;
                r[3] += top;
            }
            layouts.push(PopupLayout {
                menu: p.menu,
                left,
                top,
                width: w,
                height: total_height,
                selected: p.selected,
                rows,
            });
        }
        layouts
    }

    fn point_row(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        menus: &HashMap<u32, Vec<MenuItem>>,
    ) -> Option<(usize, usize)> {
        self.popup_layouts(width, height, menus)
            .iter()
            .enumerate()
            .rev()
            .find_map(|(depth, layout)| {
                layout
                    .rows
                    .iter()
                    .find(|(_, r)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
                    .map(|(index, _)| (depth, *index))
            })
    }

    fn select_row(&mut self, depth: usize, index: usize, menus: &HashMap<u32, Vec<MenuItem>>) {
        let p = &self.path[depth];
        let submenu = menus
            .get(&p.menu)
            .and_then(|items| items.get(index))
            .map_or(0, |i| i.submenu);
        if p.selected != index || !self.path.get(depth + 1).is_some_and(|p| p.menu == submenu) {
            self.path.truncate(depth + 1);
        }
        self.path[depth].selected = index;
    }

    fn choose_row(
        &mut self,
        depth: usize,
        menus: &HashMap<u32, Vec<MenuItem>>,
        states: &HashMap<u32, HashMap<u32, u32>>,
    ) -> Action {
        if self.path.len() > depth + 1 {
            self.path.truncate(depth + 2);
            return Action::Consumed;
        }
        self.choose(menus, states)
    }
    fn titles(
        &self,
        bar: &Bar,
        menus: &HashMap<u32, Vec<MenuItem>>,
        width: i32,
    ) -> Vec<(i32, i32)> {
        let items = menus.get(&bar.menu).map(Vec::as_slice).unwrap_or(&[]);
        let available = (width - if bar.sip { 26 } else { 0 }).max(0);
        let desired: Vec<i32> = items
            .iter()
            .map(|i| (label(i).len() as i32 * 6 + 10).max(24))
            .collect();
        let total: i32 = desired.iter().sum();
        let mut x = 0;
        desired
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let end = if i + 1 == desired.len() && bar.bounds.is_none() {
                    available
                } else if total > available {
                    desired[..=i].iter().sum::<i32>() * available / total
                } else {
                    x + d
                };
                let result = (x, end - x);
                x = end;
                result
            })
            .collect()
    }
    fn activate(
        &mut self,
        handle: u32,
        index: usize,
        menus: &HashMap<u32, Vec<MenuItem>>,
        states: &HashMap<u32, HashMap<u32, u32>>,
    ) -> Action {
        let Some(bar) = self.bars.get(&handle) else {
            return Action::Consumed;
        };
        let Some(item) = menus.get(&bar.menu).and_then(|v| v.get(index)) else {
            return Action::Consumed;
        };
        if !enabled(item, bar.menu, states) {
            return Action::Consumed;
        }
        self.active = handle;
        self.column = index;
        self.path.clear();
        if item.submenu != 0 {
            self.path.push(Popup {
                menu: item.submenu,
                selected: 0,
                first: 0,
            });
            Action::Open(bar.owner, item.submenu)
        } else {
            let a = Action::Command(bar.owner, item.id);
            self.close();
            a
        }
    }
    fn choose(
        &mut self,
        menus: &HashMap<u32, Vec<MenuItem>>,
        states: &HashMap<u32, HashMap<u32, u32>>,
    ) -> Action {
        let Some(p) = self.path.last() else {
            return Action::Consumed;
        };
        let Some(i) = menus.get(&p.menu).and_then(|v| v.get(p.selected)) else {
            return Action::Consumed;
        };
        if !enabled(i, p.menu, states) {
            return Action::Consumed;
        }
        let owner = self.bars.get(&self.active).map(|b| b.owner).unwrap_or(0);
        if i.submenu != 0 {
            let menu = i.submenu;
            self.path.push(Popup {
                menu,
                selected: 0,
                first: 0,
            });
            Action::Open(owner, menu)
        } else {
            let a = Action::Command(owner, i.id);
            self.close();
            a
        }
    }
    pub fn input(
        &mut self,
        handle: u32,
        ev: InputEvent,
        width: i32,
        height: i32,
        menus: &HashMap<u32, Vec<MenuItem>>,
        states: &HashMap<u32, HashMap<u32, u32>>,
    ) -> Option<Action> {
        if !self.bars.contains_key(&handle) {
            self.close();
            return None;
        }
        if self.active != handle && self.close_capture != Some(handle) {
            self.close();
        }
        let rows = ((height - HEIGHT) / 20).max(1) as usize;
        let bar_rect = self.bars[&handle]
            .bounds
            .unwrap_or([0, height - HEIGHT, width, height]);
        let in_bar = |x: i32, y: i32| {
            x >= bar_rect[0] && y >= bar_rect[1] && x < bar_rect[2] && y < bar_rect[3]
        };
        let close_rect = self.bars[&handle].close_rect(bar_rect);
        let in_close = |x: u16, y: u16| {
            close_rect.is_some_and(|r| {
                i32::from(x) >= r[0]
                    && i32::from(y) >= r[1]
                    && i32::from(x) < r[2]
                    && i32::from(y) < r[3]
            })
        };
        if matches!(ev, InputEvent::PointerDown { x, y } if in_close(x, y))
            && self.tracking.is_none()
        {
            self.close();
            self.captured = false;
            self.close_capture = Some(handle);
            return Some(Action::Consumed);
        }
        if self.close_capture == Some(handle) {
            match ev {
                InputEvent::PointerUp { x, y } => {
                    self.close_capture = None;
                    return Some(if in_close(x, y) {
                        Action::Close(self.bars[&handle].owner)
                    } else {
                        Action::Consumed
                    });
                }
                InputEvent::PointerMove { .. } => return Some(Action::Consumed),
                InputEvent::PointerDown { .. } => self.close_capture = None,
                _ => {}
            }
        }
        match ev {
            InputEvent::PointerDown { x, y } => {
                let (x, y) = (x as i32, y as i32);
                if self.tracking.is_none() && in_bar(x, y) {
                    self.captured = true;
                    let bar = &self.bars[&handle];
                    if bar.sip && x >= bar_rect[2] - 26 {
                        self.close();
                        return Some(Action::Sip);
                    }
                    let index = self
                        .titles(
                            bar,
                            menus,
                            close_rect.map_or(bar_rect[2], |r| r[0]) - bar_rect[0],
                        )
                        .iter()
                        .position(|(a, w)| x >= bar_rect[0] + *a && x < bar_rect[0] + *a + *w);
                    if index.is_some_and(|i| {
                        self.active == handle && self.column == i && !self.path.is_empty()
                    }) {
                        self.close();
                        return Some(Action::Consumed);
                    }
                    return Some(
                        index
                            .map(|i| self.activate(handle, i, menus, states))
                            .unwrap_or(Action::Consumed),
                    );
                }
                if !self.path.is_empty() {
                    self.captured = true;
                    if let Some((depth, index)) = self.point_row(x, y, width, height, menus) {
                        self.select_row(depth, index, menus);
                        return Some(Action::Consumed);
                    }
                    self.close();
                    return Some(Action::Consumed);
                }
            }
            InputEvent::PointerUp { x, y } if self.captured => {
                self.captured = false;
                if (self.tracking.is_some() || !in_bar(i32::from(x), i32::from(y)))
                    && !self.path.is_empty()
                {
                    if let Some((depth, index)) =
                        self.point_row(i32::from(x), i32::from(y), width, height, menus)
                    {
                        self.select_row(depth, index, menus);
                        return Some(self.choose_row(depth, menus, states));
                    }
                    self.close();
                }
                return Some(Action::Consumed);
            }
            InputEvent::PointerMove { x, y } if !self.path.is_empty() => {
                if let Some((depth, index)) =
                    self.point_row(i32::from(x), i32::from(y), width, height, menus)
                {
                    self.select_row(depth, index, menus);
                    if menus
                        .get(&self.path[depth].menu)
                        .and_then(|items| items.get(index))
                        .is_some_and(|item| item.submenu != 0)
                    {
                        return Some(self.choose_row(depth, menus, states));
                    }
                }
                return Some(Action::Consumed);
            }
            InputEvent::KeyDown { vk } if !self.path.is_empty() => {
                match vk {
                    0x1b if self.path.len() > 1 => {
                        self.path.pop();
                    }
                    0x1b => {
                        self.close();
                    }
                    0x0d | 0x20 => return Some(self.choose(menus, states)),
                    0x25 if self.path.len() > 1 => {
                        self.path.pop();
                    }
                    0x27 if self.tracking.is_some() => {
                        let p = self.path.last().unwrap();
                        if menus
                            .get(&p.menu)
                            .and_then(|items| items.get(p.selected))
                            .is_some_and(|item| item.submenu != 0)
                        {
                            return Some(self.choose(menus, states));
                        }
                    }
                    0x25 | 0x27 if self.tracking.is_none() => {
                        let count = menus.get(&self.bars[&handle].menu).map_or(0, Vec::len);
                        if count > 0 {
                            return Some(self.activate(
                                handle,
                                (self.column + if vk == 0x25 { count - 1 } else { 1 }) % count,
                                menus,
                                states,
                            ));
                        }
                    }
                    0x26 | 0x28 => {
                        let p = self.path.last_mut().unwrap();
                        let count = menus.get(&p.menu).map_or(0, Vec::len);
                        if count > 0 {
                            p.selected =
                                (p.selected + if vk == 0x26 { count - 1 } else { 1 }) % count;
                            p.first = p.first.min(p.selected);
                            if p.selected >= p.first + rows {
                                p.first = p.selected + 1 - rows;
                            }
                        }
                    }
                    _ => {}
                }
                return Some(Action::Consumed);
            }
            InputEvent::KeyUp { .. } if !self.path.is_empty() => return Some(Action::Consumed),
            _ => {}
        }
        None
    }
    pub fn render(
        &self,
        handle: u32,
        surface: &mut Surface<'_>,
        gdi: &GdiState,
        menus: &HashMap<u32, Vec<MenuItem>>,
        states: &HashMap<u32, HashMap<u32, u32>>,
    ) {
        let Some(bar) = self.bars.get(&handle) else {
            return;
        };
        let (w, h) = surface.dimensions();
        let (w, h) = (w as i32, h as i32);
        let bar_rect = bar.bounds.unwrap_or([0, h - HEIGHT, w, h]);
        let y = bar_rect[1];
        let bh = bar_rect[3] - y;
        let bw = bar_rect[2] - bar_rect[0];
        let font = text::selected(gdi, STOCK_SYSTEM_FONT);
        if handle != TRACKING_HANDLE {
            surface.fill_rect(bar_rect[0], y, bw, bh, bar.background);
            surface.fill_rect(bar_rect[0], y, bw, 1, 0x8410);
            if let Some(items) = menus.get(&bar.menu) {
                for (index, (item, (x, width))) in items
                    .iter()
                    .zip(self.titles(
                        bar,
                        menus,
                        bar.close_rect(bar_rect).map_or(bw, |r| r[0] - bar_rect[0]),
                    ))
                    .enumerate()
                {
                    let x = x + bar_rect[0];
                    let active =
                        self.active == handle && self.column == index && !self.path.is_empty();
                    if active {
                        surface.fill_rect(x, y + 1, width, bh - 1, 0x0010);
                    }
                    text::layout(font.as_ref(), &label(item)).draw(
                        surface,
                        x + 5,
                        y + (bh - text::layout(font.as_ref(), &label(item)).height) / 2,
                        if active { 0xffff } else { 0 },
                        [x, y, x + width, bar_rect[3]],
                    );
                }
            }
            if bar.sip {
                // A keyboard glyph, with a separate real SIP hit target.
                let icon_y = y + (HEIGHT - 12) / 2;
                surface.fill_rect(w - 23, icon_y, 19, 12, 0x8410);
                for row in 0..3 {
                    for col in 0..5 {
                        surface.fill_rect(w - 21 + col * 3, icon_y + 2 + row * 3, 2, 2, 0xffff);
                    }
                }
            }
            if let Some(r) = bar.close_rect(bar_rect) {
                let pressed = self.close_capture == Some(handle);
                crate::windowing::draw_edge(surface, r, if pressed { 10 } else { 5 }, 0xf);
                let offset = i32::from(pressed);
                let center = ((r[0] + r[2]) / 2 + offset, (r[1] + r[3]) / 2 + offset);
                for delta in -3..=3 {
                    surface.put_pixel(center.0 + delta, center.1 + delta, 0);
                    surface.put_pixel(center.0 + delta, center.1 - delta, 0);
                }
            }
        }
        if self.active != handle {
            return;
        }
        for layout in self.popup_layouts(w, h, menus) {
            let Some(items) = menus.get(&layout.menu) else {
                continue;
            };
            let (left, top, w) = (layout.left, layout.top, layout.width);
            surface.fill_rect(
                left,
                top,
                w,
                layout.height,
                if self.tracking.is_some() {
                    0xc618
                } else {
                    0xffff
                },
            );
            if let Some(paint) = self.tracking.as_ref().and_then(|t| {
                t.paints
                    .iter()
                    .find(|paint| paint.ready && paint.layout == layout && paint.items == *items)
            }) {
                if let Some(bitmap) = gdi.bitmap(paint.bitmap) {
                    surface.blit_from_bytes(
                        left,
                        top,
                        0,
                        0,
                        bitmap.width as i32,
                        bitmap.height as i32,
                        &bitmap.pixels,
                        bitmap.width,
                        bitmap.height,
                    );
                }
            }
            for &(row, r) in &layout.rows {
                let item = &items[row];
                let yy = r[1];
                let row_height = r[3] - r[1];
                if item.kind & 0x800 != 0 && item.kind & 0x100 == 0 {
                    surface.fill_rect(left + 4, yy + row_height / 2, w - 8, 1, 0x8410);
                    continue;
                }
                let selected = row == layout.selected;
                if selected && item.kind & 0x100 == 0 {
                    surface.fill_rect(r[0], yy, r[2] - r[0], row_height, 0x0010);
                }
                let color = if !enabled(item, layout.menu, states) {
                    0x8410
                } else if selected {
                    0xffff
                } else {
                    0
                };
                if item.kind & 0x100 == 0 {
                    text::layout(font.as_ref(), &label(item)).draw(
                        surface,
                        left + 16,
                        yy + (row_height - text::layout(font.as_ref(), &[]).height) / 2,
                        color,
                        [left + 1, yy, left + w - 12, r[3]],
                    );
                }
                if states
                    .get(&layout.menu)
                    .and_then(|m| m.get(&item.id))
                    .copied()
                    .unwrap_or(0)
                    & 8
                    != 0
                {
                    surface.fill_rect(left + 5, yy + 8, 5, 5, color);
                }
                if item.submenu != 0 {
                    text::layout(font.as_ref(), &[62]).draw(
                        surface,
                        left + w - 10,
                        yy + (row_height - text::layout(font.as_ref(), &[]).height) / 2,
                        color,
                        [left, yy, left + w, r[3]],
                    );
                }
            }
            if self.tracking.is_some() {
                crate::windowing::draw_edge(
                    surface,
                    [left, top, left + w, top + layout.height],
                    5,
                    0xf,
                );
            } else {
                surface.fill_rect(left, top, w, 1, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        ShellMenus,
        HashMap<u32, Vec<MenuItem>>,
        HashMap<u32, HashMap<u32, u32>>,
    ) {
        let mut shell = ShellMenus::default();
        shell.bars.insert(
            1,
            Bar {
                owner: 7,
                menu: 10,
                sip: true,
                close_button: false,
                background: 0xc618,
                bounds: None,
            },
        );
        let item = |id, text: &str, submenu| MenuItem {
            id,
            text: text.encode_utf16().collect(),
            submenu,
            ..Default::default()
        };
        let menus = HashMap::from([
            (10, vec![item(0, "&File", 11)]),
            (11, vec![item(80, "Disabled", 0), item(81, "Nested", 12)]),
            (12, vec![item(82, "Execute", 0)]),
        ]);
        let states = HashMap::from([(11, HashMap::from([(80, 3)]))]);
        (shell, menus, states)
    }
    #[test]
    fn ce_command_bar_close_captures_its_press_cancels_outside_and_keeps_file_menu() {
        let (mut shell, menus, states) = fixture();
        let bar = shell.bars.get_mut(&1).unwrap();
        bar.bounds = Some([1, 1, 319, 25]);
        bar.sip = false;
        bar.close_button = true;
        let rect = bar.close_rect(bar.bounds.unwrap()).unwrap();
        let (x, y) = (
            ((rect[0] + rect[2]) / 2) as u16,
            ((rect[1] + rect[3]) / 2) as u16,
        );
        let send = |shell: &mut ShellMenus, event| shell.input(1, event, 320, 240, &menus, &states);
        assert_eq!(send(&mut shell, InputEvent::PointerUp { x, y }), None);
        assert_eq!(
            send(&mut shell, InputEvent::PointerDown { x, y }),
            Some(Action::Consumed)
        );
        let mut frame = crate::framebuffer::Framebuffer::new(320, 240);
        shell.render(
            1,
            &mut Surface::Screen(&mut frame),
            &GdiState::default(),
            &menus,
            &states,
        );
        let center = ((u32::from(y) + 1) * 320 + u32::from(x) + 1) as usize * 2;
        assert_eq!(&frame.pixels[center..center + 2], &[0, 0]);
        assert_eq!(
            send(&mut shell, InputEvent::PointerMove { x: 100, y: 80 }),
            Some(Action::Consumed)
        );
        assert_eq!(
            send(&mut shell, InputEvent::PointerUp { x: 100, y: 80 }),
            Some(Action::Consumed)
        );
        assert_eq!(
            send(&mut shell, InputEvent::PointerDown { x, y }),
            Some(Action::Consumed)
        );
        assert_eq!(
            send(&mut shell, InputEvent::PointerUp { x, y }),
            Some(Action::Close(7))
        );
        assert_eq!(shell.bars[&1].menu, 10);
        assert_eq!(
            send(&mut shell, InputEvent::PointerDown { x: 10, y: 12 }),
            Some(Action::Open(7, 11))
        );
        assert!(shell.active_popup().is_some());
    }

    #[test]
    fn cascades_stay_visible_and_pointer_navigation_returns_the_guest_command() {
        let (mut shell, menus, states) = fixture();
        shell.begin_tracking(
            TrackingMenu {
                thread: 0,
                thunk: 0,
                frame: crate::GuestCallFrame {
                    args: [0; 4],
                    sp: 0,
                    lr: 0,
                },
                owner: 7,
                flags: 0x100,
                point: (50, 70),
                result: None,
                callback: None,
                scratch: 0,
                paints: Vec::new(),
            },
            11,
        );
        let root = shell.popup_layout(640, 480, &menus).unwrap();
        let row = root.rows[1].1;
        assert_eq!(
            shell.input(
                TRACKING_HANDLE,
                InputEvent::PointerMove {
                    x: (row[0] + 10) as u16,
                    y: (row[1] + 5) as u16,
                },
                640,
                480,
                &menus,
                &states
            ),
            Some(Action::Open(7, 12))
        );
        let layouts = shell.popup_layouts(640, 480, &menus);
        assert_eq!(layouts.len(), 2);
        let child = &layouts[1];
        assert_eq!(child.top, row[1]);
        assert!(child.left > root.left);
        let mut fb = crate::Framebuffer::new(640, 480);
        fb.fill(0x1234);
        shell.render(
            TRACKING_HANDLE,
            &mut Surface::Screen(&mut fb),
            &GdiState::new(),
            &menus,
            &states,
        );
        let pixel = |x: i32, y: i32| {
            let at = (y as usize * 640 + x as usize) * 2;
            u16::from_le_bytes(fb.pixels[at..at + 2].try_into().unwrap())
        };
        assert_eq!(
            pixel(root.left + root.width - 8, root.rows[0].1[1] + 2),
            0xc618
        );
        assert_eq!(
            pixel(child.left + child.width - 8, child.rows[0].1[1] + 2),
            0x0010
        );
        assert_eq!(
            shell.input(
                TRACKING_HANDLE,
                InputEvent::KeyDown { vk: 0x27 },
                640,
                480,
                &menus,
                &states
            ),
            Some(Action::Consumed)
        );
        assert!(shell.tracking.as_ref().unwrap().result.is_none());
        assert_eq!(
            shell.input(
                TRACKING_HANDLE,
                InputEvent::KeyDown { vk: 0x1b },
                640,
                480,
                &menus,
                &states
            ),
            Some(Action::Consumed)
        );
        assert_eq!(shell.active_popup().unwrap().0, 11);
        shell.input(
            TRACKING_HANDLE,
            InputEvent::PointerMove {
                x: (row[0] + 10) as u16,
                y: (row[1] + 5) as u16,
            },
            640,
            480,
            &menus,
            &states,
        );
        let x = (child.rows[0].1[0] + 10) as u16;
        let y = (child.rows[0].1[1] + 5) as u16;
        shell.input(
            TRACKING_HANDLE,
            InputEvent::PointerDown { x, y },
            640,
            480,
            &menus,
            &states,
        );
        assert_eq!(
            shell.input(
                TRACKING_HANDLE,
                InputEvent::PointerUp { x, y },
                640,
                480,
                &menus,
                &states
            ),
            Some(Action::Command(7, 82))
        );
        assert!(shell.popup_layouts(640, 480, &menus).is_empty());
    }

    #[test]
    fn cascade_flips_left_and_fits_the_screen_at_the_bottom_right() {
        let (mut shell, menus, states) = fixture();
        shell.begin_tracking(
            TrackingMenu {
                thread: 0,
                thunk: 0,
                frame: crate::GuestCallFrame {
                    args: [0; 4],
                    sp: 0,
                    lr: 0,
                },
                owner: 7,
                flags: 0,
                point: (638, 478),
                result: None,
                callback: None,
                scratch: 0,
                paints: Vec::new(),
            },
            11,
        );
        shell.input(
            TRACKING_HANDLE,
            InputEvent::KeyDown { vk: 0x28 },
            640,
            480,
            &menus,
            &states,
        );
        shell.input(
            TRACKING_HANDLE,
            InputEvent::KeyDown { vk: 0x27 },
            640,
            480,
            &menus,
            &states,
        );
        let layouts = shell.popup_layouts(640, 480, &menus);
        assert_eq!(layouts.len(), 2);
        assert!(layouts[1].left < layouts[0].left);
        for layout in layouts {
            assert!(layout.left >= 0 && layout.top >= 0);
            assert!(layout.left + layout.width <= 640 && layout.top + layout.height <= 480);
        }
    }
    #[test]
    fn shell_menu_pointer_opens_nested_menu_and_dispatches_real_command() {
        let (mut shell, menus, states) = fixture();
        let mut input = |ev| shell.input(1, ev, 240, 320, &menus, &states);
        assert_eq!(
            input(InputEvent::PointerDown { x: 10, y: 306 }),
            Some(Action::Open(7, 11))
        );
        assert_eq!(
            input(InputEvent::PointerUp { x: 10, y: 306 }),
            Some(Action::Consumed)
        );
        assert_eq!(
            input(InputEvent::PointerDown { x: 40, y: 264 }),
            Some(Action::Consumed)
        );
        assert_eq!(
            input(InputEvent::PointerUp { x: 40, y: 264 }),
            Some(Action::Consumed)
        );
        assert_eq!(
            input(InputEvent::PointerDown { x: 40, y: 283 }),
            Some(Action::Consumed)
        );
        assert_eq!(
            input(InputEvent::PointerUp { x: 40, y: 283 }),
            Some(Action::Open(7, 12))
        );
        assert_eq!(
            input(InputEvent::KeyDown { vk: 13 }),
            Some(Action::Command(7, 82))
        );
        assert_eq!(
            input(InputEvent::PointerDown { x: 230, y: 306 }),
            Some(Action::Sip)
        );
    }
    #[test]
    fn shell_menu_renders_only_reserved_bottom_strip_when_closed() {
        let (shell, menus, states) = fixture();
        let mut fb = crate::Framebuffer::new(240, 320);
        fb.fill(0x1234);
        shell.render(
            1,
            &mut Surface::Screen(&mut fb),
            &GdiState::new(),
            &menus,
            &states,
        );
        let pixel = |x: usize, y: usize| {
            u16::from_le_bytes(
                fb.pixels[(y * 240 + x) * 2..(y * 240 + x) * 2 + 2]
                    .try_into()
                    .unwrap(),
            )
        };
        assert_eq!(pixel(120, 293), 0x1234);
        assert_eq!(pixel(120, 300), 0xc618);
    }
    #[test]
    fn context_menu_ignores_initial_hold_release_and_accepts_next_tap() {
        let (mut shell, menus, states) = fixture();
        shell.begin_tracking(
            TrackingMenu {
                thread: 0,
                thunk: 0,
                frame: crate::GuestCallFrame {
                    args: [0; 4],
                    sp: 0,
                    lr: 0,
                },
                owner: 7,
                flags: 0x100,
                point: (20, 17),
                result: None,
                callback: None,
                scratch: 0,
                paints: Vec::new(),
            },
            12,
        );
        assert_eq!(
            shell.input(
                TRACKING_HANDLE,
                InputEvent::PointerUp { x: 134, y: 14 },
                240,
                320,
                &menus,
                &states
            ),
            None
        );
        assert!(shell.tracking.as_ref().unwrap().result.is_none());
        shell.input(
            TRACKING_HANDLE,
            InputEvent::PointerDown { x: 40, y: 25 },
            240,
            320,
            &menus,
            &states,
        );
        assert_eq!(
            shell.input(
                TRACKING_HANDLE,
                InputEvent::PointerUp { x: 40, y: 25 },
                240,
                320,
                &menus,
                &states
            ),
            Some(Action::Command(7, 82))
        );
    }
    #[test]
    fn second_title_tap_closes_and_next_tap_reopens() {
        let (mut shell, menus, states) = fixture();
        for expected in [Action::Open(7, 11), Action::Consumed, Action::Open(7, 11)] {
            assert_eq!(
                shell.input(
                    1,
                    InputEvent::PointerDown { x: 10, y: 306 },
                    240,
                    320,
                    &menus,
                    &states
                ),
                Some(expected)
            );
            assert_eq!(
                shell.input(
                    1,
                    InputEvent::PointerUp { x: 10, y: 306 },
                    240,
                    320,
                    &menus,
                    &states
                ),
                Some(Action::Consumed)
            );
        }
    }
    #[test]
    fn separator_renders_a_line_and_never_dispatches_a_command() {
        let (mut shell, mut menus, states) = fixture();
        menus.get_mut(&11).unwrap().insert(
            0,
            MenuItem {
                kind: 0x800,
                ..Default::default()
            },
        );
        shell.input(
            1,
            InputEvent::PointerDown { x: 10, y: 306 },
            240,
            320,
            &menus,
            &states,
        );
        shell.input(
            1,
            InputEvent::PointerUp { x: 10, y: 306 },
            240,
            320,
            &menus,
            &states,
        );
        let mut fb = crate::Framebuffer::new(240, 320);
        shell.render(
            1,
            &mut Surface::Screen(&mut fb),
            &GdiState::new(),
            &menus,
            &states,
        );
        let at = (244 * 240 + 30) * 2;
        assert_eq!(
            u16::from_le_bytes(fb.pixels[at..at + 2].try_into().unwrap()),
            0x8410
        );
        shell.input(
            1,
            InputEvent::PointerDown { x: 30, y: 244 },
            240,
            320,
            &menus,
            &states,
        );
        assert_eq!(
            shell.input(
                1,
                InputEvent::PointerUp { x: 30, y: 244 },
                240,
                320,
                &menus,
                &states
            ),
            Some(Action::Consumed)
        );
        assert!(!shell.path.is_empty());
    }
}
