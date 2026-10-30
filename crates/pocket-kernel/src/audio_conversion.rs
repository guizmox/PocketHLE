//! Process-owned synchronous ACM streams. Conversion does not open an output
//! device or consume a waveOut voice, so platform voice budgets do not apply.
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub channels: u16,
    pub rate: u32,
    pub bits: u16,
}
impl Format {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let word = |i| Some(u16::from_le_bytes(bytes.get(i..i + 2)?.try_into().ok()?));
        let dword = |i| Some(u32::from_le_bytes(bytes.get(i..i + 4)?.try_into().ok()?));
        if word(0)? != 1 {
            return None;
        }
        let result = Self {
            channels: word(2)?,
            rate: dword(4)?,
            bits: word(14)?,
        };
        if result.channels == 0 || result.rate == 0 || !matches!(result.bits, 8 | 16) {
            return None;
        }
        if result.frame_bytes() != u32::from(word(12)?)
            || result.rate.checked_mul(result.frame_bytes())? != dword(8)?
        {
            return None;
        }
        Some(result)
    }
    pub fn frame_bytes(self) -> u32 {
        u32::from(self.channels) * u32::from(self.bits / 8)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub source: u32,
    pub source_length: u32,
    pub destination: u32,
    pub destination_length: u32,
}
pub struct Stream {
    pub source: Format,
    pub destination: Format,
    pub headers: HashMap<u32, Prepared>,
}
impl Stream {
    pub fn new(source: Format, destination: Format) -> Option<Self> {
        if source.channels != destination.channels || source.rate != destination.rate {
            return None;
        }
        Some(Self {
            source,
            destination,
            headers: HashMap::new(),
        })
    }
    /// Conservative frame-aligned estimate, with checked guest DWORD arithmetic.
    pub fn size(&self, bytes: u32, destination_input: bool) -> Option<u32> {
        let (input, output) = if destination_input {
            (self.destination.frame_bytes(), self.source.frame_bytes())
        } else {
            (self.source.frame_bytes(), self.destination.frame_bytes())
        };
        bytes.div_ceil(input).checked_mul(output)
    }
    pub fn convert(&self, source: &[u8], capacity: u32) -> (u32, Vec<u8>) {
        let frames = (source.len() / self.source.frame_bytes() as usize)
            .min((capacity / self.destination.frame_bytes()) as usize);
        let used = frames * self.source.frame_bytes() as usize;
        let input = &source[..used];
        let output = match (self.source.bits, self.destination.bits) {
            (8, 16) => input
                .iter()
                .flat_map(|&sample| ((i16::from(sample) - 128) << 8).to_le_bytes())
                .collect(),
            (16, 8) => input
                .chunks_exact(2)
                .map(|s| ((i32::from(i16::from_le_bytes([s[0], s[1]])) + 32768) >> 8) as u8)
                .collect(),
            _ => input.to_vec(),
        };
        (used as u32, output)
    }
}
#[derive(Default)]
pub struct Streams {
    next: u32,
    pub entries: HashMap<u32, Stream>,
}
impl Streams {
    pub fn insert(&mut self, stream: Stream) -> Option<u32> {
        let next = self.next.checked_add(1)?;
        let handle = 0xac00_0000u32.checked_add(next)?;
        self.next = next;
        self.entries.insert(handle, stream);
        Some(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn format(bits: u16, channels: u16) -> Format {
        Format {
            channels,
            rate: 22050,
            bits,
        }
    }
    #[test]
    fn pcm_endpoints_silence_and_frame_aligned_partial_buffers() {
        let stream = Stream::new(format(16, 1), format(8, 1)).unwrap();
        let (used, output) = stream.convert(&[0, 128, 0, 0, 255, 127, 99], 3);
        assert_eq!((used, output), (6, vec![0, 128, 255]));
        let stream = Stream::new(format(8, 2), format(16, 2)).unwrap();
        assert_eq!(
            stream.convert(&[0, 128, 255, 127], 7),
            (2, vec![0, 128, 0, 0])
        );
    }
    #[test]
    fn size_queries_round_up_and_reject_overflow() {
        let stream = Stream::new(format(8, 2), format(16, 2)).unwrap();
        assert_eq!(stream.size(3, false), Some(8));
        assert_eq!(stream.size(7, true), Some(4));
        assert_eq!(stream.size(u32::MAX, false), None);
        assert!(Stream::new(format(8, 1), format(16, 2)).is_none());
    }
    #[test]
    fn handles_are_local_to_each_conversion_table() {
        let mut a = Streams::default();
        let b = Streams::default();
        let handle = a
            .insert(Stream::new(format(8, 1), format(16, 1)).unwrap())
            .unwrap();
        assert!(a.entries.contains_key(&handle));
        assert!(!b.entries.contains_key(&handle));
        a.entries.remove(&handle);
        let next = a
            .insert(Stream::new(format(8, 1), format(8, 1)).unwrap())
            .unwrap();
        assert_ne!(next, handle);
    }
}
