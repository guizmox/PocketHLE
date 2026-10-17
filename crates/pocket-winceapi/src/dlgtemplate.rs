//! `DLGTEMPLATE` / `DLGITEMTEMPLATE` parsing.
//!
//! A dialog created with `CreateDialogIndirectParamW` arrives as a blob
//! of packed structures rather than a series of `CreateWindowExW` calls:
//! the header describes the panel, and `cdit` variable-length items
//! describe the controls inside it. On a device `USER` walks that blob
//! and creates the children itself before `WM_INITDIALOG` is sent, which
//! is why an application never creates them and why ignoring the
//! template leaves the dialog empty.
//!
//! Solitaire's right-hand button strip — Exit / Help / Deal / Undo /
//! Options and the Time / Score readouts — is one of these.

/// The predefined class ordinals a `DLGITEMTEMPLATE` may name instead of
/// spelling the class out.
pub mod class_ordinal {
    pub const BUTTON: u16 = 0x0080;
    pub const EDIT: u16 = 0x0081;
    pub const STATIC: u16 = 0x0082;
    pub const LISTBOX: u16 = 0x0083;
    pub const SCROLLBAR: u16 = 0x0084;
    pub const COMBOBOX: u16 = 0x0085;
}

/// `DS_SETFONT` — the header carries a point size and typeface.
const DS_SETFONT: u32 = 0x0040;

/// How a `DLGITEMTEMPLATE` named its window class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemClass {
    /// One of the `class_ordinal` values.
    Ordinal(u16),
    /// A registered class name, for a custom control.
    Named(String),
}

/// One control inside a dialog template. Coordinates are in dialog
/// units, not pixels — see [`Dlu::to_px`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogItem {
    pub style: u32,
    pub ex_style: u32,
    pub x: i16,
    pub y: i16,
    pub cx: i16,
    pub cy: i16,
    pub id: u16,
    pub class: ItemClass,
    pub title: String,
}

/// A parsed dialog template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogTemplate {
    pub style: u32,
    pub ex_style: u32,
    pub x: i16,
    pub y: i16,
    pub cx: i16,
    pub cy: i16,
    pub title: String,
    pub items: Vec<DialogItem>,
}

/// Dialog-unit to pixel conversion.
///
/// `MapDialogRect` scales by the dialog's base units: a quarter of the
/// average character width horizontally, an eighth of the character
/// height vertically. Those come from the dialog's font, and a template
/// without `DS_SETFONT` — Solitaire's panel is one — uses the system
/// font, so a single pair of constants covers every dialog we see.
///
/// The pair below is fitted to the Windows CE 4.0 shell font: it
/// reproduces Solitaire's reference layout exactly, with its 44x13 DLU
/// buttons landing on the 77x22 pixel bands of the real screenshot.
#[derive(Debug, Clone, Copy)]
pub struct Dlu {
    pub base_x: i32,
    pub base_y: i32,
}

impl Default for Dlu {
    fn default() -> Self {
        Self {
            base_x: 7,
            base_y: 14,
        }
    }
}

impl Dlu {
    /// Scale a horizontal dialog-unit span to pixels.
    pub fn x_to_px(&self, dlu: i32) -> i32 {
        dlu * self.base_x / 4
    }

    /// Scale a vertical dialog-unit span to pixels.
    pub fn y_to_px(&self, dlu: i32) -> i32 {
        dlu * self.base_y / 8
    }

    /// Scale a whole rectangle.
    pub fn to_px(&self, x: i32, y: i32, cx: i32, cy: i32) -> (i32, i32, i32, i32) {
        (
            self.x_to_px(x),
            self.y_to_px(y),
            self.x_to_px(cx),
            self.y_to_px(cy),
        )
    }
}

/// A bounds-checked cursor over the template blob.
///
/// The blob is guest memory of unknown length — we read a generous
/// window and parse within it — so every field read has to be able to
/// fail rather than panic on a short buffer.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn u16(&mut self) -> Option<u16> {
        let b = self.buf.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }

    fn u32(&mut self) -> Option<u32> {
        let b = self.buf.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Advance to the next 4-byte boundary, as each `DLGITEMTEMPLATE`
    /// requires.
    fn align4(&mut self) {
        self.pos = (self.pos + 3) & !3;
    }

    /// A `sz_Or_Ord`: nothing, an ordinal, or a NUL-terminated UTF-16
    /// string.
    fn sz_or_ord(&mut self) -> Option<Option<ItemClass>> {
        match self.u16()? {
            0x0000 => Some(None),
            0xFFFF => Some(Some(ItemClass::Ordinal(self.u16()?))),
            first => {
                let mut units = vec![first];
                loop {
                    match self.u16()? {
                        0 => break,
                        u => units.push(u),
                    }
                }
                Some(Some(ItemClass::Named(String::from_utf16_lossy(&units))))
            }
        }
    }

    /// The same field where only a string is meaningful — a title given
    /// as an ordinal is a resource id we cannot resolve, so it reads as
    /// empty.
    fn sz(&mut self) -> Option<String> {
        Some(match self.sz_or_ord()? {
            Some(ItemClass::Named(s)) => s,
            _ => String::new(),
        })
    }
}

/// Parse a `DLGTEMPLATE` and its items.
///
/// Returns `None` if the buffer is too short or the header is not
/// plausibly a template — a guest passing a bad pointer must not take
/// the emulator down with it.
pub fn parse(buf: &[u8]) -> Option<DialogTemplate> {
    let mut r = Reader::new(buf);
    let style = r.u32()?;
    let ex_style = r.u32()?;
    let cdit = r.u16()?;
    let x = r.i16()?;
    let y = r.i16()?;
    let cx = r.i16()?;
    let cy = r.i16()?;

    // `DLGTEMPLATEEX` starts with a version/signature pair of 1 and
    // 0xFFFF. We do not implement it, and misreading one as a classic
    // template would produce nonsense geometry.
    if style & 0xFFFF == 0x0001 && style >> 16 == 0xFFFF {
        return None;
    }
    // Empty dialogs are valid containers for controls created by the guest
    // during WM_INITDIALOG. Keep their parent/visibility state too.
    // A four-digit count means we are not looking at a template.
    if cdit > 255 {
        return None;
    }

    // menu and class, both usually absent for a dialog.
    let _menu = r.sz_or_ord()?;
    let _class = r.sz_or_ord()?;
    let title = r.sz()?;
    if style & DS_SETFONT != 0 {
        let _point_size = r.u16()?;
        let _typeface = r.sz()?;
    }

    let mut items = Vec::with_capacity(cdit as usize);
    for _ in 0..cdit {
        r.align4();
        let istyle = r.u32()?;
        let iex = r.u32()?;
        let ix = r.i16()?;
        let iy = r.i16()?;
        let icx = r.i16()?;
        let icy = r.i16()?;
        let id = r.u16()?;
        let class = r.sz_or_ord()?;
        let ititle = r.sz()?;
        // Creation data: a counted blob handed to the control's
        // WM_CREATE. Nothing we model reads it, but it has to be
        // stepped over to reach the next item.
        let cb = r.u16()? as usize;
        r.pos = r.pos.checked_add(cb)?;

        items.push(DialogItem {
            style: istyle,
            ex_style: iex,
            x: ix,
            y: iy,
            cx: icx,
            cy: icy,
            id,
            class: class?,
            title: ititle,
        });
    }

    Some(DialogTemplate {
        style,
        ex_style,
        x,
        y,
        cx,
        cy,
        title,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a template byte-for-byte the way a resource compiler does,
    /// so the parser is tested against the layout rather than against
    /// itself.
    struct Builder(Vec<u8>);

    impl Builder {
        fn new() -> Self {
            Self(Vec::new())
        }
        fn u16(&mut self, v: u16) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn u32(&mut self, v: u32) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn wstr(&mut self, s: &str) -> &mut Self {
            for u in s.encode_utf16() {
                self.u16(u);
            }
            self.u16(0)
        }
        fn align4(&mut self) -> &mut Self {
            while !self.0.len().is_multiple_of(4) {
                self.0.push(0);
            }
            self
        }
    }

    /// Solitaire's right-hand panel, reduced to three of its nine items.
    fn solitaire_panel() -> Vec<u8> {
        let mut b = Builder::new();
        b.u32(0x5080_0000) // WS_CHILD | WS_VISIBLE | WS_BORDER
            .u32(0) // exstyle
            .u16(3) // cdit
            .u16(267)
            .u16(3)
            .u16(47)
            .u16(118)
            .u16(0) // no menu
            .u16(0) // no class
            .u16(0); // no title

        // id=1000 BUTTON "E&xit"
        b.align4()
            .u32(0x5001_0000)
            .u32(0)
            .u16(2)
            .u16(2)
            .u16(44)
            .u16(13)
            .u16(1000)
            .u16(0xFFFF)
            .u16(class_ordinal::BUTTON)
            .wstr("E&xit")
            .u16(0); // cbCreationData

        // id=1002 BUTTON "&Undo", disabled
        b.align4()
            .u32(0x5801_0000)
            .u32(0)
            .u16(2)
            .u16(51)
            .u16(44)
            .u16(13)
            .u16(1002)
            .u16(0xFFFF)
            .u16(class_ordinal::BUTTON)
            .wstr("&Undo")
            .u16(0);

        // id=1006 STATIC "0:00"
        b.align4()
            .u32(0x5002_0001)
            .u32(0)
            .u16(2)
            .u16(89)
            .u16(44)
            .u16(10)
            .u16(1006)
            .u16(0xFFFF)
            .u16(class_ordinal::STATIC)
            .wstr("0:00")
            .u16(0);

        b.0
    }

    #[test]
    fn parses_the_solitaire_panel_header_and_items() {
        let t = parse(&solitaire_panel()).expect("template parses");
        assert_eq!(t.style, 0x5080_0000);
        assert_eq!((t.x, t.y, t.cx, t.cy), (267, 3, 47, 118));
        assert_eq!(t.title, "");
        assert_eq!(t.items.len(), 3);

        assert_eq!(t.items[0].id, 1000);
        assert_eq!(t.items[0].class, ItemClass::Ordinal(class_ordinal::BUTTON));
        assert_eq!(t.items[0].title, "E&xit");
        assert_eq!(
            (t.items[0].x, t.items[0].y, t.items[0].cx, t.items[0].cy),
            (2, 2, 44, 13)
        );

        // WS_DISABLED survives into the item style.
        assert_eq!(t.items[1].id, 1002);
        assert_ne!(t.items[1].style & 0x0800_0000, 0);
        assert_eq!(t.items[0].style & 0x0800_0000, 0);

        assert_eq!(t.items[2].id, 1006);
        assert_eq!(t.items[2].class, ItemClass::Ordinal(class_ordinal::STATIC));
        assert_eq!(t.items[2].title, "0:00");
    }

    #[test]
    fn a_named_class_and_creation_data_are_stepped_over() {
        let mut b = Builder::new();
        b.u32(0x5000_0000)
            .u32(0)
            .u16(2)
            .u16(0)
            .u16(0)
            .u16(100)
            .u16(50)
            .u16(0)
            .u16(0)
            .u16(0);
        // A custom class, plus creation data the next item must not be
        // read out of.
        b.align4()
            .u32(0x5000_0000)
            .u32(0)
            .u16(1)
            .u16(1)
            .u16(10)
            .u16(10)
            .u16(7)
            .wstr("MyGrid")
            .wstr("grid")
            .u16(3);
        b.0.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        b.align4()
            .u32(0x5000_0000)
            .u32(0)
            .u16(2)
            .u16(2)
            .u16(20)
            .u16(20)
            .u16(8)
            .u16(0xFFFF)
            .u16(class_ordinal::EDIT)
            .wstr("tail")
            .u16(0);

        let t = parse(&b.0).expect("template parses");
        assert_eq!(t.items.len(), 2);
        assert_eq!(t.items[0].class, ItemClass::Named("MyGrid".into()));
        assert_eq!(t.items[0].title, "grid");
        // The second item is intact, so the creation data was skipped by
        // exactly its own length.
        assert_eq!(t.items[1].id, 8);
        assert_eq!(t.items[1].class, ItemClass::Ordinal(class_ordinal::EDIT));
        assert_eq!(t.items[1].title, "tail");
    }

    #[test]
    fn a_dsetfont_header_skips_the_typeface() {
        let mut b = Builder::new();
        b.u32(0x5000_0040) // DS_SETFONT
            .u32(0)
            .u16(1)
            .u16(0)
            .u16(0)
            .u16(100)
            .u16(50)
            .u16(0)
            .u16(0)
            .wstr("Options")
            .u16(9) // point size
            .wstr("Tahoma");
        b.align4()
            .u32(0x5000_0000)
            .u32(0)
            .u16(5)
            .u16(5)
            .u16(30)
            .u16(12)
            .u16(42)
            .u16(0xFFFF)
            .u16(class_ordinal::BUTTON)
            .wstr("&Vegas")
            .u16(0);

        let t = parse(&b.0).expect("template parses");
        assert_eq!(t.title, "Options");
        assert_eq!(t.items.len(), 1);
        assert_eq!(t.items[0].id, 42);
        assert_eq!(t.items[0].title, "&Vegas");
    }

    #[test]
    fn a_truncated_or_implausible_blob_is_refused() {
        let full = solitaire_panel();
        // Cut mid-item: the parser must fail rather than invent one.
        assert!(parse(&full[..full.len() - 6]).is_none());
        assert!(parse(&[]).is_none());
        assert!(parse(&[0u8; 8]).is_none());
        // DLGTEMPLATEEX, which we do not implement.
        let mut ex = Builder::new();
        ex.u32(0xFFFF_0001)
            .u32(0)
            .u16(2)
            .u16(0)
            .u16(0)
            .u16(9)
            .u16(9);
        assert!(parse(&ex.0).is_none());
    }

    #[test]
    fn dialog_units_scale_the_reference_layout() {
        let dlu = Dlu::default();
        // Solitaire's panel: 47x118 DLU is the 82x206 pixel client area
        // measured off the reference screenshot.
        assert_eq!(dlu.x_to_px(47), 82);
        assert_eq!(dlu.y_to_px(118), 206);
        // Its buttons: 44x13 DLU on the reference's 77x22 bands.
        assert_eq!(dlu.x_to_px(44), 77);
        assert_eq!(dlu.y_to_px(13), 22);
        // And the five button tops, at 3 px below the client origin.
        let tops: Vec<i32> = [2, 17, 36, 51, 66]
            .iter()
            .map(|d| dlu.y_to_px(*d))
            .collect();
        assert_eq!(tops, vec![3, 29, 63, 89, 115]);
    }
}
