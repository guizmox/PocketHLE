//! TrueType text for explicit WinCE LOGFONT requests. Stock fonts retain the
//! historical bitmap renderer, so games that never select a font do not change.
use crate::gdi::{Font, GdiObject, GdiState, Surface, STOCK_SYSTEM_FONT};
use ab_glyph::{point, Font as _, FontArc, Glyph, PxScale, ScaleFont};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

#[derive(Clone)]
struct Face {
    font: FontArc,
    shear: bool,
}
static FACES: OnceLock<Mutex<HashMap<String, Face>>> = OnceLock::new();

pub fn selected(gdi: &GdiState, handle: u32) -> Option<Font> {
    if handle == STOCK_SYSTEM_FONT {
        return None;
    }
    match gdi.get(handle) {
        Some(GdiObject::Font(f)) => Some(*f),
        _ => None,
    }
}

fn resolve(request: &Font) -> Face {
    let name = request.face_name().to_lowercase();
    let bold = request.field(16) >= 600;
    let italic = request.logfont[20] != 0;
    let key = format!("{name}/{bold}/{italic}");
    let mut cache = FACES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(face) = cache.get(&key) {
        return face.clone();
    }
    // Only known file names are used: a guest face name is never a host path.
    let (windows, family) = match name.as_str() {
        "times new roman" => (
            ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"],
            "Serif",
        ),
        "courier new" | "courier" => (
            ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"],
            "Mono",
        ),
        "tahoma" | "ms sans serif" => (
            ["tahoma.ttf", "tahomabd.ttf", "tahoma.ttf", "tahomabd.ttf"],
            "Tahoma",
        ),
        "ms serif" => (
            ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"],
            "Serif",
        ),
        _ => (
            ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"],
            "Sans",
        ),
    };
    let index = usize::from(bold) + 2 * usize::from(italic);
    let mut roots = Vec::new();
    if let Some(root) = std::env::var_os("POCKETHLE_FONTS_DIR") {
        roots.push(PathBuf::from(root));
    }
    // Opt-in external directory on every platform. Do not depend on host fonts:
    // bundled fonts give Android and Windows identical default substitution.
    let mut found = None;
    for root in roots {
        let path = root.join(windows[index]);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() <= 8 * 1024 * 1024) {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(font) = FontArc::try_from_vec(bytes) {
                    found = Some(Face {
                        font,
                        shear: italic && family == "Tahoma",
                    });
                    break;
                }
            }
        }
    }
    let face = found.unwrap_or_else(|| {
        let bytes: &[u8] = match (family, bold, italic) {
            ("Sans", false, false) => include_bytes!("../assets/fonts/LiberationSans-Regular.ttf"),
            ("Sans", true, false) => include_bytes!("../assets/fonts/LiberationSans-Bold.ttf"),
            ("Sans", false, true) => include_bytes!("../assets/fonts/LiberationSans-Italic.ttf"),
            ("Sans", true, true) => include_bytes!("../assets/fonts/LiberationSans-BoldItalic.ttf"),
            ("Serif", false, false) => {
                include_bytes!("../assets/fonts/LiberationSerif-Regular.ttf")
            }
            ("Serif", true, false) => include_bytes!("../assets/fonts/LiberationSerif-Bold.ttf"),
            ("Serif", false, true) => include_bytes!("../assets/fonts/LiberationSerif-Italic.ttf"),
            ("Serif", true, true) => {
                include_bytes!("../assets/fonts/LiberationSerif-BoldItalic.ttf")
            }
            ("Mono", false, false) => include_bytes!("../assets/fonts/LiberationMono-Regular.ttf"),
            ("Mono", true, false) => include_bytes!("../assets/fonts/LiberationMono-Bold.ttf"),
            ("Mono", false, true) => include_bytes!("../assets/fonts/LiberationMono-Italic.ttf"),
            ("Mono", true, true) => include_bytes!("../assets/fonts/LiberationMono-BoldItalic.ttf"),
            ("Tahoma", false, _) => include_bytes!("../assets/fonts/WineTahoma.ttf"),
            ("Tahoma", true, _) => include_bytes!("../assets/fonts/WineTahoma-Bold.ttf"),
            _ => unreachable!(),
        };
        log::debug!(
            "GDI font substitution: {:?} bold={} italic={} -> bundled {}",
            request.face_name(),
            bold,
            italic,
            family
        );
        Face {
            font: FontArc::try_from_slice(bytes).expect("bundled valid font"),
            shear: italic && family == "Tahoma",
        }
    });
    // A guest can request arbitrary face names; keep host memory bounded.
    if cache.len() >= 128 {
        cache.clear();
    }
    cache.insert(key, face.clone());
    face
}

pub struct Layout {
    pub width: i32,
    pub height: i32,
    pub ascent: i32,
    /// Cumulative advance per UTF-16 code unit, as GetTextExtentExPointW requires.
    pub advances: Vec<i32>,
    face: Option<Face>,
    glyphs: Vec<Glyph>,
    legacy: Vec<(char, i32)>,
    underline: bool,
    strikeout: bool,
    monochrome: bool,
}

pub fn layout(font: Option<&Font>, text: &[u16]) -> Layout {
    layout_dx(font, text, None)
}

pub fn layout_dx(font: Option<&Font>, text: &[u16], dx: Option<&[i32]>) -> Layout {
    let face = font.map(resolve);
    let mut result = Layout {
        width: 0,
        height: 8,
        ascent: 8,
        advances: Vec::new(),
        face,
        glyphs: Vec::new(),
        legacy: Vec::new(),
        underline: false,
        strikeout: false,
        monochrome: false,
    };
    let mut cursor = 0.0f32;
    let mut offset = 0usize;
    let mut scaled = None;
    if let (Some(f), Some(face)) = (font, result.face.as_ref()) {
        let pixels = if f.height == 0 {
            12.0
        } else {
            f.height.unsigned_abs().clamp(1, 256) as f32
        };
        // Positive LOGFONT height is the cell, negative height is the EM square.
        let y = if f.height < 0 {
            pixels * face.font.height_unscaled()
                / face
                    .font
                    .units_per_em()
                    .unwrap_or(face.font.height_unscaled())
        } else {
            pixels
        };
        let mut scale = PxScale { x: y, y };
        if f.field(4) != 0 {
            let sf = face.font.as_scaled(scale);
            let average = ('A'..='Z')
                .chain('a'..='z')
                .map(|c| sf.h_advance(sf.glyph_id(c)))
                .sum::<f32>()
                / 52.0;
            scale.x *= (f.field(4).unsigned_abs().clamp(1, 256) as f32) / average.max(1.0);
        }
        let sf = face.font.as_scaled(scale);
        result.height = sf.height().ceil() as i32;
        result.ascent = sf.ascent().ceil() as i32;
        result.underline = f.logfont[21] != 0;
        result.strikeout = f.logfont[22] != 0;
        result.monochrome = f.logfont[26] == 3; // NONANTIALIASED_QUALITY
        scaled = Some(scale);
    }
    for decoded in char::decode_utf16(text.iter().copied()) {
        let ch = decoded.unwrap_or(char::REPLACEMENT_CHARACTER);
        let units = ch.len_utf16();
        let advance = if let (Some(face), Some(scale)) = (result.face.as_ref(), scaled) {
            let sf = face.font.as_scaled(scale);
            let id = sf.glyph_id(ch);
            result
                .glyphs
                .push(id.with_scale_and_position(scale, point(cursor, result.ascent as f32)));
            sf.h_advance(id)
        } else {
            result.legacy.push((ch, cursor.round() as i32));
            6.0
        };
        cursor += dx
            .and_then(|values| values.get(offset..offset + units))
            .map(|values| values.iter().map(|&v| v.clamp(-4096, 4096) as f32).sum())
            .unwrap_or(advance);
        for _ in 0..units {
            result.advances.push(cursor.round() as i32);
        }
        offset += units;
    }
    result.width = cursor.round() as i32;
    result
}

impl Layout {
    pub fn draw(&self, surface: &mut Surface<'_>, x: i32, y: i32, color: u16, clip: [i32; 4]) {
        if let Some(face) = &self.face {
            for glyph in &self.glyphs {
                if let Some(outline) = face.font.outline_glyph(glyph.clone()) {
                    let bounds = outline.px_bounds();
                    outline.draw(|gx, gy, mut coverage| {
                        let py = bounds.min.y as i32 + gy as i32;
                        let shear = if face.shear {
                            ((self.ascent - py) as f32 * 0.2).round() as i32
                        } else {
                            0
                        };
                        let px = x
                            .saturating_add(bounds.min.x as i32)
                            .saturating_add(gx as i32)
                            .saturating_add(shear);
                        let py = y.saturating_add(py);
                        if self.monochrome {
                            coverage = if coverage >= 0.5 { 1.0 } else { 0.0 };
                        }
                        blend(surface, px, py, color, coverage, clip);
                    });
                }
            }
        } else {
            for &(ch, advance) in &self.legacy {
                crate::font::draw_char_sized_clipped(
                    surface,
                    x.saturating_add(advance),
                    y,
                    ch,
                    color,
                    6,
                    8,
                    clip,
                );
            }
        }
        for line in [
            self.underline.then_some(self.ascent + 1),
            self.strikeout.then_some(self.ascent * 2 / 3),
        ]
        .into_iter()
        .flatten()
        {
            let (sw, _) = surface.dimensions();
            for px in
                x.max(clip[0]).max(0)..x.saturating_add(self.width).min(clip[2]).min(sw as i32)
            {
                blend(surface, px, y.saturating_add(line), color, 1.0, clip);
            }
        }
    }
}

fn blend(surface: &mut Surface<'_>, x: i32, y: i32, color: u16, coverage: f32, clip: [i32; 4]) {
    let (w, h) = surface.dimensions();
    if coverage <= 0.0
        || x < 0
        || y < 0
        || x >= w as i32
        || y >= h as i32
        || x < clip[0]
        || y < clip[1]
        || x >= clip[2]
        || y >= clip[3]
    {
        return;
    }
    let offset = (y as usize * w as usize + x as usize) * 2;
    let pixels = surface.pixels();
    let bg = u16::from_le_bytes([pixels[offset], pixels[offset + 1]]);
    let a = (coverage.clamp(0.0, 1.0) * 255.0).round() as u32;
    let channel = |shift, mask: u16| {
        let fg = ((color >> shift) & mask) as u32;
        let bg = ((bg >> shift) & mask) as u32;
        ((fg * a + bg * (255 - a) + 127) / 255) as u16
    };
    surface.put_pixel(
        x,
        y,
        (channel(11, 31) << 11) | (channel(5, 63) << 5) | channel(0, 31),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn font_truetype_metrics_unicode_and_clipping() {
        let f = Font::new(-16);
        let thin = layout(Some(&f), &"iii".encode_utf16().collect::<Vec<_>>());
        let wide = layout(Some(&f), &"WWWé".encode_utf16().collect::<Vec<_>>());
        assert!(wide.width > thin.width * 2);
        assert_eq!(wide.advances.last(), Some(&wide.width));
        let mut bitmap = crate::gdi::Bitmap::new(80, 32);
        let mut surface = Surface::Bitmap(&mut bitmap);
        wide.draw(&mut surface, 0, 0, 0xffff, [3, 2, 25, 20]);
        let mut painted = false;
        for y in 0..32 {
            for x in 0..80 {
                let off = (y * 80 + x) * 2;
                let pixel = u16::from_le_bytes([surface.pixels()[off], surface.pixels()[off + 1]]);
                if !(3..25).contains(&x) || !(2..20).contains(&y) {
                    assert_eq!(pixel, 0);
                }
                painted |= pixel != 0;
            }
        }
        assert!(painted);
    }
    #[test]
    fn font_explicit_spacing_and_surrogates() {
        let f = Font::new(14);
        let text: Vec<_> = "A😀B".encode_utf16().collect();
        let result = layout_dx(Some(&f), &text, Some(&[5, 4, 6, 8]));
        assert_eq!(result.advances, [5, 15, 15, 23]);
        assert_eq!(result.width, 23);
        assert_eq!(layout(None, &[65, 66]).width, 12);
    }
}

/// TEXTMETRICW from the face actually selected by the renderer.
pub fn metrics_w(font: Option<&Font>) -> [u8; 60] {
    let mut out = [0; 60];
    let sample: Vec<u16> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
        .encode_utf16()
        .collect();
    let m = layout(font, &sample);
    let (mut internal, mut external, mut max_width) = (0, 0, 6);
    let (mut first, mut last) = (32u16, 126u16);
    let mut pitch = 0;
    if let (Some(face), Some(glyph)) = (m.face.as_ref(), m.glyphs.first()) {
        let sf = face.font.as_scaled(glyph.scale);
        let em = face
            .font
            .units_per_em()
            .unwrap_or(face.font.height_unscaled())
            * sf.scale_factor().vertical;
        internal = (sf.height() - em).ceil().max(0.0) as i32;
        external = sf.line_gap().round().max(0.0) as i32;
        max_width = (0..face.font.glyph_count().min(65536))
            .map(|i| sf.h_advance(ab_glyph::GlyphId(i as u16)))
            .fold(0f32, f32::max)
            .ceil() as i32;
        let points: Vec<_> = face
            .font
            .codepoint_ids()
            .map(|(_, c)| c as u32)
            .filter(|c| *c <= 65535)
            .collect();
        first = points.iter().copied().min().unwrap_or(32) as u16;
        last = points.iter().copied().max().unwrap_or(126) as u16;
        let fixed = (' '..='~')
            .map(|c| sf.h_advance(sf.glyph_id(c)))
            .all(|w| (w - sf.h_advance(sf.glyph_id('M'))).abs() < 0.01);
        pitch = 4 | u8::from(!fixed);
    }
    let weight = font.map(|f| f.field(16)).filter(|w| *w != 0).unwrap_or(400);
    let values = [
        m.height,
        m.ascent,
        (m.height - m.ascent).max(0),
        internal,
        external,
        ((m.width as f32) / 52.0).round() as i32,
        max_width,
        weight,
        0,
        96,
        96,
    ];
    for (chunk, value) in out[..44].chunks_exact_mut(4).zip(values) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    for (chunk, value) in out[44..52].chunks_exact_mut(2).zip([first, last, 63, 32]) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    if let Some(f) = font {
        out[52] = f.logfont[20];
        out[53] = f.logfont[21];
        out[54] = f.logfont[22];
        out[55] = pitch | (f.logfont[27] & 0xf0);
        out[56] = f.logfont[23];
    }
    out
}
