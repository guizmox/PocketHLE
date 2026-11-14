//! Classic CE non-client geometry and the shared GDI button/edge rasterizer.
//! CerfDemo measures its caption/frame before animating the client bitmap;
//! BlankApp uses AdjustWindowRectEx to reserve that same space for controls.
use crate::{framebuffer::colorref_to_rgb565 as rgb, gdi::Surface};
use std::collections::HashMap;

pub const WS_CAPTION: u32 = 0x00c0_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
pub const CAPTION_HEIGHT: i32 = 20;
pub const MENU_HEIGHT: i32 = 20;
pub const SCROLLBAR: i32 = 16;

/// The classic profile gives PocketPC application windows their requested
/// frame buttons. Portable Explorer omits WS_CAPTION on its browse window;
/// popup desktops and ordinary controls keep their own geometry. Border-only
/// windows such as CE6 Control Panel lay out against SPI_GETWORKAREA and use
/// a command-bar Close button instead of an extra caption inset.
pub fn classic_style(style: u32) -> u32 {
    if style & 0xc000_0000 == 0
        && style & (0x0040_0000 | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX) != 0
        && style & WS_CAPTION != WS_CAPTION
    {
        let resize = if style & (WS_MINIMIZEBOX | WS_MAXIMIZEBOX) != 0 {
            WS_THICKFRAME
        } else {
            0
        };
        style | WS_CAPTION | WS_SYSMENU | resize
    } else {
        style
    }
}

pub fn is_desktop_layer(style: u32, rect: (i32, i32, i32, i32), screen: (u32, u32)) -> bool {
    style & (0x4000_0000 | WS_CAPTION) == 0
        && rect.0 <= 0
        && rect.1 <= 0
        && rect.2 >= screen.0 as i32
        && ((style & 0x8000_0000 != 0 && rect.3 >= screen.1 as i32)
            || (style & 0x0600_0000 == 0x0600_0000
                && rect.3 >= (screen.1 as i32 - crate::shell_menu::HEIGHT).max(0)))
}

#[derive(Debug, Default)]
pub struct ClassicWindows {
    pub enabled: bool,
    pub z_order: Vec<u32>,
    pub restored: HashMap<u32, (i32, i32, i32, i32)>,
    pub window_dcs: HashMap<u32, u32>,
    pub pressed: Option<(u32, u32)>,
    pub drag: Option<Drag>,
    pub system_menu: Option<u32>,
    pub last_stamp: Option<u64>,
}
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub hwnd: u32,
    pub hit: u32,
    pub start: (i32, i32),
    pub bounds: (i32, i32, i32, i32),
}
impl ClassicWindows {
    pub fn raise(&mut self, hwnd: u32, topmost: bool, styles: &HashMap<u32, (u32, u32)>) {
        self.z_order.retain(|&h| h != hwnd);
        let index = if topmost {
            self.z_order.len()
        } else {
            self.z_order
                .iter()
                .position(|h| styles.get(h).is_some_and(|s| s.1 & 8 != 0))
                .unwrap_or(self.z_order.len())
        };
        self.z_order.insert(index, hwnd);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Metrics {
    pub border: i32,
    pub caption: i32,
    pub menu: i32,
    pub client_edge: i32,
    pub vertical_scroll: i32,
    pub horizontal_scroll: i32,
}
impl Metrics {
    pub fn new(style: u32, extended: u32, menu: bool) -> Self {
        let caption = style & WS_CAPTION == WS_CAPTION;
        Self {
            border: if style & WS_THICKFRAME != 0 {
                4
            } else if caption || style & 0x0040_0000 != 0 || extended & 1 != 0 {
                3
            } else if style & 0x0080_0000 != 0 {
                1
            } else {
                0
            },
            caption: if caption {
                if extended & 0x80 != 0 {
                    16
                } else {
                    CAPTION_HEIGHT
                }
            } else {
                0
            },
            menu: if menu { MENU_HEIGHT } else { 0 },
            client_edge: if extended & 0x200 != 0 { 2 } else { 0 },
            vertical_scroll: if style & 0x0020_0000 != 0 {
                SCROLLBAR
            } else {
                0
            },
            horizontal_scroll: if style & 0x0010_0000 != 0 {
                SCROLLBAR
            } else {
                0
            },
        }
    }
    pub fn insets(self) -> [i32; 4] {
        let edge = self.border + self.client_edge;
        [
            edge,
            edge + self.caption + self.menu,
            edge + self.vertical_scroll,
            edge + self.horizontal_scroll,
        ]
    }
    pub fn client_size(self, width: i32, height: i32) -> (i32, i32) {
        let r = self.insets();
        (
            (width.saturating_sub(r[0] + r[2])).max(0),
            (height.saturating_sub(r[1] + r[3])).max(0),
        )
    }
    pub fn adjust(self, r: [i32; 4]) -> [i32; 4] {
        let i = self.insets();
        [
            r[0].saturating_sub(i[0]),
            r[1].saturating_sub(i[1]),
            r[2].saturating_add(i[2]),
            r[3].saturating_add(i[3]),
        ]
    }
}

pub fn system_color(index: i32) -> u32 {
    match index & !0x4000_0000 {
        0 => 0x00c0c0c0,
        1 => 0x00a56e3a,
        2 => 0x00808080,
        3 | 13 => 0x00800000,
        4 | 15 => 0x00c0c0c0,
        5 | 9 | 14 | 20 => 0x00ffffff,
        6 | 7 | 8 | 18 | 23 => 0,
        10 | 11 | 12 => 0x00808080,
        16 | 17 => 0x00808080,
        19 => 0x00c0c0c0,
        21 => 0,
        22 => 0x00dfdfdf,
        24 => 0x00e1ffff,
        _ => 0x00c0c0c0,
    }
}
fn fill(s: &mut Surface<'_>, r: [i32; 4], c: u16) {
    s.fill_rect(
        r[0],
        r[1],
        r[2].saturating_sub(r[0]),
        r[3].saturating_sub(r[1]),
        c,
    );
}
fn inset(r: [i32; 4], n: i32) -> [i32; 4] {
    [
        r[0].saturating_add(n),
        r[1].saturating_add(n),
        r[2].saturating_sub(n),
        r[3].saturating_sub(n),
    ]
}
fn border(s: &mut Surface<'_>, r: [i32; 4], top: u16, bottom: u16, flags: u32) {
    if r[0] >= r[2] || r[1] >= r[3] {
        return;
    }
    if flags & 1 != 0 {
        s.fill_rect(r[0], r[1], 1, r[3] - r[1], top);
    }
    if flags & 2 != 0 {
        s.fill_rect(r[0], r[1], r[2] - r[0], 1, top);
    }
    if flags & 4 != 0 {
        s.fill_rect(r[2] - 1, r[1], 1, r[3] - r[1], bottom);
    }
    if flags & 8 != 0 {
        s.fill_rect(r[0], r[3] - 1, r[2] - r[0], 1, bottom);
    }
}
pub fn draw_edge(s: &mut Surface<'_>, mut r: [i32; 4], edge: u32, flags: u32) -> [i32; 4] {
    for (raised, sunken, outer) in [(1, 2, true), (4, 8, false)] {
        if edge & (raised | sunken) == 0 {
            continue;
        }
        let down = edge & sunken != 0;
        let (a, b) = if flags & 0xc000 != 0 {
            (rgb(system_color(16)), rgb(system_color(16)))
        } else if down {
            (
                rgb(system_color(if outer { 16 } else { 21 })),
                rgb(system_color(if outer { 20 } else { 22 })),
            )
        } else {
            (
                rgb(system_color(if outer { 22 } else { 20 })),
                rgb(system_color(if outer { 21 } else { 16 })),
            )
        };
        border(s, r, a, b, flags);
        if flags & 1 != 0 {
            r[0] = r[0].saturating_add(1);
        }
        if flags & 2 != 0 {
            r[1] = r[1].saturating_add(1);
        }
        if flags & 4 != 0 {
            r[2] = r[2].saturating_sub(1);
        }
        if flags & 8 != 0 {
            r[3] = r[3].saturating_sub(1);
        }
    }
    if flags & 0x800 != 0 {
        fill(s, r, rgb(system_color(15)));
    }
    r
}
pub fn frame_control_supported(kind: u32, state: u32) -> bool {
    match kind {
        1 => state & 0xff <= 4,
        2 => matches!(state & 0xff, 0 | 1 | 2 | 4),
        3 => matches!(state & 0xff, 0 | 1 | 2 | 3 | 5 | 8 | 16),
        4 => matches!(state & 0xff, 0 | 1 | 2 | 4 | 8 | 16),
        _ => false,
    }
}
fn arrow(s: &mut Surface<'_>, x: i32, y: i32, direction: u32, radius: i32, c: u16) {
    for i in 0..=radius {
        for j in -i..=i {
            let (dx, dy) = match direction {
                0 => (j, i),
                1 => (j, -i),
                2 => (i, j),
                _ => (-i, j),
            };
            s.put_pixel(x + dx, y + dy, c);
        }
    }
}
fn check(s: &mut Surface<'_>, x: i32, y: i32, c: u16) {
    s.line((x - 3, y), (x, y + 3), c, 2);
    s.line((x, y + 3), (x + 5, y - 3), c, 2);
}
pub fn draw_frame_control(s: &mut Surface<'_>, rect: [i32; 4], kind: u32, state: u32) {
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return;
    }
    let code = state & 0xff;
    let pushed = state & 0x200 != 0;
    let inactive = state & 0x100 != 0;
    let flat = state & 0xc000 != 0;
    let face = rgb(system_color(15));
    let ink = rgb(system_color(if inactive { 17 } else { 18 }));
    let delta = i32::from(pushed);
    let x = rect[0] + (rect[2] - rect[0]) / 2 + delta;
    let y = rect[1] + (rect[3] - rect[1]) / 2 + delta;
    let radius = ((rect[2] - rect[0]).min(rect[3] - rect[1]) / 4).clamp(0, 6);
    if kind == 2 {
        match code {
            0 | 4 => arrow(s, x + 2, y, 3, radius, ink),
            1 => check(s, x, y, ink),
            2 => s.ellipse([x - 2, y - 2, x + 3, y + 3], Some(ink), None),
            _ => (),
        }
        return;
    }
    if kind == 4 && code != 16 {
        let size = (rect[2] - rect[0]).min(rect[3] - rect[1]).min(13);
        let r = [
            x - size / 2,
            y - size / 2,
            x - size / 2 + size,
            y - size / 2 + size,
        ];
        let background = rgb(system_color(if pushed { 15 } else { 5 }));
        if matches!(code, 1 | 2 | 4) {
            s.ellipse(
                r,
                Some(if code == 2 { 0xffff } else { background }),
                Some((ink, 1)),
            );
            if state & 0x400 != 0 {
                s.ellipse(inset(r, 4), Some(ink), None);
            }
        } else {
            fill(s, r, background);
            draw_edge(s, r, 10, 15 | if flat { 0x4000 } else { 0 });
            if state & 0x400 != 0 {
                check(
                    s,
                    x,
                    y,
                    if code == 8 {
                        rgb(system_color(17))
                    } else {
                        ink
                    },
                );
            }
        }
        return;
    }
    if state & 0x800 == 0 {
        fill(s, rect, face);
    }
    if kind == 3 && matches!(code, 8 | 16) {
        for i in [3, 7, 11] {
            s.line(
                (rect[2] - i, rect[3] - 2),
                (rect[2] - 2, rect[3] - i),
                rgb(system_color(16)),
                1,
            );
            s.line(
                (rect[2] - i + 1, rect[3] - 2),
                (rect[2] - 2, rect[3] - i + 1),
                0xffff,
                1,
            );
        }
        return;
    }
    draw_edge(
        s,
        rect,
        if pushed { 10 } else { 5 },
        15 | if flat { 0x4000 } else { 0 },
    );
    if kind == 4 {
        return;
    }
    if kind == 3 {
        let direction = if code == 5 { 1 } else { code };
        let offset = radius / 2;
        let (ax, ay) = match direction {
            0 => (x, y - offset),
            1 => (x, y + offset),
            2 => (x - offset, y),
            _ => (x + offset, y),
        };
        arrow(s, ax, ay, direction, radius, ink);
        return;
    }
    match code {
        0 => {
            s.line((x - 3, y - 3), (x + 4, y + 4), ink, 2);
            s.line((x - 3, y + 3), (x + 4, y - 4), ink, 2);
        }
        1 => s.fill_rect(x - 4, y + 3, 7, 2, ink),
        2 | 3 => {
            let r = [x - 4, y - 4, x + 4, y + 3];
            if code == 3 {
                s.stroke_rect(x - 1, y - 4, 6, 6, ink);
            }
            s.stroke_rect(r[0], r[1] + i32::from(code == 3) * 2, 8, 7, ink);
            s.fill_rect(r[0], r[1] + i32::from(code == 3) * 2, 8, 2, ink);
        }
        4 => {
            crate::font::draw_str(s, x - 3, y - 4, "?", ink);
        }
        _ => (),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CaptionButton {
    pub hit: u32,
    pub command: u32,
    pub glyph: u32,
    pub rect: [i32; 4],
}
pub fn caption_buttons(rect: (i32, i32, i32, i32), style: u32, m: Metrics) -> Vec<CaptionButton> {
    if m.caption == 0 || style & WS_SYSMENU == 0 {
        return Vec::new();
    }
    let (x, y, w, _) = rect;
    let size = (m.caption - 4).max(1);
    let mut right = x.saturating_add(w).saturating_sub(m.border + 2);
    let top = y.saturating_add(m.border + 2);
    let mut buttons = Vec::new();
    for (allowed, hit, command, glyph) in [
        (true, 20, 0xf060, 0),
        (
            style & WS_MAXIMIZEBOX != 0,
            9,
            if style & 0x0100_0000 != 0 {
                0xf120
            } else {
                0xf030
            },
            if style & 0x0100_0000 != 0 { 3 } else { 2 },
        ),
        (style & WS_MINIMIZEBOX != 0, 8, 0xf020, 1),
    ] {
        if allowed {
            buttons.push(CaptionButton {
                hit,
                command,
                glyph,
                rect: [right - size, top, right, top + size],
            });
            right -= size + if hit == 20 { 3 } else { 1 };
        }
    }
    buttons
}
pub fn contains(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

pub fn hit_test(rect: (i32, i32, i32, i32), style: u32, metrics: Metrics, x: i32, y: i32) -> u32 {
    let (left, top, width, height) = rect;
    let (right, bottom) = (left.saturating_add(width), top.saturating_add(height));
    if !contains([left, top, right, bottom], x, y) {
        return 0;
    }
    if style & WS_THICKFRAME != 0 && style & 0x2100_0000 == 0 {
        let b = metrics.border;
        let corner = (metrics.caption + b).max(12);
        if y < top + b {
            return if x < left + corner {
                13
            } else if x >= right - corner {
                14
            } else {
                12
            };
        }
        if y >= bottom - b {
            return if x < left + corner {
                16
            } else if x >= right - corner {
                17
            } else {
                15
            };
        }
        if x < left + b {
            return if y < top + corner {
                13
            } else if y >= bottom - corner {
                16
            } else {
                10
            };
        }
        if x >= right - b {
            return if y < top + corner {
                14
            } else if y >= bottom - corner {
                17
            } else {
                11
            };
        }
    }
    if metrics.caption != 0
        && y >= top + metrics.border
        && y < top + metrics.border + metrics.caption
    {
        if let Some(button) = caption_buttons(rect, style, metrics)
            .into_iter()
            .find(|b| contains(b.rect, x, y))
        {
            return button.hit;
        }
        if style & WS_SYSMENU != 0 && x < left + metrics.border + 22 {
            return 3;
        }
        return 2;
    }
    let [l, t, r, b] = metrics.insets();
    let client = [left + l, top + t, right - r, bottom - b];
    if contains(client, x, y) {
        return 1;
    }
    if metrics.menu != 0
        && y >= top + metrics.border + metrics.caption
        && y < top + t - metrics.client_edge
    {
        return 5;
    }
    if metrics.vertical_scroll != 0 && x >= client[2] && y >= client[1] && y < client[3] {
        return 7;
    }
    if metrics.horizontal_scroll != 0 && y >= client[3] && x >= client[0] && x < client[2] {
        return 6;
    }
    18
}

pub fn system_menu_items(style: u32) -> Vec<(u32, &'static str, bool)> {
    vec![
        (0xf120, "Restore", style & 0x2100_0000 != 0),
        (0xf010, "Move", style & 0x0100_0000 == 0),
        (
            0xf000,
            "Size",
            style & WS_THICKFRAME != 0 && style & 0x2100_0000 == 0,
        ),
        (
            0xf020,
            "Minimize",
            style & WS_MINIMIZEBOX != 0 && style & 0x2000_0000 == 0,
        ),
        (
            0xf030,
            "Maximize",
            style & WS_MAXIMIZEBOX != 0 && style & 0x0100_0000 == 0,
        ),
        (0xf060, "Close", true),
    ]
}

pub fn system_menu_rect(rect: (i32, i32, i32, i32), metrics: Metrics) -> [i32; 4] {
    let (x, y) = (
        rect.0 + metrics.border,
        rect.1 + metrics.border + metrics.caption,
    );
    [x, y, x + 144, y + 6 * MENU_HEIGHT + 4]
}

pub fn draw_system_menu(
    s: &mut Surface<'_>,
    rect: (i32, i32, i32, i32),
    style: u32,
    metrics: Metrics,
    font: &crate::gdi::Font,
) {
    let r = system_menu_rect(rect, metrics);
    fill(s, r, rgb(system_color(4)));
    draw_edge(s, r, 5, 15);
    for (index, (_, title, enabled)) in system_menu_items(style).into_iter().enumerate() {
        let text = crate::text::layout(Some(font), &title.encode_utf16().collect::<Vec<_>>());
        let y = r[1] + 2 + index as i32 * MENU_HEIGHT;
        text.draw(
            s,
            r[0] + 20,
            y + (MENU_HEIGHT - text.height) / 2,
            rgb(system_color(if enabled { 7 } else { 17 })),
            [r[0] + 2, y, r[2] - 2, y + MENU_HEIGHT],
        );
    }
}

pub fn draw_window_frame(
    s: &mut Surface<'_>,
    rect: (i32, i32, i32, i32),
    style: u32,
    m: Metrics,
    title: &str,
    active: bool,
    pressed: Option<u32>,
    face: Option<&crate::gdi::Font>,
    icon: Option<&crate::imaging::Image>,
) {
    let (x, y, w, h) = rect;
    let r = [x, y, x.saturating_add(w), y.saturating_add(h)];
    if m.border >= 2 {
        draw_edge(s, r, 5, 15);
        for i in 2..m.border {
            border(
                s,
                inset(r, i),
                rgb(system_color(15)),
                rgb(system_color(15)),
                15,
            );
        }
    } else if m.border != 0 {
        border(s, r, 0, 0, 15);
    }
    if m.caption != 0 {
        let top = y + m.border;
        let cap = [x + m.border, top, x + w - m.border, top + m.caption];
        fill(s, cap, rgb(system_color(if active { 3 } else { 2 })));
        let buttons = caption_buttons(rect, style, m);
        let text_right = buttons
            .iter()
            .map(|b| b.rect[0] - 3)
            .min()
            .unwrap_or(cap[2] - 3);
        let mut left = cap[0] + 4;
        if style & WS_SYSMENU != 0 {
            draw_caption_icon(s, rect, style, m, active, icon);
            left += 18;
        }
        let layout = crate::text::layout(face, &title.encode_utf16().collect::<Vec<_>>());
        layout.draw(
            s,
            left,
            top + (m.caption - layout.height) / 2,
            rgb(system_color(if active { 9 } else { 19 })),
            [left, top, text_right, top + m.caption],
        );
        for b in buttons {
            draw_frame_control(
                s,
                b.rect,
                1,
                b.glyph | if pressed == Some(b.hit) { 0x200 } else { 0 },
            );
        }
    }
    if m.client_edge != 0 {
        draw_edge(
            s,
            [
                x + m.border,
                y + m.border + m.caption + m.menu,
                x + w - m.border - m.vertical_scroll,
                y + h - m.border - m.horizontal_scroll,
            ],
            10,
            15,
        );
    }
}

pub fn draw_caption_icon(
    s: &mut Surface<'_>,
    rect: (i32, i32, i32, i32),
    style: u32,
    m: Metrics,
    active: bool,
    icon: Option<&crate::imaging::Image>,
) {
    if m.caption == 0 || style & WS_SYSMENU == 0 {
        return;
    }
    let left = rect.0 + m.border + 4;
    let top = rect.1 + m.border + (m.caption - 16) / 2;
    let bounds = [left, top, left + 16, top + 16];
    fill(s, bounds, rgb(system_color(if active { 3 } else { 2 })));
    if let Some(icon) = icon {
        icon.draw(s, left, top, 16, 16, bounds);
    }
}
