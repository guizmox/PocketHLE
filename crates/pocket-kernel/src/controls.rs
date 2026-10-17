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
    pub list_selected: Option<usize>,
    pub list_column_count: u32,
    pub font: u32,
}

impl ChildWindow {
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
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
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
    fn draw_caption(&self, surf: &mut Surface<'_>, x: i32, y: i32, right: i32) {
        if self.enabled {
            draw_clipped(surf, x, y, &self.text, Self::TEXT, right);
            return;
        }
        draw_clipped(surf, x + 1, y + 1, &self.text, Self::LIGHT, right + 1);
        draw_clipped(surf, x, y, &self.text, Self::SHADOW, right);
    }

    /// Paint the control. `focused` drives the focus rectangle and the
    /// edit caret; the caller knows which control holds the focus.
    pub fn render(&self, surf: &mut Surface<'_>, focused: bool) {
        if !self.visible || self.w <= 0 || self.h <= 0 {
            return;
        }
        match self.class {
            ControlClass::Button => self.render_button(surf, focused),
            ControlClass::Edit => self.render_edit(surf, focused),
            ControlClass::Static => self.render_static(surf),
            ControlClass::ListView => {
                surf.fill_rect(self.x, self.y, self.w, self.h, Self::WINDOW);
                for (index, (text, _)) in self.list_items.iter().enumerate() {
                    let y = self.y + index as i32 * 14;
                    if y + 14 > self.y + self.h {
                        break;
                    }
                    let selected = self.list_selected == Some(index);
                    if selected {
                        surf.fill_rect(self.x, y, self.w, 14, 0x001f);
                    }
                    draw_clipped(
                        surf,
                        self.x + 2,
                        y + 3,
                        text,
                        if selected { Self::WINDOW } else { Self::TEXT },
                        self.x + self.w - 1,
                    );
                }
            }
        }
    }

    fn render_button(&self, surf: &mut Surface<'_>, focused: bool) {
        if self.is_group_box() {
            return self.render_group_box(surf);
        }
        if self.is_check_like() {
            return self.render_check(surf, focused);
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
        let text_w = font::str_width(&self.text);
        let tx = x + ((w - text_w) / 2).max(1) + nudge;
        let ty = y + ((h - font::GLYPH_H) / 2).max(0) + nudge;
        self.draw_caption(surf, tx, ty, x + w - 2);

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
    fn render_group_box(&self, surf: &mut Surface<'_>) {
        let top = self.y + font::GLYPH_H / 2;
        let h = self.h - font::GLYPH_H / 2;
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
        let text_w = font::str_width(&self.text).min(self.w - Self::GROUP_LABEL_X - 2);
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
        );
    }

    fn render_check(&self, surf: &mut Surface<'_>, focused: bool) {
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
        let ty = self.y + ((self.h - font::GLYPH_H) / 2).max(0);
        self.draw_caption(surf, tx, ty, self.x + self.w);
        if focused {
            dotted_rect(surf, tx - 2, self.y, self.w - side - 2, self.h, Self::TEXT);
        }
    }

    fn render_edit(&self, surf: &mut Surface<'_>, focused: bool) {
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
        let max_chars = (inner_w / font::GLYPH_W).max(0) as usize;
        let len = self.text.chars().count();
        let shown: String = if len > max_chars {
            self.text.chars().skip(len - max_chars).collect()
        } else {
            self.text.clone()
        };
        let ty = self.y + ((self.h - font::GLYPH_H) / 2).max(1);
        font::draw_str(surf, self.x + 3, ty, &shown, Self::TEXT);
        if focused {
            let caret_x = self.x + 3 + font::str_width(&shown);
            if caret_x < self.x + self.w - 1 {
                surf.fill_rect(caret_x, ty, 1, font::GLYPH_H, Self::TEXT);
            }
        }
    }

    fn render_static(&self, surf: &mut Surface<'_>) {
        let ty = self.y + ((self.h - font::GLYPH_H) / 2).max(0);
        self.draw_caption(surf, self.x, ty, self.x + self.w);
    }
}

/// Draw `text` but stop before `right`, so a caption never bleeds out
/// of its control.
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
            x,
            y,
            w,
            h,
            visible: style & WS_VISIBLE != 0,
            enabled: style & WS_DISABLED == 0,
            pressed: false,
            checked: false,
            list_items: Vec::new(),
            list_selected: None,
            list_column_count: 0,
            font: crate::gdi::STOCK_SYSTEM_FONT,
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
        if child.class == ControlClass::ListView && child.enabled {
            let index = ((y - origin.1 - child.y).max(0) / 24) as usize;
            if index < child.list_items.len() {
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
            ControlClass::ListView if vk == 0x26 || vk == 0x28 => {
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
    /// every dialog; panels then follow in creation order, so the most
    /// recently created dialog — the modal — ends up on top.
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
        self.render_children_of(surf, |parent| self.panel(parent).is_none());
        layer(0, surf);
        for panel in &self.panels {
            panel.render(surf);
            self.render_children_of(surf, |parent| parent == panel.hwnd);
            if panel.visible {
                layer(panel.hwnd, surf);
            }
        }
    }

    /// Paint every visible child whose parent satisfies `owned`, group
    /// boxes first.
    fn render_children_of(&self, surf: &mut Surface<'_>, owned: impl Fn(u32) -> bool) {
        for group_pass in [true, false] {
            for child in &self.children {
                if child.is_group_box() != group_pass
                    || !owned(child.parent)
                    || !self.is_showing(child)
                {
                    continue;
                }
                let (ox, oy) = self.parent_origin(child.parent);
                let focused = child.hwnd == self.focus;
                if (ox, oy) == (0, 0) {
                    child.render(surf, focused);
                } else {
                    // Cheap: the translated copy lives only for the call.
                    let mut moved = child.clone();
                    moved.x += ox;
                    moved.y += oy;
                    moved.render(surf, focused);
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

    const PUSH: u32 = WS_VISIBLE;
    const PARENT: u32 = 0xDEAD_0001;

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
