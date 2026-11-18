//! Owned host zlib streams for WinCE's 32-bit z_stream ABI.
//! Unfinished Business loads its CFL assets through zlib.dll ordinals 22/19/20.
use flate2::Compression;
use miniz_oxide::inflate::{
    core::{decompress, inflate_flags, DecompressorOxide},
    TINFLStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;

const FIRST_HANDLE: u32 = 0x6e180000;
const WINDOW: usize = 32768;

#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    pub consumed: usize,
    pub written: usize,
    pub status: i32,
    pub adler: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct Stream {
    decoder: Box<DecompressorOxide>,
    /// The last 32 KiB, plus at most one batch awaiting compaction. Passing the
    /// exact output boundary avoids consuming the zlib trailer ahead of a PNG
    /// client's final row. No compressed-input or lifetime-output journal exists.
    #[serde(with = "serde_bytes")]
    window: Vec<u8>,
    header: Option<Vec<u8>>,
    terminal: Option<i32>,
    adler: u32,
}
impl Default for Stream {
    fn default() -> Self {
        Self {
            decoder: Box::default(),
            window: Vec::new(),
            header: Some(Vec::new()),
            terminal: None,
            adler: 1,
        }
    }
}
impl Stream {
    fn inflate(&mut self, input: &[u8], output: &mut [u8], finish: bool) -> Step {
        let mut step = Step {
            consumed: 0,
            written: 0,
            status: -5,
            adler: self.adler,
        };
        if output.is_empty() {
            return step;
        }
        if let Some(status) = self.terminal {
            step.status = status;
            return step;
        }
        // The guest ABI has no inflateSetDictionary handler. Preserve zlib's
        // NEED_DICT negotiation (including the requested dictionary checksum),
        // rather than reporting malformed input or inventing a dictionary.
        if let Some(header) = &mut self.header {
            let mut prefix = header.clone();
            prefix.extend(input.iter().take(2usize.saturating_sub(prefix.len())));
            if prefix.len() >= 2
                && prefix[0] & 15 == 8
                && prefix[0] >> 4 <= 7
                && u16::from_be_bytes([prefix[0], prefix[1]]) % 31 == 0
                && prefix[1] & 32 != 0
            {
                step.consumed = input.len().min(6 - header.len());
                header.extend_from_slice(&input[..step.consumed]);
                step.status = if header.len() == 6 {
                    2
                } else if step.consumed != 0 && !finish {
                    0
                } else {
                    -5
                };
                if header.len() == 6 {
                    self.adler = u32::from_be_bytes(header[2..6].try_into().unwrap());
                    step.adler = self.adler;
                }
                return step;
            }
        }
        let flags = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
            | inflate_flags::TINFL_FLAG_COMPUTE_ADLER32
            | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
            // Z_FINISH is retryable in zlib. Do not irreversibly mark a short
            // input as corrupt; translate unfinished progress to Z_BUF_ERROR.
            | inflate_flags::TINFL_FLAG_HAS_MORE_INPUT;
        loop {
            let start = self.window.len();
            let capacity = (output.len() - step.written).min(2 * WINDOW - start);
            if self.window.capacity() < 2 * WINDOW {
                self.window.reserve_exact(2 * WINDOW - start);
            }
            self.window.resize(start + capacity, 0);
            let (status, consumed, written) = decompress(
                &mut self.decoder,
                &input[step.consumed..],
                &mut self.window,
                start,
                flags,
            );
            if let Some(header) = &mut self.header {
                header.extend(
                    input[step.consumed..step.consumed + consumed]
                        .iter()
                        .take(2usize.saturating_sub(header.len())),
                );
                if header.len() >= 2 {
                    self.header = None;
                }
            }
            output[step.written..step.written + written]
                .copy_from_slice(&self.window[start..start + written]);
            self.window.truncate(start + written);
            // Compact once per 32 KiB of output, not once per small PNG row.
            if self.window.len() >= 2 * WINDOW {
                let offset = self.window.len() - WINDOW;
                self.window.copy_within(offset.., 0);
                self.window.truncate(WINDOW);
            }
            step.consumed += consumed;
            step.written += written;
            if (status as i32) < 0 {
                self.adler = miniz_oxide::mz_adler32_oxide(
                    self.adler,
                    &output[step.written - written..step.written],
                );
            } else if let Some(adler) = self.decoder.adler32() {
                self.adler = adler;
            }
            step.adler = self.adler;
            match status {
                TINFLStatus::Done => {
                    self.terminal = Some(1);
                    step.status = 1;
                    break;
                }
                TINFLStatus::NeedsMoreInput | TINFLStatus::HasMoreOutput => {
                    if step.written < output.len() && (consumed != 0 || written != 0) {
                        continue;
                    }
                    step.status = if finish || step.consumed == 0 && step.written == 0 {
                        -5
                    } else {
                        0
                    };
                    break;
                }
                _ => {
                    self.terminal = Some(-3);
                    step.status = -3;
                    break;
                }
            }
        }
        step
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ZlibStreams {
    next: u32,
    streams: HashMap<u32, Stream>,
}
impl ZlibStreams {
    pub(crate) fn capture_snapshot(&self) -> Self {
        self.clone()
    }

    pub(crate) fn restore_snapshot(self) -> Result<Self, String> {
        if self.streams.len() > 1024
            || (self.next == 0 && !self.streams.is_empty())
            || (self.next != 0 && self.next < FIRST_HANDLE)
            || self.streams.iter().any(|(&handle, stream)| {
                handle < FIRST_HANDLE
                    || stream.window.len() >= 2 * WINDOW
                    || stream
                        .header
                        .as_ref()
                        .is_some_and(|header| header.len() > 6)
                    || stream
                        .terminal
                        .is_some_and(|status| status != 1 && status != -3)
            })
        {
            return Err("invalid saved zlib stream state".into());
        }
        Ok(self)
    }

    pub fn init(&mut self) -> Option<u32> {
        if self.streams.len() >= 1024 {
            return None;
        }
        let mut handle = self.next.max(FIRST_HANDLE);
        while self.streams.contains_key(&handle) {
            handle = handle.wrapping_add(1).max(FIRST_HANDLE);
        }
        self.next = handle.wrapping_add(1).max(FIRST_HANDLE);
        self.streams.insert(handle, Stream::default());
        Some(handle)
    }
    pub fn contains(&self, handle: u32) -> bool {
        self.streams.contains_key(&handle)
    }
    pub fn inflate(
        &mut self,
        handle: u32,
        input: &[u8],
        output: &mut [u8],
        flush: u32,
    ) -> Result<Step, i32> {
        if flush > 4 {
            return Err(-2);
        }
        Ok(self
            .streams
            .get_mut(&handle)
            .ok_or(-2)?
            .inflate(input, output, flush == 4))
    }
    pub fn reset(&mut self, handle: u32) -> bool {
        let Some(stream) = self.streams.get_mut(&handle) else {
            return false;
        };
        *stream = Stream::default();
        true
    }
    pub fn end(&mut self, handle: u32) -> bool {
        self.streams.remove(&handle).is_some()
    }
    pub fn compress(input: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input)?;
        encoder.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_sized_rows_do_not_consume_all_input_before_output_is_drained() {
        let pixels: Vec<u8> = (0..15)
            .flat_map(|row| {
                std::iter::once(0).chain((0..93).map(move |x| ((x / 7 + row) % 5) as u8))
            })
            .collect();
        let compressed = ZlibStreams::compress(&pixels).unwrap();
        let mut streams = ZlibStreams::default();
        let stream = streams.init().unwrap();
        let mut input = 0;
        let mut output = Vec::new();
        let mut status = 0;
        // Old libpng consumes each IDAT while avail_in > 0. A decoder that
        // buffers all compressed input before returning a single row leaves
        // the rest inaccessible to that client and triggers "Not enough image data".
        while input < compressed.len() && status != 1 {
            let mut row = [0; 94];
            let step = streams
                .inflate(stream, &compressed[input..], &mut row, 1)
                .unwrap();
            assert!(step.consumed != 0 || step.written != 0);
            input += step.consumed;
            output.extend_from_slice(&row[..step.written]);
            status = step.status;
        }
        assert_eq!(output, pixels);
        assert_eq!(status, 1);
        assert_eq!(input, compressed.len());
    }
    fn round_trip(streams: &ZlibStreams) -> ZlibStreams {
        let encoded = serde_json::to_vec(streams).unwrap();
        serde_json::from_slice::<ZlibStreams>(&encoded)
            .unwrap()
            .restore_snapshot()
            .unwrap()
    }

    fn payload(size: usize) -> Vec<u8> {
        let mut seed = 123456789u32;
        (0..size)
            .map(|index| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if index % 512 < 400 {
                    (index % 71) as u8
                } else {
                    (seed >> 24) as u8
                }
            })
            .collect()
    }

    #[test]
    fn cold_restore_continues_small_rows_huffman_matches_and_window_rollover() {
        let expected = payload(250_000);
        for level in [
            Compression::none(),
            Compression::fast(),
            Compression::best(),
        ] {
            let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), level);
            encoder.write_all(&expected).unwrap();
            let encoded = encoder.finish().unwrap();
            for (input_size, output_size) in
                [(1, 7), (37, 94), (4096, 32768), (usize::MAX, 100_000)]
            {
                let mut live = ZlibStreams::default();
                let handle = live.init().unwrap();
                let (mut consumed, mut output, mut calls) = (0usize, Vec::new(), 0);
                loop {
                    let end = consumed.saturating_add(input_size).min(encoded.len());
                    let mut row = vec![0; output_size];
                    let step = live
                        .inflate(handle, &encoded[consumed..end], &mut row, 0)
                        .unwrap();
                    consumed += step.consumed;
                    output.extend_from_slice(&row[..step.written]);
                    calls += 1;
                    if [1, 3, 17, 135, 2000].contains(&calls) {
                        live = round_trip(&live);
                    }
                    if step.status == 1 {
                        break;
                    }
                    assert_eq!(step.status, 0);
                    assert!(step.consumed != 0 || step.written != 0);
                    assert!(calls < 1_000_000);
                }
                assert_eq!(consumed, encoded.len());
                assert_eq!(output, expected);
                assert!(live.streams[&handle].window.len() < 2 * WINDOW);
                let mut reference = flate2::Decompress::new(true);
                let mut decoded = vec![0; expected.len()];
                assert_eq!(
                    reference
                        .decompress(&encoded, &mut decoded, flate2::FlushDecompress::Finish)
                        .unwrap(),
                    flate2::Status::StreamEnd
                );
                assert_eq!(decoded, output);
            }
        }
    }

    #[test]
    fn short_finish_can_retry_after_a_checkpoint_without_losing_input() {
        let expected = payload(90_000);
        let encoded = ZlibStreams::compress(&expected).unwrap();
        let mut streams = ZlibStreams::default();
        let handle = streams.init().unwrap();
        let mut row = [0; 13];
        let step = streams
            .inflate(handle, &encoded[..encoded.len() / 2], &mut row, 4)
            .unwrap();
        assert_eq!(step.status, -5);
        let mut consumed = step.consumed;
        let mut decoded = row[..step.written].to_vec();
        streams = round_trip(&streams);
        loop {
            let step = streams
                .inflate(handle, &encoded[consumed..], &mut row, 0)
                .unwrap();
            consumed += step.consumed;
            decoded.extend_from_slice(&row[..step.written]);
            if step.status == 1 {
                break;
            }
            assert_eq!(step.status, 0);
        }
        assert_eq!(decoded, expected);
        assert!(streams.end(handle));
        assert!(!streams.contains(handle));
    }

    #[test]
    fn dictionary_negotiation_survives_header_fragments_and_reset() {
        let header = [0x78, 0xbb, 0x12, 0x34, 0x56, 0x78];
        for fragment in 1..=6 {
            let mut streams = ZlibStreams::default();
            let handle = streams.init().unwrap();
            let mut consumed = 0;
            while consumed < header.len() {
                let end = (consumed + fragment).min(header.len());
                let step = streams
                    .inflate(handle, &header[consumed..end], &mut [0; 1], 0)
                    .unwrap();
                consumed += step.consumed;
                assert_eq!(step.written, 0);
                assert_eq!(step.status, if consumed == 6 { 2 } else { 0 });
                streams = round_trip(&streams);
            }
            let step = streams.inflate(handle, &[1, 2, 3], &mut [0; 1], 0).unwrap();
            assert_eq!(step.status, 2);
            assert_eq!(step.adler, 0x12345678);
            assert_eq!(step.consumed, 0);
            assert!(streams.reset(handle));
            let bytes = ZlibStreams::compress(b"reset works").unwrap();
            let mut out = [0; 20];
            let step = streams.inflate(handle, &bytes, &mut out, 4).unwrap();
            assert_eq!(step.status, 1);
            assert_eq!(&out[..step.written], b"reset works");
        }
    }

    #[test]
    fn invalid_checksum_and_completed_stream_keep_terminal_state() {
        let expected = payload(1000);
        for corrupt in [false, true] {
            let mut bytes = ZlibStreams::compress(&expected).unwrap();
            if corrupt {
                *bytes.last_mut().unwrap() ^= 1;
            }
            let mut streams = ZlibStreams::default();
            let handle = streams.init().unwrap();
            let mut out = vec![0; 2000];
            let step = streams.inflate(handle, &bytes, &mut out, 0).unwrap();
            assert_eq!(step.status, if corrupt { -3 } else { 1 });
            assert_eq!(&out[..step.written], expected);
            streams = round_trip(&streams);
            let resumed = streams.inflate(handle, &[99; 10], &mut out, 0).unwrap();
            assert_eq!(resumed.status, step.status);
            assert_eq!((resumed.consumed, resumed.written), (0, 0));
            assert_eq!(resumed.adler, step.adler);
        }
    }

    #[test]
    fn stream_namespace_reset_and_close_remain_independent_after_restore() {
        let mut streams = ZlibStreams::default();
        let a = streams.init().unwrap();
        let b = streams.init().unwrap();
        let encoded = ZlibStreams::compress(&payload(1000)).unwrap();
        streams.inflate(a, &encoded, &mut [0; 17], 0).unwrap();
        streams = round_trip(&streams);
        assert!(streams.end(a));
        assert!(!streams.end(a));
        assert!(streams.contains(b));
        let c = streams.init().unwrap();
        assert_ne!(a, c);
        assert_ne!(b, c);
        assert!(streams.reset(b));
        assert_eq!(streams.streams[&b].adler, 1);
        assert!(streams.streams[&b].window.is_empty());
    }
}
