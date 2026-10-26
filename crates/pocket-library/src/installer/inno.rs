//! Legacy 32-bit Inno Setup 2.0.11 containers. Only data is decoded; setup
//! scripts and desktop executables are never run. Format reference:
//! https://github.com/dscharrer/innoextract (loader/offsets, stream/block,
//! setup/data). Other Inno versions need their own metadata layouts.
use super::{charge, u32_at, Result};
use std::io::{Cursor, Read};

fn word(b: &[u8], p: usize) -> Result<usize> {
    u32_at(b, p).ok_or_else(|| "Truncated Inno metadata".into())
}
fn range(b: &[u8], p: usize, n: usize) -> Result<&[u8]> {
    b.get(p..p.checked_add(n).ok_or("Inno offset overflow")?)
        .ok_or_else(|| "Invalid Inno bounds".into())
}
fn crc32(b: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in b {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
fn adler32(b: &[u8]) -> u32 {
    let (mut a, mut c) = (1u32, 0u32);
    for chunk in b.chunks(5552) {
        for byte in chunk {
            a += *byte as u32;
            c += a;
        }
        a %= 65521;
        c %= 65521;
    }
    (c << 16) | a
}
fn block(b: &[u8], pos: &mut usize, budget: &mut usize) -> Result<Vec<u8>> {
    let header = range(b, *pos, 12)?;
    if crc32(&header[4..]) != word(header, 0)? as u32 {
        return Err("Inno block header CRC mismatch".into());
    }
    let compressed = word(header, 4)?;
    let size = word(header, 8)?;
    if size > *budget {
        return Err("Inno block exceeds decoded size limit".into());
    }
    let stored = compressed == u32::MAX as usize;
    let mut left = if stored { size } else { compressed };
    if left > super::LIMIT {
        return Err("Inno block exceeds input size limit".into());
    }
    *pos += 12;
    let mut packed = Vec::new();
    while left != 0 {
        let n = left.min(4096);
        let expected = word(b, *pos)? as u32;
        let data = range(b, *pos + 4, n)?;
        if crc32(data) != expected {
            return Err("Inno block CRC mismatch".into());
        }
        packed.extend_from_slice(data);
        *pos += 4 + n;
        left -= n;
    }
    let out = if stored {
        charge(packed.len(), budget)?;
        packed
    } else {
        super::inflate(&packed, true, budget)?
    };
    if out.len() != size {
        return Err("Inno block size mismatch".into());
    }
    Ok(out)
}

pub(super) fn extract(b: &[u8], budget: &mut usize) -> Result<Option<Vec<(String, Vec<u8>)>>> {
    if !b.starts_with(b"MZ") || b.get(0x30..0x34) != Some(b"Inno") {
        return Ok(None);
    }
    let table = word(b, 0x34)?;
    if word(b, 0x38)? as u32 != !(table as u32) {
        return Err("Invalid Inno loader pointer".into());
    }
    let loader = range(b, table, 44)?;
    if &loader[..12] != b"rDlPtS02\x87eVx" {
        return Err("Unsupported Inno loader version".into());
    }
    let header = word(loader, 36)?;
    let data = word(loader, 40)?;
    let version = range(b, header, 64)?;
    if version.split(|c| *c == 0).next() != Some(b"Inno Setup Setup Data (2.0.11)".as_slice()) {
        return Err("Unsupported Inno metadata version".into());
    }
    let mut pos = header + 64;
    // The first stream contains setup instructions. Validate it, but never
    // execute it or use its paths for host filesystem writes.
    let _setup = block(b, &mut pos, budget)?;
    let entries = block(b, &mut pos, budget)?;
    if entries.len() % 41 != 0 || entries.len() / 41 > 4096 {
        return Err("Invalid Inno file table".into());
    }
    let slice = range(b, data, 12)?;
    if &slice[..8] != b"idska32\x1a" {
        return Err("Unsupported Inno data slice".into());
    }
    let slice_size = word(slice, 8)?;
    let payload = range(b, data, slice_size)?;
    if data.checked_add(slice_size).is_none_or(|end| end > header) {
        return Err("Overlapping Inno data and metadata".into());
    }
    let mut out = Vec::new();
    for (i, entry) in entries.chunks_exact(41).enumerate() {
        if word(entry, 0)? != 1 || word(entry, 4)? != 1 {
            return Err("External Inno data slices are not supported".into());
        }
        let offset = word(entry, 8)?;
        let size = word(entry, 12)?;
        let packed_size = word(entry, 16)?;
        if size > *budget {
            return Err("Inno file exceeds decoded size limit".into());
        }
        if range(payload, offset, 4)? != b"zlb\x1a" {
            return Err("Invalid Inno file chunk".into());
        }
        let packed = range(payload, offset + 4, packed_size)?;
        let mut decoder = flate2::bufread::ZlibDecoder::new(Cursor::new(packed));
        let mut decoded = Vec::new();
        decoder
            .by_ref()
            .take(size as u64 + 1)
            .read_to_end(&mut decoded)
            .map_err(|e| format!("Inno file decompression failed: {e}"))?;
        if decoded.len() != size
            || decoder.total_in() as usize != packed_size
            || adler32(&decoded) != word(entry, 20)? as u32
        {
            return Err("Inno file size or checksum mismatch".into());
        }
        charge(decoded.len(), budget)?;
        out.push((format!("inno-file-{i}"), decoded));
    }
    Ok(Some(out))
}
