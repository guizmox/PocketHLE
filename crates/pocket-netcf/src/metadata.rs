//! PE32 and CLR metadata reader.

use crate::vm;
use std::str;

#[derive(Debug, PartialEq, Eq)]
pub struct EntryPoint<'a> {
    pub runtime_version: &'a str,
    pub token: u32,
    pub method_name: &'a str,
    pub max_stack: u16,
    pub local_signature: u32,
    pub has_extra_sections: bool,
    pub il: &'a [u8],
    pub(crate) extra_sections: &'a [u8],
}

fn range(bytes: &[u8], offset: usize, length: usize) -> Option<&[u8]> {
    bytes.get(offset..offset.checked_add(length)?)
}

pub(crate) fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        range(bytes, offset, 2)?.try_into().ok()?,
    ))
}

pub(crate) fn dword(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        range(bytes, offset, 4)?.try_into().ok()?,
    ))
}

pub(crate) fn aligned(value: usize) -> Option<usize> {
    Some(value.checked_add(3)? & !3)
}

fn string_at(bytes: &[u8], start: usize) -> Option<&str> {
    let tail = bytes.get(start..)?;
    let end = tail.iter().position(|&byte| byte == 0)?;
    str::from_utf8(&tail[..end]).ok()
}

/// Locate a managed IL entry method. Native entry points and malformed or
/// unsupported assemblies return None.
pub fn entry_point(bytes: &[u8]) -> Option<EntryPoint<'_>> {
    Assembly::read(bytes)?.entry_point()
}

/// Locate an IL method by MethodDef token in the same assembly.
pub fn method(bytes: &[u8], token: u32) -> Option<EntryPoint<'_>> {
    Assembly::read(bytes)?.method(token)
}

pub(crate) struct Assembly<'a> {
    bytes: &'a [u8],
    sections: &'a [u8],
    runtime_version: &'a str,
    entry: u32,
    strings: &'a [u8],
    blobs: &'a [u8],
    user_strings: &'a [u8],
    resources: &'a [u8],
    tables: &'a [u8],
    methods: usize,
    members: usize,
    type_defs: usize,
    type_def_size: usize,
    type_refs: usize,
    fields: usize,
    rows: [usize; 64],
    string_size: usize,
    blob_size: usize,
}

fn mapped<'a>(bytes: &'a [u8], sections: &[u8], rva: u32) -> Option<&'a [u8]> {
    for section in sections.chunks_exact(40) {
        let base = dword(section, 12)?;
        let length = dword(section, 16)?;
        if let Some(delta) = rva.checked_sub(base).filter(|&delta| delta < length) {
            let raw = range(bytes, dword(section, 20)? as usize, length as usize)?;
            return raw.get(delta as usize..);
        }
    }
    None
}

fn index_width(rows: usize) -> usize {
    if rows < 65536 {
        2
    } else {
        4
    }
}

fn coded_width(rows: &[usize; 64], bits: u32, targets: &[usize]) -> usize {
    if targets
        .iter()
        .any(|&table| rows[table] >= (65536usize >> bits))
    {
        4
    } else {
        2
    }
}

fn read_index(bytes: &[u8], offset: usize, width: usize) -> Option<usize> {
    if width == 2 {
        Some(word(bytes, offset)? as usize)
    } else {
        Some(dword(bytes, offset)? as usize)
    }
}

impl<'a> Assembly<'a> {
    pub(crate) fn read(bytes: &'a [u8]) -> Option<Self> {
        if !bytes.starts_with(b"MZ") {
            return None;
        }
        let pe = bytes.get(dword(bytes, 60)? as usize..)?;
        if range(pe, 0, 4)? != b"PE\0\0" {
            return None;
        }
        let optional_size = word(pe, 20)? as usize;
        let optional = range(pe, 24, optional_size)?;
        if word(optional, 0)? != 0x10b || dword(optional, 92)? < 15 {
            return None;
        }
        let section_count = word(pe, 6)? as usize;
        let sections = range(pe, 24 + optional_size, section_count * 40)?;
        let clr = mapped(bytes, sections, dword(optional, 208)?)?;
        let flags = dword(clr, 16)?;
        let entry = dword(clr, 20)?;
        if flags & 0x10 != 0 {
            return None;
        }
        let metadata = range(
            mapped(bytes, sections, dword(clr, 8)?)?,
            0,
            dword(clr, 12)? as usize,
        )?;
        if !metadata.starts_with(b"BSJB") {
            return None;
        }
        let version_len = dword(metadata, 12)? as usize;
        let runtime_version = string_at(range(metadata, 16, version_len)?, 0)?;
        let stream_header = aligned(16usize.checked_add(version_len)?)?;
        let count = word(metadata, stream_header + 2)?;
        let mut cursor = stream_header + 4;
        let mut tables = None;
        let mut strings = None;
        let mut blobs = None;
        let mut user_strings = &[][..];
        for _ in 0..count {
            let offset = dword(metadata, cursor)? as usize;
            let length = dword(metadata, cursor + 4)? as usize;
            let name = string_at(metadata, cursor + 8)?;
            match name {
                "#~" | "#-" => tables = Some(range(metadata, offset, length)?),
                "#Strings" => strings = Some(range(metadata, offset, length)?),
                "#Blob" => blobs = Some(range(metadata, offset, length)?),
                "#US" => user_strings = range(metadata, offset, length)?,
                _ => {}
            }
            cursor = aligned(cursor.checked_add(9 + name.len())?)?;
        }
        let tables = tables?;
        let heaps = *tables.get(6)?;
        let valid = u64::from_le_bytes(range(tables, 8, 8)?.try_into().ok()?);
        let mut rows = [0usize; 64];
        let mut cursor = 24;
        for (table, count) in rows.iter_mut().enumerate() {
            if valid & (1 << table) != 0 {
                *count = dword(tables, cursor)? as usize;
                cursor += 4;
            }
        }
        if heaps & 0x40 != 0 {
            cursor += 4;
        }
        let heap = |bit| if heaps & bit == 0 { 2 } else { 4 };
        let coded = |bits, targets: &[usize]| coded_width(&rows, bits, targets);
        let str_size = heap(1);
        let blob_size = heap(4);
        let preceding = [
            2 + str_size + 3 * heap(2),
            coded(2, &[0, 26, 35, 1]) + 2 * str_size,
            4 + 2 * str_size
                + coded(2, &[2, 1, 27])
                + index_width(rows[3].max(rows[4]))
                + index_width(rows[5].max(rows[6])),
            index_width(rows[4]),
            2 + str_size + blob_size,
            index_width(rows[6]),
        ];
        let type_refs = cursor.checked_add(rows[0].checked_mul(preceding[0])?)?;
        let type_defs = type_refs.checked_add(rows[1].checked_mul(preceding[1])?)?;
        let type_def_size = preceding[2];
        let mut fields = cursor;
        for (count, size) in rows[..4].iter().zip(preceding) {
            fields = fields.checked_add(count.checked_mul(size)?)?;
        }
        for (count, size) in rows.iter().zip(preceding) {
            cursor = cursor.checked_add(count.checked_mul(size)?)?;
        }
        let mut members = cursor;
        for (count, size) in rows[6..10].iter().zip([
            8 + str_size + blob_size + index_width(rows[7].max(rows[8])),
            index_width(rows[8]),
            4 + str_size,
            index_width(rows[2]) + coded(2, &[2, 1, 27]),
        ]) {
            members = members.checked_add(count.checked_mul(size)?)?;
        }
        Some(Self {
            bytes,
            sections,
            runtime_version,
            entry,
            strings: strings?,
            blobs: blobs?,
            user_strings,
            resources: match dword(clr, 24)? {
                0 => &[],
                rva => range(mapped(bytes, sections, rva)?, 0, dword(clr, 28)? as usize)?,
            },
            tables,
            methods: cursor,
            members,
            type_defs,
            type_def_size,
            type_refs,
            fields,
            rows,
            string_size: str_size,
            blob_size,
        })
    }

    pub(crate) fn entry_point(&self) -> Option<EntryPoint<'a>> {
        self.method(self.entry)
    }

    fn method_row_size(&self) -> usize {
        8 + self.string_size + self.blob_size + index_width(self.rows[7].max(self.rows[8]))
    }

    fn method_row(&self, token: u32) -> Option<&'a [u8]> {
        let row = (token & 0xffffff) as usize;
        if token >> 24 != 6 || row == 0 || row > self.rows[6] {
            return None;
        }
        let size = self.method_row_size();
        range(
            self.tables,
            self.methods.checked_add((row - 1).checked_mul(size)?)?,
            size,
        )
    }

    pub(crate) fn method(&self, token: u32) -> Option<EntryPoint<'a>> {
        let method = self.method_row(token)?;
        let rva = dword(method, 0)?;
        if rva == 0 || word(method, 4)? & 7 != 0 {
            return None;
        }
        let name_index = read_index(method, 8, self.string_size)?;
        let method_name = string_at(self.strings, name_index)?;
        let body = mapped(self.bytes, self.sections, rva)?;
        let first = *body.first()?;
        let (header, length, max_stack, local_signature, has_extra_sections) = match first & 3 {
            2 => (1, usize::from(first >> 2), 8, 0, false),
            3 => {
                let flags = word(body, 0)?;
                let header = usize::from(flags >> 12) * 4;
                if header < 12 {
                    return None;
                }
                (
                    header,
                    dword(body, 4)? as usize,
                    word(body, 2)?,
                    dword(body, 8)?,
                    flags & 8 != 0,
                )
            }
            _ => return None,
        };
        Some(EntryPoint {
            runtime_version: self.runtime_version,
            token,
            method_name,
            max_stack,
            local_signature,
            has_extra_sections,
            il: range(body, header, length)?,
            extra_sections: if has_extra_sections {
                body.get(aligned(header.checked_add(length)?)?..)?
            } else {
                &[]
            },
        })
    }
}

fn compressed(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let first = usize::from(*bytes.first()?);
    let (value, size) = match first {
        0..=0x7f => (first, 1),
        0x80..=0xbf => (((first & 0x3f) << 8) | usize::from(*bytes.get(1)?), 2),
        0xc0..=0xdf => {
            let tail = range(bytes, 1, 3)?;
            (
                ((first & 0x1f) << 24)
                    | (usize::from(tail[0]) << 16)
                    | (usize::from(tail[1]) << 8)
                    | usize::from(tail[2]),
                4,
            )
        }
        _ => return None,
    };
    Some((value, bytes.get(size..)?))
}

impl<'a> Assembly<'a> {
    fn member_row(&self, token: u32) -> Option<(&'a [u8], usize)> {
        let row = (token & 0xffffff) as usize;
        if token >> 24 != 10 || row == 0 || row > self.rows[10] {
            return None;
        }
        let parent_size = coded_width(&self.rows, 3, &[2, 1, 26, 6, 27]);
        let size = parent_size + self.string_size + self.blob_size;
        let member = range(
            self.tables,
            self.members.checked_add((row - 1).checked_mul(size)?)?,
            size,
        )?;
        Some((member, parent_size))
    }

    fn type_method_list(&self, index: usize) -> Option<usize> {
        if index == self.rows[2] {
            return self.rows[6].checked_add(1);
        }
        let row = self.type_row(index)?;
        let width = index_width(self.rows[5].max(self.rows[6]));
        read_index(row, row.len().checked_sub(width)?, width)
    }

    fn type_field_list(&self, index: usize) -> Option<usize> {
        if index == self.rows[2] {
            return self.rows[4].checked_add(1);
        }
        let row = self.type_row(index)?;
        let methods = index_width(self.rows[5].max(self.rows[6]));
        let fields = index_width(self.rows[3].max(self.rows[4]));
        read_index(row, row.len().checked_sub(methods + fields)?, fields)
    }

    fn type_row(&self, index: usize) -> Option<&'a [u8]> {
        if index >= self.rows[2] {
            return None;
        }
        range(
            self.tables,
            self.type_defs
                .checked_add(index.checked_mul(self.type_def_size)?)?,
            self.type_def_size,
        )
    }

    fn type_method(
        &self,
        index: usize,
        name: &str,
        instance: bool,
        parameters: usize,
    ) -> Option<u32> {
        let start = self.type_method_list(index)?;
        let end = self
            .type_method_list(index + 1)?
            .min(self.rows[6].checked_add(1)?);
        for row in start..end {
            let token = 0x06000000 | row as u32;
            let Some(candidate) = self.method(token) else {
                continue;
            };
            if candidate.method_name == name
                && self.signature(token).is_some_and(|sig| {
                    sig.instance == instance && sig.parameters == parameters && !sig.returns_value
                })
            {
                return Some(token);
            }
        }
        None
    }

    pub(crate) fn override_method(&self, owner: u32, name: &str, parameters: usize) -> Option<u32> {
        if owner >> 24 != 6 {
            return None;
        }
        let method = (owner & 0xffffff) as usize;
        for index in 0..self.rows[2] {
            if (self.type_method_list(index)?..self.type_method_list(index + 1)?).contains(&method)
            {
                return self.type_method(index, name, true, parameters);
            }
        }
        None
    }

    pub(crate) fn primitive_fields(
        &self,
        token: u32,
    ) -> Option<(&'a str, Vec<(u32, &'a str, u8)>)> {
        let index = ((token & 0xffffff) as usize).checked_sub(1)?;
        if token >> 24 != 2 {
            return None;
        }
        let row = self.type_row(index)?;
        let name = string_at(self.strings, read_index(row, 4, self.string_size)?)?;
        let start = self.type_field_list(index)?;
        let end = self
            .type_field_list(index + 1)?
            .min(self.rows[4].checked_add(1)?);
        let mut fields = Vec::new();
        for field in start..end {
            let size = 2 + self.string_size + self.blob_size;
            let row = range(
                self.tables,
                self.fields
                    .checked_add(field.checked_sub(1)?.checked_mul(size)?)?,
                size,
            )?;
            let flags = word(row, 0)?;
            if flags & 7 != 6 || flags & 0x10 != 0 {
                continue;
            }
            let key = read_index(row, 2, self.string_size)?;
            let blob = read_index(row, 2 + self.string_size, self.blob_size)?;
            let (length, signature) = compressed(self.blobs.get(blob..)?)?;
            let signature = signature.get(..length)?;
            if let [6, kind @ (2 | 8 | 12)] = signature {
                fields.push((
                    0x04000000 | field as u32,
                    string_at(self.strings, key)?,
                    *kind,
                ));
            }
        }
        Some((name, fields))
    }

    pub(crate) fn type_owns_method(&self, ty: u32, method: u32) -> bool {
        if ty >> 24 != 2 || method >> 24 != 6 {
            return false;
        }
        let Some(index) = ((ty & 0xffffff) as usize).checked_sub(1) else {
            return false;
        };
        self.type_method_list(index)
            .zip(self.type_method_list(index + 1))
            .is_some_and(|(start, end)| (start..end).contains(&((method & 0xffffff) as usize)))
    }

    pub(crate) fn constructor(&self, token: u32) -> Option<u32> {
        if token >> 24 != 2 {
            return None;
        }
        self.type_method(
            ((token & 0xffffff) as usize).checked_sub(1)?,
            ".ctor",
            true,
            0,
        )
    }

    pub(crate) fn field_default(&self, token: u32) -> vm::Value {
        use vm::Value;
        let field = (token & 0xffffff) as usize;
        if token >> 24 != 4 || field == 0 || field > self.rows[4] {
            return Value::Null;
        }
        let read = || {
            let size = 2 + self.string_size + self.blob_size;
            let row = range(
                self.tables,
                self.fields.checked_add((field - 1).checked_mul(size)?)?,
                size,
            )?;
            let index = read_index(row, 2 + self.string_size, self.blob_size)?;
            let (length, signature) = compressed(self.blobs.get(index..)?)?;
            let signature = signature.get(..length)?;
            if signature.first() != Some(&6) {
                return None;
            }
            Some(match *signature.get(1)? {
                0x02..=0x09 | 0x18 | 0x19 => Value::Int(0),
                0x0c => Value::Float(0),
                0x0d => Value::Double(0),
                _ => Value::Null,
            })
        };
        read().unwrap_or(Value::Null)
    }

    pub(crate) fn static_initializer(&self, field: u32) -> Option<u32> {
        if field >> 24 != 4 {
            return None;
        }
        let field = (field & 0xffffff) as usize;
        for index in 0..self.rows[2] {
            if (self.type_field_list(index)?..self.type_field_list(index + 1)?).contains(&field) {
                return self.type_method(index, ".cctor", false, 0);
            }
        }
        None
    }

    pub(crate) fn user_string(&self, token: u32) -> Option<String> {
        if token >> 24 != 0x70 {
            return None;
        }
        let (length, bytes) = compressed(self.user_strings.get((token & 0xffffff) as usize..)?)?;
        if length == 0 {
            return Some(String::new());
        }
        if length % 2 != 1 {
            return None;
        }
        String::from_utf16(
            &bytes
                .get(..length - 1)?
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        )
        .ok()
    }

    pub(crate) fn field_data(&self, token: u32, length: usize) -> Option<&'a [u8]> {
        let field = (token & 0xffffff) as usize;
        if token >> 24 != 4 || field == 0 || field > self.rows[4] {
            return None;
        }
        let cursor = self.table_after_members(29)?;
        let idx = index_width(self.rows[4]);
        let size = 4 + idx;
        for row in 0..self.rows[29] {
            let record = range(
                self.tables,
                cursor.checked_add(row.checked_mul(size)?)?,
                size,
            )?;
            if read_index(record, 4, idx)? == field {
                return range(
                    mapped(self.bytes, self.sections, dword(record, 0)?)?,
                    0,
                    length,
                );
            }
        }
        None
    }

    fn table_after_members(&self, table: usize) -> Option<usize> {
        let idx = |table: usize| index_width(self.rows[table]);
        let coded = |bits, tables: &[usize]| coded_width(&self.rows, bits, tables);
        let strings = self.string_size;
        let blob = self.blob_size;
        let sizes = [
            coded(3, &[2, 1, 26, 6, 27]) + strings + blob,
            2 + coded(2, &[4, 8, 23]) + blob,
            coded(
                5,
                &[
                    6, 4, 1, 2, 8, 9, 10, 0, 14, 23, 20, 17, 26, 27, 32, 35, 38, 39, 40, 42, 44, 43,
                ],
            ) + coded(3, &[6, 10])
                + blob,
            coded(1, &[4, 8]) + blob,
            2 + coded(2, &[2, 6, 32]) + blob,
            6 + idx(2),
            4 + idx(4),
            blob,
            idx(2) + index_width(self.rows[19].max(self.rows[20])),
            idx(20),
            2 + strings + coded(2, &[2, 1, 27]),
            idx(2) + index_width(self.rows[22].max(self.rows[23])),
            idx(23),
            2 + strings + blob,
            2 + idx(6) + coded(1, &[20, 23]),
            idx(2) + 2 * coded(1, &[6, 10]),
            strings,
            blob,
            2 + coded(1, &[4, 6]) + strings + idx(26),
        ];
        let sizes_after_field_rva = [
            4 + idx(4),
            8,
            4,
            16 + blob + 2 * strings,
            4,
            12,
            12 + 2 * blob + 2 * strings,
            4 + idx(35),
            12 + idx(35),
            4 + strings + blob,
            8 + 2 * strings + coded(2, &[38, 35, 39]),
            8 + strings + coded(2, &[38, 35, 39]),
            idx(2) * 2,
            4 + coded(1, &[2, 6]) + strings,
            coded(1, &[6, 10]) + blob,
        ];
        let mut cursor = self.members;
        for (count, size) in self.rows[10..table]
            .iter()
            .zip(sizes.into_iter().chain(sizes_after_field_rva))
        {
            cursor = cursor.checked_add(count.checked_mul(size)?)?;
        }
        Some(cursor)
    }

    pub(crate) fn resource_type_name(&self, token: u32) -> Option<String> {
        if token >> 24 != 2 {
            return None;
        }
        let row = self.type_row(((token & 0xffffff) as usize).checked_sub(1)?)?;
        let name = string_at(self.strings, read_index(row, 4, self.string_size)?)?;
        let namespace = string_at(
            self.strings,
            read_index(row, 4 + self.string_size, self.string_size)?,
        )?;
        Some(if namespace.is_empty() {
            format!("{name}.resources")
        } else {
            format!("{namespace}.{name}.resources")
        })
    }

    pub(crate) fn resource(&self, name: &str) -> Option<&'a [u8]> {
        let start = self.table_after_members(40)?;
        let implementation = coded_width(&self.rows, 2, &[38, 35, 39]);
        let size = 8 + self.string_size + implementation;
        for row in 0..self.rows[40] {
            let entry = range(
                self.tables,
                start.checked_add(row.checked_mul(size)?)?,
                size,
            )?;
            let key = read_index(entry, 8, self.string_size)?;
            if string_at(self.strings, key)? == name
                && read_index(entry, 8 + self.string_size, implementation)? == 0
            {
                let data = self.resources.get(dword(entry, 0)? as usize..)?;
                return range(data, 4, dword(data, 0)? as usize);
            }
        }
        None
    }

    pub(crate) fn pinvoke(&self, token: u32) -> Option<(&str, &str)> {
        self.method_row(token)?;
        let scope_width = index_width(self.rows[26]);
        let forwarded = coded_width(&self.rows, 1, &[4, 6]);
        let size = 2 + forwarded + self.string_size + scope_width;
        let start = self.table_after_members(28)?;
        for index in 0..self.rows[28] {
            let row = range(
                self.tables,
                start.checked_add(index.checked_mul(size)?)?,
                size,
            )?;
            if read_index(row, 2, forwarded)? != (((token & 0xffffff) as usize) << 1 | 1) {
                continue;
            }
            let name = string_at(
                self.strings,
                read_index(row, 2 + forwarded, self.string_size)?,
            )?;
            let scope =
                read_index(row, 2 + forwarded + self.string_size, scope_width)?.checked_sub(1)?;
            if scope >= self.rows[26] {
                return None;
            }
            let module = self
                .table_after_members(26)?
                .checked_add(scope.checked_mul(self.string_size)?)?;
            return Some((
                string_at(
                    self.strings,
                    read_index(self.tables, module, self.string_size)?,
                )?,
                name,
            ));
        }
        None
    }

    pub(crate) fn method_spec(&self, token: u32) -> Option<(u32, &[u8])> {
        let row = (token & 0xffffff) as usize;
        if token >> 24 != 43 || !(1..=self.rows[43]).contains(&row) {
            return None;
        }
        let width = coded_width(&self.rows, 1, &[6, 10]);
        let size = width + self.blob_size;
        let offset = self
            .table_after_members(43)?
            .checked_add((row - 1).checked_mul(size)?)?;
        let row = range(self.tables, offset, size)?;
        let target = read_index(row, 0, width)?;
        let blob = read_index(row, width, self.blob_size)?;
        let (length, bytes) = compressed(self.blobs.get(blob..)?)?;
        Some((
            (if target & 1 == 0 { 6 } else { 10 }) << 24 | (target >> 1) as u32,
            bytes.get(..length)?,
        ))
    }

    fn method_signature_blob(&self, token: u32) -> Option<&[u8]> {
        let token = self.method_spec(token).map_or(token, |(target, _)| target);
        let (row, offset) = if token >> 24 == 6 {
            (self.method_row(token)?, 8 + self.string_size)
        } else {
            let (row, parent_size) = self.member_row(token)?;
            (row, parent_size + self.string_size)
        };
        let index = read_index(row, offset, self.blob_size)?;
        let (length, blob) = compressed(self.blobs.get(index..)?)?;
        blob.get(..length)
    }

    pub(crate) fn signature(&self, token: u32) -> Option<vm::Signature> {
        let (&flags, mut signature) = self.method_signature_blob(token)?.split_first()?;
        if flags & 0x10 != 0 {
            (_, signature) = compressed(signature)?;
        }
        let (parameters, result) = compressed(signature)?;
        Some(vm::Signature {
            parameters,
            instance: flags & 0x20 != 0,
            returns_value: *result.first()? != 1,
        })
    }

    pub(crate) fn member_type(&self, token: u32) -> Option<(&'a str, &'a str)> {
        let (row, parent_size) = self.member_row(token)?;
        let parent = read_index(row, 0, parent_size)?;
        let index = parent >> 3;
        match parent & 7 {
            1 => self.type_ref(index),
            0 | 4 => self.local_member_owner(token).and_then(|index| {
                let row = self.type_row(index)?;
                let name = string_at(self.strings, read_index(row, 4, self.string_size)?)?;
                let namespace = string_at(
                    self.strings,
                    read_index(row, 4 + self.string_size, self.string_size)?,
                )?;
                Some((namespace, name))
            }),
            _ => None,
        }
    }

    fn local_member_owner(&self, token: u32) -> Option<usize> {
        let (row, width) = self.member_row(token)?;
        let parent = read_index(row, 0, width)?;
        let index = parent >> 3;
        match parent & 7 {
            0 => index.checked_sub(1),
            4 if (1..=self.rows[27]).contains(&index) => {
                let offset = self
                    .table_after_members(27)?
                    .checked_add((index - 1).checked_mul(self.blob_size)?)?;
                let blob = read_index(self.tables, offset, self.blob_size)?;
                let (length, signature) = compressed(self.blobs.get(blob..)?)?;
                // GENERICINST followed by CLASS or VALUETYPE and a TypeDefOrRef.
                let [0x15, 0x11 | 0x12, ty @ ..] = signature.get(..length)? else {
                    return None;
                };
                let (owner, _) = compressed(ty)?;
                if owner & 3 != 0 {
                    return None;
                }
                (owner >> 2).checked_sub(1)
            }
            _ => None,
        }
    }

    pub(crate) fn local_field(&self, token: u32) -> Option<u32> {
        let owner = self.local_member_owner(token)?;
        let name = self.member_name(token)?;
        let signature = self.method_signature_blob(token)?;
        if signature.first() != Some(&6) {
            return None;
        }
        let size = 2 + self.string_size + self.blob_size;
        let end = self
            .type_field_list(owner + 1)?
            .min(self.rows[4].checked_add(1)?);
        for field in self.type_field_list(owner)?..end {
            let row = range(
                self.tables,
                self.fields
                    .checked_add(field.checked_sub(1)?.checked_mul(size)?)?,
                size,
            )?;
            if string_at(self.strings, read_index(row, 2, self.string_size)?)? != name {
                continue;
            }
            let blob = read_index(row, 2 + self.string_size, self.blob_size)?;
            let (length, bytes) = compressed(self.blobs.get(blob..)?)?;
            if bytes.get(..length)? == signature {
                return Some(0x04000000 | field as u32);
            }
        }
        None
    }

    pub(crate) fn generic_type(&self, token: u32, args: &[u8], method: bool) -> Option<u32> {
        let index = (token & 0xffffff) as usize;
        if token >> 24 != 27 || !(1..=self.rows[27]).contains(&index) {
            return None;
        }
        let offset = self
            .table_after_members(27)?
            .checked_add((index - 1).checked_mul(self.blob_size)?)?;
        let blob = read_index(self.tables, offset, self.blob_size)?;
        let (length, signature) = compressed(self.blobs.get(blob..)?)?;
        let [kind, 0] = signature.get(..length)? else {
            return None;
        };
        if *kind != if method { 0x1e } else { 0x13 } {
            return None;
        }
        let [0x11 | 0x12, args @ ..] = args else {
            return None;
        };
        let (ty, _) = compressed(args)?;
        let table = match ty & 3 {
            0 => 2,
            1 => 1,
            2 => 27,
            _ => return None,
        };
        Some(table << 24 | (ty >> 2) as u32)
    }

    pub(crate) fn generic_context(&self, token: u32) -> Option<(bool, &[u8])> {
        if let Some((_, [0x0a, 1, args @ ..])) = self.method_spec(token) {
            return Some((true, args));
        }
        let (row, width) = self.member_row(token)?;
        let parent = read_index(row, 0, width)?;
        let index = parent >> 3;
        if parent & 7 != 4 || !(1..=self.rows[27]).contains(&index) {
            return None;
        }
        let offset = self
            .table_after_members(27)?
            .checked_add((index - 1).checked_mul(self.blob_size)?)?;
        let blob = read_index(self.tables, offset, self.blob_size)?;
        let (length, signature) = compressed(self.blobs.get(blob..)?)?;
        let [0x15, 0x11 | 0x12, rest @ ..] = signature.get(..length)? else {
            return None;
        };
        let (_, rest) = compressed(rest)?;
        let [1, args @ ..] = rest else {
            return None;
        };
        Some((false, args))
    }

    pub(crate) fn local_method(&self, token: u32) -> Option<u32> {
        if let Some((target, _)) = self.method_spec(token) {
            return if target >> 24 == 6 {
                Some(target)
            } else {
                self.local_method(target)
            };
        }
        let index = self.local_member_owner(token)?;
        let name = self.member_name(token)?;
        let signature = self.method_signature_blob(token)?;
        let start = self.type_method_list(index)?.max(1);
        let end = self
            .type_method_list(index + 1)?
            .min(self.rows[6].checked_add(1)?);
        (start..end)
            .map(|row| 0x06000000 | row as u32)
            .find(|&candidate| {
                self.method(candidate)
                    .is_some_and(|method| method.method_name == name)
                    && self.method_signature_blob(candidate) == Some(signature)
            })
    }

    pub(crate) fn type_ref(&self, index: usize) -> Option<(&'a str, &'a str)> {
        if index == 0 || index > self.rows[1] {
            return None;
        }
        let scope = coded_width(&self.rows, 2, &[0, 26, 35, 1]);
        let size = scope + 2 * self.string_size;
        let row = range(
            self.tables,
            self.type_refs.checked_add((index - 1).checked_mul(size)?)?,
            size,
        )?;
        let name = string_at(self.strings, read_index(row, scope, self.string_size)?)?;
        let namespace = string_at(
            self.strings,
            read_index(row, scope + self.string_size, self.string_size)?,
        )?;
        Some((namespace, name))
    }

    pub(crate) fn vector_axis(&self, token: u32) -> Option<usize> {
        if self.member_type(token)? != ("OpenGLES", "Vector3f") {
            return None;
        }
        match self.member_name(token)? {
            "X" | "x" => Some(0),
            "Y" | "y" => Some(1),
            "Z" | "z" => Some(2),
            _ => None,
        }
    }

    pub(crate) fn member_name(&self, token: u32) -> Option<&'a str> {
        let (row, parent_size) = self.member_row(token)?;
        string_at(
            self.strings,
            read_index(row, parent_size, self.string_size)?,
        )
    }
}

/// Resolve the type and name of a member reference for diagnostics.
pub fn referenced_member(bytes: &[u8], token: u32) -> Option<(&str, &str, &str)> {
    let assembly = Assembly::read(bytes)?;
    let (namespace, kind) = assembly.member_type(token)?;
    Some((namespace, kind, assembly.member_name(token)?))
}

/// Detect references to the managed sensor methods supported by PocketHLE.
pub fn references_accelerometer(bytes: &[u8]) -> bool {
    let Some(assembly) = Assembly::read(bytes) else {
        return false;
    };
    let width = coded_width(&assembly.rows, 3, &[2, 1, 26, 6, 27])
        + assembly.string_size
        + assembly.blob_size;
    let count =
        assembly.rows[10].min(assembly.tables.len().saturating_sub(assembly.members) / width);
    (1..=count.min(0xffffff)).any(|row| {
        let token = 0x0a000000 | row as u32;
        matches!(
            (assembly.member_type(token), assembly.member_name(token)),
            (Some(("Sensors", "GSensorFactory")), Some("CreateGSensor"))
                | (Some(("CompassHTC", "HTCCompass")), Some("get_Angle"))
                | (
                    Some(("Sensors", "IGSensor")),
                    Some("GetGVector" | "get_CurrentValue")
                )
        )
    })
}
