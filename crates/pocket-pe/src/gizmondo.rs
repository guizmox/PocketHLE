//! Native Gizmondo executable decoding, independent of the host OS.
//! Format recovered from the supplied giz_exe_dec utility (2023-09-15).
//! This decodes the executable envelope; it does not emulate SD-card identity.
use std::io::{self, ErrorKind};

mod tables {
    include!("gizmondo_tables.rs");
}
use tables::{HEADERS, KEYS, SBOX};

/// Recognition only: importing a card never rewrites or decrypts its files.
pub fn is_encrypted(bytes: &[u8]) -> bool {
    bytes
        .get(..8)
        .is_some_and(|h| HEADERS.iter().any(|v| v.as_slice() == h))
}

fn round(value: u32) -> u32 {
    let b = value.to_be_bytes();
    let s = SBOX[b[3] as usize];
    let mixed = u32::from_be_bytes([
        (s ^ 0xc9).rotate_left(2) ^ b[0],
        (s ^ 0x2b).rotate_right(3) ^ b[1],
        (s ^ 0x65).rotate_left(1) ^ b[2],
        s,
    ]);
    mixed ^ mixed.rotate_left(9) ^ mixed.rotate_right(10)
}

fn schedule(key: [u8; 7], count: usize) -> [u32; 10] {
    let mut low = u32::from_be_bytes([0, key[0], key[1], key[2]]);
    let mut high = u32::from_be_bytes(key[3..].try_into().unwrap());
    let mut result = [0; 10];
    for (i, word) in result.iter_mut().enumerate().take(count) {
        low &= 0xffffff;
        *word = high.wrapping_add(u32::from(SBOX[((low as u8) ^ i as u8) as usize]) << 4);
        (high, low) = ((high << 17) | (low >> 7), (low << 17) | (high >> 15));
    }
    result
}

/// Known encrypted headers are decoded; ordinary/unknown files are untouched.
/// A recognised but corrupt envelope fails loading instead of mapping bad code.
/// Work on a copy so an error never leaves a partially decoded caller buffer.
pub fn decode(bytes: &mut Vec<u8>) -> io::Result<bool> {
    let Some(header) = bytes.get(..8) else {
        return Ok(false);
    };
    let Some(index) = HEADERS.iter().position(|h| h.as_slice() == header) else {
        return Ok(false);
    };
    if bytes.len() % 8 != 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Truncated encrypted Gizmondo executable",
        ));
    }
    let mut decoded = bytes.clone();
    decode_records(&mut decoded, KEYS[index]);
    let pe_offset = decoded
        .get(60..64)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    if !decoded.starts_with(b"MZ")
        || !pe_offset.is_some_and(|o| decoded.get(o..o.saturating_add(4)) == Some(b"PE\0\0"))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Gizmondo decryption did not produce a valid PE header",
        ));
    }
    *bytes = decoded;
    Ok(true)
}

fn decode_records(decoded: &mut [u8], original: [u8; 7]) {
    // The device format restarts each 1 KiB record. The first block has ten
    // subkeys; subsequent blocks use two, repeated across the ten rounds.
    // Round five supplies the following block's seven-byte feedback key.
    for record in decoded.chunks_mut(1024) {
        let mut key = original;
        for (block_index, block) in record.chunks_exact_mut(8).enumerate() {
            let count = if block_index == 0 { 10 } else { 2 };
            let keys = schedule(key, count);
            let mut left = u32::from_be_bytes(block[..4].try_into().unwrap());
            let mut right = u32::from_be_bytes(block[4..].try_into().unwrap());
            for i in (0..10).rev() {
                if i & 1 != 0 {
                    left = left.wrapping_sub(round(right.wrapping_add(keys[i % count])));
                } else {
                    right = right.wrapping_sub(round(left.wrapping_add(keys[i % count])));
                }
                if i == 5 {
                    let l = left.to_be_bytes();
                    let r = right.to_be_bytes();
                    let state = [l[0], l[1], l[2], l[3], r[0], r[1], r[2], r[3]];
                    for j in 0..7 {
                        key[j] = original[j] ^ state[j + 1];
                    }
                }
            }
            block[..4].copy_from_slice(&right.to_be_bytes());
            block[4..].copy_from_slice(&left.to_be_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_utility_vectors_cover_feedback_and_record_restarts() {
        // Golden outputs from the supplied x86 routine, not a roundtrip
        // through a second implementation of the same Rust algorithm.
        let vectors: &[(usize, u64)] = &[
            (8, 0x8a11c55a6ecf04d8),
            (16, 0x02df40f3a9fc0044),
            (24, 0xc8da7dddfa110dc4),
            (64, 0xe130aa6feda81a68),
            (1024, 0x0b68283e9f8f7454),
            (1032, 0x56f0134b69dc8139),
            (2056, 0x1f54e6ca8b4d2ff4),
        ];
        for &(length, expected) in vectors {
            let mut bytes: Vec<u8> = (0..length).map(|i| (i * 37 + 11) as u8).collect();
            decode_records(&mut bytes, [0, 1, 2, 3, 4, 5, 6]);
            let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(hash, expected, "length {length}");
        }
    }
    #[test]
    fn every_known_header_resolves_to_the_dos_header() {
        for i in 0..HEADERS.len() {
            let mut bytes = HEADERS[i];
            decode_records(&mut bytes, KEYS[i]);
            assert_eq!(bytes, *b"MZ\x90\0\x03\0\0\0", "key {i}");
        }
    }
    #[test]
    fn ordinary_images_untouched_and_corrupt_images_fail_atomically() {
        for original in [b"".to_vec(), b"MZ ordinary file".to_vec(), vec![0x44; 64]] {
            let mut bytes = original.clone();
            assert!(!decode(&mut bytes).unwrap());
            assert_eq!(bytes, original);
        }
        for length in [9, 64] {
            let mut bytes = vec![0x55; length];
            bytes[..8].copy_from_slice(&HEADERS[52]);
            let original = bytes.clone();
            assert!(decode(&mut bytes).is_err());
            assert_eq!(bytes, original);
        }
    }
}
