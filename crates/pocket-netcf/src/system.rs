//! Compact Framework system services backed by managed object identities.

use crate::vm::{Fault, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct Services {
    random: BTreeMap<usize, Random>,
}

impl Services {
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
