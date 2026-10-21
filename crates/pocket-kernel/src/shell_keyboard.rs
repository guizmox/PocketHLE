//! Small controller-operated text panel for the resident Gizmondo shell.
//! Application text and address lookup remain guest-owned.
use crate::{font, gdi::Surface};
pub const HWND: u32 = 0xdead6f00;
const KEYS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .-'";
#[derive(Debug, Default)]
pub struct ShellKeyboard {
    pub visible: bool,
    /// WinCE SIP request, independent of the Gizmondo shell protocol.
    pub sip: bool,
    pub pointer_captured: bool,
    pub rotation: u8,
    pub requested: bool,
    pub target: u32,
    pub selection: usize,
    /// Sticky case selection for the PocketPC SIP only.
    pub lowercase: bool,
}
impl ShellKeyboard {
    /// Same geometry as render; last row supplies delete and dismiss.
    pub fn tap(&mut self, x: i32, y: i32, w: i32, h: i32) -> Option<u32> {
        let height = Self::panel_height(h);
        let top = h - height;
        if x < 0 || x >= w || y < top || y >= h {
            return None;
        }
        if y >= h - 28 {
            if self.sip {
                if x < w / 3 {
                    self.lowercase = !self.lowercase;
                    return Some(u32::MAX);
                }
                return Some(if x < w * 2 / 3 { 8 } else { 0 });
            }
            return Some(if x < w / 2 { 8 } else { 0 });
        }
        let col = x / (w / 10).max(1);
        let row = (y - top - 6) / ((height - 35) / 4).max(1);
        if y < top + 6 || col >= 10 || !(0..4).contains(&row) {
            return Some(u32::MAX);
        }
        let i = (row * 10 + col) as usize;
        if i >= KEYS.len() {
            return Some(u32::MAX);
        }
        self.selection = i;
        Some(self.character())
    }
    pub fn navigate(&mut self, vk: u32) {
        let columns = 10;
        self.selection = match vk {
            0x25 => self.selection.saturating_sub(1),
            0x27 => (self.selection + 1).min(KEYS.len() - 1),
            0x26 => self.selection.saturating_sub(columns),
            0x28 => (self.selection + columns).min(KEYS.len() - 1),
            _ => self.selection,
        };
    }
    pub fn character(&self) -> u32 {
        let ch = KEYS[self.selection.min(KEYS.len() - 1)];
        if self.sip && self.lowercase {
            ch.to_ascii_lowercase() as u32
        } else {
            ch as u32
        }
    }
    pub fn panel_height(h: i32) -> i32 {
        (h / 2).clamp(1, 128)
    }
    pub fn display_point(&self, x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
        match self.rotation % 4 {
            1 => (h - 1 - y, x, h, w),
            2 => (w - 1 - x, h - 1 - y, w, h),
            3 => (y, w - 1 - x, h, w),
            _ => (x, y, w, h),
        }
    }
    pub fn render(&self, surf: &mut Surface<'_>) {
        if !self.sip || self.rotation % 4 == 0 {
            self.render_panel(surf);
            return;
        }
        if !self.visible {
            return;
        }
        let (w, h) = surf.dimensions();
        let (_, _, dw, dh) = self.display_point(0, 0, w as i32, h as i32);
        let mut bitmap = crate::gdi::Bitmap::new(dw as u32, dh as u32);
        self.render_panel(&mut Surface::Bitmap(&mut bitmap));
        for y in dh - Self::panel_height(dh)..dh {
            for x in 0..dw {
                let (gx, gy) = match self.rotation % 4 {
                    1 => (y, h as i32 - 1 - x),
                    2 => (w as i32 - 1 - x, h as i32 - 1 - y),
                    _ => (w as i32 - 1 - y, x),
                };
                let i = ((y * dw + x) * 2) as usize;
                surf.put_pixel(
                    gx,
                    gy,
                    u16::from_le_bytes([bitmap.pixels[i], bitmap.pixels[i + 1]]),
                );
            }
        }
    }
    fn render_panel(&self, surf: &mut Surface<'_>) {
        if !self.visible {
            return;
        }
        let (w, h) = surf.dimensions();
        let w = w as i32;
        let h = h as i32;
        let height = if self.sip {
            Self::panel_height(h)
        } else {
            160.min(h)
        };
        let top = h - height;
        let cell_w = (w / 10).max(1);
        let cell_h = ((height - 35) / 4).max(1);
        surf.fill_rect(0, top, w, height, 0x2104);
        for (i, ch) in KEYS.iter().enumerate() {
            let x = (i % 10) as i32 * cell_w;
            let y = top + 6 + (i / 10) as i32 * cell_h;
            let selected = i == self.selection;
            surf.fill_rect(
                x + 1,
                y,
                cell_w - 2,
                cell_h - 2,
                if selected { 0x001f } else { 0xc618 },
            );
            if *ch == b' ' {
                font::draw_str(
                    surf,
                    x + (cell_w - 12) / 2,
                    y + (cell_h - 8) / 2,
                    "SP",
                    if selected { 0xffff } else { 0 },
                );
            } else {
                font::draw_char(
                    surf,
                    x + (cell_w - 6) / 2,
                    y + (cell_h - 8) / 2,
                    if self.sip && self.lowercase {
                        ch.to_ascii_lowercase() as char
                    } else {
                        *ch as char
                    },
                    if selected { 0xffff } else { 0 },
                );
            }
        }
        if self.sip {
            let case_label = if self.lowercase { "ABC" } else { "abc" };
            for (index, label) in [case_label, "Delete", "Done"].into_iter().enumerate() {
                let x = w * index as i32 / 3;
                let button_w = w * (index as i32 + 1) / 3 - x;
                surf.fill_rect(x + 1, h - 27, button_w - 2, 26, 0x8410);
                font::draw_str(
                    surf,
                    x + (button_w - label.len() as i32 * 6) / 2,
                    h - 18,
                    label,
                    0xffff,
                );
            }
            return;
        }
        font::draw_str(surf, 3, h - 22, "Play: type  A: delete  B: done", 0xffff);
        font::draw_str(surf, 3, h - 11, "Soft1: field  Soft2: close", 0xffff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sip_taps_match_key_cells_and_do_not_claim_application_area() {
        let mut k = ShellKeyboard::default();
        assert_eq!(k.tap(10, 100, 240, 320), None);
        assert_eq!(k.tap(10, 202, 240, 320), Some('A' as u32));
        assert_eq!(k.tap(35, 202, 240, 320), Some('B' as u32));
        assert_eq!(k.tap(10, 310, 240, 320), Some(8));
        assert_eq!(k.tap(200, 310, 240, 320), Some(0));
        assert_eq!(k.tap(240, 310, 240, 320), None);
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::*;
    #[test]
    fn every_rotation_maps_the_whole_panel_and_its_keys() {
        for (w, h) in [(240, 320), (320, 240), (480, 640), (480, 800)] {
            for rotation in 0..4 {
                let mut k = ShellKeyboard {
                    sip: true,
                    visible: true,
                    rotation,
                    ..Default::default()
                };
                let (_, _, dw, dh) = k.display_point(0, 0, w, h);
                let dx = dw / 20;
                let dy = dh - ShellKeyboard::panel_height(dh) + 10;
                let (gx, gy) = match rotation {
                    1 => (dy, h - 1 - dx),
                    2 => (w - 1 - dx, h - 1 - dy),
                    3 => (w - 1 - dy, dx),
                    _ => (dx, dy),
                };
                let (x, y, _, _) = k.display_point(gx, gy, w, h);
                assert_eq!((x, y), (dx, dy));
                assert_eq!(k.tap(x, y, dw, dh), Some('A' as u32));
                let mut bitmap = crate::gdi::Bitmap::new(w as u32, h as u32);
                k.render(&mut Surface::Bitmap(&mut bitmap));
                assert!(bitmap.pixels.iter().any(|b| *b != 0));
            }
        }
    }
}

#[cfg(test)]
mod case_tests {
    use super::*;
    #[test]
    fn sip_case_toggle_preserves_selection_digits_and_gizmondo_letters() {
        let mut k = ShellKeyboard {
            sip: true,
            ..Default::default()
        };
        assert_eq!(k.tap(10, 310, 240, 320), Some(u32::MAX));
        assert_eq!(k.tap(10, 202, 240, 320), Some('a' as u32));
        k.selection = 26;
        assert_eq!(k.character(), '0' as u32);
        assert_eq!(k.tap(10, 310, 240, 320), Some(u32::MAX));
        assert_eq!(k.tap(10, 202, 240, 320), Some('A' as u32));
        assert_eq!(k.tap(120, 310, 240, 320), Some(8));
        assert_eq!(k.tap(200, 310, 240, 320), Some(0));
        k.lowercase = true;
        k.sip = false;
        assert_eq!(k.character(), 'A' as u32);
    }
    #[test]
    fn case_button_keeps_its_hit_target_in_every_rotation() {
        for rotation in 0..4 {
            let mut k = ShellKeyboard {
                sip: true,
                rotation,
                ..Default::default()
            };
            let (w, h) = (240, 320);
            let (_, _, dw, dh) = k.display_point(0, 0, w, h);
            let (dx, dy) = (dw / 6, dh - 10);
            let (gx, gy) = match rotation {
                1 => (dy, h - 1 - dx),
                2 => (w - 1 - dx, h - 1 - dy),
                3 => (w - 1 - dy, dx),
                _ => (dx, dy),
            };
            let (x, y, _, _) = k.display_point(gx, gy, w, h);
            assert_eq!(k.tap(x, y, dw, dh), Some(u32::MAX));
            assert_eq!(k.character(), 'a' as u32);
        }
    }
}
