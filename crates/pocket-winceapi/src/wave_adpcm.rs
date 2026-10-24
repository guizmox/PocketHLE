//! Bounded Microsoft ADPCM WAVE decoding, shared by PlaySound and HSS.
use pocket_kernel::audio::GuestFormat;
const LIMIT: usize = 64 * 1024 * 1024; // decoded i16 samples
const ADAPT: [i64; 16] = [
    230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
];
fn word(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(p..p.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn dword(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(p..p.checked_add(4)?)?.try_into().ok()?,
    ))
}
#[derive(Clone, Copy, Default)]
struct Channel {
    a: i64,
    b: i64,
    delta: i64,
    s1: i64,
    s2: i64,
}
impl Channel {
    fn next(&mut self, nibble: u8) -> i16 {
        let signed = if nibble < 8 {
            i64::from(nibble)
        } else {
            i64::from(nibble) - 16
        };
        let sample = (((self.s1 * self.a + self.s2 * self.b) >> 8) + signed * self.delta)
            .clamp(-32768, 32767);
        self.s2 = self.s1;
        self.s1 = sample;
        self.delta = (self.delta * ADAPT[nibble as usize] / 256).clamp(16, i64::from(i32::MAX));
        sample as i16
    }
}
pub(crate) fn decode(bytes: &[u8]) -> Option<(GuestFormat, Vec<i16>)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let end = usize::try_from(dword(bytes, 4)?).ok()?.checked_add(8)?;
    if end > bytes.len() || end < 12 {
        return None;
    }
    let mut p = 12usize;
    let mut fmt = None;
    let mut data = None;
    let mut fact = None;
    while p.checked_add(8)? <= end {
        let n = dword(bytes, p + 4)? as usize;
        let start = p + 8;
        let next = start.checked_add(n)?;
        if next > end {
            return None;
        }
        match &bytes[p..p + 4] {
            b"fmt " => fmt = Some(&bytes[start..next]),
            b"data" => data = Some(&bytes[start..next]),
            b"fact" if n >= 4 => fact = Some(dword(bytes, start)? as usize),
            _ => {}
        }
        p = next.checked_add(n & 1)?;
    }
    let fmt = fmt?;
    let data = data?;
    if word(fmt, 0)? != 2 || word(fmt, 14)? != 4 {
        return None;
    }
    let channels = usize::from(word(fmt, 2)?);
    let rate = dword(fmt, 4)?;
    if !(1..=2).contains(&channels) || rate == 0 {
        return None;
    }
    let align = usize::from(word(fmt, 12)?);
    let frames = usize::from(word(fmt, 18)?);
    let coefficients = usize::from(word(fmt, 20)?);
    let extra = usize::from(word(fmt, 16)?);
    if coefficients == 0
        || coefficients > 256
        || extra < 4 + coefficients * 4
        || fmt.len() < 18 + extra
        || align < 7 * channels
        || frames < 2
        || frames > 2 + (align - 7 * channels) * 2 / channels
    {
        return None;
    }
    let mut table = Vec::with_capacity(coefficients);
    for i in 0..coefficients {
        table.push((
            i64::from(word(fmt, 22 + i * 4)? as i16),
            i64::from(word(fmt, 24 + i * 4)? as i16),
        ));
    }
    let blocks = data.len().div_ceil(align);
    if blocks.checked_mul(frames)?.checked_mul(channels)? > LIMIT {
        return None;
    }
    let mut out = Vec::new();
    for block in data.chunks(align) {
        if block.len() < 7 * channels {
            return None;
        }
        let available = (2 + (block.len() - 7 * channels) * 2 / channels).min(frames);
        let mut states = [Channel::default(); 2];
        for c in 0..channels {
            let &(a, b) = table.get(block[c] as usize)?;
            states[c] = Channel {
                a,
                b,
                delta: i64::from(word(block, channels + 2 * c)?).max(16),
                s1: i64::from(word(block, 3 * channels + 2 * c)? as i16),
                s2: i64::from(word(block, 5 * channels + 2 * c)? as i16),
            };
        }
        for ch in &states[..channels] {
            out.push(ch.s2 as i16);
        }
        for ch in &states[..channels] {
            out.push(ch.s1 as i16);
        }
        let mut produced = 2usize;
        for &byte in &block[7 * channels..] {
            for (c, nibble) in [(0, byte >> 4), (channels - 1, byte & 15)] {
                if produced >= available {
                    break;
                }
                out.push(states[c].next(nibble));
                if c == channels - 1 {
                    produced += 1;
                }
            }
        }
    }
    if let Some(frames) = fact {
        let samples = frames.checked_mul(channels)?;
        if samples > out.len() {
            return None;
        }
        out.truncate(samples);
    }
    Some((
        GuestFormat {
            sample_rate: rate,
            channels: channels as u16,
            bits_per_sample: 16,
        },
        out,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wav(ch: u16, block: &[u8], frames: u16, fact: u32) -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend(2u16.to_le_bytes());
        fmt.extend(ch.to_le_bytes());
        fmt.extend(8000u32.to_le_bytes());
        fmt.extend(0u32.to_le_bytes());
        fmt.extend((block.len() as u16).to_le_bytes());
        fmt.extend(4u16.to_le_bytes());
        fmt.extend(8u16.to_le_bytes());
        fmt.extend(frames.to_le_bytes());
        fmt.extend(1u16.to_le_bytes());
        fmt.extend(256i16.to_le_bytes());
        fmt.extend(0i16.to_le_bytes());
        let mut b = b"RIFF\0\0\0\0WAVE".to_vec();
        for (id, payload) in [
            (b"fmt ", fmt),
            (b"fact", fact.to_le_bytes().to_vec()),
            (b"data", block.to_vec()),
        ] {
            b.extend(id);
            b.extend((payload.len() as u32).to_le_bytes());
            b.extend(&payload);
            if payload.len() % 2 != 0 {
                b.push(0);
            }
        }
        let size = (b.len() - 8) as u32;
        b[4..8].copy_from_slice(&size.to_le_bytes());
        b
    }
    #[test]
    fn mono_and_stereo_have_correct_seed_order_nibbles_and_fact_length() {
        let mono = wav(1, &[0, 16, 0, 0xe8, 3, 0x84, 3, 0x12], 4, 4);
        assert_eq!(decode(&mono).unwrap().1, [900, 1000, 1016, 1048]);
        let stereo = wav(
            2,
            &[
                0, 0, 16, 0, 16, 0, 0xe8, 3, 0x18, 0xfc, 0x84, 3, 0x7c, 0xfc, 0x1f,
            ],
            3,
            3,
        );
        assert_eq!(
            decode(&stereo).unwrap().1,
            [900, -900, 1000, -1000, 1016, -1016]
        );
        let trimmed = wav(1, &[0, 16, 0, 0xe8, 3, 0x84, 3, 0x12], 4, 3);
        assert_eq!(decode(&trimmed).unwrap().1, [900, 1000, 1016]);
    }
    #[test]
    fn negative_predictor_rounds_like_the_ms_adpcm_arithmetic_shift() {
        let mut b = wav(1, &[0, 16, 0, 255, 255, 0, 0, 0], 4, 4);
        b[42..44].copy_from_slice(&128i16.to_le_bytes());
        assert_eq!(decode(&b).unwrap().1, [0, -1, -1, -1]);
    }

    #[test]
    fn bad_predictor_header_and_truncation_are_rejected() {
        let b = wav(1, &[1, 16, 0, 0, 0, 0, 0, 0], 4, 4);
        assert!(decode(&b).is_none());
        let b = wav(1, &[0, 16, 0, 0, 0, 0, 0, 0], 4, 4);
        assert!(decode(&b[..b.len() - 1]).is_none());
        let bad = wav(1, &[0, 16, 0, 0, 0, 0, 0, 0], 100, 4);
        assert!(decode(&bad).is_none());
    }
}
