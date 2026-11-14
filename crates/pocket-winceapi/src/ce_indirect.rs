//! CE shell display names stored as @module,-resourceID strings.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::{CpuError, Prot};
use pocket_kernel::{DispatchOutcome, KernelError};
use pocket_pe::ResourceKey;

const E_FAIL: u32 = 0x8000_4005;
const E_POINTER: u32 = 0x8000_4003;
const E_INVALIDARG: u32 = 0x8007_0057;
const MAX_SOURCE_CHARS: u32 = 32_768;

pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "SHLoadIndirectString", load);
}

fn source_units(ctx: &mut CallCtx<'_>, source: u32) -> Result<Result<Vec<u16>, u32>, KernelError> {
    if source == 0 {
        return Ok(Err(E_INVALIDARG));
    }
    let mut units = Vec::new();
    for index in 0..MAX_SOURCE_CHARS {
        let Some(addr) = source.checked_add(index * 2) else {
            return Ok(Err(E_POINTER));
        };
        let access = ctx
            .cpu
            .check_guest_access(addr, 2, Prot::READ)
            .and_then(|()| ctx.cpu.read_mem(addr, 2));
        let bytes = match access {
            Ok(bytes) => bytes,
            // Demand commitment of a cold image page must retain the CPU's
            // explicit RAM failure, rather than masquerading as a bad string.
            Err(error @ CpuError::ImageOutOfMemory { .. }) => return Err(error.into()),
            Err(_) => return Ok(Err(E_POINTER)),
        };
        let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
        if unit == 0 {
            return Ok(Ok(units));
        }
        units.push(unit);
    }
    Ok(Err(E_INVALIDARG))
}

fn parse_locator(source: &str) -> Result<(&str, u32), u32> {
    let (module, locator) = source
        .strip_prefix('@')
        .and_then(|s| s.rsplit_once(','))
        .ok_or(E_INVALIDARG)?;
    let module = module.trim();
    let module = module
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(module);
    if module.is_empty() {
        return Err(E_INVALIDARG);
    }
    let locator = locator.trim();
    let locator = if let Some((id, version)) = locator.split_once(';') {
        if !version.starts_with('v')
            || version.len() == 1
            || !version[1..].bytes().all(|b| b.is_ascii_digit())
        {
            return Err(E_INVALIDARG);
        }
        id
    } else {
        locator
    };
    // Resource indices are a different locator from numeric resource IDs.
    // Report unsupported indices honestly instead of borrowing an arbitrary ID.
    let id = locator.strip_prefix('-').ok_or(E_FAIL)?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(E_INVALIDARG);
    }
    let id: u32 = id.parse().map_err(|_| E_INVALIDARG)?;
    if id > u16::MAX as u32 {
        return Err(E_INVALIDARG);
    }
    Ok((module, id))
}

fn block_string(bytes: &[u8], id: u32) -> Result<Vec<u16>, u32> {
    let mut pos = 0usize;
    for index in 0..=(id & 15) {
        let length = bytes.get(pos..pos + 2).ok_or(E_FAIL)?;
        let length = u16::from_le_bytes([length[0], length[1]]) as usize;
        pos += 2;
        let end = pos.checked_add(length * 2).ok_or(E_FAIL)?;
        let record = bytes.get(pos..end).ok_or(E_FAIL)?;
        if index == (id & 15) {
            let units: Vec<_> = record
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .take_while(|&unit| unit != 0)
                .collect();
            return if units.is_empty() {
                Err(E_FAIL)
            } else {
                Ok(units)
            };
        }
        pos = end;
    }
    Err(E_FAIL)
}

fn resolve(ctx: &mut CallCtx<'_>, source: &[u16]) -> Result<(Vec<u16>, bool), u32> {
    if source.first() != Some(&(b'@' as u16)) {
        return Ok((source.to_vec(), false));
    }
    let text = String::from_utf16(source).map_err(|_| E_INVALIDARG)?;
    let (module, id) = parse_locator(&text)?;
    // Original CE6 Explorer resolves registry display names while opening
    // Start. Read the named file's resources only: loading its native import
    // graph/DllMain here would re-enter shell initialization, and a global
    // resource fallback can return another DLL's unrelated string with this ID.
    let path = ctx.kernel.find_module_file(module).ok_or(E_FAIL)?;
    let file = std::fs::read(path).map_err(|_| E_FAIL)?;
    let block =
        pocket_pe::file_resource(&file, &ResourceKey::Id(6), &ResourceKey::Id((id >> 4) + 1))
            .map_err(|_| E_FAIL)?
            .ok_or(E_FAIL)?;
    let units = block_string(block, id)?;
    log::debug!("SHLoadIndirectString({text:?}) -> {} chars", units.len());
    Ok((units, true))
}

fn load(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let source = ctx.arg_u32(0)?;
    let output = ctx.arg_u32(1)?;
    let capacity = ctx.arg_u32(2)?;
    let reserved = ctx.arg_u32(3)?;
    // Explorer passes the same pointer for both arguments. Snapshot the source
    // before clearing or replacing the caller's output, including failure paths.
    let result = if output == 0 || reserved != 0 || capacity > i32::MAX as u32 {
        Err(E_INVALIDARG)
    } else {
        source_units(ctx, source)?.and_then(|units| resolve(ctx, &units))
    };
    let (units, mut hr) = match result {
        Ok((units, indirect)) => {
            let copied = units.len().min(capacity.saturating_sub(1) as usize);
            let hr = if indirect && copied == 0 { E_FAIL } else { 0 };
            (units[..copied].to_vec(), hr)
        }
        Err(hr) => (Vec::new(), hr),
    };
    if output != 0 && capacity != 0 {
        let bytes: Vec<_> = units
            .into_iter()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        if output.checked_add(bytes.len() as u32).is_none() {
            hr = E_POINTER;
        } else {
            let write = ctx
                .cpu
                .check_guest_access(output, bytes.len() as u32, Prot::WRITE)
                .and_then(|()| ctx.cpu.write_mem(output, &bytes));
            match write {
                Ok(()) => {}
                Err(error @ CpuError::ImageOutOfMemory { .. }) => return Err(error.into()),
                Err(_) => hr = E_POINTER,
            }
        }
    }
    Ok(DispatchOutcome::ReturnedR0(hr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{stub::StubCpu, Cpu};

    fn put(cpu: &mut StubCpu, addr: u32, text: &str) {
        cpu.write_mem(
            addr,
            &text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }

    fn get(cpu: &mut StubCpu, addr: u32) -> String {
        let mut units = Vec::new();
        loop {
            let bytes = cpu.read_mem(addr + units.len() as u32 * 2, 2).unwrap();
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                return String::from_utf16(&units).unwrap();
            }
            units.push(unit);
        }
    }

    // Resource-only PE fixture. It has no executable entry point or imports.
    fn resource_file(id: u32, text: &str) -> Vec<u8> {
        fn word(bytes: &mut [u8], at: usize, value: u16) {
            bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn dword(bytes: &mut [u8], at: usize, value: u32) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut bytes = vec![0; 0x400];
        bytes[..2].copy_from_slice(b"MZ");
        dword(&mut bytes, 0x3c, 0x80);
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        word(&mut bytes, 0x84, pocket_pe::machine::ARM);
        word(&mut bytes, 0x86, 1);
        word(&mut bytes, 0x94, 224);
        word(&mut bytes, 0x96, 0x2102);
        word(&mut bytes, 0x98, 0x10b);
        dword(&mut bytes, 0xb4, 0x400000);
        dword(&mut bytes, 0xb8, 0x1000);
        dword(&mut bytes, 0xbc, 0x200);
        dword(&mut bytes, 0xd0, 0x2000);
        dword(&mut bytes, 0xd4, 0x200);
        word(&mut bytes, 0xdc, pocket_pe::subsystem::WINDOWS_CE_GUI);
        dword(&mut bytes, 0xf4, 16);
        dword(&mut bytes, 0x108, 0x1000);
        dword(&mut bytes, 0x10c, 0x200);
        bytes[0x178..0x180].copy_from_slice(b".rsrc\0\0\0");
        for (at, value) in [
            (0x180, 0x200),
            (0x184, 0x1000),
            (0x188, 0x200),
            (0x18c, 0x200),
            (0x19c, 0x40000040),
        ] {
            dword(&mut bytes, at, value);
        }
        for at in [0x20e, 0x226, 0x23e] {
            word(&mut bytes, at, 1);
        }
        for (at, value) in [
            (0x210, 6),
            (0x214, 0x80000018),
            (0x228, (id >> 4) + 1),
            (0x22c, 0x80000030),
            (0x240, 0x409),
            (0x244, 0x48),
            (0x248, 0x1058),
        ] {
            dword(&mut bytes, at, value);
        }
        let mut block = Vec::new();
        for index in 0..16 {
            let units: Vec<_> = if index == (id & 15) {
                text.encode_utf16().collect()
            } else {
                Vec::new()
            };
            block.extend_from_slice(&(units.len() as u16).to_le_bytes());
            block.extend(units.into_iter().flat_map(u16::to_le_bytes));
        }
        dword(&mut bytes, 0x24c, block.len() as u32);
        bytes[0x258..0x258 + block.len()].copy_from_slice(&block);
        bytes
    }

    #[test]
    fn shell_literal_aliases_copy_unicode_and_preserve_buffer_bounds() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        for (input, output, cap, expected) in [
            (0x1000, 0x1000, 64, "Параметры"),
            (0x1000, 0x1002, 64, "Параметры"),
            (0x1000, 0x2000, 4, "Пар"),
            (0x1000, 0x2000, 1, ""),
        ] {
            put(&mut cpu, input, "Параметры");
            cpu.write_mem(output + cap * 2, &[0xa5, 0x5a]).unwrap();
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "COREDLL.dll",
                    "SHLoadIndirectString",
                    &[input, output, cap, 0]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
            assert_eq!(get(&mut cpu, output), expected);
            assert_eq!(cpu.read_mem(output + cap * 2, 2).unwrap(), [0xa5, 0x5a]);
        }
        // Literal copies operate on UTF-16 units, without lossy Unicode repair.
        cpu.write_mem(0x1000, &[0x00, 0xd8, 0, 0]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SHLoadIndirectString",
                &[0x1000, 0x2000, 2, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_mem(0x2000, 4).unwrap(), [0x00, 0xd8, 0, 0]);
    }

    #[test]
    fn shell_indirect_names_use_only_the_named_file_without_loading_its_code() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let dir = tempfile::tempdir().unwrap();
        let windows = dir.path().join("Windows");
        std::fs::create_dir(&windows).unwrap();
        let file = resource_file(35, "Параметры");
        std::fs::write(windows.join("Names.DLL"), &file).unwrap();
        std::fs::write(windows.join("Other.DLL"), resource_file(36, "Other")).unwrap();
        kernel.vfs.mount("\\", dir.path());
        kernel.module_search_dirs.push(windows.clone());
        kernel.thread_last_errors.insert(0, 0x1234);
        for input in ["@\"\\Windows\\Names.DLL\",-35", "@NAMES.DLL,-35;v2"] {
            put(&mut cpu, 0x1000, input);
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "SHLoadIndirectString",
                    &[0x1000, 0x1000, 260, 0]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
            assert_eq!(get(&mut cpu, 0x1000), "Параметры");
            assert!(kernel.modules.is_empty());
            assert_eq!(kernel.thread_last_errors[&0], 0x1234);
        }
        for input in [
            "@Names.DLL,-36",
            "@missing.dll,-35",
            "@Other.DLL,-35",
            "@Names.DLL,0",
        ] {
            put(&mut cpu, 0x1000, input);
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "SHLoadIndirectString",
                    &[0x1000, 0x1000, 260, 0]
                ),
                DispatchOutcome::ReturnedR0(E_FAIL)
            );
            assert_eq!(get(&mut cpu, 0x1000), "");
        }
        assert_eq!(std::fs::read(windows.join("Names.DLL")).unwrap(), file);
    }

    #[test]
    fn shell_name_failures_do_not_overwrite_unwritable_or_reserved_memory() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        for input in [
            "@names.dll",
            "@,-1",
            "@names.dll,-65536",
            "@names.dll,-1;v",
            "@names.dll,-x",
        ] {
            put(&mut cpu, 0x1000, input);
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "SHLoadIndirectString",
                    &[0x1000, 0x1000, 64, 0]
                ),
                DispatchOutcome::ReturnedR0(E_INVALIDARG)
            );
            assert_eq!(get(&mut cpu, 0x1000), "");
        }
        put(&mut cpu, 0x1000, "literal");
        cpu.write_mem(0x2100, &[0xa5; 4]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SHLoadIndirectString",
                &[0x1000, 0x2000, 64, 0x2100]
            ),
            DispatchOutcome::ReturnedR0(E_INVALIDARG)
        );
        assert_eq!(cpu.read_mem(0x2100, 4).unwrap(), [0xa5; 4]);
        cpu.map_region(0x6000, 0x1000, Prot::READ).unwrap();
        for output in [0x6000, 0xffff_fffe] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "SHLoadIndirectString",
                    &[0x1000, output, 64, 0]
                ),
                DispatchOutcome::ReturnedR0(E_POINTER)
            );
        }
        put(&mut cpu, 0x2000, "unchanged");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SHLoadIndirectString",
                &[0x1000, 0x2000, 0, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(get(&mut cpu, 0x2000), "unchanged");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SHLoadIndirectString",
                &[0x6000, 0x2000, 64, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SHLoadIndirectString",
                &[0xffff_fffe, 0x2000, 64, 0]
            ),
            DispatchOutcome::ReturnedR0(E_POINTER)
        );
    }

    #[test]
    fn truncated_string_resources_fail_within_the_declared_file_bounds() {
        let mut file = resource_file(35, "Options");
        file[0x24c..0x250].copy_from_slice(&0x400u32.to_le_bytes());
        assert!(pocket_pe::file_resource(&file, &ResourceKey::Id(6), &ResourceKey::Id(3)).is_err());
        assert!(block_string(&[2, 0, 65, 0], 0).is_err());
        assert!(block_string(&[0, 0, 0xff, 0xff], 1).is_err());
    }

    #[test]
    fn cold_source_and_output_page_failures_retain_the_explicit_ram_fault() {
        struct NoRam;
        impl pocket_cpu::image_pages::ImagePageBudget for NoRam {
            fn acquire(&self) -> bool {
                false
            }
            fn release(&self) {
                panic!("an uncommitted page cannot be refunded");
            }
        }
        for (source, output) in [(0x6000, 0x2000), (0x1000, 0x6000)] {
            let (mut cpu, mut kernel, _) = setup();
            put(&mut cpu, 0x1000, "literal");
            cpu.map_image_region(
                0x6000,
                4096,
                Prot::READ | Prot::WRITE,
                vec![65, 0, 0, 0],
                std::sync::Arc::new(NoRam),
            )
            .unwrap();
            for (reg, value) in [
                (pocket_cpu::regs::ArmReg::R0, source),
                (pocket_cpu::regs::ArmReg::R1, output),
                (pocket_cpu::regs::ArmReg::R2, 64),
                (pocket_cpu::regs::ArmReg::R3, 0),
            ] {
                cpu.write_reg(reg, value).unwrap();
            }
            let thunk = pocket_kernel::Thunk {
                thunk_va: 0x70000000,
                iat_va: 0,
                dll: "coredll.dll".into(),
                binding: pocket_pe::ImportBinding::Name("SHLoadIndirectString".into()),
                friendly_name: Some("SHLoadIndirectString".into()),
            };
            assert!(matches!(
                load(&mut CallCtx {
                    cpu: &mut cpu,
                    kernel: &mut kernel,
                    thunk: &thunk
                }),
                Err(KernelError::Cpu(CpuError::ImageOutOfMemory { va: 0x6000 }))
            ));
        }
    }
}
