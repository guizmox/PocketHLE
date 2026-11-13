//! Compact Framework system services backed by managed object identities.

use crate::vm::{Fault, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct Services {
    vb_random: u32,
    vb_last: f32,
    random: BTreeMap<usize, Random>,
}

impl Services {
    pub fn sweep(&mut self, live: &[bool]) {
        self.random.retain(|&id, _| live[id]);
    }

    pub fn vb_random(&mut self, name: &str, args: &[Value]) -> Option<Option<Value>> {
        match (name, args) {
            ("Randomize", []) => {
                self.vb_random = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0x50000, |now| now.subsec_nanos())
                    & 0xff_ffff;
                Some(None)
            }
            ("Rnd", [] | [Value::Float(_)]) => {
                let bits = match args {
                    [Value::Float(bits)] => *bits,
                    _ => 1.0f32.to_bits(),
                };
                let number = f32::from_bits(bits);
                if number < 0.0 {
                    self.vb_random = bits.wrapping_add(bits >> 24) & 0xff_ffff;
                }
                if number != 0.0 {
                    self.vb_random = self
                        .vb_random
                        .wrapping_mul(1_140_671_485)
                        .wrapping_add(12_820_163)
                        & 0xff_ffff;
                    self.vb_last = self.vb_random as f32 / 16_777_216.0;
                }
                Some(Some(Value::Float(self.vb_last.to_bits())))
            }
            _ => None,
        }
    }

    pub fn random(&mut self, method: &str, args: &[Value]) -> Option<Result<Option<Value>, Fault>> {
        match (method, args) {
            (".ctor", [Value::Reference(id)]) => {
                let seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_millis() as i32);
                self.random.insert(*id, Random::new(seed));
                Some(Ok(None))
            }
            (".ctor", [Value::Reference(id), Value::Int(seed)]) => {
                self.random.insert(*id, Random::new(*seed));
                Some(Ok(None))
            }
            ("Next", [Value::Reference(id)]) => Some(
                self.random
                    .get_mut(id)
                    .map(|rng| Some(Value::Int(rng.next())))
                    .ok_or(Fault::Reference),
            ),
            ("Next", [Value::Reference(id), Value::Int(max)]) if *max >= 0 => Some(
                self.random
                    .get_mut(id)
                    .map(|rng| {
                        Some(Value::Int(
                            (i64::from(rng.next()) * i64::from(*max) / i64::from(i32::MAX)) as i32,
                        ))
                    })
                    .ok_or(Fault::Reference),
            ),
            ("Next", [Value::Reference(id), Value::Int(min), Value::Int(max)]) if min <= max => {
                Some(
                    self.random
                        .get_mut(id)
                        .map(|rng| {
                            let sample = f64::from(rng.next()) / f64::from(i32::MAX);
                            let span = i64::from(*max) - i64::from(*min);
                            Some(Value::Int(
                                (i64::from(*min) + (span as f64 * sample) as i64) as i32,
                            ))
                        })
                        .ok_or(Fault::Reference),
                )
            }
            ("NextDouble", [Value::Reference(id)]) => Some(
                self.random
                    .get_mut(id)
                    .map(|rng| {
                        Some(Value::Double(
                            (f64::from(rng.next()) / f64::from(i32::MAX)).to_bits(),
                        ))
                    })
                    .ok_or(Fault::Reference),
            ),
            _ => None,
        }
    }
}

struct Random {
    values: [i32; 56],
    index: usize,
    other: usize,
}

impl Random {
    fn new(seed: i32) -> Self {
        let mut values = [0; 56];
        let mut previous = 161_803_398 - seed.saturating_abs();
        values[55] = previous;
        let mut next = 1;
        for step in 1..55 {
            let index = 21 * step % 55;
            values[index] = next;
            next = ((i64::from(previous) - i64::from(next)).rem_euclid(i64::from(i32::MAX))) as i32;
            previous = values[index];
        }
        for _ in 0..4 {
            for index in 1..56 {
                values[index] = ((i64::from(values[index])
                    - i64::from(values[1 + (index + 30) % 55]))
                .rem_euclid(i64::from(i32::MAX))) as i32;
            }
        }
        Self {
            values,
            index: 0,
            other: 21,
        }
    }

    fn next(&mut self) -> i32 {
        self.index = self.index % 55 + 1;
        self.other = self.other % 55 + 1;
        let value = (self.values[self.index] - self.values[self.other]).rem_euclid(i32::MAX);
        self.values[self.index] = value;
        value
    }
}

pub fn date_text(seconds: u64) -> String {
    // Convert days since the Unix epoch to the proleptic Gregorian calendar.
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days / 146_097;
    let day = days % 146_097;
    let year_of_era = (day - day / 1460 + day / 36_524 - day / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let date = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = (month_index + 2) % 12 + 1;
    let year = year + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{date:02}")
}
