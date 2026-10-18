//! The CE CRT uses the system ANSI code page (GetACP returns 1252).
//! Keep every narrow byte reversible through the formatter's Unicode buffer;
//! interpreting narrow strings as UTF-8 destroys accented asset filenames.

const C1: [char; 32] = [
    '\u{20ac}', '\u{0081}', '\u{201a}', '\u{0192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02c6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008d}', '\u{017d}', '\u{008f}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02dc}', '\u{2122}', '\u{0161}', '\u{203a}', '\u{0153}', '\u{009d}', '\u{017e}', '\u{0178}',
];

pub(crate) fn decode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9f => C1[(b - 0x80) as usize],
            _ => char::from(b),
        })
        .collect()
}

pub(crate) fn encode(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            let code = c as u32;
            if code < 0x80 || (0xa0..=0xff).contains(&code) {
                code as u8
            } else {
                C1.iter()
                    .position(|&candidate| candidate == c)
                    .map_or(b'?', |i| i as u8 + 0x80)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_narrow_byte_round_trips_without_utf8_expansion() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(encode(&decode(&bytes)), bytes);
        // Even valid UTF-8 byte sequences must remain byte-for-byte unchanged.
        assert_eq!(encode(&decode("ären €".as_bytes())), "ären €".as_bytes());
    }

    #[test]
    fn ansi_paths_and_wide_fallback_use_the_system_code_page() {
        assert_eq!(decode(b"\xe4ren.lsf"), "ären.lsf");
        assert_eq!(encode("ären.lsf"), b"\xe4ren.lsf");
        assert_eq!(decode(b"\x80\x9c\x9f"), "€œŸ");
        assert_eq!(encode("€œŸ"), b"\x80\x9c\x9f");
        assert_eq!(encode("\u{4e2d}"), b"?");
    }
}
