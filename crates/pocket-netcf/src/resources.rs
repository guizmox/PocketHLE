//! Read named bitmap resources without deserializing guest objects.

pub fn image(bytes: &[u8], key: &str) -> Option<image::RgbaImage> {
    use crate::metadata::dword;
    if dword(bytes, 0)? != 0xbeef_cace {
        return None;
    }
    let mut pos = 12usize.checked_add(dword(bytes, 8)? as usize)?;
    if dword(bytes, pos)? != 2 {
        return None;
    }
    let count = dword(bytes, pos + 4)? as usize;
    let types = dword(bytes, pos + 8)? as usize;
    pos += 12;
    fn string<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
        let mut size = 0usize;
        for shift in (0..35).step_by(7) {
            let byte = *bytes.get(*pos)?;
            *pos += 1;
            size |= usize::from(byte & 127).checked_shl(shift)?;
            if byte & 128 == 0 {
                let end = pos.checked_add(size)?;
                let text = bytes.get(*pos..end)?;
                *pos = end;
                return Some(text);
            }
        }
        None
    }
    for _ in 0..types {
        string(bytes, &mut pos)?;
    }
    pos = (pos + 7) & !7;
    let offsets = pos.checked_add(count.checked_mul(4)?)?;
    let data_ptr = offsets.checked_add(count.checked_mul(4)?)?;
    let data_start = dword(bytes, data_ptr)? as usize;
    let names = data_ptr + 4;
    let mut data = Vec::new();
    for i in 0..count {
        let mut pos = names.checked_add(dword(bytes, offsets + i * 4)? as usize)?;
        let name = string(bytes, &mut pos)?;
        let offset = data_start.checked_add(dword(bytes, pos)? as usize)?;
        let matches = name
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .eq(key.encode_utf16());
        data.push((offset, matches));
    }
    data.sort_unstable();
    let index = data.iter().position(|(_, found)| *found)?;
    let resource =
        bytes.get(data[index].0..data.get(index + 1).map_or(bytes.len(), |entry| entry.0))?;
    let png = b"\x89PNG\r\n\x1a\n";
    let start = resource
        .windows(png.len())
        .position(|window| window == png)?;
    let mut reader = image::io::Reader::with_format(
        std::io::Cursor::new(&resource[start..]),
        image::ImageFormat::Png,
    );
    let mut limits = image::io::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    Some(reader.decode().ok()?.to_rgba8())
}
