//! Desktop installer containers. Never execute host code; names are not used
//! for format detection. Every decoded layer shares an allocation budget.
use std::io::{Cursor, Read};
mod installshield;
mod nsis_bzip;
mod pocketsetup;
mod rar;
mod sharedparts;
mod vise;
mod wise;

const LIMIT: usize = 256 * 1024 * 1024;
const DEPTH: usize = 6;
type Result<T> = std::result::Result<T, String>;

#[derive(Debug)]
pub(crate) struct Cabinet {
    pub name: String,
    pub bytes: Vec<u8>,
}

fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(p..p.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(b: &[u8], p: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(p..p.checked_add(4)?)?.try_into().ok()?) as usize)
}
fn inflate(input: &[u8], zlib: bool, budget: &mut usize) -> Result<Vec<u8>> {
    let reader: Box<dyn Read + '_> = if zlib {
        Box::new(flate2::read::ZlibDecoder::new(input))
    } else {
        Box::new(flate2::read::DeflateDecoder::new(input))
    };
    let mut out = Vec::new();
    reader
        .take(*budget as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    charge(out.len(), budget)?;
    Ok(out)
}
fn charge(size: usize, budget: &mut usize) -> Result<()> {
    *budget = budget
        .checked_sub(size)
        .ok_or("Installer exceeds decoded size limit")?;
    Ok(())
}

/// None means an ordinary guest executable, not a recognized installer.
pub(crate) fn extract(bytes: &[u8]) -> Result<Option<Vec<Cabinet>>> {
    if bytes.len() > LIMIT {
        return Err("Installer exceeds input size limit".into());
    }
    // Guest PE binaries must retain the standalone executable import path.
    if bytes.starts_with(b"MZ") {
        if let Some(pe) = u32_at(bytes, 60).filter(|p| *p <= bytes.len().saturating_sub(24)) {
            if matches!(
                u16_at(bytes, pe + 4),
                Some(0x01c0 | 0x01c2 | 0x01c4 | 0x0162 | 0x0166)
            ) {
                return Ok(None);
            }
        }
    }
    let mut out = Vec::new();
    let mut budget = LIMIT;
    let recognized = layer(bytes, "payload", 0, &mut budget, &mut out)?;
    if !recognized {
        return Ok(None);
    }
    if out.is_empty() {
        return Err("Recognized installer contains no supported PocketPC cabinet".into());
    }
    Ok(Some(out))
}

fn layer(
    b: &[u8],
    name: &str,
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<Cabinet>,
) -> Result<bool> {
    if depth > DEPTH {
        return Err("Installer nesting limit exceeded".into());
    }
    if b.starts_with(b"ISc(") {
        return Err("InstallShield CAB detected: this archive format is not supported yet".into());
    }
    if b.starts_with(b"MSCF") {
        let size = u32_at(b, 8).ok_or("Truncated cabinet")?;
        if size < 36 || size > b.len() || u32_at(b, 16).is_none_or(|p| p >= size) {
            return Err("Invalid cabinet bounds".into());
        }
        // Some desktop launchers wrap the CE cabinet in another Microsoft
        // cabinet containing Setup.exe/Setup.ini. Decode the container rather
        // than mistaking its desktop files for the installed PocketPC game.
        let mut archive = cab::Cabinet::new(Cursor::new(&b[..size])).map_err(|e| e.to_string())?;
        let names: Vec<_> = archive
            .folder_entries()
            .flat_map(|f| f.file_entries())
            .map(|f| f.name().to_owned())
            .collect();
        if names.len() > 4096 {
            return Err("Too many cabinet entries".into());
        }
        let is_ce = names.iter().any(|n| {
            n.eq_ignore_ascii_case("_setup.xml") || n.to_ascii_lowercase().ends_with(".000")
        });
        if !is_ce {
            let previous = out.len();
            let mut entries = Vec::new();
            for child in names.iter().filter(|n| {
                let name = n.replace('\\', "/").to_ascii_lowercase();
                name.ends_with(".cab")
                    || name.ends_with(".hdr")
                    || name
                        .rsplit('/')
                        .next()
                        .is_some_and(|n| n == "files.dat" || n == "parts.dat")
            }) {
                let mut data = Vec::new();
                archive
                    .read_file(child)
                    .map_err(|e| e.to_string())?
                    .take(*budget as u64 + 1)
                    .read_to_end(&mut data)
                    .map_err(|e| e.to_string())?;
                charge(data.len(), budget)?;
                entries.push((child.clone(), data));
            }
            if let Some(rebuilt) = sharedparts::extract(&entries, budget)? {
                for cabinet in rebuilt {
                    layer(&cabinet.bytes, &cabinet.name, depth + 1, budget, out)?;
                }
            } else {
                archive_layers(entries, depth, budget, out)?;
            }
            if out.len() > previous {
                return Ok(true);
            }
        }
        if !out.iter().any(|c| c.bytes == b[..size]) {
            if out.len() >= 32 {
                return Err("Too many embedded cabinets".into());
            }
            charge(size, budget)?;
            out.push(Cabinet {
                name: name.to_owned(),
                bytes: b[..size].to_vec(),
            });
        }
        return Ok(true);
    }
    // ZIP supports self-extracting executables; its central directory supplies
    // offsets and sizes. Never extract archive paths onto the host filesystem.
    if let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(b)) {
        if zip.len() > 4096 {
            return Err("Too many installer archive entries".into());
        }
        let mut entries = Vec::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            if entry.size() > *budget as u64 {
                return Err("Installer archive exceeds size limit".into());
            }
            let child_name = entry.name().to_owned();
            let mut data = Vec::new();
            (&mut entry)
                .take(*budget as u64 + 1)
                .read_to_end(&mut data)
                .map_err(|e| e.to_string())?;
            charge(data.len(), budget)?;
            entries.push((child_name, data));
        }
        archive_layers(entries, depth, budget, out)?;
        return Ok(true);
    }
    if let Some(entries) = rar::extract(b, budget)? {
        archive_layers(entries, depth, budget, out)?;
        return Ok(true);
    }
    // Legacy NSIS non-solid Deflate. The complete first-header signature
    // distinguishes it from instruction constants in the executable stub.
    let sig = b"\xef\xbe\xad\xdeNullsoftInst";
    if let Some(p) = b.windows(sig.len()).position(|w| w == sig) {
        if p < 4 {
            return Err("Invalid NSIS header".into());
        }
        let base = p - 4;
        let length = u32_at(b, base + 24).ok_or("Truncated NSIS header")?;
        let end = base
            .checked_add(length)
            .filter(|e| *e <= b.len())
            .ok_or("Invalid NSIS length")?;
        if let Some(cabinets) = nsis_bzip::extract(b, base, end, budget)? {
            for cabinet in cabinets {
                layer(&cabinet.bytes, &cabinet.name, depth + 1, budget, out)?;
            }
            return Ok(true);
        }
        let mut pos = base + 28;
        while pos < end {
            // Optional trailing installer CRC is not an archive block.
            if end - pos == 4 {
                break;
            }
            let raw = u32_at(b, pos).ok_or("Truncated NSIS block")?;
            let count = raw & 0x7fff_ffff;
            let start = pos + 4;
            let next = start
                .checked_add(count)
                .filter(|n| *n <= end)
                .ok_or("Invalid NSIS block length")?;
            let data = &b[start..next];
            if raw & 0x8000_0000 != 0 {
                let decoded = inflate(data, false, budget)?;
                layer(&decoded, name, depth + 1, budget, out)?;
            } else {
                layer(data, name, depth + 1, budget, out)?;
            }
            pos = next;
        }
        return Ok(true);
    }
    if !b.starts_with(b"MZ") {
        return Ok(false);
    }
    let pe = u32_at(b, 60)
        .filter(|p| *p <= b.len().saturating_sub(24))
        .ok_or("Truncated PE header")?;
    if b.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Ok(false);
    }
    let count = u16_at(b, pe + 6).ok_or("Truncated PE header")? as usize;
    if count > 96 {
        return Err("Too many PE sections".into());
    }
    let optional = u16_at(b, pe + 20).ok_or("Truncated PE header")? as usize;
    let sections = pe + 24 + optional;
    let raw = |rva: usize| -> Option<usize> {
        for i in 0..count {
            let p = sections + 40 * i;
            let va = u32_at(b, p + 12)?;
            let len = u32_at(b, p + 16)?;
            if rva >= va && rva - va < len {
                return u32_at(b, p + 20)?.checked_add(rva - va);
            }
        }
        None
    };
    let mut overlay = 0usize;
    for i in 0..count {
        let p = sections + 40 * i;
        let offset = u32_at(b, p + 20).ok_or("Truncated section")?;
        let length = u32_at(b, p + 16).ok_or("Truncated section")?;
        overlay = overlay.max(offset.checked_add(length).ok_or("Section overflow")?);
    }
    if let Some(cabinets) = pocketsetup::extract(b, budget)? {
        for cabinet in cabinets {
            layer(&cabinet.bytes, &cabinet.name, depth + 1, budget, out)?;
        }
        return Ok(true);
    }
    if let Some(cabinets) = vise::extract(b, overlay, budget)? {
        for cabinet in cabinets {
            layer(&cabinet.bytes, &cabinet.name, depth + 1, budget, out)?;
        }
        return Ok(true);
    }
    if let Some(cabinets) = wise::extract(b, overlay, budget)? {
        for cabinet in cabinets {
            layer(&cabinet.bytes, &cabinet.name, depth + 1, budget, out)?;
        }
        return Ok(true);
    }
    let mut recognized = false;
    // Direct cabinets inside a PE overlay (including an inflated launcher).
    for (pos, _) in b.windows(4).enumerate().filter(|(_, w)| *w == b"MSCF") {
        if let Some(size) = u32_at(b, pos + 8) {
            if size >= 36
                && pos.checked_add(size).is_some_and(|e| e <= b.len())
                && b.get(pos + 24..pos + 26) == Some(&[3, 1][..])
                && u32_at(b, pos + 4) == Some(0)
                && u32_at(b, pos + 12) == Some(0)
                && u32_at(b, pos + 20) == Some(0)
                && u32_at(b, pos + 16).is_some_and(|p| p >= 36 && p < size)
            {
                layer(&b[pos..pos + size], name, depth + 1, budget, out)?;
                recognized = true;
            }
        }
    }
    if u16_at(b, pe + 24) != Some(0x10b) || optional < 120 {
        return Ok(recognized);
    }
    let rva = u32_at(b, pe + 24 + 112).unwrap_or(0);
    let size = u32_at(b, pe + 24 + 116).unwrap_or(0);
    let Some(base) = raw(rva) else {
        return Ok(recognized);
    };
    let end = base
        .checked_add(size)
        .filter(|e| *e <= b.len())
        .ok_or("Invalid resource bounds")?;
    let tree = &b[base..end];
    let mut pending = vec![(0usize, 0usize)];
    let mut visited = std::collections::HashSet::new();
    while let Some((dir, level)) = pending.pop() {
        if level > 3 || !visited.insert(dir) {
            return Err("Invalid resource directory cycle/depth".into());
        }
        let n = u16_at(tree, dir + 12).ok_or("Truncated resource directory")? as usize
            + u16_at(tree, dir + 14).ok_or("Truncated resource directory")? as usize;
        if visited.len() > 4096 || n > 4096 || pending.len().saturating_add(n) > 4096 {
            return Err("Resource directory limit exceeded".into());
        }
        for i in 0..n {
            let value = u32_at(tree, dir + 16 + i * 8 + 4).ok_or("Truncated resource entry")?;
            let offset = value & 0x7fff_ffff;
            if value & 0x8000_0000 != 0 {
                pending.push((offset, level + 1));
                continue;
            }
            let rva = u32_at(tree, offset).ok_or("Truncated resource data")?;
            let len = u32_at(tree, offset + 4).ok_or("Truncated resource size")?;
            let start = raw(rva).ok_or("Invalid resource address")?;
            let data = b
                .get(start..start.checked_add(len).ok_or("Resource overflow")?)
                .ok_or("Truncated resource payload")?;
            // zlib CMF/FLG, DEFLATE, legal window, no preset dictionary.
            if data.len() >= 2
                && data[0] & 15 == 8
                && data[0] >> 4 <= 7
                && (u16::from(data[0]) * 256 + u16::from(data[1])) % 31 == 0
                && data[1] & 32 == 0
            {
                let decoded = inflate(data, true, budget)?;
                recognized |= layer(&decoded, name, depth + 1, budget, out)?;
            } else if data.starts_with(b"MSCF") || data.starts_with(b"MZ") {
                recognized |= layer(data, name, depth + 1, budget, out)?;
            }
        }
    }
    Ok(recognized)
}

fn archive_layers(
    entries: Vec<(String, Vec<u8>)>,
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<Cabinet>,
) -> Result<()> {
    if entries.iter().any(|(_, b)| b.starts_with(b"ISc(")) {
        for cabinet in installshield::extract(&entries, budget)? {
            layer(&cabinet.bytes, &cabinet.name, depth + 1, budget, out)?;
        }
    }
    for (child_name, data) in entries {
        if !data.starts_with(b"ISc(") {
            layer(&data, &child_name, depth + 1, budget, out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspect_supplied_installers() {
        let Some(paths) = std::env::var_os("POCKET_INSTALLER_SAMPLES") else {
            return;
        };
        for path in std::env::split_paths(&paths) {
            let b = std::fs::read(&path).unwrap();
            let result = extract(&b);
            eprintln!(
                "SAMPLE {}: {:?}",
                path.display(),
                result.as_ref().map(|v| v.as_ref().map(|v| v
                    .iter()
                    .map(|c| (&c.name, c.bytes.len()))
                    .collect::<Vec<_>>()))
            );
            assert!(result.unwrap().is_some());
        }
    }
    #[test]
    fn arbitrary_data_is_not_an_installer() {
        assert!(extract(b"not an installer").unwrap().is_none());
    }
    #[test]
    fn malformed_cab_and_installshield_fail_explicitly() {
        assert!(extract(b"MSCF").is_err());
        assert!(extract(b"ISc(").unwrap_err().contains("InstallShield"));
    }
    #[test]
    fn decoded_budget_is_enforced() {
        let mut budget = 3;
        assert!(charge(4, &mut budget).is_err());
        assert_eq!(budget, 3);
    }
}
