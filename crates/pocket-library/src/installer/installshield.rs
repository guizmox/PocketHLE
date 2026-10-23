//! InstallShield 6–13 cabinet sets, including length-prefixed Deflate blocks.
//! Layout reference: Unshield (MIT); see the bundled third-party notice.
use super::{charge, inflate, u16_at, u32_at, Cabinet, Result};

fn number(b: &[u8], p: usize) -> Result<usize> {
    u32_at(b, p).ok_or_else(|| "Truncated InstallShield metadata".into())
}
fn wide(b: &[u8], p: usize) -> Result<usize> {
    let value = u64::from_le_bytes(b.get(p..p + 8).ok_or("Truncated InstallShield size")?.try_into().unwrap());
    usize::try_from(value).map_err(|_| "InstallShield size exceeds host address space".into())
}

pub(super) fn extract(entries: &[(String, Vec<u8>)], budget: &mut usize) -> Result<Vec<Cabinet>> {
    let mut out = Vec::new();
    let headers: Vec<_> = entries.iter().filter(|(n, b)|
        n.to_ascii_lowercase().ends_with(".hdr") && b.starts_with(b"ISc(")
    ).collect();
    if headers.is_empty() { return Err("InstallShield cabinet set is missing its .hdr file".into()); }
    for (header_name, header) in headers {
        let version = number(header, 4)?;
        let major = if version >> 24 == 1 { (version >> 12) & 15 } else { (version & 0xffff) / 100 };
        if !(6..=13).contains(&major) { return Err(format!("Unsupported InstallShield cabinet version {major}")); }
        let base = number(header, 12)?;
        let table = base.checked_add(number(header, base + 12)?).ok_or("InstallShield table overflow")?;
        let count = number(header, base + 40)?;
        let descriptors = table.checked_add(number(header, base + 44)?).ok_or("InstallShield descriptor overflow")?;
        if count > 4096 { return Err("Too many InstallShield entries".into()); }
        let prefix = header_name.rsplit_once('.').ok_or("Invalid InstallShield header name")?.0;
        let prefix = prefix.trim_end_matches(|c: char| c.is_ascii_digit());
        for i in 0..count {
            let p = descriptors.checked_add(i * 87).ok_or("InstallShield descriptor overflow")?;
            let descriptor = header.get(p..p + 87).ok_or("Truncated InstallShield descriptor")?;
            let flags = u16_at(descriptor, 0).unwrap();
            let name_offset = number(descriptor, 58)?;
            let data_offset = wide(descriptor, 18)?;
            if flags & 8 != 0 || name_offset == 0 || data_offset == 0 { continue; }
            let start = table.checked_add(name_offset).ok_or("InstallShield name overflow")?;
            let name = header.get(start..).ok_or("Invalid InstallShield filename")?;
            let end = name.iter().position(|b| *b == 0).ok_or("Unterminated InstallShield filename")?;
            let name = String::from_utf8_lossy(&name[..end]).into_owned();
            if !name.to_ascii_lowercase().ends_with(".cab") { continue; }
            // Split/obfuscated/linked payloads need different decoding. Never
            // silently concatenate volumes or substitute a neighbouring file.
            if flags & !4 != 0 || descriptor[84] != 0 {
                return Err("Unsupported split, obfuscated or linked InstallShield cabinet".into());
            }
            let expanded = wide(descriptor, 2)?;
            let compressed = wide(descriptor, 10)?;
            if expanded > *budget { return Err("InstallShield payload exceeds size limit".into()); }
            let volume = u16_at(descriptor, 85).unwrap();
            let volume_name = format!("{prefix}{volume}.cab");
            let (_, data) = entries.iter().find(|(n, b)| n.eq_ignore_ascii_case(&volume_name) && b.starts_with(b"ISc("))
                .ok_or_else(|| format!("Missing InstallShield volume {volume_name}"))?;
            if number(data, 4)? != version { return Err("InstallShield volume version mismatch".into()); }
            let size = if flags & 4 != 0 { compressed } else { expanded };
            let end = data_offset.checked_add(size).ok_or("InstallShield payload overflow")?;
            let payload = data.get(data_offset..end).ok_or("Truncated InstallShield payload")?;
            let decoded = if flags & 4 == 0 {
                charge(expanded, budget)?;
                payload.to_vec()
            } else {
                let mut result = Vec::new();
                let mut pos = 0;
                while pos < payload.len() {
                    let len = u16_at(payload, pos).ok_or("Truncated InstallShield block length")? as usize;
                    if len == 0 { return Err("Unsupported InstallShield old compression".into()); }
                    let end = pos.checked_add(2 + len).ok_or("InstallShield block overflow")?;
                    let block = payload.get(pos + 2..end).ok_or("Truncated InstallShield Deflate block")?;
                    let decoded = inflate(block, false, budget)?;
                    if result.len() + decoded.len() > expanded { return Err("InstallShield expanded size mismatch".into()); }
                    result.extend_from_slice(&decoded);
                    pos = end;
                }
                result
            };
            if decoded.len() != expanded || md5::compute(&decoded).0 != descriptor[26..42] {
                return Err("InstallShield payload size or MD5 mismatch".into());
            }
            out.push(Cabinet { name, bytes: decoded });
        }
    }
    Ok(out)
}
