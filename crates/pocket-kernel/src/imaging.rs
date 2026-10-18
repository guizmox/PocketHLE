//! Process-owned icon pixels and image lists. No guest pointers survive decoding.
use crate::gdi::{Bitmap, Surface};

pub const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u16>,
    pub alpha: Vec<u8>,
    pub invert: Vec<bool>,
}
impl Image {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let n = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 || n > MAX_IMAGE_PIXELS {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels: vec![0; n as usize],
            alpha: vec![255; n as usize],
            invert: vec![false; n as usize],
        })
    }
    pub fn from_bitmap(bitmap: &Bitmap) -> Option<Self> {
        let mut image = Self::new(bitmap.width, bitmap.height)?;
        if bitmap.pixels.len() != image.pixels.len() * 2 {
            return None;
        }
        for (p, b) in image.pixels.iter_mut().zip(bitmap.pixels.chunks_exact(2)) {
            *p = u16::from_le_bytes([b[0], b[1]]);
        }
        Some(image)
    }
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Self> {
        if x.checked_add(w)? > self.width || y.checked_add(h)? > self.height {
            return None;
        }
        let mut out = Self::new(w, h)?;
        for row in 0..h as usize {
            let a = (y as usize + row) * self.width as usize + x as usize;
            let b = row * w as usize;
            out.pixels[b..b + w as usize].copy_from_slice(&self.pixels[a..a + w as usize]);
            out.alpha[b..b + w as usize].copy_from_slice(&self.alpha[a..a + w as usize]);
            out.invert[b..b + w as usize].copy_from_slice(&self.invert[a..a + w as usize]);
        }
        Some(out)
    }
    pub fn scaled(&self, w: u32, h: u32) -> Option<Self> {
        let mut out = Self::new(w, h)?;
        for y in 0..h {
            for x in 0..w {
                let a = ((u64::from(y) * u64::from(self.height) / u64::from(h))
                    * u64::from(self.width)
                    + u64::from(x) * u64::from(self.width) / u64::from(w))
                    as usize;
                let b = (y * w + x) as usize;
                out.pixels[b] = self.pixels[a];
                out.alpha[b] = self.alpha[a];
                out.invert[b] = self.invert[a];
            }
        }
        Some(out)
    }
    /// Clip destination before iterating; malicious coordinates cannot cause huge loops.
    pub fn draw(&self, surface: &mut Surface<'_>, x: i32, y: i32, w: u32, h: u32, clip: [i32; 4]) {
        if w == 0 || h == 0 {
            return;
        }
        let (sw, sh) = surface.dimensions();
        let l = i64::from(x).max(i64::from(clip[0])).max(0);
        let t = i64::from(y).max(i64::from(clip[1])).max(0);
        let r = (i64::from(x) + i64::from(w))
            .min(i64::from(clip[2]))
            .min(i64::from(sw));
        let b = (i64::from(y) + i64::from(h))
            .min(i64::from(clip[3]))
            .min(i64::from(sh));
        for dy in t..b {
            for dx in l..r {
                let sx = (dx - i64::from(x)) * i64::from(self.width) / i64::from(w);
                let sy = (dy - i64::from(y)) * i64::from(self.height) / i64::from(h);
                let i = (sy * i64::from(self.width) + sx) as usize;
                let off = (dy as usize * sw as usize + dx as usize) * 2;
                let dest = u16::from_le_bytes([surface.pixels()[off], surface.pixels()[off + 1]]);
                let result = if self.invert[i] {
                    dest ^ 0xffff
                } else {
                    blend(self.pixels[i], dest, self.alpha[i])
                };
                if result != dest {
                    surface.put_pixel(dx as i32, dy as i32, result);
                }
            }
        }
    }
}
fn blend(s: u16, d: u16, a: u8) -> u16 {
    if a == 255 {
        return s;
    }
    if a == 0 {
        return d;
    }
    let a = u32::from(a);
    let f = |shift: u32, mask: u32| {
        (((u32::from(s) >> shift) & mask) * a + ((u32::from(d) >> shift) & mask) * (255 - a) + 127)
            / 255
    };
    ((f(11, 31) << 11) | (f(5, 63) << 5) | f(0, 31)) as u16
}
#[derive(Debug, Clone)]
pub struct ImageList {
    pub width: u32,
    pub height: u32,
    pub images: Vec<Image>,
    pub background: u32,
}
impl ImageList {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return None;
        }
        Some(Self {
            width,
            height,
            images: Vec::new(),
            background: 0xffff_ffff,
        })
    }
    pub fn replace(&mut self, index: i32, image: &Image) -> Option<usize> {
        let i = if index == -1 {
            self.images.len()
        } else {
            usize::try_from(index).ok()?
        };
        if i > self.images.len()
            || (index != -1 && i == self.images.len())
            || (i == self.images.len()
                && (i >= 4096
                    || (i as u64 + 1) * u64::from(self.width) * u64::from(self.height)
                        > MAX_IMAGE_PIXELS))
        {
            return None;
        }
        let image = image.scaled(self.width, self.height)?;
        if i == self.images.len() {
            self.images.push(image);
        } else {
            self.images[i] = image;
        }
        Some(i)
    }
    pub fn add_strip(&mut self, image: &Image, mask: Option<u16>) -> Option<usize> {
        if image.height < self.height {
            return None;
        }
        let n = image.width / self.width;
        if n == 0
            || self.images.len() + n as usize > 4096
            || (self.images.len() as u64 + u64::from(n))
                * u64::from(self.width)
                * u64::from(self.height)
                > MAX_IMAGE_PIXELS
        {
            return None;
        }
        let first = self.images.len();
        for i in 0..n {
            let mut tile = image.crop(i * self.width, 0, self.width, self.height)?;
            if let Some(mask) = mask {
                for (p, a) in tile.pixels.iter().zip(&mut tile.alpha) {
                    if *p == mask {
                        *a = 0;
                    }
                }
            }
            self.images.push(tile);
        }
        Some(first)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strip_ignores_only_the_incomplete_trailing_cell() {
        let mut image = Image::new(5, 1).unwrap();
        image.pixels = vec![1, 2, 3, 4, 5];
        let mut list = ImageList::new(2, 1).unwrap();
        assert_eq!(list.add_strip(&image, None), Some(0));
        assert_eq!(list.images.len(), 2);
        assert_eq!(list.images[1].pixels, vec![3, 4]);
    }
    #[test]
    fn masked_strip_and_alpha_draw_preserve_background_and_clip() {
        let mut image = Image::new(4, 1).unwrap();
        image.pixels = vec![0xf81f, 0xf800, 0x07e0, 0x001f];
        let mut list = ImageList::new(2, 1).unwrap();
        assert_eq!(list.add_strip(&image, Some(0xf81f)), Some(0));
        assert_eq!(list.images.len(), 2);
        let mut bitmap = Bitmap::new(4, 2);
        bitmap.pixels.fill(0xff);
        list.images[0].draw(&mut Surface::Bitmap(&mut bitmap), 1, 0, 2, 1, [0, 0, 4, 2]);
        assert_eq!(&bitmap.pixels[2..6], &[0xff, 0xff, 0, 0xf8]);
        list.images[1].alpha[0] = 128;
        list.images[1].draw(&mut Surface::Bitmap(&mut bitmap), -1, 1, 2, 1, [0, 1, 1, 2]);
        assert_eq!(&bitmap.pixels[8..12], &[0x1f, 0, 0xff, 0xff]);
        assert_ne!(blend(0xf800, 0x001f, 128), 0xf800);
    }
    #[test]
    fn independent_lists_append_and_replace_own_icon_copies() {
        let mut first = ImageList::new(2, 2).unwrap();
        let second = ImageList::new(1, 1).unwrap();
        let mut icon = Image::new(1, 1).unwrap();
        icon.pixels[0] = 0x07e0;
        assert_eq!(first.replace(-1, &icon), Some(0));
        icon.pixels[0] = 0xf800;
        assert_eq!(first.images[0].pixels[3], 0x07e0);
        assert!(second.images.is_empty());
        assert_eq!(first.replace(2, &icon), None);
        assert_eq!(first.replace(0, &icon), Some(0));
        assert_eq!(first.images.len(), 1);
    }
}
