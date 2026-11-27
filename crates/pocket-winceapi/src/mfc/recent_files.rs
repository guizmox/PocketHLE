//! CE3 CRecentFileList is not a CObject. Its six-slot vtable and owned
//! CString array use guest memory; profile reads share the CWinApp registry path.
use super::*;
const ROOT: &str = "__PocketHLE_MFC_RecentFiles:table";
const METHODS: &[&str] = &[
    "__PocketHLE_MFC_RecentFiles:Remove",
    "__PocketHLE_MFC_RecentFiles:Add",
    "__PocketHLE_MFC_RecentFiles:UpdateMenu",
    "__PocketHLE_MFC_RecentFiles:ReadList",
    "__PocketHLE_MFC_RecentFiles:WriteList",
    "__PocketHLE_MFC_RecentFiles:Delete",
];
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2499,
        "?UpdateMenu@CRecentFileList@@UAAXPAVCCmdUI@@@Z",
        update_menu,
    ),
    (
        2052,
        "?OnUpdateRecentFileMenu@CWinApp@@IAAXPAVCCmdUI@@@Z",
        app_update,
    ),
    (
        1271,
        "?GetDisplayName@CRecentFileList@@QBAHAAVCString@@HPBGHH@Z",
        display_name,
    ),
    (561, "?Add@CRecentFileList@@UAAXPBG@Z", add),
    (2589, "?WriteList@CRecentFileList@@UAAXXZ", write_list),
    (599, "?AddToRecentFileList@CWinApp@@UAAXPBG@Z", app_add),
    (
        1723,
        "?LoadStdProfileSettings@CWinApp@@IAAXI@Z",
        load_standard,
    ),
    (155, "??0CRecentFileList@@QAA@IPBG0HH@Z", construct),
    (281, "??1CRecentFileList@@UAA@XZ", destruct),
    (2145, "?ReadList@CRecentFileList@@UAAXXZ", read_list),
    (2171, "?Remove@CRecentFileList@@UAAXH@Z", remove),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in METHODS.iter().zip([
        remove as crate::Handler,
        add,
        update_menu,
        read_list,
        write_list,
        deleting,
    ]) {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn table(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let owner = super::application::owner(ctx)?;
    if let Some(&p) = ctx.kernel.dynamic_exports[&owner].get(ROOT) {
        return Ok(p);
    }
    let methods = METHODS
        .iter()
        .map(|name| {
            ctx.kernel.dynamic_exports[&owner]
                .get(*name)
                .copied()
                .ok_or_else(|| KernelError::Loader("MFC recent-file virtual export missing".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let p = alloc(ctx, 24)?;
    let bytes: Vec<_> = methods.into_iter().flat_map(u32::to_le_bytes).collect();
    if let Err(e) = ctx.cpu.write_mem(p, &bytes) {
        ctx.kernel.heap.free(p);
        return Err(e.into());
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(ROOT.into(), p);
    Ok(p)
}
fn initialize(
    ctx: &mut CallCtx<'_>,
    object: u32,
    start: u32,
    section: &[u16],
    format: &[u16],
    count: u32,
    display: u32,
) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 32, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    if count > 0x100000 || display > i32::MAX as u32 {
        return Err(KernelError::Dispatch(
            "Invalid MFC recent-file capacity".into(),
        ));
    }
    let vtable = table(ctx)?;
    ctx.cpu.write_mem(object, &[0; 32])?;
    let storage = alloc(ctx, (count + 1) * 4)?;
    put(ctx, storage, count)?;
    let mut initialized = Vec::new();
    let result = (|| {
        for field in [object + 12, object + 16, object + 28] {
            new_string(
                ctx,
                field,
                if field == object + 12 {
                    section
                } else if field == object + 16 {
                    format
                } else {
                    &[]
                },
            )?;
            initialized.push(field);
        }
        for i in 0..count {
            let field = storage + 4 + i * 4;
            new_string(ctx, field, &[])?;
            initialized.push(field);
        }
        for (offset, value) in [
            (0, vtable),
            (4, count),
            (8, storage + 4),
            (20, start),
            (24, display),
        ] {
            put(ctx, object + offset, value)?;
        }
        Ok::<_, KernelError>(())
    })();
    if let Err(e) = result {
        for field in initialized.into_iter().rev() {
            release_string(ctx, field)?;
        }
        ctx.kernel.heap.free(storage);
        ctx.cpu.write_mem(object, &[0; 32])?;
        return Err(e);
    }
    Ok(())
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let start = ctx.arg_u32(1)?;
    let sp = ctx.arg_u32(2)?;
    let fp = ctx.arg_u32(3)?;
    let section = super::strings::wide(ctx, sp)?;
    let format = super::strings::wide(ctx, fp)?;
    let count = ctx.arg_u32(4)?;
    let display = ctx.arg_u32(5)?;
    initialize(ctx, o, start, &section, &format, count, display)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn storage(ctx: &mut CallCtx<'_>, object: u32) -> Result<(u32, u32), KernelError> {
    ctx.cpu
        .check_guest_access(object, 32, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let count = word(ctx, object + 4)?;
    let data = word(ctx, object + 8)?;
    if count > 0x100000 || data < 4 || word(ctx, data - 4)? != count {
        return Err(KernelError::Dispatch(
            "Invalid MFC recent-file array".into(),
        ));
    }
    ctx.cpu.check_guest_access(
        data,
        count * 4,
        pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
    )?;
    Ok((data, count))
}
pub(super) fn destroy(ctx: &mut CallCtx<'_>, object: u32) -> Result<(), KernelError> {
    let (data, count) = storage(ctx, object)?;
    for i in 0..count {
        release_string(ctx, data + i * 4)?;
    }
    ctx.kernel.heap.free(data - 4);
    for offset in [28, 16, 12] {
        release_string(ctx, object + offset)?;
    }
    ctx.cpu.write_mem(object + 4, &[0; 28])?;
    Ok(())
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    destroy(ctx, o)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    if flags & !1 != 0 {
        return Err(KernelError::Dispatch(
            "Unsupported MFC recent-list destructor flags".into(),
        ));
    }
    destroy(ctx, o)?;
    if flags & 1 != 0 {
        ctx.kernel.heap.free(o);
    }
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn read_into(ctx: &mut CallCtx<'_>, object: u32, app: u32) -> Result<(), KernelError> {
    let (data, count) = storage(ctx, object)?;
    let section = word(ctx, object + 12)?;
    let format = String::from_utf16_lossy(&super::strings::value(ctx, object + 16)?);
    // Read the real profile values, including empty defaults. The shared printf
    // engine handles the configured integer format without changing guest registers.
    for i in 0..count {
        let entry = crate::coredll::render_printf_values(ctx, &format, true, &[i + 1])?;
        let text = super::application::profile_string_value(ctx, app, section, &entry)?
            .unwrap_or_default();
        super::strings::replace(ctx, data + i * 4, &text)?;
    }
    Ok(())
}
fn read_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let app = current_app(ctx)?;
    read_into(ctx, object, app)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let (data, count) = storage(ctx, object)?;
    if index >= count {
        return Err(KernelError::Dispatch(
            "MFC recent-file index out of bounds".into(),
        ));
    }
    for i in index..count - 1 {
        let next = super::strings::value(ctx, data + (i + 1) * 4)?;
        super::strings::replace(ctx, data + i * 4, &next)?;
    }
    super::strings::replace(ctx, data + (count - 1) * 4, &[])?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn load_standard(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::{virtual_function, Frame};
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let app = f.arg(ctx, 0)?;
    let count = f.arg(ctx, 1)?;
    ctx.cpu
        .check_guest_access(app, 0xb4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    if count == 0 {
        return f.finish(ctx, 0);
    }
    if word(ctx, app + 0xa4)? != 0 {
        return Err(KernelError::Dispatch(
            "MFC recent-file list is already initialized".into(),
        ));
    }
    let object = alloc(ctx, 32)?;
    let section: Vec<_> = "Recent File List".encode_utf16().collect();
    let format: Vec<_> = "File%d".encode_utf16().collect();
    if let Err(e) = initialize(ctx, object, 0, &section, &format, count, 24) {
        ctx.kernel.heap.free(object);
        return Err(e);
    }
    put(ctx, app + 0xa4, object)?;
    let callback = virtual_function(ctx, object, 3)?;
    f.call_args(ctx, 1, callback, &[object])
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::{registry::RegistryValue, DispatchOutcome};
    fn string(cpu: &mut pocket_cpu::stub::StubCpu, object: u32) -> String {
        let p = cpu.read_u32_le(object).unwrap();
        let n = cpu.read_u32_le(p - 8).unwrap();
        String::from_utf16(
            &cpu.read_mem(p, n * 2)
                .unwrap()
                .chunks_exact(2)
                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn standard_profile_reads_real_registry_through_virtual_callback_and_releases_owned_list() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (p, s) in [
            (0x1000, "App"),
            (0x1100, "Vendor"),
            (0x1200, "Recent File List"),
            (0x1300, "File%02d"),
        ] {
            cpu.write_mem(
                p,
                &s.encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        // Warm only permanent class/module/vtable allocations before checking
        // application and vector ownership, including the zero-length vector cookie.
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:201",
            &[0x2000, 0x1000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:310",
            &[0x2000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:155",
            &[0x2200, 17, 0x1200, 0x1300, 0, 24],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:281",
            &[0x2200],
        );
        let baseline = k.heap.free_bytes();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:201",
            &[0x2000, 0x1000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2400",
            &[0x2000, 0x1100],
        );
        let path = r"HKCU\Software\Vendor\App\Recent File List";
        assert!(k.registry.create_key(path));
        for (name, value) in [
            ("File1", r"\My Documents\flight.flt"),
            ("File2", r"\My Documents\second.flt"),
        ] {
            assert!(k
                .registry
                .set_value(path, name, RegistryValue::Sz(value.into())));
        }
        let jump = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1723",
            &[0x2000, 3],
        );
        let list = cpu.read_u32_le(0x20a4).unwrap();
        let table = cpu.read_u32_le(list).unwrap();
        let read = cpu.read_u32_le(table + 12).unwrap();
        assert!(matches!(jump,DispatchOutcome::JumpTo(p) if p==read));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_RecentFiles:ReadList",
            &[list],
        );
        super::super::callbacks::return_from_callback(&mut cpu, &mut k, &mut d);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1723",
                &[]
            )),
            0
        );
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        let cells = cpu.read_u32_le(list + 8).unwrap();
        assert_eq!(cpu.read_u32_le(cells - 4).unwrap(), 3);
        assert_eq!(string(&mut cpu, cells), r"\My Documents\flight.flt");
        assert_eq!(string(&mut cpu, cells + 4), r"\My Documents\second.flt");
        assert_eq!(string(&mut cpu, cells + 8), "");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2171",
            &[list, 0],
        );
        assert_eq!(string(&mut cpu, cells), r"\My Documents\second.flt");
        assert_eq!(string(&mut cpu, cells + 4), "");
        assert_eq!(cpu.read_u32_le(list + 4).unwrap(), 3);
        // A derived list may override destruction. The app must call it, not
        // silently free the base fields. Simulate that guest override calling base.
        let copied = cpu.read_mem(table, 24).unwrap();
        cpu.write_mem(0x2500, &copied).unwrap();
        cpu.write_mem(0x2514, &0x6000u32.to_le_bytes()).unwrap();
        cpu.write_mem(list, &0x2500u32.to_le_bytes()).unwrap();
        let destroy = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:310",
            &[0x2000],
        );
        let deleting = 0x6000;
        assert!(matches!(destroy, DispatchOutcome::JumpTo(p) if p == deleting));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_RecentFiles:Delete",
            &[],
        );
        super::super::callbacks::return_from_callback(&mut cpu, &mut k, &mut d);
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:310", &[]);
        assert_eq!(k.heap.free_bytes(), baseline);
        assert!(!k
            .dynamic_exports
            .values()
            .any(|m| m.keys().any(|n| n.starts_with("__PocketHLE_MFC_Frame:"))));
        // A caller-supplied format uses the same typed printf implementation.
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:201",
            &[0x2000, 0x1000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2400",
            &[0x2000, 0x1100],
        );
        assert!(k
            .registry
            .set_value(path, "File01", RegistryValue::Sz("formatted".into())));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:155",
            &[0x2200, 17, 0x1200, 0x1300, 1, 24],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2145",
            &[0x2200],
        );
        let data = cpu.read_u32_le(0x2208).unwrap();
        assert_eq!(string(&mut cpu, data), "formatted");
        assert_eq!(cpu.read_u32_le(0x2214).unwrap(), 17);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:281",
            &[0x2200],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:310",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

fn current_app(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let state = super::application::state(ctx)?;
    let app = word(ctx, state + 4)?;
    if app == 0 {
        return Err(KernelError::Dispatch(
            "MFC recent files require an application".into(),
        ));
    }
    Ok(app)
}
fn write_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let (data, count) = storage(ctx, object)?;
    let app = current_app(ctx)?;
    let section = word(ctx, object + 12)?;
    let format = String::from_utf16_lossy(&super::strings::value(ctx, object + 16)?);
    // CE3 removes the old section first, so shortening a list cannot leave
    // old FileN entries. Registry limits/errors remain owned by Registry.
    super::application::write_profile_value(ctx, app, section, None, None)?;
    for i in 0..count {
        let text = super::strings::value(ctx, data + i * 4)?;
        if !text.is_empty() {
            let entry = crate::coredll::render_printf_values(ctx, &format, true, &[i + 1])?;
            super::application::write_profile_value(
                ctx,
                app,
                section,
                Some(&entry),
                Some(pocket_kernel::registry::RegistryValue::Sz(
                    String::from_utf16_lossy(&text),
                )),
            )?;
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let path = ctx.arg_u32(1)?;
    let (data, count) = storage(ctx, object)?;
    if count == 0 {
        return Err(KernelError::Dispatch(
            "MFC recent-file list has no capacity".into(),
        ));
    }
    let input = String::from_utf16_lossy(&super::strings::wide(ctx, path)?);
    let path = ctx.kernel.vfs.full_guest_path(&input);
    let text: Vec<_> = path.encode_utf16().collect();
    if text.len() >= 260 {
        return Err(KernelError::Dispatch(
            "MFC recent-file path exceeds MAX_PATH".into(),
        ));
    }
    let mut index = count - 1;
    for i in 0..count - 1 {
        let old = String::from_utf16_lossy(&super::strings::value(ctx, data + i * 4)?);
        if old.eq_ignore_ascii_case(&path) {
            index = i;
            break;
        }
    }
    for i in (1..=index).rev() {
        let previous = super::strings::value(ctx, data + (i - 1) * 4)?;
        super::strings::assign_units(ctx, data + i * 4, &previous)?;
    }
    super::strings::assign_units(ctx, data, &text)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn app_add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = super::callbacks::Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let app = f.arg(ctx, 0)?;
    let path = f.arg(ctx, 1)?;
    let list = word(ctx, app + 0xa4)?;
    if list == 0 {
        return f.finish(ctx, 0);
    }
    let target = super::callbacks::virtual_function(ctx, list, 1)?;
    f.call_args(ctx, 1, target, &[list, path])
}

#[cfg(test)]
mod persistence_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    use pocket_kernel::registry::RegistryValue;
    fn wide(cpu: &mut dyn Cpu, at: u32, s: &str) {
        cpu.write_mem(
            at,
            &s.encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    fn text(cpu: &mut dyn Cpu, field: u32) -> String {
        let p = cpu.read_u32_le(field).unwrap();
        let n = cpu.read_u32_le(p - 8).unwrap();
        String::from_utf16_lossy(
            &cpu.read_mem(p, n * 2)
                .unwrap()
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
    }
    #[test]
    fn recent_paths_deduplicate_persist_and_remove_stale_profile_values() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (at, s) in [
            (0x1000, "App"),
            (0x1100, "Vendor"),
            (0x1200, "Recent"),
            (0x1300, "File%02d"),
        ] {
            wide(&mut cpu, at, s);
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:201",
            &[0x2000, 0x1000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2400",
            &[0x2000, 0x1100],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:155",
            &[0x2200, 0, 0x1200, 0x1300, 3, 24],
        );
        k.vfs.set_default_dir(r"\My Documents");
        for path in [
            r"first.flt",
            r"second.flt",
            r".\FIRST.flt",
            r"sub\..\third.flt",
        ] {
            wide(&mut cpu, 0x1400, path);
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:561",
                &[0x2200, 0x1400],
            );
        }
        let cells = cpu.read_u32_le(0x2208).unwrap();
        assert_eq!(text(&mut cpu, cells), r"\My Documents\third.flt");
        assert_eq!(text(&mut cpu, cells + 4), r"\My Documents\FIRST.flt");
        assert_eq!(text(&mut cpu, cells + 8), r"\My Documents\second.flt");
        let path = r"HKCU\Software\Vendor\App\Recent";
        assert!(k.registry.create_key(path));
        assert!(k
            .registry
            .set_value(path, "File99", RegistryValue::Sz("obsolete".into())));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2589",
            &[0x2200],
        );
        assert_eq!(
            k.registry.value(path, "File01"),
            Some(RegistryValue::Sz(r"\My Documents\third.flt".into()))
        );
        assert_eq!(k.registry.value(path, "File99"), None);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2171",
            &[0x2200, 0],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2145",
            &[0x2200],
        );
        assert_eq!(text(&mut cpu, cells), r"\My Documents\third.flt");
        // NULL text deletes a value, NULL entry deletes the section. An empty
        // string is an ordinary REG_SZ, not one of those deletion requests.
        wide(&mut cpu, 0x1500, "File01");
        wide(&mut cpu, 0x1600, "");
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2593",
                &[0x2000, 0x1200, 0x1500, 0x1600]
            )),
            1
        );
        assert_eq!(
            k.registry.value(path, "File01"),
            Some(RegistryValue::Sz("".into()))
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2593",
                &[0x2000, 0x1200, 0x1500, 0]
            )),
            1
        );
        assert_eq!(k.registry.value(path, "File01"), None);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2593",
                &[0x2000, 0x1200, 0, 0]
            )),
            1
        );
        assert!(!k.registry.contains_key(path));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:281",
            &[0x2200],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:310",
            &[0x2000],
        );
    }
}

// Native AfxAbbreviateName keeps the first directory and a component-aligned
// suffix. It falls back to the file name when there is no room for that form.
fn abbreviate(path: &[u16], max: usize, at_least_name: bool) -> Vec<u16> {
    if path.len() <= max {
        return path.to_vec();
    }
    let slash = u16::from(b'\\');
    let leaf = path.iter().rposition(|&v| v == slash).map_or(0, |i| i + 1);
    let name = &path[leaf..];
    if max < name.len() {
        return if at_least_name { name.to_vec() } else { vec![] };
    }
    let first = path
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(i, &c)| (c == slash).then_some(i))
        .unwrap_or(0);
    if max < first + name.len() + 5 {
        return name.to_vec();
    }
    for i in first..leaf {
        if path[i] == slash && first + path.len() - i + 4 <= max {
            let mut result = path[..first].to_vec();
            result.extend("\\...".encode_utf16());
            result.extend_from_slice(&path[i..]);
            return result;
        }
    }
    name.to_vec()
}
fn display(
    ctx: &mut CallCtx<'_>,
    object: u32,
    index: u32,
    directory: &[u16],
    directory_len: u32,
    at_least: bool,
) -> Result<Option<Vec<u16>>, KernelError> {
    let (data, count) = storage(ctx, object)?;
    if index >= count {
        return Err(KernelError::Dispatch(
            "MFC recent-file display index out of bounds".into(),
        ));
    }
    let path = super::strings::value(ctx, data + index * 4)?;
    if path.is_empty() {
        return Ok(None);
    }
    let start = path
        .iter()
        .rposition(|&u| u == u16::from(b'\\'))
        .map_or(0, |i| i + 1);
    if start == directory_len as usize
        && String::from_utf16_lossy(&path[..start])
            .eq_ignore_ascii_case(&String::from_utf16_lossy(directory))
    {
        return Ok(Some(path[start..].to_vec()));
    }
    let max = word(ctx, object + 24)?;
    Ok(Some(if max == u32::MAX {
        path
    } else {
        abbreviate(&path, max as usize, at_least)
    }))
}
fn display_name(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let index = ctx.arg_u32(2)?;
    let directory = ctx.arg_u32(3)?;
    let directory_len = ctx.arg_u32(4)?;
    let at_least = ctx.arg_u32(5)? != 0;
    super::strings::validate_assignment_target(ctx, out)?;
    let directory = super::strings::wide(ctx, directory)?;
    match display(ctx, object, index, &directory, directory_len, at_least)? {
        None => Ok(DispatchOutcome::ReturnedR0(0)),
        Some(text) => {
            super::strings::assign_units(ctx, out, &text)?;
            Ok(DispatchOutcome::ReturnedR0(1))
        }
    }
}
fn app_update(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = super::callbacks::Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let app = f.arg(ctx, 0)?;
    let ui = f.arg(ctx, 1)?;
    let list = word(ctx, app + 0xa4)?;
    let (target, args) = if list == 0 {
        (super::callbacks::virtual_function(ctx, ui, 0)?, [ui, 0])
    } else {
        (
            super::callbacks::virtual_function(ctx, list, 2)?,
            [list, ui],
        )
    };
    f.call_args(ctx, 1, target, &args)
}
fn menu_call(
    ctx: &mut CallCtx<'_>,
    f: &super::callbacks::Frame,
    phase: u32,
    name: &str,
    args: &[u32],
) -> Result<DispatchOutcome, KernelError> {
    let target = super::callbacks::service(ctx, name)?;
    f.call_args(ctx, phase, target, args)
}
fn update_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = super::callbacks::Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let ui = f.arg(ctx, 1)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(ui, 40, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
        let (data, count) = storage(ctx, object)?;
        if count == 0 {
            return Err(KernelError::Dispatch(
                "MFC recent-file list has no capacity".into(),
            ));
        }
        let wrapper = word(ctx, ui + 12)?;
        let menu = if wrapper == 0 {
            0
        } else {
            word(ctx, wrapper + 4)?
        };
        let id = word(ctx, ui + 4)?;
        if menu != 0 && super::strings::value(ctx, object + 28)?.is_empty() {
            let label = ctx
                .kernel
                .menu_items
                .get(&menu)
                .and_then(|rows| rows.iter().find(|r| r.id == id))
                .map(|r| r.text.clone())
                .unwrap_or_default();
            super::strings::assign_units(ctx, object + 28, &label)?;
        }
        if super::strings::value(ctx, data)?.is_empty() {
            let placeholder = word(ctx, object + 28)?;
            if !super::strings::value(ctx, object + 28)?.is_empty() {
                let target = super::callbacks::virtual_function(ctx, ui, 3)?;
                return f.call_args(ctx, 10, target, &[ui, placeholder]);
            }
            let target = super::callbacks::virtual_function(ctx, ui, 0)?;
            return f.call_args(ctx, 11, target, &[ui, 0]);
        }
        if menu == 0 {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, menu)?;
        f.set(ctx, 1, id)?;
        f.set(ctx, 2, 0)?;
        f.set(ctx, 3, count)?;
        let label = alloc(ctx, 4096)?;
        f.own(ctx, label)?;
        f.set(ctx, 4, label)?;
    } else {
        match f.phase(ctx)? {
            10 => {
                let target = super::callbacks::virtual_function(ctx, ui, 0)?;
                return f.call_args(ctx, 11, target, &[ui, 0]);
            }
            11 => return f.finish(ctx, 0),
            1 => {
                let index = f.get(ctx, 2)?;
                f.set(ctx, 2, index + 1)?;
            }
            2 => {
                let n = f.get(ctx, 2)?;
                f.set(ctx, 2, n + 1)?;
            }
            3 => {
                let count = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
                put(ctx, ui + 32, count)?;
                put(ctx, ui + 24, 1)?;
                return f.finish(ctx, 0);
            }
            _ => {
                return Err(KernelError::Dispatch(
                    "Invalid MFC recent-menu continuation".into(),
                ))
            }
        }
    }
    let menu = f.get(ctx, 0)?;
    let count = f.get(ctx, 3)?;
    let n = f.get(ctx, 2)?;
    if (f.fresh || f.phase(ctx)? == 1) && n < count {
        let id = f.get(ctx, 1)?.wrapping_add(n);
        return menu_call(ctx, &f, 1, "DeleteMenu", &[menu, id, 0]);
    }
    if f.fresh || f.phase(ctx)? == 1 {
        f.set(ctx, 2, 0)?;
    }
    let n = f.get(ctx, 2)?;
    if n < count {
        if let Some(text) = display(ctx, object, n, &[u16::from(b'\\')], 1, true)? {
            let start = word(ctx, object + 20)?.wrapping_add(n).wrapping_add(1) % 10;
            let mut label: Vec<_> = format!("&{start} ").encode_utf16().collect();
            for c in text {
                label.push(c);
                if c == u16::from(b'&') {
                    label.push(c);
                }
            }
            label.push(0);
            if label.len() > 2048 {
                return Err(KernelError::Dispatch(
                    "MFC recent-menu label exceeds its bound".into(),
                ));
            }
            let buffer = f.get(ctx, 4)?;
            ctx.cpu.write_mem(
                buffer,
                &label
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            let index = word(ctx, ui + 8)?;
            let id = word(ctx, ui + 4)?;
            put(ctx, ui + 8, index.wrapping_add(1))?;
            put(ctx, ui + 4, id.wrapping_add(1))?;
            return menu_call(ctx, &f, 2, "InsertMenuW", &[menu, index, 0x400, id, buffer]);
        }
    }
    let index = word(ctx, ui + 8)?.wrapping_sub(1);
    put(ctx, ui + 8, index)?;
    menu_call(ctx, &f, 3, "GetMenuItemCount", &[menu])
}

#[cfg(test)]
mod menu_tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn recent_menu_replaces_only_its_command_range_escapes_mnemonics_and_restores_the_empty_label()
    {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (at, s) in [
            (0x1000, "Recent"),
            (0x1100, "File%d"),
            (0x1200, r"\other.flt"),
            (0x1300, r"\a&b.flt"),
        ] {
            cpu.write_mem(
                at,
                &s.encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:155",
            &[0x2200, 0, 0x1000, 0x1100, 3, 24],
        );
        for path in [0x1200, 0x1300] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:561",
                &[0x2200, path],
            );
        }
        let menu = crate::coredll::menu_api::alloc(
            &mut k,
            vec![
                pocket_kernel::MenuItem {
                    id: 1,
                    text: "Before".encode_utf16().collect(),
                    ..Default::default()
                },
                pocket_kernel::MenuItem {
                    id: 100,
                    text: "Recent".encode_utf16().collect(),
                    ..Default::default()
                },
                pocket_kernel::MenuItem {
                    id: 2,
                    text: "After".encode_utf16().collect(),
                    ..Default::default()
                },
            ],
        );
        let wrapper = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1206",
            &[menu],
        ));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:69",
            &[0x2600],
        );
        for (offset, v) in [(4, 100), (8, 1), (12, wrapper)] {
            cpu.write_mem(0x2600 + offset, &v.to_le_bytes()).unwrap();
        }
        cpu.write_arg(0, 0x2200).unwrap();
        cpu.write_arg(1, 0x2600).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(2499), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        let rows = &k.menu_items[&menu];
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            [1, 100, 101, 2]
        );
        assert_eq!(String::from_utf16_lossy(&rows[1].text), "&1 a&&b.flt");
        assert_eq!(String::from_utf16_lossy(&rows[2].text), "&2 other.flt");
        assert_eq!(cpu.read_u32_le(0x2608).unwrap(), 2);
        assert_eq!(cpu.read_u32_le(0x2620).unwrap(), 4);
        for _ in 0..2 {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2171",
                &[0x2200, 0],
            );
        }
        cpu.write_mem(0x2604, &100u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2608, &1u32.to_le_bytes()).unwrap();
        cpu.write_arg(0, 0x2200).unwrap();
        cpu.write_arg(1, 0x2600).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(2499), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert_eq!(
            String::from_utf16_lossy(&k.menu_items[&menu][1].text),
            "Recent"
        );
        assert_ne!(k.menus[&menu][&100] & 3, 0);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|n| n.starts_with("__PocketHLE_MFC_Frame:")));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:281",
            &[0x2200],
        );
    }
}
