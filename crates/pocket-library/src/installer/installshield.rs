//! InstallShield 5–13 cabinet sets, including length-prefixed Deflate blocks.
//! Layout reference: Unshield (MIT); see the bundled third-party notice.
use super::{charge, inflate, u16_at, u32_at, Cabinet, Result};

fn number(b: &[u8], p: usize) -> Result<usize> {
    u32_at(b, p).ok_or_else(|| "Truncated InstallShield metadata".into())
}
fn wide(b: &[u8], p: usize) -> Result<usize> {
    let value = u64::from_le_bytes(
        b.get(p..p + 8)
            .ok_or("Truncated InstallShield size")?
            .try_into()
            .unwrap(),
    );
    usize::try_from(value).map_err(|_| "InstallShield size exceeds host address space".into())
}

pub(super) fn extract(entries: &[(String, Vec<u8>)], budget: &mut usize) -> Result<Vec<Cabinet>> {
    let files = extract_files(entries, budget, false)?;
    // Preserve existing CAB imports; rebuild only when no prebuilt CAB exists.
    if !files
        .iter()
        .any(|f| f.name.to_ascii_lowercase().ends_with(".cab"))
        && files.iter().any(super::cabwiz::is_manifest)
    {
        let assets = extract_files(entries, budget, true)?;
        return super::cabwiz::extract(&assets, budget);
    }
    Ok(files
        .into_iter()
        .filter(|f| f.name.to_ascii_lowercase().ends_with(".cab"))
        .collect())
}

fn extract_files(
    entries: &[(String, Vec<u8>)],
    budget: &mut usize,
    assets: bool,
) -> Result<Vec<Cabinet>> {
    let mut out = Vec::new();
    let headers: Vec<_> = entries
        .iter()
        .filter(|(n, b)| n.to_ascii_lowercase().ends_with(".hdr") && b.starts_with(b"ISc("))
        .collect();
    if headers.is_empty() {
        return Err("InstallShield cabinet set is missing its .hdr file".into());
    }
    for (header_name, header) in headers {
        let version = number(header, 4)?;
        let major = if version >> 24 == 1 {
            (version >> 12) & 15
        } else {
            (version & 0xffff) / 100
        };
        if !(5..=13).contains(&major) {
            return Err(format!("Unsupported InstallShield cabinet version {major}"));
        }
        let base = number(header, 12)?;
        let descriptor_end = base
            .checked_add(48)
            .ok_or("InstallShield cabinet descriptor overflow")?;
        header
            .get(base..descriptor_end)
            .ok_or("Truncated InstallShield cabinet descriptor")?;
        let table = base
            .checked_add(number(header, base + 12)?)
            .ok_or("InstallShield table overflow")?;
        let count = number(header, base + 40)?;
        let descriptors = if major == 5 {
            let directories = number(header, base + 28)?;
            if directories > 4096 {
                return Err("Too many InstallShield directories".into());
            }
            table.checked_add(directories * 4)
        } else {
            table.checked_add(number(header, base + 44)?)
        }
        .ok_or("InstallShield descriptor overflow")?;
        if count > 4096 {
            return Err("Too many InstallShield entries".into());
        }
        let prefix = header_name
            .rsplit_once('.')
            .ok_or("Invalid InstallShield header name")?
            .0;
        let prefix = prefix.trim_end_matches(|c: char| c.is_ascii_digit());
        for i in 0..count {
            let (descriptor, flags, name_offset) = if major == 5 {
                let index = descriptors
                    .checked_add(i * 4)
                    .ok_or("InstallShield index overflow")?;
                let p = table
                    .checked_add(number(header, index)?)
                    .ok_or("InstallShield descriptor overflow")?;
                let d = header
                    .get(
                        p..p.checked_add(58)
                            .ok_or("InstallShield descriptor overflow")?,
                    )
                    .ok_or("Truncated InstallShield 5 descriptor")?;
                (d, u16_at(d, 8).unwrap(), number(d, 0)?)
            } else {
                let p = descriptors
                    .checked_add(i * 87)
                    .ok_or("InstallShield descriptor overflow")?;
                let d = header
                    .get(
                        p..p.checked_add(87)
                            .ok_or("InstallShield descriptor overflow")?,
                    )
                    .ok_or("Truncated InstallShield descriptor")?;
                (d, u16_at(d, 0).unwrap(), number(d, 58)?)
            };
            let data_offset = if major == 5 {
                number(descriptor, 38)?
            } else {
                wide(descriptor, 18)?
            };
            if flags & 8 != 0 || name_offset == 0 || data_offset == 0 {
                continue;
            }
            let start = table
                .checked_add(name_offset)
                .ok_or("InstallShield name overflow")?;
            let name = header
                .get(start..)
                .ok_or("Invalid InstallShield filename")?;
            let end = name
                .iter()
                .position(|b| *b == 0)
                .ok_or("Unterminated InstallShield filename")?;
            let mut name = String::from_utf8_lossy(&name[..end]).into_owned();
            if !assets
                && !name.to_ascii_lowercase().ends_with(".cab")
                && !name.to_ascii_lowercase().ends_with(".inf")
            {
                continue;
            }
            if assets {
                if major != 5 {
                    return Err(
                        "Raw CabWiz payloads require InstallShield 5 directory metadata".into(),
                    );
                }
                let directory = number(descriptor, 4)?;
                let directories = number(header, base + 28)?;
                if directory >= directories {
                    return Err("Invalid InstallShield source directory".into());
                }
                let offset = table
                    .checked_add(number(header, table + directory * 4)?)
                    .ok_or("InstallShield directory overflow")?;
                let directory = header
                    .get(offset..)
                    .ok_or("Invalid InstallShield source directory")?;
                let end = directory
                    .iter()
                    .position(|c| *c == 0)
                    .ok_or("Unterminated InstallShield directory")?;
                let directory = String::from_utf8_lossy(&directory[..end]);
                if !directory.is_empty() {
                    name = format!("{directory}/{name}");
                }
            }
            // Split/obfuscated/linked payloads need different decoding. Never
            // silently concatenate volumes or substitute a neighbouring file.
            if flags & !4 != 0 || (major != 5 && descriptor[84] != 0) {
                return Err("Unsupported split, obfuscated or linked InstallShield cabinet".into());
            }
            let expanded = if major == 5 {
                number(descriptor, 10)?
            } else {
                wide(descriptor, 2)?
            };
            let compressed = if major == 5 {
                number(descriptor, 14)?
            } else {
                wide(descriptor, 10)?
            };
            if expanded > *budget {
                return Err("InstallShield payload exceeds size limit".into());
            }
            let volume_name = if major == 5 {
                format!("{}.cab", header_name.rsplit_once('.').unwrap().0)
            } else {
                let volume = u16_at(descriptor, 85).unwrap();
                format!("{prefix}{volume}.cab")
            };
            let (_, data) = entries
                .iter()
                .find(|(n, b)| n.eq_ignore_ascii_case(&volume_name) && b.starts_with(b"ISc("))
                .ok_or_else(|| format!("Missing InstallShield volume {volume_name}"))?;
            if number(data, 4)? != version {
                return Err("InstallShield volume version mismatch".into());
            }
            if major == 5 {
                let first = number(data, 28)?;
                let last = number(data, 32)?;
                if i < first
                    || i > last
                    || (i + 1 < count && i == last && number(data, 56)? != compressed)
                    || (i > 0 && i == first && number(data, 44)? != compressed)
                {
                    return Err("Unsupported split InstallShield 5 cabinet".into());
                }
            }
            let size = if flags & 4 != 0 { compressed } else { expanded };
            let end = data_offset
                .checked_add(size)
                .ok_or("InstallShield payload overflow")?;
            let payload = data
                .get(data_offset..end)
                .ok_or("Truncated InstallShield payload")?;
            let decoded = if flags & 4 == 0 {
                charge(expanded, budget)?;
                payload.to_vec()
            } else {
                let mut result = Vec::new();
                let mut pos = 0;
                while pos < payload.len() {
                    let len = u16_at(payload, pos).ok_or("Truncated InstallShield block length")?
                        as usize;
                    if len == 0 {
                        return Err("Unsupported InstallShield old compression".into());
                    }
                    let end = pos
                        .checked_add(2 + len)
                        .ok_or("InstallShield block overflow")?;
                    let block = payload
                        .get(pos + 2..end)
                        .ok_or("Truncated InstallShield Deflate block")?;
                    let decoded = inflate(block, false, budget)?;
                    if result.len() + decoded.len() > expanded {
                        return Err("InstallShield expanded size mismatch".into());
                    }
                    result.extend_from_slice(&decoded);
                    pos = end;
                }
                result
            };
            let expected_md5 = if major == 5 {
                &descriptor[42..58]
            } else {
                &descriptor[26..42]
            };
            if decoded.len() != expanded || md5::compute(&decoded).0 != expected_md5 {
                return Err("InstallShield payload size or MD5 mismatch".into());
            }
            out.push(Cabinet {
                name,
                bytes: decoded,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put32(b: &mut [u8], p: usize, n: u32) {
        b[p..p + 4].copy_from_slice(&n.to_le_bytes());
    }
    fn fixture(compressed: bool) -> Vec<(String, Vec<u8>)> {
        use std::io::Write;
        let payload = b"MSCF synthetic cabinet bytes";
        let mut header = vec![0; 180];
        header[..4].copy_from_slice(b"ISc(");
        put32(&mut header, 4, 0x0100600c);
        put32(&mut header, 12, 32);
        put32(&mut header, 44, 48);
        put32(&mut header, 72, 1);
        let data = if compressed {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(payload).unwrap();
            let bytes = encoder.finish().unwrap();
            let mut block = (bytes.len() as u16).to_le_bytes().to_vec();
            block.extend(bytes);
            block
        } else {
            payload.to_vec()
        };
        header[80..82].copy_from_slice(&(if compressed { 4u16 } else { 0 }).to_le_bytes());
        header[82..90].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        header[90..98].copy_from_slice(&(data.len() as u64).to_le_bytes());
        header[98..106].copy_from_slice(&128u64.to_le_bytes());
        header[106..122].copy_from_slice(&md5::compute(payload).0);
        put32(&mut header, 138, 87);
        header[165..167].copy_from_slice(&2u16.to_le_bytes());
        header[167..173].copy_from_slice(b"x.cab\0");
        let mut volume = vec![0; 128];
        volume[..4].copy_from_slice(b"ISc(");
        put32(&mut volume, 4, 0x0100600c);
        volume.extend(data);
        vec![("data1.hdr".into(), header), ("data2.cab".into(), volume)]
    }
    #[test]
    fn stored_and_deflated_payloads_are_verified() {
        for compressed in [false, true] {
            let result = extract(&fixture(compressed), &mut 65536).unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].name, "x.cab");
            assert_eq!(result[0].bytes, b"MSCF synthetic cabinet bytes");
        }
    }
    #[test]
    fn missing_volume_tampering_and_truncation_are_rejected() {
        let mut entries = fixture(false);
        assert!(extract(&entries[..1], &mut 65536)
            .unwrap_err()
            .contains("Missing"));
        entries[1].1[128] ^= 1;
        assert!(extract(&entries, &mut 65536).unwrap_err().contains("MD5"));
        entries[1].1.truncate(129);
        assert!(extract(&entries, &mut 65536).is_err());
    }
    fn fixture5(compressed: bool) -> Vec<(String, Vec<u8>)> {
        let original = fixture(compressed);
        let d = &original[0].1[80..167];
        let mut header = vec![0; 152];
        header[..4].copy_from_slice(b"ISc(");
        put32(&mut header, 4, 0x01005201);
        put32(&mut header, 12, 32);
        put32(&mut header, 44, 48);
        put32(&mut header, 60, 1); // one directory
        put32(&mut header, 72, 1); // one file
        put32(&mut header, 84, 8); // file descriptor relative to table
        put32(&mut header, 88, 66); // name relative to table
        header[96..98].copy_from_slice(&d[0..2]);
        header[98..102].copy_from_slice(&d[2..6]);
        header[102..106].copy_from_slice(&d[10..14]);
        header[126..130].copy_from_slice(&d[18..22]);
        header[130..146].copy_from_slice(&d[26..42]);
        header[146..152].copy_from_slice(b"x.cab\0");
        let mut volume = original[1].1.clone();
        put32(&mut volume, 4, 0x01005201);
        vec![("data1.hdr".into(), header), ("data1.cab".into(), volume)]
    }
    #[test]
    fn version5_indexed_descriptors_stored_and_deflated() {
        for compressed in [false, true] {
            let result = extract(&fixture5(compressed), &mut 65536).unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].name, "x.cab");
            assert_eq!(result[0].bytes, b"MSCF synthetic cabinet bytes");
        }
    }
    #[test]
    fn version5_integrity_bad_indexes_and_split_volumes() {
        let original = fixture5(false);
        let mut bad = original.clone();
        bad[1].1[128] ^= 1;
        assert!(extract(&bad, &mut 65536).unwrap_err().contains("MD5"));
        assert!(extract(&original[..1], &mut 65536)
            .unwrap_err()
            .contains("Missing"));
        let mut bad = original.clone();
        put32(&mut bad[0].1, 84, u32::MAX);
        assert!(extract(&bad, &mut 65536).is_err());
        let mut bad = original.clone();
        put32(&mut bad[1].1, 28, 1);
        assert!(extract(&bad, &mut 65536).unwrap_err().contains("split"));
        let mut bad = original;
        put32(&mut bad[0].1, 12, u32::MAX);
        assert!(extract(&bad, &mut 65536).is_err());
    }
    fn stored_cab(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
        let table_len: usize = entries.iter().map(|(n, _)| 17 + n.len()).sum();
        let data_len: usize = entries.iter().map(|(_, b)| b.len()).sum();
        assert!(data_len <= 32768);
        let data_offset = 44 + table_len;
        let mut cab = vec![0u8; 44];
        cab[..4].copy_from_slice(b"MSCF");
        put32(&mut cab, 8, (data_offset + 8 + data_len) as u32);
        put32(&mut cab, 16, 44);
        cab[24] = 3;
        cab[25] = 1;
        cab[26..28].copy_from_slice(&1u16.to_le_bytes());
        cab[28..30].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        put32(&mut cab, 36, data_offset as u32);
        cab[40..42].copy_from_slice(&1u16.to_le_bytes());
        let mut offset = 0;
        for (name, bytes) in entries {
            cab.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            cab.extend_from_slice(&(offset as u32).to_le_bytes());
            cab.extend_from_slice(&[0; 8]);
            cab.extend_from_slice(name.as_bytes());
            cab.push(0);
            offset += bytes.len();
        }
        cab.extend_from_slice(&0u32.to_le_bytes());
        cab.extend_from_slice(&(data_len as u16).to_le_bytes());
        cab.extend_from_slice(&(data_len as u16).to_le_bytes());
        for (_, bytes) in entries {
            cab.extend_from_slice(bytes);
        }
        cab
    }

    #[test]
    fn microsoft_cab_preserves_installshield_headers_for_nested_extraction() {
        let inner = stored_cab(&[("GAME.000".into(), b"synthetic CE header".to_vec())]);
        let mut entries = fixture5(false);
        put32(&mut entries[0].1, 98, inner.len() as u32);
        put32(&mut entries[0].1, 102, inner.len() as u32);
        entries[0].1[130..146].copy_from_slice(&md5::compute(&inner).0);
        entries[1].1.truncate(128);
        entries[1].1.extend_from_slice(&inner);
        for (name, _) in &mut entries {
            *name = format!("\\Disk1\\{name}");
        }
        let outer = stored_cab(&entries);
        let result = super::super::extract(&outer).unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].bytes, inner);
    }
}
