// SPDX-FileCopyrightText: 2026 Aleksei Trankov and contributors
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Codec detection and decoding for NSIS payload streams.
//!
//! NSIS sniffs the compression method from the first bytes of the data area
//! (`XADNSISParser.m` `LooksLike*` and the format constants `:9-15,:51-69`).
//! We decode all four NSIS methods: **LZMA** (raw LZMA1, five property bytes, no
//! size field — via the mature `lzma-rs` crate), **NSIS-deflate** (via
//! `newtua-common::deflate::inflate_nsis`), the custom **NSIS-bzip2** (both the
//! NSIS2 and randomized NSIS1 variants, [`crate::bzip2`]) and **FilteredLZMA**
//! (a one-byte filter selector plus LZMA, with the x86 BCJ filter in
//! [`crate::bcj`]). The legacy zlib branch is out of scope and surfaces as
//! `Unsupported`, as does an unknown LZMA filter byte.

use std::io::{self, Cursor};

use newtua_common::deflate;

use crate::{bcj, bzip2, invalid, unsupported};

/// The compression method backing an NSIS archive's data area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// Raw LZMA1: five property bytes, an end-of-stream marker, no size field.
    Lzma,
    /// LZMA preceded by a one-byte filter selector; filter `1` is the x86 BCJ
    /// branch filter (`XADNSISParser.m:1190-1202`).
    FilteredLzma,
    /// The custom NSIS bzip2. `hasrand` marks the NSIS1 (v1.9x) randomized variant.
    NsisBzip2 { hasrand: bool },
    /// NSIS's modified deflate (`XADNSISDeflateVariant`).
    NsisDeflate,
}

/// `LooksLikeLZMA` (`XADNSISParser.m:51-54`): the raw-LZMA property bytes always
/// begin `5D 00 00 xx xx 00`.
fn looks_like_lzma(sig: &[u8]) -> bool {
    sig.len() >= 6 && sig[0] == 0x5d && sig[1] == 0 && sig[2] == 0 && sig[5] == 0
}

/// `LooksLikeFilteredLZMA` (`:56-59`): a leading filter byte (0 or 1), then LZMA.
fn looks_like_filtered_lzma(sig: &[u8]) -> bool {
    sig.len() >= 7 && (sig[0] == 0 || sig[0] == 1) && looks_like_lzma(&sig[1..])
}

/// `LooksLikeNSISBzip2` (`:61-64`): `'1'` then a 24-bit block size below 900000.
fn looks_like_nsis_bzip2(sig: &[u8]) -> bool {
    sig.len() >= 4
        && sig[0] == b'1'
        && ((u32::from(sig[1]) << 16) + (u32::from(sig[2]) << 8) + u32::from(sig[3])) < 900_000
}

/// `LooksLikeZlib` (`:66-69`): a zlib header with best-compression flags.
fn looks_like_zlib(sig: &[u8]) -> bool {
    sig.len() >= 2 && sig[0] == 0x78 && sig[1] == 0xda
}

/// Sniff the codec from the first bytes of a stream, mirroring the reference's
/// probe order. `Lzma` and `NsisDeflate` are decodable in 20a; the deferred and
/// out-of-scope methods return `Unsupported` rather than silently mis-decoding.
pub fn sniff(sig: &[u8]) -> io::Result<Codec> {
    if looks_like_lzma(sig) {
        Ok(Codec::Lzma)
    } else if looks_like_filtered_lzma(sig) {
        Ok(Codec::FilteredLzma)
    } else if looks_like_nsis_bzip2(sig) {
        // Auto-detection only recognises the NSIS2 (non-randomized) variant; the
        // randomized NSIS1 form is reached solely by the solid probe (`:1153`).
        Ok(Codec::NsisBzip2 { hasrand: false })
    } else if looks_like_zlib(sig) {
        Err(unsupported("nsis: legacy zlib format is out of scope"))
    } else {
        // No recognised signature: NSIS-deflate is the fallback (`:96,:329`).
        Ok(Codec::NsisDeflate)
    }
}

/// Decode a whole codec stream to the end (LZMA end-marker or deflate stream
/// end). `data` starts at the codec payload — for LZMA, the five property bytes.
pub fn decode(codec: Codec, data: &[u8]) -> io::Result<Vec<u8>> {
    match codec {
        Codec::Lzma => decode_lzma_raw(data),
        Codec::FilteredLzma => decode_filtered_lzma(data),
        Codec::NsisBzip2 { hasrand } => bzip2::decode(data, hasrand),
        Codec::NsisDeflate => deflate::inflate_nsis(data),
    }
}

/// Decode FilteredLZMA: a one-byte filter selector, then a raw LZMA stream whose
/// decoded output the filter is applied to (`XADNSISParser.m:1190-1202`). Filter
/// `0` is plain LZMA; filter `1` is the x86 BCJ branch filter (applied in one
/// pass, `ip = 0`); any other value is unsupported.
fn decode_filtered_lzma(data: &[u8]) -> io::Result<Vec<u8>> {
    let filter = *data
        .first()
        .ok_or_else(|| invalid("nsis: empty filtered-LZMA stream"))?;
    // Validate the filter before decoding (the reference decides the filter at
    // `:1190-1202` before building the handle): an unknown filter is rejected
    // without paying for a full LZMA decompression.
    if filter > 1 {
        return Err(unsupported(format!(
            "nsis: unsupported LZMA filter {filter}"
        )));
    }
    let mut out = decode_lzma_raw(&data[1..])?;
    if filter == 1 {
        let mut state = 0u32;
        bcj::x86_convert(&mut out, 0, &mut state, false);
    }
    Ok(out)
}

/// Decode raw LZMA1 as NSIS stores it: five property bytes, then the LZMA
/// stream terminated by an end-of-stream marker, with **no** eight-byte size
/// field (`XADLZMAHandle.m`, `XADNSISParser.m:1181-1188`).
///
/// `lzma-rs`'s `UnpackedSize::UseProvided(None)` reads exactly the five-byte
/// property header, skips the (absent) size field, and decodes until the end
/// marker — precisely NSIS's on-disk layout, so no synthetic 13-byte header is
/// needed.
fn decode_lzma_raw(data: &[u8]) -> io::Result<Vec<u8>> {
    let options = lzma_rs::decompress::Options {
        unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(None),
        ..Default::default()
    };
    let mut out = Vec::new();
    let mut input = Cursor::new(data);
    lzma_rs::lzma_decompress_with_options(&mut input, &mut out, &options)
        .map_err(|e| invalid(format!("nsis: lzma decode failed: {e}")))?;
    Ok(out)
}

#[cfg(test)]
pub(crate) mod test_support {
    //! LZMA encoder used by tests to build fixtures in NSIS's raw layout.
    use std::io::Cursor;

    /// Compress `data` to NSIS raw LZMA: five property bytes + stream + end
    /// marker, no size field — the inverse of [`super::decode_lzma_raw`].
    ///
    /// `lzma-rs` only writes an end-of-stream marker together with the eight-byte
    /// size field (`WriteToHeader(None)`); its `SkipWritingToHeader` mode omits
    /// both. NSIS wants the marker but *not* the size, so we compress with
    /// `WriteToHeader(None)` and then drop the size field (bytes 5..13), leaving
    /// exactly NSIS's on-disk layout.
    pub fn lzma_encode_raw(data: &[u8]) -> Vec<u8> {
        let options = lzma_rs::compress::Options {
            unpacked_size: lzma_rs::compress::UnpackedSize::WriteToHeader(None),
        };
        let mut out = Vec::new();
        let mut input = Cursor::new(data);
        lzma_rs::lzma_compress_with_options(&mut input, &mut out, &options).unwrap();
        out.drain(5..13); // remove the eight-byte uncompressed-size field
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::lzma_encode_raw;

    #[test]
    fn lzma_round_trips_raw_stream() {
        let data = b"raw LZMA1 with an end marker and no size field. ".repeat(30);
        let comp = lzma_encode_raw(&data);
        // A genuine raw stream starts with the 0x5D property byte.
        assert_eq!(comp[0], 0x5d);
        assert_eq!(decode(Codec::Lzma, &comp).unwrap(), data);
    }

    #[test]
    fn lzma_round_trips_empty_and_small() {
        for data in [b"".to_vec(), b"x".to_vec(), vec![0u8; 1000]] {
            let comp = lzma_encode_raw(&data);
            assert_eq!(decode(Codec::Lzma, &comp).unwrap(), data);
        }
    }

    #[test]
    fn nsis_deflate_round_trips() {
        let data = b"nsis deflate payload payload payload. ".repeat(20);
        let comp = deflate::deflate_dynamic(&data, &deflate::ZIP_ORDER);
        assert_eq!(decode(Codec::NsisDeflate, &comp).unwrap(), data);
    }

    #[test]
    fn sniff_detects_lzma() {
        assert_eq!(sniff(&[0x5d, 0, 0, 0x00, 0x10, 0, 0]).unwrap(), Codec::Lzma);
    }

    #[test]
    fn sniff_defaults_to_deflate() {
        // A deflate stream begins with arbitrary bits, not any known signature.
        assert_eq!(
            sniff(&[0x78, 0x01, 0, 0, 0, 0, 0]).unwrap(),
            Codec::NsisDeflate
        );
    }

    #[test]
    fn sniff_detects_filtered_lzma() {
        assert_eq!(
            sniff(&[0x00, 0x5d, 0, 0, 0x00, 0x10, 0]).unwrap(),
            Codec::FilteredLzma
        );
    }

    #[test]
    fn sniff_detects_nsis_bzip2() {
        // '1' followed by a 24-bit block size < 900000; auto-detect is always the
        // non-randomized NSIS2 variant.
        assert_eq!(
            sniff(&[b'1', 0x00, 0x00, 0x10, 0, 0, 0]).unwrap(),
            Codec::NsisBzip2 { hasrand: false }
        );
    }

    #[test]
    fn filtered_lzma_bcj_round_trips() {
        // Payload with x86 branches: BCJ-encode, LZMA-compress, prefix filter `1`.
        let mut payload: Vec<u8> = Vec::new();
        for i in 0..300u32 {
            if i % 5 == 0 {
                payload.push(0xE8);
                payload.extend_from_slice(&(i * 0x0101).to_le_bytes());
            } else {
                payload.push((i & 0xff) as u8);
            }
        }
        payload.extend_from_slice(&[0u8; 8]); // complete the last instruction
        let mut encoded = payload.clone();
        let mut s = 0u32;
        bcj::x86_convert(&mut encoded, 0, &mut s, true);
        let mut stream = vec![1u8];
        stream.extend_from_slice(&lzma_encode_raw(&encoded));
        assert_eq!(decode(Codec::FilteredLzma, &stream).unwrap(), payload);
    }

    #[test]
    fn filtered_lzma_filter_zero_is_plain_lzma() {
        let payload = b"filter 0 means the LZMA output passes through untouched".repeat(4);
        let mut stream = vec![0u8];
        stream.extend_from_slice(&lzma_encode_raw(&payload));
        assert_eq!(decode(Codec::FilteredLzma, &stream).unwrap(), payload);
    }

    #[test]
    fn filtered_lzma_unknown_filter_is_unsupported() {
        let mut stream = vec![2u8];
        stream.extend_from_slice(&lzma_encode_raw(b"x"));
        let e = decode(Codec::FilteredLzma, &stream).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn sniff_zlib_is_unsupported() {
        let e = sniff(&[0x78, 0xda, 0, 0, 0, 0, 0]).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
    }
}
