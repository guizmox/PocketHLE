// SPDX-FileCopyrightText: 2026 Aleksei Trankov and contributors
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Cross-check oracle: build real NSIS installers with `makensis` and verify our
//! extraction matches `unar` (XADMaster) byte-for-byte — both the entry paths
//! (including NSIS 3's quirky unexpanded `$INSTDIR` marker, a literal `U+0003`
//! plus an encoded code unit) and the file contents.
//!
//! Gated on both `unar` and `makensis` being installed; on this machine both are
//! present, so the oracle runs for real (the report notes "0 skipped"). The
//! installers are Unicode builds (the NSIS 3 default), whose entry names are
//! valid UTF-8 — this lets us compare raw path bytes exactly against the UTF-8
//! paths `unar` writes to disk. ANSI-path expansion is covered by unit tests.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use newtua_nsis::NsisArchive;
use newtua_testutil::{unar_extract_all, unar_installed};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// The payload files every fixture installs (path under `$INSTDIR` -> bytes).
const FILES: [(&str, &[u8]); 3] = [
    ("hello.txt", b"hello world from nsis"),
    ("readme.txt", b"second file contents here"),
    ("sub/data.bin", b"BINARYDATA\x00\x01\x02payload"),
];

/// Whether a working `makensis` is on `PATH` (the gate, like `unar_installed`).
fn makensis_installed() -> bool {
    Command::new("makensis")
        .arg("-VERSION")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A fresh, unique scratch directory under the system temp dir.
fn unique_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "newtua-nsis-oracle-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Compile an installer with `makensis` and return the `.exe` bytes. `compressor`
/// is the `SetCompressor` argument (e.g. `"/SOLID lzma"`, `"lzma"`, `"zlib"`).
fn build_installer(tag: &str, compressor: &str) -> Vec<u8> {
    let dir = unique_dir(tag);
    std::fs::write(dir.join("hello.txt"), FILES[0].1).unwrap();
    std::fs::write(dir.join("readme.txt"), FILES[1].1).unwrap();
    std::fs::write(dir.join("data_src.bin"), FILES[2].1).unwrap();

    // A minimal installer: two files in $INSTDIR and one in a subdirectory.
    let script = format!(
        "Unicode true\n\
         Name \"t\"\n\
         OutFile \"installer.exe\"\n\
         InstallDir \"$TEMP\\\\nsistest\"\n\
         SetCompressor {compressor}\n\
         Section\n\
         \tSetOutPath \"$INSTDIR\"\n\
         \tFile hello.txt\n\
         \tFile readme.txt\n\
         \tSetOutPath \"$INSTDIR\\\\sub\"\n\
         \tFile /oname=data.bin data_src.bin\n\
         SectionEnd\n"
    );
    std::fs::write(dir.join("test.nsi"), script).unwrap();

    let out = Command::new("makensis")
        .arg("-V2")
        .arg("test.nsi")
        .current_dir(&dir)
        .output()
        .expect("run makensis");
    assert!(
        out.status.success(),
        "makensis failed for {tag}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let exe = std::fs::read(dir.join("installer.exe")).expect("read installer.exe");
    let _ = std::fs::remove_dir_all(&dir);
    exe
}

/// Extract every file entry with our crate: raw path bytes -> contents.
fn ours(exe: &[u8]) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let ar = NsisArchive::open(Cursor::new(exe.to_vec())).expect("open installer");
    let mut map = BTreeMap::new();
    for (i, e) in ar.entries().iter().enumerate() {
        if e.is_dir() {
            continue;
        }
        let mut buf = Vec::new();
        ar.read_entry(i, &mut buf).expect("read entry");
        map.insert(e.name().to_vec(), buf);
    }
    map
}

/// `unar`'s extraction as raw path bytes -> contents (Unicode names are UTF-8).
fn theirs(exe: &[u8]) -> BTreeMap<Vec<u8>, Vec<u8>> {
    unar_extract_all(exe, "installer.exe")
        .into_iter()
        .map(|(k, v)| (k.into_bytes(), v))
        .collect()
}

/// Build a fixture with `compressor`, then assert our extraction equals `unar`'s.
fn assert_matches_unar(tag: &str, compressor: &str) {
    if !unar_installed() || !makensis_installed() {
        eprintln!("skipping NSIS oracle `{tag}`: unar/makensis not installed");
        return;
    }
    let exe = build_installer(tag, compressor);
    assert!(
        NsisArchive::recognize(&exe),
        "{tag}: not recognised as NSIS"
    );

    let ours = ours(&exe);
    let theirs = theirs(&exe);
    // Sanity: all three payloads survived the round trip through `unar`.
    assert_eq!(
        theirs.len(),
        FILES.len(),
        "{tag}: unexpected unar file count"
    );
    assert_eq!(ours, theirs, "{tag}: our extraction != unar");
}

#[test]
fn solid_lzma_matches_unar() {
    assert_matches_unar("solid-lzma", "/SOLID lzma");
}

#[test]
fn nonsolid_lzma_matches_unar() {
    assert_matches_unar("nonsolid-lzma", "lzma");
}

#[test]
fn nonsolid_deflate_matches_unar() {
    // The `zlib` compressor emits NSIS-deflate (raw, no zlib header), exercising
    // `newtua-common::deflate::inflate_nsis`.
    assert_matches_unar("nonsolid-deflate", "zlib");
}

#[test]
fn solid_bzip2_matches_unar() {
    // Solid bzip2 is reached only via the solid probe (`XADNSISParser.m:322-329`).
    assert_matches_unar("solid-bzip2", "/SOLID bzip2");
}

#[test]
fn nonsolid_bzip2_matches_unar() {
    // Non-solid bzip2 auto-detects as the NSIS2 (no-randomization) variant.
    assert_matches_unar("nonsolid-bzip2", "bzip2");
}
