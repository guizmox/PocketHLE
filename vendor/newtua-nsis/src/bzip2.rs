// SPDX-FileCopyrightText: 2026 Aleksei Trankov and contributors
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Custom NSIS bzip2 decoder.
//!
//! A faithful port of `XADNSISBzip2Handle.m` — Rob Landley's micro-bunzip
//! (LGPL-2.1), as NSIS ships it. It is standard bzip2 block decoding with three
//! NSIS-specific framing changes, all reproduced exactly:
//!
//! * **No stream header.** There is no `BZh<n>` prefix; the dictionary/block
//!   size is hard-wired to 900000 (`XADNSISBzip2Handle.m:459`).
//! * **1-byte block markers.** Before each block a single byte is read: `0x31`
//!   introduces a block, `0x17` ends the stream (`:106-108`); the original 48-bit
//!   magics are gone.
//! * **No CRCs** — neither per-block nor per-stream — are present or checked.
//! * **Randomization bit** (NSIS1 / v1.9x, `hasrand`): a single bit after the
//!   marker is read and **discarded**; the reference performs no de-randomization
//!   of the BWT, and neither do we (`:109`).
//!
//! The MSB-first bit reader mirrors the reference `get_bits`, including its
//! "read N bits then push a few back" trick (`inbufBitCount++`, `:164`/`:271`).
//! The shared `newtua-common::BitReaderMsb` has no bit-pushback, so this reader
//! is local (see the report's note to the planner).

use std::io;

/// Number of Huffman coding groups a block may use.
const MAX_GROUPS: usize = 6;
/// Symbols decoded before the next group selector applies.
const GROUP_SIZE: i32 = 50;
/// Longest Huffman code permitted.
const MAX_HUFCODE_BITS: usize = 20;
/// 256 literals + RUNA + RUNB.
const MAX_SYMBOLS: usize = 258;
/// The highest run symbol (RUNA is 0, RUNB is 1); values above are literals.
const SYMBOL_RUNB: i32 = 1;
/// Hard-wired BWT buffer size (`dbufSize`, `:459`).
const DBUF_SIZE: usize = 900_000;
/// `nSelectors` is a 15-bit field, so at most 32768 selectors.
const MAX_SELECTORS: usize = 32768;

fn data_error(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("nsis bzip2: {msg}"))
}

/// MSB-first bit reader over an in-memory stream, with the reference's bit
/// "un-get" (`inbufBitCount += k`). A 64-bit accumulator makes the reference's
/// 32-bit overflow dump (`:66-72`) unnecessary while yielding identical bits —
/// that dump only triggers for reads wider than 24 bits, which never occur here.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    nbits: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            acc: 0,
            nbits: 0,
        }
    }

    /// Read `n` bits (`n` ≤ 24), most-significant first (`get_bits`, `:53-82`).
    fn read(&mut self, n: u32) -> io::Result<u32> {
        while self.nbits < n {
            let b = *self
                .data
                .get(self.pos)
                .ok_or_else(|| data_error("unexpected end of input"))?;
            self.pos += 1;
            self.acc = (self.acc << 8) | u64::from(b);
            self.nbits += 8;
        }
        self.nbits -= n;
        let mask = (1u32 << n) - 1;
        Ok(((self.acc >> self.nbits) as u32) & mask)
    }

    /// Push `n` just-read bits back onto the stream (the `inbufBitCount++` trick).
    fn unget(&mut self, n: u32) {
        self.nbits += n;
    }
}

/// One Huffman group's decode tables. `limit`/`base` are 1-based to match the
/// reference's `base-1`/`limit-1` pointer shift; index 0 is unused.
#[derive(Clone, Copy)]
struct GroupData {
    limit: [i32; MAX_HUFCODE_BITS + 2],
    base: [i32; MAX_HUFCODE_BITS + 2],
    permute: [i32; MAX_SYMBOLS],
    min_len: i32,
    max_len: i32,
}

const GROUP_DEFAULT: GroupData = GroupData {
    limit: [0; MAX_HUFCODE_BITS + 2],
    base: [0; MAX_HUFCODE_BITS + 2],
    permute: [0; MAX_SYMBOLS],
    min_len: 0,
    max_len: 0,
};

/// Persistent decode state (the parts of `bunzip_data` we need).
struct Bunzip<'a> {
    reader: BitReader<'a>,
    dbuf: Vec<u32>,
    hasrand: bool,
    selectors: Vec<u8>,
    groups: [GroupData; MAX_GROUPS],
    // RLE-of-4 output state, carried across `produce` calls.
    write_copies: i32,
    write_pos: i32,
    write_run_countdown: i32,
    write_count: i32,
    write_current: i32,
}

impl<'a> Bunzip<'a> {
    fn new(data: &'a [u8], hasrand: bool) -> Self {
        Self {
            reader: BitReader::new(data),
            dbuf: vec![0u32; DBUF_SIZE],
            hasrand,
            selectors: vec![0u8; MAX_SELECTORS],
            groups: [GROUP_DEFAULT; MAX_GROUPS],
            write_copies: 0,
            write_pos: 0,
            write_run_countdown: 0,
            write_count: 0,
            write_current: 0,
        }
    }

    /// Decode the next block into `dbuf` and set up the inverse BWT
    /// (`get_next_block`, `:86-368`). Returns `Ok(None)` at the last-block marker.
    fn get_next_block(&mut self) -> io::Result<Option<()>> {
        // 1-byte block marker (`:106-108`).
        let marker = self.reader.read(8)?;
        if marker == 0x17 {
            return Ok(None);
        }
        if marker != 0x31 {
            return Err(data_error("bad block marker"));
        }
        // Randomization bit (NSIS1): read and discard, no de-randomization (`:109`).
        if self.hasrand {
            self.reader.read(1)?;
        }
        let orig_ptr = self.reader.read(24)? as usize;
        if orig_ptr > DBUF_SIZE {
            return Err(data_error("origPtr out of range"));
        }

        // Sparse symbol map (`:116-124`).
        let mut sym_to_byte = [0u8; 256];
        let present = self.reader.read(16)?;
        let mut sym_total = 0usize;
        for i in 0..16 {
            if present & (1 << (15 - i)) != 0 {
                let k = self.reader.read(16)?;
                for j in 0..16 {
                    if k & (1 << (15 - j)) != 0 {
                        sym_to_byte[sym_total] = (16 * i + j) as u8;
                        sym_total += 1;
                    }
                }
            }
        }

        let group_count = self.reader.read(3)? as usize;
        if !(2..=MAX_GROUPS).contains(&group_count) {
            return Err(data_error("bad group count"));
        }

        // Selector list, MTF-encoded (`:132-141`).
        let n_selectors = self.reader.read(15)? as usize;
        if n_selectors == 0 {
            return Err(data_error("no selectors"));
        }
        let mut mtf_symbol = [0u8; 256];
        for (i, m) in mtf_symbol.iter_mut().take(group_count).enumerate() {
            *m = i as u8;
        }
        for i in 0..n_selectors {
            let mut j = 0usize;
            while self.reader.read(1)? != 0 {
                j += 1;
                if j >= group_count {
                    return Err(data_error("selector out of range"));
                }
            }
            let uc = mtf_symbol[j];
            while j > 0 {
                mtf_symbol[j] = mtf_symbol[j - 1];
                j -= 1;
            }
            mtf_symbol[0] = uc;
            self.selectors[i] = uc;
        }

        // Huffman tables per group (`:145-226`).
        let sym_count = sym_total + 2;
        for g in 0..group_count {
            let mut length = [0u8; MAX_SYMBOLS];
            let mut t: i32 = self.reader.read(5)? as i32 - 1;
            for slot in length.iter_mut().take(sym_count) {
                loop {
                    if (t as u32) > (MAX_HUFCODE_BITS as u32 - 1) {
                        return Err(data_error("bad code length"));
                    }
                    let k = self.reader.read(2)?;
                    if k < 2 {
                        self.reader.unget(1);
                        break;
                    }
                    t += (((k + 1) & 2) as i32) - 1;
                }
                *slot = (t + 1) as u8;
            }

            let mut min_len = length[0] as i32;
            let mut max_len = length[0] as i32;
            for &l in length.iter().take(sym_count).skip(1) {
                let l = l as i32;
                if l > max_len {
                    max_len = l;
                } else if l < min_len {
                    min_len = l;
                }
            }

            let hg = &mut self.groups[g];
            hg.min_len = min_len;
            hg.max_len = max_len;

            // permute[], and clear limit[] over [minLen, maxLen] (`:197-203`); the
            // group is reused across blocks, so its old limits must be cleared.
            // `temp` is already zero-initialized (the reference clears it here only
            // because C leaves it uninitialized on the stack).
            let mut temp = [0i32; MAX_HUFCODE_BITS + 2];
            hg.limit[min_len as usize..=max_len as usize].fill(0);
            let mut pp = 0usize;
            for i in min_len..=max_len {
                for (t_idx, &l) in length.iter().take(sym_count).enumerate() {
                    if l as i32 == i {
                        hg.permute[pp] = t_idx as i32;
                        pp += 1;
                    }
                }
            }
            for &l in length.iter().take(sym_count) {
                temp[l as usize] += 1;
            }
            // limit[] and base[] (`:210-225`).
            let mut pp2: i32 = 0;
            let mut tacc: i32 = 0;
            let mut i = min_len;
            while i < max_len {
                pp2 += temp[i as usize];
                hg.limit[i as usize] = (pp2 << (max_len - i)) - 1;
                pp2 <<= 1;
                tacc += temp[i as usize];
                hg.base[(i + 1) as usize] = pp2 - tacc;
                i += 1;
            }
            hg.limit[(max_len + 1) as usize] = i32::MAX;
            hg.limit[max_len as usize] = pp2 + temp[max_len as usize] - 1;
            hg.base[min_len as usize] = 0;
        }

        // Decode the block's symbols, undoing Huffman + RLE1 into dbuf (`:231-335`).
        let mut byte_count = [0i32; 256];
        for (i, m) in mtf_symbol.iter_mut().enumerate() {
            *m = i as u8;
        }
        let mut run_pos: i32 = 0;
        let mut dbuf_count: usize = 0;
        let mut group_pos: i32 = 0;
        let mut selector: usize = 0;
        let mut t_run: i32 = 0;
        let mut cur_group: usize = 0;

        loop {
            // Pick the Huffman group for this run of GROUP_SIZE symbols (`:239-246`).
            if group_pos == 0 {
                group_pos = GROUP_SIZE - 1;
                if selector >= n_selectors {
                    return Err(data_error("selector overrun"));
                }
                cur_group = self.selectors[selector] as usize;
                selector += 1;
            } else {
                group_pos -= 1;
            }

            let (min_len, max_len) = {
                let hg = &self.groups[cur_group];
                (hg.min_len, hg.max_len)
            };
            // Read maxLen bits, then push the surplus back (`:257-271`).
            let mut j = self.reader.read(max_len as u32)? as i32;
            let mut i = min_len;
            while j > self.groups[cur_group].limit[i as usize] {
                i += 1;
            }
            self.reader.unget((max_len - i) as u32);
            if i > max_len {
                return Err(data_error("symbol too long"));
            }
            j = (j >> (max_len - i)) - self.groups[cur_group].base[i as usize];
            if (j as u32) >= MAX_SYMBOLS as u32 {
                return Err(data_error("symbol out of range"));
            }
            let next_sym = self.groups[cur_group].permute[j as usize];

            // RUNA/RUNB accumulate a run length in bijective base-2 (`:282-298`).
            if (next_sym as u32) <= SYMBOL_RUNB as u32 {
                if run_pos == 0 {
                    run_pos = 1;
                    t_run = 0;
                }
                t_run += run_pos << next_sym;
                run_pos <<= 1;
                continue;
            }
            // First non-run symbol flushes the pending run (`:303-310`).
            if run_pos != 0 {
                run_pos = 0;
                if dbuf_count + (t_run as usize) >= DBUF_SIZE {
                    return Err(data_error("run overflows buffer"));
                }
                let uc = sym_to_byte[mtf_symbol[0] as usize];
                byte_count[uc as usize] += t_run;
                for _ in 0..t_run {
                    self.dbuf[dbuf_count] = u32::from(uc);
                    dbuf_count += 1;
                }
            }
            // Terminating symbol (`:312`).
            if next_sym > sym_total as i32 {
                break;
            }
            if dbuf_count >= DBUF_SIZE {
                return Err(data_error("too many symbols"));
            }
            // Literal via MTF (`:321-334`).
            let mut idx = (next_sym - 1) as usize;
            let uc0 = mtf_symbol[idx];
            while idx > 0 {
                mtf_symbol[idx] = mtf_symbol[idx - 1];
                idx -= 1;
            }
            mtf_symbol[0] = uc0;
            let uc = sym_to_byte[uc0 as usize];
            byte_count[uc as usize] += 1;
            self.dbuf[dbuf_count] = u32::from(uc);
            dbuf_count += 1;
        }

        // Inverse Burrows-Wheeler setup (`:342-365`).
        let mut acc = 0i32;
        for bc in byte_count.iter_mut() {
            let k = acc + *bc;
            *bc = acc;
            acc = k;
        }
        for i in 0..dbuf_count {
            let uc = (self.dbuf[i] & 0xff) as usize;
            let slot = byte_count[uc] as usize;
            self.dbuf[slot] |= (i as u32) << 8;
            byte_count[uc] += 1;
        }
        if dbuf_count != 0 {
            if orig_ptr >= dbuf_count {
                return Err(data_error("origPtr past block"));
            }
            self.write_pos = self.dbuf[orig_ptr] as i32;
            self.write_current = self.write_pos & 0xff;
            self.write_pos >>= 8;
            self.write_run_countdown = 5;
        }
        self.write_count = dbuf_count as i32;
        Ok(Some(()))
    }

    /// Produce up to `outbuf.len()` output bytes, undoing the BWT and the RLE-of-4
    /// output coding (`read_bunzip`, `:377-448`). Returns the byte count, or a
    /// negative sentinel once the stream is exhausted.
    fn produce(&mut self, outbuf: &mut [u8]) -> io::Result<i32> {
        let len = outbuf.len() as i32;
        if self.write_count < 0 {
            return Ok(self.write_count);
        }
        let mut gotcount: i32 = 0;
        let mut pos: i32;
        let mut current: i32;
        let mut goto_decode: bool;

        if self.write_copies != 0 {
            self.write_copies -= 1;
            pos = self.write_pos;
            current = self.write_current;
            goto_decode = false;
        } else {
            // First call, or the previous block just ended: decode a fresh block.
            if self.get_next_block()?.is_none() {
                self.write_count = -1; // RETVAL_LAST_BLOCK
                return Ok(gotcount);
            }
            pos = self.write_pos;
            current = self.write_current;
            goto_decode = true;
        }

        loop {
            if !goto_decode {
                if gotcount >= len {
                    self.write_pos = pos;
                    self.write_current = current;
                    self.write_copies += 1;
                    return Ok(len);
                }
                outbuf[gotcount as usize] = current as u8;
                gotcount += 1;
                if self.write_copies != 0 {
                    self.write_copies -= 1;
                    continue;
                }
            }
            goto_decode = false;

            // decode_next_byte: follow the BWT vector, applying RLE-of-4.
            loop {
                let wc = self.write_count;
                self.write_count -= 1;
                if wc == 0 {
                    if self.get_next_block()?.is_none() {
                        self.write_count = -1;
                        return Ok(gotcount);
                    }
                    pos = self.write_pos;
                    current = self.write_current;
                    continue;
                }
                let previous = current;
                pos = self.dbuf[pos as usize] as i32;
                current = pos & 0xff;
                pos >>= 8;
                self.write_run_countdown -= 1;
                if self.write_run_countdown != 0 {
                    if current != previous {
                        self.write_run_countdown = 4;
                    }
                    break;
                } else {
                    self.write_copies = current;
                    current = previous;
                    self.write_run_countdown = 5;
                    if self.write_copies == 0 {
                        continue;
                    }
                    self.write_copies -= 1;
                    break;
                }
            }
        }
    }
}

/// Decode a complete NSIS-bzip2 stream. `hasrand` selects the v1.9x variant
/// (NSIS1) that carries a per-block randomization bit; `false` is NSIS2 (2.0+).
pub fn decode(data: &[u8], hasrand: bool) -> io::Result<Vec<u8>> {
    decode_limited(data, hasrand, usize::MAX)
}

pub fn decode_limited(data: &[u8], hasrand: bool, max_output: usize) -> io::Result<Vec<u8>> {
    let mut bd = Bunzip::new(data, hasrand);
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = bd.produce(&mut buf)?;
        if n < 0 {
            break; // stream exhausted (RETVAL_LAST_BLOCK is sticky)
        }
        if n as usize > max_output.saturating_sub(out.len()) {
            return Err(data_error("decoded output exceeds caller limit"));
        }
        out.extend_from_slice(&buf[..n as usize]);
        if (n as usize) < buf.len() {
            break; // a short read means end of stream
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real single-block NSIS2 bzip2 stream, extracted from a `makensis`
    /// installer (`SetCompressor bzip2`); it decodes to `"abcdefgh"` × 400.
    const NSIS2_BLOCK: &[u8] = &[
        0x31, 0x00, 0x01, 0x8f, 0x02, 0x00, 0x7f, 0x80, 0x40, 0x00, 0xa1, 0x0c, 0x08, 0x0a, 0x54,
        0x01, 0xe0, 0x8b, 0x01, 0x16, 0x42, 0x2d, 0x04, 0x5b, 0x08, 0xb8, 0x11, 0x74, 0x22, 0xf0,
        0x45, 0xf1, 0x70,
    ];

    #[test]
    fn decodes_nsis2_block() {
        let out = decode(NSIS2_BLOCK, false).unwrap();
        assert_eq!(out, b"abcdefgh".repeat(400));
    }

    /// Build the NSIS1 form of an NSIS2 stream: the sole difference is a single
    /// `0` randomization bit inserted right after the 8-bit block marker.
    fn insert_randomization_bit(nsis2: &[u8]) -> Vec<u8> {
        let mut bits: Vec<bool> = Vec::with_capacity(nsis2.len() * 8 + 1);
        for &b in nsis2 {
            for i in (0..8).rev() {
                bits.push(b >> i & 1 != 0);
            }
        }
        bits.insert(8, false); // the randomization bit, after the marker byte
        let mut out = vec![0u8; bits.len().div_ceil(8)];
        for (i, &bit) in bits.iter().enumerate() {
            if bit {
                out[i / 8] |= 1 << (7 - i % 8);
            }
        }
        out
    }

    #[test]
    fn randomization_bit_is_swallowed() {
        // Decoding the NSIS1 stream with `hasrand` must match the NSIS2 stream:
        // the extra bit is read and discarded, with no de-randomization.
        let expected = decode(NSIS2_BLOCK, false).unwrap();
        let nsis1 = insert_randomization_bit(NSIS2_BLOCK);
        assert_eq!(decode(&nsis1, true).unwrap(), expected);
    }

    #[test]
    fn bad_marker_is_rejected() {
        // A stream that does not start with 0x31 or 0x17 is a data error.
        let err = decode(&[0x99, 0, 0, 0], false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn truncated_stream_is_rejected() {
        // The marker promises a block, but the input ends immediately.
        assert!(decode(&[0x31], false).is_err());
    }
}
