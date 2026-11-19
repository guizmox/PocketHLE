//! Native compressed storage layout; never infer a format from its byte count.
//! Khronos KHR_texture_compression_astc_hdr and IMG_texture_compression_pvrtc.
pub(super) fn layout(format: u32) -> Option<(usize, usize, usize)> {
    match format {
        0x83f0 | 0x83f1 | 0x8c4c | 0x8c4d | 0x8d64 | 0x8c92 | 0x9270 | 0x9271 | 0x9274..=0x9277 => {
            Some((4, 4, 8))
        }
        0x83f2 | 0x83f3 | 0x8c4e | 0x8c4f | 0x8c93 | 0x87ee | 0x9272 | 0x9273 | 0x9278 | 0x9279 => {
            Some((4, 4, 16))
        }
        0x93b0..=0x93bd | 0x93d0..=0x93dd => {
            const SHAPES: [(usize, usize); 14] = [
                (4, 4),
                (5, 4),
                (5, 5),
                (6, 5),
                (6, 6),
                (8, 5),
                (8, 6),
                (8, 8),
                (10, 5),
                (10, 6),
                (10, 8),
                (10, 10),
                (12, 10),
                (12, 12),
            ];
            let (w, h) = SHAPES[(format & 15) as usize];
            Some((w, h, 16))
        }
        _ => None,
    }
}
/// The driver has already accepted this call. Whole-image replacement is valid
/// even for PVRTC (whose adjacent data prevents independent block edits).
pub(super) fn update(
    format: u32,
    size: [i32; 2],
    rect: [i32; 4],
    storage: &mut Vec<u8>,
    data: &[u8],
) -> bool {
    let [width, height] = size;
    let [x, y, w, h] = rect;
    if width <= 0
        || height <= 0
        || x < 0
        || y < 0
        || w < 0
        || h < 0
        || x.checked_add(w).is_none_or(|end| end > width)
        || y.checked_add(h).is_none_or(|end| end > height)
    {
        return false;
    }
    if rect == [0, 0, width, height] {
        storage.clear();
        storage.extend_from_slice(data);
        return true;
    }
    let Some((bw, bh, bytes)) = layout(format) else {
        return false;
    };
    let (width, height, x, y, w, h) = (
        width as usize,
        height as usize,
        x as usize,
        y as usize,
        w as usize,
        h as usize,
    );
    if x % bw != 0
        || y % bh != 0
        || (w % bw != 0 && x + w != width)
        || (h % bh != 0 && y + h != height)
    {
        return false;
    }
    let columns = width.div_ceil(bw);
    let rows = h.div_ceil(bh);
    let count = w.div_ceil(bw) * bytes;
    if storage.len() != columns * height.div_ceil(bh) * bytes || data.len() != rows * count {
        return false;
    }
    for row in 0..rows {
        let at = ((y / bh + row) * columns + x / bw) * bytes;
        storage[at..at + count].copy_from_slice(&data[row * count..(row + 1) * count]);
    }
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn astc_non_square_and_edge_updates_preserve_other_blocks() {
        let mut blocks = vec![1; 3 * 2 * 16]; // 13x7 ASTC 5x4
        assert!(update(0x93b1, [13, 7], [5, 4, 8, 3], &mut blocks, &[9; 32]));
        assert_eq!(&blocks[..64], &[1; 64]);
        assert_eq!(&blocks[64..], &[9; 32]);
    }
    #[test]
    fn whole_pvrtc_and_unknown_images_need_no_block_guess() {
        let mut blocks = vec![1; 32];
        assert!(update(0x8c01, [8, 4], [0, 0, 8, 4], &mut blocks, &[2; 32]));
        assert_eq!(blocks, vec![2; 32]);
        assert!(!update(0xffff, [8, 4], [0, 0, 4, 4], &mut blocks, &[3; 8]));
        assert_eq!(blocks, vec![2; 32]);
    }
    #[test]
    fn equal_byte_counts_do_not_make_unknown_partial_formats_safe() {
        let mut blocks = vec![1; 128];
        assert!(!update(
            0xffff,
            [16, 16],
            [0, 0, 4, 4],
            &mut blocks,
            &[2; 8]
        ));
        assert!(!update(
            0x83f1,
            [16, 16],
            [2, 0, 4, 4],
            &mut blocks,
            &[2; 8]
        ));
        assert_eq!(blocks, vec![1; 128]);
    }
}
