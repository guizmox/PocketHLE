//! Native compressed storage layout; never infer a format from its byte count.
//! Layouts follow Khronos ASTC, S3TC, ETC/EAC, ATC, RGTC, LATC and BPTC.
//! PVRTC remains opaque: its storage cannot be edited as independent blocks.
pub(super) fn layout(format: u32) -> Option<(usize, usize, usize)> {
    match format {
        0x83f0
        | 0x83f1
        | 0x8c4c
        | 0x8c4d
        | 0x8d64
        | 0x8c92
        | 0x8c70
        | 0x8c71
        | 0x8dbb
        | 0x8dbc
        | 0x9270
        | 0x9271
        | 0x9274..=0x9277 => Some((4, 4, 8)),
        0x83f2
        | 0x83f3
        | 0x8c4e
        | 0x8c4f
        | 0x8c93
        | 0x87ee
        | 0x8c72
        | 0x8c73
        | 0x8dbd
        | 0x8dbe
        | 0x8e8c..=0x8e8f
        | 0x9272
        | 0x9273
        | 0x9278
        | 0x9279 => Some((4, 4, 16)),
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
    // A successfully accepted empty update changes no texels, even when its
    // format has no independently editable block layout (for example PVRTC).
    if w == 0 || h == 0 {
        return data.is_empty();
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
    let Some(count) = w.div_ceil(bw).checked_mul(bytes) else {
        return false;
    };
    let expected_storage = columns
        .checked_mul(height.div_ceil(bh))
        .and_then(|v| v.checked_mul(bytes));
    let expected_data = rows.checked_mul(count);
    if expected_storage != Some(storage.len()) || expected_data != Some(data.len()) {
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

#[cfg(test)]
mod extended_tests {
    use super::*;
    #[test]
    fn signed_and_unsigned_channel_formats_preserve_untouched_blocks() {
        for (format, bytes) in [
            (0x8dbb, 8),
            (0x8dbc, 8),
            (0x8dbd, 16),
            (0x8dbe, 16),
            (0x8c70, 8),
            (0x8c71, 8),
            (0x8c72, 16),
            (0x8c73, 16),
            (0x8e8c, 16),
            (0x8e8d, 16),
            (0x8e8e, 16),
            (0x8e8f, 16),
        ] {
            let mut storage = vec![3; 6 * bytes];
            assert!(update(
                format,
                [12, 8],
                [4, 0, 4, 8],
                &mut storage,
                &vec![7; 2 * bytes]
            ));
            for block in 0..6 {
                assert_eq!(
                    &storage[block * bytes..(block + 1) * bytes],
                    &vec![if block % 3 == 1 { 7 } else { 3 }; bytes],
                    "format {format:#x}, block {block}"
                );
            }
        }
    }
    #[test]
    fn accepted_empty_updates_do_not_require_a_block_layout_or_mutate_storage() {
        let mut storage = vec![3; 32];
        for rect in [[0, 0, 0, 4], [0, 0, 8, 0], [8, 4, 0, 0]] {
            assert!(update(0xffff, [8, 4], rect, &mut storage, &[]));
            assert_eq!(storage, vec![3; 32]);
        }
        assert!(!update(0xffff, [8, 4], [0, 0, 0, 4], &mut storage, &[1]));
        assert!(!update(0xffff, [8, 4], [9, 0, 0, 0], &mut storage, &[]));
    }
}
