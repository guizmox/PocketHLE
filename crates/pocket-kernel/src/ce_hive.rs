//! Read-only parser for CE's MIKE hive format, not the desktop REGF format.
//! The supplied CE 6 boot/default/user hives use indexed allocation cells;
//! key/value chains run backwards and names are counted UTF-16 strings.
use crate::registry::RegistryValue;
use std::collections::HashSet;

#[derive(Debug, Default, Clone)]
pub struct Hive {
    pub keys: Vec<String>,
    pub values: Vec<(String, String, RegistryValue)>,
}
const CELLS_PER_TABLE: usize = 1024;
const TABLE_HEADER: u32 = 0x2000_1004;
struct Reader<'a> {
    bytes: &'a [u8],
    base: usize,
    tables: Vec<usize>,
    count: usize,
}
impl Reader<'_> {
    fn bytes(&self, at: usize, count: usize) -> Result<&[u8], String> {
        self.bytes
            .get(at..at.checked_add(count).ok_or("hive offset overflow")?)
            .ok_or_else(|| "truncated CE registry hive".into())
    }
    fn u32(&self, at: usize) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    fn u16(&self, at: usize) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes(at, 2)?.try_into().unwrap()))
    }
    fn text(&self, at: usize, units: usize) -> Result<String, String> {
        let units: Vec<_> = self
            .bytes(at, units.checked_mul(2).ok_or("hive string overflow")?)?
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        String::from_utf16(&units).map_err(|_| "invalid UTF-16 in CE hive".into())
    }
    fn cell(&self, handle: u32, tag: u32) -> Result<(usize, usize), String> {
        let index = (handle & 0x00ff_ffff) as usize;
        if index >= self.count {
            return Err("CE hive cell index out of range".into());
        }
        let table = self.tables[index / CELLS_PER_TABLE];
        let offset = self.u32(table + 12 + (index % CELLS_PER_TABLE) * 4)?;
        if offset & 3 != 1 {
            return Err("CE hive references an unallocated cell".into());
        }
        // Every table stores offsets from the first allocation block, even
        // when the table itself lives later among packed key/value cells.
        let at = self
            .base
            .checked_add((offset & !3) as usize)
            .ok_or("hive cell overflow")?;
        let header = self.u32(at)?;
        let size = (header & 0x0fff_ffff) as usize + 12;
        self.bytes(at, size)?;
        if header >> 28 != tag
            || size < if tag == 13 { 22 } else { 28 }
            || self.u32(at + 8)? != index as u32
        {
            return Err("invalid CE hive cell header".into());
        }
        Ok((at, size))
    }
    fn chain(
        &self,
        first: u32,
        path: &str,
        depth: usize,
        seen: &mut HashSet<u32>,
        hive: &mut Hive,
    ) -> Result<(), String> {
        if depth > 128 {
            return Err("CE hive nesting exceeds 128 keys".into());
        }
        let mut handle = first;
        while handle != 0 {
            if !seen.insert(handle & 0x00ff_ffff) {
                return Err("cycle or aliased key in CE hive".into());
            }
            let (at, size) = self.cell(handle, 12)?;
            let chars = self.u32(at + 24)? as usize;
            if chars == 0 || chars > (size - 28) / 2 {
                return Err("invalid CE hive key length".into());
            }
            let name = self.text(at + 28, chars)?;
            if name.contains(['\0', '\\']) {
                return Err("invalid CE hive key name".into());
            }
            let key = format!("{path}\\{name}");
            hive.keys.push(key.clone());
            let mut value = self.u32(at + 20)?;
            while value != 0 {
                if !seen.insert(value & 0x00ff_ffff) {
                    return Err("cycle or aliased value in CE hive".into());
                }
                let (vp, vs) = self.cell(value, 13)?;
                let kind = u32::from(self.u16(vp + 16)?);
                let len = self.u16(vp + 18)? as usize;
                let nc = self.u16(vp + 20)? as usize;
                let data_at = 22usize.checked_add(nc * 2).ok_or("hive value overflow")?;
                if data_at.checked_add(len).is_none_or(|end| end > vs) {
                    return Err("invalid CE hive value length".into());
                }
                let name = self.text(vp + 22, nc)?;
                if name.contains('\0') {
                    return Err("invalid CE hive value name".into());
                }
                let bytes = self.bytes(vp + data_at, len)?.to_vec();
                let decoded = match kind {
                    1 if len % 2 == 0 => RegistryValue::Sz(
                        self.text(vp + data_at, len / 2)?
                            .trim_end_matches('\0')
                            .into(),
                    ),
                    3 => RegistryValue::Binary(bytes),
                    4 if len == 4 => {
                        RegistryValue::Dword(u32::from_le_bytes(bytes.try_into().unwrap()))
                    }
                    1 | 4 => return Err("invalid CE hive string/DWORD size".into()),
                    _ => RegistryValue::Raw { kind, bytes },
                };
                hive.values.push((key.clone(), name, decoded));
                value = self.u32(vp + 12)?;
            }
            self.chain(self.u32(at + 16)?, &key, depth + 1, seen, hive)?;
            handle = self.u32(at + 12)?;
        }
        Ok(())
    }
}

pub fn parse(bytes: &[u8]) -> Result<Hive, String> {
    if bytes.len() < 0x118 || bytes.len() > 64 * 1024 * 1024 || bytes.get(8..12) != Some(b"EKIM") {
        return Err("not a Windows CE MIKE registry hive".into());
    }
    // The header records the last index in the final allocation table, not
    // the total number of cells. The CE 6 wince6 default.hv spans three tables.
    let last_index = u32::from_le_bytes(bytes[0x114..0x118].try_into().unwrap()) as usize;
    if last_index >= CELLS_PER_TABLE {
        return Err("invalid CE hive last cell index".into());
    }
    let base = (0..bytes.len().saturating_sub(12))
        .step_by(4096)
        .find(|&at| {
            bytes[at..at + 4] == TABLE_HEADER.to_le_bytes() && bytes[at + 8..at + 12] == [0; 4]
        })
        .ok_or("missing CE hive allocation table")?;
    if base <= 0x1000 {
        return Err("missing CE hive table directory".into());
    }
    let mut reader = Reader {
        bytes,
        base,
        tables: Vec::new(),
        count: 0,
    };
    // The directory starts at 0x1000. Entry zero names the base itself;
    // subsequent offsets can be unaligned to a page and terminate with zero.
    for slot in 0..(base - 0x1000) / 4 {
        let offset = reader.u32(0x1000 + slot * 4)? as usize;
        if slot != 0 && offset == 0 {
            break;
        }
        if slot == 0 && offset != 0 {
            return Err("invalid CE hive first table offset".into());
        }
        let table = base
            .checked_add(offset)
            .ok_or("CE hive table offset overflow")?;
        if offset & 3 != 0
            || reader.u32(table)? != TABLE_HEADER
            || reader.u32(table + 8)? as usize != slot
        {
            return Err("invalid CE hive allocation table directory".into());
        }
        reader.bytes(table, 12 + CELLS_PER_TABLE * 4 + 4)?;
        reader.tables.push(table);
    }
    reader.count = reader
        .tables
        .len()
        .checked_sub(1)
        .and_then(|n| n.checked_mul(CELLS_PER_TABLE))
        .and_then(|n| n.checked_add(last_index + 1))
        .ok_or("CE hive cell count overflow")?;
    let (root, _) = reader.cell(0, 11)?;
    let mut seen = HashSet::new();
    let mut result = Hive::default();
    for (index, prefix) in ["HKCR", "HKCU", "HKLM", "HKU"].into_iter().enumerate() {
        reader.chain(
            reader.u32(root + 12 + index * 4)?,
            prefix,
            0,
            &mut seen,
            &mut result,
        )?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: usize = 0x5000;
    const TABLE_BYTES: usize = 12 + CELLS_PER_TABLE * 4 + 4;

    struct Fixture {
        bytes: Vec<u8>,
        tables: Vec<usize>,
        cursor: usize,
    }

    impl Fixture {
        fn new(table_count: usize, last_index: u32) -> Self {
            // CE 6 default.hv packs later tables between cells, off page
            // boundaries. Keep that layout in the fixture to catch rebasing.
            let tables: Vec<_> = (0..table_count).map(|i| BASE + i * 0x150c).collect();
            let cursor = tables.last().unwrap() + TABLE_BYTES;
            let mut result = Self {
                bytes: vec![0; cursor + 0x2000],
                tables,
                cursor,
            };
            result.bytes[8..12].copy_from_slice(b"EKIM");
            result.dword(0x114, last_index);
            for i in 0..table_count {
                let table = result.tables[i];
                result.dword(0x1000 + i * 4, (table - BASE) as u32);
                result.dword(table, TABLE_HEADER);
                result.dword(table + 8, i as u32);
            }
            result
        }

        fn dword(&mut self, at: usize, value: u32) {
            self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn cell(&mut self, index: usize, tag: u32, payload: &[u8]) -> usize {
            let at = self.cursor;
            let size = (12 + payload.len() + 3) & !3;
            self.cursor += size;
            self.dword(
                self.tables[index / CELLS_PER_TABLE] + 12 + index % CELLS_PER_TABLE * 4,
                ((at - BASE) as u32) | 1,
            );
            self.dword(at, (tag << 28) | (size - 12) as u32);
            self.dword(at + 8, index as u32);
            self.bytes[at + 12..at + 12 + payload.len()].copy_from_slice(payload);
            at
        }

        fn root(&mut self, roots: [u32; 4]) {
            self.cell(
                0,
                11,
                &roots
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            );
        }

        fn key(&mut self, index: usize, name: &str, sibling: u32, child: u32, value: u32) -> usize {
            let text: Vec<_> = name.encode_utf16().collect();
            let mut payload: Vec<_> = [sibling, child, value, text.len() as u32]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            payload.extend(text.into_iter().flat_map(u16::to_le_bytes));
            self.cell(index, 12, &payload)
        }

        fn value(
            &mut self,
            index: usize,
            name: &str,
            kind: u16,
            data: &[u8],
            previous: u32,
        ) -> usize {
            let text: Vec<_> = name.encode_utf16().collect();
            let mut payload = previous.to_le_bytes().to_vec();
            payload.extend(kind.to_le_bytes());
            payload.extend((data.len() as u16).to_le_bytes());
            payload.extend((text.len() as u16).to_le_bytes());
            payload.extend(text.into_iter().flat_map(u16::to_le_bytes));
            payload.extend(data);
            self.cell(index, 13, &payload)
        }
    }

    fn handle(index: u32) -> u32 {
        0x2000_0000 | index
    }

    #[test]
    fn one_table_preserves_root_paths_unicode_and_value_types() {
        let mut f = Fixture::new(1, 6);
        f.root([0, handle(1), handle(6), 0]);
        f.key(1, "Desktop", 0, handle(2), 0);
        f.key(2, "\u{041e}\u{043a}\u{043d}\u{0430}", 0, 0, handle(5));
        f.value(3, "", 1, &[0x54, 0, 0, 0], 0);
        f.value(4, "Count", 4, &42u32.to_le_bytes(), handle(3));
        f.value(5, "Multi", 7, &[0x41, 0, 0, 0, 0, 0], handle(4));
        f.key(6, "Drivers", 0, 0, 0);
        let hive = parse(&f.bytes).unwrap();
        assert_eq!(
            hive.keys,
            [
                "HKCU\\Desktop",
                "HKCU\\Desktop\\\u{041e}\u{043a}\u{043d}\u{0430}",
                "HKLM\\Drivers"
            ]
        );
        assert_eq!(
            hive.values,
            [
                (
                    "HKCU\\Desktop\\\u{041e}\u{043a}\u{043d}\u{0430}".into(),
                    "Multi".into(),
                    RegistryValue::Raw {
                        kind: 7,
                        bytes: vec![0x41, 0, 0, 0, 0, 0]
                    }
                ),
                (
                    "HKCU\\Desktop\\\u{041e}\u{043a}\u{043d}\u{0430}".into(),
                    "Count".into(),
                    RegistryValue::Dword(42)
                ),
                (
                    "HKCU\\Desktop\\\u{041e}\u{043a}\u{043d}\u{0430}".into(),
                    "".into(),
                    RegistryValue::Sz("T".into())
                ),
            ]
        );
    }

    #[test]
    fn multiple_packed_tables_use_global_indices_and_primary_relative_offsets() {
        let mut f = Fixture::new(3, 1);
        f.root([0, 0, handle(2048), 0]);
        f.key(2048, "Explorer", 0, handle(1024), handle(2049));
        f.key(1024, "Settings", 0, 0, handle(1025));
        f.value(1025, "Flags", 4, &9u32.to_le_bytes(), 0);
        f.value(2049, "Data", 3, &[1, 2, 3], 0);
        let hive = parse(&f.bytes).unwrap();
        assert_eq!(hive.keys, ["HKLM\\Explorer", "HKLM\\Explorer\\Settings"]);
        assert_eq!(
            hive.values,
            [
                (
                    "HKLM\\Explorer".into(),
                    "Data".into(),
                    RegistryValue::Binary(vec![1, 2, 3])
                ),
                (
                    "HKLM\\Explorer\\Settings".into(),
                    "Flags".into(),
                    RegistryValue::Dword(9)
                ),
            ]
        );
    }

    #[test]
    fn corrupt_directory_and_out_of_range_cells_are_rejected() {
        let mut f = Fixture::new(2, 0);
        f.root([0, 0, handle(1024), 0]);
        f.key(1024, "Explorer", 0, 0, 0);
        let valid = f.bytes.clone();
        f.dword(0x1004, u32::MAX - 3);
        assert!(parse(&f.bytes).is_err());
        f.bytes = valid.clone();
        f.dword(f.tables[1] + 8, 0);
        assert!(parse(&f.bytes).unwrap_err().contains("directory"));
        f.bytes = valid.clone();
        let root = BASE
            + ((u32::from_le_bytes(f.bytes[BASE + 12..BASE + 16].try_into().unwrap()) & !3)
                as usize);
        f.dword(root + 20, handle(1025));
        assert!(parse(&f.bytes).unwrap_err().contains("index out of range"));
        f.bytes = valid;
        f.bytes.truncate(f.tables[1] + TABLE_BYTES - 1);
        assert!(parse(&f.bytes).is_err());
    }

    #[test]
    fn links_with_different_handle_flags_cannot_hide_key_or_value_cycles() {
        let mut f = Fixture::new(1, 2);
        f.root([0, 0, handle(1), 0]);
        let key = f.key(1, "Explorer", 0x1000_0001, 0, handle(2));
        let value = f.value(2, "Flag", 4, &1u32.to_le_bytes(), 0);
        assert!(parse(&f.bytes).unwrap_err().contains("cycle"));
        f.dword(key + 12, 0);
        f.dword(value + 12, 0x1000_0002);
        assert!(parse(&f.bytes).unwrap_err().contains("cycle"));
    }

    #[test]
    fn invalid_utf16_and_value_sizes_fail_before_import() {
        let mut f = Fixture::new(1, 2);
        f.root([0, 0, handle(1), 0]);
        let key = f.key(1, "A", 0, 0, handle(2));
        let value = f.value(2, "Flag", 4, &[1, 2, 3], 0);
        assert!(parse(&f.bytes).unwrap_err().contains("DWORD size"));
        f.bytes[key + 28..key + 30].copy_from_slice(&0xd800u16.to_le_bytes());
        assert!(parse(&f.bytes).unwrap_err().contains("UTF-16"));
        f.bytes[key + 28..key + 30].copy_from_slice(&65u16.to_le_bytes());
        f.bytes[value + 18..value + 20].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(parse(&f.bytes).unwrap_err().contains("value length"));
    }
}
