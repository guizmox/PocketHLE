//! PocketSetup PS2 chunked streams, including uncompressed fallback blocks.
use super::*;
pub(super) fn extract(b: &[u8], budget: &mut usize) -> Result<Option<Vec<Cabinet>>> {
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
}
