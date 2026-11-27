//! MFC resource-module search uses strict PE ownership. The shared WinCE APIs
//! retain their broader fallback for callers with imprecise module handles.
use super::*;
use pocket_pe::ResourceKey;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[(
    628,
    "?AfxFindResourceHandle@@YAPAUHINSTANCE__@@PBG0@Z",
    find_resource_module,
)];
fn contains(ctx: &CallCtx<'_>, module: u32, name: &ResourceKey, ty: &ResourceKey) -> bool {
    crate::coredll::lookup_resource_in_module(ctx.kernel, module, ty, name).is_some()
}
fn extension(
    ctx: &mut CallCtx<'_>,
    mut node: u32,
    system: bool,
    name: &ResourceKey,
    ty: &ResourceKey,
) -> Result<Option<u32>, KernelError> {
    let mut seen = std::collections::HashSet::new();
    while node != 0 {
        if !seen.insert(node) {
            return Err(KernelError::Dispatch(
                "Cyclic MFC extension-module list".into(),
            ));
        }
        ctx.cpu
            .check_guest_access(node, 64, pocket_cpu::Prot::READ)?;
        let resource = word(ctx, node + 0x24)?;
        if (word(ctx, node + 0x38)? != 0) == system
            && resource != 0
            && contains(ctx, resource, name, ty)
        {
            return Ok(Some(resource));
        }
        node = word(ctx, node + 0x3c)?;
    }
    Ok(None)
}
fn find_resource_module(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let raw_name = ctx.arg_u32(0)?;
    let raw_type = ctx.arg_u32(1)?;
    let name = crate::coredll::read_wide_resource_key(ctx, raw_name)?;
    let ty = crate::coredll::read_wide_resource_key(ctx, raw_type)?;
    Ok(DispatchOutcome::ReturnedR0(module_for(ctx, &name, &ty)?))
}
pub(super) fn module_for(
    ctx: &mut CallCtx<'_>,
    name: &ResourceKey,
    ty: &ResourceKey,
) -> Result<u32, KernelError> {
    let state = super::application::state(ctx)?;
    let primary = word(ctx, state + 12)?;
    let system = ctx.cpu.read_mem(state + 0x15, 1)?[0] != 0;
    if !system && contains(ctx, primary, name, ty) {
        return Ok(primary);
    }
    let extensions = word(ctx, state + 0x2048)?;
    if let Some(module) = extension(ctx, extensions, false, name, ty)? {
        return Ok(module);
    }
    let language = word(ctx, state + 0x2050)?;
    if language != 0 && contains(ctx, language, name, ty) {
        return Ok(language);
    }
    if system && contains(ctx, primary, name, ty) {
        return Ok(primary);
    }
    if let Some(module) = extension(ctx, extensions, true, name, ty)? {
        return Ok(module);
    }
    // The CE framework returns its primary resource module after a complete
    // miss; a subsequent FindResource/LoadBitmap still observes the real miss.
    Ok(primary)
}

pub(super) fn string(ctx: &mut CallCtx<'_>, id: u32) -> Result<Vec<u16>, KernelError> {
    let id = id & 0xffff;
    let module = module_for(ctx, &ResourceKey::Id((id >> 4) + 1), &ResourceKey::Id(6))?;
    Ok(crate::coredll::resource_string_in_module(ctx, module, id))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{Cpu, Prot};
    use pocket_kernel::{LoadedModule, PROCESS_INSTANCE_HANDLE};
    use pocket_pe::{ResourceEntry, ResourceKey};
    fn entry() -> ResourceEntry {
        ResourceEntry {
            ty: ResourceKey::Id(2),
            name: ResourceKey::Id(130),
            data_rva: 0x100,
            size: 32,
            codepage: 0,
            native_leaves: vec![],
        }
    }
    fn module(handle: u32) -> LoadedModule {
        LoadedModule {
            handle,
            base: handle,
            name: format!("{handle:x}.dll"),
            dependencies: vec![],
            attached: true,
            satellites: vec![],
            resident_regions: vec![],
            guest_path: String::new(),
            image_size: 4096,
            image_entry: 0,
            resources: vec![entry()],
            refcount: 1,
        }
    }
    #[test]
    fn resource_search_honors_primary_extension_language_and_system_order() {
        let (mut cpu, mut k, mut d) = framework_setup();
        k.resources.push(entry());
        k.modules = vec![module(0x30000000), module(0x31000000), module(0x32000000)];
        let state = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:644",
            &[],
        ));
        for (node, handle, system, next) in [
            (0x2500, 0x30000000u32, 0u32, 0x2600u32),
            (0x2600, 0x31000000u32, 1u32, 0u32),
        ] {
            cpu.write_mem(node, &[0; 64]).unwrap();
            for (offset, value) in [(0x24, handle), (0x38, system), (0x3c, next)] {
                cpu.write_mem(node + offset, &value.to_le_bytes()).unwrap();
            }
        }
        cpu.write_mem(state + 0x2048, &0x2500u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(state + 0x2050, &0x32000000u32.to_le_bytes())
            .unwrap();
        let query = |cpu: &mut _, k: &mut _, d: &mut _| {
            result(call(cpu, k, d, "mfcce300.dll", "ord:628", &[130, 2]))
        };
        assert_eq!(query(&mut cpu, &mut k, &mut d), PROCESS_INSTANCE_HANDLE);
        k.resources.clear();
        // Broad WinCE lookup can find an extension, but that must not falsely
        // identify the primary EXE as the resource owner in the MFC search.
        assert_eq!(query(&mut cpu, &mut k, &mut d), 0x30000000);
        cpu.write_mem(0x2538, &1u32.to_le_bytes()).unwrap();
        assert_eq!(query(&mut cpu, &mut k, &mut d), 0x32000000);
        cpu.write_mem(state + 0x15, &[1]).unwrap();
        k.resources.push(entry());
        assert_eq!(query(&mut cpu, &mut k, &mut d), 0x32000000);
        cpu.write_mem(state + 0x2050, &0u32.to_le_bytes()).unwrap();
        assert_eq!(query(&mut cpu, &mut k, &mut d), PROCESS_INSTANCE_HANDLE);
        k.resources.clear();
        assert_eq!(query(&mut cpu, &mut k, &mut d), 0x30000000);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:628",
                &[131, 2]
            )),
            PROCESS_INSTANCE_HANDLE
        );
        // Named identifiers retain the existing case-insensitive PE matching.
        k.modules[0].resources[0].name = ResourceKey::Name("LOGO".into());
        // LOWORD values encode resource IDs, so named keys need a real pointer
        // outside that range, as they do in a loaded ARM image.
        cpu.map_region(0x10000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.write_mem(
            0x10000,
            &"logo"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:628",
                &[0x10000, 2]
            )),
            0x30000000
        );
    }
}

/// Runtime-owned diagnostics when an application/extension supplies no MFC
/// resource. This does not fabricate FindResource/LoadString availability.
pub(super) fn framework_string(ctx: &mut CallCtx<'_>, id: u32) -> Result<Vec<u16>, KernelError> {
    let text = string(ctx, id)?;
    if !text.is_empty() {
        return Ok(text);
    }
    let fallback = match id {
        0xf112 | 0xf113 => "Enter a value between %1 and %2.",
        0xf114 => "Enter no more than %1 characters.",
        0xf108 => "The value could not be read from the control.",
        0xf109 => "The command could not be completed.",
        0xf003 => "Untitled",
        0xf004 => "Save As",
        0xf103 => "Save changes to %1?",
        0xf006 => "an unnamed file",
        0xf1a0 | 0xf1b0 => "No error occurred in %1.",
        0xf1a1 => "An unspecified error occurred while accessing %1.",
        0xf1a2 => "%1 was not found.",
        0xf1a3 => "The path for %1 was not found.",
        0xf1a4 => "Too many files are open to access %1.",
        0xf1a5 => "Access to %1 was denied.",
        0xf1a6 => "%1 is not a valid file.",
        0xf1a7 => "%1 could not be removed.",
        0xf1a8 => "The directory for %1 is full.",
        0xf1a9 => "The seek in %1 failed.",
        0xf1aa => "A hardware I/O error occurred while accessing %1.",
        0xf1ab => "%1 is being used by another process.",
        0xf1ac => "The requested region of %1 is locked.",
        0xf1ad => "The disk is full while writing %1.",
        0xf1ae => "The end of %1 was reached unexpectedly.",
        0xf1b1 => "An unspecified archive error occurred in %1.",
        0xf1b2 => "An attempt was made to read a storing archive: %1.",
        0xf1b3 => "The end of archive %1 was reached unexpectedly.",
        0xf1b4 => "An attempt was made to write a loading archive: %1.",
        0xf1b5 => "Archive %1 contains an invalid object or reference.",
        0xf1b6 => "Archive %1 contains an unknown runtime class.",
        0xf1b7 => "Archive %1 has an incompatible schema.",
        0xf020 => "An application error occurred.",
        0xf022 => "A required system resource is unavailable.",
        0xf023 => "There is not enough memory to complete this operation.",
        0xf024 => "The operation was cancelled.",
        _ => return Ok(text),
    };
    Ok(fallback.encode_utf16().collect())
}
