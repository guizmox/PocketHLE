//! Pocket PC shell command bars. Resource ownership and commands stay with the guest.
use crate::{
    gdi::{GdiState, Surface, STOCK_SYSTEM_FONT},
    text, InputEvent, MenuItem,
};
use std::collections::HashMap;
pub const TRACKING_HANDLE: u32 = 0xfffffff0;
#[derive(Debug)]
pub struct TrackingMenu {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub owner: u32,
    pub flags: u32,
    pub point: (i32, i32),
    pub result: Option<u32>,
    pub callback: bool,
}

pub const HEIGHT: i32 = 26;
#[derive(Debug, Clone)]
pub struct Bar {
    pub owner: u32,
    pub menu: u32,
    pub sip: bool,
    pub background: u16,
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
    active: u32,
    column: usize,
    path: Vec<Popup>,
    captured: bool,
}
#[derive(Debug, PartialEq)]
pub enum Action {
    Consumed,
    Command(u32, u32),
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
    pub fn close(&mut self) {
        if let Some(t) = self.tracking.as_mut() {
            t.result.get_or_insert(0);
        }
        self.path.clear();
        self.active = 0;
    }
    pub fn begin_tracking(&mut self, tracking: TrackingMenu, menu: u32) {
        self.close();
        self.bars.insert(
            TRACKING_HANDLE,
            Bar {
                owner: tracking.owner,
                menu: 0,
                sip: false,
                background: 0xffff,
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
    fn popup_rect(&self, width: i32, height: i32, count: usize) -> (i32, i32, i32, usize) {
        let rows =
            ((height - if self.tracking.is_some() { 0 } else { HEIGHT }) / 20).max(1) as usize;
        let shown = count.min(rows);
        if let Some(t) = &self.tracking {
            let w = width.min(220);
            let mut x = t.point.0;
            let mut y = t.point.1;
            if t.flags & 8 != 0 {
                x -= w;
            } else if t.flags & 4 != 0 {
                x -= w / 2;
            }
            if t.flags & 0x20 != 0 {
                y -= shown as i32 * 20;
            } else if t.flags & 0x10 != 0 {
                y -= shown as i32 * 10;
            }
            (
                x.clamp(0, (width - w).max(0)),
                y.clamp(0, (height - shown as i32 * 20).max(0)),
                w,
                shown,
            )
        } else {
            (0, height - HEIGHT - shown as i32 * 20, width, shown)
        }
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
                let end = if i + 1 == desired.len() {
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
        if self.active != handle {
            self.close();
        }
        let rows = ((height - HEIGHT) / 20).max(1) as usize;
        match ev {
            InputEvent::PointerDown { x, y } => {
                let (x, y) = (x as i32, y as i32);
                if self.tracking.is_none() && y >= height - HEIGHT {
                    self.captured = true;
                    let bar = &self.bars[&handle];
                    if bar.sip && x >= width - 26 {
                        self.close();
                        return Some(Action::Sip);
                    }
                    let index = self
                        .titles(bar, menus, width)
                        .iter()
                        .position(|(a, w)| x >= *a && x < *a + *w);
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
                if let Some(p) = self.path.last() {
                    self.captured = true;
                    let count = menus
                        .get(&p.menu)
                        .map_or(0, Vec::len)
                        .saturating_sub(p.first);
                    let (left, top, popup_width, shown) = self.popup_rect(width, height, count);
                    if x >= left
                        && x < left + popup_width
                        && y >= top
                        && y < top + shown as i32 * 20
                    {
                        let p = self.path.last_mut().unwrap();
                        p.selected = p.first + ((y - top) / 20) as usize;
                        return Some(Action::Consumed);
                    }
                    self.close();
                    return Some(Action::Consumed);
                }
            }
            InputEvent::PointerUp { x, y } if self.captured => {
                self.captured = false;
                if (self.tracking.is_some() || (y as i32) < height - HEIGHT)
                    && !self.path.is_empty()
                {
                    let p = self.path.last().unwrap();
                    let count = menus
                        .get(&p.menu)
                        .map_or(0, Vec::len)
                        .saturating_sub(p.first);
                    let (left, top, popup_width, shown) = self.popup_rect(width, height, count);
                    if i32::from(x) >= left
                        && i32::from(x) < left + popup_width
                        && i32::from(y) >= top
                        && i32::from(y) < top + shown as i32 * 20
                    {
                        return Some(self.choose(menus, states));
                    }
                    self.close();
                }
                return Some(Action::Consumed);
            }
            InputEvent::PointerMove { .. } if self.captured => return Some(Action::Consumed),
            InputEvent::KeyDown { vk } if !self.path.is_empty() => {
                match vk {
                    0x1b => {
                        self.close();
                    }
                    0x0d | 0x20 => return Some(self.choose(menus, states)),
                    0x25 if self.path.len() > 1 => {
                        self.path.pop();
                    }
                    0x27 if self.tracking.is_some() => return Some(self.choose(menus, states)),
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
        let y = h - HEIGHT;
        let font = text::selected(gdi, STOCK_SYSTEM_FONT);
        if handle != TRACKING_HANDLE {
            surface.fill_rect(0, y, w, HEIGHT, bar.background);
            surface.fill_rect(0, y, w, 1, 0x8410);
            if let Some(items) = menus.get(&bar.menu) {
                for (index, (item, (x, width))) in
                    items.iter().zip(self.titles(bar, menus, w)).enumerate()
                {
                    let active =
                        self.active == handle && self.column == index && !self.path.is_empty();
                    if active {
                        surface.fill_rect(x, y + 1, width, HEIGHT - 1, 0x0010);
                    }
                    text::layout(font.as_ref(), &label(item)).draw(
                        surface,
                        x + 5,
                        y + 7,
                        if active { 0xffff } else { 0 },
                        [x, y, x + width, h],
                    );
                }
            }
            if bar.sip {
                // A keyboard glyph, with a separate real SIP hit target.
                surface.fill_rect(w - 23, y + 8, 19, 12, 0x8410);
                for row in 0..3 {
                    for col in 0..5 {
                        surface.fill_rect(w - 21 + col * 3, y + 10 + row * 3, 2, 2, 0xffff);
                    }
                }
            }
        }
        if self.active != handle {
            return;
        }
        if let Some(p) = self.path.last() {
            let Some(items) = menus.get(&p.menu) else {
                return;
            };
            let (left, top, w, shown) = self.popup_rect(w, h, items.len().saturating_sub(p.first));
            surface.fill_rect(left, top, w, shown as i32 * 20, 0xffff);
            surface.fill_rect(left, top, w, 1, 0);
            for (row, item) in items.iter().enumerate().skip(p.first).take(shown) {
                let yy = top + (row - p.first) as i32 * 20;
                if item.kind & 0x800 != 0 {
                    surface.fill_rect(left + 4, yy + 10, w - 8, 1, 0x8410);
                    continue;
                }
                let selected = row == p.selected;
                if selected {
                    surface.fill_rect(left + 1, yy + 1, w - 2, 19, 0x0010);
                }
                let color = if !enabled(item, p.menu, states) {
                    0x8410
                } else if selected {
                    0xffff
                } else {
                    0
                };
                text::layout(font.as_ref(), &label(item)).draw(
                    surface,
                    left + 16,
                    yy + 4,
                    color,
                    [left + 1, yy, left + w - 12, yy + 20],
                );
                if states
                    .get(&p.menu)
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
                        yy + 4,
                        color,
                        [left, yy, left + w, yy + 20],
                    );
                }
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
                background: 0xc618,
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
                callback: false,
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
