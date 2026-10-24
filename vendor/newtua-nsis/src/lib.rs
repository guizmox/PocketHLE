// SPDX-FileCopyrightText: 2026 Aleksei Trankov and contributors
// SPDX-License-Identifier: LGPL-3.0-or-later

//! NSIS (Nullsoft Scriptable Install System) installer archives.
//!
//! NSIS installers are self-extracting Windows executables: the payload lives
//! *after* the exe stub, introduced by a 512-byte-aligned "firstheader"
//! (`0xDEADBEEF` + `NullsoftInst`). This crate is a faithful port of the
//! **deterministic** branch of XADMaster's `XADNSISParser` — the *sectioned
//! header* used by NSIS **2.0 and newer** (including 3.x Unicode builds), where
//! every offset is read from a fixed header field. The pre-2.0 heuristic
//! branches (which guess opcode strides statistically) are intentionally not
//! ported and surface as [`io::ErrorKind::Unsupported`].
//!
//! Supported payload codecs, solid and non-solid: **LZMA** (the modern default),
//! **NSIS-deflate**, the custom **NSIS-bzip2** (both the NSIS2 and the randomized
//! NSIS1 variants) and **FilteredLZMA** (a filter selector plus LZMA, including
//! the x86 BCJ branch filter). Only the legacy zlib format is out of scope and
//! yields `Unsupported` rather than corrupt output.
//!
//! The public API is charset-agnostic: entry names are raw bytes with `/`
//! separators (ANSI names are left in their original codepage; UTF-16 Unicode
//! names are decoded to UTF-8). Windows shell variables (`$WINDIR`, …) are left
//! as the symbolic placeholders the reference emits, not resolved to real paths.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::{self, Read, Write};

mod bcj;
mod bzip2;
mod codec;
mod paths;

use codec::Codec;

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn unsupported(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, msg.into())
}

/// Read a little-endian `u32` at `pos`; caller must have bounded `pos + 4`.
fn rd32(data: &[u8], pos: usize) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&data[pos..pos + 4]);
    u32::from_le_bytes(a)
}

/// The New-format firstheader magic checked at `firstheader + 4`: `0xDEADBEEF`
/// (little-endian) followed by ASCII `NullsoftInst` (`XADNSISParser.m:43-49`).
/// The lowercase `s` distinguishes it from the pre-1.6 `NullSoft` signature,
/// which this crate does not handle.
const NEW_MAGIC: [u8; 16] = [
    0xef, 0xbe, 0xad, 0xde, b'N', b'u', b'l', b'l', b's', b'o', b'f', b't', b'I', b'n', b's', b't',
];

/// The maximum span scanned for the firstheader (`requiredHeaderSize`, `:76`).
const REQUIRED_HEADER_SIZE: usize = 0x10000;

/// One entry (file or directory) of an NSIS installer.
pub struct NsisEntry {
    name: Vec<u8>,
    is_dir: bool,
    /// Key into the block map (the install-script's data offset).
    data_offset: u32,
    /// Compressed size for a compressed member; for stored/solid members this is
    /// the exact byte length.
    compressed_size: u32,
    /// Uncompressed size when known ahead of extraction (stored or solid).
    stored_size: Option<u32>,
    /// Windows FILETIME (100 ns ticks since 1601-01-01), or `None` if absent.
    filetime: Option<u64>,
}

impl NsisEntry {
    /// Raw path bytes, `/`-separated (see the crate docs on charset handling).
    pub fn name(&self) -> &[u8] {
        &self.name
    }
    /// Whether this entry is a directory (a `SetOutPath`).
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
    /// Uncompressed size, if known without extracting (stored or solid members).
    pub fn size(&self) -> Option<u64> {
        self.stored_size.map(u64::from)
    }
    /// Compressed size of the member's data block (the exact byte length for
    /// stored/solid members).
    pub fn compressed_size(&self) -> u64 {
        u64::from(self.compressed_size)
    }
    /// The raw Windows FILETIME, or `None`. Conversion to Unix time is left to
    /// the caller (the reference does it centrally, not here).
    pub fn filetime(&self) -> Option<u64> {
        self.filetime
    }
}

/// How an archive's member data is read back.
enum Reader {
    /// One continuous decompressed stream; members are slices of it.
    Solid { stream: Vec<u8>, base: usize },
    /// Per-member blocks in the original file; each is decoded on demand.
    NonSolid { data: Vec<u8>, base: usize },
}

/// A parsed NSIS installer.
pub struct NsisArchive {
    entries: Vec<NsisEntry>,
    reader: Reader,
}

impl NsisArchive {
    /// Whether `data` contains a recognisable New-format NSIS firstheader (and is
    /// not an uninstaller). Scans on 512-byte boundaries, like the reference.
    pub fn recognize(data: &[u8]) -> bool {
        find_firstheader(data).is_some()
    }

    /// Read a whole installer from `r` and parse its sectioned header.
    ///
    /// Errors: [`InvalidData`](io::ErrorKind::InvalidData) if no New firstheader
    /// is found; [`Unsupported`](io::ErrorKind::Unsupported) for pre-2.0
    /// (non-sectioned) headers and the deferred/out-of-scope codecs.
    pub fn open<R: Read>(mut r: R) -> io::Result<Self> {
        let mut data = Vec::new();
        r.read_to_end(&mut data)?;
        Self::from_data(data)
    }

    fn from_data(data: Vec<u8>) -> io::Result<Self> {
        let fh_off =
            find_firstheader(&data).ok_or_else(|| invalid("nsis: no NSIS firstheader found"))?;
        let (entries, reader) = parse(data, fh_off)?;
        Ok(Self { entries, reader })
    }

    /// The entries, directories first then files (in the reference's order).
    pub fn entries(&self) -> &[NsisEntry] {
        &self.entries
    }

    /// Decode member `idx` and write its bytes to `out`.
    pub fn read_entry(&self, idx: usize, out: &mut dyn Write) -> io::Result<()> {
        let e = self
            .entries
            .get(idx)
            .ok_or_else(|| invalid("nsis: entry index out of range"))?;
        if e.is_dir {
            return Err(invalid("nsis: entry is a directory"));
        }
        match &self.reader {
            Reader::Solid { stream, base } => {
                let off = base + e.data_offset as usize;
                let len = block_len(stream, off)?;
                let slice = stream
                    .get(off + 4..off + 4 + len)
                    .ok_or_else(|| invalid("nsis: solid member past end of stream"))?;
                out.write_all(slice)
            }
            Reader::NonSolid { data, base } => {
                let off = base + e.data_offset as usize;
                let raw = rd32(data, off);
                let len = (raw & 0x7fffffff) as usize;
                let block = data
                    .get(off + 4..off + 4 + len)
                    .ok_or_else(|| invalid("nsis: member data past end of file"))?;
                if raw & 0x80000000 != 0 {
                    let codec = codec::sniff(&block[..block.len().min(7)])?;
                    let decoded = codec::decode(codec, block)?;
                    out.write_all(&decoded)
                } else {
                    out.write_all(block)
                }
            }
        }
    }
}

/// Read a block's `len` field at `off`, verifying the four length bytes fit.
fn block_len(data: &[u8], off: usize) -> io::Result<usize> {
    if off + 4 > data.len() {
        return Err(invalid("nsis: block length past end of stream"));
    }
    Ok((rd32(data, off) & 0x7fffffff) as usize)
}

/// Scan for a New-format firstheader on 512-byte boundaries, rejecting
/// uninstaller archives (`flags & 2`) — `XADNSISParser.m:78-108`.
fn find_firstheader(data: &[u8]) -> Option<usize> {
    let limit = data.len().min(REQUIRED_HEADER_SIZE);
    let mut off = 0;
    while off + 4 + 16 <= limit {
        if data[off + 4..off + 20] == NEW_MAGIC && rd32(data, off) & 2 == 0 {
            return Some(off);
        }
        off += 512;
    }
    None
}

/// Parse the payload starting at firstheader `fh_off`.
fn parse(data: Vec<u8>, fh_off: usize) -> io::Result<(Vec<NsisEntry>, Reader)> {
    if fh_off + 28 > data.len() {
        return Err(invalid("nsis: truncated firstheader"));
    }
    let flags = rd32(&data, fh_off);
    let headerlength = rd32(&data, fh_off + 20) as usize;
    let totallength = rd32(&data, fh_off + 24) as usize;
    let pos = fh_off + 28;

    let mut datalength = totallength.saturating_sub(32);
    if flags & 1 != 0 {
        datalength = datalength.saturating_sub(4);
    }
    let region_end = (pos + datalength).min(data.len());
    let region = &data[pos.min(data.len())..region_end];

    let sig = &data[pos.min(data.len())..(pos + 7).min(data.len())];
    let sniffed = codec::sniff(sig);

    // Solid attempt (`attemptSolidHandleAtPosition:`, `:1209-1224`): decode the
    // whole stream with each candidate codec; if its first u32 is the header
    // length, the guess is right. The order mirrors the reference (`:322-329`) —
    // it is here that the randomized NSIS1 bzip2 variant is reached.
    let candidates = [
        Codec::Lzma,
        Codec::FilteredLzma,
        Codec::NsisBzip2 { hasrand: false },
        Codec::NsisBzip2 { hasrand: true },
        Codec::NsisDeflate,
    ];
    for codec in candidates {
        if let Some(stream) = try_solid(codec, region, headerlength) {
            return build_solid(stream, headerlength);
        }
    }

    // Non-solid: the header is the first block, the rest follow after it.
    match build_nonsolid(data, pos, headerlength, datalength) {
        Ok(result) => Ok(result),
        Err(e) => {
            // If the data area starts with a known-unsupported signature, report
            // that rather than the incidental non-solid failure.
            match sniffed {
                Err(unsup) => Err(unsup),
                Ok(_) => Err(e),
            }
        }
    }
}

/// Decode the whole `region` with `codec`; return it only if it opens with the
/// expected header length (confirming a solid layout).
fn try_solid(codec: Codec, region: &[u8], headerlength: usize) -> Option<Vec<u8>> {
    let decoded = codec::decode(codec, region).ok()?;
    if decoded.len() >= 4 && rd32(&decoded, 0) == headerlength as u32 {
        Some(decoded)
    } else {
        None
    }
}

/// Build entries for a solid archive (`:334-343`). The decompressed stream is
/// `[u32 headerlength][header][blocks…][optional CRC]`; `base = headerlength+4`.
fn build_solid(stream: Vec<u8>, headerlength: usize) -> io::Result<(Vec<NsisEntry>, Reader)> {
    if stream.len() < 4 + headerlength {
        return Err(invalid("nsis: solid stream shorter than header"));
    }
    let header = &stream[4..4 + headerlength];
    let base = headerlength + 4;
    require_sectioned(header)?;
    let blocks = find_blocks(&stream, base, stream.len());
    let entries = parse_opcodes(header, &blocks, true)?;
    Ok((entries, Reader::Solid { stream, base }))
}

/// Build entries for a non-solid archive (`:351-360`). The install header is the
/// first block at `pos`; file blocks start at `base = pos + 4 + headercompsize`.
fn build_nonsolid(
    data: Vec<u8>,
    pos: usize,
    headerlength: usize,
    datalength: usize,
) -> io::Result<(Vec<NsisEntry>, Reader)> {
    if pos + 4 > data.len() {
        return Err(invalid("nsis: truncated non-solid header block"));
    }
    let headerblocklen = rd32(&data, pos);
    let headercompsize = (headerblocklen & 0x7fffffff) as usize;
    let compressed = headerblocklen & 0x80000000 != 0;
    let hstart = pos + 4;
    let hend = hstart
        .checked_add(headercompsize)
        .filter(|&e| e <= data.len())
        .ok_or_else(|| invalid("nsis: header block past end of file"))?;
    let hblock = &data[hstart..hend];

    let header: Vec<u8> = if compressed {
        let codec = codec::sniff(&hblock[..hblock.len().min(7)])?;
        let mut decoded = codec::decode(codec, hblock)?;
        if decoded.len() < headerlength {
            return Err(invalid("nsis: decoded header shorter than expected"));
        }
        decoded.truncate(headerlength);
        decoded
    } else {
        hblock
            .get(..headerlength)
            .ok_or_else(|| invalid("nsis: stored header shorter than expected"))?
            .to_vec()
    };

    require_sectioned(&header)?;

    let base = pos + 4 + headercompsize;
    let end = (pos + datalength).min(data.len());
    let blocks = find_blocks(&data, base, end);
    let entries = parse_opcodes(&header, &blocks, false)?;
    Ok((entries, Reader::NonSolid { data, base }))
}

/// Scan the block map (`findBlocksWithHandle:`, `:612-632`). Each block is
/// `[u32 len][ (len & 0x7fffffff) bytes ]`; the map keys are data offsets
/// relative to `base` and values keep the compressed-flag high bit. A final
/// lone `u32` with nothing after it is the trailing CRC and is not recorded.
fn find_blocks(data: &[u8], base: usize, end: usize) -> BTreeMap<u32, u32> {
    let mut map = BTreeMap::new();
    let end = end.min(data.len());
    let mut pos = base;
    let mut size: u32 = 0;
    while pos + 4 <= end {
        let blocklen = rd32(data, pos);
        pos += 4;
        if pos >= end {
            break; // hit the trailing CRC in a solid file
        }
        let reallen = (blocklen & 0x7fffffff) as usize;
        map.insert(size, blocklen);
        if pos + reallen > end {
            break; // truncated final block
        }
        pos += reallen;
        size = size.wrapping_add(reallen as u32 + 4);
    }
    map
}

/// `isSectionedHeader:` (`:753-770`): new-style headers have data (with zeroes)
/// after the string table, so a `00 00` pair appears in the last 32 bytes.
fn is_sectioned(header: &[u8]) -> bool {
    if header.len() < 32 {
        return false;
    }
    for i in (header.len() - 32)..=(header.len() - 2) {
        if header[i] == 0 && header[i + 1] == 0 {
            return true;
        }
    }
    false
}

/// Reject a pre-2.0 (non-sectioned) header, which this crate does not handle.
fn require_sectioned(header: &[u8]) -> io::Result<()> {
    if is_sectioned(header) {
        Ok(())
    } else {
        Err(unsupported(
            "nsis: pre-2.0 non-sectioned header is out of scope",
        ))
    }
}

/// `isUnicodeHeader:` (`:772-782`): an aligned `00 00` pair inside the string
/// table means UTF-16 strings.
fn is_unicode(header: &[u8], stringoffs: usize, stringendoffs: usize) -> bool {
    let mut i = stringoffs;
    while i + 2 <= stringendoffs && i + 2 <= header.len() {
        if header[i] == 0 && header[i + 1] == 0 {
            return true;
        }
        i += 2;
    }
    false
}

/// Walk the install-script opcodes (`parseOpcodesWithHeader:`, `:429-577`) for a
/// sectioned header: extract = 20, dir = 11, assign = 25, stride 7.
fn parse_opcodes(
    header: &[u8],
    blocks: &BTreeMap<u32, u32>,
    solid: bool,
) -> io::Result<Vec<NsisEntry>> {
    if header.len() < 40 {
        return Err(invalid("nsis: header shorter than sectioned fields"));
    }
    let entryoffs = rd32(header, 20) as usize;
    let entrynum = rd32(header, 24) as usize;
    let stringoffs = rd32(header, 28) as usize;
    let nextoffs = rd32(header, 36) as usize;
    let unicode = is_unicode(header, stringoffs, nextoffs);
    let string_span = nextoffs.saturating_sub(stringoffs);

    let mut current_dir: Vec<u8> = Vec::new();
    let mut outdir: Vec<u8> = Vec::new();
    let mut files: Vec<NsisEntry> = Vec::new();
    let mut dirs: Vec<NsisEntry> = Vec::new();

    let end = entryoffs.saturating_add(entrynum.saturating_mul(4 * 7));
    let mut i = entryoffs;
    while i < end && i + 28 <= header.len() {
        let opcode = rd32(header, i);
        let mut args = [0u32; 6];
        for (j, arg) in args.iter_mut().enumerate() {
            *arg = rd32(header, i + 4 + j * 4);
        }

        if opcode == 20 {
            // Extract file (ignoreOverwrite = YES, so overwrite is not checked).
            let filename = args[1] as usize;
            let dataoffset = args[2];
            if let Some(&len) = blocks.get(&dataoffset) {
                if filename < string_span {
                    let seg = paths::expand(
                        header,
                        stringoffs,
                        nextoffs,
                        filename,
                        unicode,
                        &current_dir,
                        &outdir,
                    );
                    let name = paths::join(&current_dir, &seg);
                    let filetime = if args[4] != 0xffffffff && args[3] != 0xffffffff {
                        Some((u64::from(args[4]) << 32) | u64::from(args[3]))
                    } else {
                        None
                    };
                    let comp = len & 0x7fffffff;
                    // A non-solid member's size is known up front only when its
                    // high bit is clear (stored). Solid members are slices of the
                    // decoded stream, so their length is always known too.
                    let stored_size = if !solid && (len & 0x80000000) != 0 {
                        None
                    } else {
                        Some(comp)
                    };
                    files.push(NsisEntry {
                        name,
                        is_dir: false,
                        data_offset: dataoffset,
                        compressed_size: comp,
                        stored_size,
                        filetime,
                    });
                    i += 28;
                    continue;
                }
            }
        }
        if opcode == 11 && args[1] == 1 && args[2] == 0 && args[3] == 0 && args[4] == 0 {
            // SetOutPath: sets the current directory and emits a directory entry.
            let dir = paths::expand(
                header,
                stringoffs,
                nextoffs,
                args[0] as usize,
                unicode,
                &current_dir,
                &outdir,
            );
            current_dir = dir.clone();
            dirs.push(NsisEntry {
                name: dir,
                is_dir: true,
                data_offset: args[2],
                compressed_size: 0,
                stored_size: Some(0),
                filetime: None,
            });
            i += 28;
            continue;
        }
        if opcode == 25 && (args[0] == 31 || args[0] == 29) {
            // Assign $OUTDIR.
            outdir = paths::expand(
                header,
                stringoffs,
                nextoffs,
                args[1] as usize,
                unicode,
                &current_dir,
                &outdir,
            );
            i += 28;
            continue;
        }
        i += 28;
    }

    // Sort and de-duplicate exactly as the reference (`:533-565`): files by data
    // offset, directories by name (descending), then drop consecutive dups.
    files.sort_by_key(|e| e.data_offset);
    dirs.sort_by(|a, b| b.name.cmp(&a.name));
    dirs.dedup_by(|a, b| a.name == b.name);
    files.dedup_by(|a, b| a.data_offset == b.data_offset && a.name == b.name);

    let mut entries = dirs;
    entries.extend(files);
    Ok(entries)
}

#[cfg(test)]
mod tests;

/// Decode a legacy NSIS headerless bzip2 stream with a caller-owned output limit.
/// The installer container/header and checksum remain the caller's responsibility.
pub fn decode_legacy_bzip2(data: &[u8], randomized: bool, max_output: usize) -> io::Result<Vec<u8>> {
    bzip2::decode_limited(data, randomized, max_output)
}
