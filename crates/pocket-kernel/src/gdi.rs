//! Stateful GDI object table.
//!
//! Win32 GDI exposes `HDC`, `HBITMAP`, `HBRUSH`, `HPEN`, `HFONT` as
//! opaque handles. PocketHLE used to return a fake non-zero handle
//! per `Create*` call and never tracked any of them; calls to the
//! actual rendering primitives were therefore no-ops. This module
//! adds a minimal but real implementation of the parts that
//! JumpyBall (and most equivalent Pocket PC games) exercise:
//!
//! * Memory device contexts that own a back-buffer bitmap.
//! * `CreateCompatibleBitmap` allocates a 16 bpp surface so that a
//!   subsequent `BitBlt` between the memory DC and the screen DC is
//!   a straight 1:1 copy.
//! * `CreateSolidBrush` / `CreatePen` colour values get tracked per
//!   handle, then per-DC when `SelectObject` ties them together.
//!
//! The rendering primitives themselves live in
//! [`pocket-winceapi`](../../pocket-winceapi); this module only
//! manages the **data**.

use std::collections::HashMap;

use crate::framebuffer::Framebuffer;

/// Minimum non-zero handle used for GDI objects. Picked to look
/// obviously synthetic in logs.
pub const GDI_HANDLE_BASE: u32 = 0xDEAD_1000;

/// Pseudo-handle returned by `GetDC(NULL)` etc. — represents the
/// hardware screen.
pub const GDI_SCREEN_DC: u32 = 0xDEAD_0DC0;
/// Stock white brush handle (matches `GetStockObject(WHITE_BRUSH)`).
pub const STOCK_WHITE_BRUSH: u32 = 0xDEAD_5701;
pub const STOCK_BLACK_BRUSH: u32 = 0xDEAD_5704;
pub const STOCK_NULL_BRUSH: u32 = 0xDEAD_5705;
pub const STOCK_BLACK_PEN: u32 = 0xDEAD_5707;
pub const STOCK_WHITE_PEN: u32 = 0xDEAD_5706;
pub const STOCK_NULL_PEN: u32 = 0xDEAD_5708;
pub const STOCK_LTGRAY_BRUSH: u32 = 0xDEAD_5702;
pub const STOCK_GRAY_BRUSH: u32 = 0xDEAD_5703;
pub const STOCK_DKGRAY_BRUSH: u32 = 0xDEAD_5709;
/// `GetStockObject(SYSTEM_FONT)` / `DEFAULT_GUI_FONT`.
pub const STOCK_SYSTEM_FONT: u32 = 0xDEAD_5710;
/// The 1x1 monochrome bitmap every real memory DC starts out with.
/// `SelectObject(memdc, hbm)` must return *this* rather than NULL for a
/// fresh DC — guests routinely test the result and bail on NULL.
pub const STOCK_DEFAULT_BITMAP: u32 = 0xDEAD_5720;
pub const STOCK_DEFAULT_PALETTE: u32 = 0xDEAD_5721;

/// Handles that are pre-registered in [`GdiState::new`] and must outlive
/// every `DeleteObject`, just like real GDI stock objects.
pub fn is_stock_handle(handle: u32) -> bool {
    matches!(
        handle,
        GDI_SCREEN_DC
            | STOCK_WHITE_BRUSH
            | STOCK_LTGRAY_BRUSH
            | STOCK_GRAY_BRUSH
            | STOCK_BLACK_BRUSH
            | STOCK_NULL_BRUSH
            | STOCK_WHITE_PEN
            | STOCK_BLACK_PEN
            | STOCK_NULL_PEN
            | STOCK_DKGRAY_BRUSH
            | STOCK_SYSTEM_FONT
            | STOCK_DEFAULT_PALETTE
            | STOCK_DEFAULT_BITMAP
    )
}

/// Whether a DC paints into the on-screen framebuffer or into an
/// off-screen [`Bitmap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DcSurface {
    /// Paints into [`Framebuffer`].
    Screen,
    /// Paints into the bitmap with this handle, if one is selected.
    Memory,
}

#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// Bits per pixel of the *original* DIB. Internally we always
    /// keep an RGB565 copy in [`Bitmap::pixels`], but DIB-backed
    /// bitmaps remember their original depth so callers like
    /// `GetObjectW` can report the correct value.
    pub bpp: u16,
    /// 16 bpp RGB565 little-endian. `width * 2` is the row stride.
    /// For DIB-backed bitmaps this is a synced copy of the guest's
    /// pixel buffer at [`Bitmap::dib_bits_va`] — `BitBlt` re-reads
    /// the guest VA on demand to stay current.
    pub pixels: Vec<u8>,
    /// Window backing stores retain only pixels actually painted by GDI.
    /// Unpainted pixels must reveal the underlying window, not allocation zeros.
    /// Ordinary compatible bitmaps and guest DIBs remain opaque (`None`).
    pub painted: Option<Vec<u8>>,
    /// If `Some`, this bitmap was created via `CreateDIBSection` and
    /// the guest can write its pixels directly into the mapped guest
    /// VA at `dib_bits_va`. Source-side `BitBlt`s and `GetObjectW`
    /// pull from this address. `dib_bpp` records the original bit
    /// depth so we can decode 8-bpp palette formats etc.
    pub dib_bits_va: Option<u32>,
    /// DIB palette table, in RGB565. Empty for non-paletted DIBs.
    pub dib_palette: Vec<u16>,
    /// Whether the original DIB is bottom-up (Windows default) or
    /// top-down. Bottom-up DIBs need to be flipped on read.
    pub dib_bottom_up: bool,
    /// Stride in bytes of one row of the **DIB** layout (already
    /// padded to a 4-byte boundary). 0 for non-DIB bitmaps.
    pub dib_row_stride: u32,
    /// `true` when a 16-bpp DIB stores pixels as RGB555 rather than
    /// RGB565. Win32 defines `biBitCount = 16` with `BI_RGB` as
    /// 5-5-5 with the top bit unused; 5-6-5 only applies when the
    /// header says `BI_BITFIELDS` and supplies 565 masks. Our
    /// internal surfaces are always RGB565, so the DIB sync paths
    /// re-pack every pixel when this is set.
    pub dib_rgb555: bool,
    /// `true` if [`Bitmap::pixels`] has been modified host-side
    /// since the last sync to the guest's DIB pixel buffer at
    /// [`Bitmap::dib_bits_va`]. The DIB sync helper in
    /// `pocket-winceapi` short-circuits when this is `false`,
    /// since the guest already sees the current pixels — Derby
    /// chains many BitBlts that hit the same memory DC without
    /// the guest ever reading `ppvBits` between them, and the
    /// per-pixel RGB565 -> dib_bpp encode used to dominate the
    /// per-frame budget.
    pub host_dirty: bool,
}

impl Bitmap {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            bpp: 16,
            pixels: vec![0u8; (width.max(1) * height.max(1) * 2) as usize],
            painted: None,
            dib_bits_va: None,
            dib_palette: Vec::new(),
            dib_bottom_up: false,
            dib_row_stride: 0,
            dib_rgb555: false,
            host_dirty: false,
        }
    }

    /// Construct a DIB-backed bitmap. The host-side [`Bitmap::pixels`]
    /// buffer is allocated empty; callers (i.e. `BitBlt` source path)
    /// are expected to refresh it from the guest pixel store via
    /// [`Bitmap::sync_from_dib`] before reading.
    #[allow(clippy::too_many_arguments)]
    pub fn new_dib(
        width: u32,
        height: u32,
        bpp: u16,
        bits_va: u32,
        row_stride: u32,
        bottom_up: bool,
        palette: Vec<u16>,
        rgb555: bool,
    ) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            bpp,
            pixels: vec![0u8; (width.max(1) * height.max(1) * 2) as usize],
            painted: None,
            dib_bits_va: Some(bits_va),
            dib_palette: palette,
            dib_bottom_up: bottom_up,
            dib_row_stride: row_stride,
            dib_rgb555: rgb555,
            host_dirty: false,
        }
    }
}

/// Widen an RGB555 pixel (`0RRRRRGGGGGBBBBB`) into RGB565. The extra
/// green bit is filled from the source's own high green bit so a
/// fully-saturated green stays fully saturated.
#[inline(always)]
pub fn rgb555_to_rgb565(p: u16) -> u16 {
    let r = (p >> 10) & 0x1f;
    let g = (p >> 5) & 0x1f;
    let b = p & 0x1f;
    (r << 11) | (g << 6) | ((g >> 4) << 5) | b
}

/// Narrow an RGB565 pixel down to RGB555, dropping the low green bit.
#[inline(always)]
pub fn rgb565_to_rgb555(p: u16) -> u16 {
    let r = (p >> 11) & 0x1f;
    let g = (p >> 6) & 0x1f;
    let b = p & 0x1f;
    (r << 10) | (g << 5) | b
}

#[derive(Debug, Clone)]
pub struct Dc {
    /// Application clipping region, copied by SelectClipRgn; None means unrestricted.
    pub clip_rects: Option<Vec<[i32; 4]>>,
    pub surface: DcSurface,
    pub selected_bitmap: Option<u32>,
    /// Handles currently selected into the DC. Kept alongside the cached
    /// colours below so `SelectObject` can hand the guest back a real
    /// handle to restore later, the way GDI does.
    pub selected_brush: u32,
    pub selected_pen: u32,
    pub selected_font: u32,
    pub selected_palette: u32,
    pub brush_color: u32,
    pub pen_color: u32,
    pub text_color: u32,
    pub text_align: u32,
    pub bk_color: u32,
    pub bk_transparent: bool,
    pub current_position: (i32, i32),
}

impl Default for Dc {
    fn default() -> Self {
        Self {
            clip_rects: None,
            surface: DcSurface::Memory,
            selected_bitmap: None,
            selected_brush: STOCK_WHITE_BRUSH,
            selected_pen: STOCK_BLACK_PEN,
            selected_font: STOCK_SYSTEM_FONT,
            selected_palette: STOCK_DEFAULT_PALETTE,
            brush_color: 0x00ff_ffff,
            pen_color: 0,
            text_color: 0,
            text_align: 0,
            bk_color: 0x00ff_ffff,
            bk_transparent: false,
            current_position: (0, 0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Brush {
    pub color: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Pen {
    pub color: u32,
    pub width: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Font {
    pub height: i32,
    /// Original CE LOGFONTW, including face name, weight, charset and quality.
    pub logfont: [u8; 92],
}
impl Font {
    pub fn new(height: i32) -> Self {
        let mut raw = [0; 92];
        raw[..4].copy_from_slice(&height.to_le_bytes());
        Self::from_logfont(raw)
    }
    pub fn from_logfont(logfont: [u8; 92]) -> Self {
        Self {
            height: i32::from_le_bytes(logfont[..4].try_into().unwrap()),
            logfont,
        }
    }
    pub fn field(&self, offset: usize) -> i32 {
        i32::from_le_bytes(self.logfont[offset..offset + 4].try_into().unwrap())
    }
    pub fn face_name(&self) -> String {
        let units: Vec<_> = self.logfont[28..]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|&u| u != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }
}

#[derive(Debug, Clone)]
pub enum GdiObject {
    Dc(Dc),
    Bitmap(Bitmap),
    Brush(Brush),
    Pen(Pen),
    Font(Font),
    Palette(Vec<[u8; 4]>),
}

#[derive(Debug, Clone)]
pub struct OwnerDrawFrame {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub scratch: u32,
}

#[derive(Debug, Clone)]
pub struct ControlColorFrame {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub hwnd: u32,
    pub dc: u32,
    pub dialog: bool,
}

#[derive(Debug, Clone)]
pub struct ListPaintFrame {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub hwnd: u32,
    pub dc: u32,
    pub scratch: u32,
    pub stage: u32,
    pub item: usize,
    pub flags: u32,
    pub item_flags: u32,
}

#[derive(Debug, Clone)]
pub struct PaintEraseFrame {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
}

/// A streamed IMGDECMP call suspended in a guest GETDATAFUNC callback.
#[derive(Debug)]
pub struct ImageDecodeFrame {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub info: [u32; 15],
    pub bytes: Vec<u8>,
    pub completion: Option<(u32, u32)>,
}

#[derive(Debug)]
pub struct ImageRenderer {
    pub bitmap: u32,
    pub refs: u32,
    pub owns_bitmap: bool,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Default)]
pub struct GdiState {
    pub image_decode_frames: Vec<ImageDecodeFrame>,
    pub image_renderers: HashMap<u32, ImageRenderer>,
    saved_dcs: HashMap<u32, Vec<Dc>>,
    pub shell_menus: crate::shell_menu::ShellMenus,
    objects: HashMap<u32, GdiObject>,
    next_handle: u32,
    pub class_values: HashMap<(String, i32), u32>,
    pub regions: HashMap<u32, Vec<[i32; 4]>>,
    pub scroll_erases: HashMap<u32, Vec<[i32; 4]>>,
    pub icons: HashMap<u32, crate::imaging::Image>,
    pub image_lists: HashMap<u32, crate::imaging::ImageList>,
    pub system_brushes: HashMap<i32, u32>,
    pub dialog_bitmaps: HashMap<(u32, u32), u32>,
    pub owner_draw_frames: Vec<OwnerDrawFrame>,
    pub list_paint_frames: Vec<ListPaintFrame>,
    pub control_color_frames: Vec<ControlColorFrame>,
    pub dialog_backgrounds: HashMap<u32, u32>,
    pub list_paint_stamps: HashMap<u32, u64>,
    pub paint_erase_frames: Vec<PaintEraseFrame>,
    pub pending_window_erases: std::collections::HashSet<u32>,
    /// Windows with a generated (rather than explicitly posted) paint in the queue.
    pub generated_window_paints: std::collections::HashSet<u32>,
    pub owner_draw_states: HashMap<u32, (bool, bool, bool, i32, i32)>,
}

impl GdiState {
    pub fn new() -> Self {
        let mut s = Self {
            image_decode_frames: Vec::new(),
            image_renderers: HashMap::new(),
            saved_dcs: HashMap::new(),
            shell_menus: Default::default(),
            objects: HashMap::new(),
            next_handle: GDI_HANDLE_BASE,
            class_values: HashMap::new(),
            regions: HashMap::new(),
            scroll_erases: HashMap::new(),
            icons: HashMap::new(),
            image_lists: HashMap::new(),
            system_brushes: HashMap::new(),
            dialog_bitmaps: HashMap::new(),
            owner_draw_frames: Vec::new(),
            list_paint_frames: Vec::new(),
            control_color_frames: Vec::new(),
            dialog_backgrounds: HashMap::new(),
            list_paint_stamps: HashMap::new(),
            paint_erase_frames: Vec::new(),
            pending_window_erases: std::collections::HashSet::new(),
            generated_window_paints: std::collections::HashSet::new(),
            owner_draw_states: HashMap::new(),
        };
        // Pre-register stock objects so SelectObject(GetStockObject(...))
        // resolves through the same code path as user-created handles.
        s.objects.insert(
            STOCK_WHITE_BRUSH,
            GdiObject::Brush(Brush { color: 0x00ff_ffff }),
        );
        s.objects
            .insert(STOCK_BLACK_BRUSH, GdiObject::Brush(Brush { color: 0 }));
        s.objects.insert(
            STOCK_NULL_BRUSH,
            GdiObject::Brush(Brush { color: 0xff00_0000 }),
        );
        s.objects
            .insert(STOCK_BLACK_PEN, GdiObject::Pen(Pen { color: 0, width: 1 }));
        s.objects.insert(
            STOCK_WHITE_PEN,
            GdiObject::Pen(Pen {
                color: 0x00ff_ffff,
                width: 1,
            }),
        );
        s.objects.insert(
            STOCK_NULL_PEN,
            GdiObject::Pen(Pen {
                color: 0xff00_0000,
                width: 0,
            }),
        );
        s.objects.insert(
            STOCK_LTGRAY_BRUSH,
            GdiObject::Brush(Brush { color: 0x00c0_c0c0 }),
        );
        s.objects.insert(
            STOCK_GRAY_BRUSH,
            GdiObject::Brush(Brush { color: 0x0080_8080 }),
        );
        s.objects.insert(
            STOCK_DKGRAY_BRUSH,
            GdiObject::Brush(Brush { color: 0x0040_4040 }),
        );
        s.objects
            .insert(STOCK_SYSTEM_FONT, GdiObject::Font(Font::new(12)));
        // Every memory DC starts with a 1x1 monochrome bitmap selected.
        // We never draw through it; it exists so the first
        // SelectObject(memdc, hbm) can return a non-NULL "previous".
        s.objects
            .insert(STOCK_DEFAULT_BITMAP, GdiObject::Bitmap(Bitmap::new(1, 1)));
        // Screen DC is also pre-registered so that get_screen_dc()
        // returns a stable handle whose surface is `Screen`.
        s.objects.insert(
            GDI_SCREEN_DC,
            GdiObject::Dc(Dc {
                surface: DcSurface::Screen,
                ..Default::default()
            }),
        );
        let colors = [
            0x000000u32,
            0x800000,
            0x008000,
            0x808000,
            0x000080,
            0x800080,
            0x008080,
            0xc0c0c0,
            0xc0dcc0,
            0xa6caf0,
            0xfffbf0,
            0xa0a0a4,
            0x808080,
            0xff0000,
            0x00ff00,
            0xffff00,
            0x0000ff,
            0xff00ff,
            0x00ffff,
            0xffffff,
        ];
        s.objects.insert(
            STOCK_DEFAULT_PALETTE,
            GdiObject::Palette(
                colors
                    .into_iter()
                    .map(|c| [(c >> 16) as u8, (c >> 8) as u8, c as u8, 0])
                    .collect(),
            ),
        );
        s
    }

    pub fn create_palette(&mut self, entries: Vec<[u8; 4]>) -> u32 {
        let handle = self.alloc_handle();
        self.objects.insert(handle, GdiObject::Palette(entries));
        handle
    }
    pub fn palette(&self, handle: u32) -> Option<&[[u8; 4]]> {
        match self.objects.get(&handle)? {
            GdiObject::Palette(entries) => Some(entries),
            _ => None,
        }
    }

    fn alloc_handle(&mut self) -> u32 {
        loop {
            let h = self.next_handle;
            self.next_handle = self.next_handle.wrapping_add(1);
            // The dynamic sequence eventually reaches the reserved stock
            // range. Released handles may be reused after wraparound, live
            // objects and icons must never be overwritten.
            if h != 0
                && !is_stock_handle(h)
                && !self.objects.contains_key(&h)
                && !self.icons.contains_key(&h)
            {
                return h;
            }
        }
    }

    pub fn register_icon(&mut self, image: crate::imaging::Image) -> u32 {
        let handle = self.alloc_handle();
        self.icons.insert(handle, image);
        handle
    }
    pub fn create_image_list(&mut self, width: u32, height: u32) -> u32 {
        let Some(list) = crate::imaging::ImageList::new(width, height) else {
            return 0;
        };
        let handle = self.alloc_handle();
        self.image_lists.insert(handle, list);
        handle
    }

    pub fn create_region(&mut self, rects: Vec<[i32; 4]>) -> u32 {
        let handle = self.alloc_handle();
        self.regions.insert(handle, rects);
        handle
    }

    pub fn create_memory_dc(&mut self) -> u32 {
        let h = self.alloc_handle();
        self.objects.insert(h, GdiObject::Dc(Dc::default()));
        h
    }

    pub fn create_compatible_bitmap(&mut self, width: u32, height: u32) -> u32 {
        let h = self.alloc_handle();
        self.objects
            .insert(h, GdiObject::Bitmap(Bitmap::new(width, height)));
        h
    }

    /// Register a DIB-backed bitmap. The pixel storage lives in guest
    /// memory at `bits_va`; we keep [`Bitmap`] metadata (palette,
    /// width, etc.) host-side so `BitBlt` can render through it.
    pub fn register_dib(&mut self, bitmap: Bitmap) -> u32 {
        let h = self.alloc_handle();
        self.objects.insert(h, GdiObject::Bitmap(bitmap));
        h
    }

    pub fn create_solid_brush(&mut self, color: u32) -> u32 {
        let h = self.alloc_handle();
        self.objects.insert(h, GdiObject::Brush(Brush { color }));
        h
    }

    pub fn create_pen(&mut self, color: u32, width: u32) -> u32 {
        let h = self.alloc_handle();
        self.objects.insert(h, GdiObject::Pen(Pen { color, width }));
        h
    }

    /// Fonts owned by the HLE dialog manager are shared across identical
    /// templates. Guest CreateFontIndirectW still creates independent objects.
    pub fn create_dialog_font(&mut self, font: Font) -> u32 {
        if let Some((&handle, _)) = self
            .objects
            .iter()
            .find(|(_, object)| matches!(object, GdiObject::Font(f) if f.logfont == font.logfont))
        {
            return handle;
        }
        self.create_logfont(font)
    }

    pub fn create_logfont(&mut self, font: Font) -> u32 {
        let h = self.alloc_handle();
        self.objects.insert(h, GdiObject::Font(font));
        h
    }

    pub fn create_font(&mut self, height: i32) -> u32 {
        let h = self.alloc_handle();
        self.objects.insert(h, GdiObject::Font(Font::new(height)));
        h
    }

    pub fn delete(&mut self, handle: u32) -> bool {
        // Stock objects are immortal.
        if is_stock_handle(handle) || self.system_brushes.values().any(|h| *h == handle) {
            return true;
        }
        // Selected GDI objects remain owned by their DC. In particular the
        // DIB's guest pixel allocation must remain live after a failed delete.
        if self.objects.values().any(|object| {
            matches!(object, GdiObject::Dc(dc)
            if dc.selected_bitmap == Some(handle)
                || dc.selected_palette == handle || dc.selected_brush == handle
                || dc.selected_pen == handle || dc.selected_font == handle)
        }) {
            return false;
        }
        self.saved_dcs.remove(&handle);
        self.regions.remove(&handle).is_some() || self.objects.remove(&handle).is_some()
    }

    pub fn get(&self, handle: u32) -> Option<&GdiObject> {
        self.objects.get(&handle)
    }

    pub fn get_mut(&mut self, handle: u32) -> Option<&mut GdiObject> {
        self.objects.get_mut(&handle)
    }

    /// SaveDC levels belong to one DC and capture its complete modeled state.
    pub fn save_dc(&mut self, handle: u32) -> Option<u32> {
        let snapshot = self.dc(handle)?.clone();
        let stack = self.saved_dcs.entry(handle).or_default();
        if stack.len() >= 4096 {
            return None;
        }
        stack.push(snapshot);
        Some(stack.len() as u32)
    }

    /// Positive levels are absolute; negative levels count from the newest.
    /// Restoring consumes the selected snapshot and all snapshots after it.
    pub fn restore_dc(&mut self, handle: u32, level: i32) -> bool {
        if level == 0 || self.dc(handle).is_none() {
            return false;
        }
        let Some(stack) = self.saved_dcs.get_mut(&handle) else {
            return false;
        };
        let index = if level < 0 {
            stack.len() as i64 + i64::from(level)
        } else {
            i64::from(level) - 1
        };
        if index < 0 || index as usize >= stack.len() {
            return false;
        }
        let snapshot = stack[index as usize].clone();
        stack.truncate(index as usize);
        *self.dc_mut(handle).unwrap() = snapshot;
        true
    }

    pub fn dc(&self, handle: u32) -> Option<&Dc> {
        match self.objects.get(&handle)? {
            GdiObject::Dc(d) => Some(d),
            _ => None,
        }
    }

    pub fn dc_mut(&mut self, handle: u32) -> Option<&mut Dc> {
        match self.objects.get_mut(&handle)? {
            GdiObject::Dc(d) => Some(d),
            _ => None,
        }
    }

    pub fn bitmap(&self, handle: u32) -> Option<&Bitmap> {
        match self.objects.get(&handle)? {
            GdiObject::Bitmap(b) => Some(b),
            _ => None,
        }
    }

    pub fn bitmap_mut(&mut self, handle: u32) -> Option<&mut Bitmap> {
        match self.objects.get_mut(&handle)? {
            GdiObject::Bitmap(b) => Some(b),
            _ => None,
        }
    }

    pub fn brush(&self, handle: u32) -> Option<&Brush> {
        match self.objects.get(&handle)? {
            GdiObject::Brush(b) => Some(b),
            _ => None,
        }
    }

    pub fn pen(&self, handle: u32) -> Option<&Pen> {
        match self.objects.get(&handle)? {
            GdiObject::Pen(p) => Some(p),
            _ => None,
        }
    }

    /// SelectObject semantics: tie `obj_handle` into `dc_handle`.
    /// Returns the previous handle of the same kind (for callers that
    /// want to restore it later), or 0 if no such object existed.
    pub fn select_into(&mut self, dc_handle: u32, obj_handle: u32) -> u32 {
        // Read the object's kind without holding a borrow on self.
        let kind = match self.objects.get(&obj_handle) {
            Some(GdiObject::Bitmap(_)) => "bitmap",
            Some(GdiObject::Brush(_)) => "brush",
            Some(GdiObject::Pen(_)) => "pen",
            Some(GdiObject::Font(_)) => "font",
            _ => return 0,
        };
        // For brushes and pens, store the colour straight onto the DC
        // so primitive draws don't need a second lookup.
        let color = match self.objects.get(&obj_handle) {
            Some(GdiObject::Brush(b)) => Some(b.color),
            Some(GdiObject::Pen(p)) => Some(p.color),
            _ => None,
        };
        let dc = match self.dc_mut(dc_handle) {
            Some(d) => d,
            None => return 0,
        };
        match kind {
            "bitmap" => {
                // A fresh memory DC has the 1x1 stock bitmap selected in real
                // GDI, so the first SelectObject must not return NULL: guests
                // treat NULL as failure and skip the blit entirely.
                let prev = dc.selected_bitmap.unwrap_or(STOCK_DEFAULT_BITMAP);
                dc.selected_bitmap = Some(obj_handle);
                prev
            }
            "brush" => {
                let prev = dc.selected_brush;
                dc.selected_brush = obj_handle;
                if let Some(c) = color {
                    dc.brush_color = c;
                }
                prev
            }
            "pen" => {
                let prev = dc.selected_pen;
                dc.selected_pen = obj_handle;
                if let Some(c) = color {
                    dc.pen_color = c;
                }
                prev
            }
            "font" => {
                let prev = dc.selected_font;
                dc.selected_font = obj_handle;
                prev
            }
            _ => 0,
        }
    }
}

/// Borrow either the framebuffer or a memory bitmap as a writable
/// surface.
pub enum Surface<'a> {
    Screen(&'a mut Framebuffer),
    Bitmap(&'a mut Bitmap),
}

impl<'a> Surface<'a> {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Surface::Screen(fb) => (fb.width, fb.height),
            Surface::Bitmap(bm) => (bm.width, bm.height),
        }
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        match self {
            Surface::Screen(fb) => &mut fb.pixels,
            Surface::Bitmap(bm) => &mut bm.pixels,
        }
    }

    pub fn pixels(&self) -> &[u8] {
        match self {
            Surface::Screen(fb) => &fb.pixels,
            Surface::Bitmap(bm) => &bm.pixels,
        }
    }

    pub fn mark_dirty(&mut self) {
        match self {
            Surface::Screen(fb) => fb.mark_dirty(),
            // Memory DCs back DIB sections — flag them as needing
            // a host -> guest pixel resync before the next time the
            // guest is allowed to observe the DIB through `ppvBits`.
            Surface::Bitmap(bm) => bm.host_dirty = true,
        }
    }

    pub fn put_pixel(&mut self, x: i32, y: i32, color: u16) {
        let (sw, sh) = self.dimensions();
        if x < 0 || y < 0 || (x as u32) >= sw || (y as u32) >= sh {
            return;
        }
        let off = (y as u32 * sw + x as u32) as usize * 2;
        let bytes = color.to_le_bytes();
        let pix = self.pixels_mut();
        pix[off] = bytes[0];
        pix[off + 1] = bytes[1];
        self.cover_rect(x as u32, y as u32, x as u32 + 1, y as u32 + 1);
    }

    fn cover_rect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        if let Surface::Bitmap(bitmap) = self {
            if let Some(painted) = &mut bitmap.painted {
                for y in y0..y1 {
                    let start = (y * bitmap.width + x0) as usize;
                    painted[start..start + (x1 - x0) as usize].fill(1);
                }
            }
        }
    }

    /// Present a retained window, preserving holes left by keyed drawing.
    pub fn blit_window(&mut self, x: i32, y: i32, bitmap: &Bitmap) {
        self.blit_window_clipped(x, y, bitmap, bitmap.width, bitmap.height);
    }

    /// A backing bitmap can outlive a resize; never present it outside its window.
    pub fn blit_window_clipped(
        &mut self,
        x: i32,
        y: i32,
        bitmap: &Bitmap,
        width: u32,
        height: u32,
    ) {
        let width = width.min(bitmap.width);
        let height = height.min(bitmap.height);
        let Some(painted) = bitmap.painted.as_ref().filter(|p| p.contains(&0)) else {
            self.blit_from_bytes(
                x,
                y,
                0,
                0,
                width as i32,
                height as i32,
                &bitmap.pixels,
                bitmap.width,
                bitmap.height,
            );
            return;
        };
        for row in 0..height {
            let mut col = 0;
            while col < width {
                if painted[(row * bitmap.width + col) as usize] == 0 {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < width && painted[(row * bitmap.width + col) as usize] != 0 {
                    col += 1;
                }
                self.blit_from_bytes(
                    x + start as i32,
                    y + row as i32,
                    start as i32,
                    row as i32,
                    (col - start) as i32,
                    1,
                    &bitmap.pixels,
                    bitmap.width,
                    bitmap.height,
                );
            }
        }
    }

    /// Clip before rasterization: native map coordinates can be far outside
    /// the viewport. LineTo excludes its terminal point, as Windows GDI does.
    pub fn line(&mut self, from: (i32, i32), to: (i32, i32), color: u16, width: u32) {
        let (w, h) = self.dimensions();
        if w == 0 || h == 0 {
            return;
        }
        let width = width.clamp(1, 2048) as i32;
        let (x, y) = (from.0 as f64, from.1 as f64);
        let (dx, dy) = (to.0 as f64 - x, to.1 as f64 - y);
        let mut a = 0.0f64;
        let mut b = 1.0f64;
        for (p, q) in [
            (-dx, x + width as f64),
            (dx, w as f64 + width as f64 - x),
            (-dy, y + width as f64),
            (dy, h as f64 + width as f64 - y),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    a = a.max(t);
                } else {
                    b = b.min(t);
                }
                if a > b {
                    return;
                }
            }
        }
        let (mut x0, mut y0) = ((x + a * dx).round() as i32, (y + a * dy).round() as i32);
        let (x1, y1) = ((x + b * dx).round() as i32, (y + b * dy).round() as i32);
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let mut error = dx + dy;
        loop {
            if (x0, y0) == to {
                break;
            }
            self.fill_rect(x0 - width / 2, y0 - width / 2, width, width, color);
            if (x0, y0) == (x1, y1) {
                break;
            }
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x0 += sx;
            }
            if twice <= dx {
                error += dx;
                y0 += sy;
            }
        }
    }

    /// Alternate-fill polygon, with scanlines clipped to the target bitmap.
    pub fn polygon(&mut self, points: &[(i32, i32)], color: u16) {
        if points.len() < 3 {
            return;
        }
        let (width, height) = self.dimensions();
        let top = points.iter().map(|p| p.1).min().unwrap().max(0);
        let bottom = points.iter().map(|p| p.1).max().unwrap().min(height as i32);
        let mut intersections = Vec::new();
        for y in top..bottom {
            intersections.clear();
            let scan = y as f64 + 0.5;
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                if (a.1 as f64 <= scan && b.1 as f64 > scan)
                    || (b.1 as f64 <= scan && a.1 as f64 > scan)
                {
                    intersections.push(
                        a.0 as f64
                            + (scan - a.1 as f64) * (b.0 as f64 - a.0 as f64)
                                / (b.1 as f64 - a.1 as f64),
                    );
                }
            }
            intersections.sort_by(f64::total_cmp);
            for pair in intersections.chunks_exact(2) {
                let l = (pair[0] - 0.5).ceil().clamp(0.0, width as f64) as i32;
                let r = (pair[1] - 0.5).ceil().clamp(0.0, width as f64) as i32;
                self.fill_rect(l, y, r - l, 1, color);
            }
        }
    }

    /// Rasterize an ellipse inside an exclusive bounding rectangle. Clip
    /// before iterating so far-off native map coordinates remain bounded.
    pub fn ellipse(&mut self, rect: [i32; 4], fill: Option<u16>, stroke: Option<(u16, u32)>) {
        let [left, top, right, bottom] = rect.map(i64::from);
        if right <= left || bottom <= top || (fill.is_none() && stroke.is_none()) {
            return;
        }
        let (width, height) = self.dimensions();
        let (rx, ry) = ((right - left) as f64 / 2.0, (bottom - top) as f64 / 2.0);
        let (cx, cy) = ((left + right) as f64 / 2.0, (top + bottom) as f64 / 2.0);
        let pen_width = stroke.map_or(0, |(_, w)| w.clamp(1, 2048)) as f64;
        let (inner_x, inner_y) = (rx - pen_width, ry - pen_width);
        let mut changed = false;
        for y in top.max(0)..bottom.min(height as i64) {
            for x in left.max(0)..right.min(width as i64) {
                let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                if (dx / rx).powi(2) + (dy / ry).powi(2) > 1.0 {
                    continue;
                }
                let inside = inner_x > 0.0
                    && inner_y > 0.0
                    && (dx / inner_x).powi(2) + (dy / inner_y).powi(2) < 1.0;
                let color = if !inside {
                    stroke.map(|(c, _)| c).or(fill)
                } else {
                    fill
                };
                if let Some(color) = color {
                    self.put_pixel(x as i32, y as i32, color);
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_dirty();
        }
    }

    /// Rounded corners use the supplied ellipse diameters, limited to the
    /// bounding box. Iterate only visible pixels, even for huge guest bounds.
    pub fn round_rect(
        &mut self,
        rect: [i32; 4],
        corner: [i32; 2],
        fill: Option<u16>,
        stroke: Option<(u16, u32)>,
    ) {
        let [l, t, r, b] = rect.map(i64::from);
        if r <= l || b <= t || (fill.is_none() && stroke.is_none()) {
            return;
        }
        let rx = (i64::from(corner[0]).unsigned_abs().min((r - l) as u64)) as f64 / 2.0;
        let ry = (i64::from(corner[1]).unsigned_abs().min((b - t) as u64)) as f64 / 2.0;
        let inside = |x: f64, y: f64, inset: f64| {
            let (left, top, right, bottom) = (
                l as f64 + inset,
                t as f64 + inset,
                r as f64 - inset,
                b as f64 - inset,
            );
            if x < left || y < top || x >= right || y >= bottom {
                return false;
            }
            let (ax, ay) = ((rx - inset).max(0.0), (ry - inset).max(0.0));
            if ax == 0.0 || ay == 0.0 {
                return true;
            }
            let dx = if x < left + ax {
                (left + ax - x) / ax
            } else if x > right - ax {
                (x - right + ax) / ax
            } else {
                0.0
            };
            let dy = if y < top + ay {
                (top + ay - y) / ay
            } else if y > bottom - ay {
                (y - bottom + ay) / ay
            } else {
                0.0
            };
            dx * dx + dy * dy <= 1.0
        };
        let (sw, sh) = self.dimensions();
        let pen = stroke.map_or(0.0, |(_, w)| w.clamp(1, 2048) as f64);
        let mut changed = false;
        for y in t.max(0)..b.min(sh as i64) {
            for x in l.max(0)..r.min(sw as i64) {
                if !inside(x as f64 + 0.5, y as f64 + 0.5, 0.0) {
                    continue;
                }
                let color = if !inside(x as f64 + 0.5, y as f64 + 0.5, pen) {
                    stroke.map(|(c, _)| c).or(fill)
                } else {
                    fill
                };
                if let Some(color) = color {
                    self.put_pixel(x as i32, y as i32, color);
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_dirty();
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u16) {
        let (sw, sh) = self.dimensions();
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.max(0).min(sw as i32) as u32;
        let y0 = y.max(0).min(sh as i32) as u32;
        let x1 = (x + w).max(0).min(sw as i32) as u32;
        let y1 = (y + h).max(0).min(sh as i32) as u32;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        // Reinterpret the surface as `[u16]` so each row collapses
        // into one `slice::fill`. The naive per-byte loop became a
        // hot spot in profiles of games that issue many `FillRect`
        // calls per frame to clear the background.
        let pix = self.pixels_mut();
        let words: &mut [u16] = bytemuck::cast_slice_mut(pix);
        let stride = sw as usize;
        let row_len = (x1 - x0) as usize;
        let color_le = color.to_le();
        for row in y0..y1 {
            let off = row as usize * stride + x0 as usize;
            words[off..off + row_len].fill(color_le);
        }
        self.cover_rect(x0, y0, x1, y1);
        self.mark_dirty();
    }

    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u16) {
        if w <= 0 || h <= 0 {
            return;
        }
        let put = |this: &mut Self, px: i32, py: i32| {
            this.put_pixel(px, py, color);
        };
        for i in 0..w {
            put(self, x + i, y);
            put(self, x + i, y + h - 1);
        }
        for i in 0..h {
            put(self, x, y + i);
            put(self, x + w - 1, y + i);
        }
        self.mark_dirty();
    }

    /// Copy a rectangle from `src` into `(dx, dy)` of this surface.
    ///
    /// Equivalent to [`Self::blit_from_bytes_rop`] with `SRCCOPY`.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_from_bytes(
        &mut self,
        dx: i32,
        dy: i32,
        sx: i32,
        sy: i32,
        w: i32,
        h: i32,
        src: &[u8],
        src_w: u32,
        src_h: u32,
    ) {
        self.blit_from_bytes_rop(dx, dy, sx, sy, w, h, src, src_w, src_h, rop3::SRCCOPY, 0);
    }

    /// Combine a rectangle from `src` into `(dx, dy)` of this surface
    /// under the ternary raster operation `rop`.
    ///
    /// `pat` supplies the pattern (brush) operand for the ROP codes that
    /// reference one; pass 0 when there is no selected brush. Both the
    /// source and this surface are RGB565, and the RGB565 channels are
    /// contiguous bit fields, so evaluating the ROP bitwise on the packed
    /// `u16` matches what real GDI does on a 16 bpp device.
    ///
    /// The masked-sprite idiom that PegCards (and most Pocket PC card
    /// games) use depends on this: an `SRCAND` pass punches the sprite
    /// silhouette into the destination, then an `SRCPAINT`/`SRCCOPY` pass
    /// lays the colour art into the hole. Executing the mask pass as a
    /// plain copy paints the mask's own black-and-white pattern on screen
    /// instead of the artwork.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_from_bytes_rop(
        &mut self,
        dx: i32,
        dy: i32,
        sx: i32,
        sy: i32,
        w: i32,
        h: i32,
        src: &[u8],
        src_w: u32,
        src_h: u32,
        rop: u32,
        pat: u16,
    ) {
        if w <= 0 || h <= 0 || src_w == 0 || src_h == 0 {
            return;
        }
        // Clip source to its own bounds first.
        let sx0 = sx.max(0).min(src_w as i32) as u32;
        let sy0 = sy.max(0).min(src_h as i32) as u32;
        let sx1 = (sx + w).max(0).min(src_w as i32) as u32;
        let sy1 = (sy + h).max(0).min(src_h as i32) as u32;
        if sx0 >= sx1 || sy0 >= sy1 {
            return;
        }
        let dest_x0 = dx + (sx0 as i32 - sx);
        let dest_y0 = dy + (sy0 as i32 - sy);
        let copy_w = (sx1 - sx0) as i32;
        let copy_h = (sy1 - sy0) as i32;

        let (dw, dh) = self.dimensions();
        let dx0 = dest_x0.max(0).min(dw as i32) as u32;
        let dy0 = dest_y0.max(0).min(dh as i32) as u32;
        let dx1 = (dest_x0 + copy_w).max(0).min(dw as i32) as u32;
        let dy1 = (dest_y0 + copy_h).max(0).min(dh as i32) as u32;
        if dx0 >= dx1 || dy0 >= dy1 {
            return;
        }
        let dst_stride = dw as usize * 2;
        let src_stride = src_w as usize * 2;
        let row_bytes = (dx1 - dx0) as usize * 2;
        let src_skip_x = dx0 as i32 - dest_x0;
        let src_skip_y = dy0 as i32 - dest_y0;
        let src_x0 = sx0 as i32 + src_skip_x;
        let src_y0 = sy0 as i32 + src_skip_y;
        let pix = self.pixels_mut();
        for row in 0..(dy1 - dy0) as i32 {
            let dst_off = (dy0 as i32 + row) as usize * dst_stride + dx0 as usize * 2;
            let src_off = (src_y0 + row) as usize * src_stride + src_x0 as usize * 2;
            if src_off + row_bytes > src.len() || dst_off + row_bytes > pix.len() {
                continue;
            }
            if rop == rop3::SRCCOPY {
                // Whole-row memcpy — this is the overwhelmingly common
                // case and worth keeping off the per-pixel path.
                pix[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src[src_off..src_off + row_bytes]);
                continue;
            }
            for col in (0..row_bytes).step_by(2) {
                let s = u16::from_le_bytes([src[src_off + col], src[src_off + col + 1]]);
                let d = u16::from_le_bytes([pix[dst_off + col], pix[dst_off + col + 1]]);
                let out = rop3::apply(rop, pat, s, d).to_le_bytes();
                pix[dst_off + col] = out[0];
                pix[dst_off + col + 1] = out[1];
            }
        }
        self.cover_rect(dx0, dy0, dx1, dy1);
        self.mark_dirty();
    }

    /// Combine the brush pattern `pat` into a rectangle of this surface
    /// under `rop`, with no source operand. This is the `PatBlt` path,
    /// and also where `BitBlt` lands for ROP codes that ignore the source
    /// entirely (`BLACKNESS`, `WHITENESS`, `DSTINVERT`, `PATCOPY`, ...).
    pub fn fill_rect_rop(&mut self, x: i32, y: i32, w: i32, h: i32, pat: u16, rop: u32) {
        if rop == rop3::PATCOPY {
            self.fill_rect(x, y, w, h, pat);
            return;
        }
        let (sw, sh) = self.dimensions();
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.max(0).min(sw as i32) as u32;
        let y0 = y.max(0).min(sh as i32) as u32;
        let x1 = (x + w).max(0).min(sw as i32) as u32;
        let y1 = (y + h).max(0).min(sh as i32) as u32;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let stride = sw as usize;
        let pix = self.pixels_mut();
        let words: &mut [u16] = bytemuck::cast_slice_mut(pix);
        for row in y0..y1 {
            let off = row as usize * stride;
            for word in &mut words[off + x0 as usize..off + x1 as usize] {
                // The surface stores little-endian RGB565; normalise to
                // native order before the ROP and back afterwards so the
                // result is byte-order correct on any host.
                let d = u16::from_le(*word);
                *word = rop3::apply(rop, pat, 0, d).to_le();
            }
        }
        self.cover_rect(x0, y0, x1, y1);
        self.mark_dirty();
    }
}

/// Ternary raster operations (ROP3).
///
/// A ROP3 code packs, in its high byte, the truth table of a boolean
/// function of three inputs: the **pattern** (selected brush), the
/// **source** pixel and the **destination** pixel. The low three bytes
/// encode the operation as a stack program for real GDI's interpreter;
/// PocketHLE only needs the truth table, so it evaluates the high byte
/// directly and therefore supports all 256 codes rather than a
/// hand-written subset.
pub mod rop3 {
    pub const BLACKNESS: u32 = 0x0000_0042;
    pub const NOTSRCERASE: u32 = 0x0011_0000;
    pub const NOTSRCCOPY: u32 = 0x0033_0008;
    pub const SRCERASE: u32 = 0x0044_0328;
    pub const DSTINVERT: u32 = 0x0055_0009;
    pub const PATINVERT: u32 = 0x005A_0049;
    pub const SRCINVERT: u32 = 0x0066_0046;
    pub const SRCAND: u32 = 0x0088_00C6;
    pub const MERGEPAINT: u32 = 0x00BB_0226;
    pub const MERGECOPY: u32 = 0x00C0_00CA;
    pub const SRCCOPY: u32 = 0x00CC_0020;
    pub const SRCPAINT: u32 = 0x00EE_0086;
    pub const PATCOPY: u32 = 0x00F0_0021;
    pub const PATPAINT: u32 = 0x00FB_0A09;
    pub const WHITENESS: u32 = 0x00FF_0062;

    /// The truth-table byte of a ROP3 code.
    #[inline]
    pub fn table(rop: u32) -> u8 {
        ((rop >> 16) & 0xff) as u8
    }

    /// Whether `rop` actually reads the source operand. Codes like
    /// `BLACKNESS` and `PATCOPY` do not, and real GDI does not require a
    /// valid source rectangle for them.
    pub fn uses_src(rop: u32) -> bool {
        let t = table(rop);
        // Compare the S=0 and S=1 halves of the table for every (P, D).
        (0..8).any(|i| {
            let p = (i >> 2) & 1;
            let d = i & 1;
            let with_s0 = (t >> (p * 4 + d)) & 1;
            let with_s1 = (t >> (p * 4 + 2 + d)) & 1;
            with_s0 != with_s1
        })
    }

    /// Whether `rop` reads the destination operand.
    pub fn uses_dst(rop: u32) -> bool {
        let t = table(rop);
        (0..8).any(|i| {
            let p = (i >> 2) & 1;
            let s = (i >> 1) & 1;
            let with_d0 = (t >> (p * 4 + s * 2)) & 1;
            let with_d1 = (t >> (p * 4 + s * 2 + 1)) & 1;
            with_d0 != with_d1
        })
    }

    /// Evaluate `rop` bitwise across `pat`, `src` and `dst`.
    ///
    /// Bit `n` of the truth table gives the output for the input
    /// combination `n = P*4 + S*2 + D`, so each of the eight minterms
    /// contributes `mask(P) & mask(S) & mask(D)` when its table bit is
    /// set. Every bit position of the three operands is evaluated in
    /// parallel, which is what makes this a single pass over 16 bits
    /// rather than sixteen.
    #[inline]
    pub fn apply(rop: u32, pat: u16, src: u16, dst: u16) -> u16 {
        match rop {
            // Fast paths for the codes that dominate real workloads.
            SRCCOPY => src,
            SRCAND => dst & src,
            SRCPAINT => dst | src,
            SRCINVERT => dst ^ src,
            NOTSRCCOPY => !src,
            PATCOPY => pat,
            BLACKNESS => 0,
            WHITENESS => u16::MAX,
            DSTINVERT => !dst,
            _ => {
                let t = table(rop);
                let mut out = 0u16;
                for i in 0..8u32 {
                    if (t >> i) & 1 == 0 {
                        continue;
                    }
                    let pm = if i & 4 != 0 { pat } else { !pat };
                    let sm = if i & 2 != 0 { src } else { !src };
                    let dm = if i & 1 != 0 { dst } else { !dst };
                    out |= pm & sm & dm;
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_handles_skip_stock_objects_and_live_handles_after_wrap() {
        let mut gdi = GdiState::new();
        gdi.next_handle = STOCK_WHITE_BRUSH - 1;
        for _ in 0..64 {
            let h = gdi.create_solid_brush(0x123456);
            assert!(!is_stock_handle(h));
        }
        assert_eq!(gdi.brush(STOCK_WHITE_BRUSH).unwrap().color, 0xffffff);
        assert_eq!(gdi.brush(STOCK_BLACK_BRUSH).unwrap().color, 0);
        assert!(matches!(
            gdi.objects.get(&STOCK_SYSTEM_FONT),
            Some(GdiObject::Font(_))
        ));
        assert!(gdi.bitmap(STOCK_DEFAULT_BITMAP).is_some());
        gdi.next_handle = u32::MAX;
        let high = gdi.create_solid_brush(0x112233);
        assert_eq!(high, u32::MAX);
        let low = gdi.create_solid_brush(0x445566);
        assert_ne!(low, 0);
        gdi.next_handle = high;
        let next = gdi.create_solid_brush(0x778899);
        assert_ne!(next, high);
        assert_ne!(next, low);
        assert_eq!(gdi.brush(high).unwrap().color, 0x112233);
        assert_eq!(gdi.brush(low).unwrap().color, 0x445566);
    }

    #[test]
    fn window_paint_tracks_primitive_coverage_and_clips_partial_layers() {
        let mut window = Bitmap::new(5, 5);
        window.painted = Some(vec![0; 25]);
        let mut surface = Surface::Bitmap(&mut window);
        surface.put_pixel(1, 1, 0);
        surface.fill_rect(2, 2, 1, 1, 0);
        surface.stroke_rect(3, 3, 1, 1, 0);
        surface.blit_from_bytes(4, 4, 0, 0, 1, 1, &[0, 0], 1, 1);
        surface.fill_rect_rop(0, 0, 1, 1, 0, rop3::BLACKNESS);
        let mut background = Bitmap::new(4, 4);
        let mut surface = Surface::Bitmap(&mut background);
        surface.fill_rect(0, 0, 4, 4, 0x07e0);
        surface.blit_window(-1, -1, &window);
        for y in 0..4usize {
            for x in 0..4usize {
                let at = (y * 4 + x) * 2;
                let color = u16::from_le_bytes(background.pixels[at..at + 2].try_into().unwrap());
                assert_eq!(color, if x == y { 0 } else { 0x07e0 });
            }
        }
        assert!(Bitmap::new(1, 1).painted.is_none());
    }

    /// Cross-check the generic 8-minterm evaluator against independent
    /// hand-written implementations of the named ROP codes. Every
    /// operand triple is exercised, so a wrong truth-table bit for any
    /// of these codes shows up here rather than as a rendering glitch.
    #[test]
    fn navigator_rendering_round_rect_preserves_corners_and_hollow_center() {
        let mut bitmap = Bitmap::new(16, 12);
        let mut surface = Surface::Bitmap(&mut bitmap);
        surface.fill_rect(0, 0, 16, 12, 0x1234);
        surface.round_rect([1, 1, 15, 11], [8, 8], Some(0x07e0), Some((0xffff, 1)));
        let pixel = |s: &Surface<'_>, x: usize, y: usize| {
            u16::from_le_bytes(
                s.pixels()[(y * 16 + x) * 2..(y * 16 + x) * 2 + 2]
                    .try_into()
                    .unwrap(),
            )
        };
        assert_eq!(pixel(&surface, 1, 1), 0x1234);
        assert_eq!(pixel(&surface, 8, 1), 0xffff);
        assert_eq!(pixel(&surface, 8, 6), 0x07e0);
        surface.round_rect([1, 1, 15, 11], [8, 8], None, Some((0xf800, 2)));
        assert_eq!(pixel(&surface, 8, 6), 0x07e0);
        assert_eq!(pixel(&surface, 8, 2), 0xf800);
        surface.round_rect(
            [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
            [0, 0],
            Some(0xabcd),
            None,
        );
        assert_eq!(pixel(&surface, 0, 0), 0xabcd);
        assert_eq!(pixel(&surface, 15, 11), 0xabcd);
    }

    #[test]
    fn rop3_matches_reference_formulas() {
        type Ref = (u32, &'static str, fn(u16, u16, u16) -> u16);
        // (code, name, |pat, src, dst| expected)
        let refs: &[Ref] = &[
            (rop3::SRCCOPY, "SRCCOPY", |_p, s, _d| s),
            (rop3::SRCAND, "SRCAND", |_p, s, d| s & d),
            (rop3::SRCPAINT, "SRCPAINT", |_p, s, d| s | d),
            (rop3::SRCINVERT, "SRCINVERT", |_p, s, d| s ^ d),
            (rop3::NOTSRCCOPY, "NOTSRCCOPY", |_p, s, _d| !s),
            (rop3::NOTSRCERASE, "NOTSRCERASE", |_p, s, d| !s & !d),
            (rop3::SRCERASE, "SRCERASE", |_p, s, d| s & !d),
            (rop3::DSTINVERT, "DSTINVERT", |_p, _s, d| !d),
            (rop3::PATINVERT, "PATINVERT", |p, _s, d| p ^ d),
            (rop3::MERGEPAINT, "MERGEPAINT", |_p, s, d| !s | d),
            (rop3::MERGECOPY, "MERGECOPY", |p, s, _d| p & s),
            (rop3::PATCOPY, "PATCOPY", |p, _s, _d| p),
            (rop3::PATPAINT, "PATPAINT", |p, s, d| (p | !s) | d),
            (rop3::BLACKNESS, "BLACKNESS", |_p, _s, _d| 0),
            (rop3::WHITENESS, "WHITENESS", |_p, _s, _d| u16::MAX),
        ];
        // A spread that covers all-zero, all-one, and RGB565 fields
        // independently so a channel-crossing bug can't hide.
        const VALUES: [u16; 8] = [
            0x0000, 0xffff, 0xf800, 0x07e0, 0x001f, 0xa5a5, 0x5a5a, 0x1234,
        ];
        for &(code, name, expect) in refs {
            for &p in &VALUES {
                for &s in &VALUES {
                    for &d in &VALUES {
                        assert_eq!(
                            rop3::apply(code, p, s, d),
                            expect(p, s, d),
                            "{name} (0x{code:08x}) pat=0x{p:04x} src=0x{s:04x} dst=0x{d:04x}"
                        );
                    }
                }
            }
        }
    }

    /// The fast paths in [`rop3::apply`] must agree with the generic
    /// minterm evaluation they short-circuit.
    #[test]
    fn rop3_fast_paths_agree_with_generic_evaluation() {
        fn generic(rop: u32, pat: u16, src: u16, dst: u16) -> u16 {
            let t = rop3::table(rop);
            let mut out = 0u16;
            for i in 0..8u32 {
                if (t >> i) & 1 == 0 {
                    continue;
                }
                let pm = if i & 4 != 0 { pat } else { !pat };
                let sm = if i & 2 != 0 { src } else { !src };
                let dm = if i & 1 != 0 { dst } else { !dst };
                out |= pm & sm & dm;
            }
            out
        }
        for code in [
            rop3::SRCCOPY,
            rop3::SRCAND,
            rop3::SRCPAINT,
            rop3::SRCINVERT,
            rop3::NOTSRCCOPY,
            rop3::PATCOPY,
            rop3::BLACKNESS,
            rop3::WHITENESS,
            rop3::DSTINVERT,
        ] {
            for &p in &[0x0000u16, 0xffff, 0xa5a5] {
                for &s in &[0x0000u16, 0xffff, 0x07e0] {
                    for &d in &[0x0000u16, 0xffff, 0xf81f] {
                        assert_eq!(
                            rop3::apply(code, p, s, d),
                            generic(code, p, s, d),
                            "fast path diverges for 0x{code:08x}"
                        );
                    }
                }
            }
        }
    }

    /// `uses_src` / `uses_dst` decide whether we bother fetching an
    /// operand at all, so a false negative silently drops pixels.
    /// PATINVERT is `pat ^ dst`: it is source-independent even though
    /// its name suggests otherwise.
    #[test]
    fn rop3_operand_dependence() {
        for code in [
            rop3::PATCOPY,
            rop3::BLACKNESS,
            rop3::WHITENESS,
            rop3::DSTINVERT,
            rop3::PATINVERT,
        ] {
            assert!(
                !rop3::uses_src(code),
                "0x{code:08x} must not require a source"
            );
        }
        for code in [
            rop3::SRCCOPY,
            rop3::SRCAND,
            rop3::SRCPAINT,
            rop3::SRCINVERT,
            rop3::NOTSRCCOPY,
            rop3::MERGECOPY,
        ] {
            assert!(rop3::uses_src(code), "0x{code:08x} must require a source");
        }
        for code in [rop3::SRCCOPY, rop3::NOTSRCCOPY, rop3::PATCOPY] {
            assert!(
                !rop3::uses_dst(code),
                "0x{code:08x} must not require a destination"
            );
        }
        for code in [rop3::SRCAND, rop3::SRCPAINT, rop3::DSTINVERT] {
            assert!(
                rop3::uses_dst(code),
                "0x{code:08x} must require a destination"
            );
        }
    }

    /// The masked-sprite idiom PegCards uses: `SRCAND` the mask to
    /// punch a hole, then `SRCPAINT` the art into it. Getting this
    /// wrong is what made the card backs render as the mask pattern.
    #[test]
    fn two_pass_masked_blit_composites_sprite() {
        let mut bm = Bitmap::new(4, 1);
        let mut dst = Surface::Bitmap(&mut bm);
        dst.fill_rect(0, 0, 4, 1, 0x07e0); // green background

        // Mask: black (0x0000) where the sprite is opaque, white
        // (0xffff) where the background must survive.
        let mask: Vec<u8> = [0x0000u16, 0x0000, 0xffff, 0xffff]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        // Art: colour where opaque, black where transparent.
        let art: Vec<u8> = [0xf800u16, 0x001f, 0x0000, 0x0000]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();

        dst.blit_from_bytes_rop(0, 0, 0, 0, 4, 1, &mask, 4, 1, rop3::SRCAND, 0);
        dst.blit_from_bytes_rop(0, 0, 0, 0, 4, 1, &art, 4, 1, rop3::SRCPAINT, 0);

        let out: Vec<u16> = dst
            .pixels()
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(out, vec![0xf800, 0x001f, 0x07e0, 0x07e0]);
    }

    #[test]
    fn fill_rect_rop_inverts_destination() {
        let mut bm = Bitmap::new(2, 1);
        let mut s = Surface::Bitmap(&mut bm);
        s.fill_rect(0, 0, 2, 1, 0x1234);
        s.fill_rect_rop(0, 0, 2, 1, 0, rop3::DSTINVERT);
        let out: Vec<u16> = s
            .pixels()
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(out, vec![!0x1234u16, !0x1234u16]);
    }

    #[test]
    fn create_then_select_brush() {
        let mut g = GdiState::new();
        let dc = g.create_memory_dc();
        let brush = g.create_solid_brush(0x00_22_44_66);
        let prev = g.select_into(dc, brush);
        assert_ne!(prev, 0);
        assert_eq!(g.dc(dc).unwrap().brush_color, 0x00_22_44_66);
    }

    #[test]
    fn delete_removes_object() {
        let mut g = GdiState::new();
        let p = g.create_pen(0x00_ff_ff_ff, 1);
        assert!(g.delete(p));
        assert!(g.get(p).is_none());
    }

    #[test]
    fn selected_bitmap_survives_delete_until_deselected() {
        let mut g = GdiState::new();
        let dc = g.create_memory_dc();
        let bitmap = g.create_compatible_bitmap(4, 3);
        let previous = g.select_into(dc, bitmap);
        assert!(!g.delete(bitmap));
        assert!(g.bitmap(bitmap).is_some());
        g.select_into(dc, previous);
        assert!(g.delete(bitmap));
        assert!(g.bitmap(bitmap).is_none());
    }

    #[test]
    fn stock_objects_are_immortal() {
        let mut g = GdiState::new();
        assert!(g.delete(STOCK_WHITE_BRUSH));
        assert!(g.brush(STOCK_WHITE_BRUSH).is_some());
    }

    #[test]
    fn fill_rect_on_bitmap() {
        let mut g = GdiState::new();
        let bm_h = g.create_compatible_bitmap(4, 4);
        {
            let bm = g.bitmap_mut(bm_h).unwrap();
            let mut surf = Surface::Bitmap(bm);
            surf.fill_rect(0, 0, 4, 4, 0xf800); // pure red in RGB565
        }
        let bm = g.bitmap(bm_h).unwrap();
        assert_eq!(bm.pixels[0], 0x00);
        assert_eq!(bm.pixels[1], 0xf8);
    }
    #[test]
    fn native_map_lines_clip_extreme_coordinates_and_exclude_last_pixel() {
        let mut bitmap = Bitmap::new(16, 16);
        let mut surface = Surface::Bitmap(&mut bitmap);
        surface.line((i32::MIN, 8), (i32::MAX, 8), 0xffff, 1);
        assert_eq!(
            surface
                .pixels()
                .chunks_exact(2)
                .filter(|b| *b == [255, 255])
                .count(),
            16
        );
        surface.line((2, 2), (5, 2), 0x1234, 1);
        assert_eq!(
            &surface.pixels()[(2 * 16 + 4) * 2..(2 * 16 + 4) * 2 + 2],
            &0x1234u16.to_le_bytes()
        );
        assert_eq!(
            &surface.pixels()[(2 * 16 + 5) * 2..(2 * 16 + 5) * 2 + 2],
            &[0, 0]
        );
        surface.polygon(&[(0, 0), (4, 0), (4, 4), (0, 4)], 0xabcd);
        assert_eq!(&surface.pixels()[0..2], &0xabcdu16.to_le_bytes());
    }
}
