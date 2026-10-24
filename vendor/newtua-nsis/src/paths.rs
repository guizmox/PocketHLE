// SPDX-FileCopyrightText: 2026 Aleksei Trankov and contributors
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Filename/path expansion for NSIS sectioned headers.
//!
//! NSIS stores paths in a string table with embedded variable markers. This is
//! a faithful port of the sectioned-header cases of `XADNSISParser.m`:
//! `expandNewVariablesWithBytes:` (`:930-990`, ANSI 3-byte `FD 8x 80` markers)
//! and `expandUnicodePathWithOffset:` (`:1046-1125`, UTF-16LE with `0xE001` /
//! `0xE002` markers). The older `$`-style and old-binary expansions are not part
//! of the sectioned (NSIS 2.0+/3.x) branch and are not ported.
//!
//! Shell variables (`$WINDIR` and friends) are **not** resolved to real paths —
//! exactly as the reference, we emit the symbolic placeholder text it produces
//! and leave resolution to the caller.
//!
//! Path bytes are normalised to `/`-separated components (dropping empty
//! components) to match how `unar` lays extracted files out on disk. ANSI names
//! stay raw (no charset decoding); Unicode (UTF-16) names are decoded to UTF-8,
//! which is the only sensible byte form for that text.

/// The 32-entry variable table shared by the ANSI and Unicode expanders
/// (`NewBinaryExpansions` `:932-987` and the Unicode `strings[]` `:1049-1067`
/// are identical). `None` means "prepend the current or out directory"; an empty
/// string means "expands to nothing" (used for `$OUTDIR`/`$INSTDIR`).
const EXPANSIONS: [Option<&str>; 32] = [
    Some("Register 0"),
    Some("Register 1"),
    Some("Register 2"),
    Some("Register 3"),
    Some("Register 4"),
    Some("Register 5"),
    Some("Register 6"),
    Some("Register 7"),
    Some("Register 8"),
    Some("Register 9"),
    Some("Register R0"),
    Some("Register R1"),
    Some("Register R2"),
    Some("Register R3"),
    Some("Register R4"),
    Some("Register R5"),
    Some("Register R6"),
    Some("Register R7"),
    Some("Register R8"),
    Some("Register R9"),
    Some("CMDLINE"),
    Some(""), // OUTDIR expands to empty
    None,     // _outdir prefix
    Some("Installer Executable Directory"),
    Some("Language"),
    Some("Windows Temporary Directory"),
    Some("NSIS Plugins Directory"),
    Some("Installer Executable Path"),
    Some("Installer Executable Name"),
    None,
    Some("_CLICK"),
    None,
];

/// Which directory a leading `None` variable prepends to the expanded path.
#[derive(Clone, Copy)]
enum Prepend {
    /// The current output directory (the last `SetOutPath`).
    Dir,
    /// The saved `$OUTDIR` (`assign` opcode 25).
    Outdir,
}

/// Read a little-endian `u16` at `idx`, or `0` past the end (defensive; callers
/// bound their indices to the code-unit count first).
fn u16_at(bytes: &[u8], idx: usize) -> u16 {
    match (bytes.get(idx), bytes.get(idx + 1)) {
        (Some(&lo), Some(&hi)) => u16::from(lo) | (u16::from(hi) << 8),
        _ => 0,
    }
}

/// Split a Windows-separated (`\`) path into `/`-joined components, dropping
/// empty components (leading/duplicated separators).
fn norm_win(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for part in bytes.split(|&b| b == b'\\') {
        if part.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(b'/');
        }
        out.extend_from_slice(part);
    }
    out
}

/// Join two already-normalised path fragments with a single `/`.
pub fn join(a: &[u8], b: &[u8]) -> Vec<u8> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let mut out = Vec::with_capacity(a.len() + 1 + b.len());
    out.extend_from_slice(a);
    out.push(b'/');
    out.extend_from_slice(b);
    out
}

/// Apply a leading-variable prepend, matching the reference's
/// `[prependdir pathByAppendingPath:path]`.
fn apply(prepend: Option<Prepend>, seg: Vec<u8>, current_dir: &[u8], outdir: &[u8]) -> Vec<u8> {
    match prepend {
        Some(Prepend::Dir) => join(current_dir, &seg),
        Some(Prepend::Outdir) => join(outdir, &seg),
        None => seg,
    }
}

/// Expand the path string at `offset` in the string table `[stringoffs,
/// stringendoffs)` of `header`, returning `/`-joined path bytes with any
/// leading `$INSTDIR`/`$OUTDIR` prepend already applied.
pub fn expand(
    header: &[u8],
    stringoffs: usize,
    stringendoffs: usize,
    offset: usize,
    unicode: bool,
    current_dir: &[u8],
    outdir: &[u8],
) -> Vec<u8> {
    if unicode {
        let (string, prepend) = expand_unicode(header, stringoffs, stringendoffs, offset);
        apply(prepend, norm_win(&string), current_dir, outdir)
    } else {
        let raw = read_cstring(header, stringoffs, stringendoffs, offset);
        match expand_ansi(raw) {
            Some((data, prepend)) => apply(prepend, norm_win(&data), current_dir, outdir),
            None => norm_win(raw),
        }
    }
}

/// Slice the NUL-terminated ANSI string at `stringoffs + offset`, bounded by the
/// string table end and the header length (`:800-802`).
fn read_cstring(header: &[u8], stringoffs: usize, stringendoffs: usize, offset: usize) -> &[u8] {
    let start = (stringoffs + offset).min(header.len());
    let mut length = 0;
    while stringoffs + length < header.len()
        && stringoffs + length < stringendoffs
        && start + length < header.len()
        && header[start + length] != 0
    {
        length += 1;
    }
    &header[start..start + length]
}

/// Port of `expandNewVariablesWithBytes:` / `expandVariables:` (`:930-1044`).
///
/// Returns `None` if no variable marker occurred (the caller then uses the raw
/// bytes), else the rewritten bytes (still `\`-separated) plus any leading-var
/// prepend. Mirrors the reference's quirk that copying into the output buffer
/// only starts once the first marker is seen.
fn expand_ansi(bytes: &[u8]) -> Option<(Vec<u8>, Option<Prepend>)> {
    let mut data: Option<Vec<u8>> = None;
    let mut prepend = None;
    let mut i = 0;
    while i < bytes.len() {
        let mut found = false;
        for (j, entry) in EXPANSIONS.iter().enumerate() {
            // Variable marker: FD (80+j) 80.
            if i + 3 <= bytes.len()
                && bytes[i] == 0xfd
                && bytes[i + 1] == 0x80 + j as u8
                && bytes[i + 2] == 0x80
            {
                let d = data.get_or_insert_with(|| bytes[..i].to_vec());
                let exp: &str = match entry {
                    None => {
                        if i == 0 {
                            prepend = Some(if j <= 23 {
                                Prepend::Dir
                            } else {
                                Prepend::Outdir
                            });
                        }
                        ""
                    }
                    Some(e) => e,
                };
                if exp.is_empty() {
                    // Skip a single leading separator after an empty expansion.
                    if i == 0 && bytes.get(3) == Some(&b'\\') {
                        i += 1;
                    }
                } else {
                    d.extend_from_slice(exp.as_bytes());
                }
                i += 3 - 1; // varlen - 1
                found = true;
                break;
            }
        }
        if !found {
            if let Some(d) = data.as_mut() {
                d.push(bytes[i]);
            }
        }
        i += 1;
    }
    data.map(|d| (d, prepend))
}

/// Port of `expandUnicodePathWithOffset:` (`:1046-1125`). Builds a UTF-8 string
/// from the UTF-16LE path, resolving `0xE001` user-variable and `0xE002`
/// shell-variable markers.
fn expand_unicode(
    header: &[u8],
    stringoffs: usize,
    stringendoffs: usize,
    offset: usize,
) -> (Vec<u8>, Option<Prepend>) {
    let start = stringoffs + offset * 2;
    // Count code units up to the aligned `00 00` terminator.
    let mut length = 0;
    while stringoffs + length * 2 < header.len()
        && stringoffs + length * 2 < stringendoffs
        && start + length * 2 + 1 < header.len()
        && !(header[start + length * 2] == 0 && header[start + length * 2 + 1] == 0)
    {
        length += 1;
    }

    let mut out: Vec<u8> = Vec::new();
    let mut prepend = None;
    let mut i = 0;
    while i < length {
        let c = u16_at(header, start + i * 2);
        if c == 0xe001 && i + 1 < length {
            let raw_next = u16_at(header, start + i * 2 + 2);
            let val = (raw_next & 0x7fff) as usize;
            if val < 32 {
                let exp: &str = match EXPANSIONS[val] {
                    None => {
                        if i == 0 {
                            prepend = Some(if val == 22 {
                                Prepend::Dir
                            } else {
                                Prepend::Outdir
                            });
                        }
                        ""
                    }
                    Some(e) => e,
                };
                out.extend_from_slice(exp.as_bytes());
                if i == 0
                    && length >= 3
                    && exp.is_empty()
                    && u16_at(header, start + i * 2 + 4) == 0x5c
                {
                    i += 1;
                }
            } else {
                out.extend_from_slice(format!("User variable 0x{raw_next:x}").as_bytes());
            }
            i += 1;
        } else if c == 0xe002 && i + 1 < length {
            let raw_next = u16_at(header, start + i * 2 + 2);
            out.extend_from_slice(format!("Shell variable 0x{raw_next:x}").as_bytes());
            i += 1;
        } else if let Some(ch) = char::from_u32(u32::from(c)) {
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
        i += 1;
    }

    (out, prepend)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal header whose string table is the given bytes at `stringoffs`.
    fn header_with_strings(stringoffs: usize, table: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; stringoffs];
        h.extend_from_slice(table);
        h
    }

    #[test]
    fn plain_ansi_name() {
        // "a\\b.txt\0"
        let table = b"a\\b.txt\0";
        let h = header_with_strings(4, &table[..]);
        let p = expand(&h, 4, 4 + table.len(), 0, false, b"", b"");
        assert_eq!(p, b"a/b.txt");
    }

    #[test]
    fn ansi_name_with_current_dir() {
        let table = b"file.bin\0";
        let h = header_with_strings(8, table);
        let p = expand(&h, 8, 8 + table.len(), 0, false, b"dir/sub", b"");
        // expand() returns the bare name; the caller joins with the dir. Here we
        // pass current_dir but there is no leading variable, so it is not used.
        assert_eq!(p, b"file.bin");
    }

    #[test]
    fn ansi_register_variable_expands_to_placeholder() {
        // "$0\\x" where $0 is FD 80 80.
        let table = vec![0xfd, 0x80, 0x80, b'\\', b'x', 0];
        let h = header_with_strings(4, &table);
        let end = h.len();
        let p = expand(&h, 4, end, 0, false, b"", b"");
        assert_eq!(p, b"Register 0/x");
    }

    #[test]
    fn ansi_outdir_prepend_variable() {
        // FD 96 80 is index 22 (None, j<=23 -> current dir prepend) at head, then
        // "\\name". Index 22 <= 23 so it prepends the *current* dir.
        let table = vec![0xfd, 0x96, 0x80, b'\\', b'n', b'a', b'm', b'e', 0];
        let h = header_with_strings(4, &table);
        let end = h.len();
        let p = expand(&h, 4, end, 0, false, b"base", b"out");
        assert_eq!(p, b"base/name");
    }

    #[test]
    fn ansi_outdir_index_29_prepends_outdir() {
        // FD 9D 80 is index 29 (None, j>23 -> outdir prepend).
        let table = vec![0xfd, 0x9d, 0x80, b'\\', b'z', 0];
        let h = header_with_strings(4, &table);
        let end = h.len();
        let p = expand(&h, 4, end, 0, false, b"base", b"out");
        assert_eq!(p, b"out/z");
    }

    #[test]
    fn unicode_plain_name() {
        // UTF-16LE "a\\b" then 00 00 terminator.
        let mut table = Vec::new();
        for ch in "a\\b".encode_utf16() {
            table.extend_from_slice(&ch.to_le_bytes());
        }
        table.extend_from_slice(&[0, 0]);
        let h = header_with_strings(4, &table);
        let end = h.len();
        let p = expand(&h, 4, end, 0, true, b"", b"");
        assert_eq!(p, b"a/b");
    }

    #[test]
    fn unicode_user_variable_placeholder() {
        // 0xE001 then value 0x8025: low 15 bits = 0x25 (37) >= 32, so it renders
        // as the placeholder for the full raw value.
        let mut table = Vec::new();
        table.extend_from_slice(&0xe001u16.to_le_bytes());
        table.extend_from_slice(&0x8025u16.to_le_bytes());
        table.extend_from_slice(&[0, 0]);
        let h = header_with_strings(4, &table);
        let end = h.len();
        let p = expand(&h, 4, end, 0, true, b"", b"");
        assert_eq!(p, b"User variable 0x8025");
    }

    #[test]
    fn unicode_shell_variable_placeholder() {
        // 0xE002 then some value -> "Shell variable 0x...".
        let mut table = Vec::new();
        table.extend_from_slice(&0xe002u16.to_le_bytes());
        table.extend_from_slice(&0x0026u16.to_le_bytes());
        table.extend_from_slice(&[0, 0]);
        let h = header_with_strings(4, &table);
        let end = h.len();
        let p = expand(&h, 4, end, 0, true, b"", b"");
        assert_eq!(p, b"Shell variable 0x26");
    }

    #[test]
    fn unicode_instdir_prepend() {
        // 0xE001 val 22 (INSTDIR, None -> current dir) at head, then "\\file".
        let mut table = Vec::new();
        table.extend_from_slice(&0xe001u16.to_le_bytes());
        table.extend_from_slice(&22u16.to_le_bytes());
        for ch in "\\file".encode_utf16() {
            table.extend_from_slice(&ch.to_le_bytes());
        }
        table.extend_from_slice(&[0, 0]);
        let h = header_with_strings(4, &table);
        let end = h.len();
        let p = expand(&h, 4, end, 0, true, b"cur", b"out");
        assert_eq!(p, b"cur/file");
    }
}
