//! Preserve pixels outside a DC's application clip for every drawing primitive,
//! including fast ROP and text paths which write pixel slices directly.
use super::Surface;
use std::ops::{Deref, DerefMut};

pub(super) struct ClippedSurface<'a> {
    surface: Surface<'a>,
    clip: Option<Vec<[i32; 4]>>,
    pixels: Vec<u8>,
    painted: Option<Vec<u8>>,
}
impl<'a> ClippedSurface<'a> {
    pub(super) fn new(surface: Surface<'a>, clip: Option<Vec<[i32; 4]>>) -> Self {
        let pixels = if clip.is_some() {
            surface.pixels().to_vec()
        } else {
            Vec::new()
        };
        let painted = if clip.is_some() {
            match &surface {
                Surface::Bitmap(b) => b.painted.clone(),
                _ => None,
            }
        } else {
            None
        };
        Self {
            surface,
            clip,
            pixels,
            painted,
        }
    }
}
impl<'a> Deref for ClippedSurface<'a> {
    type Target = Surface<'a>;
    fn deref(&self) -> &Self::Target {
        &self.surface
    }
}
impl DerefMut for ClippedSurface<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.surface
    }
}
impl Drop for ClippedSurface<'_> {
    fn drop(&mut self) {
        let Some(rects) = &self.clip else {
            return;
        };
        let (width, height) = self.surface.dimensions();
        let mut intervals = Vec::with_capacity(rects.len());
        for y in 0..height as i32 {
            intervals.clear();
            for r in rects {
                if y >= r[1] && y < r[3] {
                    let left = r[0].max(0).min(width as i32) as usize;
                    let right = r[2].max(0).min(width as i32) as usize;
                    if left < right {
                        intervals.push((left, right));
                    }
                }
            }
            intervals.sort_unstable();
            let row = y as usize * width as usize;
            let mut cursor = 0;
            // The union is retained once, including overlapping rectangles.
            for &(left, right) in &intervals {
                if left > cursor {
                    restore(
                        &mut self.surface,
                        &self.pixels,
                        self.painted.as_deref(),
                        row + cursor,
                        row + left,
                    );
                }
                cursor = cursor.max(right);
            }
            if cursor < width as usize {
                restore(
                    &mut self.surface,
                    &self.pixels,
                    self.painted.as_deref(),
                    row + cursor,
                    row + width as usize,
                );
            }
        }
    }
}
fn restore(
    surface: &mut Surface<'_>,
    original: &[u8],
    coverage: Option<&[u8]>,
    start: usize,
    end: usize,
) {
    surface.pixels_mut()[start * 2..end * 2].copy_from_slice(&original[start * 2..end * 2]);
    if let (Surface::Bitmap(bitmap), Some(coverage)) = (surface, coverage) {
        if let Some(painted) = &mut bitmap.painted {
            painted[start..end].copy_from_slice(&coverage[start..end]);
        }
    }
}

pub(super) fn region_kind(rects: &[[i32; 4]]) -> u32 {
    let rects: Vec<_> = rects
        .iter()
        .filter(|r| r[0] < r[2] && r[1] < r[3])
        .collect();
    if rects.is_empty() {
        return 1;
    }
    let left = rects.iter().map(|r| r[0]).min().unwrap();
    let right = rects.iter().map(|r| r[2]).max().unwrap();
    let mut ys: Vec<_> = rects.iter().flat_map(|r| [r[1], r[3]]).collect();
    ys.sort_unstable();
    ys.dedup();
    for pair in ys.windows(2) {
        let mut xs: Vec<_> = rects
            .iter()
            .filter(|r| r[1] <= pair[0] && r[3] >= pair[1])
            .map(|r| (r[0], r[2]))
            .collect();
        xs.sort_unstable();
        let mut end = left;
        for (l, r) in xs {
            if l > end {
                return 3;
            }
            end = end.max(r);
        }
        if end != right {
            return 3;
        }
    }
    2
}

#[cfg(test)]
mod tests {
    use super::region_kind;
    #[test]
    fn region_complexity_uses_union_instead_of_rectangle_count() {
        assert_eq!(region_kind(&[]), 1);
        assert_eq!(region_kind(&[[0, 0, 0, 5]]), 1);
        assert_eq!(region_kind(&[[0, 0, 3, 2], [2, 0, 5, 2]]), 2);
        assert_eq!(region_kind(&[[0, 0, 2, 2], [3, 0, 5, 2]]), 3);
        assert_eq!(region_kind(&[[0, 0, 2, 2], [0, 3, 2, 5]]), 3);
    }
}
