//! Built-in `BUTTON`, `EDIT` and `STATIC` window controls.
//!
//! On a real device these come from the OS: `CreateWindowExW` with a
//! system class name hands back a window whose procedure lives inside
//! `coredll`. That procedure owns the control's pixels, keeps its text,
//! tracks focus, and turns stylus taps into `WM_COMMAND` notifications
//! for the parent — the application never draws a button itself.
//!
//! PocketHLE had no such procedure, so every child collapsed onto the
//! application's own `HWND` and `WndProc`. That was not merely a
//! cosmetic gap: because the child reused the parent's handle it also
//! re-entered the parent's `WM_CREATE`, which creates the children, so
//! CERF BlankApp looped creating `BUTTON`/`BUTTON`/`EDIT` forever and
//! never reached its first `WM_PAINT`.
//!
//! This module models the controls instead. Each one gets its own
//! handle, its own rectangle in parent-client coordinates, and paints
//! itself after the parent's `WM_PAINT` — the same "control owns its
//! pixels" reasoning as [`crate::StatusBar`].

use crate::font;
use crate::gdi::Surface;

/// Which built-in window class a control was created from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlClass {
    Button,
    Edit,
    Static,
    ListView,
    ListBox,
    ComboBox,
    TrackBar,
}

impl ControlClass {
    /// Match a `CreateWindowExW` class name against the built-in
    /// classes. Win32 class names are case-insensitive, and apps are
    /// inconsistent about it — CERF BlankApp passes `BUTTON` and
    /// `EDIT`, MFC-generated code tends to pass `Button` / `Edit`.
    pub fn from_class_name(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("BUTTON") {
            Some(Self::Button)
        } else if name.eq_ignore_ascii_case("EDIT") {
            Some(Self::Edit)
        } else if name.eq_ignore_ascii_case("STATIC") {
            Some(Self::Static)
        } else if name.eq_ignore_ascii_case("LISTBOX") {
            Some(Self::ListBox)
        } else if name.eq_ignore_ascii_case("COMBOBOX") {
            Some(Self::ComboBox)
        } else if name.eq_ignore_ascii_case("msctls_trackbar32") {
            Some(Self::TrackBar)
        } else if name.eq_ignore_ascii_case("SysListView32") {
            Some(Self::ListView)
        } else {
            None
        }
    }
}

/// `BS_*` button type, held in the low nibble of a `BUTTON`'s style.
const BS_TYPE_MASK: u32 = 0x000F;
const BS_DEFPUSHBUTTON: u32 = 0x0001;
const BS_CHECKBOX: u32 = 0x0002;
const BS_AUTOCHECKBOX: u32 = 0x0003;
const BS_RADIOBUTTON: u32 = 0x0004;
/// `BS_GROUPBOX` — not a button at all: an etched frame with its
/// caption let into the top-left of the border. Solitaire's Options
/// dialog uses three (Draw, Scoring, Card back), each enclosing the
/// radio buttons it labels.
const BS_GROUPBOX: u32 = 0x0007;
const BS_AUTORADIOBUTTON: u32 = 0x0009;

/// `IDOK`, reported when the caption bar's OK box is tapped.
pub const IDOK: u32 = 1;
/// Standard cancel command understood by the dialog manager.
pub const IDCANCEL: u32 = 2;

/// `WS_VISIBLE`.
const WS_VISIBLE: u32 = 0x1000_0000;
/// `WS_DISABLED`.
const WS_DISABLED: u32 = 0x0800_0000;

/// A dialog window that owns controls, positioned in screen space.
///
/// Dialogs built from a `DLGTEMPLATE` are containers: the template gives
/// the panel a rectangle and every item a rectangle *inside* it. Keeping
/// the panel here — rather than flattening its children to screen
/// coordinates at creation — is what lets the application move the whole
/// thing afterwards, which Solitaire does: it reads the panel's width
/// back with `GetWindowRect` and `SetWindowPos`es it against the right
/// edge of the screen.
#[derive(Debug, Clone)]
pub struct DialogPanel {
    pub hwnd: u32,
    /// Top-left of the *window*, in screen coordinates. Children are
    /// laid out relative to the client area just inside the border.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub visible: bool,
    /// `WS_BORDER` — the one-pixel black frame around the panel.
    pub border: bool,
    /// Title shown in the caption bar, for a dialog that has one.
    ///
    /// `None` is a bare panel — Solitaire's button strip is a
    /// `WS_CHILD` dialog with no caption, and drawing one over the card
    /// table would be wrong. `Some` adds the Windows CE chrome: a
    /// coloured bar above the client area carrying the OK box.
    pub caption: Option<String>,
    /// Is the OK box currently held down?
    pub ok_pressed: bool,
}

impl DialogPanel {
    /// Height of the caption bar. Our font is 8 px tall; a Pocket PC
    /// caption is a little over twice that, and 14 px leaves 3 px of
    /// padding above and below the text.
    pub const CAPTION_H: i32 = 14;

    /// Height of the boxes in the caption, and the width of the OK one.
    ///
    /// A device labels it with the word "OK", not a tick, so the box has
    /// to be wide enough for two glyphs plus a pixel of padding either
    /// side. The `?` box beside it is square.
    const BOX_H: i32 = 11;
    const OK_BOX: i32 = 2 * font::GLYPH_W + 4;
    const HELP_BOX: i32 = Self::BOX_H;
    /// Gap between the two boxes.
    const BOX_GAP: i32 = 2;

    /// Caption background — the CE title bar blue.
    const CAPTION_BG: u16 = 0x0010;
    /// Caption text, white on that blue.
    const CAPTION_FG: u16 = 0xFFFF;
    /// The OK label is drawn in the caption blue, as on a device.
    const OK_FG: u16 = 0x0010;

    /// Height the caption takes off the top of the window, or 0.
    pub fn caption_h(&self) -> i32 {
        if self.caption.is_some() {
            Self::CAPTION_H
        } else {
            0
        }
    }

    /// Client origin: one pixel in from each edge when the panel has a
    /// border, and below the caption bar when it has one.
    pub fn client_origin(&self) -> (i32, i32) {
        let inset = i32::from(self.border);
        (self.x + inset, self.y + inset + self.caption_h())
    }

    /// Screen rectangle of the OK box, for a captioned dialog.
    ///
    /// Windows CE has no close button on a dialog: the shell puts a
    /// single OK box at the right end of the caption, and tapping it is
    /// how the user accepts the dialog. There is no Cancel — which is
    /// why an app like Solitaire applies its Options as they are
    /// toggled rather than on the way out.
    pub fn ok_box(&self) -> Option<(i32, i32, i32, i32)> {
        self.caption.as_ref()?;
        let inset = i32::from(self.border);
        let pad = (Self::CAPTION_H - Self::BOX_H) / 2;
        Some((
            self.x + self.w - inset - pad - Self::OK_BOX,
            self.y + inset + pad,
            Self::OK_BOX,
            Self::BOX_H,
        ))
    }

    /// Screen rectangle of the `?` box, immediately left of the OK one.
    ///
    /// Pocket PC puts context help here. We draw it because the caption
    /// looks wrong without it, but it is inert: there is no help file to
    /// show, and tapping it must not be mistaken for the OK box.
    pub fn help_box(&self) -> Option<(i32, i32, i32, i32)> {
        let (bx, by, _, bh) = self.ok_box()?;
        Some((bx - Self::BOX_GAP - Self::HELP_BOX, by, Self::HELP_BOX, bh))
    }

    /// Is `(x, y)`, in screen coordinates, inside the OK box?
    pub fn ok_box_contains(&self, x: i32, y: i32) -> bool {
        match self.ok_box() {
            Some((bx, by, bw, bh)) => x >= bx && x < bx + bw && y >= by && y < by + bh,
            None => false,
        }
    }

    /// Is `(x, y)` anywhere inside this panel's window rectangle?
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn render(&self, surf: &mut Surface<'_>) {
        if !self.visible || self.w <= 0 || self.h <= 0 {
            return;
        }
        surf.fill_rect(self.x, self.y, self.w, self.h, ChildWindow::FACE);
        if self.border {
            stroke(surf, self.x, self.y, self.w, self.h, ChildWindow::TEXT);
        }
        self.render_caption(surf);
    }

    /// Paint the caption bar and its OK box.
    fn render_caption(&self, surf: &mut Surface<'_>) {
        let Some(title) = self.caption.as_deref() else {
            return;
        };
        let inset = i32::from(self.border);
        let (cx, cy) = (self.x + inset, self.y + inset);
        let cw = self.w - 2 * inset;
        if cw <= 0 {
            return;
        }
        surf.fill_rect(cx, cy, cw, Self::CAPTION_H, Self::CAPTION_BG);
        let ty = cy + (Self::CAPTION_H - font::GLYPH_H) / 2;
        // Stop the title before the boxes rather than under them.
        let right = self
            .help_box()
            .map(|(bx, ..)| bx - 2)
            .unwrap_or(cx + cw)
            .min(cx + cw);
        draw_clipped(surf, cx + 3, ty, title, Self::CAPTION_FG, right);

        if let Some((bx, by, bw, bh)) = self.help_box() {
            self.render_caption_box(surf, (bx, by, bw, bh), "?", false);
        }
        if let Some((bx, by, bw, bh)) = self.ok_box() {
            self.render_caption_box(surf, (bx, by, bw, bh), "OK", self.ok_pressed);
        }
    }

    /// One raised box in the caption bar, with its label centred.
    ///
    /// The label is drawn in the caption blue on the button face, which
    /// is how a device renders both the `?` and the `OK`; pressing it
    /// swaps the edge and nudges the label a pixel down and right.
    fn render_caption_box(
        &self,
        surf: &mut Surface<'_>,
        (bx, by, bw, bh): (i32, i32, i32, i32),
        label: &str,
        pressed: bool,
    ) {
        surf.fill_rect(bx, by, bw, bh, ChildWindow::FACE);
        let (tl, br) = if pressed {
            (ChildWindow::SHADOW, ChildWindow::LIGHT)
        } else {
            (ChildWindow::LIGHT, ChildWindow::SHADOW)
        };
        draw_edge(surf, bx, by, bw, bh, tl, br);
        let nudge = i32::from(pressed);
        let tx = bx + (bw - font::str_width(label)).max(0) / 2 + nudge;
        let ty = by + (bh - font::GLYPH_H) / 2 + nudge;
        draw_clipped(surf, tx, ty, label, Self::OK_FG, bx + bw - 1);
    }
}

/// One built-in control owned by the HLE rather than by the guest.
#[derive(Debug, Clone)]
pub struct ChildWindow {
    /// Handle handed back to the guest from `CreateWindowExW`.
    pub hwnd: u32,
    /// Owning top-level window.
    pub parent: u32,
    pub class: ControlClass,
    /// Control id — the `hMenu` argument of `CreateWindowExW` for a
    /// child window. This is what `GetDlgItem` looks up and what the
    /// parent matches on in `LOWORD(wParam)` of `WM_COMMAND`.
    pub id: u32,
    pub style: u32,
    /// Current caption / edit contents.
    pub text: String,
    pub text_color: u32,
    pub background_color: Option<u32>,
    /// Rectangle in parent-client coordinates. Controls are routinely
    /// created `0x0` and positioned later from the parent's `WM_SIZE`
    /// handler via `MoveWindow`, which is exactly what BlankApp does.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub visible: bool,
    /// `WS_DISABLED` clear. A disabled control draws its caption in the
    /// etched grey Win32 uses, never takes the focus, and swallows taps
    /// without notifying its parent — Solitaire ships its Undo button
    /// disabled until there is a move to undo.
    pub enabled: bool,
    /// Depressed while the stylus is held down inside a push button.
    pub pressed: bool,
    /// `BM_SETCHECK` state of a check box or radio button.
    pub checked: bool,
    pub list_items: Vec<(String, u32)>,
    pub list_images: Vec<i32>,
    pub list_image_lists: [u32; 3],
    pub list_image_sizes: [(u32, u32); 3],
    pub list_scroll_y: i32,
    pub list_positions: Vec<Option<(i32, i32)>>,
    pub list_spacing: (i32, i32),
    pub list_background: u32,
    pub list_text_color: u32,
    pub image: u32,
    pub list_selected: Option<usize>,
    pub list_column_count: u32,
    pub font: u32,
    pub choice_open: bool,
    pub choice_row_height: i32,
    pub choice_popup_y: i32,
    pub choice_popup_h: i32,
    pub track_range: (i32, i32),
    pub track_pos: i32,
    pub track_line: i32,
    pub track_page: i32,
    pub track_tick_freq: u32,
    pub track_ticks: Vec<i32>,
}

const LIST_VIEW_ROW_HEIGHT: i32 = 14;

impl ChildWindow {
    pub fn list_image_list(&self) -> (u32, u32, u32) {
        let preferred = usize::from(self.style & 3 != 0);
        let slot = if self.list_image_lists[preferred] != 0 {
            preferred
        } else {
            1 - preferred
        };
        let (w, h) = self.list_image_sizes[slot];
        (self.list_image_lists[slot], w, h)
    }
    pub fn list_row_height(&self) -> i32 {
        LIST_VIEW_ROW_HEIGHT.max(self.list_image_list().2.min(4096) as i32 + 2)
    }
    pub fn list_icon_view(&self) -> bool {
        self.style & 3 == 0 && self.list_image_list().0 != 0
    }
    pub fn list_columns(&self) -> usize {
        (self.w.max(1) / self.list_spacing.0.max(4)).max(1) as usize
    }
    pub fn list_item_position(&self, index: usize) -> (i32, i32) {
        self.list_positions
            .get(index)
            .copied()
            .flatten()
            .unwrap_or_else(|| {
                if self.list_icon_view() {
                    let columns = self.list_columns();
                    (
                        ((index % columns) as i32).saturating_mul(self.list_spacing.0),
                        ((index / columns) as i32).saturating_mul(self.list_spacing.1),
                    )
                } else {
                    (
                        0,
                        (index as i64 * i64::from(self.list_row_height())).min(i64::from(i32::MAX))
                            as i32,
                    )
                }
            })
    }
    pub fn list_item_at_point(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || x >= self.w || y < 0 || y >= self.h {
            return None;
        }
        if !self.list_icon_view() {
            return self.list_item_at(y);
        }
        let y = y.saturating_add(self.list_scroll_y);
        (0..self.list_items.len()).find(|i| {
            let (left, top) = self.list_item_position(*i);
            x >= left
                && x < left.saturating_add(self.list_spacing.0)
                && y >= top
                && y < top.saturating_add(self.list_spacing.1)
        })
    }
    pub fn list_ensure_visible(&mut self, index: usize) {
        let top = self.list_item_position(index).1;
        let height = if self.list_icon_view() {
            self.list_spacing.1
        } else {
            self.list_row_height()
        };
        if top < self.list_scroll_y {
            self.list_scroll_y = top;
        } else if top.saturating_add(height) > self.list_scroll_y.saturating_add(self.h) {
            self.list_scroll_y = top.saturating_add(height).saturating_sub(self.h).max(0);
        }
    }
    fn render_icon_list(
        &self,
        surf: &mut Surface<'_>,
        gdi: Option<&crate::gdi::GdiState>,
        only: Option<usize>,
    ) {
        let face = gdi.and_then(|g| crate::text::selected(g, self.font));
        let face = face.as_ref();

        let (handle, iw, ih) = self.list_image_list();
        let clip = [
            self.x,
            self.y,
            self.x.saturating_add(self.w),
            self.y.saturating_add(self.h),
        ];
        for (index, (text, _)) in self.list_items.iter().enumerate() {
            if only.is_some_and(|i| i != index) {
                continue;
            }
            let (left, top) = self.list_item_position(index);
            let x = self.x.saturating_add(left);
            let y = self
                .y
                .saturating_add(top)
                .saturating_sub(self.list_scroll_y);
            if y < self.y
                || y.saturating_add(ih as i32 + crate::text::layout(face, &[]).height + 6) > clip[3]
            {
                continue;
            }
            let cell = self.list_spacing.0;
            if let Some(image) = gdi.and_then(|g| g.image_lists.get(&handle)).and_then(|l| {
                self.list_images
                    .get(index)
                    .and_then(|i| usize::try_from(*i).ok())
                    .and_then(|i| l.images.get(i))
            }) {
                image.draw(
                    surf,
                    x.saturating_add((cell - iw as i32) / 2),
                    y + 2,
                    iw,
                    ih,
                    clip,
                );
            }
            let mut lines = vec![String::new()];
            for word in text.split_whitespace() {
                let line = lines.last_mut().unwrap();
                if !line.is_empty()
                    && crate::text::layout(
                        face,
                        &format!("{line} {word}").encode_utf16().collect::<Vec<_>>(),
                    )
                    .width
                        > cell - 4
                    && self.style & 0x80 == 0
                    && lines.len() < 2
                {
                    lines.push(word.to_string());
                } else {
                    let line = lines.last_mut().unwrap();
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(word);
                }
            }
            let selected = self.list_selected == Some(index);
            for (row, line) in lines.iter().enumerate() {
                let text_width =
                    crate::text::layout(face, &line.encode_utf16().collect::<Vec<_>>())
                        .width
                        .min(cell - 4);
                let tx = x.saturating_add((cell - text_width) / 2).max(self.x);
                let ty = y.saturating_add(
                    ih as i32 + 5 + row as i32 * (crate::text::layout(face, &[]).height + 1),
                );
                if ty < self.y || ty + crate::text::layout(face, &[]).height > clip[3] {
                    continue;
                }
                if selected {
                    surf.fill_rect(
                        tx - 1,
                        ty - 1,
                        (text_width + 2).min(clip[2] - tx + 1).max(0),
                        crate::text::layout(face, &[]).height + 2,
                        0x001f,
                    );
                }
                draw_clipped_font(
                    surf,
                    tx,
                    ty,
                    line,
                    if selected {
                        Self::WINDOW
                    } else {
                        crate::framebuffer::colorref_to_rgb565(self.list_text_color)
                    },
                    x.saturating_add(cell - 2).min(clip[2]),
                    face,
                );
            }
        }
    }
    /// Default item paint following an item's NM_CUSTOMDRAW prepaint.
    /// In icon mode do not erase the background already painted by the guest.
    pub fn render_custom_draw_item(
        &self,
        surf: &mut Surface<'_>,
        gdi: &crate::gdi::GdiState,
        index: usize,
    ) {
        if self.list_icon_view() {
            self.render_icon_list(surf, Some(gdi), Some(index));
        } else {
            let mut item = self.clone();
            item.list_items = self.list_items.get(index).cloned().into_iter().collect();
            item.list_images = self.list_images.get(index).copied().into_iter().collect();
            item.list_selected = (self.list_selected == Some(index)).then_some(0);
            let y =
                index as i64 * i64::from(self.list_row_height()) - i64::from(self.list_scroll_y);
            item.y = self
                .y
                .saturating_add(y.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
            item.h = self
                .list_row_height()
                .min(self.h.saturating_sub(item.y - self.y));
            item.list_scroll_y = 0;
            if item.y >= self.y && item.h > 0 {
                item.render_list(surf, Some(gdi));
            }
        }
    }

    pub fn list_item_at(&self, y: i32) -> Option<usize> {
        if y < 0 || y >= self.h {
            return None;
        }
        let index =
            (i64::from(y) + i64::from(self.list_scroll_y)) / i64::from(self.list_row_height());
        usize::try_from(index)
            .ok()
            .filter(|i| *i < self.list_items.len())
    }
    fn render_list(&self, surf: &mut Surface<'_>, gdi: Option<&crate::gdi::GdiState>) {
        let face = gdi.and_then(|g| crate::text::selected(g, self.font));
        let face = face.as_ref();

        surf.fill_rect(
            self.x,
            self.y,
            self.w,
            self.h,
            crate::framebuffer::colorref_to_rgb565(self.list_background),
        );
        if self.list_icon_view() {
            self.render_icon_list(surf, gdi, None);
            return;
        }
        let row_height = self.list_row_height();
        let (handle, iw, ih) = self.list_image_list();
        let first = (self.list_scroll_y.max(0) / row_height) as usize;
        for (index, (text, _)) in self.list_items.iter().enumerate().skip(first) {
            let y = i64::from(self.y) + index as i64 * i64::from(row_height)
                - i64::from(self.list_scroll_y);
            if y >= i64::from(self.y) + i64::from(self.h) {
                break;
            }
            let y = y as i32;
            let top = y.max(self.y);
            let bottom = (y + row_height).min(self.y + self.h);
            let selected = self.list_selected == Some(index);
            if selected {
                surf.fill_rect(self.x, top, self.w, bottom - top, 0x001f);
            }
            if let Some(image) = gdi.and_then(|g| g.image_lists.get(&handle)).and_then(|l| {
                self.list_images
                    .get(index)
                    .and_then(|i| usize::try_from(*i).ok())
                    .and_then(|i| l.images.get(i))
            }) {
                image.draw(
                    surf,
                    self.x + 2,
                    y + (row_height - ih as i32) / 2,
                    iw,
                    ih,
                    [self.x, self.y, self.x + self.w, self.y + self.h],
                );
            }
            let ty = y + (row_height - crate::text::layout(face, &[]).height) / 2;
            if ty >= self.y && ty + crate::text::layout(face, &[]).height <= self.y + self.h {
                draw_clipped_font(
                    surf,
                    self.x + 2 + if iw > 0 { iw as i32 + 3 } else { 0 },
                    ty,
                    text,
                    if selected { Self::WINDOW } else { Self::TEXT },
                    self.x + self.w - 1,
                    face,
                );
            }
        }
    }

    /// Face colour — `COLOR_BTNFACE`, the same light grey the status
    /// bar uses.
    const FACE: u16 = 0xC618;
    /// `COLOR_BTNSHADOW`.
    const SHADOW: u16 = 0x8410;
    /// `COLOR_BTNHIGHLIGHT`.
    const LIGHT: u16 = 0xFFFF;
    /// `COLOR_BTNTEXT` / `COLOR_WINDOWTEXT`.
    const TEXT: u16 = 0x0000;
    /// `COLOR_WINDOW` — the edit control's background.
    const WINDOW: u16 = 0xFFFF;

    /// Side of the square glyph box drawn for a check box / radio.
    const CHECK_BOX: i32 = 12;

    /// How far in from the left edge a group box's caption starts.
    const GROUP_LABEL_X: i32 = 7;

    /// Is `(px, py)`, in parent-client coordinates, inside this control?
    pub fn contains(&self, px: i32, py: i32) -> bool {
        let y = py - self.y;
        let inside_y = if self.class == ControlClass::ComboBox {
            (0..self.choice_closed_height()).contains(&y)
                || (self.choice_open
                    && (self.choice_popup_y..self.choice_popup_y + self.choice_popup_h)
                        .contains(&y))
        } else {
            (0..self.h).contains(&y)
        };
        px >= self.x && px < self.x + self.w && inside_y
    }

    /// `BS_*` type of a button, or `None` for the other classes.
    fn button_type(&self) -> Option<u32> {
        (self.class == ControlClass::Button).then_some(self.style & BS_TYPE_MASK)
    }

    /// A push button — the kind that sends `BN_CLICKED` and pops back
    /// up, as opposed to a check box or radio button that latches, or a
    /// group box that is not interactive at all.
    pub fn is_push_button(&self) -> bool {
        matches!(self.button_type(), Some(t) if t != BS_CHECKBOX
            && t != BS_AUTOCHECKBOX
            && t != BS_RADIOBUTTON
            && t != BS_AUTORADIOBUTTON
            && t != BS_GROUPBOX)
    }

    /// `BS_GROUPBOX` — a frame around a set of related controls.
    ///
    /// Rendered as an etched rectangle rather than a raised face: a
    /// group box is the same size as everything it encloses, so drawing
    /// it as a button would bury its own contents.
    pub fn is_group_box(&self) -> bool {
        self.button_type() == Some(BS_GROUPBOX)
    }

    /// A button that latches its check state when clicked. Only the
    /// `AUTO` variants toggle themselves; the plain ones leave it to
    /// the application's `WM_COMMAND` handler.
    pub fn is_auto_check(&self) -> bool {
        matches!(
            self.button_type(),
            Some(BS_AUTOCHECKBOX) | Some(BS_AUTORADIOBUTTON)
        )
    }

    /// Any latching button, auto or not — these draw a box/dot rather
    /// than a raised face.
    fn is_check_like(&self) -> bool {
        matches!(
            self.button_type(),
            Some(BS_CHECKBOX)
                | Some(BS_AUTOCHECKBOX)
                | Some(BS_RADIOBUTTON)
                | Some(BS_AUTORADIOBUTTON)
        )
    }

    /// `BS_DEFPUSHBUTTON` — drawn with the extra black outline that
    /// marks the button `VK_RETURN` would activate.
    fn is_default(&self) -> bool {
        self.button_type() == Some(BS_DEFPUSHBUTTON)
    }

    pub fn choice_closed_height(&self) -> i32 {
        self.h.min(self.choice_row_height.max(20)).max(0)
    }

    pub fn choice_height(&self) -> i32 {
        if self.class == ControlClass::ComboBox {
            if self.choice_open {
                self.choice_closed_height() + self.choice_popup_h
            } else {
                self.choice_closed_height()
            }
        } else {
            self.h
        }
    }
    /// Position a dropdown inside the display without moving its header.
    pub fn layout_choice_popup(&mut self, screen_y: i32, screen_h: i32) {
        let wanted = 2 + self.list_items.len().min(6) as i32 * self.choice_row_height;
        let below = screen_h
            .saturating_sub(screen_y.saturating_add(self.choice_closed_height()))
            .max(0);
        let above = screen_y.max(0);
        let upwards = below < wanted && above > below;
        let available = if upwards { above } else { below };
        let rows = self
            .list_items
            .len()
            .min(6)
            .min(available.saturating_sub(2) as usize / self.choice_row_height as usize);
        self.choice_popup_h = (2 + rows as i32 * self.choice_row_height).min(available);
        self.choice_popup_y = if upwards {
            -self.choice_popup_h
        } else {
            self.choice_closed_height()
        };
        if let Some(selected) = self.list_selected {
            let first = self.list_scroll_y.max(0) as usize;
            if selected < first {
                self.list_scroll_y = selected as i32;
            } else if rows > 0 && selected >= first + rows {
                self.list_scroll_y = (selected + 1 - rows) as i32;
            }
        }
    }
    fn render_track(&self, surf: &mut Surface<'_>, focused: bool) {
        let vertical = self.style & 2 != 0;
        let length = if vertical { self.h } else { self.w };
        let span = i64::from(self.track_range.1) - i64::from(self.track_range.0);
        let offset = if span > 0 {
            ((i64::from(self.track_pos) - i64::from(self.track_range.0))
                * i64::from((length - 10).max(0))
                / span) as i32
        } else {
            0
        };
        if self.style & 0x10 == 0 && length >= 10 {
            let travel = i64::from(length - 10);
            let mut positions = std::collections::BTreeSet::new();
            positions.insert(5);
            positions.insert(length - 5);
            let mut add_tick = |value: i32| {
                if span > 0 && value > self.track_range.0 && value < self.track_range.1 {
                    positions.insert(
                        5 + ((i64::from(value) - i64::from(self.track_range.0)) * travel / span)
                            as i32,
                    );
                }
            };
            for &value in &self.track_ticks {
                add_tick(value);
            }
            if self.style & 1 != 0 && self.track_tick_freq != 0 && span > 0 {
                let freq = i64::from(self.track_tick_freq);
                let count = (span - 1) / freq;
                // Bound raster work even for an application supplying a huge range.
                let stride = ((count + 4095) / 4096).max(1);
                let mut index = 1;
                while index <= count {
                    add_tick((i64::from(self.track_range.0) + index * freq) as i32);
                    index += stride;
                }
            }
            let before = self.style & (4 | 8) != 0;
            let after = self.style & 4 == 0 || self.style & 8 != 0;
            for pos in positions {
                if vertical {
                    if before {
                        surf.fill_rect(self.x, self.y + pos, 3, 1, Self::TEXT);
                    }
                    if after {
                        surf.fill_rect(self.x + self.w - 3, self.y + pos, 3, 1, Self::TEXT);
                    }
                } else {
                    if before {
                        surf.fill_rect(self.x + pos, self.y, 1, 3, Self::TEXT);
                    }
                    if after {
                        surf.fill_rect(self.x + pos, self.y + self.h - 3, 1, 3, Self::TEXT);
                    }
                }
            }
        }
        let color = if focused { 0x001f } else { 0x7bef };
        if vertical {
            surf.fill_rect(
                self.x + self.w / 2,
                self.y + 5,
                2,
                (self.h - 10).max(0),
                0x7bef,
            );
            surf.fill_rect(self.x + 2, self.y + offset, (self.w - 4).max(0), 10, color);
        } else {
            surf.fill_rect(
                self.x + 5,
                self.y + self.h / 2,
                (self.w - 10).max(0),
                2,
                0x7bef,
            );
            surf.fill_rect(self.x + offset, self.y + 2, 10, (self.h - 4).max(0), color);
        }
    }

    fn render_choice(
        &self,
        surf: &mut Surface<'_>,
        focused: bool,
        face: Option<&crate::gdi::Font>,
    ) {
        let combo = self.class == ControlClass::ComboBox;
        let height = if combo {
            self.choice_closed_height()
        } else {
            self.h
        };
        surf.fill_rect(self.x, self.y, self.w, height, Self::WINDOW);
        stroke(surf, self.x, self.y, self.w, height, Self::SHADOW);
        if combo {
            let value = self
                .list_selected
                .and_then(|i| self.list_items.get(i))
                .map(|i| i.0.as_str())
                .unwrap_or(&self.text);
            let line = crate::text::layout(face, &value.encode_utf16().collect::<Vec<_>>());
            line.draw(
                surf,
                self.x + 3,
                self.y + (height - line.height).max(0) / 2,
                Self::TEXT,
                [
                    self.x + 2,
                    self.y + 1,
                    self.x + self.w - 18,
                    self.y + height - 1,
                ],
            );
            let arrow_x = self.x + self.w - 17;
            surf.fill_rect(arrow_x, self.y + 1, 16, height - 2, Self::FACE);
            draw_edge(
                surf,
                arrow_x,
                self.y + 1,
                16,
                height - 2,
                Self::LIGHT,
                Self::SHADOW,
            );
            for row in 0..4 {
                surf.fill_rect(
                    arrow_x + 4 + row,
                    self.y + height / 2 - 2 + row,
                    7 - row * 2,
                    1,
                    Self::TEXT,
                );
            }
        }
        if !combo || self.choice_open {
            let (top, bottom) = if combo {
                let y = self.y + self.choice_popup_y;
                surf.fill_rect(self.x, y, self.w, self.choice_popup_h, Self::WINDOW);
                stroke(surf, self.x, y, self.w, self.choice_popup_h, Self::SHADOW);
                (y + 1, y + self.choice_popup_h - 1)
            } else {
                (self.y + 1, self.y + height - 1)
            };
            for (i, (text, _)) in self
                .list_items
                .iter()
                .enumerate()
                .skip(self.list_scroll_y.max(0) as usize)
            {
                let y = top + (i as i32 - self.list_scroll_y.max(0)) * self.choice_row_height;
                if y + self.choice_row_height > bottom {
                    break;
                }
                let selected = self.list_selected == Some(i);
                if selected {
                    surf.fill_rect(self.x + 1, y, self.w - 2, self.choice_row_height, 0x001f);
                }
                let line = crate::text::layout(face, &text.encode_utf16().collect::<Vec<_>>());
                line.draw(
                    surf,
                    self.x + 3,
                    y + (self.choice_row_height - line.height).max(0) / 2,
                    if selected { Self::WINDOW } else { Self::TEXT },
                    [
                        self.x + 2,
                        y,
                        self.x + self.w - 2,
                        (y + self.choice_row_height).min(bottom),
                    ],
                );
            }
        }
        if focused {
            stroke(
                surf,
                self.x + 1,
                self.y + 1,
                self.w - 2,
                height - 2,
                Self::TEXT,
            );
        }
    }

    /// Can this control take the input focus? Statics never do, which
    /// is what keeps a tap on a label from stealing the caret from an
    /// edit field, and neither does a disabled control nor a group box.
    pub fn is_focusable(&self) -> bool {
        self.class != ControlClass::Static && self.enabled && !self.is_group_box()
    }

    /// Draw a caption, etched when the control is disabled.
    ///
    /// Win32 has no grey text colour for this: it embosses instead,
    /// stamping the string in `COLOR_BTNHIGHLIGHT` one pixel down and
    /// right and then in `COLOR_BTNSHADOW` on top. The result reads as
    /// engraved into the face rather than merely faint, which is what
    /// makes Solitaire's disabled Undo button obviously dead.
    fn draw_caption(
        &self,
        surf: &mut Surface<'_>,
        x: i32,
        y: i32,
        right: i32,
        face: Option<&crate::gdi::Font>,
    ) {
        if face.is_none() {
            if self.enabled {
                draw_clipped(
                    surf,
                    x,
                    y,
                    &self.text,
                    crate::framebuffer::colorref_to_rgb565(self.text_color),
                    right,
                );
            } else {
                draw_clipped(surf, x + 1, y + 1, &self.text, Self::LIGHT, right + 1);
                draw_clipped(surf, x, y, &self.text, Self::SHADOW, right);
            }
            return;
        }
        let line = crate::text::layout(face, &self.text.encode_utf16().collect::<Vec<_>>());
        let clip = [self.x, self.y, right, self.y.saturating_add(self.h)];
        if self.enabled {
            line.draw(
                surf,
                x,
                y,
                crate::framebuffer::colorref_to_rgb565(self.text_color),
                clip,
            );
        } else {
            line.draw(surf, x + 1, y + 1, Self::LIGHT, clip);
            line.draw(surf, x, y, Self::SHADOW, clip);
        }
    }

    /// Paint the control. `focused` drives the focus rectangle and the
    /// edit caret; the caller knows which control holds the focus.
    pub fn render(&self, surf: &mut Surface<'_>, focused: bool) {
        self.render_with_images(surf, focused, None);
    }
    fn render_with_images(
        &self,
        surf: &mut Surface<'_>,
        focused: bool,
        gdi: Option<&crate::gdi::GdiState>,
    ) {
        if !self.visible || self.w <= 0 || self.h <= 0 {
            return;
        }
        if let Some(color) = self.background_color {
            // A combo's template height reserves space for its dropdown.
            // Its parent brush must not erase the siblings beneath the closed field.
            let height = if self.class == ControlClass::ComboBox {
                self.choice_closed_height()
            } else {
                self.h
            };
            surf.fill_rect(
                self.x,
                self.y,
                self.w,
                height,
                crate::framebuffer::colorref_to_rgb565(color),
            );
        }
        let face = gdi.and_then(|g| crate::text::selected(g, self.font));
        match self.class {
            ControlClass::TrackBar => self.render_track(surf, focused),
            ControlClass::Button => self.render_button(surf, focused, face.as_ref()),
            ControlClass::Edit => self.render_edit(surf, focused, face.as_ref()),
            ControlClass::Static => self.render_static(surf, face.as_ref()),
            ControlClass::ListView => self.render_list(surf, gdi),
            ControlClass::ListBox | ControlClass::ComboBox => {
                self.render_choice(surf, focused, face.as_ref())
            }
        }
        if self.image != 0 {
            if let Some(gdi) = gdi {
                let bitmap_image = gdi
                    .bitmap(self.image)
                    .and_then(crate::imaging::Image::from_bitmap);
                if let Some(image) = gdi.icons.get(&self.image).or(bitmap_image.as_ref()) {
                    let centered = self.class == ControlClass::Button || self.style & 0x200 != 0;
                    let x = self.x
                        + if centered {
                            (self.w - image.width as i32) / 2
                        } else {
                            0
                        };
                    let y = self.y
                        + if centered {
                            (self.h - image.height as i32) / 2
                        } else {
                            0
                        };
                    let stretch = self.class == ControlClass::Static
                        && self.style & 0x1f == 0x0e
                        && self.style & 0x40 != 0
                        && !centered;
                    image.draw(
                        surf,
                        x,
                        y,
                        if stretch { self.w as u32 } else { image.width },
                        if stretch { self.h as u32 } else { image.height },
                        [self.x, self.y, self.x + self.w, self.y + self.h],
                    );
                }
            }
        }
    }

    fn render_button(
        &self,
        surf: &mut Surface<'_>,
        focused: bool,
        face: Option<&crate::gdi::Font>,
    ) {
        if self.is_group_box() {
            return self.render_group_box(surf, face);
        }
        if self.is_check_like() {
            return self.render_check(surf, focused, face);
        }
        let (mut x, mut y, mut w, mut h) = (self.x, self.y, self.w, self.h);
        if self.is_default() {
            // The default button's outline sits *outside* the raised
            // face, so the face shrinks by a pixel on every side.
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
        // Held down: the light and shadow edges swap so the face reads
        // as sunken, and the caption shifts a pixel down-right with it.
        let (tl, br) = if self.pressed {
            (Self::SHADOW, Self::LIGHT)
        } else {
            (Self::LIGHT, Self::SHADOW)
        };
        draw_edge(surf, x, y, w, h, tl, br);

        let nudge = i32::from(self.pressed);
        let text_w = crate::text::layout(face, &self.text.encode_utf16().collect::<Vec<_>>()).width;
        let tx = x + ((w - text_w) / 2).max(1) + nudge;
        let ty = y + ((h - crate::text::layout(face, &[]).height) / 2).max(0) + nudge;
        if self.image != 0 {
            return;
        }
        self.draw_caption(surf, tx, ty, x + w - 2, face);

        if focused {
            dotted_rect(surf, x + 3, y + 3, w - 6, h - 6, Self::TEXT);
        }
    }

    /// Paint a `BS_GROUPBOX`: an etched frame with its caption let into
    /// the top edge.
    ///
    /// Win32 etches the frame the same way it etches disabled text —
    /// shadow up and left, highlight down and right — and breaks the top
    /// border for the caption rather than drawing the text over it. The
    /// frame's top runs along the middle of the caption's line, so the
    /// enclosed controls start below it.
    fn render_group_box(&self, surf: &mut Surface<'_>, face: Option<&crate::gdi::Font>) {
        let top = self.y + crate::text::layout(face, &[]).height / 2;
        let h = self.h - crate::text::layout(face, &[]).height / 2;
        if h <= 0 || self.w <= 0 {
            return;
        }
        // Etched: the shadow rectangle with a highlight one pixel
        // down-right of it, which is what gives the groove its depth.
        stroke(surf, self.x + 1, top + 1, self.w, h, Self::LIGHT);
        stroke(surf, self.x, top, self.w, h, Self::SHADOW);

        if self.text.is_empty() {
            return;
        }
        // Break the border under the caption, then stamp the text into
        // the gap. `Self::GROUP_LABEL_X` is Win32's indent.
        let text_w = crate::text::layout(face, &self.text.encode_utf16().collect::<Vec<_>>())
            .width
            .min(self.w - Self::GROUP_LABEL_X - 2);
        if text_w <= 0 {
            return;
        }
        surf.fill_rect(
            self.x + Self::GROUP_LABEL_X - 1,
            top,
            text_w + 3,
            2,
            Self::FACE,
        );
        self.draw_caption(
            surf,
            self.x + Self::GROUP_LABEL_X,
            self.y,
            self.x + self.w - 1,
            face,
        );
    }

    fn render_check(&self, surf: &mut Surface<'_>, focused: bool, face: Option<&crate::gdi::Font>) {
        let box_y = self.y + ((self.h - Self::CHECK_BOX) / 2).max(0);
        let side = Self::CHECK_BOX.min(self.h);
        surf.fill_rect(self.x, box_y, side, side, Self::WINDOW);
        draw_edge(surf, self.x, box_y, side, side, Self::SHADOW, Self::LIGHT);
        if self.checked {
            // A filled core rather than a literal tick: at 12 px with a
            // 6x8 font there is no room for a convincing check glyph.
            surf.fill_rect(self.x + 3, box_y + 3, side - 6, side - 6, Self::TEXT);
        }
        let tx = self.x + side + 4;
        let ty = self.y + ((self.h - crate::text::layout(face, &[]).height) / 2).max(0);
        self.draw_caption(surf, tx, ty, self.x + self.w, face);
        if focused {
            dotted_rect(surf, tx - 2, self.y, self.w - side - 2, self.h, Self::TEXT);
        }
    }

    fn render_edit(&self, surf: &mut Surface<'_>, focused: bool, face: Option<&crate::gdi::Font>) {
        // A disabled edit takes the button face rather than the window
        // colour, which is how Win32 says "read-only" without changing
        // the text.
        let bg = if self.enabled {
            Self::WINDOW
        } else {
            Self::FACE
        };
        surf.fill_rect(self.x, self.y, self.w, self.h, bg);
        // Sunken: shadow along the top/left, highlight along the
        // bottom/right — the inverse of a raised button.
        draw_edge(
            surf,
            self.x,
            self.y,
            self.w,
            self.h,
            Self::SHADOW,
            Self::LIGHT,
        );
        let inner_w = self.w - 6;
        if inner_w <= 0 {
            return;
        }
        // `ES_AUTOHSCROLL` behaviour: once the text outruns the field
        // the caret stays visible, so it is the *tail* that shows.
        let mut shown = self.text.clone();
        while !shown.is_empty()
            && crate::text::layout(face, &shown.encode_utf16().collect::<Vec<_>>()).width > inner_w
        {
            shown.remove(0);
        }
        let line = crate::text::layout(face, &shown.encode_utf16().collect::<Vec<_>>());
        let ty = self.y + ((self.h - crate::text::layout(face, &[]).height) / 2).max(1);
        line.draw(
            surf,
            self.x + 3,
            ty,
            Self::TEXT,
            [
                self.x + 3,
                self.y + 1,
                self.x + self.w - 2,
                self.y + self.h - 1,
            ],
        );
        if focused {
            let caret_x = self.x + 3 + line.width;
            if caret_x < self.x + self.w - 1 {
                surf.fill_rect(
                    caret_x,
                    ty,
                    1,
                    crate::text::layout(face, &[]).height,
                    Self::TEXT,
                );
            }
        }
    }

    fn render_static(&self, surf: &mut Surface<'_>, face: Option<&crate::gdi::Font>) {
        if self.image != 0 || self.w <= 0 || self.h <= 0 {
            return;
        }
        let kind = self.style & 0x1f;
        if !matches!(kind, 0 | 1 | 2 | 11 | 12) {
            return;
        }
        let wrap = matches!(kind, 0 | 1 | 2);
        let measure =
            |text: &str| crate::text::layout(face, &text.encode_utf16().collect::<Vec<_>>()).width;
        let mut lines = Vec::new();
        let text = self.text.replace("\r\n", "\n").replace('\r', "\n");
        let text = if kind == 11 {
            text.replace('\n', " ")
        } else {
            text
        };
        for paragraph in text.split('\n') {
            if !wrap {
                lines.push(paragraph.to_string());
                continue;
            }
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let candidate = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if !line.is_empty() && measure(&candidate) > self.w {
                    lines.push(std::mem::take(&mut line));
                    line.push_str(word);
                } else {
                    line = candidate;
                }
            }
            lines.push(line);
        }
        let line_height = crate::text::layout(face, &[]).height.max(1);
        let height = line_height.saturating_mul(lines.len() as i32);
        let mut y = self.y
            + if self.style & 0x200 != 0 && lines.len() == 1 {
                (self.h - height).max(0) / 2
            } else {
                0
            };
        let clip = [self.x, self.y, self.x + self.w, self.y + self.h];
        for text in lines {
            if y >= clip[3] {
                break;
            }
            let line = crate::text::layout(face, &text.encode_utf16().collect::<Vec<_>>());
            let x = self.x
                + match kind {
                    1 => (self.w - line.width) / 2,
                    2 => self.w - line.width,
                    _ => 0,
                };
            if self.enabled || kind == 11 {
                line.draw(
                    surf,
                    x,
                    y,
                    crate::framebuffer::colorref_to_rgb565(self.text_color),
                    clip,
                );
            } else {
                line.draw(surf, x + 1, y + 1, Self::LIGHT, clip);
                line.draw(surf, x, y, Self::SHADOW, clip);
            }
            y = y.saturating_add(line_height);
        }
    }
}

/// Draw `text` but stop before `right`, so a caption never bleeds out
/// of its control.
fn draw_clipped_font(
    surf: &mut Surface<'_>,
    x: i32,
    y: i32,
    text: &str,
    color: u16,
    right: i32,
    face: Option<&crate::gdi::Font>,
) {
    if face.is_none() {
        return draw_clipped(surf, x, y, text, color, right);
    }
    let line = crate::text::layout(face, &text.encode_utf16().collect::<Vec<_>>());
    let (_, h) = surf.dimensions();
    line.draw(
        surf,
        x,
        y,
        color,
        [x, y, right, y.saturating_add(line.height).min(h as i32)],
    );
}

fn draw_clipped(surf: &mut Surface<'_>, x: i32, y: i32, text: &str, color: u16, right: i32) {
    let room = (right - x).max(0);
    let max_chars = (room / font::GLYPH_W) as usize;
    if max_chars == 0 {
        return;
    }
    let shown: String = text.chars().take(max_chars).collect();
    font::draw_str(surf, x, y, &shown, color);
}

/// The classic Win32 3D edge: `tl` down the top and left, `br` up the
/// bottom and right.
pub(crate) fn draw_edge(surf: &mut Surface<'_>, x: i32, y: i32, w: i32, h: i32, tl: u16, br: u16) {
    if w <= 0 || h <= 0 {
        return;
    }
    surf.fill_rect(x, y, w, 1, tl);
    surf.fill_rect(x, y, 1, h, tl);
    surf.fill_rect(x, y + h - 1, w, 1, br);
    surf.fill_rect(x + w - 1, y, 1, h, br);
}

/// Solid one-pixel outline.
pub(crate) fn stroke(surf: &mut Surface<'_>, x: i32, y: i32, w: i32, h: i32, color: u16) {
    draw_edge(surf, x, y, w, h, color, color);
}

/// The alternating-pixel focus rectangle Windows draws inside a button
/// that holds the keyboard focus.
fn dotted_rect(surf: &mut Surface<'_>, x: i32, y: i32, w: i32, h: i32, color: u16) {
    if w <= 0 || h <= 0 {
        return;
    }
    for i in 0..w {
        if (x + i + y) % 2 == 0 {
            surf.put_pixel(x + i, y, color);
        }
        if (x + i + y + h - 1) % 2 == 0 {
            surf.put_pixel(x + i, y + h - 1, color);
        }
    }
    for j in 0..h {
        if (x + y + j) % 2 == 0 {
            surf.put_pixel(x, y + j, color);
        }
        if (x + w - 1 + y + j) % 2 == 0 {
            surf.put_pixel(x + w - 1, y + j, color);
        }
    }
}

/// What a stylus or key event did to the control set, so the caller can
/// synthesise the window message a real control would have sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    /// A push button was released inside its own rectangle, or a check
    /// box latched: the parent must receive
    /// `WM_COMMAND(MAKEWPARAM(id, BN_CLICKED), hwndChild)`.
    Clicked { parent: u32, id: u32, hwnd: u32 },
    Notify {
        parent: u32,
        id: u32,
        hwnd: u32,
        code: u32,
        index: i32,
    },
    /// The event was consumed by a control (focus change, a character
    /// typed into an edit) but produces no notification.
    Consumed,
}

/// Every built-in control the guest has created, plus the focus.
#[derive(Debug, Clone)]
pub struct Controls {
    children: Vec<ChildWindow>,
    /// Dialog panels created from a `DLGTEMPLATE`. A child whose parent
    /// is one of these is positioned relative to it.
    panels: Vec<DialogPanel>,
    /// Handle of the control holding the input focus, or `0`.
    pub focus: u32,
    next_hwnd: u32,
    /// Push button the stylus went down on. Win32 captures the mouse
    /// here: releasing outside the button cancels the click instead of
    /// notifying the parent.
    capture: u32,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            children: Vec::new(),
            panels: Vec::new(),
            focus: 0,
            next_hwnd: Self::HWND_BASE,
            capture: 0,
        }
    }
}

impl Controls {
    /// First handle handed out to a control. Kept in the same
    /// `0xDEAD_xxxx` space as the other synthetic window handles, in a
    /// range wide enough that [`Self::is_child_hwnd`] can recognise one
    /// without consulting the state.
    pub const HWND_BASE: u32 = 0xDEAD_0E00;
    /// One past the last control handle.
    pub const HWND_END: u32 = 0xDEAD_0F00;

    /// Does `hwnd` fall in the control handle range? Stateless so the
    /// `IsWindow` family can answer without borrowing the kernel.
    pub fn is_child_hwnd(hwnd: u32) -> bool {
        (Self::HWND_BASE..Self::HWND_END).contains(&hwnd)
    }

    /// Register a new control and return its handle, or `0` if the
    /// handle range is exhausted.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &mut self,
        parent: u32,
        class: ControlClass,
        id: u32,
        text: String,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> u32 {
        if self.next_hwnd >= Self::HWND_END {
            return 0;
        }
        let hwnd = self.next_hwnd;
        self.next_hwnd += 1;
        self.children.push(ChildWindow {
            hwnd,
            parent,
            class,
            id,
            style,
            text,
            text_color: 0,
            background_color: None,
            x,
            y,
            w,
            h,
            visible: style & WS_VISIBLE != 0,
            enabled: style & WS_DISABLED == 0,
            pressed: false,
            checked: false,
            list_items: Vec::new(),
            list_images: Vec::new(),
            list_image_lists: [0; 3],
            list_image_sizes: [(0, 0); 3],
            list_scroll_y: 0,
            list_positions: Vec::new(),
            list_spacing: (75, 75),
            list_background: 0x00ffffff,
            list_text_color: 0,
            image: 0,
            list_selected: None,
            list_column_count: 0,
            font: crate::gdi::STOCK_SYSTEM_FONT,
            choice_open: false,
            choice_row_height: 16,
            choice_popup_y: 20,
            choice_popup_h: 98,
            track_range: (0, 100),
            track_pos: 0,
            track_line: 1,
            track_page: 20,
            track_tick_freq: 1,
            track_ticks: Vec::new(),
        });
        hwnd
    }

    /// Register a dialog panel, replacing any previous one with the same
    /// handle — a guest that recreates a dialog gets a fresh panel
    /// rather than two stacked on each other.
    pub fn add_panel(&mut self, panel: DialogPanel) {
        self.panels.retain(|p| p.hwnd != panel.hwnd);
        self.panels.push(panel);
    }

    /// Panels are composited bottom to top. Activation can bring an older
    /// modeless dialog above a newer one without destroying either window.
    pub fn raise_panel(&mut self, hwnd: u32) {
        if let Some(index) = self.panels.iter().position(|p| p.hwnd == hwnd) {
            let panel = self.panels.remove(index);
            self.panels.push(panel);
        }
    }

    /// Reorder a child among siblings, stored bottom to top for painting and
    /// reverse hit testing. HWND_TOP/TOPMOST/NOTOPMOST raise; HWND_BOTTOM lowers.
    pub fn set_child_z_order(&mut self, hwnd: u32, insert_after: u32) -> bool {
        let Some(index) = self.children.iter().position(|c| c.hwnd == hwnd) else {
            return false;
        };
        let parent = self.children[index].parent;
        if insert_after == hwnd {
            return false;
        }
        if !matches!(insert_after, 0 | 1 | 0xffff_ffff | 0xffff_fffe)
            && !self
                .children
                .iter()
                .any(|c| c.hwnd == insert_after && c.parent == parent)
        {
            return false;
        }
        let child = self.children.remove(index);
        let target = match insert_after {
            1 => self
                .children
                .iter()
                .position(|c| c.parent == parent)
                .unwrap_or(self.children.len()),
            0 | 0xffff_ffff | 0xffff_fffe => self
                .children
                .iter()
                .rposition(|c| c.parent == parent)
                .map_or(self.children.len(), |i| i + 1),
            _ => self
                .children
                .iter()
                .position(|c| c.hwnd == insert_after)
                .unwrap(),
        };
        self.children.insert(target, child);
        true
    }

    pub fn update_choice_fonts(&mut self, gdi: &crate::gdi::GdiState) {
        for child in &mut self.children {
            if matches!(child.class, ControlClass::ComboBox | ControlClass::ListBox) {
                let face = crate::text::selected(gdi, child.font);
                child.choice_row_height =
                    (crate::text::layout(face.as_ref(), &[]).height + 4).max(16);
            }
        }
    }

    pub fn layout_choice_popups(&mut self, screen_h: i32) {
        let positions: Vec<_> = self
            .children
            .iter()
            .filter(|c| c.class == ControlClass::ComboBox && c.choice_open)
            .filter_map(|c| self.screen_rect(c.hwnd).map(|r| (c.hwnd, r.1)))
            .collect();
        for (hwnd, y) in positions {
            if let Some(c) = self.get_mut(hwnd) {
                c.layout_choice_popup(y, screen_h);
            }
        }
    }

    /// Panel handles in compositing order, bottom to top.
    pub fn panel_handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.panels.iter().map(|panel| panel.hwnd)
    }

    pub fn panel(&self, hwnd: u32) -> Option<&DialogPanel> {
        self.panels.iter().find(|p| p.hwnd == hwnd)
    }

    pub fn panel_mut(&mut self, hwnd: u32) -> Option<&mut DialogPanel> {
        self.panels.iter_mut().find(|p| p.hwnd == hwnd)
    }

    /// Screen-space origin a child's coordinates are relative to: its
    /// panel's client origin, or `(0, 0)` for a control parented
    /// straight to a full-screen frame window.
    fn parent_origin(&self, parent: u32) -> (i32, i32) {
        self.panel(parent)
            .map(DialogPanel::client_origin)
            .unwrap_or((0, 0))
    }

    /// A child's rectangle in screen coordinates.
    pub fn screen_rect(&self, hwnd: u32) -> Option<(i32, i32, i32, i32)> {
        let child = self.get(hwnd)?;
        let (ox, oy) = self.parent_origin(child.parent);
        Some((child.x + ox, child.y + oy, child.w, child.h))
    }

    pub fn get(&self, hwnd: u32) -> Option<&ChildWindow> {
        self.children.iter().find(|c| c.hwnd == hwnd)
    }

    pub fn get_mut(&mut self, hwnd: u32) -> Option<&mut ChildWindow> {
        self.children.iter_mut().find(|c| c.hwnd == hwnd)
    }

    /// `GetDlgItem`: find a control by its parent and id.
    pub fn children_of(&self, parent: u32) -> Vec<u32> {
        self.children
            .iter()
            .filter(|c| c.parent == parent)
            .map(|c| c.hwnd)
            .collect()
    }

    pub fn by_id(&self, parent: u32, id: u32) -> Option<&ChildWindow> {
        self.children
            .iter()
            .find(|c| c.parent == parent && c.id == id)
    }

    /// `CheckRadioButton`: check the control with id `check` and clear
    /// every other control in `first..=last`, returning how many were
    /// touched.
    ///
    /// The range is matched on id rather than on style, because that is
    /// what the API contract says — an app is free to pass a range that
    /// happens to include a static, and the real one clears it too.
    /// `parent` is honoured so a dialog cannot reach into another one's
    /// controls that happen to share ids.
    pub fn check_radio_range(&mut self, parent: u32, first: u32, last: u32, check: u32) -> usize {
        let mut hit = 0;
        for child in &mut self.children {
            if child.parent != parent || child.id < first || child.id > last {
                continue;
            }
            child.checked = child.id == check;
            hit += 1;
        }
        hit
    }

    /// Nothing to paint and nothing to hit.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty() && self.panels.is_empty()
    }

    /// Drop a control and anything that referred to it.
    pub fn destroy(&mut self, hwnd: u32) {
        self.children.retain(|c| c.hwnd != hwnd);
        if self.focus == hwnd {
            self.focus = 0;
        }
        if self.capture == hwnd {
            self.capture = 0;
        }
    }

    /// Drop every control owned by `parent` — a top-level window going
    /// away takes its children with it, and a dialog takes its panel.
    pub fn destroy_children_of(&mut self, parent: u32) {
        let doomed: Vec<u32> = self
            .children
            .iter()
            .filter(|c| c.parent == parent)
            .map(|c| c.hwnd)
            .collect();
        for hwnd in doomed {
            self.destroy(hwnd);
        }
        self.panels.retain(|p| p.hwnd != parent);
    }

    /// Is this control on screen — itself visible, and inside a panel
    /// that is?
    fn is_showing(&self, child: &ChildWindow) -> bool {
        child.visible && self.panel(child.parent).map(|p| p.visible).unwrap_or(true)
    }

    /// `IsWindowVisible` for anything we own: a control, or a dialog
    /// panel. `None` means the handle is not one of ours, and the caller
    /// should answer for it however it answers for top-level windows.
    ///
    /// Answering this honestly matters more than it looks. Solitaire
    /// builds its status readouts hidden, then asks whether they are
    /// visible before showing them; a handle that always claims to be
    /// visible makes the app skip its own `ShowWindow` and the Time and
    /// Score fields never appear.
    pub fn visible_color_controls(&self) -> impl Iterator<Item = &ChildWindow> {
        self.children.iter().filter(|c| {
            c.class != ControlClass::ListView
                && self.is_showing(c)
                && c.w > 0
                && c.h > 0
                && !(c.class == ControlClass::Button && c.style & 0xf == 0xb)
        })
    }
    pub fn visible_list_views(&self) -> impl Iterator<Item = &ChildWindow> {
        self.children.iter().filter(|c| {
            c.class == ControlClass::ListView && self.is_showing(c) && c.w > 0 && c.h > 0
        })
    }
    pub fn owner_draw_controls(&self) -> impl Iterator<Item = &ChildWindow> {
        self.children.iter().filter(|c| {
            self.is_showing(c)
                && c.w > 0
                && c.h > 0
                && c.class == ControlClass::Button
                && c.style & 0xf == 0xb
        })
    }
    pub fn is_visible(&self, hwnd: u32) -> Option<bool> {
        if let Some(child) = self.get(hwnd) {
            return Some(self.is_showing(child));
        }
        self.panel(hwnd).map(|p| p.visible)
    }

    /// Topmost visible control under `(x, y)`, in screen coordinates.
    /// Later-created controls sit on top, matching the z-order a freshly
    /// built dialog has.
    ///
    /// Group boxes are skipped: they enclose the controls they label, so
    /// a frame that answered a hit test would shadow every radio inside
    /// it whenever the z-order put it on top.
    pub fn hit_test(&self, x: i32, y: i32) -> Option<u32> {
        self.children
            .iter()
            .rev()
            .filter(|c| c.class == ControlClass::ComboBox && c.choice_open)
            .chain(
                self.children
                    .iter()
                    .rev()
                    .filter(|c| !(c.class == ControlClass::ComboBox && c.choice_open)),
            )
            .find(|c| {
                let (ox, oy) = self.parent_origin(c.parent);
                !c.is_group_box() && self.is_showing(c) && c.contains(x - ox, y - oy)
            })
            .map(|c| c.hwnd)
    }

    /// Stylus down. Takes the focus, and arms a push button.
    pub fn pointer_down(&mut self, x: i32, y: i32) -> Option<ControlAction> {
        let hwnd = self.hit_test(x, y)?;
        let origin = self
            .get(hwnd)
            .map(|c| self.parent_origin(c.parent))
            .unwrap_or((0, 0));
        let child = self.get_mut(hwnd)?;
        if matches!(child.class, ControlClass::ListBox | ControlClass::ComboBox) && child.enabled {
            self.focus = hwnd;
            let child = self.get_mut(hwnd).unwrap();
            let combo = child.class == ControlClass::ComboBox;
            let local_y = y - origin.1 - child.y;
            if combo && (0..child.choice_closed_height()).contains(&local_y) {
                child.choice_open = !child.choice_open;
                return Some(ControlAction::Consumed);
            }
            let index = ((local_y - if combo { child.choice_popup_y + 1 } else { 1 }).max(0)
                / child.choice_row_height
                + child.list_scroll_y.max(0)) as usize;
            if index < child.list_items.len() {
                child.list_selected = Some(index);
                child.choice_open = false;
                return Some(ControlAction::Notify {
                    parent: child.parent,
                    id: child.id,
                    hwnd,
                    code: 1,
                    index: index as i32,
                });
            }
            return Some(ControlAction::Consumed);
        }
        if child.class == ControlClass::ListView && child.enabled {
            if let Some(index) =
                child.list_item_at_point(x - origin.0 - child.x, y - origin.1 - child.y)
            {
                child.list_selected = Some(index);
            }
        }
        if !child.is_focusable() {
            return Some(ControlAction::Consumed);
        }
        if child.class == ControlClass::Button {
            child.pressed = true;
            self.capture = hwnd;
        }
        self.focus = hwnd;
        Some(ControlAction::Consumed)
    }

    /// Stylus up. A button only notifies its parent when the release
    /// lands back inside the rectangle the press started in.
    pub fn pointer_up(&mut self, x: i32, y: i32) -> Option<ControlAction> {
        if let Some(hwnd) = self.hit_test(x, y) {
            if let Some(c) = self
                .get(hwnd)
                .filter(|c| c.class == ControlClass::ListView && c.enabled)
            {
                if let Some(index) = c.list_selected {
                    return Some(ControlAction::Notify {
                        parent: c.parent,
                        id: c.id,
                        hwnd,
                        code: (-2i32) as u32,
                        index: index as i32,
                    });
                }
            }
        }
        let captured = self.capture;
        if captured == 0 {
            // Not our press — but still swallow a release over a
            // control so the parent does not act on it.
            return self.hit_test(x, y).map(|_| ControlAction::Consumed);
        }
        self.capture = 0;
        let (ox, oy) = self
            .get(captured)
            .map(|c| self.parent_origin(c.parent))
            .unwrap_or((0, 0));
        let child = self.get_mut(captured)?;
        child.pressed = false;
        if !child.contains(x - ox, y - oy) {
            return Some(ControlAction::Consumed);
        }
        if child.is_auto_check() {
            child.checked = !child.checked;
        }
        Some(ControlAction::Clicked {
            parent: child.parent,
            id: child.id,
            hwnd: child.hwnd,
        })
    }

    /// [`Self::pointer_down`] restricted to the children of `parent`.
    ///
    /// A modal dialog owns the stylus for as long as it is up: a tap
    /// outside it does nothing at all, rather than reaching the window
    /// underneath. Every event is still reported as consumed for that
    /// reason.
    ///
    /// The caption's OK box is tested first: it sits above the client
    /// area, so no control can be under it, and it has to arm even
    /// though it is chrome rather than a child window.
    pub fn pointer_down_in(&mut self, parent: u32, x: i32, y: i32) -> Option<ControlAction> {
        if let Some(panel) = self.panel_mut(parent) {
            if panel.ok_box_contains(x, y) {
                panel.ok_pressed = true;
                return Some(ControlAction::Consumed);
            }
        }
        match self.hit_test(x, y) {
            Some(hwnd) if self.get(hwnd).map(|c| c.parent) == Some(parent) => {
                self.pointer_down(x, y)
            }
            _ => Some(ControlAction::Consumed),
        }
    }

    /// [`Self::pointer_up`] restricted to the children of `parent`.
    pub fn pointer_up_in(&mut self, parent: u32, x: i32, y: i32) -> Option<ControlAction> {
        // Releasing the caption's OK box is the dialog's accept: report
        // it as `IDOK` from the panel itself, which is what the shell
        // sends on a device.
        if let Some(panel) = self.panel_mut(parent) {
            if panel.ok_pressed {
                panel.ok_pressed = false;
                let hit = panel.ok_box_contains(x, y);
                let hwnd = panel.hwnd;
                return Some(if hit {
                    ControlAction::Clicked {
                        parent,
                        id: IDOK,
                        hwnd,
                    }
                } else {
                    ControlAction::Consumed
                });
            }
        }
        // The press that captured has to belong to the dialog too, or a
        // release could fire a click on the window behind it.
        let captured_elsewhere =
            self.capture != 0 && self.get(self.capture).map(|c| c.parent) != Some(parent);
        if captured_elsewhere {
            self.capture = 0;
            return Some(ControlAction::Consumed);
        }
        match self.pointer_up(x, y) {
            Some(ControlAction::Clicked {
                parent: p,
                id,
                hwnd,
            }) if p == parent => Some(ControlAction::Clicked {
                parent: p,
                id,
                hwnd,
            }),
            Some(_) | None => Some(ControlAction::Consumed),
        }
    }

    /// [`Self::key_down`] restricted to the children of `parent`.
    pub fn key_down_in(&mut self, parent: u32, vk: u16) -> Option<ControlAction> {
        // Dialog-manager keys apply even when WM_INITDIALOG left no child
        // focused. Never borrow a default button from the window underneath.
        if vk == 0x1B {
            return Some(ControlAction::Clicked {
                parent,
                id: IDCANCEL,
                hwnd: parent,
            });
        }
        if vk == 0x09 {
            let candidates: Vec<u32> = self
                .children
                .iter()
                .filter(|c| {
                    c.parent == parent && c.visible && c.is_focusable() && c.style & 0x00010000 != 0
                })
                .map(|c| c.hwnd)
                .collect();
            if !candidates.is_empty() {
                let next = candidates
                    .iter()
                    .position(|h| *h == self.focus)
                    .map(|i| (i + 1) % candidates.len())
                    .unwrap_or(0);
                self.focus = candidates[next];
            }
            return Some(ControlAction::Consumed);
        }
        if vk == 0x0D {
            let focused = self.get(self.focus).filter(|c| {
                c.parent == parent && c.visible && c.enabled && c.class == ControlClass::Button
            });
            let button = focused.or_else(|| {
                self.children
                    .iter()
                    .find(|c| c.parent == parent && c.visible && c.enabled && c.is_default())
            });
            if let Some(button) = button {
                let hwnd = button.hwnd;
                // Preserve checkbox/radio activation behaviour in key_down.
                self.focus = hwnd;
                return self.key_down(vk);
            }
        }
        if self.get(self.focus).map(|c| c.parent) != Some(parent) {
            return Some(ControlAction::Consumed);
        }
        match self.key_down(vk) {
            Some(ControlAction::Clicked {
                parent: p,
                id,
                hwnd,
            }) if p == parent => Some(ControlAction::Clicked {
                parent: p,
                id,
                hwnd,
            }),
            Some(_) | None => Some(ControlAction::Consumed),
        }
    }

    /// Feed a virtual-key press to the focused control.
    ///
    /// Returns `None` when nothing has the focus or the key means
    /// nothing here, in which case the caller passes it to the
    /// application as usual.
    pub fn key_down(&mut self, vk: u16) -> Option<ControlAction> {
        const VK_BACK: u16 = 0x08;
        const VK_RETURN: u16 = 0x0D;
        const VK_SPACE: u16 = 0x20;

        let focus = self.focus;
        let child = self.get_mut(focus)?;
        if !child.enabled {
            // `SetFocus` on a disabled control is not something Win32
            // does, but a guest can ask for it; the control still must
            // not act on the key.
            return Some(ControlAction::Consumed);
        }
        match child.class {
            ControlClass::ComboBox
                if vk == 0x0d || (vk == 0x1b && child.choice_open) || vk == 0x73 =>
            {
                child.choice_open = vk != 0x1b && !child.choice_open;
                Some(ControlAction::Consumed)
            }
            ControlClass::TrackBar if matches!(vk, 0x21..=0x28) => {
                let (value, code) = match vk {
                    0x24 => (child.track_range.0, 6),
                    0x23 => (child.track_range.1, 7),
                    0x21 => (child.track_pos.saturating_sub(child.track_page), 2),
                    0x22 => (child.track_pos.saturating_add(child.track_page), 3),
                    0x25 | 0x26 => (child.track_pos.saturating_sub(child.track_line), 0),
                    _ => (child.track_pos.saturating_add(child.track_line), 1),
                };
                child.track_pos = value.clamp(child.track_range.0, child.track_range.1);
                Some(ControlAction::Notify {
                    parent: child.parent,
                    id: child.id,
                    hwnd: child.hwnd,
                    code,
                    index: child.track_pos,
                })
            }
            ControlClass::ListBox | ControlClass::ComboBox if matches!(vk, 0x26 | 0x28) => {
                if child.list_items.is_empty() {
                    return Some(ControlAction::Consumed);
                }
                let next = match child.list_selected {
                    Some(i) if vk == 0x26 => i.saturating_sub(1),
                    Some(i) => (i + 1).min(child.list_items.len() - 1),
                    None => 0,
                };
                child.list_selected = Some(next);
                Some(ControlAction::Notify {
                    parent: child.parent,
                    id: child.id,
                    hwnd: child.hwnd,
                    code: 1,
                    index: next as i32,
                })
            }
            ControlClass::ListView if matches!(vk, 0x25..=0x28) => {
                if child.list_items.is_empty() {
                    return Some(ControlAction::Consumed);
                }
                if !child.list_icon_view() && (vk == 0x25 || vk == 0x27) {
                    return None;
                }
                let step = if child.list_icon_view() && (vk == 0x26 || vk == 0x28) {
                    child.list_columns()
                } else {
                    1
                };
                let next = match child.list_selected {
                    Some(i) if vk == 0x25 || vk == 0x26 => i.saturating_sub(step),
                    Some(i) => i.saturating_add(step).min(child.list_items.len() - 1),
                    None => 0,
                };
                child.list_selected = Some(next);
                child.list_ensure_visible(next);
                Some(ControlAction::Notify {
                    parent: child.parent,
                    id: child.id,
                    hwnd: child.hwnd,
                    code: (-101i32) as u32,
                    index: next as i32,
                })
            }
            ControlClass::Edit => {
                if vk == VK_BACK {
                    child.text.pop();
                    return Some(ControlAction::Consumed);
                }
                let ch = vk_to_char(vk)?;
                child.text.push(ch);
                Some(ControlAction::Consumed)
            }
            ControlClass::Button if vk == VK_SPACE || vk == VK_RETURN => {
                if child.is_auto_check() {
                    child.checked = !child.checked;
                }
                Some(ControlAction::Clicked {
                    parent: child.parent,
                    id: child.id,
                    hwnd: child.hwnd,
                })
            }
            _ => None,
        }
    }

    /// Paint every visible panel and control over `surf`.
    ///
    /// Called once the application's own `WM_PAINT` has finished: on a
    /// device the controls are sibling windows that paint after the
    /// parent has filled its client area, and an app that blits over
    /// the whole client rect would otherwise erase them.
    ///
    /// Each window is painted as a unit — a panel's face immediately
    /// followed by its own children — rather than every panel first and
    /// every child after. Otherwise a dialog that opens over another
    /// one covers its face but not its buttons, and Solitaire's Exit /
    /// Help strip shows through the Options dialog on top of it.
    ///
    /// Controls parented straight to a frame window go down first, below
    /// every dialog; panels follow their stacking order, initially creation
    /// order. Activating an existing dialog brings it to the top.
    ///
    /// Group boxes go down before their siblings. A template lists a
    /// group box *before* the radios it encloses and gives it a
    /// rectangle large enough to contain them, so painting in template
    /// order is right, but only as long as nothing later re-orders
    /// them — the separate pass makes the "frames are backdrop" rule
    /// explicit rather than incidental.
    pub fn render(&self, surf: &mut Surface<'_>) {
        self.render_composited(surf, |_, _| {});
    }
    pub fn render_composited(
        &self,
        surf: &mut Surface<'_>,
        mut layer: impl FnMut(u32, &mut Surface<'_>),
    ) {
        self.render_layers(surf, None, &mut layer);
    }
    pub fn render_composited_with_images(
        &self,
        surf: &mut Surface<'_>,
        gdi: &crate::gdi::GdiState,
        mut layer: impl FnMut(u32, &mut Surface<'_>),
    ) {
        self.render_layers(surf, Some(gdi), &mut layer);
    }
    pub fn render_with_images(&self, surf: &mut Surface<'_>, gdi: &crate::gdi::GdiState) {
        self.render_composited_with_images(surf, gdi, |_, _| {});
    }
    fn render_layers(
        &self,
        surf: &mut Surface<'_>,
        gdi: Option<&crate::gdi::GdiState>,
        layer: &mut impl FnMut(u32, &mut Surface<'_>),
    ) {
        layer(0, surf);
        self.render_children_of(surf, |parent| self.panel(parent).is_none(), gdi, layer);
        self.render_open_choices(surf, |parent| self.panel(parent).is_none(), gdi);
        for panel in &self.panels {
            panel.render(surf);
            if panel.visible {
                if let Some(color) = gdi
                    .and_then(|g| g.dialog_backgrounds.get(&panel.hwnd))
                    .copied()
                {
                    let (x, y) = panel.client_origin();
                    surf.fill_rect(
                        x,
                        y,
                        panel.w - 2 * i32::from(panel.border),
                        panel.h - 2 * i32::from(panel.border) - panel.caption_h(),
                        crate::framebuffer::colorref_to_rgb565(color),
                    );
                }

                layer(panel.hwnd, surf);
            }
            self.render_children_of(surf, |parent| parent == panel.hwnd, gdi, layer);
            self.render_open_choices(surf, |parent| parent == panel.hwnd, gdi);
        }
    }

    fn render_open_choices(
        &self,
        surf: &mut Surface<'_>,
        owned: impl Fn(u32) -> bool,
        gdi: Option<&crate::gdi::GdiState>,
    ) {
        for child in &self.children {
            if child.class == ControlClass::ComboBox
                && child.choice_open
                && owned(child.parent)
                && self.is_showing(child)
            {
                let (ox, oy) = self.parent_origin(child.parent);
                let mut moved = child.clone();
                moved.x += ox;
                moved.y += oy;
                let face = gdi.and_then(|g| crate::text::selected(g, child.font));
                moved.render_choice(surf, child.hwnd == self.focus, face.as_ref());
            }
        }
    }

    /// Paint every visible child whose parent satisfies `owned`, group
    /// boxes first.
    fn render_children_of(
        &self,
        surf: &mut Surface<'_>,
        owned: impl Fn(u32) -> bool,
        gdi: Option<&crate::gdi::GdiState>,
        layer: &mut impl FnMut(u32, &mut Surface<'_>),
    ) {
        for pass in 0..3 {
            for child in &self.children {
                let child_pass = if child.class == ControlClass::ComboBox && child.choice_open {
                    2
                } else if child.is_group_box() {
                    0
                } else {
                    1
                };
                if child_pass != pass || !owned(child.parent) || !self.is_showing(child) {
                    continue;
                }
                let (ox, oy) = self.parent_origin(child.parent);
                let focused = child.hwnd == self.focus;
                if (ox, oy) == (0, 0) {
                    child.render_with_images(surf, focused, gdi);
                } else {
                    // Cheap: the translated copy lives only for the call.
                    let mut moved = child.clone();
                    moved.x += ox;
                    moved.y += oy;
                    moved.render_with_images(surf, focused, gdi);
                }
                // A sibling is one compositing unit: its native backing pixels
                // must not be replayed after all later siblings have painted.
                layer(child.hwnd, surf);
                // WS_BORDER is non-client decoration, outside the guest's
                // ListView paint. Replay it after the native client bitmap.
                if child.class == ControlClass::ListView && child.style & 0x0080_0000 != 0 {
                    stroke(surf, child.x + ox, child.y + oy, child.w, child.h, 0);
                }
            }
        }
    }
}

/// Map a virtual-key code to the character an `EDIT` would receive.
///
/// The host frontends deliver key presses as virtual keys, not
/// `WM_CHAR`, so this is where typing into a field becomes text. Only
/// the unshifted layout is modelled — there is no shift state to
/// consult.
fn vk_to_char(vk: u16) -> Option<char> {
    match vk {
        0x30..=0x39 => Some((b'0' + (vk - 0x30) as u8) as char),
        0x41..=0x5A => Some((b'a' + (vk - 0x41) as u8) as char),
        0x20 => Some(' '),
        // VK_NUMPAD0..9
        0x60..=0x69 => Some((b'0' + (vk - 0x60) as u8) as char),
        0xBD => Some('-'),
        0xBE => Some('.'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gdi::Bitmap;

    #[test]
    fn combo_parent_brush_does_not_erase_buttons_created_before_it() {
        let mut controls = Controls::default();
        // Dialog templates may declare the bottom buttons before the combo.
        controls.create(
            PARENT,
            ControlClass::Button,
            1,
            "OK".into(),
            WS_VISIBLE,
            62,
            190,
            80,
            34,
        );
        controls.create(
            PARENT,
            ControlClass::Button,
            2,
            "Cancel".into(),
            WS_VISIBLE,
            182,
            190,
            80,
            34,
        );
        let mut gdi = crate::gdi::GdiState::new();
        let font = gdi.create_font(18);
        controls.create(
            PARENT,
            ControlClass::Button,
            4,
            "Use Default Device Language".into(),
            WS_VISIBLE | 3,
            12,
            8,
            290,
            24,
        );
        controls.create(
            PARENT,
            ControlClass::Static,
            5,
            "Language".into(),
            WS_VISIBLE,
            4,
            40,
            64,
            20,
        );
        for child in &mut controls.children {
            child.font = font;
        }
        let mut bitmap = standard_canvas(320, 240);
        Surface::Bitmap(&mut bitmap).fill_rect(
            0,
            0,
            320,
            240,
            crate::framebuffer::colorref_to_rgb565(0x00efe2d6),
        );
        controls.render_with_images(&mut Surface::Bitmap(&mut bitmap), &gdi);
        let before = bitmap.pixels.clone();
        let combo = controls.create(
            PARENT,
            ControlClass::ComboBox,
            3,
            "UK English".into(),
            WS_VISIBLE | 3,
            72,
            40,
            150,
            180,
        );
        // WM_CTLCOLORSTATIC/WM_CTLCOLORLISTBOX returns the dialog's blue brush.
        controls.get_mut(combo).unwrap().background_color = Some(0x00efe2d6);
        controls.get_mut(combo).unwrap().font = font;
        controls.update_choice_fonts(&gdi);
        controls.render_with_images(&mut Surface::Bitmap(&mut bitmap), &gdi);
        if let Ok(path) = std::env::var("POCKETHLE_LANGUAGE_PREVIEW") {
            let mut fb = crate::Framebuffer::new(320, 240);
            fb.pixels.copy_from_slice(&bitmap.pixels);
            std::fs::write(path, fb.snapshot_ppm()).unwrap();
        }
        for (x, y) in [(75usize, 210usize), (210, 210)] {
            let offset = (y * 320 + x) * 2;
            assert_eq!(
                &bitmap.pixels[offset..offset + 2],
                &before[offset..offset + 2],
                "closed combo must preserve the button beneath ({x},{y})"
            );
        }
        // Every pixel below the closed field remains untouched by the combo.
        let top = 40 + controls.get(combo).unwrap().choice_closed_height() as usize;
        for y in top..240usize {
            let row = (y * 320 + 72) * 2;
            assert_eq!(
                &bitmap.pixels[row..row + 150 * 2],
                &before[row..row + 150 * 2]
            );
        }
    }

    fn standard_test_control(
        class: ControlClass,
        style: u32,
        text: &str,
        w: i32,
        h: i32,
    ) -> ChildWindow {
        let mut controls = Controls::default();
        let hwnd = controls.create(
            PARENT,
            class,
            1,
            text.to_string(),
            WS_VISIBLE | style,
            0,
            0,
            w,
            h,
        );
        controls.get(hwnd).unwrap().clone()
    }

    fn standard_canvas(w: u32, h: u32) -> Bitmap {
        let mut bitmap = Bitmap::new(w, h);
        Surface::Bitmap(&mut bitmap).fill_rect(0, 0, w as i32, h as i32, 0xffff);
        bitmap
    }

    #[test]
    fn standard_static_wraps_centers_and_clips_near_route() {
        let c = standard_test_control(ControlClass::Static, 1, "Near Route", 34, 16);
        let mut actual = standard_canvas(40, 24);
        c.render_static(&mut Surface::Bitmap(&mut actual), None);
        let mut expected = standard_canvas(40, 24);
        let mut surf = Surface::Bitmap(&mut expected);
        font::draw_str(&mut surf, 5, 0, "Near", 0);
        font::draw_str(&mut surf, 2, 8, "Route", 0);
        assert_eq!(actual.pixels, expected.pixels);
        let mut short = c;
        short.h = 8;
        let mut clipped = standard_canvas(40, 24);
        short.render_static(&mut Surface::Bitmap(&mut clipped), None);
        assert!(clipped.pixels[40 * 8 * 2..].iter().all(|&p| p == 255));
    }

    #[test]
    fn standard_static_respects_right_top_nowrap_and_vertical_center() {
        for (style, x, y) in [(2, 18, 0), (1, 9, 0), (0x200, 0, 8), (12, 0, 0)] {
            let c = standard_test_control(ControlClass::Static, style, "Type", 42, 24);
            let mut actual = standard_canvas(42, 24);
            c.render_static(&mut Surface::Bitmap(&mut actual), None);
            let mut expected = standard_canvas(42, 24);
            font::draw_str(&mut Surface::Bitmap(&mut expected), x, y, "Type", 0);
            assert_eq!(actual.pixels, expected.pixels, "style={style:x}");
        }
        let c = standard_test_control(ControlClass::Static, 12, "Near Route", 34, 24);
        let mut bitmap = standard_canvas(40, 24);
        c.render_static(&mut Surface::Bitmap(&mut bitmap), None);
        assert!(bitmap.pixels[40 * 8 * 2..].iter().all(|&p| p == 255));
    }

    #[test]
    fn standard_trackbar_draws_frequency_explicit_ticks_and_style_sides() {
        let mut c = standard_test_control(ControlClass::TrackBar, 1, "", 100, 30);
        c.track_range = (0, 10);
        c.track_tick_freq = 2;
        c.track_ticks.push(1);
        let color = |b: &Bitmap, x: usize, y: usize| {
            u16::from_le_bytes(
                b.pixels[(y * b.width as usize + x) * 2..][..2]
                    .try_into()
                    .unwrap(),
            )
        };
        let mut b = standard_canvas(100, 30);
        c.render_track(&mut Surface::Bitmap(&mut b), false);
        for x in [5, 14, 23, 41, 59, 77, 95] {
            assert_eq!(color(&b, x, 29), 0);
        }
        assert_eq!(color(&b, 32, 29), 0xffff);
        c.style |= 4;
        let mut top = standard_canvas(100, 30);
        c.render_track(&mut Surface::Bitmap(&mut top), false);
        assert_eq!(color(&top, 23, 0), 0);
        assert_eq!(color(&top, 23, 29), 0xffff);
        c.style |= 8;
        let mut both = standard_canvas(100, 30);
        c.render_track(&mut Surface::Bitmap(&mut both), false);
        assert_eq!(color(&both, 23, 0), 0);
        assert_eq!(color(&both, 23, 29), 0);
        c.style |= 0x10;
        let mut none = standard_canvas(100, 30);
        c.render_track(&mut Surface::Bitmap(&mut none), false);
        assert_eq!(color(&none, 23, 0), 0xffff);
        assert_eq!(color(&none, 23, 29), 0xffff);
    }

    const PUSH: u32 = WS_VISIBLE;
    const PARENT: u32 = 0xDEAD_0001;

    #[test]
    fn native_backing_is_painted_before_the_next_hle_sibling() {
        let mut controls = Controls::default();
        let background = controls.create(
            PARENT,
            ControlClass::Static,
            1,
            String::new(),
            WS_VISIBLE,
            0,
            0,
            100,
            80,
        );
        let _button = button(&mut controls, 2, 10, 10);
        let mut bitmap = Bitmap::new(140, 90);
        controls.render_composited(&mut Surface::Bitmap(&mut bitmap), |hwnd, surface| {
            if hwnd == background {
                surface.fill_rect(0, 0, 100, 80, 0xf800);
            }
        });
        let offset = (15 * 140 + 15) * 2;
        assert_ne!(
            u16::from_le_bytes([bitmap.pixels[offset], bitmap.pixels[offset + 1]]),
            0xf800
        );
    }

    #[test]
    fn child_z_order_controls_native_layers_and_pointer_targets() {
        let mut controls = Controls::default();
        let first = button(&mut controls, 1, 0, 0);
        let second = button(&mut controls, 2, 0, 0);
        let background = controls.create(
            PARENT,
            ControlClass::Static,
            3,
            String::new(),
            WS_VISIBLE,
            0,
            0,
            120,
            26,
        );
        assert!(controls.set_child_z_order(background, 1));
        let mut layers = Vec::new();
        let mut bitmap = Bitmap::new(120, 26);
        controls.render_composited(&mut Surface::Bitmap(&mut bitmap), |hwnd, _| {
            layers.push(hwnd)
        });
        assert_eq!(layers, [0, background, first, second]);
        assert!(controls.set_child_z_order(first, 0));
        assert_eq!(controls.hit_test(10, 10), Some(first));
        assert!(controls.set_child_z_order(first, second));
        assert_eq!(controls.hit_test(10, 10), Some(second));
        assert!(!controls.set_child_z_order(first, 0x1234));
        assert!(!controls.set_child_z_order(first, first));
    }

    fn button(ctrls: &mut Controls, id: u32, x: i32, y: i32) -> u32 {
        ctrls.create(
            PARENT,
            ControlClass::Button,
            id,
            "MsgBox".into(),
            PUSH,
            x,
            y,
            120,
            26,
        )
    }

    #[test]
    fn open_dropdown_covers_native_sibling_layers() {
        let mut controls = Controls::default();
        let combo = controls.create(
            PARENT,
            ControlClass::ComboBox,
            1,
            String::new(),
            WS_VISIBLE,
            10,
            70,
            100,
            25,
        );
        let c = controls.get_mut(combo).unwrap();
        c.list_items = vec![("First".into(), 0), ("Second".into(), 0)];
        c.choice_open = true;
        c.layout_choice_popup(70, 100);
        let sibling = controls.create(
            PARENT,
            ControlClass::Static,
            2,
            String::new(),
            WS_VISIBLE,
            10,
            40,
            100,
            20,
        );
        let mut bm = Bitmap::new(120, 100);
        controls.render_composited(&mut Surface::Bitmap(&mut bm), |h, surf| {
            if h == sibling {
                surf.fill_rect(10, 40, 100, 20, 0xf800);
            }
        });
        let index = (50 * 120 + 100) * 2;
        assert_eq!(
            u16::from_le_bytes([bm.pixels[index], bm.pixels[index + 1]]),
            0xffff
        );
    }

    #[test]
    fn dropdown_at_bottom_opens_above_and_hit_tests_above_siblings() {
        let mut controls = Controls::default();
        let combo = controls.create(
            PARENT,
            ControlClass::ComboBox,
            1444,
            String::new(),
            WS_VISIBLE,
            35,
            145,
            265,
            25,
        );
        let c = controls.get_mut(combo).unwrap();
        c.list_items = (0..12).map(|i| (format!("Choice {i}"), i)).collect();
        c.list_selected = Some(11);
        c.choice_open = true;
        c.layout_choice_popup(145, 240);
        assert!(c.choice_popup_y < 0);
        assert_eq!(c.list_scroll_y, 6);
        let y = c.y + c.choice_popup_y + 1 + 5 * 16 + 8;
        assert!(y >= 0 && y < 240);
        // A later sibling must not steal the dropdown's click.
        controls.create(
            PARENT,
            ControlClass::Button,
            10,
            "Behind".into(),
            WS_VISIBLE,
            35,
            y - 8,
            265,
            25,
        );
        assert_eq!(controls.hit_test(40, y), Some(combo));
        assert!(matches!(
            controls.pointer_down(40, y),
            Some(ControlAction::Notify { index: 11, .. })
        ));
        assert!(!controls.get(combo).unwrap().choice_open);
    }

    #[test]
    fn dropdown_rows_fit_small_viewport_and_scroll_to_selection() {
        let mut controls = Controls::default();
        let combo = controls.create(
            PARENT,
            ControlClass::ComboBox,
            1,
            String::new(),
            WS_VISIBLE,
            0,
            10,
            100,
            25,
        );
        let c = controls.get_mut(combo).unwrap();
        c.list_items = (0..12).map(|i| (i.to_string(), i)).collect();
        c.list_selected = Some(10);
        c.choice_open = true;
        c.layout_choice_popup(10, 80);
        assert_eq!(c.choice_popup_y, 20);
        assert!(10 + c.choice_popup_y + c.choice_popup_h <= 80);
        assert_eq!(c.list_scroll_y, 8);
        assert!(c.contains(5, 31));
        assert!(!c.contains(5, 80));
    }

    #[test]
    fn modal_keyboard_uses_own_cancel_default_and_tab_stops() {
        let mut ctrls = Controls::default();
        let other = button(&mut ctrls, 90, 0, 0);
        let modal = 0xDEAD0003;
        let accept = ctrls.create(
            modal,
            ControlClass::Button,
            IDOK,
            "Accept".into(),
            WS_VISIBLE | 0x10000 | 1,
            0,
            0,
            70,
            30,
        );
        let cancel = ctrls.create(
            modal,
            ControlClass::Button,
            IDCANCEL,
            "Cancel".into(),
            WS_VISIBLE | 0x10000,
            80,
            0,
            70,
            30,
        );
        let hidden = ctrls.create(
            modal,
            ControlClass::Button,
            99,
            "Hidden".into(),
            0x10000,
            0,
            0,
            70,
            30,
        );
        ctrls.focus = other;
        assert_eq!(
            ctrls.key_down_in(modal, 0x1B),
            Some(ControlAction::Clicked {
                parent: modal,
                id: IDCANCEL,
                hwnd: modal
            })
        );
        assert_eq!(
            ctrls.key_down_in(modal, 0x0D),
            Some(ControlAction::Clicked {
                parent: modal,
                id: IDOK,
                hwnd: accept
            })
        );
        assert_eq!(
            ctrls.key_down_in(modal, 0x09),
            Some(ControlAction::Consumed)
        );
        assert_eq!(ctrls.focus, cancel);
        assert_eq!(
            ctrls.key_down_in(modal, 0x0D),
            Some(ControlAction::Clicked {
                parent: modal,
                id: IDCANCEL,
                hwnd: cancel
            })
        );
        ctrls.key_down_in(modal, 0x09);
        assert_eq!(ctrls.focus, accept);
        assert_ne!(ctrls.focus, hidden);
    }

    #[test]
    fn controls_get_distinct_handles_from_the_parent() {
        let mut ctrls = Controls::default();
        let a = button(&mut ctrls, 201, 60, 72);
        let b = button(&mut ctrls, 202, 60, 108);
        assert_ne!(a, b);
        assert_ne!(a, PARENT);
        assert!(Controls::is_child_hwnd(a));
        assert!(Controls::is_child_hwnd(b));
        assert!(!Controls::is_child_hwnd(PARENT));
    }

    #[test]
    fn get_dlg_item_finds_a_control_by_id() {
        let mut ctrls = Controls::default();
        let a = button(&mut ctrls, 201, 60, 72);
        let b = button(&mut ctrls, 202, 60, 108);
        assert_eq!(ctrls.by_id(PARENT, 201).map(|c| c.hwnd), Some(a));
        assert_eq!(ctrls.by_id(PARENT, 202).map(|c| c.hwnd), Some(b));
        assert!(ctrls.by_id(PARENT, 999).is_none());
        // A different parent must not match.
        assert!(ctrls.by_id(0xDEAD_0002, 201).is_none());
    }

    #[test]
    fn a_press_and_release_inside_a_button_notifies_the_parent() {
        let mut ctrls = Controls::default();
        let hwnd = button(&mut ctrls, 201, 60, 72);
        assert_eq!(ctrls.pointer_down(70, 80), Some(ControlAction::Consumed));
        assert!(ctrls.get(hwnd).unwrap().pressed);
        assert_eq!(ctrls.focus, hwnd);
        assert_eq!(
            ctrls.pointer_up(70, 80),
            Some(ControlAction::Clicked {
                parent: PARENT,
                id: 201,
                hwnd
            })
        );
        assert!(!ctrls.get(hwnd).unwrap().pressed);
    }

    #[test]
    fn releasing_outside_the_button_cancels_the_click() {
        let mut ctrls = Controls::default();
        let hwnd = button(&mut ctrls, 201, 60, 72);
        ctrls.pointer_down(70, 80);
        // Slide off the button before lifting: Win32 captures the
        // mouse and drops the notification.
        assert_eq!(ctrls.pointer_up(70, 300), Some(ControlAction::Consumed));
        assert!(!ctrls.get(hwnd).unwrap().pressed);
    }

    #[test]
    fn a_tap_outside_every_control_is_left_to_the_application() {
        let mut ctrls = Controls::default();
        button(&mut ctrls, 201, 60, 72);
        assert_eq!(ctrls.pointer_down(10, 10), None);
        assert_eq!(ctrls.pointer_up(10, 10), None);
    }

    #[test]
    fn typing_into_a_focused_edit_accumulates_text() {
        let mut ctrls = Controls::default();
        let edit = ctrls.create(
            PARENT,
            ControlClass::Edit,
            203,
            String::new(),
            WS_VISIBLE,
            60,
            144,
            120,
            22,
        );
        ctrls.pointer_down(70, 150);
        assert_eq!(ctrls.focus, edit);
        for vk in [0x41u16, 0x42, 0x20, 0x39] {
            assert_eq!(ctrls.key_down(vk), Some(ControlAction::Consumed));
        }
        assert_eq!(ctrls.get(edit).unwrap().text, "ab 9");
        // VK_BACK rubs out the last character.
        assert_eq!(ctrls.key_down(0x08), Some(ControlAction::Consumed));
        assert_eq!(ctrls.get(edit).unwrap().text, "ab ");
    }

    #[test]
    fn keys_are_left_alone_when_no_control_has_the_focus() {
        let mut ctrls = Controls::default();
        ctrls.create(
            PARENT,
            ControlClass::Edit,
            203,
            String::new(),
            WS_VISIBLE,
            60,
            144,
            120,
            22,
        );
        // Never tapped, so nothing is focused: the game still gets its
        // D-pad presses.
        assert_eq!(ctrls.key_down(0x41), None);
    }

    #[test]
    fn an_auto_checkbox_latches_on_click() {
        let mut ctrls = Controls::default();
        let cb = ctrls.create(
            PARENT,
            ControlClass::Button,
            300,
            "Sound".into(),
            WS_VISIBLE | BS_AUTOCHECKBOX,
            10,
            10,
            100,
            16,
        );
        assert!(!ctrls.get(cb).unwrap().checked);
        ctrls.pointer_down(20, 15);
        ctrls.pointer_up(20, 15);
        assert!(ctrls.get(cb).unwrap().checked);
        ctrls.pointer_down(20, 15);
        ctrls.pointer_up(20, 15);
        assert!(!ctrls.get(cb).unwrap().checked);
    }

    #[test]
    fn a_static_label_never_takes_the_focus() {
        let mut ctrls = Controls::default();
        let label = ctrls.create(
            PARENT,
            ControlClass::Static,
            100,
            "Hello, world".into(),
            WS_VISIBLE,
            10,
            10,
            120,
            16,
        );
        assert_eq!(ctrls.pointer_down(20, 15), Some(ControlAction::Consumed));
        assert_ne!(ctrls.focus, label);
        assert_eq!(ctrls.focus, 0);
    }

    #[test]
    fn destroying_a_parent_takes_its_controls_with_it() {
        let mut ctrls = Controls::default();
        let a = button(&mut ctrls, 201, 60, 72);
        ctrls.focus = a;
        ctrls.destroy_children_of(PARENT);
        assert!(ctrls.is_empty());
        assert_eq!(ctrls.focus, 0);
    }

    #[test]
    fn list_view_tap_selects_the_row_actually_drawn_at_that_position() {
        let mut controls = Controls::default();
        let hwnd = controls.create(
            PARENT,
            ControlClass::ListView,
            1536,
            String::new(),
            0x10000000,
            10,
            10,
            100,
            60,
        );
        controls.get_mut(hwnd).unwrap().list_items = vec![
            ("New Trip".into(), 1),
            ("Enter Destination".into(), 2),
            ("Start Driving".into(), 3),
        ];
        controls.pointer_down(80, 44);
        assert_eq!(controls.get(hwnd).unwrap().list_selected, Some(2));
        assert!(matches!(
            controls.pointer_up(80, 44),
            Some(ControlAction::Notify { index: 2, .. })
        ));
        let mut bitmap = Bitmap::new(120, 80);
        let mut surface = Surface::Bitmap(&mut bitmap);
        controls.render(&mut surface);
        let offset = (44 * 120 + 108) * 2;
        assert_eq!(
            u16::from_le_bytes(surface.pixels()[offset..offset + 2].try_into().unwrap()),
            0x001f
        );
    }

    #[test]
    fn controls_paint_inside_their_own_rectangles() {
        let mut bm = Bitmap::new(240, 320);
        let mut surf = Surface::Bitmap(&mut bm);
        surf.fill_rect(0, 0, 240, 320, 0x07E0); // green backdrop
        let mut ctrls = Controls::default();
        button(&mut ctrls, 201, 60, 72);
        ctrls.render(&mut surf);

        let pixel = |x: i32, y: i32| -> u16 {
            let i = (y as usize * 240 + x as usize) * 2;
            let px = surf.pixels();
            u16::from_le_bytes([px[i], px[i + 1]])
        };
        // Inside the button: repainted.
        assert_ne!(pixel(100, 80), 0x07E0);
        // One pixel outside every edge: untouched.
        assert_eq!(pixel(59, 80), 0x07E0);
        assert_eq!(pixel(180, 80), 0x07E0);
        assert_eq!(pixel(100, 71), 0x07E0);
        assert_eq!(pixel(100, 98), 0x07E0);
    }

    #[test]
    fn a_zero_sized_control_paints_nothing() {
        let mut bm = Bitmap::new(64, 64);
        let mut surf = Surface::Bitmap(&mut bm);
        surf.fill_rect(0, 0, 64, 64, 0x07E0);
        let mut ctrls = Controls::default();
        // Created 0x0, as BlankApp does before its WM_SIZE runs.
        ctrls.create(
            PARENT,
            ControlClass::Button,
            201,
            "MsgBox".into(),
            WS_VISIBLE,
            0,
            0,
            0,
            0,
        );
        ctrls.render(&mut surf);
        assert!(surf
            .pixels()
            .chunks_exact(2)
            .all(|p| u16::from_le_bytes([p[0], p[1]]) == 0x07E0));
    }

    #[test]
    fn a_disabled_button_neither_focuses_nor_notifies() {
        let mut ctrls = Controls::default();
        // Solitaire's Undo button: WS_DISABLED until there is a move.
        let undo = ctrls.create(
            PARENT,
            ControlClass::Button,
            1002,
            "&Undo".into(),
            WS_VISIBLE | WS_DISABLED,
            10,
            10,
            77,
            22,
        );
        assert!(!ctrls.get(undo).unwrap().enabled);
        // The tap is swallowed — it must not reach the card table
        // underneath — but it takes neither the focus nor the press.
        assert_eq!(ctrls.pointer_down(20, 15), Some(ControlAction::Consumed));
        assert_eq!(ctrls.focus, 0);
        assert!(!ctrls.get(undo).unwrap().pressed);
        assert_eq!(ctrls.pointer_up(20, 15), Some(ControlAction::Consumed));

        // Nor does it answer a key, even if something forced the focus.
        ctrls.focus = undo;
        assert_eq!(ctrls.key_down(0x0D), Some(ControlAction::Consumed));
    }

    #[test]
    fn a_disabled_caption_is_etched_rather_than_plain_black() {
        let render = |style: u32| -> Vec<u16> {
            let mut bm = Bitmap::new(120, 40);
            let mut surf = Surface::Bitmap(&mut bm);
            // A static draws no background of its own, and a fresh
            // bitmap is all zeroes — which is `TEXT`. Lay down the
            // dialog face it would really sit on so "is there black
            // here" means something.
            surf.fill_rect(0, 0, 120, 40, ChildWindow::FACE);
            let mut ctrls = Controls::default();
            ctrls.create(
                PARENT,
                ControlClass::Static,
                1004,
                "Time:".into(),
                style,
                4,
                4,
                80,
                12,
            );
            ctrls.render(&mut surf);
            surf.pixels()
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect()
        };
        let on = render(WS_VISIBLE);
        let off = render(WS_VISIBLE | WS_DISABLED);
        assert_ne!(on, off);
        // Enabled: pure black glyphs, no highlight. Disabled: the
        // shadow/highlight pair and no black at all.
        assert!(on.contains(&ChildWindow::TEXT));
        assert!(!on.contains(&ChildWindow::SHADOW));
        assert!(off.contains(&ChildWindow::SHADOW));
        assert!(off.contains(&ChildWindow::LIGHT));
        assert!(!off.contains(&ChildWindow::TEXT));
    }

    /// A dialog panel with the Windows CE caption, sized and placed the
    /// way Solitaire's Options dialog is.
    fn captioned_panel(hwnd: u32) -> DialogPanel {
        DialogPanel {
            hwnd,
            x: 20,
            y: 24,
            w: 200,
            h: 100,
            visible: true,
            border: true,
            caption: Some("Options".into()),
            ok_pressed: false,
        }
    }

    #[test]
    fn a_caption_pushes_the_client_origin_below_it() {
        let bare = DialogPanel {
            caption: None,
            ..captioned_panel(0xDEAD_0002)
        };
        assert_eq!(bare.caption_h(), 0);
        assert_eq!(bare.client_origin(), (21, 25));

        let titled = captioned_panel(0xDEAD_0002);
        assert_eq!(titled.caption_h(), DialogPanel::CAPTION_H);
        assert_eq!(titled.client_origin(), (21, 25 + DialogPanel::CAPTION_H));
        // The OK box sits inside the caption, hard against the right
        // edge — never over the client area.
        let (bx, by, bw, bh) = titled.ok_box().expect("captioned");
        assert!(bx + bw < titled.x + titled.w);
        assert!(by > titled.y);
        assert!(by + bh <= titled.y + 1 + DialogPanel::CAPTION_H);
        assert!(titled.ok_box_contains(bx + bw / 2, by + bh / 2));
        assert!(!titled.ok_box_contains(bx - 4, by + bh / 2));
        assert!(bare.ok_box().is_none());
        assert!(!bare.ok_box_contains(bx + bw / 2, by + bh / 2));
    }

    #[test]
    fn tapping_the_caption_ok_box_reports_idok_from_the_panel() {
        const MODAL: u32 = 0xDEAD_0002;
        let mut ctrls = Controls::default();
        ctrls.add_panel(captioned_panel(MODAL));
        let (bx, by, bw, bh) = ctrls.panel(MODAL).unwrap().ok_box().unwrap();
        let (cx, cy) = (bx + bw / 2, by + bh / 2);

        assert_eq!(
            ctrls.pointer_down_in(MODAL, cx, cy),
            Some(ControlAction::Consumed)
        );
        assert!(ctrls.panel(MODAL).unwrap().ok_pressed);
        assert_eq!(
            ctrls.pointer_up_in(MODAL, cx, cy),
            Some(ControlAction::Clicked {
                parent: MODAL,
                id: IDOK,
                hwnd: MODAL,
            })
        );
        assert!(!ctrls.panel(MODAL).unwrap().ok_pressed);

        // Released off the box: armed, then cancelled, like any button.
        ctrls.pointer_down_in(MODAL, cx, cy);
        assert_eq!(
            ctrls.pointer_up_in(MODAL, cx - 40, cy),
            Some(ControlAction::Consumed)
        );
        assert!(!ctrls.panel(MODAL).unwrap().ok_pressed);
    }

    #[test]
    fn the_help_box_sits_left_of_the_ok_box_and_is_not_part_of_it() {
        const MODAL: u32 = 0xDEAD_0002;
        let panel = captioned_panel(MODAL);
        let (ox, oy, ow, oh) = panel.ok_box().unwrap();
        let (hx, hy, hw, hh) = panel.help_box().unwrap();

        // Same row, to the left, not overlapping.
        assert_eq!((hy, hh), (oy, oh));
        assert!(hx + hw < ox);
        // The OK box is wide enough for the word, which is the whole
        // reason it is not square like the `?` beside it.
        assert!(ow >= font::str_width("OK"));
        assert_eq!(hw, hh);
        // Tapping the `?` is not tapping OK.
        assert!(!panel.ok_box_contains(hx + hw / 2, hy + hh / 2));
    }

    #[test]
    fn the_caption_ok_box_is_labelled_rather_than_left_blank() {
        const MODAL: u32 = 0xDEAD_0002;
        let mut bm = Bitmap::new(120, 40);
        let mut surf = Surface::Bitmap(&mut bm);
        let mut panel = captioned_panel(MODAL);
        (panel.x, panel.y, panel.w, panel.h) = (0, 0, 120, 40);
        panel.render(&mut surf);

        let (bx, by, bw, bh) = panel.ok_box().unwrap();
        let px = |x: i32, y: i32| -> u16 {
            let o = (y * 120 + x) as usize * 2;
            let p = surf.pixels();
            u16::from_le_bytes([p[o], p[o + 1]])
        };
        // Some pixel inside the box carries the label colour, and it is
        // neither the face nor the caption background behind it.
        let labelled = (by..by + bh).any(|y| (bx..bx + bw).any(|x| px(x, y) == DialogPanel::OK_FG));
        assert!(labelled, "the OK box drew no label");
        assert_ne!(DialogPanel::OK_FG, ChildWindow::FACE);
    }

    #[test]
    fn check_radio_button_selects_one_and_clears_the_rest_of_the_range() {
        const DLG: u32 = 0xDEAD_0002;
        const OTHER: u32 = 0xDEAD_0003;
        let mut ctrls = Controls::default();
        // Three radios in one group, plus a same-id control belonging to
        // a different dialog that must not be disturbed.
        let radio = |ctrls: &mut Controls, parent: u32, id: u32| {
            ctrls.create(
                parent,
                ControlClass::Button,
                id,
                String::new(),
                WS_VISIBLE | BS_AUTORADIOBUTTON,
                0,
                0,
                10,
                10,
            )
        };
        for id in 100..103 {
            let h = radio(&mut ctrls, DLG, id);
            ctrls.get_mut(h).unwrap().checked = id == 100;
        }
        let stranger = radio(&mut ctrls, OTHER, 101);
        ctrls.get_mut(stranger).unwrap().checked = true;

        assert_eq!(ctrls.check_radio_range(DLG, 100, 102, 101), 3);
        assert!(!ctrls.by_id(DLG, 100).unwrap().checked);
        assert!(ctrls.by_id(DLG, 101).unwrap().checked);
        assert!(!ctrls.by_id(DLG, 102).unwrap().checked);
        // The other dialog's control kept its state.
        assert!(ctrls.get(stranger).unwrap().checked);
        // A range that matches nothing reports so rather than panicking.
        assert_eq!(ctrls.check_radio_range(DLG, 900, 999, 950), 0);
    }

    #[test]
    fn a_dialog_on_top_hides_the_buttons_of_the_one_below() {
        const STRIP: u32 = 0xDEAD_0002;
        const MODAL: u32 = 0xDEAD_0003;
        let mut bm = Bitmap::new(120, 60);
        let mut surf = Surface::Bitmap(&mut bm);
        let mut ctrls = Controls::default();

        // A button strip along the right, then a modal over the top of
        // it — the Solitaire layout that exposed this.
        ctrls.add_panel(DialogPanel {
            hwnd: STRIP,
            x: 80,
            y: 0,
            w: 40,
            h: 60,
            visible: true,
            border: false,
            caption: None,
            ok_pressed: false,
        });
        ctrls.create(
            STRIP,
            ControlClass::Button,
            1000,
            "Exit".into(),
            WS_VISIBLE,
            0,
            0,
            38,
            20,
        );
        ctrls.add_panel(DialogPanel {
            hwnd: MODAL,
            x: 10,
            y: 5,
            w: 100,
            h: 50,
            visible: true,
            border: true,
            caption: Some("Options".into()),
            ok_pressed: false,
        });
        ctrls.render(&mut surf);

        let px = |x: usize, y: usize| -> u16 {
            let o = (y * 120 + x) * 2;
            let p = surf.pixels();
            u16::from_le_bytes([p[o], p[o + 1]])
        };
        // Inside the modal's caption, over where Exit sits: the caption
        // colour, not the button's face or its text.
        assert_eq!(px(90, 8), DialogPanel::CAPTION_BG);
        // The strip's button is still painted where the modal does not
        // reach it.
        assert_ne!(px(112, 2), DialogPanel::CAPTION_BG);
    }

    #[test]
    fn a_group_box_frames_its_radios_without_covering_them() {
        const MODAL: u32 = 0xDEAD_0002;
        const BS_AUTORADIO: u32 = 0x0009;
        let mut ctrls = Controls::default();
        // Solitaire's "Draw" group and the two radios inside it, at the
        // sizes the template really gives them.
        let group = ctrls.create(
            MODAL,
            ControlClass::Button,
            1009,
            "Draw".into(),
            WS_VISIBLE | BS_GROUPBOX,
            5,
            0,
            113,
            84,
        );
        let one = ctrls.create(
            MODAL,
            ControlClass::Button,
            1010,
            "One".into(),
            WS_VISIBLE | BS_AUTORADIO,
            15,
            24,
            75,
            21,
        );

        // The frame is not a button: it never takes the focus, and a tap
        // inside it reaches the radio rather than the frame.
        assert!(ctrls.get(group).unwrap().is_group_box());
        assert!(!ctrls.get(group).unwrap().is_push_button());
        assert!(!ctrls.get(group).unwrap().is_focusable());
        assert_eq!(ctrls.hit_test(20, 30), Some(one));
        // A point inside the frame but on no radio hits nothing at all.
        assert_eq!(ctrls.hit_test(20, 75), None);
    }

    #[test]
    fn a_group_box_is_an_etched_frame_not_a_raised_face() {
        let render = |style: u32| -> Vec<u16> {
            let mut bm = Bitmap::new(120, 60);
            let mut surf = Surface::Bitmap(&mut bm);
            surf.fill_rect(0, 0, 120, 60, ChildWindow::FACE);
            let mut ctrls = Controls::default();
            ctrls.create(
                PARENT,
                ControlClass::Button,
                1009,
                "Draw".into(),
                style,
                4,
                4,
                100,
                50,
            );
            ctrls.render(&mut surf);
            surf.pixels()
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect()
        };
        let group = render(WS_VISIBLE | BS_GROUPBOX);
        let push = render(WS_VISIBLE);
        assert_ne!(group, push);
        // Etched: both groove colours are present, and the caption is
        // still drawn in black.
        assert!(group.contains(&ChildWindow::SHADOW));
        assert!(group.contains(&ChildWindow::LIGHT));
        assert!(group.contains(&ChildWindow::TEXT));
        // The middle of a group box is untouched face — a push button
        // would have stamped its caption across the centre.
        let centre = 30 * 120 + 54;
        assert_eq!(group[centre], ChildWindow::FACE);
        assert_ne!(push[centre], ChildWindow::FACE);
    }

    #[test]
    fn class_names_are_matched_case_insensitively() {
        assert_eq!(
            ControlClass::from_class_name("BUTTON"),
            Some(ControlClass::Button)
        );
        assert_eq!(
            ControlClass::from_class_name("Edit"),
            Some(ControlClass::Edit)
        );
        assert_eq!(
            ControlClass::from_class_name("static"),
            Some(ControlClass::Static)
        );
        assert_eq!(ControlClass::from_class_name("CerfBlankApp"), None);
    }
}
