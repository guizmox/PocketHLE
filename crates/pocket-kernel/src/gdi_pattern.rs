//! Owned bitmap brushes and a shared paint operand for GDI primitives/ROPs.
use super::{rop3, Bitmap, Dc, GdiObject, GdiState, Surface};
use crate::framebuffer::colorref_to_rgb565;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternBrush {
    width: u32,
    height: u32,
    monochrome: bool,
    indexed: bool,
    zero: u16,
    pixels: Arc<Vec<u8>>,
}
impl PatternBrush {
    pub fn from_bitmap(bitmap: &Bitmap) -> Option<Self> {
        let pattern = Self {
            width: bitmap.width,
            height: bitmap.height,
            monochrome: bitmap.bpp == 1,
            indexed: bitmap.ddb_indices.is_some(),
            zero: bitmap.dib_palette.first().copied().unwrap_or(0),
            pixels: Arc::new(
                bitmap
                    .ddb_indices
                    .as_ref()
                    .unwrap_or(&bitmap.pixels)
                    .clone(),
            ),
        };
        pattern.valid().then_some(pattern)
    }
    pub(super) fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= 16384
            && self.height <= 16384
            && u64::from(self.width) * u64::from(self.height) * if self.indexed { 1 } else { 2 }
                == self.pixels.len() as u64
            && self.pixels.len() <= 64 * 1024 * 1024
    }
}
#[derive(Debug, Clone)]
pub enum BrushPaint {
    Solid(u16),
    Pattern {
        brush: PatternBrush,
        origin: (i32, i32),
        text: u16,
        background: u16,
        palette: Vec<u16>,
    },
}
impl BrushPaint {
    pub fn sample(&self, x: i32, y: i32) -> u16 {
        match self {
            Self::Solid(color) => *color,
            Self::Pattern {
                brush,
                origin,
                text,
                background,
                palette,
            } => {
                let x = (i64::from(x) - i64::from(origin.0)).rem_euclid(i64::from(brush.width))
                    as usize;
                let y = (i64::from(y) - i64::from(origin.1)).rem_euclid(i64::from(brush.height))
                    as usize;
                let offset = y * brush.width as usize + x;
                if brush.indexed {
                    return palette
                        .get(brush.pixels[offset] as usize)
                        .copied()
                        .unwrap_or(0);
                }
                let i = offset * 2;
                let color = u16::from_le_bytes([brush.pixels[i], brush.pixels[i + 1]]);
                if brush.monochrome {
                    if color == brush.zero {
                        *text
                    } else {
                        *background
                    }
                } else {
                    color
                }
            }
        }
    }
}
impl GdiState {
    fn dc_palette(&self, dc: &Dc) -> Vec<u16> {
        self.palette(dc.selected_palette)
            .unwrap_or(&[])
            .iter()
            .map(|p| crate::framebuffer::pack_rgb565(p[0], p[1], p[2]))
            .collect()
    }
    /// Resolve a DDB using the palette of the DC reading or painting it.
    pub fn resolve_dc_bitmap(&mut self, dc: &Dc) {
        let Some(handle) = dc.selected_bitmap else {
            return;
        };
        if !self.bitmap(handle).is_some_and(|b| b.ddb_indices.is_some()) {
            return;
        }
        let palette = self.dc_palette(dc);
        let bitmap = self.bitmap_mut(handle).unwrap();
        for (&index, pixel) in bitmap
            .ddb_indices
            .as_ref()
            .unwrap()
            .iter()
            .zip(bitmap.pixels.chunks_exact_mut(2))
        {
            pixel.copy_from_slice(
                &palette
                    .get(index as usize)
                    .copied()
                    .unwrap_or(0)
                    .to_le_bytes(),
            );
        }
    }

    pub fn create_pattern_brush(&mut self, bitmap: &Bitmap) -> Option<u32> {
        let brush = PatternBrush::from_bitmap(bitmap)?;
        let handle = self.alloc_handle();
        self.objects.insert(handle, GdiObject::PatternBrush(brush));
        Some(handle)
    }
    pub fn brush_paint(&self, handle: u32, dc: &Dc) -> Option<BrushPaint> {
        match self.objects.get(&handle)? {
            GdiObject::Brush(brush) => Some(BrushPaint::Solid(colorref_to_rgb565(brush.color))),
            GdiObject::PatternBrush(brush) => Some(BrushPaint::Pattern {
                brush: brush.clone(),
                origin: dc.brush_origin,
                text: colorref_to_rgb565(dc.text_color),
                background: colorref_to_rgb565(dc.bk_color),
                palette: if brush.indexed {
                    self.dc_palette(dc)
                } else {
                    Vec::new()
                },
            }),
            _ => None,
        }
    }
}
impl Surface<'_> {
    pub fn pixel(&self, x: i32, y: i32) -> u16 {
        let (width, height) = self.dimensions();
        if x < 0 || y < 0 || x as u32 >= width || y as u32 >= height {
            return 0;
        }
        let i = (y as usize * width as usize + x as usize) * 2;
        u16::from_le_bytes([self.pixels()[i], self.pixels()[i + 1]])
    }
    pub fn fill_rect_paint(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        paint: &BrushPaint,
        rop: u32,
    ) {
        if let BrushPaint::Solid(color) = paint {
            self.fill_rect_rop(x, y, w, h, *color, rop);
            return;
        }
        if w <= 0 || h <= 0 {
            return;
        }
        let (width, height) = self.dimensions();
        let left = x.max(0).min(width as i32);
        let top = y.max(0).min(height as i32);
        let right = (i64::from(x) + i64::from(w)).clamp(0, i64::from(width)) as i32;
        let bottom = (i64::from(y) + i64::from(h)).clamp(0, i64::from(height)) as i32;
        if left >= right || top >= bottom {
            return;
        }
        for y in top..bottom {
            for x in left..right {
                let out = rop3::apply(rop, paint.sample(x, y), 0, self.pixel(x, y));
                self.put_pixel(x, y, out);
            }
        }
        self.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gdi::{GdiSnapshot, STOCK_WHITE_BRUSH};
    fn bitmap() -> Bitmap {
        let mut bitmap = Bitmap::new(2, 2);
        bitmap.pixels = [0x1111u16, 0x2222, 0x3333, 0x4444]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        bitmap
    }
    #[test]
    fn indexed_brush_uses_the_drawing_dc_palette_and_retains_its_indices() {
        let mut state = GdiState::new();
        let mut bitmap = Bitmap::new(2, 1);
        bitmap.bpp = 8;
        bitmap.ddb_indices = Some(vec![0, 1]);
        let brush = state.create_pattern_brush(&bitmap).unwrap();
        let palette = state.create_palette(vec![[255, 0, 0, 0], [0, 0, 255, 0]]);
        let mut dc = state.dc(super::super::GDI_SCREEN_DC).unwrap().clone();
        dc.selected_palette = palette;
        let paint = state.brush_paint(brush, &dc).unwrap();
        assert_eq!(paint.sample(0, 0), 0xf800);
        assert_eq!(paint.sample(1, 0), 0x001f);
        let palette = state.create_palette(vec![[0, 255, 0, 0], [255, 255, 255, 0]]);
        dc.selected_palette = palette;
        assert_eq!(state.brush_paint(brush, &dc).unwrap().sample(0, 0), 0x07e0);
        // An already captured paint operand and the source buffer remain independent.
        assert_eq!(paint.sample(0, 0), 0xf800);
        assert_eq!(bitmap.ddb_indices.unwrap(), [0, 1]);
    }

    #[test]
    fn brush_owns_a_copy_and_repeats_in_device_coordinates_after_snapshot() {
        let mut state = GdiState::new();
        let mut source = bitmap();
        let handle = state.create_pattern_brush(&source).unwrap();
        source.pixels.fill(0);
        drop(source);
        let encoded = serde_json::to_vec(&state.capture_snapshot().unwrap()).unwrap();
        let state = serde_json::from_slice::<GdiSnapshot>(&encoded)
            .unwrap()
            .restore()
            .unwrap();
        let mut dc = Dc::default();
        dc.brush_origin = (-1, 1);
        let paint = state.brush_paint(handle, &dc).unwrap();
        assert_eq!(paint.sample(-1, 1), 0x1111);
        assert_eq!(paint.sample(0, 1), 0x2222);
        assert_eq!(paint.sample(1, 0), 0x3333);
        let mut target = Bitmap::new(4, 3);
        let mut surface = Surface::Bitmap(&mut target);
        surface.fill_rect_paint(-2, -1, 6, 4, &paint, rop3::PATCOPY);
        assert_eq!(surface.pixel(0, 0), 0x4444);
        assert_eq!(surface.pixel(3, 2), 0x3333);
        surface.fill_rect_paint(0, 0, 1, 1, &paint, rop3::PATINVERT);
        assert_eq!(surface.pixel(0, 0), 0);
    }
    #[test]
    fn monochrome_colours_are_resolved_from_the_destination_dc() {
        let mut source = Bitmap::new(2, 1);
        source.bpp = 1;
        source.pixels = vec![0, 0, 255, 255];
        let mut state = GdiState::new();
        let brush = state.create_pattern_brush(&source).unwrap();
        let mut dc = Dc::default();
        dc.text_color = 0x000000ff;
        dc.bk_color = 0x00ff0000;
        let paint = state.brush_paint(brush, &dc).unwrap();
        assert_eq!(paint.sample(0, 0), 0xf800);
        assert_eq!(paint.sample(1, 0), 0x001f);
        dc.text_color = 0x0000ff00;
        assert_eq!(state.brush_paint(brush, &dc).unwrap().sample(0, 0), 0x07e0);
        assert!(state.brush_paint(STOCK_WHITE_BRUSH, &dc).is_some());
    }
    #[test]
    fn shape_fills_and_source_rops_use_the_same_pattern() {
        let mut state = GdiState::new();
        let brush = state.create_pattern_brush(&bitmap()).unwrap();
        let paint = state.brush_paint(brush, &Dc::default()).unwrap();
        let mut target = Bitmap::new(6, 6);
        let mut surface = Surface::Bitmap(&mut target);
        surface.ellipse_paint([0, 0, 6, 6], Some(paint.clone()), None);
        assert_eq!(surface.pixel(2, 2), 0x1111);
        assert_eq!(surface.pixel(3, 2), 0x2222);
        surface.round_rect_paint([0, 0, 6, 6], [2, 2], Some(paint.clone()), None);
        assert_eq!(surface.pixel(3, 3), 0x4444);
        surface.polygon_paint(&[(0, 0), (4, 0), (4, 4), (0, 4)], &paint);
        assert_eq!(surface.pixel(1, 2), 0x2222);
        surface.blit_from_bytes_rop_paint(
            0,
            0,
            0,
            0,
            2,
            1,
            &[255; 4],
            2,
            1,
            rop3::MERGECOPY,
            &paint,
        );
        assert_eq!(surface.pixel(0, 0), 0x1111);
        assert_eq!(surface.pixel(1, 0), 0x2222);
    }
}
