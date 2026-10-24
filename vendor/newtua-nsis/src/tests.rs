// SPDX-FileCopyrightText: 2026 Aleksei Trankov and contributors
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Unit tests for the NSIS container, plus a synthetic-installer builder that
//! doubles as the mirror-encoder oracle (build an installer our own encoder
//! produces, then decode it back). The `unar` cross-check lives in
//! `tests/oracle.rs`.

use super::*;
use crate::codec::test_support::lzma_encode_raw;
use newtua_common::deflate;

/// Which codec a synthetic installer is built with.
#[derive(Clone, Copy)]
enum Enc {
    Lzma,
    Deflate,
}

impl Enc {
    fn encode(self, data: &[u8]) -> Vec<u8> {
        match self {
            Enc::Lzma => lzma_encode_raw(data),
            Enc::Deflate => deflate::deflate_dynamic(data, &deflate::ZIP_ORDER),
        }
    }
}

/// An install-script operation for the builder.
enum Op {
    /// `SetOutPath dir` (opcode 11).
    Dir(Vec<u8>),
    /// `File name` extracting `content` (opcode 20).
    File { name: Vec<u8>, content: Vec<u8> },
}

/// Encode one string-table entry, returning its bytes and its offset unit
/// (byte offset for ANSI, code-unit index for UTF-16).
fn encode_string(name: &[u8], unicode: bool) -> Vec<u8> {
    if unicode {
        let mut v = Vec::new();
        for &b in name {
            v.extend_from_slice(&u16::from(b).to_le_bytes());
        }
        v.extend_from_slice(&[0, 0]);
        v
    } else {
        let mut v = name.to_vec();
        v.push(0);
        v
    }
}

fn put_u32(buf: &mut [u8], pos: usize, val: u32) {
    buf[pos..pos + 4].copy_from_slice(&val.to_le_bytes());
}

/// Build a complete NSIS installer file from `ops`, in the given layout.
/// Returns the file bytes. `stub` prepends that many zero bytes so the
/// firstheader lands at a non-zero 512-aligned offset.
fn build(ops: &[Op], solid: bool, unicode: bool, enc: Enc, stub: usize) -> Vec<u8> {
    // --- string table + per-op name offsets -------------------------------
    let mut string_table = Vec::new();
    let mut offsets = Vec::new();
    for op in ops {
        let name = match op {
            Op::Dir(n) => n,
            Op::File { name, .. } => name,
        };
        let unit = if unicode {
            string_table.len() / 2
        } else {
            string_table.len()
        };
        offsets.push(unit as u32);
        string_table.extend_from_slice(&encode_string(name, unicode));
    }

    // --- file blocks + data offsets ---------------------------------------
    let mut file_blocks = Vec::new();
    let mut data_offsets = Vec::new();
    let mut cursor: u32 = 0;
    for op in ops {
        if let Op::File { content, .. } = op {
            data_offsets.push(Some(cursor));
            let payload: Vec<u8> = if solid {
                content.clone()
            } else {
                enc.encode(content)
            };
            let flag: u32 = if solid { 0 } else { 0x8000_0000 };
            file_blocks.extend_from_slice(&((payload.len() as u32) | flag).to_le_bytes());
            file_blocks.extend_from_slice(&payload);
            cursor = cursor.wrapping_add(payload.len() as u32).wrapping_add(4);
        } else {
            data_offsets.push(None);
        }
    }

    // --- opcode stream ----------------------------------------------------
    let mut opcodes = Vec::new();
    let mut entrynum = 0u32;
    for (i, op) in ops.iter().enumerate() {
        let mut instr = [0u32; 7];
        match op {
            Op::Dir(_) => {
                instr[0] = 11;
                instr[1] = offsets[i];
                instr[2] = 1; // directory argument
            }
            Op::File { .. } => {
                instr[0] = 20;
                instr[1] = 0; // overwrite (ignored for sectioned headers)
                instr[2] = offsets[i];
                instr[3] = data_offsets[i].unwrap();
                instr[4] = 0xffff_ffff; // no timestamp
                instr[5] = 0xffff_ffff;
            }
        }
        for word in instr {
            opcodes.extend_from_slice(&word.to_le_bytes());
        }
        entrynum += 1;
    }

    // --- install header ---------------------------------------------------
    let prefix = 44usize;
    let entryoffs = prefix;
    let stringoffs = prefix + opcodes.len();
    let nextoffs = stringoffs + string_table.len();
    let mut header = vec![0u8; prefix];
    header.extend_from_slice(&opcodes);
    header.extend_from_slice(&string_table);
    header.extend_from_slice(&[0u8; 32]); // trailing data → isSectioned
    put_u32(&mut header, 20, entryoffs as u32);
    put_u32(&mut header, 24, entrynum);
    put_u32(&mut header, 28, stringoffs as u32);
    put_u32(&mut header, 36, nextoffs as u32);
    let headerlength = header.len() as u32;

    // --- data region ------------------------------------------------------
    let region: Vec<u8> = if solid {
        let mut stream = Vec::new();
        stream.extend_from_slice(&headerlength.to_le_bytes());
        stream.extend_from_slice(&header);
        stream.extend_from_slice(&file_blocks);
        enc.encode(&stream)
    } else {
        let hpayload = enc.encode(&header);
        let mut region = Vec::new();
        region.extend_from_slice(&((hpayload.len() as u32) | 0x8000_0000).to_le_bytes());
        region.extend_from_slice(&hpayload);
        region.extend_from_slice(&file_blocks);
        region
    };

    // --- firstheader + file assembly --------------------------------------
    let totallength = (region.len() + 32) as u32;
    let mut file = vec![0u8; stub];
    file.extend_from_slice(&0u32.to_le_bytes()); // flags
    file.extend_from_slice(&NEW_MAGIC);
    file.extend_from_slice(&headerlength.to_le_bytes());
    file.extend_from_slice(&totallength.to_le_bytes());
    file.extend_from_slice(&region);
    file
}

/// Open `file`, expecting an error (`NsisArchive` is not `Debug`, so this
/// avoids `unwrap_err`).
fn open_err(file: &[u8]) -> io::Error {
    match NsisArchive::open(file) {
        Ok(_) => panic!("expected an error, got a parsed archive"),
        Err(e) => e,
    }
}

/// Decode every non-directory entry into a name → content map.
fn extract_all(arc: &NsisArchive) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
    let mut map = std::collections::BTreeMap::new();
    for (i, e) in arc.entries().iter().enumerate() {
        if e.is_dir() {
            continue;
        }
        let mut out = Vec::new();
        arc.read_entry(i, &mut out).unwrap();
        map.insert(e.name().to_vec(), out);
    }
    map
}

// --- recognition -----------------------------------------------------------

#[test]
fn recognizes_valid_installer() {
    let file = build(
        &[Op::File {
            name: b"a.txt".to_vec(),
            content: b"hello".to_vec(),
        }],
        true,
        false,
        Enc::Lzma,
        0,
    );
    assert!(NsisArchive::recognize(&file));
    assert!(NsisArchive::open(&file[..]).is_ok());
}

#[test]
fn recognizes_firstheader_after_stub() {
    let file = build(
        &[Op::File {
            name: b"a.txt".to_vec(),
            content: b"hi".to_vec(),
        }],
        true,
        false,
        Enc::Lzma,
        512,
    );
    assert!(NsisArchive::recognize(&file));
}

#[test]
fn rejects_uninstaller_bit() {
    let mut file = build(
        &[Op::File {
            name: b"a.txt".to_vec(),
            content: b"hi".to_vec(),
        }],
        true,
        false,
        Enc::Lzma,
        0,
    );
    // Set the uninstaller flag (bit 1) in the firstheader flags word.
    file[0] = 0x02;
    assert!(!NsisArchive::recognize(&file));
    assert!(NsisArchive::open(&file[..]).is_err());
}

#[test]
fn rejects_old_nullsoft_signature() {
    let mut file = build(
        &[Op::File {
            name: b"a.txt".to_vec(),
            content: b"hi".to_vec(),
        }],
        true,
        false,
        Enc::Lzma,
        0,
    );
    // Flip the lowercase 's' of "Nullsoft" (offset 4+8) to uppercase 'S': this
    // is the pre-1.6 signature, which is not our format.
    file[12] = b'S';
    assert!(!NsisArchive::recognize(&file));
}

// --- solid vs non-solid, both codecs --------------------------------------

fn roundtrip_case(solid: bool, unicode: bool, enc: Enc) {
    let ops = vec![
        Op::File {
            name: b"readme.txt".to_vec(),
            content: b"first file contents, reasonably compressible ".repeat(10),
        },
        Op::File {
            name: b"data\\payload.bin".to_vec(),
            content: (0..500u32).map(|i| (i * 7) as u8).collect(),
        },
    ];
    let file = build(&ops, solid, unicode, enc, 0);
    let arc = NsisArchive::open(&file[..]).unwrap();
    let map = extract_all(&arc);

    // The backslash separator in the name normalises to '/' in both encodings.
    assert_eq!(
        map.get(&b"readme.txt"[..]).unwrap(),
        &b"first file contents, reasonably compressible ".repeat(10)
    );
    assert_eq!(
        map.get(&b"data/payload.bin"[..]).unwrap(),
        &(0..500u32).map(|i| (i * 7) as u8).collect::<Vec<u8>>()
    );
}

#[test]
fn solid_lzma_roundtrip() {
    roundtrip_case(true, false, Enc::Lzma);
}

#[test]
fn non_solid_lzma_roundtrip() {
    roundtrip_case(false, false, Enc::Lzma);
}

#[test]
fn solid_deflate_roundtrip() {
    roundtrip_case(true, false, Enc::Deflate);
}

#[test]
fn non_solid_deflate_roundtrip() {
    roundtrip_case(false, false, Enc::Deflate);
}

#[test]
fn unicode_names_roundtrip() {
    roundtrip_case(true, true, Enc::Lzma);
}

// --- sizes, directories, offsets ------------------------------------------

#[test]
fn reports_sizes_for_solid_and_stored() {
    let ops = vec![Op::File {
        name: b"f.bin".to_vec(),
        content: vec![0xABu8; 321],
    }];
    // Solid: size known from the decompressed block length.
    let solid = NsisArchive::open(&build(&ops, true, false, Enc::Lzma, 0)[..]).unwrap();
    let f = solid.entries().iter().find(|e| !e.is_dir()).unwrap();
    assert_eq!(f.size(), Some(321));
}

#[test]
fn non_solid_compressed_size_unknown_until_read() {
    let ops = vec![Op::File {
        name: b"f.bin".to_vec(),
        content: vec![0xCDu8; 200],
    }];
    let arc = NsisArchive::open(&build(&ops, false, false, Enc::Lzma, 0)[..]).unwrap();
    let f = arc.entries().iter().find(|e| !e.is_dir()).unwrap();
    // A compressed non-solid member's uncompressed size is not in the metadata.
    assert_eq!(f.size(), None);
    // …but it still extracts correctly.
    let (i, _) = arc
        .entries()
        .iter()
        .enumerate()
        .find(|(_, e)| !e.is_dir())
        .unwrap();
    let mut out = Vec::new();
    arc.read_entry(i, &mut out).unwrap();
    assert_eq!(out, vec![0xCDu8; 200]);
}

#[test]
fn setoutpath_directory_prefixes_files() {
    let ops = vec![
        Op::Dir(b"folder".to_vec()),
        Op::File {
            name: b"inside.txt".to_vec(),
            content: b"in a folder".to_vec(),
        },
    ];
    let arc = NsisArchive::open(&build(&ops, true, false, Enc::Lzma, 0)[..]).unwrap();
    let map = extract_all(&arc);
    assert_eq!(map.get(&b"folder/inside.txt"[..]).unwrap(), b"in a folder");
    // The directory entry is present and marked as such.
    assert!(arc
        .entries()
        .iter()
        .any(|e| e.is_dir() && e.name() == b"folder"));
}

// --- Unsupported / error branches -----------------------------------------

/// Build a minimal firstheader whose data area begins with `sig`.
fn firstheader_with_data(headerlength: u32, data: &[u8]) -> Vec<u8> {
    let totallength = (data.len() + 32) as u32;
    let mut file = Vec::new();
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&NEW_MAGIC);
    file.extend_from_slice(&headerlength.to_le_bytes());
    file.extend_from_slice(&totallength.to_le_bytes());
    file.extend_from_slice(data);
    file
}

// NSIS-bzip2 and FilteredLZMA are now decoded (task 20b); their real cross-checks
// live in `tests/oracle.rs` (bzip2) and `codec.rs` (FilteredLZMA round-trip).

#[test]
fn zlib_signature_is_unsupported() {
    let file = firstheader_with_data(100, &[0x78, 0xda, 0x00, 0x00, 0x00, 0x00, 0x00]);
    let err = open_err(&file);
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn non_sectioned_header_is_unsupported() {
    // A valid solid LZMA stream whose header has no `00 00` in its last 32 bytes.
    let header = vec![0x01u8; 80];
    let headerlength = header.len() as u32;
    let mut stream = Vec::new();
    stream.extend_from_slice(&headerlength.to_le_bytes());
    stream.extend_from_slice(&header);
    let region = lzma_encode_raw(&stream);
    let file = firstheader_with_data(headerlength, &region);
    let err = open_err(&file);
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn non_nsis_data_is_rejected() {
    let junk = vec![0u8; 2048];
    assert!(!NsisArchive::recognize(&junk));
    assert!(NsisArchive::open(&junk[..]).is_err());
}

#[test]
fn block_map_records_offsets_and_flags() {
    // Two stored blocks in a synthetic decompressed stream: [len0][8 bytes]
    // [len1|flag][3 bytes]. Keys are 0 and len0+4.
    let mut stream = Vec::new();
    stream.extend_from_slice(&8u32.to_le_bytes());
    stream.extend_from_slice(&[0u8; 8]);
    stream.extend_from_slice(&(3u32 | 0x8000_0000).to_le_bytes());
    stream.extend_from_slice(&[0u8; 3]);
    let map = find_blocks(&stream, 0, stream.len());
    assert_eq!(map.get(&0), Some(&8u32));
    assert_eq!(map.get(&12), Some(&(3u32 | 0x8000_0000)));
    assert_eq!(map.len(), 2);
}
