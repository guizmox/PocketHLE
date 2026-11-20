//! Streaming, independently versioned state records. IDs are permanent schema keys.
use crate::{codec, PAYLOAD_LIMIT};
use bincode::Options;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashSet;
use std::io::{self, Read, Write};
pub const REQUIRED: u16 = 0x8000;
const HEADER: u64 = 12;
const MAX_FIELDS: usize = 4096;
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
#[derive(Clone, Copy)]
pub struct Header {
    pub id: u16,
    pub version: u16,
    pub len: u64,
}
pub fn field<T: Serialize>(id: u16, version: u16, value: &T) -> io::Result<Header> {
    Ok(Header {
        id,
        version,
        len: codec().serialized_size(value).map_err(io::Error::other)?,
    })
}
/// Owners define their schema; the codec never invents missing guest resources.
pub trait FieldSet: Sized {
    fn layout(&self) -> io::Result<Vec<Header>>;
    fn write_values(&self, output: &mut dyn Write, layout: &[Header]) -> io::Result<()>;
    fn read_archive(input: &mut dyn Read) -> io::Result<Self>;
}
pub struct Plan<'a, T> {
    value: &'a T,
    layout: Vec<Header>,
    len: u64,
}
impl<'a, T: FieldSet> Plan<'a, T> {
    pub fn new(value: &'a T) -> io::Result<Self> {
        let layout = value.layout()?;
        if layout.len() > MAX_FIELDS {
            return Err(invalid("too many state records"));
        }
        let mut len = 2u64;
        let mut seen = HashSet::new();
        for h in &layout {
            if h.id & !REQUIRED == 0 || !seen.insert(h.id & !REQUIRED) {
                return Err(invalid("duplicate or zero state record ID"));
            }
            len = len
                .checked_add(HEADER)
                .and_then(|n| n.checked_add(h.len))
                .filter(|n| *n <= PAYLOAD_LIMIT)
                .ok_or_else(|| invalid("state records exceed size limit"))?;
        }
        Ok(Self { value, layout, len })
    }
    pub fn encoded_len(&self) -> u64 {
        self.len
    }
    pub fn write(&self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(&(self.layout.len() as u16).to_le_bytes())?;
        self.value.write_values(output, &self.layout)
    }
}
struct LimitedWriter<'a> {
    output: &'a mut dyn Write,
    remaining: u64,
}
impl Write for LimitedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(invalid("state record write exceeds declared length"));
        }
        let n = self.output.write(bytes)?;
        self.remaining -= n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}
pub fn write_value<T: Serialize>(output: &mut dyn Write, h: Header, value: &T) -> io::Result<()> {
    output.write_all(&h.id.to_le_bytes())?;
    output.write_all(&h.version.to_le_bytes())?;
    output.write_all(&h.len.to_le_bytes())?;
    let mut bounded = LimitedWriter {
        output,
        remaining: h.len,
    };
    codec()
        .with_no_limit()
        .serialize_into(&mut bounded, value)
        .map_err(io::Error::other)?;
    if bounded.remaining != 0 {
        return Err(invalid("state record write length mismatch"));
    }
    Ok(())
}
pub fn read_value<T: DeserializeOwned>(h: Header, input: &mut dyn Read) -> io::Result<T> {
    codec()
        .with_limit(h.len)
        .deserialize_from(input)
        .map_err(io::Error::other)
}
pub fn visit(
    input: &mut dyn Read,
    mut decode: impl FnMut(Header, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    let mut count = [0; 2];
    input.read_exact(&mut count)?;
    let count = u16::from_le_bytes(count) as usize;
    if count > MAX_FIELDS {
        return Err(invalid("too many state records"));
    }
    let mut remaining = PAYLOAD_LIMIT - 2;
    let mut seen = HashSet::new();
    for _ in 0..count {
        let mut bytes = [0; 12];
        input.read_exact(&mut bytes)?;
        let h = Header {
            id: u16::from_le_bytes(bytes[..2].try_into().unwrap()),
            version: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
            len: u64::from_le_bytes(bytes[4..].try_into().unwrap()),
        };
        if h.id & !REQUIRED == 0 || !seen.insert(h.id & !REQUIRED) {
            return Err(invalid("duplicate or zero state record ID"));
        }
        remaining = remaining
            .checked_sub(HEADER)
            .and_then(|n| n.checked_sub(h.len))
            .ok_or_else(|| invalid("state records exceed size limit"))?;
        let mut bounded = (&mut *input).take(h.len);
        if !decode(h, &mut bounded)? {
            if h.id & REQUIRED != 0 {
                return Err(invalid(format!(
                    "unsupported required state record {:#06x} version {}",
                    h.id & !REQUIRED,
                    h.version
                )));
            }
            io::copy(&mut bounded, &mut io::sink())?;
        }
        if bounded.limit() != 0 {
            return Err(invalid("state record length mismatch"));
        }
    }
    Ok(())
}
pub fn missing(id: u16, name: &str) -> io::Error {
    invalid(format!("missing required state record {id:#06x} ({name})"))
}
pub fn version(id: u16, name: &str, actual: u16) -> io::Error {
    invalid(format!(
        "unsupported state record {id:#06x} ({name}) version {actual}"
    ))
}

/// Explicit numeric keys survive field insertion/reordering. Add optional data
/// only when its default is semantically valid for snapshots that lack it.
#[macro_export]
macro_rules! snapshot_fields {
    ($vis:vis struct $name:ident { $( $id:literal, $version:literal => $field:ident : $ty:ty $(, decode = $decoder:path)? $(= $default:expr)? ; )* }) => {
        $vis struct $name { $( $field: $ty, )* }
        impl $crate::fields::FieldSet for $name {
            fn layout(&self) -> std::io::Result<Vec<$crate::fields::Header>> {
                Ok(vec![$($crate::fields::field($crate::snapshot_fields!(@tag $id $(, $default)?), $version, &self.$field)?,)*])
            }
            fn write_values(&self, output: &mut dyn std::io::Write, layout: &[$crate::fields::Header]) -> std::io::Result<()> {
                let mut iter = layout.iter();
                $( $crate::fields::write_value(output, *iter.next().expect("schema layout"), &self.$field)?; )*
                Ok(())
            }
            fn read_archive(input: &mut dyn std::io::Read) -> std::io::Result<Self> {
                $( let mut $field: Option<$ty> = None; )*
                $crate::fields::visit(input, |h, input| {
                    match h.id & !$crate::fields::REQUIRED {
                        $( $id => {
                            if h.id != $crate::snapshot_fields!(@tag $id $(, $default)?) {
                                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
                                    format!("invalid flags for state record {:#06x} ({})", $id, stringify!($field))));
                            }
                            $field = Some($crate::snapshot_fields!(@read h, input, $id, $field, $version $(, $decoder)?));
                        }, )*
                        _ => return Ok(false),
                    }
                    Ok(true)
                })?;
                Ok(Self { $( $field: $crate::snapshot_fields!(@value $field, $id $(, $default)?), )* })
            }
        }
    };
    (@read $h:ident, $input:ident, $id:literal, $field:ident, $v:literal) => {{
        if $h.version != $v { return Err($crate::fields::version($id, stringify!($field), $h.version)); }
        $crate::fields::read_value($h, $input)?
    }};
    (@read $h:ident, $input:ident, $id:literal, $field:ident, $v:literal, $decoder:path) => { $decoder($h, $input)? };
    (@tag $id:literal) => { $id | $crate::fields::REQUIRED };
    (@tag $id:literal, $default:expr) => { $id };
    (@value $field:ident, $id:literal) => { $field.ok_or_else(|| $crate::fields::missing($id, stringify!($field)))? };
    (@value $field:ident, $id:literal, $default:expr) => { $field.unwrap_or_else(|| $default) };
}

#[cfg(test)]
mod tests {
    use super::*;
    crate::snapshot_fields! { struct Old { 0x0101, 1 => money: u32; } }
    // Optional metadata may be absent without changing the restored guest state.
    crate::snapshot_fields! { struct Extended {
        0x0201, 1 => annotation: Option<i32> = None;
        0x0101, 1 => money: u32;
    } }
    fn money_v2(h: Header, input: &mut dyn Read) -> io::Result<u64> {
        match h.version {
            1 => read_value::<u32>(h, input).map(u64::from),
            2 => read_value(h, input),
            _ => Err(version(0x0101, "money", h.version)),
        }
    }
    crate::snapshot_fields! { struct Migrated {
        0x0101, 2 => money: u64, decode = money_v2;
    } }
    fn encode<T: FieldSet>(v: &T) -> Vec<u8> {
        let plan = Plan::new(v).unwrap();
        let mut bytes = Vec::new();
        plan.write(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, plan.encoded_len());
        bytes
    }
    #[test]
    fn additions_order_and_scoped_migration_preserve_old_state() {
        let old = encode(&Old { money: 735 });
        // Frozen original wire record, independent of the current writer.
        assert_eq!(
            old,
            vec![1, 0, 1, 129, 1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 223, 2, 0, 0]
        );
        let new = Extended::read_archive(&mut old.as_slice()).unwrap();
        assert_eq!((new.money, new.annotation), (735, None));
        let extended = encode(&Extended {
            money: 900,
            annotation: Some(7),
        });
        assert_eq!(
            Old::read_archive(&mut extended.as_slice()).unwrap().money,
            900
        );
        assert_eq!(
            Migrated::read_archive(&mut old.as_slice()).unwrap().money,
            735
        );
        let changed = encode(&Migrated {
            money: u64::from(u32::MAX) + 1,
        });
        assert!(Old::read_archive(&mut changed.as_slice()).is_err());
        assert_eq!(
            Migrated::read_archive(&mut changed.as_slice())
                .unwrap()
                .money,
            u64::from(u32::MAX) + 1
        );
    }
    #[test]
    fn damaged_required_duplicate_length_and_truncated_records_fail() {
        let original = encode(&Old { money: 15 });
        assert!(Old::read_archive(&mut [0u8, 0].as_slice()).is_err());
        let mut bytes = original.clone();
        bytes[4..6].copy_from_slice(&9u16.to_le_bytes());
        assert!(Old::read_archive(&mut bytes.as_slice()).is_err());
        let mut bytes = original.clone();
        bytes[2..4].copy_from_slice(&(REQUIRED | 0x1111).to_le_bytes());
        assert!(Old::read_archive(&mut bytes.as_slice()).is_err());
        let mut bytes = original.clone();
        bytes[..2].copy_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&original[2..]);
        assert!(Old::read_archive(&mut bytes.as_slice()).is_err());
        for len in [0, 3, 5, PAYLOAD_LIMIT, u64::MAX] {
            let mut bytes = original.clone();
            bytes[6..14].copy_from_slice(&len.to_le_bytes());
            assert!(Old::read_archive(&mut bytes.as_slice()).is_err());
        }
        for size in 0..original.len() {
            assert!(Old::read_archive(&mut original[..size].as_ref()).is_err());
        }
        let mut bytes = original;
        bytes[2..4].copy_from_slice(&0x0101u16.to_le_bytes());
        assert!(Old::read_archive(&mut bytes.as_slice()).is_err());
    }
    #[test]
    fn optional_unknown_payload_is_streamed_and_truncation_is_rejected() {
        let mut bytes = encode(&Old { money: 1 });
        bytes[..2].copy_from_slice(&2u16.to_le_bytes());
        write_value(
            &mut bytes,
            Header {
                id: 0x0202,
                version: 19,
                len: 4,
            },
            &33u32,
        )
        .unwrap();
        assert_eq!(Old::read_archive(&mut bytes.as_slice()).unwrap().money, 1);
        bytes.pop();
        assert!(Old::read_archive(&mut bytes.as_slice()).is_err());
    }
}
