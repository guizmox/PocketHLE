//! Bounded metadata reader for MSFT type libraries embedded in CE COM servers.
//! This describes interfaces; guest methods still execute through their provider.

#[derive(Debug, Clone)]
pub struct TypeDesc {
    pub vt: u16,
    pub value: u32,
    pub child: Option<Box<TypeDesc>>,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub ty: TypeDesc,
    pub flags: u16,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub id: i32,
    pub name: String,
    pub result: TypeDesc,
    pub kind: u32,
    pub invoke: u32,
    pub callconv: u32,
    pub flags: u16,
    pub vtable_offset: u16,
    pub optional: i16,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub guid: [u8; 16],
    pub name: String,
    pub kind: u32,
    pub flags: u16,
    pub version: (u16, u16),
    pub size: u32,
    pub alignment: u16,
    pub vtable_size: u16,
    pub variables: u16,
    pub implementations: Vec<(u32, u32)>,
    pub functions: Vec<Function>,
    pub alias: Option<TypeDesc>,
}

#[derive(Debug, Clone)]
pub struct Library {
    pub guid: [u8; 16],
    pub lcid: u32,
    pub syskind: u32,
    pub flags: u16,
    pub version: (u16, u16),
    pub name: String,
    pub types: Vec<TypeInfo>,
    pub type_offsets: Vec<u32>,
}

#[derive(Debug, thiserror::Error)]
#[error("unsupported or malformed MSFT type library")]
pub struct Error;

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn guid(bytes: &[u8], offset: u32) -> Option<[u8; 16]> {
    bytes
        .get(offset as usize..(offset as usize).checked_add(16)?)?
        .try_into()
        .ok()
}
fn name(bytes: &[u8], offset: u32) -> Option<String> {
    if offset == u32::MAX {
        return Some(String::new());
    }
    let start = offset as usize;
    let len = (word(bytes, start.checked_add(8)?)? & 0xff) as usize;
    let text = bytes.get(start.checked_add(12)?..start.checked_add(12 + len)?)?;
    // MIDL's names here are byte strings in the library's locale. Interface
    // and member identifiers in the CE MSFT libraries are ASCII.
    Some(String::from_utf8_lossy(text).into_owned())
}
fn version(value: u32) -> (u16, u16) {
    (value as u16, (value >> 16) as u16)
}

fn desc(bytes: &[u8], encoded: u32, depth: usize) -> Option<TypeDesc> {
    if depth > 32 {
        return None;
    }
    if encoded & 0x8000_0000 != 0 {
        return Some(TypeDesc {
            vt: encoded as u16,
            value: 0,
            child: None,
        });
    }
    let start = encoded as usize;
    let vt = word(bytes, start)? as u16;
    let value = word(bytes, start.checked_add(4)?)?;
    let child = if matches!(vt, 26 | 27) {
        Some(Box::new(desc(bytes, value, depth + 1)?))
    } else {
        None
    };
    Some(TypeDesc { vt, value, child })
}

pub fn parse(bytes: &[u8]) -> Result<Library, Error> {
    parse_inner(bytes).ok_or(Error)
}
fn parse_inner(bytes: &[u8]) -> Option<Library> {
    if bytes.get(..4)? != b"MSFT" || word(bytes, 4)? != 0x0001_0002 {
        return None;
    }
    let count = word(bytes, 32)? as usize;
    if count > 16384 {
        return None;
    }
    let directory = 84usize
        .checked_add(count.checked_mul(4)?)?
        .checked_add(if word(bytes, 20)? & 0x100 != 0 { 4 } else { 0 })?;
    let mut segments: [&[u8]; 15] = [&[]; 15];
    for (index, segment) in segments.iter_mut().enumerate() {
        let entry = directory.checked_add(index * 16)?;
        let offset = word(bytes, entry)? as usize;
        let len = word(bytes, entry + 4)? as usize;
        if len != 0 {
            *segment = bytes.get(offset..offset.checked_add(len)?)?;
        }
    }
    let mut library = Library {
        guid: guid(segments[5], word(bytes, 8)?)?,
        lcid: word(bytes, 12)?,
        syskind: word(bytes, 20)? & 0xf,
        flags: word(bytes, 28)? as u16,
        version: version(word(bytes, 24)?),
        name: name(segments[7], word(bytes, 56)?)?,
        types: Vec::with_capacity(count),
        type_offsets: Vec::with_capacity(count),
    };
    for index in 0..count {
        let offset = word(bytes, 84 + index * 4)?;
        library.type_offsets.push(offset);
        let record = segments[0].get(offset as usize..(offset as usize).checked_add(100)?)?;
        let kind = word(record, 0)?;
        let elements = word(record, 24)?;
        let function_count = (elements & 0xffff) as usize;
        let variable_count = (elements >> 16) as usize;
        let members = function_count.checked_add(variable_count)?;
        let packed = word(record, 76)?;
        let impl_count = (packed & 0xffff) as usize;
        if impl_count > 16384 {
            return None;
        }
        let guid_offset = word(record, 44)?;
        let mut info = TypeInfo {
            guid: if guid_offset == u32::MAX {
                [0; 16]
            } else {
                guid(segments[5], guid_offset)?
            },
            name: name(segments[7], word(record, 52)?)?,
            kind: kind & 0xf,
            flags: word(record, 48)? as u16,
            version: version(word(record, 56)?),
            size: word(record, 80)?,
            alignment: ((kind >> 11) & 0x1f) as u16,
            vtable_size: (packed >> 16) as u16,
            variables: variable_count as u16,
            implementations: Vec::new(),
            functions: Vec::with_capacity(function_count),
            alias: if kind & 0xf == 6 {
                Some(desc(segments[9], word(record, 84)?, 0)?)
            } else {
                None
            },
        };
        let mut reference = word(record, 84)?;
        for _ in 0..impl_count {
            if info.kind == 5 {
                let start = reference as usize;
                let target = word(segments[3], start)?;
                let flags = word(segments[3], start + 4)?;
                info.implementations.push((target, flags));
                reference = word(segments[3], start + 12)?;
            } else {
                info.implementations.push((reference, 0));
                break;
            }
        }
        if function_count != 0 {
            let data = word(record, 4)? as usize;
            let len = word(bytes, data)? as usize;
            let start = data.checked_add(4)?;
            let end = start.checked_add(len)?;
            let serialized = bytes.get(start..end)?;
            let stride = members.checked_mul(4)?;
            bytes.get(end..end.checked_add(stride.checked_mul(3)?)?)?;
            for member in 0..function_count {
                let id = word(bytes, end + member * 4)? as i32;
                let member_name = name(segments[7], word(bytes, end + stride + member * 4)?)?;
                let position = word(bytes, end + 2 * stride + member * 4)? as usize;
                let record_len = (word(serialized, position)? & 0xffff) as usize;
                let function = serialized.get(position..position.checked_add(record_len)?)?;
                let args = word(function, 20)?;
                let n = (args & 0xffff) as usize;
                let params_at = record_len.checked_sub(n.checked_mul(12)?)?;
                if params_at < 24 {
                    return None;
                }
                let flags = word(function, 16)?;
                let mut parameters = Vec::with_capacity(n);
                for parameter in 0..n {
                    let base = params_at + parameter * 12;
                    parameters.push(Parameter {
                        ty: desc(segments[9], word(function, base)?, 0)?,
                        name: name(segments[7], word(function, base + 4)?)?,
                        flags: word(function, base + 8)? as u16,
                    });
                }
                info.functions.push(Function {
                    id,
                    name: member_name,
                    result: desc(segments[9], word(function, 4)?, 0)?,
                    kind: flags & 7,
                    invoke: (flags >> 3) & 15,
                    callconv: (flags >> 8) & 15,
                    flags: word(function, 8)? as u16,
                    vtable_offset: word(function, 12)? as u16,
                    optional: (args >> 16) as i16,
                    parameters,
                });
            }
        }
        library.types.push(info);
    }
    Some(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 576];
        let put = |bytes: &mut [u8], at: usize, value: u32| {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes())
        };
        put(&mut bytes, 0, 0x5446534d);
        put(&mut bytes, 4, 0x00010002);
        put(&mut bytes, 20, 1);
        put(&mut bytes, 24, 1);
        put(&mut bytes, 32, 1);
        for i in 0..15 {
            put(&mut bytes, 88 + i * 16, u32::MAX);
        }
        for (index, offset, len) in [(0, 328, 100), (5, 428, 48), (7, 476, 52)] {
            put(&mut bytes, 88 + index * 16, offset);
            put(&mut bytes, 92 + index * 16, len);
        }
        bytes[428..444].fill(1);
        bytes[452..468].fill(2);
        put(&mut bytes, 328, 4 | (4 << 11));
        put(&mut bytes, 332, 528);
        put(&mut bytes, 352, 1);
        put(&mut bytes, 372, 24);
        put(&mut bytes, 380, 24);
        put(&mut bytes, 404, 28 << 16);
        put(&mut bytes, 408, 4);
        for (start, text) in [(476, "Library"), (500, "Interface")] {
            put(&mut bytes, start + 8, text.len() as u32);
            bytes[start + 12..start + 12 + text.len()].copy_from_slice(text.as_bytes());
        }
        put(&mut bytes, 528, 24);
        put(&mut bytes, 532, 24);
        put(&mut bytes, 536, 0x80190019);
        put(&mut bytes, 544, 28);
        put(&mut bytes, 548, 0x0409);
        put(&mut bytes, 556, 42);
        put(&mut bytes, 560, 24);
        put(&mut bytes, 564, 0);
        bytes.truncate(568);
        bytes
    }
    #[test]
    fn reads_interface_identity_and_function_abi_from_metadata() {
        let value = parse(&fixture()).unwrap();
        assert_eq!(value.guid, [1; 16]);
        assert_eq!(value.name, "Library");
        assert_eq!(value.types[0].guid, [2; 16]);
        assert_eq!(value.types[0].name, "Interface");
        let function = &value.types[0].functions[0];
        assert_eq!(function.id, 42);
        assert_eq!(function.result.vt, 25);
        assert_eq!(function.vtable_offset, 28);
        assert_eq!(function.callconv, 4);
    }
    #[test]
    fn rejects_truncated_and_oversized_tables_without_panicking() {
        let bytes = fixture();
        for length in [0, 4, 83, 100, 327, 500, 560] {
            assert!(parse(&bytes[..length]).is_err());
        }
        let mut oversized = bytes;
        oversized[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&oversized).is_err());
    }
}
