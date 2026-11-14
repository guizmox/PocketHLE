//! Native edit controls own one caret per input thread. Pocket Word's RichEdit
//! relies on balanced HideCaret/ShowCaret calls while it lays out and paints.
use crate::gdi::Surface;
use std::{collections::HashMap, time::Instant};

#[derive(Debug, Clone)]
pub struct Caret {
    pub hwnd: u32,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub hidden: u32,
    pub gray: bool,
    pub mask: Option<Vec<u16>>,
    pub epoch: Instant,
}

impl Caret {
    pub fn shown(&self, now: Instant, blink_ms: u32) -> bool {
        self.hidden == 0
            && (blink_ms == u32::MAX
                || now.duration_since(self.epoch).as_millis() / u128::from(blink_ms.max(1)) % 2
                    == 0)
    }

    pub(crate) fn render(&self, surface: &mut Surface<'_>, origin: (i32, i32)) {
        let (width, height) = surface.dimensions();
        let x0 = origin.0.saturating_add(self.position.0);
        let y0 = origin.1.saturating_add(self.position.1);
        for y in 0..self.size.1 {
            let sy = y0.saturating_add(y as i32);
            if sy < 0 || sy >= height as i32 {
                continue;
            }
            for x in 0..self.size.0 {
                let sx = x0.saturating_add(x as i32);
                if sx < 0 || sx >= width as i32 || (self.gray && (x + y) % 2 != 0) {
                    continue;
                }
                let mask = self
                    .mask
                    .as_ref()
                    .map_or(u16::MAX, |bits| bits[(y * self.size.0 + x) as usize]);
                let offset = (sy as u32 * width + sx as u32) as usize * 2;
                let pixels = surface.pixels();
                let old = u16::from_le_bytes([pixels[offset], pixels[offset + 1]]);
                surface.put_pixel(sx, sy, old ^ mask);
            }
        }
    }
}

#[derive(Debug)]
pub struct Carets {
    pub threads: HashMap<usize, Caret>,
    pub blink_ms: u32,
}

impl Default for Carets {
    fn default() -> Self {
        Self {
            threads: HashMap::new(),
            blink_ms: 500,
        }
    }
}
