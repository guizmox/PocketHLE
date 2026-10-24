# newtua-nsis

Pure-Rust decoder for NSIS (Nullsoft Scriptable Install System) installer
archives.

## Format

**NSIS** installers are self-extracting Windows executables (in wide use
since the late 1990s) with the payload appended after the exe stub. This
crate ports the *sectioned header* layout used by NSIS 2.0 and newer
(including 3.x Unicode builds), where every offset comes from a fixed header
field; the older pre-2.0 heuristic layout is out of scope and surfaces as an
`Unsupported` error rather than silently mis-decoding.

Supported payload codecs, solid and non-solid: LZMA (the modern default),
NSIS-deflate, NSIS-bzip2 (both the NSIS2 and randomized NSIS1 variants), and
FilteredLZMA (LZMA plus the x86 BCJ branch filter). Entry names are exposed as
raw bytes so callers can pick their own encoding. Built on the shared
primitives in [`newtua-common`](https://crates.io/crates/newtua-common); LZMA
decoding uses the pure-Rust `lzma-rs` crate.

## Installation

```sh
cargo add newtua-nsis
```

## Example

```rust
use newtua_nsis::NsisArchive;

fn main() -> std::io::Result<()> {
    let data = std::fs::read("installer.exe")?;
    let archive = NsisArchive::open(&data[..])?;
    for (idx, entry) in archive.entries().iter().enumerate() {
        let mut out = Vec::new();
        archive.read_entry(idx, &mut out)?;
        println!("{}: {} bytes", String::from_utf8_lossy(entry.name()), out.len());
    }
    Ok(())
}
```

## Part of newtua-formats

`newtua-nsis` is one of the crates in
**[newtua-formats](https://github.com/new-the-unarchiver/newtua-formats)** — a
family of pure-Rust decoders for legacy archive formats, ported from The
Unarchiver's XADMaster.

`newtua-formats` is in turn the legacy-formats layer of
**[New The Unarchiver](https://github.com/new-the-unarchiver)** (`newtua`) — a
cross-platform archive extractor written in Rust, a modern rewrite of the
macOS tool The Unarchiver. It extracts and lists archives; it never creates
them.

Every crate here stands on its own: `newtua-nsis` needs nothing beyond
`newtua-common` and works fine as a standalone NSIS-installer reader, whether
or not you use it through `newtua`.

## License

LGPL-3.0-or-later — see [`LICENSE`](https://github.com/new-the-unarchiver/newtua-formats/blob/main/LICENSE) in the repository root.
