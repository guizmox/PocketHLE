//! CE6 typed CEDB operations required by the original Explorer recycle bin.
//! Volume files live in the guest VFS; ROM files and host registries stay untouched.
use super::*;
use pocket_kernel::cedb::generic::{self as model, Database, Guid, Properties, Sort, Value};
use std::cmp::Ordering;

#[cfg(test)]
mod tests;

pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("CeMountDBVol", mount as Handler),
        ("CeUnmountDBVol", unmount),
        ("CeFlushDBVol", flush),
        ("CeCreateDatabaseEx2", create),
        ("CeDeleteDatabaseEx", delete_database),
        ("CeReadRecordPropsEx", read_ex),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn guid(ctx: &mut CallCtx<'_>, ptr: u32) -> Option<Guid> {
    if !access(ctx, ptr, 16, Prot::READ) {
        return None;
    }
    let value: Guid = ctx.cpu.read_mem(ptr, 16).ok()?.try_into().ok()?;
    (value != [255; 16]).then_some(value)
}
pub(super) fn should_open(ctx: &mut CallCtx<'_>) -> Result<bool, KernelError> {
    let ptr = ctx.arg_u32(0)?;
    let oid = ctx.arg_u32(1)?;
    if guid(ctx, ptr) != Some([0; 16]) || !access(ctx, oid, 4, Prot::READ) {
        return Ok(true);
    }
    let oid = ctx.cpu.read_u32_le(oid)?;
    if oid != 0 {
        return Ok(oid != DATABASE_OID);
    }
    let name = ctx.arg_u32(2)?;
    Ok(!crate::ce_shortcut::text(ctx, name, 32)
        .is_ok_and(|name| name.eq_ignore_ascii_case("Contacts Database")))
}
pub(super) fn has_handle(ctx: &CallCtx<'_>, handle: u32) -> bool {
    ctx.kernel.cedb.generic.cursors.contains_key(&handle)
}
fn snapshot(ctx: &mut CallCtx<'_>, volume: Guid) -> Result<model::Volume, u32> {
    ctx.kernel
        .cedb
        .generic
        .snapshot(&mut ctx.kernel.vfs, volume)
}
fn sort(ctx: &mut CallCtx<'_>, ptr: u32) -> Option<Sort> {
    if !access(ctx, ptr, 32, Prot::READ) {
        return None;
    }
    let bytes = ctx.cpu.read_mem(ptr, 32).ok()?;
    let count = usize::from(word(&bytes, 2));
    let key_flags = word(&bytes, 4);
    if word(&bytes, 0) != 1 || count == 0 || count > 3 || key_flags & !0x200 != 0 {
        return None;
    }
    let mut properties = Vec::new();
    for i in 0..count {
        let id = dword(&bytes, 8 + i * 4);
        let flags = dword(&bytes, 20 + i * 4);
        if !matches!(id & 0xffff, 2 | 18 | 3 | 19 | 64 | 31 | 65 | 11 | 5) || flags & !0x607 != 0 {
            return None;
        }
        properties.push((id, flags));
    }
    Some(Sort {
        key_flags,
        properties,
    })
}
fn info(ctx: &mut CallCtx<'_>, ptr: u32) -> Option<Database> {
    if !access(ctx, ptr, 220, Prot::READ) {
        return None;
    }
    let bytes = ctx.cpu.read_mem(ptr, 220).ok()?;
    if word(&bytes, 0) != 1 || word(&bytes, 2) > 4 {
        return None;
    }
    let flags = dword(&bytes, 4);
    if flags & !(0x1f | 0x30000) != 0 {
        return None;
    }
    let units: Vec<u16> = bytes[8..72]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .take_while(|c| *c != 0)
        .collect();
    let name = String::from_utf16_lossy(&units);
    let mut sorts = Vec::new();
    if flags & 4 != 0 {
        for i in 0..u32::from(word(&bytes, 2)) {
            sorts.push(sort(ctx, ptr + 92 + i * 32)?);
        }
    }
    Some(Database {
        name,
        kind: dword(&bytes, 72),
        flags: flags & 0x30000,
        sorts,
        modified: u64::from_le_bytes(bytes[84..92].try_into().unwrap()),
        records: Default::default(),
    })
}
fn encode_info(db: &Database) -> Vec<u8> {
    let mut bytes = vec![0; 220];
    bytes[..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&(db.sorts.len() as u16).to_le_bytes());
    bytes[4..8].copy_from_slice(&(db.flags | 0x1f).to_le_bytes());
    for (i, unit) in db.name.encode_utf16().take(31).enumerate() {
        bytes[8 + i * 2..10 + i * 2].copy_from_slice(&unit.to_le_bytes());
    }
    bytes[72..76].copy_from_slice(&db.kind.to_le_bytes());
    bytes[76..80].copy_from_slice(&(db.records.len() as u32).to_le_bytes());
    let size = serde_json::to_vec(&db.records).map_or(0, |b| b.len() as u32);
    bytes[80..84].copy_from_slice(&size.to_le_bytes());
    bytes[84..92].copy_from_slice(&db.modified.to_le_bytes());
    for (i, s) in db.sorts.iter().enumerate() {
        let start = 92 + i * 32;
        bytes[start..start + 2].copy_from_slice(&1u16.to_le_bytes());
        bytes[start + 2..start + 4].copy_from_slice(&(s.properties.len() as u16).to_le_bytes());
        bytes[start + 4..start + 6].copy_from_slice(&s.key_flags.to_le_bytes());
        for (j, (id, flags)) in s.properties.iter().enumerate() {
            bytes[start + 8 + j * 4..start + 12 + j * 4].copy_from_slice(&id.to_le_bytes());
            bytes[start + 20 + j * 4..start + 24 + j * 4].copy_from_slice(&flags.to_le_bytes());
        }
    }
    bytes
}
fn mount(ctx: &mut CallCtx<'_>) -> Outcome {
    let out = ctx.arg_u32(0)?;
    let name = ctx.arg_u32(1)?;
    let flags = ctx.arg_u32(2)?;
    if !access(ctx, out, 16, Prot::WRITE) {
        return fail(ctx, 87, 0);
    }
    let path = match crate::ce_shortcut::text(ctx, name, 260) {
        Ok(path) => path,
        Err(error) => return fail(ctx, error, 0),
    };
    if !path.starts_with(['\\', '/'])
        || path.contains(['\0', ':'])
        || path.split(['\\', '/']).any(|p| p == "." || p == "..")
    {
        return fail(ctx, 87, 0);
    }
    match ctx
        .kernel
        .cedb
        .generic
        .mount(&mut ctx.kernel.vfs, &path, flags)
    {
        Ok(value) => {
            ctx.cpu.write_mem(out, &value)?;
            ret(1)
        }
        Err(error) => fail(ctx, error, 0),
    }
}
fn unmount(ctx: &mut CallCtx<'_>) -> Outcome {
    let ptr = ctx.arg_u32(0)?;
    let Some(value) = guid(ctx, ptr) else {
        return fail(ctx, 87, 0);
    };
    match ctx.kernel.cedb.generic.unmount(value) {
        Ok(()) => ret(1),
        Err(error) => fail(ctx, error, 0),
    }
}
fn flush(ctx: &mut CallCtx<'_>) -> Outcome {
    let ptr = ctx.arg_u32(0)?;
    let Some(value) = guid(ctx, ptr) else {
        return fail(ctx, 87, 0);
    };
    match ctx.kernel.cedb.generic.flush(&mut ctx.kernel.vfs, value) {
        Ok(()) => ret(1),
        Err(error) => fail(ctx, error, 0),
    }
}
fn create(ctx: &mut CallCtx<'_>) -> Outcome {
    let ptr = ctx.arg_u32(0)?;
    let info_ptr = ctx.arg_u32(1)?;
    let Some(volume) = guid(ctx, ptr) else {
        return fail(ctx, 87, 0);
    };
    let Some(mut database) = info(ctx, info_ptr) else {
        return fail(ctx, 87, 0);
    };
    if database.name.is_empty() || database.name.encode_utf16().count() >= 32 {
        return fail(ctx, 87, 0);
    }
    database.modified = model::modified_now();
    match ctx
        .kernel
        .cedb
        .generic
        .update(&mut ctx.kernel.vfs, volume, |v| {
            if v.databases
                .values()
                .any(|db| db.name.eq_ignore_ascii_case(&database.name))
            {
                return Err(183);
            }
            if v.databases.len() >= 4096 {
                return Err(8);
            }
            let oid = v.alloc_oid().ok_or(8u32)?;
            v.databases.insert(oid, database);
            Ok(oid)
        }) {
        Ok(oid) => ret(oid),
        Err(error) => fail(ctx, error, 0),
    }
}
pub(super) fn open(ctx: &mut CallCtx<'_>) -> Outcome {
    let ptr = ctx.arg_u32(0)?;
    let oid_ptr = ctx.arg_u32(1)?;
    let name_ptr = ctx.arg_u32(2)?;
    let sort_ptr = ctx.arg_u32(3)?;
    let flags = ctx.arg_u32(4)?;
    let notify = ctx.arg_u32(5)?;
    let Some(volume) = guid(ctx, ptr) else {
        return fail(ctx, 87, INVALID_HANDLE);
    };
    if !access(ctx, oid_ptr, 4, Prot::READ | Prot::WRITE) || flags & !1 != 0 || notify != 0 {
        return fail(ctx, 87, INVALID_HANDLE);
    }
    let oid = ctx.cpu.read_u32_le(oid_ptr)?;
    let name = if oid == 0 {
        match crate::ce_shortcut::text(ctx, name_ptr, 32) {
            Ok(name) => name,
            Err(error) => return fail(ctx, error, INVALID_HANDLE),
        }
    } else {
        String::new()
    };
    let v = match snapshot(ctx, volume) {
        Ok(v) => v,
        Err(error) => return fail(ctx, error, INVALID_HANDLE),
    };
    let selected = if oid == 0 {
        v.databases
            .iter()
            .find(|(_, db)| db.name.eq_ignore_ascii_case(&name))
    } else {
        v.databases.get_key_value(&oid)
    };
    let Some((&oid, database)) = selected else {
        return fail(ctx, 2, INVALID_HANDLE);
    };
    let ordering = if sort_ptr == 0 {
        Sort::default()
    } else {
        let Some(order) = sort(ctx, sort_ptr) else {
            return fail(ctx, 87, INVALID_HANDLE);
        };
        order
    };
    if sort_ptr != 0 && !database.sorts.contains(&ordering) {
        return fail(ctx, 87, INVALID_HANDLE);
    }
    let cursor = model::Cursor {
        volume,
        database: oid,
        record: None,
        autoincrement: flags & 1 != 0,
        sort: ordering,
    };
    match ctx.kernel.cedb.generic.open(cursor) {
        Ok(handle) => {
            ctx.cpu.write_mem(oid_ptr, &oid.to_le_bytes())?;
            ret(handle)
        }
        Err(error) => fail(ctx, error, INVALID_HANDLE),
    }
}
fn delete_database(ctx: &mut CallCtx<'_>) -> Outcome {
    let ptr = ctx.arg_u32(0)?;
    let oid = ctx.arg_u32(1)?;
    let Some(volume) = guid(ctx, ptr) else {
        return fail(ctx, 87, 0);
    };
    if ctx
        .kernel
        .cedb
        .generic
        .cursors
        .values()
        .any(|c| c.volume == volume && c.database == oid)
    {
        return fail(ctx, 32, 0);
    }
    match ctx
        .kernel
        .cedb
        .generic
        .update(&mut ctx.kernel.vfs, volume, |v| {
            v.databases.remove(&oid).map(|_| ()).ok_or(2u32)
        }) {
        Ok(()) => ret(1),
        Err(error) => fail(ctx, error, 0),
    }
}
pub(super) fn set_info(ctx: &mut CallCtx<'_>) -> Outcome {
    let ptr = ctx.arg_u32(0)?;
    let oid = ctx.arg_u32(1)?;
    let info_ptr = ctx.arg_u32(2)?;
    let Some(volume) = guid(ctx, ptr) else {
        return fail(ctx, 87, 0);
    };
    let Some(data) = info(ctx, info_ptr) else {
        return fail(ctx, 87, 0);
    };
    let flags = ctx.cpu.read_u32_le(info_ptr + 4)?;
    match ctx
        .kernel
        .cedb
        .generic
        .update(&mut ctx.kernel.vfs, volume, |v| {
            if flags & 1 != 0
                && (data.name.is_empty()
                    || v.databases.iter().any(|(other, db)| {
                        *other != oid && db.name.eq_ignore_ascii_case(&data.name)
                    }))
            {
                return Err(183);
            }
            let db = v.databases.get_mut(&oid).ok_or(2u32)?;
            if flags & 1 != 0 {
                db.name = data.name;
            }
            if flags & 2 != 0 {
                db.kind = data.kind;
            }
            if flags & 4 != 0 {
                db.sorts = data.sorts;
            }
            if flags & 0x10 != 0 {
                db.flags = data.flags;
            }
            db.modified = if flags & 8 != 0 {
                data.modified
            } else {
                model::modified_now()
            };
            Ok(())
        }) {
        Ok(()) => ret(1),
        Err(error) => fail(ctx, error, 0),
    }
}
pub(super) fn get_info(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let Some(cursor) = ctx.kernel.cedb.generic.cursors.get(&handle).cloned() else {
        return fail(ctx, 6, 0);
    };
    if !access(ctx, out, 244, Prot::READ | Prot::WRITE) || word(&ctx.cpu.read_mem(out, 2)?, 0) != 1
    {
        return fail(ctx, 87, 0);
    }
    let volume = match snapshot(ctx, cursor.volume) {
        Ok(v) => v,
        Err(error) => return fail(ctx, error, 0),
    };
    let Some(db) = volume.databases.get(&cursor.database) else {
        return fail(ctx, 2, 0);
    };
    let mut bytes = vec![0; 24];
    bytes[..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[4..20].copy_from_slice(&cursor.volume);
    bytes[20..24].copy_from_slice(&cursor.database.to_le_bytes());
    bytes.extend(encode_info(db));
    ctx.cpu.write_mem(out, &bytes)?;
    ret(1)
}
fn property(ctx: &mut CallCtx<'_>, ptr: u32) -> Option<(u32, Option<Value>)> {
    if !access(ctx, ptr, 16, Prot::READ) {
        return None;
    }
    let bytes = ctx.cpu.read_mem(ptr, 16).ok()?;
    let id = dword(&bytes, 0);
    if word(&bytes, 6) & !0x200 != 0 {
        return None;
    }
    if word(&bytes, 6) & 0x200 != 0 {
        return Some((id, None));
    }
    let low = dword(&bytes, 8);
    let high = dword(&bytes, 12);
    let value = match id & 0xffff {
        2 => Value::I16(low as i16),
        18 => Value::U16(low as u16),
        3 => Value::I32(low as i32),
        19 => Value::U32(low),
        64 => Value::FileTime(u64::from(low) | (u64::from(high) << 32)),
        11 => Value::Bool(low),
        5 => Value::Double(u64::from(low) | (u64::from(high) << 32)),
        31 => {
            let text = crate::coredll::read_wstr(ctx, low, 32735).ok()?;
            if text.len() >= 32735 || !access(ctx, low, (text.len() as u32 + 1) * 2, Prot::READ) {
                return None;
            }
            Value::Text(text)
        }
        65 => {
            if low > 65471 || (low != 0 && !access(ctx, high, low, Prot::READ)) {
                return None;
            }
            Value::Blob(if low == 0 {
                Vec::new()
            } else {
                ctx.cpu.read_mem(high, low).ok()?
            })
        }
        _ => return None,
    };
    Some((id, Some(value)))
}
pub(super) fn write(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let oid = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let ptr = ctx.arg_u32(3)?;
    let Some(cursor) = ctx.kernel.cedb.generic.cursors.get(&handle).cloned() else {
        return fail(ctx, 6, 0);
    };
    if count == 0 || count > 1024 || !access(ctx, ptr, count * 16, Prot::READ) {
        return fail(ctx, 87, 0);
    }
    let mut props = Vec::new();
    for i in 0..count {
        let Some(p) = property(ctx, ptr + i * 16) else {
            return fail(ctx, 87, 0);
        };
        props.push(p);
    }
    match ctx
        .kernel
        .cedb
        .generic
        .update(&mut ctx.kernel.vfs, cursor.volume, |v| {
            let id = if oid == 0 {
                v.alloc_oid().ok_or(8u32)?
            } else {
                oid
            };
            let db = v.databases.get_mut(&cursor.database).ok_or(2u32)?;
            let mut record = if oid == 0 {
                if db.records.len() >= 65535 {
                    return Err(8);
                }
                Properties::new()
            } else {
                db.records.get(&id).cloned().ok_or(2u32)?
            };
            for (id, value) in props {
                if let Some(value) = value {
                    record.insert(id, value);
                } else {
                    record.remove(&id);
                }
            }
            let record_size = record.values().fold(record.len() * 16, |size, value| {
                size + match value {
                    Value::Text(text) => (text.len() + 1) * 2 + 3,
                    Value::Blob(bytes) => bytes.len() + 3,
                    _ => 0,
                }
            });
            if record_size > 128 * 1024 {
                return Err(87);
            }
            for ordering in &db.sorts {
                if ordering
                    .properties
                    .iter()
                    .any(|(id, flags)| flags & 0x400 != 0 && !record.contains_key(id))
                {
                    return Err(5);
                }
                if ordering.key_flags & 0x200 != 0
                    && db.records.iter().any(|(other, r)| {
                        *other != id && ordering.compare(r, &record) == Ordering::Equal
                    })
                {
                    return Err(183);
                }
            }
            db.records.insert(id, record);
            db.modified = model::modified_now();
            Ok(id)
        }) {
        Ok(oid) => ret(oid),
        Err(error) => fail(ctx, error, 0),
    }
}
pub(super) fn delete(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let oid = ctx.arg_u32(1)?;
    let Some(cursor) = ctx.kernel.cedb.generic.cursors.get(&handle).cloned() else {
        return fail(ctx, 6, 0);
    };
    match ctx
        .kernel
        .cedb
        .generic
        .update(&mut ctx.kernel.vfs, cursor.volume, |v| {
            let db = v.databases.get_mut(&cursor.database).ok_or(2u32)?;
            db.records.remove(&oid).ok_or(2u32)?;
            db.modified = model::modified_now();
            Ok(())
        }) {
        Ok(()) => {
            for c in ctx.kernel.cedb.generic.cursors.values_mut() {
                if c.volume == cursor.volume
                    && c.database == cursor.database
                    && c.record == Some(oid)
                {
                    c.record = Some(0);
                }
            }
            ret(1)
        }
        Err(error) => fail(ctx, error, 0),
    }
}
pub(super) fn seek(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let kind = ctx.arg_u32(1)?;
    let value = ctx.arg_u32(2)?;
    let count = ctx.arg_u32(3)?;
    let out = ctx.arg_u32(4)?;
    let Some(cursor) = ctx.kernel.cedb.generic.cursors.get(&handle).cloned() else {
        return fail(ctx, 6, 0);
    };
    if out != 0 && !access(ctx, out, 4, Prot::WRITE) {
        return fail(ctx, 87, 0);
    }
    let v = match snapshot(ctx, cursor.volume) {
        Ok(v) => v,
        Err(error) => return fail(ctx, error, 0),
    };
    let Some(db) = v.databases.get(&cursor.database) else {
        return fail(ctx, 2, 0);
    };
    let ids = db.ordered(&cursor.sort);
    let current = cursor
        .record
        .and_then(|id| ids.iter().position(|oid| *oid == id));
    let index = match kind {
        1 => ids.iter().position(|id| *id == value),
        2 => Some(value as usize),
        4 => ids.len().checked_sub(1 + value as usize),
        8 => current.and_then(|i| i.checked_add_signed(value as i32 as isize)),
        0x10 | 0x20 | 0x40 | 0x80 => {
            if count == 0 || count > 3 || !access(ctx, value, count * 16, Prot::READ) {
                return fail(ctx, 87, 0);
            }
            let mut wanted = Properties::new();
            for i in 0..count {
                let Some((id, Some(p))) = property(ctx, value + i * 16) else {
                    return fail(ctx, 87, 0);
                };
                wanted.insert(id, p);
            }
            let mut order = cursor.sort.clone();
            order.properties.truncate(count as usize);
            if order.properties.len() != count as usize
                || !order
                    .properties
                    .iter()
                    .all(|(id, _)| wanted.contains_key(id))
            {
                return fail(ctx, 87, 0);
            }
            let first = if kind == 0x80 {
                current.map_or(ids.len(), |i| i + 1)
            } else {
                0
            };
            let mut found = None;
            for (i, id) in ids.iter().enumerate().skip(first) {
                let ordering = order.compare(&db.records[id], &wanted);
                if (kind == 0x10 && ordering == Ordering::Less)
                    || (matches!(kind, 0x20 | 0x80) && ordering == Ordering::Equal)
                    || (kind == 0x40 && ordering == Ordering::Greater)
                {
                    found = Some(i);
                    if kind != 0x10 {
                        break;
                    }
                }
            }
            found
        }
        _ => return fail(ctx, 87, 0),
    };
    let Some((index, id)) = index.and_then(|i| ids.get(i).map(|id| (i, *id))) else {
        ctx.kernel
            .cedb
            .generic
            .cursors
            .get_mut(&handle)
            .unwrap()
            .record = Some(0);
        return fail(ctx, 259, 0);
    };
    if out != 0 {
        ctx.cpu.write_mem(out, &(index as u32).to_le_bytes())?;
    }
    ctx.kernel
        .cedb
        .generic
        .cursors
        .get_mut(&handle)
        .unwrap()
        .record = Some(id);
    ret(id)
}
fn read_ex(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let heap = ctx.arg_u32(6)?;
    if !matches!(heap, 0 | 0x4242_4242) {
        return fail(ctx, 6, 0);
    }
    if has_handle(ctx, handle) {
        read(ctx)
    } else {
        super::read(ctx)
    }
}
pub(super) fn read(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    let count_ptr = ctx.arg_u32(2)?;
    let ids_ptr = ctx.arg_u32(3)?;
    let buffer_ptr = ctx.arg_u32(4)?;
    let size_ptr = ctx.arg_u32(5)?;
    let Some(cursor) = ctx.kernel.cedb.generic.cursors.get(&handle).cloned() else {
        return fail(ctx, 6, 0);
    };
    if flags & !1 != 0
        || !access(ctx, count_ptr, 2, Prot::READ | Prot::WRITE)
        || !access(ctx, buffer_ptr, 4, Prot::READ | Prot::WRITE)
        || !access(ctx, size_ptr, 4, Prot::READ | Prot::WRITE)
    {
        return fail(ctx, 87, 0);
    }
    let count = word(&ctx.cpu.read_mem(count_ptr, 2)?, 0) as u32;
    if count > 1024 || count != 0 && !access(ctx, ids_ptr, count * 4, Prot::READ) {
        return fail(ctx, 87, 0);
    }
    let v = match snapshot(ctx, cursor.volume) {
        Ok(v) => v,
        Err(error) => return fail(ctx, error, 0),
    };
    let Some(db) = v.databases.get(&cursor.database) else {
        return fail(ctx, 2, 0);
    };
    let ordered = db.ordered(&cursor.sort);
    let Some(oid) = cursor
        .record
        .or_else(|| ordered.first().copied())
        .filter(|id| *id != 0)
    else {
        return fail(ctx, 259, 0);
    };
    let Some(record) = db.records.get(&oid) else {
        return fail(ctx, 259, 0);
    };
    let ids: Vec<u32> = if count == 0 {
        record.keys().copied().collect()
    } else {
        let mut ids = Vec::new();
        for i in 0..count {
            ids.push(ctx.cpu.read_u32_le(ids_ptr + i * 4)?);
        }
        ids
    };
    let mut data = vec![0; ids.len() * 16];
    let mut fixups = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let p = i * 16;
        data[p..p + 4].copy_from_slice(&id.to_le_bytes());
        let Some(value) = record.get(id) else {
            data[p + 6..p + 8].copy_from_slice(&0x100u16.to_le_bytes());
            continue;
        };
        let (low, high) = match value {
            Value::I16(v) => (*v as u16 as u32, 0),
            Value::U16(v) => (u32::from(*v), 0),
            Value::I32(v) => (*v as u32, 0),
            Value::U32(v) | Value::Bool(v) => (*v, 0),
            Value::FileTime(v) | Value::Double(v) => (*v as u32, (*v >> 32) as u32),
            Value::Text(text) => {
                while data.len() % 4 != 0 {
                    data.push(0);
                }
                let offset = data.len() as u32;
                let bytes: Vec<_> = text
                    .iter()
                    .copied()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                data[p + 4..p + 6].copy_from_slice(&(bytes.len().min(65535) as u16).to_le_bytes());
                data.extend(bytes);
                fixups.push((p + 8, offset));
                (0, 0)
            }
            Value::Blob(bytes) => {
                while data.len() % 4 != 0 {
                    data.push(0);
                }
                let offset = data.len() as u32;
                data[p + 4..p + 6].copy_from_slice(&(bytes.len().min(65535) as u16).to_le_bytes());
                data.extend(bytes);
                fixups.push((p + 12, offset));
                (bytes.len() as u32, 0)
            }
        };
        let fixed_size: u16 = match value {
            Value::I16(_) | Value::U16(_) => 2,
            Value::I32(_) | Value::U32(_) | Value::Bool(_) => 4,
            Value::FileTime(_) | Value::Double(_) => 8,
            Value::Text(_) | Value::Blob(_) => 0,
        };
        if fixed_size != 0 {
            data[p + 4..p + 6].copy_from_slice(&fixed_size.to_le_bytes());
        }
        data[p + 8..p + 12].copy_from_slice(&low.to_le_bytes());
        data[p + 12..p + 16].copy_from_slice(&high.to_le_bytes());
    }
    let old = ctx.cpu.read_u32_le(buffer_ptr)?;
    let capacity = ctx.cpu.read_u32_le(size_ptr)?;
    let required = data.len() as u32;
    let allocate = old == 0 || capacity < required;
    let buffer = if allocate {
        if flags & 1 == 0 {
            ctx.cpu.write_mem(size_ptr, &required.to_le_bytes())?;
            return fail(ctx, 122, 0);
        }
        if old != 0 && ctx.kernel.heap.msize(old).is_none() {
            return fail(ctx, 6, 0);
        }
        let DispatchOutcome::ReturnedR0(buffer) = crate::coredll::do_alloc(ctx, required.max(1))?
        else {
            return fail(ctx, 8, 0);
        };
        if buffer == 0 {
            return fail(ctx, 8, 0);
        }
        buffer
    } else {
        if !access(ctx, old, required, Prot::WRITE) {
            return fail(ctx, 998, 0);
        }
        old
    };
    for (at, offset) in fixups {
        data[at..at + 4].copy_from_slice(&(buffer + offset).to_le_bytes());
    }
    if ctx.cpu.write_mem(buffer, &data).is_err() {
        if allocate {
            crate::coredll::do_free(ctx, buffer);
        }
        return fail(ctx, 998, 0);
    }
    ctx.cpu.write_mem(buffer_ptr, &buffer.to_le_bytes())?;
    ctx.cpu.write_mem(size_ptr, &required.to_le_bytes())?;
    ctx.cpu
        .write_mem(count_ptr, &(ids.len() as u16).to_le_bytes())?;
    if allocate && old != 0 {
        crate::coredll::do_free(ctx, old);
    }
    if cursor.autoincrement {
        ctx.kernel
            .cedb
            .generic
            .cursors
            .get_mut(&handle)
            .unwrap()
            .record = Some(
            ordered
                .iter()
                .position(|id| *id == oid)
                .and_then(|i| ordered.get(i + 1).copied())
                .unwrap_or(0),
        );
    }
    ret(oid)
}
