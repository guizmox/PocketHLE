//! File and XML services for the subset of Compact Framework serializers used
//! by simple settings classes. Guest paths are confined to a game's save folder.

use crate::metadata::Assembly;
use crate::vm::{Fault, Machine, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const XML_LIMIT: u64 = 64 * 1024;

#[derive(Default)]
pub struct Storage {
    pub root: Option<PathBuf>,
    readers: BTreeMap<usize, String>,
    writers: BTreeMap<usize, PathBuf>,
    serializers: BTreeMap<usize, u32>,
}

fn filename(path: &str) -> Option<&str> {
    let name = path.rsplit(['\\', '/']).next()?;
    (!name.is_empty()
        && !name.starts_with('.')
        && name.len() <= 120
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b' ')))
    .then_some(name)
}

impl Storage {
    pub fn sweep(&mut self, live: &[bool]) {
        self.readers.retain(|&id, _| live[id]);
        self.writers.retain(|&id, _| live[id]);
        self.serializers.retain(|&id, _| live[id]);
    }

    pub fn open_reader(&mut self, id: usize, path: &str) -> Result<(), Fault> {
        let root = self.root.as_ref().ok_or(Fault::MissingFile)?;
        let path = root.join(filename(path).ok_or(Fault::ManagedException)?);
        if !pocket_cab::plain_path_below(root, &path, false) {
            return Err(Fault::MissingFile);
        }
        let file = std::fs::File::open(path).map_err(|_| Fault::MissingFile)?;
        let mut text = String::new();
        file.take(XML_LIMIT + 1)
            .read_to_string(&mut text)
            .map_err(|_| Fault::ManagedException)?;
        if text.len() as u64 > XML_LIMIT {
            return Err(Fault::ManagedException);
        }
        self.readers.insert(id, text);
        Ok(())
    }

    pub fn open_writer(&mut self, id: usize, path: &str) -> Result<(), Fault> {
        let root = self.root.as_ref().ok_or(Fault::ManagedException)?;
        let path = root.join(filename(path).ok_or(Fault::ManagedException)?);
        std::fs::create_dir_all(root).map_err(|_| Fault::ManagedException)?;
        if path.exists() && !pocket_cab::plain_path_below(root, &path, false) {
            return Err(Fault::ManagedException);
        }
        self.writers.insert(id, path);
        Ok(())
    }

    pub fn close(&mut self, id: usize) {
        self.readers.remove(&id);
        self.writers.remove(&id);
    }

    pub fn serializer(&mut self, id: usize, ty: u32) {
        self.serializers.insert(id, ty);
    }

    pub fn serialize(
        &self,
        assembly: &Assembly<'_>,
        machine: &mut Machine,
        serializer: usize,
        writer: usize,
        object: Value,
    ) -> Result<(), Fault> {
        let ty = *self.serializers.get(&serializer).ok_or(Fault::Reference)?;
        let path = self.writers.get(&writer).ok_or(Fault::Reference)?;
        let (name, fields) = assembly
            .primitive_fields(ty)
            .ok_or(Fault::InvalidSignature)?;
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        let name = xml_name(name)?;
        out.push_str(&format!("<{name}>\n"));
        for (token, name, kind) in fields {
            let name = xml_name(name)?;
            let value = machine
                .field(object, token)?
                .unwrap_or_else(|| assembly.field_default(token));
            let value = match (kind, value) {
                (2, Value::Int(value)) => (value != 0).to_string(),
                (8, Value::Int(value)) => value.to_string(),
                (12, Value::Float(value)) => f32::from_bits(value).to_string(),
                _ => return Err(Fault::InvalidSignature),
            };
            out.push_str(&format!("  <{name}>{value}</{name}>\n"));
        }
        out.push_str(&format!("</{name}>\n"));
        if out.len() as u64 > XML_LIMIT {
            return Err(Fault::ManagedException);
        }
        write_atomic(path, out.as_bytes()).map_err(|_| Fault::ManagedException)
    }

    pub fn deserialize_plan(
        &self,
        assembly: &Assembly<'_>,
        serializer: usize,
        reader: usize,
    ) -> Result<(u32, Vec<(u32, Value)>), Fault> {
        let ty = *self.serializers.get(&serializer).ok_or(Fault::Reference)?;
        let text = self.readers.get(&reader).ok_or(Fault::Reference)?;
        let (name, fields) = assembly
            .primitive_fields(ty)
            .ok_or(Fault::InvalidSignature)?;
        let values = parse_xml(text, name)?;
        let mut result = Vec::new();
        for (token, name, kind) in fields {
            if let Some(value) = values.get(name) {
                let value = match kind {
                    2 => match value.as_str() {
                        "true" | "1" => Value::Int(1),
                        "false" | "0" => Value::Int(0),
                        _ => return Err(Fault::ManagedException),
                    },
                    8 => Value::Int(value.parse().map_err(|_| Fault::ManagedException)?),
                    12 => Value::Float(
                        value
                            .parse::<f32>()
                            .map_err(|_| Fault::ManagedException)?
                            .to_bits(),
                    ),
                    _ => return Err(Fault::InvalidSignature),
                };
                result.push((token, value));
            }
        }
        Ok((
            assembly.constructor(ty).ok_or(Fault::InvalidSignature)?,
            result,
        ))
    }
}

fn xml_name(name: &str) -> Result<&str, Fault> {
    let mut bytes = name.bytes();
    if bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        Ok(name)
    } else {
        Err(Fault::InvalidSignature)
    }
}

fn parse_xml(text: &str, expected: &str) -> Result<BTreeMap<String, String>, Fault> {
    use quick_xml::events::Event;
    let mut xml = quick_xml::Reader::from_str(text);
    xml.config_mut().trim_text(true);
    let mut values = BTreeMap::new();
    let mut depth = 0;
    let mut field = None;
    let mut closed = false;
    loop {
        match xml.read_event().map_err(|_| Fault::ManagedException)? {
            Event::Start(tag)
                if depth == 0 && !closed && tag.local_name().as_ref() == expected.as_bytes() =>
            {
                depth = 1
            }
            Event::Start(tag) if depth == 1 => {
                field = Some(
                    String::from_utf8(tag.local_name().as_ref().to_vec())
                        .map_err(|_| Fault::ManagedException)?,
                );
                depth = 2;
            }
            Event::Text(text) if depth == 2 => {
                let value = text.decode().map_err(|_| Fault::ManagedException)?;
                values.insert(
                    field.clone().ok_or(Fault::ManagedException)?,
                    value.into_owned(),
                );
            }
            Event::End(_) if depth == 2 => {
                depth = 1;
                field = None;
            }
            Event::End(_) if depth == 1 => {
                depth = 0;
                closed = true;
            }
            Event::Decl(_) | Event::Comment(_) => {}
            Event::Eof if closed && depth == 0 => return Ok(values),
            _ => return Err(Fault::ManagedException),
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let directory = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    let mut temp = tempfile::NamedTempFile::new_in(directory)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}
