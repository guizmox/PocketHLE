//! Shared 512-byte block reconstruction used by desktop CAB packagers.
//! Logical filenames select memory buffers only; no installer path is written.
use super::*;
pub(super) fn extract(
    entries: &[(String, Vec<u8>)],
    budget: &mut usize,
) -> Result<Option<Vec<Cabinet>>> {
    let mut result = Vec::new();
    let mut recognized = false;
    for (name, files) in entries.iter().filter(|(n, _)| {
        n.replace('\\', "/")
            .rsplit('/')
            .next()
            .is_some_and(|n| n.eq_ignore_ascii_case("files.dat"))
    }) {
        let normalized = name.replace('\\', "/");
        let prefix = normalized
            .rsplit_once('/')
            .map(|(p, _)| format!("{p}/"))
            .unwrap_or_default();
        let lookup = |name: &str| {
            entries
                .iter()
                .find(|(n, _)| n.replace('\\', "/").eq_ignore_ascii_case(name))
                .map(|(_, v)| v)
        };
        let Some(parts) = lookup(&format!("{prefix}parts.dat")) else {
            continue;
        };
        let count = files.first().copied().unwrap_or(0) as usize;
        if count == 0 || count > 32 || files.len() != 1 + count * 64 {
            continue;
        }
        let mut outputs = Vec::new();
        for record in files[1..].chunks_exact(64) {
            let end = record
                .iter()
                .position(|b| *b == 0)
                .ok_or("Shared-parts filename is not terminated")?;
            let filename =
                std::str::from_utf8(&record[..end]).map_err(|_| "Invalid shared-parts filename")?;
            let data =
                lookup(&format!("{prefix}{filename}")).ok_or("Missing shared-parts target")?;
            charge(data.len(), budget)?;
            outputs.push(Cabinet {
                name: filename.to_owned(),
                bytes: data.clone(),
            });
        }
        recognized = true;
        let mut pos = 0;
        while pos < parts.len() {
            let block = parts
                .get(pos..pos.checked_add(512).ok_or("Shared-parts overflow")?)
                .ok_or("Truncated shared-parts block")?;
            pos += 512;
            let count = *parts
                .get(pos)
                .ok_or("Missing shared-parts reference count")? as usize;
            pos += 1;
            for _ in 0..count {
                let reference = u32_at(parts, pos).ok_or("Truncated shared-parts reference")?;
                pos += 4;
                let index = reference & 255;
                let offset = reference >> 8;
                let output = outputs
                    .get_mut(index)
                    .ok_or("Invalid shared-parts target index")?;
                if offset >= output.bytes.len() {
                    return Err("Shared-parts offset exceeds target".into());
                }
                let size = 512.min(output.bytes.len() - offset);
                output.bytes[offset..offset + size].copy_from_slice(&block[..size]);
            }
        }
        for output in outputs {
            if output.bytes.starts_with(b"MSCF") {
                result.push(output);
            }
        }
    }
    Ok(recognized.then_some(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_blocks_rebuild_two_targets_and_reject_bad_indices() {
        let mut files = vec![0u8; 129];
        files[0] = 2;
        files[1..7].copy_from_slice(b"a.cab\0");
        files[65..71].copy_from_slice(b"b.cab\0");
        let mut block = vec![0u8; 512];
        block[..4].copy_from_slice(b"MSCF");
        block[8..12].copy_from_slice(&512u32.to_le_bytes());
        let mut parts = block.clone();
        parts.push(2);
        parts.extend_from_slice(&0u32.to_le_bytes());
        parts.extend_from_slice(&1u32.to_le_bytes());
        let mut entries = vec![
            ("files.dat".into(), files),
            ("parts.dat".into(), parts),
            ("a.cab".into(), vec![0; 512]),
            ("b.cab".into(), vec![0; 512]),
        ];
        let output = extract(&entries, &mut LIMIT.clone()).unwrap().unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].bytes, block);
        assert_eq!(output[1].bytes, block);
        entries[1].1[513..517].copy_from_slice(&2u32.to_le_bytes());
        assert!(extract(&entries, &mut LIMIT.clone()).is_err());
    }
}
