// SPDX-FileCopyrightText: 2026 Aleksei Trankov and contributors
// SPDX-License-Identifier: LGPL-3.0-or-later

//! 7-Zip x86 BCJ branch filter.
//!
//! A faithful port of `x86_Convert` from `lzma/Bra86.c` (Igor Pavlov, public
//! domain). NSIS's FilteredLZMA applies this filter to the LZMA-decoded output:
//! it rewrites the 32-bit operands of `E8`/`E9` (call/jmp) instructions between
//! absolute and relative form, which makes them more compressible. We only ever
//! decode, and — because `codec.rs` already has the whole decoded buffer in
//! memory — we run it in a single pass (`ip = 0`, `state = 0`), leaving any
//! trailing partial instruction (< 5 bytes) untouched, exactly like the reference.

/// `kMaskToAllowedStatus` (`Bra86.c:8`).
const MASK_TO_ALLOWED_STATUS: [bool; 8] = [true, true, true, false, true, false, false, false];
/// `kMaskToBitNumber` (`Bra86.c:9`).
const MASK_TO_BIT_NUMBER: [u32; 8] = [0, 1, 2, 2, 3, 3, 3, 3];

/// `Test86MSByte` (`Bra86.c:6`): a 0x00 or 0xFF high operand byte.
fn test86_msbyte(b: u8) -> bool {
    b == 0 || b == 0xFF
}

/// Apply the x86 BCJ filter to `data` in place. `encoding = false` decodes
/// (converts absolute call/jmp targets back to relative); `true` encodes.
/// `ip` is the base instruction pointer, `state` the carried prev-mask (both `0`
/// for NSIS's single-pass use). Returns the number of bytes processed; a partial
/// trailing instruction (< 5 bytes) is left untouched.
pub fn x86_convert(data: &mut [u8], ip: u32, state: &mut u32, encoding: bool) -> usize {
    let size = data.len();
    let mut buffer_pos: usize = 0;
    let mut prev_mask: u32 = *state & 0x7;
    if size < 5 {
        return 0;
    }
    let ip = ip.wrapping_add(5);
    // `prevPosT` is an unsigned `SizeT` seeded to `0 - 1`; the reference relies on
    // its modular wraparound, so we mirror it with `usize` wrapping arithmetic.
    let mut prev_pos_t: usize = 0usize.wrapping_sub(1);

    loop {
        // Scan for the next call/jmp opcode `(*p & 0xFE) == 0xE8`.
        let limit = size - 4;
        let mut p = buffer_pos;
        while p < limit {
            if data[p] & 0xFE == 0xE8 {
                break;
            }
            p += 1;
        }
        buffer_pos = p;
        if p >= limit {
            break;
        }

        prev_pos_t = buffer_pos.wrapping_sub(prev_pos_t);
        if prev_pos_t > 3 {
            prev_mask = 0;
        } else {
            prev_mask = (prev_mask << (prev_pos_t as u32 - 1)) & 0x7;
            if prev_mask != 0 {
                let b = data[buffer_pos + (4 - MASK_TO_BIT_NUMBER[prev_mask as usize] as usize)];
                if !MASK_TO_ALLOWED_STATUS[prev_mask as usize] || test86_msbyte(b) {
                    prev_pos_t = buffer_pos;
                    prev_mask = ((prev_mask << 1) & 0x7) | 1;
                    buffer_pos += 1;
                    continue;
                }
            }
        }
        prev_pos_t = buffer_pos;

        if test86_msbyte(data[buffer_pos + 4]) {
            let mut src = (u32::from(data[buffer_pos + 4]) << 24)
                | (u32::from(data[buffer_pos + 3]) << 16)
                | (u32::from(data[buffer_pos + 2]) << 8)
                | u32::from(data[buffer_pos + 1]);
            let dest = loop {
                let dest = if encoding {
                    ip.wrapping_add(buffer_pos as u32).wrapping_add(src)
                } else {
                    src.wrapping_sub(ip.wrapping_add(buffer_pos as u32))
                };
                if prev_mask == 0 {
                    break dest;
                }
                let index = MASK_TO_BIT_NUMBER[prev_mask as usize] * 8;
                let b = (dest >> (24 - index)) as u8;
                if !test86_msbyte(b) {
                    break dest;
                }
                // `1 << (32 - index)` is `1 << 32` when index is 0, which C leaves
                // as `1` on x86 (shift count masked to 5 bits); `wrapping_shl`
                // reproduces that.
                src = dest ^ (1u32.wrapping_shl(32 - index).wrapping_sub(1));
            };
            data[buffer_pos + 4] = !(((dest >> 24) & 1).wrapping_sub(1)) as u8;
            data[buffer_pos + 3] = (dest >> 16) as u8;
            data[buffer_pos + 2] = (dest >> 8) as u8;
            data[buffer_pos + 1] = dest as u8;
            buffer_pos += 5;
        } else {
            prev_mask = ((prev_mask << 1) & 0x7) | 1;
            buffer_pos += 1;
        }
    }

    prev_pos_t = buffer_pos.wrapping_sub(prev_pos_t);
    *state = if prev_pos_t > 3 {
        0
    } else {
        (prev_mask << (prev_pos_t as u32 - 1)) & 0x7
    };
    buffer_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_buffer_is_untouched() {
        let mut buf = [0xE8, 0x00, 0x00, 0x00]; // 4 bytes < 5
        let orig = buf;
        let mut state = 0;
        assert_eq!(x86_convert(&mut buf, 0, &mut state, false), 0);
        assert_eq!(buf, orig);
    }

    #[test]
    fn data_without_call_jmp_is_unchanged() {
        let mut buf = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a];
        let orig = buf.clone();
        let mut state = 0;
        x86_convert(&mut buf, 0, &mut state, false);
        assert_eq!(buf, orig);
    }

    /// Encode then decode must recover the original for full (non-tail) buffers.
    fn round_trip(bytes: &[u8]) {
        let mut enc = bytes.to_vec();
        let mut s = 0;
        x86_convert(&mut enc, 0, &mut s, true);
        let mut dec = enc.clone();
        let mut s2 = 0;
        x86_convert(&mut dec, 0, &mut s2, false);
        assert_eq!(dec, bytes);
    }

    #[test]
    fn round_trips_x86_like_data() {
        // A sprinkling of E8/E9 branches with absolute-looking operands.
        let mut data = Vec::new();
        for i in 0..200u32 {
            if i % 7 == 0 {
                data.push(0xE8);
                data.extend_from_slice(&(i * 0x0101).to_le_bytes());
            } else if i % 11 == 0 {
                data.push(0xE9);
                data.extend_from_slice(&0x00u32.to_le_bytes());
            } else {
                data.push((i & 0xff) as u8);
            }
        }
        // Pad so the last instruction is complete.
        data.extend_from_slice(&[0u8; 8]);
        round_trip(&data);
    }

    #[test]
    fn round_trips_all_call_bytes() {
        // Adjacent E8 bytes stress the prev-mask state machine.
        let mut data = vec![0xE8; 64];
        data.extend_from_slice(&[0u8; 8]);
        round_trip(&data);
    }
}
