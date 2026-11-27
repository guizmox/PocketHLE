//! CE3 ARM module/application state. Guest allocations own all persistent bytes;
//! the existing export roots preserve them across checkpoints and process exit.
use super::*;
pub(super) const ROOT: &str = "__PocketHLE_MFC_ModuleState";
const PRIVATE: &str = "__PocketHLE_MFC_ApplicationBoundary";
const MODULE_SIZE: u32 = 0x208c;

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2592,
        "?WriteProfileInt@CWinApp@@QAAHPBG0H@Z",
        write_profile_int,
    ),
    (
        2593,
        "?WriteProfileString@CWinApp@@QAAHPBG00@Z",
        write_profile_string,
    ),
    (1423, "?GetProfileInt@CWinApp@@QAAIPBG0H@Z", profile_int),
    (
        1424,
        "?GetProfileString@CWinApp@@QAA?AVCString@@PBG00@Z",
        profile_string,
    ),
    (2400, "?SetRegistryKey@CWinApp@@IAAXPBG@Z", set_registry_key),
    (1635, "?InitInstance@CWinApp@@UAAHXZ", init_instance),
    (609, "?AfxClassInit@@YAXPAUCRuntimeClass@@@Z", class_init),
    (775, "?AfxWinMain@@YAHPAUHINSTANCE__@@0PAGH@Z", win_main),
    (310, "??1CWinApp@@UAA@XZ", destruct),
    (1622, "?InitApplication@CWinApp@@UAAHXZ", init_application),
    (201, "??0CWinApp@@QAA@PBG@Z", construct),
    (
        644,
        "?AfxGetModuleState@@YAPAVAFX_MODULE_STATE@@XZ",
        module_state,
    ),
    (
        645,
        "?AfxGetModuleThreadState@@YAPAVAFX_MODULE_THREAD_STATE@@XZ",
        module_thread_state,
    ),
    (650, "?AfxGetThread@@YAPAVCWinThread@@XZ", current_thread),
    (
        651,
        "?AfxGetThreadState@@YAPAV_AFX_THREAD_STATE@@XZ",
        thread_state,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", PRIVATE, unsupported);
}
fn unsupported(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Err(KernelError::Dispatch(format!(
        "Unsupported MFC application operation: {:?}",
        ctx.thunk.binding
    )))
}
pub(super) fn owner(ctx: &CallCtx<'_>) -> Result<u32, KernelError> {
    ctx.kernel
        .dynamic_exports
        .iter()
        .find_map(|(h, m)| m.contains_key(PRIVATE).then_some(*h))
        .ok_or_else(|| KernelError::Loader("MFC application exports missing".into()))
}
pub(super) fn allocation(
    ctx: &mut CallCtx<'_>,
    key: &str,
    size: u32,
) -> Result<(u32, bool), KernelError> {
    let h = owner(ctx)?;
    if let Some(&p) = ctx.kernel.dynamic_exports[&h].get(key) {
        return Ok((p, false));
    }
    let p = alloc(ctx, size)?;
    if let Err(e) = ctx.cpu.write_mem(p, &vec![0; size as usize]) {
        ctx.kernel.heap.free(p);
        return Err(e.into());
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&h)
        .unwrap()
        .insert(key.into(), p);
    Ok((p, true))
}
pub(super) fn existing_state(ctx: &CallCtx<'_>) -> Result<Option<u32>, KernelError> {
    Ok(ctx.kernel.dynamic_exports[&owner(ctx)?].get(ROOT).copied())
}
pub(super) fn state(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let (p, new) = allocation(ctx, ROOT, MODULE_SIZE + 4)?;
    if new {
        let h = owner(ctx)?;
        let destructor = ctx.kernel.dynamic_exports[&h][PRIVATE];
        put(ctx, p + MODULE_SIZE, destructor)?;
        put(ctx, p, p + MODULE_SIZE)?;
        // AFX_MODULE_STATE's embedded empty lists/maps retain their native
        // element/block sizes. The guest's static init sets its version/DLL flag.
        for (offset, value) in [
            (0x24, 20),
            (0x30, 32),
            (0x38, 1),
            (0x2040, 0x600),
            (0x204c, 0x3c),
            (0x205c, 0x18),
            (0x2064, u32::MAX),
        ] {
            put(ctx, p + offset, value)?;
        }
        put(ctx, p + 8, pocket_kernel::PROCESS_INSTANCE_HANDLE)?;
        put(ctx, p + 12, pocket_kernel::PROCESS_INSTANCE_HANDLE)?;
        let procedure = super::windows::procedure(ctx)?;
        put(ctx, p + 0x203c, procedure)?;
    }
    Ok(p)
}
fn per_thread(ctx: &mut CallCtx<'_>, prefix: &str, size: u32) -> Result<u32, KernelError> {
    let key = format!("{prefix}:{}", ctx.kernel.current_thread);
    let (p, new) = allocation(ctx, &key, size)?;
    if new && size == 0x84 {
        put(ctx, p + 12, 0x6c)?;
    }
    Ok(p)
}
fn module_state(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(state(ctx)?))
}
pub(super) fn module_thread(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let _ = state(ctx)?;
    per_thread(ctx, "__PocketHLE_MFC_ModuleThread", 0x84)
}
fn module_thread_state(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(module_thread(ctx)?))
}

pub(super) fn current_thread_state(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    per_thread(ctx, "__PocketHLE_MFC_Thread", 0x13c)
}
fn thread_state(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(current_thread_state(ctx)?))
}

pub(super) fn thread_object(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let t = module_thread(ctx)?;
    let thread = word(ctx, t + 4)?;
    if thread != 0 {
        Ok(thread)
    } else {
        let module = state(ctx)?;
        word(ctx, module + 4)
    }
}
fn current_thread(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(thread_object(ctx)?))
}

fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let name = ctx.arg_u32(1)?;
    ctx.cpu.check_guest_access(
        object,
        0xb4,
        pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
    )?;
    let module = state(ctx)?;
    let table = super::classes::vtable(ctx, "CWinApp")?;
    let text_pointer = if name != 0 {
        let text = super::strings::wide(ctx, name)?;
        duplicate_wide(ctx, &text)?
    } else {
        0
    };
    ctx.cpu.write_mem(object, &[0; 0xb4])?;
    for (off, v) in [
        (0, table),
        (4, 1),
        (0x14, 1),
        (0x1c, module),
        (0x28, 1),
        (0x2c, pocket_kernel::FAKE_CURRENT_THREAD_HANDLE),
        (0x30, crate::coredll::current_thread_id(ctx)),
        (0x78, text_pointer),
        (0xb0, 512),
    ] {
        put(ctx, object + off, v)?;
    }
    put(ctx, module + 4, object)?;
    let t = per_thread(ctx, "__PocketHLE_MFC_ModuleThread", 0x84)?;
    put(ctx, t + 4, object)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup as setup;
    use super::*;
    use pocket_cpu::Cpu;
    fn invoke(
        cpu: &mut dyn Cpu,
        k: &mut pocket_kernel::KernelState,
        d: &mut WinCeDispatcher,
        name: &str,
    ) -> Result<DispatchOutcome, KernelError> {
        use pocket_kernel::Dispatcher;
        d.dispatch(
            cpu,
            &pocket_kernel::Thunk {
                thunk_va: 0x70000020,
                iat_va: 0,
                dll: "mfcce300.dll".into(),
                binding: pocket_pe::ImportBinding::Name(name.into()),
                friendly_name: None,
            },
            k,
        )
    }
    fn value(outcome: DispatchOutcome) -> u32 {
        let DispatchOutcome::ReturnedR0(v) = outcome else {
            panic!("integer result expected")
        };
        v
    }
    #[test]
    fn module_state_is_stable_writable_and_thread_state_is_isolated() {
        let (mut cpu, mut k, mut d) = setup();
        // Keep the real private window export provided by framework_setup;
        // overriding the boundary marker must not discard the module's services.
        k.dynamic_exports
            .get_mut(&1)
            .unwrap()
            .insert(PRIVATE.into(), 0x70000100);
        let p = value(
            invoke(
                &mut cpu,
                &mut k,
                &mut d,
                "?AfxGetModuleState@@YAPAVAFX_MODULE_STATE@@XZ",
            )
            .unwrap(),
        );
        assert_ne!(p, 0);
        let procedure = k.dynamic_exports[&1]["__PocketHLE_MFC_WindowProc"];
        assert_eq!(cpu.read_u32_le(p + 0x203c).unwrap(), procedure);
        assert_eq!(
            value(
                invoke(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "?AfxGetAfxWndProc@@YAP6AJPAUHWND__@@IIJ@ZXZ"
                )
                .unwrap()
            ),
            procedure
        );
        assert_eq!(cpu.read_u32_le(p + 0x2040).unwrap(), 0x600);
        cpu.write_mem(p + 0x2040, &0x601u32.to_le_bytes()).unwrap();
        assert_eq!(
            value(
                invoke(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "?AfxGetModuleState@@YAPAVAFX_MODULE_STATE@@XZ"
                )
                .unwrap()
            ),
            p
        );
        assert_eq!(cpu.read_u32_le(p + 0x2040).unwrap(), 0x601);
        let t = value(
            invoke(
                &mut cpu,
                &mut k,
                &mut d,
                "?AfxGetModuleThreadState@@YAPAVAFX_MODULE_THREAD_STATE@@XZ",
            )
            .unwrap(),
        );
        cpu.write_mem(t + 4, &0x12345678u32.to_le_bytes()).unwrap();
        assert_eq!(
            value(
                invoke(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "?AfxGetThread@@YAPAVCWinThread@@XZ"
                )
                .unwrap()
            ),
            0x12345678
        );
        k.current_thread = 1;
        let second = value(
            invoke(
                &mut cpu,
                &mut k,
                &mut d,
                "?AfxGetModuleThreadState@@YAPAVAFX_MODULE_THREAD_STATE@@XZ",
            )
            .unwrap(),
        );
        assert_ne!(t, second);
        assert_eq!(cpu.read_u32_le(second + 4).unwrap(), 0);
        assert_eq!(
            value(
                invoke(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "?AfxGetModuleState@@YAPAVAFX_MODULE_STATE@@XZ"
                )
                .unwrap()
            ),
            p
        );
    }
    #[test]
    fn unsupported_mfc_operation_is_an_error_instead_of_a_null_success() {
        let (mut cpu, mut k, mut d) = setup();
        assert!(invoke(
            &mut cpu,
            &mut k,
            &mut d,
            "?UnsupportedFrameworkMethod@@YAXXZ"
        )
        .is_err());
    }
}

pub(super) fn duplicate_wide(ctx: &mut CallCtx<'_>, text: &[u16]) -> Result<u32, KernelError> {
    let size = (text.len() as u32)
        .checked_add(1)
        .and_then(|v| v.checked_mul(2))
        .ok_or_else(|| KernelError::Loader("MFC application text too long".into()))?;
    let p = alloc(ctx, size)?;
    let bytes: Vec<_> = text
        .iter()
        .copied()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    if let Err(e) = ctx.cpu.write_mem(p, &bytes) {
        ctx.kernel.heap.free(p);
        return Err(e.into());
    }
    Ok(p)
}
fn initialize(ctx: &mut CallCtx<'_>, args: [u32; 4]) -> Result<u32, KernelError> {
    let module = state(ctx)?;
    let app = word(ctx, module + 4)?;
    if app == 0 {
        return Err(KernelError::Dispatch(
            "MFC WinMain has no CWinApp instance".into(),
        ));
    }
    put(ctx, module + 8, args[0])?;
    put(ctx, module + 12, args[0])?;
    for (i, v) in args.into_iter().enumerate() {
        put(ctx, app + 0x68 + i as u32 * 4, v)?;
    }
    let path = ctx.kernel.module_path.clone();
    let filename = path.rsplit(['\\', '/']).next().unwrap_or(&path);
    let basename = filename
        .rsplit_once('.')
        .map_or(filename, |(n, _)| n)
        .to_string();
    for (offset, text) in [
        (0x88, basename.clone()),
        (
            0x8c,
            format!(
                "{}{}.HLP",
                path.strip_suffix(filename).unwrap_or(""),
                basename
            ),
        ),
        (0x90, format!("{basename}.INI")),
    ] {
        if word(ctx, app + offset)? == 0 {
            let p = duplicate_wide(ctx, &text.encode_utf16().collect::<Vec<_>>())?;
            put(ctx, app + offset, p)?;
        }
    }
    if word(ctx, app + 0x78)? == 0 {
        let mut title = crate::coredll::resource_string(ctx, args[0], 0xe000);
        if title.is_empty() {
            title = basename.encode_utf16().collect();
        }
        let p = duplicate_wide(ctx, &title)?;
        put(ctx, app + 0x78, p)?;
    }
    let name = word(ctx, app + 0x78)?;
    put(ctx, module + 16, name)?;
    Ok(app)
}
fn init_application(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = super::callbacks::Frame::enter(ctx)?;
    if f.fresh {
        let app = f.arg(ctx, 0)?;
        let manager = super::document_manager::activate(ctx, app)?;
        if manager != 0 {
            return f.call_service(ctx, 1, super::document_manager::ADD, &[manager, 0]);
        }
    }
    f.finish(ctx, 1)
}

fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::{virtual_function, Frame};
    let frame = Frame::enter(ctx)?;
    let app = frame.arg(ctx, 0)?;
    let module = state(ctx)?;
    let recent = word(ctx, app + 0xa4)?;
    if frame.fresh && recent != 0 {
        // The application owns the list through its virtual deleting destructor,
        // including a guest-derived list. Never bypass its override with base cleanup.
        let destructor = virtual_function(ctx, recent, 5)?;
        return frame.call_args(ctx, 1, destructor, &[recent, 1]);
    }
    if !frame.fresh {
        put(ctx, app + 0xa4, 0)?;
    }
    let manager = word(ctx, app + 0x80)?;
    if manager != 0 && (frame.fresh || frame.phase(ctx)? == 1) {
        put(ctx, app + 0x80, 0)?;
        let target = virtual_function(ctx, manager, 1)?;
        return frame.call_args(ctx, 2, target, &[manager, 1]);
    }
    for offset in [0x78, 0x7c, 0x88, 0x8c, 0x90] {
        let p = word(ctx, app + offset)?;
        if p != 0 {
            ctx.kernel.heap.free(p);
            put(ctx, app + offset, 0)?;
        }
    }
    if word(ctx, module + 4)? == app {
        put(ctx, module + 4, 0)?;
        put(ctx, module + 16, 0)?;
    }
    let t = per_thread(ctx, "__PocketHLE_MFC_ModuleThread", 0x84)?;
    if word(ctx, t + 4)? == app {
        put(ctx, t + 4, 0)?;
    }
    frame.finish(ctx, app)
}
fn win_main(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::{virtual_function, Frame};
    let frame = Frame::enter(ctx)?;
    let returned = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
    if frame.fresh {
        let mut args = [0; 4];
        for (i, v) in args.iter_mut().enumerate() {
            *v = frame.arg(ctx, i as u32)?;
        }
        let app = initialize(ctx, args)?;
        frame.set(ctx, 0, app)?;
        let target = virtual_function(ctx, app, 33)?;
        return frame.call(ctx, 1, target, [app, 0, 0, 0]);
    }
    let app = frame.get(ctx, 0)?;
    match frame.phase(ctx)? {
        1 => {
            if returned == 0 {
                return frame.finish(ctx, u32::MAX);
            }
            let f = virtual_function(ctx, app, 21)?;
            frame.call(ctx, 2, f, [app, 0, 0, 0])
        }
        2 => {
            let slot = if returned == 0 { 27 } else { 22 };
            let f = virtual_function(ctx, app, slot)?;
            frame.call(ctx, 3, f, [app, 0, 0, 0])
        }
        3 => frame.finish(ctx, returned),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC WinMain continuation".into(),
        )),
    }
}

fn init_instance(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let app = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(app, 0xb4, pocket_cpu::Prot::READ)?;
    // CE3's base implementation is exactly a TRUE return. The application
    // override is invoked by WinMain and continues its own initialization.
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn class_init(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let class = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(class, 24, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let state = state(ctx)?;
    let head = word(ctx, state + 0x20)?;
    let mut at = head;
    let mut seen = std::collections::HashSet::new();
    while at != 0 {
        if at == class {
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        if !seen.insert(at) {
            return Err(KernelError::Dispatch(
                "Cyclic MFC runtime-class list".into(),
            ));
        }
        at = word(ctx, at + 20)?;
    }
    put(ctx, class + 20, head)?;
    put(ctx, state + 0x20, class)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn set_registry_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let app = ctx.arg_u32(0)?;
    let source = ctx.arg_u32(1)?;
    ctx.cpu
        .check_guest_access(app, 0xb4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let text = super::strings::wide(ctx, source)?;
    let title_pointer = word(ctx, app + 0x78)?;
    let title = super::strings::wide(ctx, title_pointer)?;
    let key = duplicate_wide(ctx, &text)?;
    let profile = match duplicate_wide(ctx, &title) {
        Ok(p) => p,
        Err(e) => {
            ctx.kernel.heap.free(key);
            return Err(e);
        }
    };
    let old_key = word(ctx, app + 0x7c)?;
    let old_profile = word(ctx, app + 0x90)?;
    put(ctx, app + 0x7c, key)?;
    put(ctx, app + 0x90, profile)?;
    if old_key != 0 {
        ctx.kernel.heap.free(old_key);
    }
    if old_profile != 0 {
        ctx.kernel.heap.free(old_profile);
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod ownership_tests {
    use super::super::tests::framework_setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn replacing_profile_strings_and_destroying_the_application_release_owned_buffers() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let bytes: Vec<_> = "Example"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &bytes).unwrap();
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
            &[0x2000, 0x1000],
        );
        let owned = cpu.read_u32_le(0x207c).unwrap();
        // A caller may pass the current key back to SetRegistryKey.
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2400",
            &[0x2000, owned],
        );
        let replacement = cpu.read_u32_le(0x207c).unwrap();
        assert_ne!(replacement, owned);
        assert_eq!(
            cpu.read_mem(replacement, bytes.len() as u32).unwrap(),
            bytes
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
        let outcome = call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:650", &[]);
        assert_eq!(super::super::tests::result(outcome), 0);
    }
}

fn profile_path(
    ctx: &mut CallCtx<'_>,
    app: u32,
    section: u32,
) -> Result<Option<String>, KernelError> {
    ctx.cpu
        .check_guest_access(app, 0xb4, pocket_cpu::Prot::READ)?;
    let key = word(ctx, app + 0x7c)?;
    if key == 0 {
        return Ok(None);
    }
    let profile = word(ctx, app + 0x90)?;
    let key = String::from_utf16_lossy(&super::strings::wide(ctx, key)?);
    let profile = String::from_utf16_lossy(&super::strings::wide(ctx, profile)?);
    let section = String::from_utf16_lossy(&super::strings::wide(ctx, section)?);
    let path = format!("HKCU\\Software\\{key}\\{profile}\\{section}");
    Ok(Some(path))
}
fn profile_section(
    ctx: &mut CallCtx<'_>,
    app: u32,
    section: u32,
) -> Result<Option<String>, KernelError> {
    Ok(profile_path(ctx, app, section)?.filter(|path| ctx.kernel.registry.create_key(path)))
}
/// Write through the shared registry, including CE's null entry/value deletion
/// semantics. Recent-file persistence uses this same owner, never host files.
pub(super) fn write_profile_value(
    ctx: &mut CallCtx<'_>,
    app: u32,
    section: u32,
    entry: Option<&str>,
    value: Option<pocket_kernel::registry::RegistryValue>,
) -> Result<bool, KernelError> {
    let Some(path) = profile_path(ctx, app, section)? else {
        return Ok(false);
    };
    match (entry, value) {
        (None, _) => Ok(ctx
            .kernel
            .registry
            .delete_key(0x80000001, path.strip_prefix("HKCU\\").unwrap())
            .is_ok()),
        (Some(name), None) => {
            if !ctx.kernel.registry.create_key(&path) {
                return Ok(false);
            }
            Ok(ctx.kernel.registry.delete_value(&path, name))
        }
        (Some(name), Some(value)) => Ok(ctx.kernel.registry.create_key(&path)
            && ctx.kernel.registry.set_value(&path, name, value)),
    }
}
fn write_profile_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let app = ctx.arg_u32(0)?;
    let section = ctx.arg_u32(1)?;
    let entry = ctx.arg_u32(2)?;
    let text = ctx.arg_u32(3)?;
    let entry = if entry == 0 {
        None
    } else {
        Some(String::from_utf16_lossy(&super::strings::wide(ctx, entry)?))
    };
    let value = if text == 0 {
        None
    } else {
        Some(pocket_kernel::registry::RegistryValue::Sz(
            String::from_utf16_lossy(&super::strings::wide(ctx, text)?),
        ))
    };
    let result = write_profile_value(ctx, app, section, entry.as_deref(), value)?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(result)))
}
pub(super) fn profile_string_value(
    ctx: &mut CallCtx<'_>,
    app: u32,
    section: u32,
    entry: &str,
) -> Result<Option<Vec<u16>>, KernelError> {
    let value = profile_section(ctx, app, section)?
        .and_then(|path| ctx.kernel.registry.value(&path, entry));
    Ok(match value {
        Some(pocket_kernel::registry::RegistryValue::Sz(s)) => Some(s.encode_utf16().collect()),
        _ => None,
    })
}
fn profile_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let app = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let section = ctx.arg_u32(2)?;
    let entry = ctx.arg_u32(3)?;
    let default = ctx.arg_u32(4)?;
    let entry = String::from_utf16_lossy(&super::strings::wide(ctx, entry)?);
    let text = match profile_string_value(ctx, app, section, &entry)? {
        Some(text) => text,
        None => super::strings::wide(ctx, default)?,
    };
    new_string(ctx, out, &text)?;
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn write_profile_int(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let app = ctx.arg_u32(0)?;
    let section = ctx.arg_u32(1)?;
    let entry = ctx.arg_u32(2)?;
    let value = ctx.arg_u32(3)?;
    let entry = String::from_utf16_lossy(&super::strings::wide(ctx, entry)?);
    let result = write_profile_value(
        ctx,
        app,
        section,
        Some(&entry),
        Some(pocket_kernel::registry::RegistryValue::Dword(value)),
    )?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(result)))
}
fn profile_int(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let app = ctx.arg_u32(0)?;
    let section = ctx.arg_u32(1)?;
    let entry = ctx.arg_u32(2)?;
    let default = ctx.arg_u32(3)?;
    let has_key = word(ctx, app + 0x7c)? != 0;
    let entry = String::from_utf16_lossy(&super::strings::wide(ctx, entry)?);
    let value = profile_section(ctx, app, section)?
        .and_then(|path| ctx.kernel.registry.value(&path, &entry));
    // CE3's registry-only implementation returns zero when no registry key is
    // configured; otherwise an absent/invalid DWORD uses the caller's default.
    let result = match value {
        Some(pocket_kernel::registry::RegistryValue::Dword(n)) => n,
        _ => {
            if has_key {
                default
            } else {
                0
            }
        }
    };
    Ok(DispatchOutcome::ReturnedR0(result))
}

#[cfg(test)]
mod profile_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn profile_reads_use_the_shared_registry_and_caller_defaults() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (p, s) in [
            (0x1000, "App"),
            (0x1100, "Vendor"),
            (0x1200, "Settings"),
            (0x1300, "Value"),
            (0x1400, "fallback"),
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
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1423",
                &[0x2000, 0x1200, 0x1300, 99]
            )),
            99
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1424",
            &[0x2000, 0x2100, 0x1200, 0x1300, 0x1400],
        );
        let p = cpu.read_u32_le(0x2100).unwrap();
        assert_eq!(cpu.read_u32_le(p - 8).unwrap(), 8);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2100],
        );
        let path = r"HKCU\Software\Vendor\App\Settings";
        assert!(k.registry.set_value(
            path,
            "Value",
            pocket_kernel::registry::RegistryValue::Sz("stored".into())
        ));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1424",
            &[0x2000, 0x2100, 0x1200, 0x1300, 0x1400],
        );
        let p = cpu.read_u32_le(0x2100).unwrap();
        assert_eq!(
            cpu.read_mem(p, 14).unwrap(),
            "stored"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2100],
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2592",
                &[0x2000, 0x1200, 0x1300, 42]
            )),
            1
        );
        assert!(matches!(
            k.registry.value(path, "Value"),
            Some(pocket_kernel::registry::RegistryValue::Dword(42))
        ));
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1423",
                &[0x2000, 0x1200, 0x1300, 99]
            )),
            42
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

/// Release only this thread's MFC context after its owned wrappers have gone.
pub(super) fn release_thread_context(ctx: &mut CallCtx<'_>) -> Result<(), KernelError> {
    let owner = owner(ctx)?;
    for prefix in ["__PocketHLE_MFC_ModuleThread", "__PocketHLE_MFC_Thread"] {
        let key = format!("{prefix}:{}", ctx.kernel.current_thread);
        if let Some(p) = ctx
            .kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .remove(&key)
        {
            ctx.kernel.heap.free(p);
        }
    }
    Ok(())
}
