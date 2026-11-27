//! Shared EDIT behavior over its existing HWND text and checkpointed state.
use super::{edit_layout::Layout, ChildWindow, ControlAction};
use crate::{
    gdi::{Font, Surface},
    text,
};
impl ChildWindow {
    pub fn edit_geometry(&self, font: Option<&Font>) -> Layout {
        let multiline = self.style & 4 != 0;
        Layout::new(
            &self.text,
            font,
            self.w.saturating_sub(6),
            multiline,
            multiline && self.style & 0x80 == 0,
            &self.edit.tab_stops,
        )
    }
    pub fn edit_caret(&self) -> usize {
        let length = self.text.encode_utf16().count();
        self.edit
            .selection
            .map(|(_, end)| end as usize)
            .unwrap_or(length)
            .min(length)
    }
    pub fn edit_scroll_caret(&mut self, font: Option<&Font>) {
        let layout = self.edit_geometry(font);
        let (x, row) = layout.position(self.edit_caret());
        let visible = (self.h.saturating_sub(2) / layout.height).max(1) as usize;
        let first = (self.edit.first_line as usize).min(layout.lines.len() - 1);
        self.edit.first_line = if row < first {
            row
        } else if row >= first + visible {
            row + 1 - visible
        } else {
            first
        } as u32;
        if self.style & 4 == 0 {
            self.edit.first_line = 0;
        }
        let width = self.w.saturating_sub(6).max(1);
        if x < self.edit.scroll_x {
            self.edit.scroll_x = x;
        } else if x >= self.edit.scroll_x.saturating_add(width) {
            self.edit.scroll_x = x.saturating_sub(width).saturating_add(1);
        }
    }
    pub fn edit_scroll_by(&mut self, horizontal: i32, vertical: i32, font: Option<&Font>) -> bool {
        if self.style & 4 == 0 {
            return false;
        }
        let layout = self.edit_geometry(font);
        self.edit.first_line = (i64::from(self.edit.first_line) + i64::from(vertical))
            .clamp(0, layout.lines.len() as i64 - 1) as u32;
        self.edit.scroll_x = (i64::from(self.edit.scroll_x)
            + i64::from(horizontal) * i64::from(layout.average_width))
        .clamp(0, 0x10000000) as i32;
        true
    }
    pub fn edit_hit(&mut self, x: i32, y: i32, extend: bool, font: Option<&Font>) {
        let layout = self.edit_geometry(font);
        let row = if self.style & 4 != 0 {
            (y.saturating_sub(1).max(0) / layout.height) as usize + self.edit.first_line as usize
        } else {
            0
        };
        let at = layout.hit(row, x.saturating_sub(3).saturating_add(self.edit.scroll_x)) as u32;
        let anchor = if extend {
            self.edit.selection.map(|(a, _)| a).unwrap_or(at)
        } else {
            at
        };
        self.edit.select(&self.text, anchor, at);
        self.edit_scroll_caret(font);
    }
    pub fn edit_navigate(
        &mut self,
        vk: u16,
        shift: bool,
        control: bool,
        font: Option<&Font>,
    ) -> bool {
        let layout = self.edit_geometry(font);
        let caret = self.edit_caret();
        let (x, row) = layout.position(caret);
        let (a, b) = self.edit.range(&self.text);
        let target = match vk {
            0x41 if control => {
                self.edit.select(&self.text, 0, u32::MAX);
                self.edit_scroll_caret(font);
                return true;
            }
            0x25 if !shift && a != b => a as usize,
            0x27 if !shift && a != b => b as usize,
            0x25 => super::edit::previous_boundary(&layout.units, caret),
            0x27 => super::edit::next_boundary(&layout.units, caret),
            0x24 => {
                if control {
                    0
                } else {
                    layout.lines[row].start
                }
            }
            0x23 => {
                if control {
                    layout.units.len()
                } else {
                    layout.lines[row].end
                }
            }
            0x26 => layout.hit(row.saturating_sub(1), x),
            0x28 => layout.hit(row.saturating_add(1), x),
            0x21 => layout.hit(
                row.saturating_sub((self.h / layout.height).max(1) as usize),
                x,
            ),
            0x22 => layout.hit(
                row.saturating_add((self.h / layout.height).max(1) as usize),
                x,
            ),
            _ => return false,
        } as u32;
        let anchor = if shift {
            self.edit
                .selection
                .map(|(start, _)| start)
                .unwrap_or(caret as u32)
        } else {
            target
        };
        self.edit.select(&self.text, anchor, target);
        self.edit_scroll_caret(font);
        true
    }
    pub fn edit_delete(&mut self, backwards: bool, font: Option<&Font>) -> bool {
        if !self.enabled || self.style & 0x800 != 0 {
            return false;
        }
        let old_selection = self.edit.selection;
        let (start, end) = self.edit.range(&self.text);
        if start == end && !backwards {
            let units = self.text.encode_utf16().collect::<Vec<_>>();
            self.edit.select(
                &self.text,
                start,
                super::edit::next_boundary(&units, end as usize) as u32,
            );
        }
        let changed = self.edit.replace(&mut self.text, "", backwards);
        if changed {
            if let Some((_, selection)) = &mut self.edit.undo {
                *selection = old_selection;
            }
            self.edit_scroll_caret(font);
        } else {
            self.edit.selection = old_selection;
        }
        changed
    }
    pub fn edit_changed(&self) -> ControlAction {
        ControlAction::Notify {
            parent: self.parent,
            id: self.id,
            hwnd: self.hwnd,
            code: 0x300,
            index: 0,
        }
    }
    pub(super) fn render_edit_text(
        &self,
        surf: &mut Surface<'_>,
        focused: bool,
        font: Option<&Font>,
    ) {
        let layout = self.edit_geometry(font);
        let clip = [
            self.x + 3,
            self.y + 1,
            self.x + self.w - 2,
            self.y + self.h - 1,
        ];
        let x = self.x + 3 - self.edit.scroll_x;
        let first = (self.edit.first_line as usize).min(layout.lines.len() - 1);
        let top = if self.style & 4 != 0 {
            self.y + 1
        } else {
            self.y + ((self.h - layout.height) / 2).max(1)
        };
        let (selected_start, selected_end) = self.edit.range(&self.text);
        let caret = self.edit_caret();
        let (_, caret_row) = layout.position(caret);
        for (index, line) in layout.lines.iter().enumerate().skip(first) {
            let y = top + (index - first) as i32 * layout.height;
            if y >= clip[3] {
                break;
            }
            let draw = |surf: &mut Surface<'_>, color: u16, region| {
                let mut start = line.start;
                for at in line.start..=line.end {
                    if at == line.end || layout.units[at] == 9 {
                        if at > start {
                            text::layout(font, &layout.units[start..at]).draw(
                                surf,
                                x + line.positions[start - line.start],
                                y,
                                color,
                                region,
                            );
                        }
                        start = at + 1;
                    }
                }
            };
            draw(surf, Self::TEXT, clip);
            if focused && selected_start < selected_end {
                let a = (selected_start as usize).clamp(line.start, line.end) - line.start;
                let b = (selected_end as usize).clamp(line.start, line.end) - line.start;
                if a < b {
                    let left = (x + line.positions[a]).max(clip[0]);
                    let right = (x + line.positions[b]).min(clip[2]);
                    let bottom = (y + layout.height).min(clip[3]);
                    if right > left && bottom > y {
                        surf.fill_rect(left, y, right - left, bottom - y, 0x0010);
                        draw(surf, 0xffff, [left, y, right, bottom]);
                    }
                }
            }
            if focused && index == caret_row {
                let at = caret.min(line.end) - line.start;
                let cx = x + line.positions[at];
                if cx >= clip[0] && cx < clip[2] {
                    surf.fill_rect(cx, y, 1, (y + layout.height).min(clip[3]) - y, Self::TEXT);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::{ControlClass, Controls};
    fn edit(text: &str) -> ChildWindow {
        let mut controls = Controls::default();
        let h = controls.create(
            7,
            ControlClass::Edit,
            10,
            text.into(),
            0x50000084,
            0,
            0,
            70,
            20,
        );
        controls.get(h).unwrap().clone()
    }
    #[test]
    fn editing_respects_direction_readonly_utf16_crlf_and_undo() {
        let mut c = edit("a😀\r\nbc");
        c.edit.select(&c.text, 3, 1);
        assert_eq!(c.edit.range(&c.text), (1, 3));
        assert!(c.edit_delete(false, None));
        assert_eq!(c.text, "a\r\nbc");
        assert!(c.edit.undo(&mut c.text));
        assert_eq!(c.edit.selection, Some((3, 1)));
        c.edit.select(&c.text, 5, 5);
        assert!(c.edit_delete(true, None));
        assert_eq!(c.text, "a😀bc");
        c.edit.undo(&mut c.text);
        c.style |= 0x800;
        let before = c.text.clone();
        assert!(!c.edit_delete(false, None));
        assert_eq!(c.text, before);
        assert!(c.edit_navigate(0x24, false, true, None));
        assert_eq!(c.edit_caret(), 0);
    }
    #[test]
    fn navigation_and_hit_testing_follow_tabs_and_scrolled_lines() {
        let mut c = edit("a\tb\r\ncd\r\nef\r\ngh");
        c.edit.tab_stops = vec![16];
        c.edit_hit(27, 2, false, None);
        assert_eq!(c.edit_caret(), 2);
        c.edit_navigate(0x28, true, false, None);
        assert_eq!(c.edit.range(&c.text), (2, 7));
        assert!(c.edit_scroll_by(0, 100, None));
        assert_eq!(c.edit.first_line, 3);
        c.edit_hit(3, 2, false, None);
        assert_eq!(c.edit_caret(), 13);
        c.edit_scroll_caret(None);
        assert_eq!(c.edit.first_line, 3);
    }
    #[test]
    fn pixels_use_multiline_tab_geometry_and_scroll_without_painting_outside_the_edit() {
        let mut c = edit("A\tB\r\nC\r\nD");
        c.edit.tab_stops = vec![16];
        let mut bm = crate::gdi::Bitmap::new(100, 60);
        for pixel in bm.pixels.chunks_exact_mut(2) {
            pixel.copy_from_slice(&0xf81fu16.to_le_bytes());
        }
        c.render_edit(&mut Surface::Bitmap(&mut bm), false, None);
        let ink = |bm: &crate::gdi::Bitmap, x: usize, y: usize, w: usize, h: usize| {
            (y..y + h).any(|yy| {
                (x..x + w).any(|xx| {
                    u16::from_le_bytes(
                        bm.pixels[(yy * 100 + xx) * 2..(yy * 100 + xx) * 2 + 2]
                            .try_into()
                            .unwrap(),
                    ) == ChildWindow::TEXT
                })
            })
        };
        assert!(ink(&bm, 3, 1, 6, 8));
        assert!(ink(&bm, 27, 1, 6, 8));
        assert!(ink(&bm, 3, 9, 6, 8));
        assert!(!ink(&bm, 10, 1, 16, 8));
        assert_eq!(&bm.pixels[(59 * 100 + 99) * 2..], &0xf81fu16.to_le_bytes());
        let before = bm.pixels.clone();
        c.edit_scroll_by(0, 1, None);
        c.render_edit(&mut Surface::Bitmap(&mut bm), false, None);
        assert_ne!(bm.pixels, before);
        let mut one = edit("abcdef");
        one.style &= !4;
        assert!(!one.edit_scroll_by(2, 1, None));
    }
}
