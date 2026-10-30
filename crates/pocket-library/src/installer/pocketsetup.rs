//! PocketSetup PS2 streams and the legacy EC2 file catalogue.
//! Decode data only; neither the desktop stub nor the uninstaller is executed.
use super::*;
pub(super) fn extract(b: &[u8], budget: &mut usize) -> Result<Option<Vec<Cabinet>>> {
    if let Some(cabinets) = ec2_catalogue(b, budget)? {
        return Ok(Some(cabinets));
    }
    let marker = b"\x02ne1PS2\0";
    if !b.windows(11).any(|w| w == b"PocketSetup") || !b.windows(6).any(|w| w == b"|FILES") {
        return Ok(None);
    }
    let mut out = Vec::new();
    for (offset, _) in b
        .windows(marker.len())
        .enumerate()
        .filter(|(_, w)| *w == marker)
    {
        if out.len() >= 32 {
            return Err("Too many PocketSetup streams".into());
        }
        let mut pos = offset + marker.len();
        let mut data = Vec::new();
        for _ in 0..16384 {
            let raw = u32_at(b, pos).ok_or("Truncated PocketSetup block")? as u32 as i32;
            pos += 4;
            if raw == 0 {
                break;
            }
            let size = raw.checked_abs().ok_or("Invalid PocketSetup block")? as usize;
            if size > 65536 {
                return Err("PocketSetup block exceeds limit".into());
            }
            let end = pos.checked_add(size).ok_or("PocketSetup offset overflow")?;
            let block = b.get(pos..end).ok_or("Truncated PocketSetup block")?;
            let decoded = if raw < 0 {
                charge(size, budget)?;
                block.to_vec()
            } else {
                inflate(block, true, budget)?
            };
            if decoded.len() > 16384 {
                return Err("PocketSetup decoded block exceeds limit".into());
            }
            data.extend_from_slice(&decoded);
            pos = end;
            if data.starts_with(b"MSCF") && data.len() >= 12 {
                let expected = u32_at(&data, 8).unwrap();
                if expected > LIMIT || expected < 36 {
                    return Err("Invalid PocketSetup cabinet size".into());
                }
                if data.len() >= expected {
                    if data.len() != expected || u32_at(b, pos) != Some(0) {
                        return Err("PocketSetup cabinet boundary mismatch".into());
                    }
                    out.push(Cabinet {
                        name: format!("pocketsetup-{}.cab", out.len()),
                        bytes: data,
                    });
                    break;
                }
            }
        }
    }
    Ok(Some(out))
}

/// PocketSetup 2001 ends with four DWORDs; the third locates |FILES\0.
/// Each catalogue record contains a counted ANSI source path, a DOS timestamp,
/// EC2\0, and signed-length chunks (positive zlib, negative stored, zero end).
/// Follow those boundaries instead of scanning compressed bytes for signatures.
fn ec2_catalogue(b: &[u8], budget: &mut usize) -> Result<Option<Vec<Cabinet>>> {
    let Some(footer) = b.len().checked_sub(16) else { return Ok(None); };
    let Some(offset) = u32_at(b, footer + 8).filter(|offset| *offset < footer) else {
        return Ok(None);
    };
    let catalogue = &b[offset..footer];
    if !catalogue.starts_with(b"|FILES\0") {
        return Ok(None);
    }
    let mut pos = 7;
    let label_length = ec2_number(catalogue, &mut pos)?;
    if ec2_take(catalogue, &mut pos, label_length)? != b"PocketSetup" {
        return Ok(None);
    }
    let expected = ec2_number(catalogue, &mut pos)?;
    if expected > *budget {
        return Err("PocketSetup catalogue exceeds decoded size limit".into());
    }
    let count = ec2_number(catalogue, &mut pos)?;
    if count == 0 || count > 4096 {
        return Err("Invalid PocketSetup file count".into());
    }
    let mut total = 0usize;
    let mut blocks = 16384usize;
    let mut out = Vec::new();
    for index in 0..count {
        let length = ec2_number(catalogue, &mut pos)?;
        if length == 0 || length > 32768 {
            return Err("Invalid PocketSetup filename length".into());
        }
        let path = ec2_take(catalogue, &mut pos, length)?;
        let _timestamp = ec2_number(catalogue, &mut pos)?;
        if ec2_take(catalogue, &mut pos, 4)? != b"EC2\0" {
            return Err("Unsupported PocketSetup stream signature".into());
        }
        let mut data = Vec::new();
        loop {
            let signed = ec2_number(catalogue, &mut pos)? as u32 as i32;
            if signed == 0 { break; }
            blocks = blocks.checked_sub(1).ok_or("Too many PocketSetup blocks")?;
            let length = signed.checked_abs().ok_or("Invalid PocketSetup block length")? as usize;
            if length > 65536 || (signed < 0 && length > 16384) {
                return Err("PocketSetup block exceeds limit".into());
            }
            let block = ec2_take(catalogue, &mut pos, length)?;
            let decoded = if signed < 0 {
                charge(length, budget)?;
                block.to_vec()
            } else {
                // Limit each inflate before allocating a large decoded block.
                let mut block_budget = (*budget).min(16384);
                let decoded = inflate(block, true, &mut block_budget)?;
                charge(decoded.len(), budget)?;
                decoded
            };
            total = total.checked_add(decoded.len()).ok_or("PocketSetup size overflow")?;
            if total > expected {
                return Err("PocketSetup catalogue size mismatch".into());
            }
            data.extend_from_slice(&decoded);
        }
        if data.starts_with(b"MSCF") {
            if data.len() < 36 || u32_at(&data, 8) != Some(data.len()) {
                return Err("Invalid PocketSetup cabinet boundary".into());
            }
            if out.len() >= 32 { return Err("Too many PocketSetup cabinets".into()); }
            let path = String::from_utf8_lossy(path);
            let basename = path.rsplit(['\\', '/']).next().filter(|s| !s.is_empty());
            out.push(Cabinet {
                name: basename.map(str::to_owned).unwrap_or_else(|| format!("pocketsetup-{index}.cab")),
                bytes: data,
            });
        }
    }
    if pos != catalogue.len() || total != expected {
        return Err("PocketSetup catalogue boundary or size mismatch".into());
    }
    Ok(Some(out))
}
fn ec2_number(b: &[u8], pos: &mut usize) -> Result<usize> {
    let value = u32_at(b, *pos).ok_or("Truncated PocketSetup catalogue")?;
    *pos += 4;
    Ok(value)
}
fn ec2_take<'a>(b: &'a [u8], pos: &mut usize, length: usize) -> Result<&'a [u8]> {
    let end = pos.checked_add(length).ok_or("PocketSetup offset overflow")?;
    let data = b.get(*pos..end).ok_or("Truncated PocketSetup catalogue")?;
    *pos = end;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn compressed_and_raw_blocks_are_combined_and_boundaries_checked() {
        let mut cab = vec![0u8; 40];
        cab[..4].copy_from_slice(b"MSCF");
        cab[8..12].copy_from_slice(&40u32.to_le_bytes());
        let mut source = b"PocketSetup|FILES\x02ne1PS2\0".to_vec();
        source.extend_from_slice(&(-20i32).to_le_bytes());
        source.extend_from_slice(&cab[..20]);
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&cab[20..]).unwrap();
        let chunk = encoder.finish().unwrap();
        source.extend_from_slice(&(chunk.len() as i32).to_le_bytes());
        source.extend_from_slice(&chunk);
        source.extend_from_slice(&0u32.to_le_bytes());
        let output = extract(&source, &mut LIMIT.clone()).unwrap().unwrap();
        assert_eq!(output[0].bytes, cab);
        source.pop();
        assert!(extract(&source, &mut LIMIT.clone()).is_err());
        assert!(extract(&source, &mut 2).is_err());
    }
    fn tiny_cab() -> Vec<u8> {
        let mut bytes=vec![0u8;40];
        bytes[..4].copy_from_slice(b"MSCF");
        bytes[8..12].copy_from_slice(&40u32.to_le_bytes());
        bytes
    }
    fn ec2_package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes=vec![0u8;64];
        let offset=bytes.len();
        bytes.extend_from_slice(b"|FILES\0");
        bytes.extend_from_slice(&11u32.to_le_bytes());
        bytes.extend_from_slice(b"PocketSetup");
        let total:usize=entries.iter().map(|(_,data)|data.len()).sum();
        bytes.extend_from_slice(&(total as u32).to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (name,data) in entries {
            bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(&0x2b6949c3u32.to_le_bytes());
            bytes.extend_from_slice(b"EC2\0");
            for (i,chunk) in data.chunks(20).enumerate() {
                if i%2==0 {
                    bytes.extend_from_slice(&(-(chunk.len() as i32)).to_le_bytes());
                    bytes.extend_from_slice(chunk);
                } else {
                    let mut encoder=flate2::write::ZlibEncoder::new(Vec::new(),flate2::Compression::default());
                    encoder.write_all(chunk).unwrap();
                    let compressed=encoder.finish().unwrap();
                    bytes.extend_from_slice(&(compressed.len() as i32).to_le_bytes());
                    bytes.extend_from_slice(&compressed);
                }
            }
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        for value in [0u32,0,offset as u32,0] {bytes.extend_from_slice(&value.to_le_bytes());}
        bytes
    }
    #[test]
    fn ec2_catalogue_combines_chunks_and_ignores_auxiliary_payloads() {
        let cab=tiny_cab();
        let bytes=ec2_package(&[(r"c:\build\application.payload",&cab),
            (r"c:\tools\uninstaller.cab",b"MZ auxiliary EC2\0 not a cabinet"),
            ("icon.ico",&[0,0,1,0])]);
        let mut budget=LIMIT;
        let out=extract(&bytes,&mut budget).unwrap().unwrap();
        assert_eq!(out.len(),1);
        assert_eq!(out[0].name,"application.payload");
        assert_eq!(out[0].bytes,cab);
        assert_eq!(budget,LIMIT-cab.len()-b"MZ auxiliary EC2\0 not a cabinet".len()-4);
    }
    #[test]
    fn ec2_checks_catalogue_size_bounds_and_budget() {
        let cab=tiny_cab();
        let bytes=ec2_package(&[("game.cab",&cab)]);
        assert!(extract(&bytes,&mut 39).is_err());
        let mut wrong=bytes.clone();
        wrong[64+22..64+26].copy_from_slice(&41u32.to_le_bytes());
        assert!(extract(&wrong,&mut LIMIT.clone()).is_err());
        let mut truncated=bytes.clone();
        truncated.remove(truncated.len()-17);
        assert!(extract(&truncated,&mut LIMIT.clone()).is_err());
        let mut extra=bytes.clone();
        extra.insert(extra.len()-16,0x42);
        assert!(extract(&extra,&mut LIMIT.clone()).is_err());
    }
    #[test]
    fn ec2_does_not_accept_a_cabinet_with_the_wrong_declared_length() {
        let mut cab=tiny_cab();cab[8..12].copy_from_slice(&39u32.to_le_bytes());
        assert!(extract(&ec2_package(&[("game.cab",&cab)]),&mut LIMIT.clone()).is_err());
    }
    #[test]
    fn ec2_invalid_signed_lengths_and_inflate_expansion_are_bounded() {
        let cab=tiny_cab();
        let original=ec2_package(&[("game.cab",&cab)]);
        let start=original.windows(4).position(|p|p==b"EC2\0").unwrap()+4;
        for length in [i32::MIN,-16385,65537] {
            let mut bad=original.clone();bad[start..start+4].copy_from_slice(&length.to_le_bytes());
            assert!(extract(&bad,&mut LIMIT.clone()).is_err());
        }
        let mut encoder=flate2::write::ZlibEncoder::new(Vec::new(),flate2::Compression::default());
        encoder.write_all(&vec![0;16385]).unwrap();
        let compressed=encoder.finish().unwrap();
        let mut oversized=original[..start].to_vec();
        oversized[64+22..64+26].copy_from_slice(&16385u32.to_le_bytes());
        oversized.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        oversized.extend(compressed);oversized.extend_from_slice(&0u32.to_le_bytes());
        oversized.extend_from_slice(&original[original.len()-16..]);
        assert!(extract(&oversized,&mut LIMIT.clone()).is_err());
    }
    #[test]
    fn ec2_requires_the_footer_to_identify_its_file_catalogue() {
        let cab=tiny_cab();let mut bytes=ec2_package(&[("game.cab",&cab)]);
        let offset=bytes.len()-8;bytes[offset..offset+4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(extract(&bytes,&mut LIMIT.clone()).unwrap().unwrap().is_empty());
    }

}
