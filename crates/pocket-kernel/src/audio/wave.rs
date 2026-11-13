//! Shared PCM and Microsoft ADPCM WAVE decoding.
use super::GuestFormat;
mod adpcm;

pub fn decode_pcm_wave(bytes: &[u8]) -> Option<(GuestFormat, Vec<i16>)> {
    let Some((fmt, data)) = parse_pcm_wave(bytes) else {
        return adpcm::decode(bytes);
    };
    let samples: Vec<i16> = match fmt.bits_per_sample {
        8 => data.iter().map(|&b| (b as i16 - 128) * 256).collect(),
        16 => data
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect(),
        _ => return None,
    };
    Some((fmt, samples))
}

fn parse_pcm_wave(bytes: &[u8]) -> Option<(GuestFormat, &[u8])> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut p = 12usize;
    let mut fmt = None;
    let mut data = None;
    while p + 8 <= bytes.len() {
        let id = &bytes[p..p + 4];
        let size = u32::from_le_bytes(bytes[p + 4..p + 8].try_into().ok()?) as usize;
        let end = p.checked_add(8)?.checked_add(size)?.min(bytes.len());
        if id == b"fmt " && size >= 16 && p + 24 <= bytes.len() {
            let tag = u16::from_le_bytes(bytes[p + 8..p + 10].try_into().ok()?);
            let channels = u16::from_le_bytes(bytes[p + 10..p + 12].try_into().ok()?);
            let rate = u32::from_le_bytes(bytes[p + 12..p + 16].try_into().ok()?);
            let bits = u16::from_le_bytes(bytes[p + 22..p + 24].try_into().ok()?);
            if tag != 1 || channels == 0 || rate == 0 || !matches!(bits, 8 | 16) {
                return None;
            }
            fmt = Some(GuestFormat {
                sample_rate: rate,
                channels,
                bits_per_sample: bits,
            });
        } else if id == b"data" {
            data = Some(&bytes[p + 8..end]);
        }
        p = p + 8 + size + (size & 1);
    }
    Some((fmt?, data?))
}
