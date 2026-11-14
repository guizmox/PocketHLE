//! Windows CE phone-book storage. No host dial-up configuration is modified.
use crate::{CallCtx, Handler, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{registry::RegistryValue, DispatchOutcome, KernelError};

const ENTRY_SIZE: u32 = 3472;
const DIAL_SIZE: u32 = 1464;
const CONN_SIZE: u32 = 52;
// CE6 ras.h uses pack(4), DWORD + WCHAR[21] + two padding bytes.
const NAME_SIZE: u32 = 48;
const MAX_BYTES: u32 = 65536;
type Result<T> = std::result::Result<T, u32>;

pub fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("RasEnumConnections", enumerate as Handler),
        ("RasEnumEntries", enumerate_entries),
        ("RasEnumEntriesW", enumerate_entries),
        ("RasGetEntryProperties", get),
        ("RasSetEntryProperties", set),
        ("RasSetEntryDialParams", set_dial),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{stub::StubCpu, Cpu};
    fn wide(cpu: &mut StubCpu, p: u32, s: &str) {
        cpu.write_mem(
            p,
            &s.encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    #[test]
    fn ce_phonebook_round_trip_sizes_devices_and_registry_persistence() {
        let (mut cpu, mut k, mut d) = setup();
        macro_rules! api {
            ($name:expr,$args:expr) => {
                call(&mut cpu, &mut k, &mut d, "coredll.dll", $name, $args)
            };
        }
        cpu.write_mem(0x1000, &[0, 0]).unwrap();
        cpu.write_mem(0x1800, &ENTRY_SIZE.to_le_bytes()).unwrap();
        cpu.write_mem(0x1400, &ENTRY_SIZE.to_le_bytes()).unwrap();
        assert_eq!(
            api!("RasGetEntryProperties", &[0, 0x1000, 0x1800, 0x1400, 0, 0]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1800 + 328).unwrap(), 1);
        wide(&mut cpu, 0x1000, "Test APN");
        cpu.write_mem(0x3000, b"opaque modem").unwrap();
        assert_eq!(
            api!(
                "RasSetEntryProperties",
                &[0, 0x1000, 0x1800, ENTRY_SIZE, 0x3000, 12]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        wide(&mut cpu, 0x1000, "test apn");
        cpu.write_mem(0x1400, &4u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1404, &0u32.to_le_bytes()).unwrap();
        assert_eq!(
            api!(
                "RasGetEntryProperties",
                &[0, 0x1000, 0x1800, 0x1400, 0, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(603)
        );
        assert_eq!(cpu.read_u32_le(0x1400).unwrap(), ENTRY_SIZE);
        assert_eq!(cpu.read_u32_le(0x1404).unwrap(), 12);
        cpu.write_mem(0x1404, &12u32.to_le_bytes()).unwrap();
        assert_eq!(
            api!(
                "RasGetEntryProperties",
                &[0, 0x1000, 0x1800, 0x1400, 0x3100, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_mem(0x3100, 12).unwrap(), b"opaque modem");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        k.registry.configure_persistence(&path).unwrap();
        k.registry.flush().unwrap();
        let mut fresh = pocket_kernel::registry::Registry::new();
        fresh.configure_persistence(&path).unwrap();
        assert!(matches!(
            fresh.value("HKCU\\Comm\\RasBook\\test apn", "PocketHLEEntry"),
            Some(RegistryValue::Binary(_))
        ));
    }
    #[test]
    fn ce_phonebook_absent_connections_and_invalid_structures_are_not_success() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.write_mem(0x1500, &CONN_SIZE.to_le_bytes()).unwrap();
        cpu.write_mem(0x1400, &CONN_SIZE.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumConnections",
                &[0x1500, 0x1400, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1400).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x1404).unwrap(), 0);
        cpu.write_mem(0x1400, &CONN_SIZE.to_le_bytes()).unwrap();
        cpu.write_mem(0x1500, &999u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumConnections",
                &[0x1500, 0x1400, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(632)
        );
        wide(&mut cpu, 0x1000, "missing");
        cpu.write_mem(0x1800, &ENTRY_SIZE.to_le_bytes()).unwrap();
        cpu.write_mem(0x1400, &ENTRY_SIZE.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasGetEntryProperties",
                &[0, 0x1000, 0x1800, 0x1400, 0, 0]
            ),
            DispatchOutcome::ReturnedR0(623)
        );
    }
    #[test]
    fn ce_phonebook_passwords_are_session_only_and_remove_clears_them() {
        let (mut cpu, mut k, mut d) = setup();
        wide(&mut cpu, 0x1000, "APN");
        let mut properties = vec![0; ENTRY_SIZE as usize];
        properties[..4].copy_from_slice(&ENTRY_SIZE.to_le_bytes());
        cpu.write_mem(0x1800, &properties).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasSetEntryProperties",
                &[0, 0x1000, 0x1800, ENTRY_SIZE, 0, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        cpu.write_mem(0x2800, &vec![0; DIAL_SIZE as usize]).unwrap();
        cpu.write_mem(0x2800, &DIAL_SIZE.to_le_bytes()).unwrap();
        wide(&mut cpu, 0x2804, "APN");
        wide(&mut cpu, 0x2800 + 916, "secret");
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasSetEntryDialParams",
                &[0, 0x2800, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let entry = load_for_test(&k);
        assert!(entry.dial[916..1430].iter().all(|b| *b == 0));
        assert!(
            matches!(k.registry.value("HKCU\\Comm\\RasBook\\APN","PocketHLESessionPassword"),Some(RegistryValue::Binary(v)) if v[0]==b's')
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasSetEntryDialParams",
                &[0, 0x2800, 1]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(
            matches!(k.registry.value("HKCU\\Comm\\RasBook\\APN","PocketHLESessionPassword"),Some(RegistryValue::Binary(v)) if v.iter().all(|b|*b==0))
        );
    }
    fn load_for_test(k: &pocket_kernel::KernelState) -> Entry {
        let Some(RegistryValue::Binary(b)) = k
            .registry
            .value("HKCU\\Comm\\RasBook\\APN", "PocketHLEEntry")
        else {
            panic!()
        };
        serde_json::from_slice(&b).unwrap()
    }
    #[test]
    fn phonebook_entry_enumeration_negotiates_the_ce48_byte_unicode_array() {
        let (mut cpu, mut k, mut d) = setup();
        for name in ["Work", "Тест"] {
            assert!(k.registry.set_value(
                &format!("HKCU\\Comm\\RasBook\\{name}"),
                "Entry",
                RegistryValue::Binary(vec![1])
            ));
        }
        cpu.write_mem(0x1400, &0u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1404, &99u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumEntries",
                &[0, 0, 0, 0x1400, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(603)
        );
        assert_eq!(cpu.read_u32_le(0x1400).unwrap(), 96);
        assert_eq!(cpu.read_u32_le(0x1404).unwrap(), 2);
        cpu.write_mem(0x1800, &[0xa5; 100]).unwrap();
        cpu.write_mem(0x1800, &NAME_SIZE.to_le_bytes()).unwrap();
        cpu.write_mem(0x1400, &NAME_SIZE.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumEntries",
                &[0, 0, 0x1800, 0x1400, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(603)
        );
        assert_eq!(cpu.read_mem(0x1804, 96).unwrap(), vec![0xa5; 96]);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumEntriesW",
                &[0, 0, 0x1800, 0x1400, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        for (index, name) in ["Work", "Тест"].into_iter().enumerate() {
            let data = cpu
                .read_mem(0x1800 + index as u32 * NAME_SIZE, NAME_SIZE)
                .unwrap();
            assert_eq!(&data[..4], &NAME_SIZE.to_le_bytes());
            assert_eq!(string(&data[4..46]).unwrap(), name);
            assert_eq!(&data[46..], &[0, 0]);
        }
        assert_eq!(cpu.read_mem(0x1860, 4).unwrap(), vec![0xa5; 4]);
    }
    #[test]
    fn empty_phonebook_has_no_invented_entries_and_bad_outputs_leave_size_intact() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.write_mem(0x1400, &0u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1404, &99u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumEntries",
                &[0, 0, 0, 0x1400, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_mem(0x1400, 8).unwrap(), vec![0; 8]);
        cpu.write_mem(0x1400, &NAME_SIZE.to_le_bytes()).unwrap();
        cpu.write_mem(0x1800, &999u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumEntries",
                &[0, 0, 0x1800, 0x1400, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(632)
        );
        assert_eq!(cpu.read_u32_le(0x1400).unwrap(), NAME_SIZE);
        cpu.write_mem(0x1800, &NAME_SIZE.to_le_bytes()).unwrap();
        cpu.map_region(0x8000, 0x1000, Prot::READ).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumEntries",
                &[0, 0, 0x1800, 0x1400, 0x8000]
            ),
            DispatchOutcome::ReturnedR0(610)
        );
        assert_eq!(cpu.read_u32_le(0x1400).unwrap(), NAME_SIZE);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RasEnumEntries",
                &[1, 0, 0, 0x1400, 0x1404]
            ),
            DispatchOutcome::ReturnedR0(87)
        );
    }
}
fn finish(value: Result<()>) -> std::result::Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value.err().unwrap_or(0)))
}
fn access(ctx: &mut CallCtx<'_>, p: u32, n: u32, prot: Prot) -> Result<()> {
    if p == 0
        || n > MAX_BYTES
        || p.checked_add(n).is_none()
        || ctx.cpu.check_guest_access(p, n, prot).is_err()
    {
        return Err(610);
    }
    Ok(())
}
fn read(ctx: &mut CallCtx<'_>, p: u32, n: u32) -> Result<Vec<u8>> {
    access(ctx, p, n, Prot::READ)?;
    ctx.cpu.read_mem(p, n).map_err(|_| 610)
}
fn word(ctx: &mut CallCtx<'_>, p: u32) -> Result<u32> {
    Ok(u32::from_le_bytes(read(ctx, p, 4)?.try_into().unwrap()))
}
fn string(bytes: &[u8]) -> Result<String> {
    let words: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let end = words.iter().position(|w| *w == 0).ok_or(87u32)?;
    String::from_utf16(&words[..end]).map_err(|_| 87)
}
fn name(ctx: &mut CallCtx<'_>, p: u32) -> Result<String> {
    if p == 0 {
        return Err(87);
    }
    let mut bytes = Vec::new();
    for i in 0..21 {
        let addr = p.checked_add(i * 2).ok_or(87u32)?;
        let next = read(ctx, addr, 2)?;
        let end = next == [0, 0];
        bytes.extend(next);
        if end {
            return string(&bytes);
        }
    }
    Err(87)
}
fn key(name: &str) -> Result<String> {
    if name.is_empty()
        || !name.chars().any(char::is_alphanumeric)
        || name
            .chars()
            .any(|c| c.is_control() || ['\\', '/'].contains(&c))
    {
        return Err(123);
    }
    Ok(format!("HKCU\\Comm\\RasBook\\{name}"))
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Entry {
    properties: Vec<u8>,
    device: Vec<u8>,
    dial: Vec<u8>,
}
fn load(ctx: &CallCtx<'_>, key: &str) -> Result<Entry> {
    match ctx.kernel.registry.value(key, "PocketHLEEntry") {
        Some(RegistryValue::Binary(data)) => {
            let entry: Entry = serde_json::from_slice(&data).map_err(|_| 621u32)?;
            if entry.properties.len() < ENTRY_SIZE as usize
                || entry.properties.len() > MAX_BYTES as usize
                || entry.device.len() > MAX_BYTES as usize
                || (!entry.dial.is_empty() && entry.dial.len() != DIAL_SIZE as usize)
                || entry.properties[..4] != ENTRY_SIZE.to_le_bytes()
            {
                return Err(621);
            }
            Ok(entry)
        }
        None => Err(623),
        _ => Err(621),
    }
}
fn save(ctx: &mut CallCtx<'_>, key: &str, entry: &Entry) -> Result<()> {
    let data = serde_json::to_vec(entry).map_err(|_| 621u32)?;
    if !ctx
        .kernel
        .registry
        .set_value(key, "PocketHLEEntry", RegistryValue::Binary(data))
    {
        return Err(8);
    }
    // Registry persistence is owned by the existing session registry lifecycle.
    Ok(())
}
fn enumerate(ctx: &mut CallCtx<'_>) -> std::result::Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let size = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    finish((|| {
        access(ctx, size, 4, Prot::READ | Prot::WRITE)?;
        access(ctx, count, 4, Prot::WRITE)?;
        let capacity = word(ctx, size)?;
        if p == 0 {
            return Err(610);
        }
        if capacity < CONN_SIZE {
            return Err(603);
        }
        if word(ctx, p)? != CONN_SIZE {
            return Err(632);
        }
        // No RasDial transport is installed. WinINet/GPRS host routing is not
        // an HRASCONN and must not manufacture an active dial-up connection.
        ctx.cpu
            .write_mem(size, &0u32.to_le_bytes())
            .map_err(|_| 610u32)?;
        ctx.cpu
            .write_mem(count, &0u32.to_le_bytes())
            .map_err(|_| 610u32)?;
        Ok(())
    })())
}
fn enumerate_entries(ctx: &mut CallCtx<'_>) -> std::result::Result<DispatchOutcome, KernelError> {
    let reserved = ctx.arg_u32(0)?;
    let phonebook = ctx.arg_u32(1)?;
    let p = ctx.arg_u32(2)?;
    let size = ctx.arg_u32(3)?;
    let count = ctx.arg_u32(4)?;
    finish((|| {
        // CE stores its phonebook in the device registry, not a host .pbk.
        if reserved != 0 || phonebook != 0 {
            return Err(87);
        }
        access(ctx, size, 4, Prot::READ | Prot::WRITE)?;
        access(ctx, count, 4, Prot::WRITE)?;
        let capacity = word(ctx, size)?;
        if p != 0 && word(ctx, p)? != NAME_SIZE {
            return Err(632);
        }
        let names = ctx.kernel.registry.subkeys("HKCU\\Comm\\RasBook");
        let needed = u32::try_from(names.len())
            .ok()
            .and_then(|count| count.checked_mul(NAME_SIZE))
            .filter(|bytes| *bytes <= MAX_BYTES)
            .ok_or(8u32)?;
        let mut data = Vec::with_capacity(needed as usize);
        for name in &names {
            key(name).map_err(|_| 621u32)?;
            let units: Vec<_> = name.encode_utf16().collect();
            if units.len() > 20 {
                return Err(621);
            }
            let mut entry = [0u8; NAME_SIZE as usize];
            entry[..4].copy_from_slice(&NAME_SIZE.to_le_bytes());
            for (index, unit) in units.into_iter().enumerate() {
                entry[4 + index * 2..6 + index * 2].copy_from_slice(&unit.to_le_bytes());
            }
            data.extend_from_slice(&entry);
        }
        if needed != 0 && capacity >= needed {
            access(ctx, p, needed, Prot::WRITE)?;
        }
        // ConnMC negotiates the complete array before showing saved entries.
        // Failed pointer/structure validation leaves both result words intact.
        ctx.cpu
            .write_mem(size, &needed.to_le_bytes())
            .map_err(|_| 610u32)?;
        ctx.cpu
            .write_mem(count, &(names.len() as u32).to_le_bytes())
            .map_err(|_| 610u32)?;
        if capacity < needed {
            return Err(603);
        }
        if needed != 0 {
            ctx.cpu.write_mem(p, &data).map_err(|_| 610u32)?;
        }
        Ok(())
    })())
}
fn set(ctx: &mut CallCtx<'_>) -> std::result::Result<DispatchOutcome, KernelError> {
    let n = ctx.arg_u32(1)?;
    let p = ctx.arg_u32(2)?;
    let size = ctx.arg_u32(3)?;
    let device = ctx.arg_u32(4)?;
    let device_size = ctx.arg_u32(5)?;
    finish((|| {
        let key = key(&name(ctx, n)?)?;
        if size < ENTRY_SIZE || size > MAX_BYTES {
            return Err(632);
        }
        let properties = read(ctx, p, size)?;
        if u32::from_le_bytes(properties[..4].try_into().unwrap()) != ENTRY_SIZE {
            return Err(632);
        }
        // Validate every fixed WCHAR field before persisting opaque properties.
        for (start, end) in [
            (16, 38),
            (38, 296),
            (332, 852),
            (852, 1372),
            (1372, 1892),
            (1892, 1926),
            (1926, 2184),
            (2184, 2250),
            (2250, 2652),
            (2652, 3054),
            (3054, 3456),
        ] {
            string(&properties[start..end])?;
        }
        let device = if device_size == 0 {
            Vec::new()
        } else {
            read(ctx, device, device_size)?
        };
        let mut entry = match load(ctx, &key) {
            Ok(v) => v,
            Err(623) => Entry::default(),
            Err(e) => return Err(e),
        };
        entry.properties = properties;
        entry.device = device;
        save(ctx, &key, &entry)
    })())
}
fn get(ctx: &mut CallCtx<'_>) -> std::result::Result<DispatchOutcome, KernelError> {
    let n = ctx.arg_u32(1)?;
    let p = ctx.arg_u32(2)?;
    let size = ctx.arg_u32(3)?;
    let device = ctx.arg_u32(4)?;
    let device_size = ctx.arg_u32(5)?;
    finish((|| {
        let name = name(ctx, n)?;
        access(ctx, size, 4, Prot::READ | Prot::WRITE)?;
        let capacity = word(ctx, size)?;
        if p == 0 {
            return Err(610);
        }
        if word(ctx, p)? != ENTRY_SIZE {
            return Err(632);
        }
        let entry = if name.is_empty() {
            let mut entry = Entry {
                properties: vec![0; ENTRY_SIZE as usize],
                ..Entry::default()
            };
            for (offset, value) in [(0, ENTRY_SIZE), (320, 1500), (324, 4), (328, 1), (3456, 1)] {
                entry.properties[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            entry
        } else {
            load(ctx, &key(&name)?)?
        };
        let needed = entry.properties.len() as u32;
        let device_needed = entry.device.len() as u32;
        let device_capacity = if device_size != 0 {
            access(ctx, device_size, 4, Prot::READ | Prot::WRITE)?;
            word(ctx, device_size)?
        } else {
            if device != 0 {
                return Err(87);
            }
            0
        };
        if capacity >= needed {
            access(ctx, p, needed, Prot::WRITE)?;
        }
        if device != 0 && device_capacity >= device_needed && device_needed != 0 {
            access(ctx, device, device_needed, Prot::WRITE)?;
        }
        ctx.cpu
            .write_mem(size, &needed.to_le_bytes())
            .map_err(|_| 610u32)?;
        if device_size != 0 {
            ctx.cpu
                .write_mem(device_size, &device_needed.to_le_bytes())
                .map_err(|_| 610u32)?;
        }
        if capacity < needed
            || (device_needed != 0
                && device_size != 0
                && (device == 0 || device_capacity < device_needed))
        {
            return Err(603);
        }
        ctx.cpu
            .write_mem(p, &entry.properties)
            .map_err(|_| 610u32)?;
        if device != 0 && device_needed != 0 {
            ctx.cpu
                .write_mem(device, &entry.device)
                .map_err(|_| 610u32)?;
        }
        Ok(())
    })())
}
fn set_dial(ctx: &mut CallCtx<'_>) -> std::result::Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(1)?;
    let remove = ctx.arg_u32(2)?;
    finish((|| {
        if word(ctx, p)? != DIAL_SIZE {
            return Err(632);
        }
        let mut dial = read(ctx, p, DIAL_SIZE)?;
        let key = key(&string(&dial[4..46])?)?;
        for (start, end) in [(46, 304), (304, 402), (402, 916), (916, 1430), (1430, 1462)] {
            string(&dial[start..end])?;
        }
        let mut entry = load(ctx, &key)?;
        if remove != 0 {
            dial[916..1430].fill(0);
        }
        let password = dial[916..1430].to_vec();
        dial[916..1430].fill(0);
        // Keep passwords in session memory, never plaintext registry backups.
        // There is no RasDial transport in this batch; persisting secrets needs
        // a platform credential provider rather than a fake encryption scheme.
        entry.dial = dial;
        save(ctx, &key, &entry)?;
        ctx.kernel.registry.set_volatile_value(
            &key,
            "PocketHLESessionPassword",
            RegistryValue::Binary(password),
        );
        Ok(())
    })())
}
