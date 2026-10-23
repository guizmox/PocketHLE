//! Wise raw-stream overlay profile. Decode sequential streams, never execute script.
use super::*;
fn stream(b: &[u8], pos: &mut usize, budget: &mut usize) -> Result<Vec<u8>> {
    let input = b.get(*pos..).ok_or("Truncated Wise stream")?;
    let mut decoder = flate2::bufread::DeflateDecoder::new(input);
    let mut data = Vec::new();
    (&mut decoder)
        .take(*budget as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    charge(data.len(), budget)?;
    let end = pos
        .checked_add(decoder.total_in() as usize)
        .ok_or("Wise offset overflow")?;
    let crc = u32_at(b, end).ok_or("Missing Wise stream CRC")? as u32;
    let mut actual = flate2::Crc::new();
    actual.update(&data);
    if actual.sum() != crc {
        return Err("Wise stream CRC mismatch".into());
    }
    *pos = end.checked_add(4).ok_or("Wise offset overflow")?;
    Ok(data)
}
pub(super) fn extract(
    b: &[u8],
    overlay: usize,
    budget: &mut usize,
) -> Result<Option<Vec<Cabinet>>> {
    if !b.windows(11).any(|w| w == b"GLBSInstall") {
        return Ok(None);
    }
    let h = b.get(overlay..).ok_or("Missing Wise overlay")?;
    if h.len() < 92 {
        return Err("Truncated Wise overlay".into());
    }
    // Other Wise dialects need their own validated header, not guessed offsets.
    if h[0] != 0 {
        return Err("Wise DLL-name header profile not supported".into());
    }
    if u32_at(h, 1).unwrap() & 0x100 != 0 {
        return Err("Wise ZIP profile requires a complete ZIP directory".into());
    }
    if u32_at(h, 77) != Some(b.len()) {
        return Err("Wise executable size mismatch".into());
    }
    let text_len = h[91] as usize;
    let mut pos = overlay
        .checked_add(92 + text_len)
        .ok_or("Wise offset overflow")?;
    for (span_field, size_field) in [(81, 85), (29, 25)] {
        let start = pos;
        let data = stream(b, &mut pos, budget)?;
        if pos - start != u32_at(h, span_field).unwrap()
            || data.len() != u32_at(h, size_field).unwrap()
        {
            return Err("Wise script/bitmap size mismatch".into());
        }
    }
    let mut out = Vec::new();
    let mut count = 0;
    while pos < b.len() {
        count += 1;
        if count > 4096 {
            return Err("Too many Wise streams".into());
        }
        let data = stream(b, &mut pos, budget)?;
        if data.starts_with(b"MSCF") {
            out.push(Cabinet {
                name: format!("wise-payload-{count}.cab"),
                bytes: data,
            });
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn encoded(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        let mut out = e.finish().unwrap();
        let mut crc = flate2::Crc::new();
        crc.update(data);
        out.extend_from_slice(&crc.sum().to_le_bytes());
        out
    }
    #[test]
    fn stream_consumption_crc_and_truncation_are_checked() {
        let first = encoded(b"first");
        let mut bytes = first.clone();
        bytes.extend_from_slice(&encoded(b"second"));
        let mut pos = 0;
        assert_eq!(
            stream(&bytes, &mut pos, &mut LIMIT.clone()).unwrap(),
            b"first"
        );
        assert_eq!(pos, first.len());
        assert_eq!(
            stream(&bytes, &mut pos, &mut LIMIT.clone()).unwrap(),
            b"second"
        );
        assert_eq!(pos, bytes.len());
        bytes[first.len() - 1] ^= 1;
        assert!(stream(&bytes, &mut 0, &mut LIMIT.clone()).is_err());
        assert!(stream(&first[..first.len() - 1], &mut 0, &mut LIMIT.clone()).is_err());
    }
}
