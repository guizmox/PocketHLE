//! RAR/SFX containers use the existing pure-Rust decoder, never the host stub.
use super::{charge, Result};
use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
};

struct Buffer {
    bytes: Rc<RefCell<Vec<u8>>>,
    limit: usize,
}
impl Write for Buffer {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut bytes = self.bytes.borrow_mut();
        if input.len() > self.limit.saturating_sub(bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "RAR entry exceeds declared size",
            ));
        }
        bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn extract(b: &[u8], budget: &mut usize) -> Result<Option<Vec<(String, Vec<u8>)>>> {
    if rars::find_archive_start(b, rars::SFX_SCAN_LIMIT).is_none() {
        return Ok(None);
    }
    let options = rars::ArchiveReadOptions::new().with_rar50_buffered_decode_limit(*budget as u64);
    let archive = rars::ArchiveReader::read_with_options(b, options).map_err(|e| e.to_string())?;
    let members: Vec<_> = archive.members().take(4097).collect();
    if members.len() > 4096 {
        return Err("Too many RAR installer entries".into());
    }
    // Preflight every entry, including files not needed by the installer.
    // Solid archives may need to decode them to reach the cabinets.
    let mut total = 0usize;
    for member in &members {
        let m = &member.meta;
        if m.is_encrypted || m.is_split_before || m.is_split_after {
            return Err("Encrypted or split RAR installer is not supported".into());
        }
        let size =
            usize::try_from(m.unpacked_size).map_err(|_| "RAR size exceeds address space")?;
        total = total
            .checked_add(size)
            .ok_or("RAR expanded size overflow")?;
    }
    charge(total, budget)?;
    let buffers: Vec<_> = members
        .iter()
        .map(|m| {
            Rc::new(RefCell::new(Vec::with_capacity(
                m.meta.unpacked_size as usize,
            )))
        })
        .collect();
    let mut index = 0;
    archive
        .extract_to_with_options(options, |meta| {
            let member = members
                .get(index)
                .ok_or(rars::Error::InvalidHeader("Unexpected RAR entry"))?;
            if member.meta.name_bytes() != meta.name_bytes() {
                return Err(rars::Error::InvalidHeader("RAR entry order mismatch"));
            }
            let writer = Buffer {
                bytes: buffers[index].clone(),
                limit: member.meta.unpacked_size as usize,
            };
            index += 1;
            Ok(Box::new(writer))
        })
        .map_err(|e| e.to_string())?;
    if index != members.len() {
        return Err("Missing RAR installer entry".into());
    }
    let mut entries = Vec::new();
    for (member, buffer) in members.into_iter().zip(buffers) {
        let bytes = std::mem::take(&mut *buffer.borrow_mut());
        if !member.meta.is_directory {
            if bytes.len() as u64 != member.meta.unpacked_size {
                return Err("RAR expanded size mismatch".into());
            }
            entries.push((member.meta.name_lossy().replace('\\', "/"), bytes));
        }
    }
    Ok(Some(entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let entries = [
            rars::rar15_40::StoredEntry {
                name: b"folder/first.cab",
                data: b"first",
                file_time: 0,
                file_attr: 0x20,
                host_os: 3,
                password: None,
                file_comment: None,
            },
            rars::rar15_40::StoredEntry {
                name: b"folder/second.hdr",
                data: b"second",
                file_time: 0,
                file_attr: 0x20,
                host_os: 3,
                password: None,
                file_comment: None,
            },
        ];
        rars::rar15_40::write_stored_archive(&entries, Default::default()).unwrap()
    }
    #[test]
    fn standalone_and_sfx_entries_keep_order_and_names() {
        let original = fixture();
        for prefix in [
            b"".as_slice(),
            b"MZ synthetic self-extractor stub".as_slice(),
        ] {
            let bytes = [prefix, original.as_slice()].concat();
            let mut budget = 100;
            let entries = extract(&bytes, &mut budget).unwrap().unwrap();
            assert_eq!(
                entries,
                vec![
                    ("folder/first.cab".into(), b"first".to_vec()),
                    ("folder/second.hdr".into(), b"second".to_vec())
                ]
            );
            assert_eq!(budget, 89);
        }
    }
    #[test]
    fn limits_truncation_and_corrupt_crc_are_rejected() {
        let mut bytes = fixture();
        assert!(extract(&bytes, &mut 10).is_err());
        let pos = bytes.windows(5).position(|w| w == b"first").unwrap();
        bytes[pos] ^= 1;
        assert!(extract(&bytes, &mut 100).is_err());
        assert!(extract(b"Rar!\x1a\x07\x00", &mut 100).is_err());
        assert!(extract(b"ordinary data", &mut 100).unwrap().is_none());
    }
}
