//! A real, modal `MessageBox`.
//!
//! `MessageBoxW` used to be a stub that returned `IDOK` without drawing
//! anything. That is invisible in the common case — an app that only
//! ever says "OK" looks the same either way — but it silently breaks
//! every app that *asks* something. Solitaire's Exit button is one:
//! `WM_COMMAND(1000)` puts up "Are you sure you want to exit?", and the
//! app only calls `PostQuitMessage` when the answer comes back `IDYES`.
//! A stub answering `IDOK` (== 1 == `IDYES`? no — `IDOK` is 1, `IDYES`
//! is 6) meant the comparison never matched and Exit did nothing at all.
//!
//! So this module owns the box for real: its geometry, its Windows CE
//! looks, its buttons, and which of them the stylus hit. The blocking
//! half — keeping the guest parked inside the `MessageBoxW` call while
//! the host keeps drawing frames and feeding input — lives in the
//! `coredll` handler, because only it can re-enter the API thunk.

use crate::controls::{draw_edge, stroke};
use crate::font;
use crate::gdi::Surface;

/// `MB_*` button-set selector, the low nibble of `MessageBox`'s `uType`.
pub mod mb {
    pub const TYPE_MASK: u32 = 0x0000_000F;
    pub const OK: u32 = 0x0000_0000;
    pub const OKCANCEL: u32 = 0x0000_0001;
    pub const ABORTRETRYIGNORE: u32 = 0x0000_0002;
    pub const YESNOCANCEL: u32 = 0x0000_0003;
    pub const YESNO: u32 = 0x0000_0004;
    pub const RETRYCANCEL: u32 = 0x0000_0005;

    /// `MB_ICON*`, the next nibble up.
    pub const ICON_MASK: u32 = 0x0000_00F0;
    pub const ICONSTOP: u32 = 0x0000_0010;
    pub const ICONQUESTION: u32 = 0x0000_0020;
    pub const ICONEXCLAMATION: u32 = 0x0000_0030;
    pub const ICONINFORMATION: u32 = 0x0000_0040;

    /// `MB_DEFBUTTON*` — which button `VK_RETURN` activates.
    pub const DEFMASK: u32 = 0x0000_0F00;
    pub const DEFBUTTON2: u32 = 0x0000_0100;
    pub const DEFBUTTON3: u32 = 0x0000_0200;
}

/// The `ID*` values a `MessageBox` can return.
pub mod id {
    pub const OK: u32 = 1;
    pub const CANCEL: u32 = 2;
    pub const ABORT: u32 = 3;
    pub const RETRY: u32 = 4;
    pub const IGNORE: u32 = 5;
    pub const YES: u32 = 6;
    pub const NO: u32 = 7;
}

/// One button along the bottom of the box.
#[derive(Debug, Clone)]
pub struct MsgBoxButton {
    pub label: String,
    /// What `MessageBoxW` returns when this one is chosen.
    pub result: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub pressed: bool,
}

impl MsgBoxButton {
    fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Which decorative glyph the box carries, from `MB_ICON*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgBoxIcon {
    None,
    Stop,
    Question,
    Exclamation,
    Information,
}

impl MsgBoxIcon {
    fn from_type(u_type: u32) -> Self {
        match u_type & mb::ICON_MASK {
            mb::ICONSTOP => Self::Stop,
            mb::ICONQUESTION => Self::Question,
            mb::ICONEXCLAMATION => Self::Exclamation,
            mb::ICONINFORMATION => Self::Information,
            _ => Self::None,
        }
    }

    /// Side of the square the glyph is drawn in, or `0` for `None`.
    fn side(self) -> i32 {
        if self == Self::None {
            0
        } else {
            MessageBox::ICON
        }
    }
}

/// A modal message box: geometry, text, buttons, and the answer once one
/// has been chosen.
#[derive(Debug, Clone)]
pub struct MessageBox {
    pub caption: String,
    /// Message text, already split into lines that fit the box.
    pub lines: Vec<String>,
    pub icon: MsgBoxIcon,
    pub buttons: Vec<MsgBoxButton>,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Index into [`Self::buttons`] that `VK_RETURN` activates.
    pub default_button: usize,
    /// Set once the user picks a button; the `coredll` handler returns it
    /// from `MessageBoxW` and drops the box.
    pub result: Option<u32>,
    screen: (i32, i32),
    movable: bool,
    drag_offset: Option<(i32, i32)>,
    close_pressed: bool,
}

impl MessageBox {
    /// Caption bar height. Windows CE's is shorter than the desktop's —
    /// there is no minimise/maximise pair, only a close box.
    const CAPTION_H: i32 = 13;
    /// Padding between the frame and its contents.
    const PAD: i32 = 6;
    /// Gap between the icon and the text.
    const ICON_GAP: i32 = 6;
    /// Icon box side.
    const ICON: i32 = 16;
    const BUTTON_H: i32 = 18;
    const BUTTON_MIN_W: i32 = 48;
    const BUTTON_GAP: i32 = 6;

    /// `COLOR_ACTIVECAPTION` — the WinCE shell's dark blue.
    const CAPTION_BG: u16 = 0x0010;
    /// `COLOR_CAPTIONTEXT`.
    const CAPTION_FG: u16 = 0xFFFF;
    const FACE: u16 = 0xC618;
    const SHADOW: u16 = 0x8410;
    const LIGHT: u16 = 0xFFFF;
    const TEXT: u16 = 0x0000;
    /// Icon fills: red for stop, blue-grey for question and info, and
    /// the classic yellow triangle for exclamation.
    const ICON_RED: u16 = 0xC000;
    const ICON_BLUE: u16 = 0x0010;
    const ICON_YELLOW: u16 = 0xFFE0;

    /// Lay a box out for `text` / `caption` on a `screen_w` x `screen_h`
    /// display, centred the way `MessageBox` centres on its owner.
    pub fn new(text: &str, caption: &str, u_type: u32, screen_w: i32, screen_h: i32) -> Self {
        let icon = MsgBoxIcon::from_type(u_type);
        let labels = Self::button_labels(u_type);

        // Text column: wrap to whatever is left after the frame, the
        // icon and the padding, but never wider than most of the screen.
        let max_text_w = (screen_w - 2 * Self::PAD - 2 - icon.side() - Self::ICON_GAP)
            .min(screen_w * 4 / 5)
            .max(font::GLYPH_W * 8);
        let lines = wrap(text, max_text_w);

        let text_w = lines
            .iter()
            .map(|l| font::str_width(l))
            .max()
            .unwrap_or(0)
            .max(font::str_width(caption));
        let text_h = (lines.len() as i32 * (font::GLYPH_H + 2)).max(font::GLYPH_H);

        // Buttons are sized to their labels but share one width, the way
        // Windows sizes a row of them.
        let button_w = labels
            .iter()
            .map(|(label, _)| font::str_width(label) + 16)
            .max()
            .unwrap_or(Self::BUTTON_MIN_W)
            .max(Self::BUTTON_MIN_W);
        let buttons_w =
            button_w * labels.len() as i32 + Self::BUTTON_GAP * (labels.len() as i32 - 1);

        let body_w = (icon.side() + Self::ICON_GAP * i32::from(icon != MsgBoxIcon::None) + text_w)
            .max(buttons_w);
        let w = (body_w + 2 * Self::PAD + 2).min(screen_w);
        let body_h = text_h.max(icon.side());
        let h = Self::CAPTION_H + Self::PAD + body_h + Self::PAD + Self::BUTTON_H + Self::PAD + 2;
        let h = h.min(screen_h);

        let x = ((screen_w - w) / 2).max(0);
        let y = ((screen_h - h) / 2).max(0);

        // Button row, right-aligned as a group and centred as a whole.
        let row_y = y + h - 1 - Self::PAD - Self::BUTTON_H;
        let mut row_x = x + (w - buttons_w) / 2;
        let mut buttons = Vec::with_capacity(labels.len());
        for (label, result) in labels {
            buttons.push(MsgBoxButton {
                label: label.to_string(),
                result,
                x: row_x,
                y: row_y,
                w: button_w,
                h: Self::BUTTON_H,
                pressed: false,
            });
            row_x += button_w + Self::BUTTON_GAP;
        }

        let default_button = match u_type & mb::DEFMASK {
            mb::DEFBUTTON2 => 1,
            mb::DEFBUTTON3 => 2,
            _ => 0,
        }
        .min(buttons.len().saturating_sub(1));

        Self {
            caption: caption.to_string(),
            lines,
            icon,
            buttons,
            x,
            y,
            w,
            h,
            default_button,
            result: None,
            screen: (screen_w, screen_h),
            movable: false,
            drag_offset: None,
            close_pressed: false,
        }
    }

    /// Classic CE composition retains the windows behind the modal, so moving
    /// it can expose their current pixels instead of leaving a painted trail.
    pub fn set_movable(&mut self, movable: bool) {
        self.movable = movable;
        if !movable {
            self.drag_offset = None;
        }
    }

    /// The label/result pairs for an `MB_*` button set.
    fn button_labels(u_type: u32) -> Vec<(&'static str, u32)> {
        match u_type & mb::TYPE_MASK {
            mb::OKCANCEL => vec![("OK", id::OK), ("Cancel", id::CANCEL)],
            mb::ABORTRETRYIGNORE => vec![
                ("Abort", id::ABORT),
                ("Retry", id::RETRY),
                ("Ignore", id::IGNORE),
            ],
            mb::YESNOCANCEL => vec![("Yes", id::YES), ("No", id::NO), ("Cancel", id::CANCEL)],
            mb::YESNO => vec![("Yes", id::YES), ("No", id::NO)],
            mb::RETRYCANCEL => vec![("Retry", id::RETRY), ("Cancel", id::CANCEL)],
            _ => vec![("OK", id::OK)],
        }
    }

    /// The result of the button `VK_RETURN` would press.
    pub fn default_result(&self) -> u32 {
        self.buttons
            .get(self.default_button)
            .map(|b| b.result)
            .unwrap_or(id::OK)
    }

    /// The result of the box's cancel path — `Escape` and the caption's
    /// close box. Windows uses `IDCANCEL` when the set has one and
    /// `IDNO` for `MB_YESNOCANCEL`'s sibling; a box with neither cannot
    /// be dismissed this way, which is why `MB_OK` returns `IDOK`.
    fn cancel_result(&self) -> u32 {
        for want in [id::CANCEL, id::NO] {
            if self.buttons.iter().any(|b| b.result == want) {
                return want;
            }
        }
        self.default_result()
    }

    /// Rectangle of the caption's close box.
    fn close_box(&self) -> (i32, i32, i32, i32) {
        let side = Self::CAPTION_H - 4;
        (self.x + self.w - 2 - side, self.y + 3, side, side)
    }

    /// Stylus down: latch whichever button was hit.
    pub fn pointer_down(&mut self, px: i32, py: i32) {
        self.drag_offset = None;
        let (cx, cy, cw, ch) = self.close_box();
        self.close_pressed = px >= cx && px < cx + cw && py >= cy && py < cy + ch;
        if self.movable
            && !self.close_pressed
            && px >= self.x + 1
            && px < self.x + self.w - 1
            && py >= self.y + 1
            && py < self.y + 1 + Self::CAPTION_H
        {
            self.drag_offset = Some((px - self.x, py - self.y));
        }
        for button in &mut self.buttons {
            button.pressed = self.drag_offset.is_none() && button.contains(px, py);
        }
    }

    /// A held caption drag moves the whole dialog, including its button hit
    /// regions. Keep the box on the logical CE screen so its caption remains
    /// reachable; a move without a caption press does nothing.
    pub fn pointer_move(&mut self, px: i32, py: i32) -> bool {
        let Some((dx, dy)) = self.drag_offset else {
            return false;
        };
        let x = px
            .saturating_sub(dx)
            .clamp(0, (self.screen.0 - self.w).max(0));
        let y = py
            .saturating_sub(dy)
            .clamp(0, (self.screen.1 - self.h).max(0));
        let (dx, dy) = (x - self.x, y - self.y);
        if dx == 0 && dy == 0 {
            return false;
        }
        self.x = x;
        self.y = y;
        for button in &mut self.buttons {
            button.x += dx;
            button.y += dy;
        }
        true
    }

    /// Stylus up. Chooses a button when released inside the one that was
    /// latched, and takes the cancel path on the close box.
    pub fn pointer_up(&mut self, px: i32, py: i32) {
        if self.drag_offset.is_some() {
            self.pointer_move(px, py);
            self.drag_offset = None;
            self.close_pressed = false;
            return;
        }
        let (cx, cy, cw, ch) = self.close_box();
        let close = self.close_pressed && px >= cx && px < cx + cw && py >= cy && py < cy + ch;
        self.close_pressed = false;
        if close {
            self.result = Some(self.cancel_result());
            return;
        }
        let mut chosen = None;
        for button in &mut self.buttons {
            if button.pressed && button.contains(px, py) {
                chosen = Some(button.result);
            }
            button.pressed = false;
        }
        if let Some(result) = chosen {
            self.result = Some(result);
        }
    }

    /// `VK_RETURN` takes the default button, `VK_ESCAPE` the cancel path.
    pub fn key_down(&mut self, vk: u16) {
        const VK_RETURN: u16 = 0x0D;
        const VK_ESCAPE: u16 = 0x1B;
        match vk {
            VK_RETURN => self.result = Some(self.default_result()),
            VK_ESCAPE => self.result = Some(self.cancel_result()),
            _ => {}
        }
    }

    /// Is `(px, py)` inside the box? A modal swallows every tap, but the
    /// host still wants to know whether one landed on the box itself.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// Paint the box over whatever is already on the surface.
    pub fn render(&self, surf: &mut Surface<'_>) {
        // Frame: black outline, raised face inside it.
        stroke(surf, self.x, self.y, self.w, self.h, Self::TEXT);
        let (ix, iy, iw, ih) = (self.x + 1, self.y + 1, self.w - 2, self.h - 2);
        if iw <= 0 || ih <= 0 {
            return;
        }
        surf.fill_rect(ix, iy, iw, ih, Self::FACE);
        draw_edge(surf, ix, iy, iw, ih, Self::LIGHT, Self::SHADOW);

        // Caption bar.
        surf.fill_rect(ix, iy, iw, Self::CAPTION_H, Self::CAPTION_BG);
        let caption_y = iy + (Self::CAPTION_H - font::GLYPH_H) / 2;
        font::draw_str(
            surf,
            ix + 3,
            caption_y,
            &clip_to(&self.caption, iw - 6 - Self::CAPTION_H),
            Self::CAPTION_FG,
        );
        self.render_close_box(surf);

        // Icon and message.
        let body_y = iy + Self::CAPTION_H + Self::PAD;
        let mut text_x = ix + Self::PAD;
        if self.icon != MsgBoxIcon::None {
            self.render_icon(surf, text_x, body_y);
            text_x += Self::ICON + Self::ICON_GAP;
        }
        for (n, line) in self.lines.iter().enumerate() {
            let ly = body_y + n as i32 * (font::GLYPH_H + 2);
            font::draw_str(surf, text_x, ly, line, Self::TEXT);
        }

        for (n, button) in self.buttons.iter().enumerate() {
            self.render_button(surf, button, n == self.default_button);
        }
    }

    fn render_close_box(&self, surf: &mut Surface<'_>) {
        let (cx, cy, cw, ch) = self.close_box();
        surf.fill_rect(cx, cy, cw, ch, Self::FACE);
        let (light, shadow) = if self.close_pressed {
            (Self::SHADOW, Self::LIGHT)
        } else {
            (Self::LIGHT, Self::SHADOW)
        };
        draw_edge(surf, cx, cy, cw, ch, light, shadow);
        // The X, drawn as two diagonals inside the box.
        for i in 2..cw - 2 {
            surf.put_pixel(cx + i, cy + i, Self::TEXT);
            surf.put_pixel(cx + cw - 1 - i, cy + i, Self::TEXT);
        }
    }

    fn render_button(&self, surf: &mut Surface<'_>, button: &MsgBoxButton, is_default: bool) {
        let (mut x, mut y, mut w, mut h) = (button.x, button.y, button.w, button.h);
        if is_default {
            stroke(surf, x, y, w, h, Self::TEXT);
            x += 1;
            y += 1;
            w -= 2;
            h -= 2;
            if w <= 0 || h <= 0 {
                return;
            }
        }
        surf.fill_rect(x, y, w, h, Self::FACE);
        let (tl, br) = if button.pressed {
            (Self::SHADOW, Self::LIGHT)
        } else {
            (Self::LIGHT, Self::SHADOW)
        };
        draw_edge(surf, x, y, w, h, tl, br);
        let nudge = i32::from(button.pressed);
        let tx = x + ((w - font::str_width(&button.label)) / 2).max(1) + nudge;
        let ty = y + ((h - font::GLYPH_H) / 2).max(0) + nudge;
        font::draw_str(surf, tx, ty, &button.label, Self::TEXT);
    }

    /// The `MB_ICON*` glyphs, drawn rather than blitted: a filled circle
    /// for stop / question / information and a triangle for exclamation,
    /// each with its symbol punched out in white.
    fn render_icon(&self, surf: &mut Surface<'_>, x: i32, y: i32) {
        let side = Self::ICON;
        let (fill, symbol) = match self.icon {
            MsgBoxIcon::Stop => (Self::ICON_RED, "X"),
            MsgBoxIcon::Question => (Self::ICON_BLUE, "?"),
            MsgBoxIcon::Exclamation => (Self::ICON_YELLOW, "!"),
            MsgBoxIcon::Information => (Self::ICON_BLUE, "i"),
            MsgBoxIcon::None => return,
        };
        if self.icon == MsgBoxIcon::Exclamation {
            // Triangle: each row from the apex widens by one pixel a
            // side, which at 16 px reads as the warning sign.
            for row in 0..side {
                let half = (row + 1) / 2;
                let cx = x + side / 2;
                surf.fill_rect(cx - half, y + row, half * 2 + 1, 1, fill);
            }
            let sym_x = x + (side - font::GLYPH_W) / 2;
            font::draw_str(
                surf,
                sym_x,
                y + side - font::GLYPH_H - 1,
                symbol,
                Self::TEXT,
            );
            return;
        }
        // Circle by radius test — cheap and exact enough at this size.
        let r = side / 2;
        for row in 0..side {
            for col in 0..side {
                let (dx, dy) = (col - r, row - r);
                if dx * dx + dy * dy <= r * r {
                    surf.put_pixel(x + col, y + row, fill);
                }
            }
        }
        let sym_x = x + (side - font::GLYPH_W) / 2;
        let sym_y = y + (side - font::GLYPH_H) / 2;
        font::draw_str(surf, sym_x, sym_y, symbol, Self::CAPTION_FG);
    }
}

/// Truncate `text` to whatever fits in `width` pixels.
fn clip_to(text: &str, width: i32) -> String {
    let max_chars = (width / font::GLYPH_W).max(0) as usize;
    text.chars().take(max_chars).collect()
}

/// Word-wrap `text` to `width` pixels, honouring the `\n` and `\r\n` an
/// app puts in its own message.
fn wrap(text: &str, width: i32) -> Vec<String> {
    let max_chars = (width / font::GLYPH_W).max(1) as usize;
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim_end_matches('\r');
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split(' ') {
            // A word longer than the column has to be broken, or it
            // would run out of the box.
            if word.chars().count() > max_chars {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let mut chunk = String::new();
                for ch in word.chars() {
                    if chunk.chars().count() == max_chars {
                        lines.push(std::mem::take(&mut chunk));
                    }
                    chunk.push(ch);
                }
                line = chunk;
                continue;
            }
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if candidate.chars().count() > max_chars {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gdi::Bitmap;

    const SCREEN: (i32, i32) = (480, 240);

    fn box_of(u_type: u32) -> MessageBox {
        MessageBox::new("Are you sure?", "Solitaire", u_type, SCREEN.0, SCREEN.1)
    }

    #[test]
    fn a_yesno_box_offers_yes_and_no_and_returns_their_ids() {
        let mb = box_of(mb::YESNO);
        let labels: Vec<&str> = mb.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, vec!["Yes", "No"]);
        assert_eq!(mb.buttons[0].result, id::YES);
        assert_eq!(mb.buttons[1].result, id::NO);
    }

    #[test]
    fn each_button_set_maps_to_its_own_ids() {
        let ok = box_of(mb::OK);
        assert_eq!(ok.buttons.len(), 1);
        assert_eq!(ok.buttons[0].result, id::OK);

        let ync = box_of(mb::YESNOCANCEL);
        let results: Vec<u32> = ync.buttons.iter().map(|b| b.result).collect();
        assert_eq!(results, vec![id::YES, id::NO, id::CANCEL]);

        let ari = box_of(mb::ABORTRETRYIGNORE);
        let results: Vec<u32> = ari.buttons.iter().map(|b| b.result).collect();
        assert_eq!(results, vec![id::ABORT, id::RETRY, id::IGNORE]);
    }

    #[test]
    fn tapping_a_button_yields_its_id_only_on_release_inside_it() {
        let mut mb = box_of(mb::YESNO);
        let no = mb.buttons[1].clone();
        let (cx, cy) = (no.x + no.w / 2, no.y + no.h / 2);

        mb.pointer_down(cx, cy);
        assert!(mb.buttons[1].pressed);
        assert_eq!(mb.result, None, "still held down");

        // Released somewhere else: the press is abandoned, as Win32 does.
        mb.pointer_up(mb.x + 2, mb.y + mb.h - 2);
        assert_eq!(mb.result, None);
        assert!(!mb.buttons[1].pressed);

        mb.pointer_down(cx, cy);
        mb.pointer_up(cx, cy);
        assert_eq!(mb.result, Some(id::NO));
    }

    #[test]
    fn enter_takes_the_default_button_and_escape_cancels() {
        let mut mb = box_of(mb::YESNO);
        mb.key_down(0x0D);
        assert_eq!(mb.result, Some(id::YES), "first button is the default");

        // MB_DEFBUTTON2 moves it to No.
        let mut mb = box_of(mb::YESNO | mb::DEFBUTTON2);
        assert_eq!(mb.default_button, 1);
        mb.key_down(0x0D);
        assert_eq!(mb.result, Some(id::NO));

        // Escape prefers IDCANCEL, then IDNO.
        let mut mb = box_of(mb::YESNOCANCEL);
        mb.key_down(0x1B);
        assert_eq!(mb.result, Some(id::CANCEL));
        let mut mb = box_of(mb::YESNO);
        mb.key_down(0x1B);
        assert_eq!(mb.result, Some(id::NO));
    }

    #[test]
    fn the_close_box_cancels() {
        let mut mb = box_of(mb::OKCANCEL);
        let (cx, cy, cw, ch) = mb.close_box();
        mb.pointer_down(cx + cw / 2, cy + ch / 2);
        mb.pointer_up(cx + cw / 2, cy + ch / 2);
        assert_eq!(mb.result, Some(id::CANCEL));
    }

    #[test]
    fn caption_drag_moves_button_hit_regions_without_answering_the_modal() {
        let mut mb = box_of(mb::YESNO);
        mb.set_movable(true);
        let original = (mb.x, mb.y);
        let old_no = mb.buttons[1].clone();
        mb.pointer_down(mb.x + 10, mb.y + 5);
        assert!(mb.pointer_move(original.0 + 90, original.1 + 45));
        assert_eq!((mb.x, mb.y), (original.0 + 80, original.1 + 40));
        assert_eq!(
            (mb.buttons[1].x, mb.buttons[1].y),
            (old_no.x + 80, old_no.y + 40)
        );
        mb.pointer_up(original.0 + 90, original.1 + 45);
        assert_eq!(mb.result, None);
        assert!(!mb.pointer_move(0, 0));
        mb.pointer_down(old_no.x + 2, old_no.y + 2);
        mb.pointer_up(old_no.x + 2, old_no.y + 2);
        assert_eq!(mb.result, None);
        let no = mb.buttons[1].clone();
        mb.pointer_down(no.x + no.w / 2, no.y + no.h / 2);
        mb.pointer_up(no.x + no.w / 2, no.y + no.h / 2);
        assert_eq!(mb.result, Some(id::NO));
    }

    #[test]
    fn caption_drag_stays_on_screen_and_cannot_turn_its_release_into_close() {
        let mut mb = box_of(mb::OKCANCEL);
        mb.set_movable(true);
        mb.pointer_down(mb.x + 10, mb.y + 5);
        mb.pointer_move(i32::MIN, i32::MIN);
        assert_eq!((mb.x, mb.y), (0, 0));
        mb.pointer_move(i32::MAX, i32::MAX);
        assert_eq!((mb.x + mb.w, mb.y + mb.h), SCREEN);
        let (cx, cy, cw, ch) = mb.close_box();
        mb.pointer_up(cx + cw / 2, cy + ch / 2);
        assert_eq!(mb.result, None);
        let (cx, cy, cw, ch) = mb.close_box();
        mb.pointer_down(cx + cw / 2, cy + ch / 2);
        assert!(!mb.pointer_move(0, 0));
        mb.pointer_up(cx + cw / 2, cy + ch / 2);
        assert_eq!(mb.result, Some(id::CANCEL));
    }

    #[test]
    fn client_and_unpressed_close_releases_neither_move_nor_dismiss_the_box() {
        let mut mb = box_of(mb::OKCANCEL);
        mb.set_movable(true);
        let original = (mb.x, mb.y);
        mb.pointer_down(mb.x + 2, mb.y + MessageBox::CAPTION_H + 4);
        assert!(!mb.pointer_move(0, 0));
        let (cx, cy, cw, ch) = mb.close_box();
        mb.pointer_up(cx + cw / 2, cy + ch / 2);
        assert_eq!((mb.x, mb.y), original);
        assert_eq!(mb.result, None);
    }

    #[test]
    fn the_box_is_centred_and_fits_the_screen() {
        let mb = box_of(mb::YESNO);
        assert!(mb.w <= SCREEN.0 && mb.h <= SCREEN.1);
        assert!(mb.x >= 0 && mb.y >= 0);
        assert_eq!(mb.x + mb.w / 2, SCREEN.0 / 2, "horizontally centred");
        // Every button sits inside the frame.
        for button in &mb.buttons {
            assert!(button.x >= mb.x && button.x + button.w <= mb.x + mb.w);
            assert!(button.y >= mb.y && button.y + button.h <= mb.y + mb.h);
        }
    }

    #[test]
    fn a_long_message_wraps_instead_of_running_off_the_box() {
        let long = "This deal cannot be completed because the deck has \
                    run out of cards to turn over, which should not happen.";
        let mb = MessageBox::new(long, "Solitaire", mb::OK, SCREEN.0, SCREEN.1);
        assert!(mb.lines.len() > 1, "wrapped onto several lines");
        assert!(mb.w <= SCREEN.0);
        for line in &mb.lines {
            assert!(
                font::str_width(line) <= mb.w,
                "line {line:?} is wider than the box"
            );
        }
    }

    #[test]
    fn explicit_newlines_are_honoured() {
        let mb = MessageBox::new("First\nSecond", "T", mb::OK, SCREEN.0, SCREEN.1);
        assert_eq!(mb.lines, vec!["First", "Second"]);
    }

    #[test]
    fn a_word_longer_than_the_column_is_broken_up() {
        let mb = MessageBox::new(&"z".repeat(400), "T", mb::OK, SCREEN.0, SCREEN.1);
        assert!(mb.lines.len() > 1);
        for line in &mb.lines {
            assert!(font::str_width(line) <= mb.w);
        }
    }

    #[test]
    fn rendering_paints_the_caption_bar_and_a_raised_face() {
        let mut bmp = Bitmap::new(SCREEN.0 as u32, SCREEN.1 as u32);
        // Fill with something that is neither the face nor the caption
        // so "we drew here" is a real observation.
        {
            let mut surf = Surface::Bitmap(&mut bmp);
            surf.fill_rect(0, 0, SCREEN.0, SCREEN.1, 0x07E0);
        }
        let mb = box_of(mb::YESNO | mb::ICONQUESTION);
        {
            let mut surf = Surface::Bitmap(&mut bmp);
            mb.render(&mut surf);
        }
        let at = |x: i32, y: i32| -> u16 {
            let i = (y as usize * SCREEN.0 as usize + x as usize) * 2;
            let px = &bmp.pixels;
            u16::from_le_bytes([px[i], px[i + 1]])
        };

        // Caption bar is the shell blue, the body below it the face.
        // Probed left of where the caption text starts and above its
        // first row, so a lit glyph pixel cannot be mistaken for the bar.
        assert_eq!(at(mb.x + 2, mb.y + 2), MessageBox::CAPTION_BG);
        // And the caption really was drawn: somewhere along its baseline
        // there is foreground.
        assert!(
            (mb.x + 4..mb.x + 4 + font::str_width("Solitaire"))
                .any(|x| at(x, mb.y + 5) == MessageBox::CAPTION_FG),
            "caption text is painted in the bar"
        );
        assert_eq!(
            at(mb.x + 3, mb.y + MessageBox::CAPTION_H + 4),
            MessageBox::FACE,
            "body is the button face"
        );
        // The frame is a black outline.
        assert_eq!(at(mb.x, mb.y), MessageBox::TEXT);
        // Outside the box the backdrop survives — a modal is not a
        // full-screen wipe.
        assert_eq!(at(1, 1), 0x07E0);
    }
}
