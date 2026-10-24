//! Narrow installation compatibility, never a global checksum bypass.
//! A few old uncompressed Pocket PC packages (Snails) have stale CFDATA
//! checksum. Recompute that metadata in an owned copy, retaining every payload
//! byte. Structural/decompression validation still happens in the CAB reader.
fn word(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}
fn dword(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
fn block_checksum(bytes: &[u8]) -> u32 {
    let n = bytes.len() / 4 * 4;
    let full = bytes[..n].chunks_exact(4).fold(0u32, |sum, c| {
        sum ^ u32::from_le_bytes(c.try_into().unwrap())
    });
    full ^ bytes[n..]
        .iter()
        .fold(0u32, |v, b| (v << 8) | u32::from(*b))
}

pub(super) fn recover(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.get(..4)? != b"MSCF"
        || dword(bytes, 8)? as usize != bytes.len()
        || word(bytes, 30)? != 0
    {
        return None;
    }
    let folders = usize::from(word(bytes, 26)?);
    if folders == 0 || folders > 1024 {
        return None;
    }
    let table_end = 36usize.checked_add(folders.checked_mul(8)?)?;
    let mut previous_end = table_end;
    let mut corrections = Vec::new();
    for folder in 0..folders {
        let table = 36 + folder * 8;
        let mut at = dword(bytes, table)? as usize;
        let blocks = word(bytes, table + 4)?;
        if word(bytes, table + 6)? != 0 || at < previous_end {
            return None;
        }
        for _ in 0..blocks {
            let stored = dword(bytes, at)?;
            let size = usize::from(word(bytes, at + 4)?);
            let unpacked = usize::from(word(bytes, at + 6)?);
            if size == 0 || size != unpacked || size > 32768 {
                return None;
            }
            let end = at.checked_add(8)?.checked_add(size)?;
            let actual = block_checksum(bytes.get(at + 4..end)?);
            if stored != 0 && stored != actual {
                if corrections.len() >= 64 {
                    return None;
                }
                corrections.push((at, actual));
            }
            at = end;
        }
        previous_end = at;
    }
    if corrections.is_empty() {
        return None;
    }
    let mut owned = bytes.to_vec();
    for (at, actual) in corrections {
        owned[at..at + 4].copy_from_slice(&actual.to_le_bytes());
    }
    Some(owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Read, Write};
    fn fixture() -> Vec<u8> {
        let mut builder = cab::CabinetBuilder::new();
        builder
            .add_folder(cab::CompressionType::None)
            .add_file("test.bin");
        let mut writer = builder.build(io::Cursor::new(Vec::new())).unwrap();
        {
            let mut file = writer.next_file().unwrap().unwrap();
            file.write_all(&vec![0x51; 40000]).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn one_stale_checksum_is_recovered_without_payload_changes() {
        let original = fixture();
        let at = dword(&original, 36).unwrap() as usize;
        let mut stale = original.clone();
        stale[at] ^= 0x80;
        let mut cabinet = cab::Cabinet::new(io::Cursor::new(&stale)).unwrap();
        let rejected = match cabinet.read_file("test.bin") {
            Err(_) => true,
            Ok(mut reader) => reader.read_to_end(&mut Vec::new()).is_err(),
        };
        assert!(rejected);
        let recovered = recover(&stale).unwrap();
        assert_eq!(recovered, original);
        let mut data = Vec::new();
        cab::Cabinet::new(io::Cursor::new(recovered))
            .unwrap()
            .read_file("test.bin")
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        assert_eq!(data, vec![0x51; 40000]);
        assert!(recover(&original).is_none());
    }
    #[test]
    fn multiple_stale_checksums_preserve_payload_and_structural_errors_stay_errors() {
        let mut bytes = fixture();
        let at = dword(&bytes, 36).unwrap() as usize;
        let second = at + 8 + usize::from(word(&bytes, at + 4).unwrap());
        bytes[at] ^= 1;
        bytes[second] ^= 1;
        assert_eq!(recover(&bytes).unwrap(), fixture());
        bytes[second] ^= 1;
        bytes.pop();
        assert!(recover(&bytes).is_none());
        let mut bytes = fixture();
        bytes[at] ^= 1;
        bytes[42] = 1;
        assert!(recover(&bytes).is_none());
        bytes[42] = 0;
        bytes[30] = 1;
        assert!(recover(&bytes).is_none());
    }
    #[test]
    fn recovery_is_bounded_to_64_metadata_words() {
        let mut builder = cab::CabinetBuilder::new();
        builder
            .add_folder(cab::CompressionType::None)
            .add_file("data.bin");
        let mut writer = builder.build(io::Cursor::new(Vec::new())).unwrap();
        writer
            .next_file()
            .unwrap()
            .unwrap()
            .write_all(&vec![0x51; 65 * 32768])
            .unwrap();
        let original = writer.finish().unwrap().into_inner();
        let mut stale = original.clone();
        let mut at = dword(&stale, 36).unwrap() as usize;
        for i in 0..65 {
            stale[at] ^= 1;
            if i == 63 {
                assert_eq!(recover(&stale).unwrap(), original);
            }
            at += 8 + usize::from(word(&stale, at + 4).unwrap());
        }
        assert!(recover(&stale).is_none());
    }
}
