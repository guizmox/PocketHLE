//! CEDB contacts used by the Gizmondo shell.
use crate::{CallCtx, Handler, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{cedb::Cursor, DispatchOutcome, KernelError};

const DATABASE_OID: u32 = 1;
const INVALID_HANDLE: u32 = u32::MAX;
const PROP_SIZE: usize = 16;
const LPWSTR: u32 = 31;
const SORT_PROPERTY: u32 = 0x4013_001f;
type Outcome = Result<DispatchOutcome, KernelError>;

pub fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("CeOpenDatabaseEx2", open as Handler),
        ("CeSetDatabaseInfoEx2", set_info),
        ("CeGetDBInformationByHandle", get_info),
        ("CeSeekDatabaseEx", seek),
        ("CeReadRecordProps", read),
        ("CeWriteRecordProps", write),
        ("CeDeleteRecord", delete),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}

fn ret(value: u32) -> Outcome {
    Ok(DispatchOutcome::ReturnedR0(value))
}

fn fail(ctx: &mut CallCtx<'_>, error: u32, value: u32) -> Outcome {
    crate::coredll::set_thread_error(ctx, error);
    ret(value)
}

fn system_volume(ctx: &mut CallCtx<'_>, ptr: u32) -> bool {
    ptr != 0 && ctx.cpu.read_mem(ptr, 16).is_ok_and(|guid| guid == [0; 16])
}

fn access(ctx: &CallCtx<'_>, ptr: u32, size: u32, prot: Prot) -> bool {
    ptr != 0 && ptr.checked_add(size).is_some() && ctx.cpu.check_guest_access(ptr, size, prot).is_ok()
}

fn open(ctx: &mut CallCtx<'_>) -> Outcome {
    let volume = ctx.arg_u32(0)?;
    let oid_ptr = ctx.arg_u32(1)?;
    let name = ctx.arg_u32(2)?;
    let sort = ctx.arg_u32(3)?;
    let flags = ctx.arg_u32(4)?;
    if !system_volume(ctx, volume) || !access(ctx, oid_ptr, 4, Prot::READ | Prot::WRITE)
        || sort != 0 || flags & !1 != 0
    {
        return fail(ctx, 87, INVALID_HANDLE);
    }
    let oid = ctx.cpu.read_u32_le(oid_ptr)?;
    let found = if oid == 0 {
        crate::ce_shortcut::text(ctx, name, 32)
            .is_ok_and(|name| name.eq_ignore_ascii_case("Contacts Database"))
    } else {
        oid == DATABASE_OID
    };
    if !found {
        return fail(ctx, 2, INVALID_HANDLE);
    }
    if ctx.kernel.cedb.snapshot().is_err() {
        return fail(ctx, 13, INVALID_HANDLE);
    }
    let store = &mut ctx.kernel.cedb;
    if store.cursors.len() >= 64 || store.next_handle >= 0x00ff_ffff {
        return fail(ctx, 8, INVALID_HANDLE);
    }
    store.next_handle += 1;
    let handle = 0xe200_0000 + store.next_handle;
    ctx.cpu.write_mem(oid_ptr, &DATABASE_OID.to_le_bytes())?;
    store.cursors.insert(handle, Cursor { oid: None, autoincrement: flags & 1 != 0 });
    ret(handle)
}

fn set_info(ctx: &mut CallCtx<'_>) -> Outcome {
    let volume = ctx.arg_u32(0)?;
    let oid = ctx.arg_u32(1)?;
    let info = ctx.arg_u32(2)?;
    if !system_volume(ctx, volume) || oid != DATABASE_OID {
        return fail(ctx, 87, 0);
    }
    let Ok(data) = ctx.cpu.read_mem(info, 0x68) else { return fail(ctx, 998, 0) };
    if word(&data, 0) != 1 || word(&data, 2) != 1 || dword(&data, 4) != 4
        || word(&data, 0x5c) != 1 || word(&data, 0x5e) != 1
        || dword(&data, 0x60) != 0 || dword(&data, 0x64) != SORT_PROPERTY
    {
        return fail(ctx, 87, 0);
    }
    match ctx.kernel.cedb.update(|db| {
        if db.sort == SORT_PROPERTY {
            return None;
        }
        db.sort = SORT_PROPERTY;
        Some(())
    }) {
        Ok(_) => ret(1),
        Err(_) => fail(ctx, 29, 0),
    }
}

fn get_info(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    if !ctx.kernel.cedb.cursors.contains_key(&handle) {
        return fail(ctx, 6, 0);
    }
    if !access(ctx, out, 0xf4, Prot::READ | Prot::WRITE)
        || word(&ctx.cpu.read_mem(out, 2)?, 0) != 1
    {
        return fail(ctx, 87, 0);
    }
    let Ok(db) = ctx.kernel.cedb.snapshot() else { return fail(ctx, 13, 0) };
    let mut data = [0; 0xf4];
    data[..2].copy_from_slice(&1u16.to_le_bytes());
    data[0x14..0x18].copy_from_slice(&DATABASE_OID.to_le_bytes());
    let info = &mut data[0x18..];
    info[..2].copy_from_slice(&1u16.to_le_bytes());
    for (i, unit) in "Contacts Database".encode_utf16().enumerate() {
        info[8 + i * 2..10 + i * 2].copy_from_slice(&unit.to_le_bytes());
    }
    info[0x4c..0x50].copy_from_slice(&(db.records.len() as u32).to_le_bytes());
    info[0x54..0x5c].copy_from_slice(&db.last_modified.to_le_bytes());
    if db.sort != 0 {
        info[2..4].copy_from_slice(&1u16.to_le_bytes());
        info[0x5c..0x60].copy_from_slice(&[1, 0, 1, 0]);
        info[0x64..0x68].copy_from_slice(&db.sort.to_le_bytes());
    }
    ctx.cpu.write_mem(out, &data)?;
    ret(1)
}

fn seek(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let kind = ctx.arg_u32(1)?;
    let value = ctx.arg_u32(2)?;
    let count = ctx.arg_u32(3)?;
    let index_out = ctx.arg_u32(4)?;
    let Some(cursor) = ctx.kernel.cedb.cursors.get(&handle) else { return fail(ctx, 6, 0) };
    let Ok(db) = ctx.kernel.cedb.snapshot() else { return fail(ctx, 13, 0) };
    let ids = db.ordered_oids();
    let current = cursor.oid.and_then(|oid| ids.iter().position(|&id| id == oid));
    let index = match kind {
        1 => ids.iter().position(|&oid| oid == value),
        2 => Some(value as usize),
        4 => ids.len().checked_sub(1 + value as usize),
        8 => current.and_then(|index| index.checked_add_signed(value as i32 as isize)),
        0x20 if count == 1 => {
            let Some((prop, Some(text))) = property(ctx, value) else { return fail(ctx, 87, 0) };
            let text = String::from_utf16_lossy(&text).to_lowercase();
            ids.iter().position(|oid| db.records[oid].get(&prop).is_some_and(|v| {
                String::from_utf16_lossy(v).to_lowercase() == text
            }))
        }
        _ => return fail(ctx, 87, 0),
    };
    let Some(index) = index.filter(|&i| i < ids.len()) else { return fail(ctx, 259, 0) };
    if index_out != 0 && ctx.cpu.write_mem(index_out, &(index as u32).to_le_bytes()).is_err() {
        return fail(ctx, 998, 0);
    }
    let oid = ids[index];
    ctx.kernel.cedb.cursors.get_mut(&handle).unwrap().oid = Some(oid);
    ret(oid)
}

fn word(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(data[off..off + 2].try_into().unwrap())
}
fn dword(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
}

fn property(ctx: &mut CallCtx<'_>, ptr: u32) -> Option<(u32, Option<Vec<u16>>)> {
    let data = ctx.cpu.read_mem(ptr, PROP_SIZE as u32).ok()?;
    let id = dword(&data, 0);
    if id & 0xffff != LPWSTR {
        return None;
    }
    if word(&data, 6) & 0x0200 != 0 {
        return Some((id, None));
    }
    let ptr = dword(&data, 8);
    if ptr == 0 || ptr > u32::MAX - 8192 {
        return None;
    }
    let value = crate::coredll::read_wstr(ctx, ptr, 4096).ok()?;
    (value.len() < 4096).then_some((id, Some(value)))
}

fn write(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let oid = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let ptr = ctx.arg_u32(3)?;
    if !ctx.kernel.cedb.cursors.contains_key(&handle) {
        return fail(ctx, 6, 0);
    }
    if count == 0 || count > 64 || !access(ctx, ptr, count * PROP_SIZE as u32, Prot::READ) {
        return fail(ctx, 87, 0);
    }
    let mut props = Vec::new();
    for i in 0..count {
        let Some(value) = property(ctx, ptr + i * PROP_SIZE as u32) else { return fail(ctx, 87, 0) };
        props.push(value);
    }
    match ctx.kernel.cedb.update(|db| {
        let oid = if oid == 0 {
            db.next_oid = db.next_oid.max(DATABASE_OID).checked_add(1)?;
            db.records.insert(db.next_oid, Default::default());
            db.next_oid
        } else {
            oid
        };
        let record = db.records.get_mut(&oid)?;
        for (id, value) in props {
            if let Some(value) = value {
                record.insert(id, value);
            } else {
                record.remove(&id);
            }
        }
        Some(oid)
    }) {
        Ok(Some(oid)) => ret(oid),
        Ok(None) => fail(ctx, 2, 0),
        Err(_) => fail(ctx, 29, 0),
    }
}

fn delete(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let oid = ctx.arg_u32(1)?;
    if !ctx.kernel.cedb.cursors.contains_key(&handle) {
        return fail(ctx, 6, 0);
    }
    match ctx.kernel.cedb.update(|db| db.records.remove(&oid)) {
        Ok(Some(_)) => {
            for cursor in ctx.kernel.cedb.cursors.values_mut() {
                if cursor.oid == Some(oid) {
                    cursor.oid = None;
                }
            }
            ret(1)
        }
        Ok(None) => fail(ctx, 2, 0),
        Err(_) => fail(ctx, 29, 0),
    }
}

fn read(ctx: &mut CallCtx<'_>) -> Outcome {
    let handle = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    let count_ptr = ctx.arg_u32(2)?;
    let ids_ptr = ctx.arg_u32(3)?;
    let buffer_ptr = ctx.arg_u32(4)?;
    let size_ptr = ctx.arg_u32(5)?;
    let Some(cursor) = ctx.kernel.cedb.cursors.get(&handle) else { return fail(ctx, 6, 0) };
    let autoincrement = cursor.autoincrement;
    let Ok(db) = ctx.kernel.cedb.snapshot() else { return fail(ctx, 13, 0) };
    let Some(oid) = cursor.oid.or_else(|| db.ordered_oids().first().copied()) else {
        return fail(ctx, 259, 0);
    };
    let Some(record) = db.records.get(&oid) else { return fail(ctx, 259, 0) };
    if flags & !1 != 0
        || !access(ctx, count_ptr, 2, Prot::READ | Prot::WRITE)
        || !access(ctx, buffer_ptr, 4, Prot::READ | Prot::WRITE)
        || !access(ctx, size_ptr, 4, Prot::READ | Prot::WRITE)
    {
        return fail(ctx, 87, 0);
    }
    let count = word(&ctx.cpu.read_mem(count_ptr, 2)?, 0);
    if count > 64 {
        return fail(ctx, 87, 0);
    }
    let ids: Vec<_> = if count == 0 || ids_ptr == 0 {
        record.keys().copied().collect()
    } else {
        let Ok(bytes) = ctx.cpu.read_mem(ids_ptr, u32::from(count) * 4) else { return fail(ctx, 998, 0) };
        bytes.chunks_exact(4).map(|chunk| dword(chunk, 0)).collect()
    };
    let mut data = vec![0; ids.len() * PROP_SIZE];
    let mut fixups = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let off = i * PROP_SIZE;
        data[off..off + 4].copy_from_slice(&id.to_le_bytes());
        if let Some(value) = record.get(id) {
            fixups.push((off + 8, data.len() as u32));
            data.extend(value.iter().copied().chain(Some(0)).flat_map(u16::to_le_bytes));
        } else {
            data[off + 6..off + 8].copy_from_slice(&0x0100u16.to_le_bytes());
        }
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
        let DispatchOutcome::ReturnedR0(buffer) = crate::coredll::do_alloc(ctx, required.max(1))? else {
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
    ctx.cpu.write_mem(buffer, &data)?;
    ctx.cpu.write_mem(buffer_ptr, &buffer.to_le_bytes())?;
    ctx.cpu.write_mem(size_ptr, &required.to_le_bytes())?;
    ctx.cpu.write_mem(count_ptr, &(ids.len() as u16).to_le_bytes())?;
    if allocate && old != 0 {
        crate::coredll::do_free(ctx, old);
    }
    if autoincrement {
        let ids = db.ordered_oids();
        ctx.kernel.cedb.cursors.get_mut(&handle).unwrap().oid = Some(
            ids.iter().position(|&id| id == oid)
                .and_then(|index| ids.get(index + 1).copied()).unwrap_or(0),
        );
    }
    ret(oid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn ce_database_contact_lifecycle_survives_reopen() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("contacts.json");
        kernel.cedb.path = Some(path.clone());
        macro_rules! invoke {
            ($name:literal, $($arg:expr),* $(,)?) => {{
                let DispatchOutcome::ReturnedR0(value) =
                    call(&mut cpu, &mut kernel, &mut dispatcher, "coredll.dll", $name, &[$($arg),*])
                else { panic!("unexpected dispatch outcome") };
                value
            }};
        }
        fn wide(value: &str) -> Vec<u8> {
            value.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect()
        }
        cpu.write_mem(0x1100, &wide("Contacts Database")).unwrap();
        let h = invoke!("CeOpenDatabaseEx2", 0x1000, 0x1020, 0x1100, 0, 0, 0);
        assert_ne!(h, INVALID_HANDLE);
        assert_eq!(cpu.read_u32_le(0x1020).unwrap(), DATABASE_OID);
        let mut info = [0; 0x68];
        info[..8].copy_from_slice(&[1, 0, 1, 0, 4, 0, 0, 0]);
        info[0x5c..0x60].copy_from_slice(&[1, 0, 1, 0]);
        info[0x64..].copy_from_slice(&SORT_PROPERTY.to_le_bytes());
        cpu.write_mem(0x1200, &info).unwrap();
        assert_eq!(invoke!("CeSetDatabaseInfoEx2", 0x1000, DATABASE_OID, 0x1200), 1);

        let mut prop = [0; PROP_SIZE];
        prop[..4].copy_from_slice(&SORT_PROPERTY.to_le_bytes());
        prop[8..12].copy_from_slice(&0x1400u32.to_le_bytes());
        cpu.write_mem(0x1300, &prop).unwrap();
        cpu.write_mem(0x1400, &wide("Zoé")).unwrap();
        let zoe = invoke!("CeWriteRecordProps", h, 0, 1, 0x1300);
        cpu.write_mem(0x1400, &wide("Ana")).unwrap();
        let ana = invoke!("CeWriteRecordProps", h, 0, 1, 0x1300);
        assert_ne!(ana, zoe);
        assert_eq!(invoke!("CloseHandle", h), 1);

        kernel.cedb = Default::default();
        kernel.cedb.path = Some(path);
        let h = invoke!("CeOpenDatabaseEx2", 0x1000, 0x1020, 0, 0, 0, 0);
        assert_eq!(invoke!("CeSeekDatabaseEx", h, 2, 0, 0, 0), ana);
        assert_eq!(invoke!("CeSeekDatabaseEx", h, 8, 1, 0, 0), zoe);
        cpu.write_mem(0x1500, &0u16.to_le_bytes()).unwrap();
        cpu.write_mem(0x1504, &0x1600u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1508, &128u32.to_le_bytes()).unwrap();
        assert_eq!(invoke!("CeReadRecordProps", h, 0, 0x1500, 0, 0x1504, 0x1508), zoe);
        let value = cpu.read_u32_le(0x1608).unwrap();
        assert_eq!(cpu.read_mem(value, 8).unwrap(), wide("Zoé"));
        cpu.write_mem(0x1400, &wide("Béa")).unwrap();
        assert_eq!(invoke!("CeWriteRecordProps", h, zoe, 1, 0x1300), zoe);
        assert_eq!(invoke!("CeSeekDatabaseEx", h, 0x20, 0x1300, 1, 0), zoe);
        assert_eq!(invoke!("CeDeleteRecord", h, ana), 1);
        assert_eq!(invoke!("CeSeekDatabaseEx", h, 2, 0, 0, 0), zoe);
    }
}
