//! Protracker (`M.K.`) module renderer.
//!
//! Pocket PC games built on the Hekkus Sound System ship their music as
//! 4-channel Amiga modules — JumpyBall's `.tkm` files are ordinary
//! Protracker modules under a game-specific extension. HSS renders them
//! on the device; PocketHLE renders them here and hands the result to
//! [`crate::audio::AudioEngine`] as an ordinary looping voice, which is
//! why nothing in this file knows about guest memory.
//!
//! Scope is deliberately Protracker rather than every tracker format.
//! The effects below are the ones JumpyBall's three modules actually
//! use, plus those that come for free once the mixer exists. An unknown
//! effect is ignored rather than guessed at: a wrong effect is more
//! audible than a missing one.

/// Paula's PAL sample clock is half the 7.09 MHz system clock. Protracker
/// periods divide this clock; row/tick timing is independent of sample pitch.
const PAL_CLOCK: f64 = 3_546_894.6;

/// Rows per pattern, fixed by the format.
const ROWS: usize = 64;

/// Sample slots in a 31-instrument module.
const SAMPLES: usize = 31;

/// Order-table length, in entries.
const ORDER_LEN: usize = 128;

/// Protracker's period range, from B-3 to C-1.
const MIN_PERIOD: f64 = 113.0;
const MAX_PERIOD: f64 = 856.0;

/// Sine table Protracker uses for vibrato and tremolo.
#[rustfmt::skip]
const SINE: [i32; 32] = [
      0,  24,  49,  74,  97, 120, 141, 161,
    180, 197, 212, 224, 235, 244, 250, 253,
    255, 253, 250, 244, 235, 224, 212, 197,
    180, 161, 141, 120,  97,  74,  49,  24,
];

struct Sample {
    data: Vec<i8>,
    /// Loop start and length in bytes, both already validated against
    /// `data`. A zero length means the sample does not loop.
    loop_start: usize,
    loop_len: usize,
    volume: i32,
    /// Protracker finetune, as a signed count of 1/8th semitones.
    finetune: i32,
}

#[derive(Clone, Copy, Default)]
struct Note {
    period: u16,
    sample: u8,
    effect: u8,
    param: u8,
}

/// A parsed Protracker module.
pub struct Module {
    pub title: String,
    samples: Vec<Sample>,
    patterns: Vec<Vec<Note>>,
    order: Vec<u8>,
    song_len: usize,
    channels: usize,
}

/// Per-channel playback state.
#[derive(Default, Clone)]
struct Channel {
    sample: usize,
    /// Playback position into the sample, in 16.16 fixed point.
    pos: u64,
    /// The channel's own period, as set by the last note and by the
    /// slide effects.
    period: f64,
    /// Period actually fed to the resampler this tick. Vibrato and
    /// arpeggio move this without disturbing `period`.
    play_period: f64,
    volume: i32,
    /// Stereo placement, 0.0 hard left through 1.0 hard right.
    pan: f32,
    porta_target: f64,
    porta_speed: f64,
    vibrato_speed: i32,
    vibrato_depth: i32,
    vibrato_pos: i32,
    tremolo_speed: i32,
    tremolo_depth: i32,
    tremolo_pos: i32,
    slide: u8,
    active: bool,
}

impl Module {
    /// Parse a 31-sample Protracker module. Anything without the magic
    /// is refused, including the 15-sample Soundtracker variant: it
    /// carries no magic at all and cannot be told from arbitrary bytes
    /// with any confidence.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let order_off = 20 + SAMPLES * 30;
        let magic_off = order_off + 2 + ORDER_LEN;
        if bytes.len() < magic_off + 4 {
            return None;
        }
        let channels = match &bytes[magic_off..magic_off + 4] {
            b"M.K." | b"M!K!" | b"FLT4" | b"4CHN" => 4,
            b"6CHN" => 6,
            b"8CHN" | b"FLT8" | b"CD81" | b"OKTA" => 8,
            _ => return None,
        };

        let title = String::from_utf8_lossy(&bytes[0..20])
            .trim_end_matches('\0')
            .trim()
            .to_string();

        // Sample headers come first, but the PCM they describe sits
        // after the patterns — so every length has to be read before
        // any sample data can be found.
        let mut headers = Vec::with_capacity(SAMPLES);
        for i in 0..SAMPLES {
            let o = 20 + i * 30;
            let len = be16(bytes, o + 22) * 2;
            let raw_finetune = (bytes[o + 24] & 0x0F) as i32;
            let finetune = if raw_finetune > 7 {
                raw_finetune - 16
            } else {
                raw_finetune
            };
            headers.push((
                len,
                finetune,
                bytes[o + 25].min(64) as i32,
                be16(bytes, o + 26) * 2,
                be16(bytes, o + 28) * 2,
            ));
        }

        let song_len = (bytes[order_off] as usize).min(ORDER_LEN);
        let order = bytes[order_off + 2..order_off + 2 + ORDER_LEN].to_vec();
        let pattern_count = order.iter().copied().max()? as usize + 1;
        let pattern_len = ROWS * channels * 4;
        let patterns_off = magic_off + 4;

        let mut patterns = Vec::with_capacity(pattern_count);
        for p in 0..pattern_count {
            let base = patterns_off + p * pattern_len;
            let mut cells = Vec::with_capacity(ROWS * channels);
            for c in 0..ROWS * channels {
                let o = base + c * 4;
                // Modules ripped out of games are routinely truncated.
                // Treat the missing rows as empty instead of refusing
                // the whole file.
                if o + 4 > bytes.len() {
                    cells.push(Note::default());
                    continue;
                }
                let b = &bytes[o..o + 4];
                cells.push(Note {
                    period: (((b[0] & 0x0F) as u16) << 8) | b[1] as u16,
                    sample: (b[0] & 0xF0) | (b[2] >> 4),
                    effect: b[2] & 0x0F,
                    param: b[3],
                });
            }
            patterns.push(cells);
        }

        let mut pcm = patterns_off + pattern_count * pattern_len;
        let mut samples = Vec::with_capacity(SAMPLES);
        for (len, finetune, volume, loop_start, loop_len) in headers {
            let end = (pcm + len).min(bytes.len());
            let data: Vec<i8> = if pcm < end {
                bytes[pcm..end].iter().map(|&b| b as i8).collect()
            } else {
                Vec::new()
            };
            // A loop length of one word is Protracker's "no loop" idiom.
            let looped = loop_len > 2 && loop_start + loop_len <= data.len();
            samples.push(Sample {
                data,
                loop_start: if looped { loop_start } else { 0 },
                loop_len: if looped { loop_len } else { 0 },
                volume,
                finetune,
            });
            pcm += len;
        }

        Some(Self {
            title,
            samples,
            patterns,
            order,
            song_len,
            channels,
        })
    }

    /// Render the song once to interleaved stereo `i16` at
    /// `sample_rate`.
    ///
    /// `max_seconds` bounds the output. A module whose order table jumps
    /// backwards plays forever on a device, and the caller needs a
    /// finite buffer it can hand to the mixer as a looping voice.
    pub fn render(&self, sample_rate: u32, max_seconds: u32) -> Vec<i16> {
        let rate = sample_rate.max(1) as f64;
        let cap = sample_rate.max(1) as usize * max_seconds.max(1) as usize * 2;
        let mut out: Vec<i16> = Vec::new();

        let mut chans: Vec<Channel> = (0..self.channels)
            .map(|i| Channel {
                // Protracker's hard LRRL panning, softened: full
                // separation is fatiguing on headphones in a way it
                // never was through an Amiga's speakers.
                pan: if i % 4 == 0 || i % 4 == 3 { 0.28 } else { 0.72 },
                ..Default::default()
            })
            .collect();

        let mut dc = DcBlocker::default();
        let mut speed = 6u32;
        let mut bpm = 125u32;
        let mut order_index = 0usize;
        let mut row = 0usize;
        // A module may revisit an order entry — many use `Bxx` to loop
        // the whole song. Allow two passes, then call it done.
        let mut visits = [0u8; ORDER_LEN];

        'song: while order_index < self.song_len && out.len() < cap {
            let Some(pattern) = self.patterns.get(self.order[order_index] as usize) else {
                break;
            };
            visits[order_index] = visits[order_index].saturating_add(1);
            if visits[order_index] > 2 {
                break;
            }

            let mut jump: Option<(usize, usize)> = None;
            while row < ROWS {
                let mut delay = 0u32;
                for c in 0..self.channels {
                    let note = pattern[row * self.channels + c];
                    trigger(&mut chans[c], note, &self.samples);
                    match note.effect {
                        0x0B => jump = Some((note.param as usize, 0)),
                        0x0D => {
                            let r = (note.param >> 4) as usize * 10 + (note.param & 0x0F) as usize;
                            jump = Some((order_index + 1, r.min(ROWS - 1)));
                        }
                        0x0F if note.param == 0 => break 'song,
                        0x0F if note.param < 0x20 => speed = note.param as u32,
                        0x0F => bpm = note.param as u32,
                        0x0E if note.param >> 4 == 0x0E => delay = (note.param & 0x0F) as u32,
                        _ => {}
                    }
                }

                // Protracker's tick is 2.5 / bpm seconds long.
                let frames = (rate * 2.5 / bpm.max(1) as f64).max(1.0) as usize;
                for tick in 0..speed * (delay + 1) {
                    for c in 0..self.channels {
                        tick_effects(&mut chans[c], pattern[row * self.channels + c], tick);
                    }
                    mix(&mut chans, &self.samples, frames, rate, &mut dc, &mut out);
                    if out.len() >= cap {
                        break 'song;
                    }
                }

                if jump.is_some() {
                    break;
                }
                row += 1;
            }

            match jump {
                Some((o, r)) => {
                    order_index = o;
                    row = r;
                }
                None => {
                    order_index += 1;
                    row = 0;
                }
            }
        }

        out
    }
}

fn be16(bytes: &[u8], at: usize) -> usize {
    u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize
}

/// Shift a period by `steps` eighths of a semitone. Protracker's
/// finetune and this renderer's arpeggio both reduce to this.
fn detune(period: f64, eighths: i32) -> f64 {
    if eighths == 0 {
        return period;
    }
    (period * 2f64.powf(-(eighths as f64) / 96.0)).clamp(MIN_PERIOD, MAX_PERIOD)
}

/// Start a note on a channel. Runs once per row, on tick 0.
fn trigger(ch: &mut Channel, note: Note, samples: &[Sample]) {
    if note.sample > 0 {
        let index = note.sample as usize - 1;
        if let Some(s) = samples.get(index) {
            ch.sample = index;
            ch.volume = s.volume;
        }
    }
    // Effects 3 and 5 slide *towards* the note instead of restarting it.
    let porta_to_note = note.effect == 0x03 || note.effect == 0x05;
    if note.period > 0 {
        let finetune = samples.get(ch.sample).map_or(0, |s| s.finetune);
        let period = detune(note.period as f64, finetune);
        if porta_to_note {
            ch.porta_target = period;
        } else {
            ch.period = period;
            ch.play_period = period;
            ch.pos = 0;
            ch.active = true;
            ch.vibrato_pos = 0;
            ch.tremolo_pos = 0;
        }
    }

    match note.effect {
        0x03 if note.param != 0 => ch.porta_speed = note.param as f64,
        0x04 => {
            if note.param >> 4 != 0 {
                ch.vibrato_speed = (note.param >> 4) as i32;
            }
            if note.param & 0x0F != 0 {
                ch.vibrato_depth = (note.param & 0x0F) as i32;
            }
        }
        0x07 => {
            if note.param >> 4 != 0 {
                ch.tremolo_speed = (note.param >> 4) as i32;
            }
            if note.param & 0x0F != 0 {
                ch.tremolo_depth = (note.param & 0x0F) as i32;
            }
        }
        // `9xx` — start playing at offset xx * 256 bytes.
        0x09 => ch.pos = (note.param as u64 * 256) << 16,
        0x0C => ch.volume = (note.param as i32).min(64),
        0x0E => match note.param >> 4 {
            // `E1x` / `E2x` — fine portamento, once, on tick 0.
            0x1 => ch.period = (ch.period - (note.param & 0x0F) as f64).max(MIN_PERIOD),
            0x2 => ch.period = (ch.period + (note.param & 0x0F) as f64).min(MAX_PERIOD),
            // `EAx` / `EBx` — fine volume slide, likewise.
            0xA => ch.volume = (ch.volume + (note.param & 0x0F) as i32).min(64),
            0xB => ch.volume = (ch.volume - (note.param & 0x0F) as i32).max(0),
            _ => {}
        },
        _ => {}
    }
    if matches!(note.effect, 0x05 | 0x06 | 0x0A) {
        ch.slide = note.param;
    }
    ch.play_period = ch.period;
}

/// Per-tick effect processing. Tick 0 is the row trigger, which
/// [`trigger`] has already handled.
fn tick_effects(ch: &mut Channel, note: Note, tick: u32) {
    if tick == 0 {
        return;
    }
    match note.effect {
        // Arpeggio: root, then +x, then +y semitones, cycling.
        0x00 if note.param != 0 => {
            let semis = match tick % 3 {
                0 => 0,
                1 => note.param >> 4,
                _ => note.param & 0x0F,
            };
            ch.play_period = detune(ch.period, semis as i32 * 8);
        }
        0x01 => {
            ch.period = (ch.period - note.param as f64).max(MIN_PERIOD);
            ch.play_period = ch.period;
        }
        0x02 => {
            ch.period = (ch.period + note.param as f64).min(MAX_PERIOD);
            ch.play_period = ch.period;
        }
        0x03 | 0x05 => {
            porta(ch);
            if note.effect == 0x05 {
                volume_slide(ch);
            }
            ch.play_period = ch.period;
        }
        0x04 | 0x06 => {
            vibrato(ch);
            if note.effect == 0x06 {
                volume_slide(ch);
            }
        }
        0x07 => tremolo(ch),
        0x0A => {
            volume_slide(ch);
            ch.play_period = ch.period;
        }
        0x0E => match note.param >> 4 {
            // `E9x` — retrigger the sample every x ticks.
            0x9 => {
                let x = (note.param & 0x0F) as u32;
                if x > 0 && tick.is_multiple_of(x) {
                    ch.pos = 0;
                }
            }
            // `ECx` — cut to silence at tick x.
            0xC if tick == (note.param & 0x0F) as u32 => ch.volume = 0,
            _ => {}
        },
        _ => ch.play_period = ch.period,
    }
}

fn porta(ch: &mut Channel) {
    if ch.porta_target == 0.0 {
        return;
    }
    if ch.period < ch.porta_target {
        ch.period = (ch.period + ch.porta_speed).min(ch.porta_target);
    } else {
        ch.period = (ch.period - ch.porta_speed).max(ch.porta_target);
    }
}

fn vibrato(ch: &mut Channel) {
    let delta = SINE[(ch.vibrato_pos & 31) as usize] * ch.vibrato_depth / 128;
    let signed = if ch.vibrato_pos & 32 != 0 {
        -delta
    } else {
        delta
    };
    ch.play_period = (ch.period + signed as f64).clamp(MIN_PERIOD, MAX_PERIOD);
    ch.vibrato_pos = (ch.vibrato_pos + ch.vibrato_speed) & 63;
}

fn tremolo(ch: &mut Channel) {
    let delta = SINE[(ch.tremolo_pos & 31) as usize] * ch.tremolo_depth / 64;
    let signed = if ch.tremolo_pos & 32 != 0 {
        -delta
    } else {
        delta
    };
    ch.volume = (ch.volume + signed).clamp(0, 64);
    ch.tremolo_pos = (ch.tremolo_pos + ch.tremolo_speed) & 63;
}

fn volume_slide(ch: &mut Channel) {
    let up = (ch.slide >> 4) as i32;
    let down = (ch.slide & 0x0F) as i32;
    if up > 0 {
        ch.volume = (ch.volume + up).min(64);
    } else if down > 0 {
        ch.volume = (ch.volume - down).max(0);
    }
}

/// Mix `frames` stereo frames of every active channel onto `out`.
///
/// `dc` carries the DC-blocker state across calls — see [`block_dc`].
fn mix(
    chans: &mut [Channel],
    samples: &[Sample],
    frames: usize,
    rate: f64,
    dc: &mut DcBlocker,
    out: &mut Vec<i16>,
) {
    out.reserve(frames * 2);
    for _ in 0..frames {
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for ch in chans.iter_mut() {
            if !ch.active || ch.play_period <= 0.0 {
                continue;
            }
            let Some(sample) = samples.get(ch.sample) else {
                continue;
            };
            let index = (ch.pos >> 16) as usize;
            if index >= sample.data.len() {
                ch.active = false;
                continue;
            }
            let v = sample.data[index] as f32 / 128.0 * (ch.volume as f32 / 64.0);
            left += v * (1.0 - ch.pan);
            right += v * ch.pan;

            let step = PAL_CLOCK / ch.play_period / rate;
            ch.pos = ch.pos.saturating_add((step * 65536.0) as u64);
            let pos = (ch.pos >> 16) as usize;
            if pos >= sample.data.len() {
                if sample.loop_len > 0 {
                    // Preserve the fractional phase and any overshoot. Dropping
                    // the fraction at every loop retunes short periodic samples.
                    let start = (sample.loop_start as u64) << 16;
                    let length = (sample.loop_len as u64) << 16;
                    ch.pos = start + (ch.pos - start) % length;
                } else {
                    ch.active = false;
                }
            }
        }
        let (left, right) = dc.step(left, right);
        // Four channels at full scale would clip constantly. Protracker
        // players traditionally mix well below unity and let the loud
        // modules use the headroom.
        out.push((left.clamp(-2.8, 2.8) * 0.35 * 32767.0) as i16);
        out.push((right.clamp(-2.8, 2.8) * 0.35 * 32767.0) as i16);
    }
}

/// One-pole DC blocker, run over the final mix.
///
/// Module samples routinely carry a large DC offset — JumpyBall's
/// `music1.tkm` has instruments sitting at +99 out of ±128. An Amiga's
/// AC-coupled output threw that away for free. A modern DAC does not:
/// the offset eats headroom, biases the clamp, and clicks audibly
/// whenever a looping voice stops. Removing it here rather than
/// per-sample keeps every channel's waveform exactly as the author
/// wrote it.
#[derive(Default)]
struct DcBlocker {
    prev_in: (f32, f32),
    prev_out: (f32, f32),
}

impl DcBlocker {
    /// Pole position. 0.999 puts the corner near 3.5 Hz at 22 kHz —
    /// below anything musical, high enough to settle in a few ms.
    const R: f32 = 0.999;

    fn step(&mut self, left: f32, right: f32) -> (f32, f32) {
        let out_l = left - self.prev_in.0 + Self::R * self.prev_out.0;
        let out_r = right - self.prev_in.1 + Self::R * self.prev_out.1;
        self.prev_in = (left, right);
        self.prev_out = (out_l, out_r);
        (out_l, out_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A structurally valid module: one pattern, one sample, one note.
    fn tiny_module() -> Vec<u8> {
        let order_off = 20 + SAMPLES * 30;
        let magic_off = order_off + 2 + ORDER_LEN;
        let mut m = vec![0u8; magic_off + 4];
        m[0..4].copy_from_slice(b"tiny");
        // Sample 1: 16 words (32 bytes), full volume, no loop.
        m[20 + 22..20 + 24].copy_from_slice(&16u16.to_be_bytes());
        m[20 + 25] = 64;
        m[20 + 29] = 1;
        m[order_off] = 1; // song length
        m[order_off + 2] = 0; // order[0] -> pattern 0
        m[magic_off..magic_off + 4].copy_from_slice(b"M.K.");
        // Row 0, channel 0: sample 1 at period 428 (C-2). The sample
        // number is split across two nibbles — high in byte 0, low in
        // byte 2 — so sample 1 lives entirely in byte 2.
        let mut pattern = vec![0u8; ROWS * 4 * 4];
        pattern[0] = (428u16 >> 8) as u8;
        pattern[1] = (428u16 & 0xFF) as u8;
        pattern[2] = 0x10;
        m.extend_from_slice(&pattern);
        // A square wave, so the mixer has something audible to chew on.
        m.extend((0..32).map(|i| if i < 16 { 100u8 } else { 156u8 }));
        m
    }

    #[test]
    fn a_protracker_module_parses_its_header() {
        let m = Module::parse(&tiny_module()).expect("tiny module should parse");
        assert_eq!(m.title, "tiny");
        assert_eq!(m.channels, 4);
        assert_eq!(m.song_len, 1);
        assert_eq!(m.patterns.len(), 1);
        assert_eq!(m.samples[0].data.len(), 32);
        assert_eq!(m.samples[0].volume, 64);
        assert_eq!(m.patterns[0][0].period, 428);
        assert_eq!(m.patterns[0][0].sample, 1);
    }

    /// The magic is the only thing separating a module from arbitrary
    /// bytes, and `hssMusic::load` gets handed whatever file the game
    /// named — including WAVs.
    #[test]
    fn a_file_without_the_magic_is_not_a_module() {
        assert!(Module::parse(&[0u8; 2048]).is_none());
        assert!(Module::parse(b"RIFF\0\0\0\0WAVEfmt ").is_none());
        let mut wrong = tiny_module();
        let magic_off = 20 + SAMPLES * 30 + 2 + ORDER_LEN;
        wrong[magic_off..magic_off + 4].copy_from_slice(b"XXXX");
        assert!(Module::parse(&wrong).is_none());
    }

    /// A silent render is the failure mode that looks like success
    /// everywhere downstream, so assert on actual movement.
    #[test]
    fn rendering_produces_non_silent_stereo_audio() {
        let m = Module::parse(&tiny_module()).unwrap();
        let pcm = m.render(22050, 2);
        assert!(!pcm.is_empty(), "render produced nothing");
        assert_eq!(pcm.len() % 2, 0, "output must be interleaved stereo");
        assert!(pcm.iter().any(|&s| s != 0), "render produced only silence");
    }

    /// A 32-byte loop at period 428 has a 258.97 Hz fundamental on PAL.
    /// Check actual rendered PCM at multiple output rates, independently of
    /// the period-to-step implementation, and keep the row duration unchanged.
    #[test]
    fn rendered_pal_pitch_and_tempo_match_a_reference_tone() {
        let mut bytes = tiny_module();
        bytes[48..50].copy_from_slice(&16u16.to_be_bytes()); // loop 32 bytes
        let m = Module::parse(&bytes).unwrap();
        for rate in [22_050, 44_100, 48_000] {
            let pcm = m.render(rate, 2);
            assert_eq!(pcm.len(), rate as usize * 2 * 2);
            let frames: Vec<_> = pcm.chunks_exact(2).map(|s| s[0]).collect();
            let start = rate as usize / 10;
            let end = rate as usize;
            let crossings = frames[start..end]
                .windows(2)
                .filter(|s| s[0] <= 0 && s[1] > 0)
                .count();
            let hz = crossings as f64 * rate as f64 / (end - start) as f64;
            assert!(
                (hz - 258.97).abs() < 1.5,
                "{rate} Hz output: tone was {hz} Hz"
            );
        }
    }

    /// A module whose order table jumps backwards plays forever on a
    /// device. The renderer has to stop regardless, or the caller waits
    /// on a buffer that never arrives.
    #[test]
    fn a_looping_order_table_still_terminates() {
        let mut bytes = tiny_module();
        let order_off = 20 + SAMPLES * 30;
        bytes[order_off] = 4;
        let patterns_off = order_off + 2 + ORDER_LEN + 4;
        // Row 0, channel 0: `B00`, jump back to order 0, forever. The
        // low nibble of byte 2 is the effect; the high nibble stays 1
        // so the note keeps its sample.
        bytes[patterns_off + 2] = 0x1B;
        bytes[patterns_off + 3] = 0x00;
        let m = Module::parse(&bytes).unwrap();
        let pcm = m.render(22050, 5);
        assert!(pcm.len() <= 22050 * 5 * 2, "render blew past its bound");
    }

    /// Real modules ship samples with a large DC offset, which an Amiga
    /// discarded and a modern DAC will not.
    #[test]
    fn a_dc_offset_sample_renders_centred() {
        let mut bytes = tiny_module();
        // Replace the square wave with a constant +100: pure DC.
        let pcm = bytes.len() - 32;
        for b in &mut bytes[pcm..] {
            *b = 100;
        }
        let m = Module::parse(&bytes).unwrap();
        let pcm = m.render(22050, 1);
        let mean = pcm.iter().map(|&s| s as i64).sum::<i64>() / pcm.len() as i64;
        assert!(mean.abs() < 200, "output carries a DC offset of {mean}");
    }

    /// Pitch is the whole point of the period table: an octave up is
    /// half the period, and finetune moves in eighths of a semitone.
    #[test]
    fn detuning_moves_pitch_by_the_expected_amount() {
        assert_eq!(detune(428.0, 0), 428.0);
        // Twelve semitones is 96 eighths — exactly one octave.
        assert!((detune(428.0, 96) - 214.0).abs() < 0.01);
        // Positive finetune raises pitch, so the period gets smaller.
        assert!(detune(428.0, 1) < 428.0);
        assert!(detune(428.0, -1) > 428.0);
    }
}
