//! Legacy VISE ESIV containers. Read bounded word-oriented Deflate streams;
//! do not execute the self-extractor, its embedded DLL or its installation script.
use super::{charge, u32_at, Cabinet, Result};
use flate2::{Decompress, FlushDecompress};

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}
impl Bits<'_> {
    fn read(&mut self, n: u8) -> Result<u32> {
        let mut value = 0;
        for i in 0..n {
            // VISE stores the least-significant Deflate bits in big-endian words.
            let byte = *self
                .data
                .get((self.at / 8) ^ 1)
                .ok_or("Truncated VISE bits")?;
            value |= u32::from((byte >> (self.at % 8)) & 1) << i;
            self.at += 1;
        }
        Ok(value)
    }
}
struct Huff {
    counts: [u16; 16],
    first: [u16; 16],
    offsets: [usize; 16],
    symbols: Vec<usize>,
}
impl Huff {
    fn new(lengths: &[u8]) -> Result<Self> {
        let mut h = Self {
            counts: [0; 16],
            first: [0; 16],
            offsets: [0; 16],
            symbols: Vec::new(),
        };
        for &n in lengths {
            if n > 15 {
                return Err("Invalid VISE code length".into());
            }
            if n != 0 {
                h.counts[n as usize] += 1;
            }
        }
        let mut code = 0u32;
        let mut offset = 0;
        for n in 1..16 {
            code = (code + u32::from(h.counts[n - 1])) << 1;
            if code + u32::from(h.counts[n]) > 1 << n {
                return Err("Oversubscribed VISE code tree".into());
            }
            h.first[n] = code as u16;
            h.offsets[n] = offset;
            offset += usize::from(h.counts[n]);
        }
        for n in 1..16 {
            for (symbol, &length) in lengths.iter().enumerate() {
                if usize::from(length) == n {
                    h.symbols.push(symbol);
                }
            }
        }
        Ok(h)
    }
    fn read(&self, bits: &mut Bits<'_>) -> Result<usize> {
        let mut code = 0u32;
        for n in 1..16 {
            code = (code << 1) | bits.read(1)?;
            let first = u32::from(self.first[n]);
            if code >= first && code - first < u32::from(self.counts[n]) {
                return Ok(self.symbols[self.offsets[n] + (code - first) as usize]);
            }
        }
        Err("Invalid VISE Huffman code".into())
    }
}
const LENGTHS: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_BITS: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCES: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_BITS: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
fn decode(input: &[u8], limit: usize) -> Result<(Vec<u8>, usize)> {
    let mut b = Bits { data: input, at: 0 };
    let mut out = Vec::new();
    loop {
        let last = b.read(1)? != 0;
        let kind = b.read(2)?;
        if kind == 0 {
            b.at = (b.at + 15) & !15;
            let n = b.read(16)? as usize;
            if b.read(16)? != (n as u32 ^ 0xffff) {
                return Err("Invalid VISE stored block length".into());
            }
            if n > limit.saturating_sub(out.len()) {
                return Err("VISE expanded size limit exceeded".into());
            }
            for _ in 0..n {
                out.push(b.read(8)? as u8);
            }
            if n % 2 != 0 {
                b.read(8)?;
            }
        } else if kind == 1 || kind == 2 {
            let (literal, distance) = if kind == 1 {
                let lengths: Vec<_> = (0..288)
                    .map(|n| match n {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    })
                    .collect();
                (Huff::new(&lengths)?, Huff::new(&[5; 32])?)
            } else {
                let nl = b.read(5)? as usize + 257;
                let nd = b.read(5)? as usize + 1;
                let nc = b.read(4)? as usize + 4;
                if nl > 286 {
                    return Err("Invalid VISE literal count".into());
                }
                let order = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut cl = [0; 19];
                for &i in &order[..nc] {
                    cl[i] = b.read(3)? as u8;
                }
                let tree = Huff::new(&cl)?;
                let mut lengths = Vec::new();
                while lengths.len() < nl + nd {
                    let symbol = tree.read(&mut b)?;
                    let (value, count) = match symbol {
                        0..=15 => (symbol as u8, 1),
                        16 => (
                            *lengths.last().ok_or("VISE repeat without prior code")?,
                            b.read(2)? as usize + 3,
                        ),
                        17 => (0, b.read(3)? as usize + 3),
                        18 => (0, b.read(7)? as usize + 11),
                        _ => return Err("Invalid VISE code-length symbol".into()),
                    };
                    if count > nl + nd - lengths.len() {
                        return Err("VISE code-length repeat overflow".into());
                    }
                    lengths.resize(lengths.len() + count, value);
                }
                if lengths[256] == 0 {
                    return Err("Missing VISE block terminator".into());
                }
                (Huff::new(&lengths[..nl])?, Huff::new(&lengths[nl..])?)
            };
            loop {
                match literal.read(&mut b)? {
                    value @ 0..=255 => {
                        if out.len() >= limit {
                            return Err("VISE expanded size limit exceeded".into());
                        }
                        out.push(value as u8);
                    }
                    256 => break,
                    value @ 257..=285 => {
                        let n = LENGTHS[value - 257] + b.read(LENGTH_BITS[value - 257])? as usize;
                        let d = distance.read(&mut b)?;
                        if d >= 30 {
                            return Err("Invalid VISE distance symbol".into());
                        }
                        let distance = DISTANCES[d] + b.read(DISTANCE_BITS[d])? as usize;
                        if distance > out.len() || n > limit.saturating_sub(out.len()) {
                            return Err("Invalid VISE copy or expanded size".into());
                        }
                        for _ in 0..n {
                            out.push(out[out.len() - distance]);
                        }
                    }
                    _ => return Err("Invalid VISE literal symbol".into()),
                }
            }
        } else {
            return Err("Reserved VISE block type".into());
        }
        if last {
            return Ok((out, (b.at + 15) / 16 * 2));
        }
    }
}

fn cabinet_header(bytes: &[u8], budget: usize) -> Option<usize> {
    if bytes.get(..4)? != b"MSCF" || bytes.get(24..26)? != [3, 1] {
        return None;
    }
    for at in [4, 12, 20] {
        if u32_at(bytes, at)? != 0 {
            return None;
        }
    }
    let size = u32_at(bytes, 8)?;
    let files = u32_at(bytes, 16)?;
    (size >= 36 && size <= budget && files >= 36 && files < size).then_some(size)
}
pub(super) fn extract(
    b: &[u8],
    overlay: usize,
    budget: &mut usize,
) -> Result<Option<Vec<Cabinet>>> {
    if b.get(overlay..overlay.saturating_add(4)) != Some(b"ESIV") {
        return Ok(None);
    }
    let end = b.len().checked_sub(8).ok_or("Truncated VISE footer")?;
    if b.get(end..end + 4) != Some(b"ESIV") || u32_at(b, end + 4) != Some(overlay) {
        return Err("Invalid VISE footer/overlay bounds".into());
    }
    if u32_at(b, overlay + 8) != Some(1) {
        return Err("Unsupported VISE container version".into());
    }
    if end.saturating_sub(overlay) > 64 * 1024 * 1024 {
        return Err("VISE stream index exceeds 64 MiB limit".into());
    }
    let mut out = Vec::new();
    let mut pos = overlay + 16;
    let mut inflate = Decompress::new(false);
    let mut prefix = [0u8; 1024];
    while pos + 8 <= end {
        // A legacy VISE stream has no common magic of its own. Locate candidate
        // CAB-bearing streams inside the validated container bounds, then
        // validate the complete stream and exact CAB header size, not a filename
        // or an offset found in one installer. Stored first blocks align to 16 bits.
        let kind = (b[pos + 1] >> 1) & 3;
        let mut header = [0; 36];
        let mut candidate = false;
        if kind == 2 || kind == 1 {
            let n = (end - pos).min(prefix.len()) & !1;
            for i in (0..n).step_by(2) {
                prefix[i] = b[pos + i + 1];
                prefix[i + 1] = b[pos + i];
            }
            inflate.reset(false);
            if inflate
                .decompress(&prefix[..n], &mut header, FlushDecompress::None)
                .is_ok()
                && inflate.total_out() == 36
            {
                candidate = true;
            }
        } else if kind == 0 && pos + 42 <= end {
            let n = u16::from_be_bytes([b[pos + 2], b[pos + 3]]);
            let inverse = u16::from_be_bytes([b[pos + 4], b[pos + 5]]);
            if n >= 36 && n ^ inverse == 0xffff {
                for i in (0..36).step_by(2) {
                    header[i] = b[pos + 7 + i];
                    header[i + 1] = b[pos + 6 + i];
                }
                candidate = true;
            }
        }
        if let Some(size) = candidate
            .then(|| cabinet_header(&header, super::LIMIT))
            .flatten()
        {
            if size > *budget {
                return Err("VISE expanded size limit exceeded".into());
            }
            let (bytes, consumed) = decode(&b[pos..end], size)?;
            if bytes.len() != size || consumed > end - pos {
                return Err("VISE cabinet size mismatch".into());
            }
            if out.len() >= 32 {
                return Err("Too many VISE cabinets".into());
            }
            charge(bytes.len(), budget)?;
            out.push(Cabinet {
                name: format!("vise-cabinet-{}.cab", out.len() + 1),
                bytes,
            });
            pos += consumed;
        } else {
            pos += 1;
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    fn swap(mut b: Vec<u8>) -> Vec<u8> {
        if b.len() % 2 != 0 {
            b.push(0);
        }
        for pair in b.chunks_exact_mut(2) {
            pair.swap(0, 1);
        }
        b
    }
    fn stored(b: &[u8]) -> Vec<u8> {
        let n = u16::try_from(b.len()).unwrap();
        let mut data = vec![0, 1];
        data.extend(n.to_be_bytes());
        data.extend((n ^ 0xffff).to_be_bytes());
        data.extend(swap(b.to_vec()));
        data
    }
    fn cab(payload: &[u8]) -> Vec<u8> {
        let mut b = cab::CabinetBuilder::new();
        b.add_folder(cab::CompressionType::None)
            .add_file("TEST.000");
        let mut w = b.build(Cursor::new(Vec::new())).unwrap();
        w.next_file().unwrap().unwrap().write_all(payload).unwrap();
        w.finish().unwrap().into_inner()
    }
    fn container(cabs: &[Vec<u8>]) -> Vec<u8> {
        let mut b = b"ESIV\0\0\0\0\x01\0\0\0\xff\xff\xff\xff".to_vec();
        for c in cabs {
            b.extend([0xff; 32]);
            b.extend(stored(c));
        }
        b.extend(b"ESIV\0\0\0\0");
        b
    }
    #[test]
    fn stored_word_alignment_and_dynamic_huffman_decode() {
        for p in [b"even".as_slice(), b"odd", b""] {
            let b = stored(p);
            assert_eq!(decode(&b, 64).unwrap(), (p.to_vec(), b.len()));
        }
        let p = b"Huffman repeated text ".repeat(500);
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&p).unwrap();
        let b = swap(e.finish().unwrap());
        assert_eq!(decode(&b, p.len()).unwrap().0, p);
    }
    #[test]
    fn discovers_multiple_cabinets_and_preserves_exact_bytes() {
        let one = cab(b"first payload");
        let two = cab(b"second payload");
        let bytes = container(&[one.clone(), two.clone()]);
        let mut budget = 1024 * 1024;
        let result = extract(&bytes, 0, &mut budget).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].bytes, one);
        assert_eq!(result[1].bytes, two);
        assert_eq!(budget, 1024 * 1024 - one.len() - two.len());
    }
    #[test]
    fn malformed_frames_size_limits_and_truncation_are_errors() {
        let bytes = container(&[cab(b"payload")]);
        assert!(extract(&bytes, 0, &mut 2).is_err());
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last] = 1;
        assert!(extract(&bad, 0, &mut 65536).is_err());
        let b = stored(b"odd");
        assert!(decode(&b[..b.len() - 1], 64).is_err());
        assert!(decode(&b, 2).is_err());
        let mut bad = stored(b"x");
        bad[5] ^= 1;
        assert!(decode(&bad, 64).is_err());
        assert!(Huff::new(&[1, 1, 1]).is_err());
        assert!(extract(b"ordinary file", 0, &mut 65536).unwrap().is_none());
    }
}
