//! Small controller-operated text panel for the resident Gizmondo shell.
//! Application text and address lookup remain guest-owned.
use crate::{font, gdi::Surface};
pub const HWND: u32 = 0xdead6f00;
const KEYS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .-'";
#[derive(Debug, Default)]
pub struct ShellKeyboard {
    pub visible: bool,
    pub requested: bool,
    pub target: u32,
    pub selection: usize,
}
impl ShellKeyboard {
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
        KEYS[self.selection.min(KEYS.len() - 1)] as u32
    }
    pub fn render(&self, surf: &mut Surface<'_>) {
        if !self.visible {
            return;
        }
        let (w, h) = surf.dimensions();
        let w = w as i32;
        let h = h as i32;
        let height = 160.min(h);
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
                    *ch as char,
                    if selected { 0xffff } else { 0 },
                );
            }
        }
        font::draw_str(surf, 3, h - 22, "Play: type  A: delete  B: done", 0xffff);
        font::draw_str(surf, 3, h - 11, "Soft1: field  Soft2: close", 0xffff);
    }
}
