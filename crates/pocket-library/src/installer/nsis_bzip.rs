//! Legacy solid NSIS bzip2 blocks. The script is not executed or heuristically
//! interpreted: the decoded container is a header followed by length-prefixed data.
use super::{charge, u32_at, Cabinet, Result};
pub(super) fn extract(
    b: &[u8],
    base: usize,
    end: usize,
    budget: &mut usize,
) -> Result<Option<Vec<Cabinet>>> {
    let start = base.checked_add(28).ok_or("NSIS payload offset overflow")?;
    let sig = b.get(start..start + 4).ok_or("Truncated NSIS payload")?;
    let block = (usize::from(sig[1]) << 16) | (usize::from(sig[2]) << 8) | usize::from(sig[3]);
    if sig[0] != 0x31 || block >= 900000 {
        return Ok(None);
    }
    // A valid non-solid block frame belongs to the existing reader. A packed
    // length ending in 0x31 must not be mistaken for a solid bzip2 marker.
    if u32_at(b, start).is_some_and(|n| {
        (n & 0x7fff_ffff) > 0 && (n & 0x7fff_ffff) <= end.saturating_sub(start + 4)
    }) {
        return Ok(None);
    }
    let flags = u32_at(b, base).ok_or("Truncated NSIS flags")?;
    // FH_FLAGS_UNINSTALL=1, FH_FLAGS_NO_CRC=4. Bit 1 does not imply a
    // second trailing CRC. The checksum covers the file after its first512 bytes.
    let payload_end = if flags & 4 == 0 {
        let crc_at = end
            .checked_sub(4)
            .filter(|n| *n >= start && *n >= 512)
            .ok_or("Truncated NSIS CRC")?;
        let mut crc = flate2::Crc::new();
        crc.update(&b[512..crc_at]);
        if u32_at(b, crc_at) != Some(crc.sum() as usize) {
            return Err("NSIS installer CRC mismatch".into());
        }
        crc_at
    } else {
        end
    };
    let expected = u32_at(b, base + 20).ok_or("Truncated NSIS header size")?;
    let mut decoded = None;
    for randomized in [false, true] {
        if let Ok(data) =
            newtua_nsis::decode_legacy_bzip2(&b[start..payload_end], randomized, *budget)
        {
            if u32_at(&data, 0) == Some(expected) {
                decoded = Some(data);
                break;
            }
        }
    }
    let data = decoded.ok_or("Unsupported or corrupt NSIS solid bzip2 stream")?;
    charge(data.len(), budget)?;
    let mut pos = 4usize
        .checked_add(expected)
        .filter(|p| *p <= data.len())
        .ok_or("Invalid NSIS decoded header bounds")?;
    let mut out = Vec::new();
    let mut count = 0;
    while pos < data.len() {
        count += 1;
        if count > 4096 {
            return Err("Too many NSIS solid blocks".into());
        }
        let size = u32_at(&data, pos).ok_or("Truncated NSIS block length")?;
        let next = pos
            .checked_add(4)
            .and_then(|p| p.checked_add(size))
            .filter(|p| *p <= data.len())
            .ok_or("Invalid NSIS solid block bounds")?;
        let payload = &data[pos + 4..next];
        if payload.starts_with(b"MSCF") {
            if out.len() >= 32 {
                return Err("Too many NSIS cabinets".into());
            }
            charge(payload.len(), budget)?;
            out.push(Cabinet {
                name: format!("nsis-solid-block-{count}.cab"),
                bytes: payload.to_vec(),
            });
        }
        pos = next;
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &[u8] = include_bytes!("../../tests/data/nsis1-bzip-synthetic.exe");
    #[test]
    fn legacy_solid_bzip_recovers_cab_and_checks_crc_and_budget() {
        let mut budget = 65536;
        let result = extract(SAMPLE, 512, SAMPLE.len(), &mut budget)
            .unwrap()
            .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].bytes.starts_with(b"MSCF"));
        assert_eq!(u32_at(&result[0].bytes, 8), Some(result[0].bytes.len()));
        assert!(extract(SAMPLE, 512, SAMPLE.len(), &mut 16).is_err());
        let mut bad = SAMPLE.to_vec();
        let n = bad.len();
        bad[n - 1] ^= 1;
        assert!(extract(&bad, 512, bad.len(), &mut 65536)
            .unwrap_err()
            .contains("CRC"));
    }
    #[test]
    fn strict_outer_cabinet_pipeline_and_truncation() {
        assert_eq!(super::super::extract(SAMPLE).unwrap().unwrap().len(), 1);
        assert!(extract(
            &SAMPLE[..SAMPLE.len() - 1],
            512,
            SAMPLE.len() - 1,
            &mut 65536
        )
        .is_err());
    }
}
