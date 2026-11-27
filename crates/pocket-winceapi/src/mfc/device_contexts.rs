//! CE3 CDC wrappers own WinCE GDI handles, not a second drawing backend.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
#[derive(Clone, Copy, PartialEq, Eq)]
enum DisposeMode {
    DeleteHandle,
    Destruct,
    DeleteObject,
}
pub(super) const CLEAR_TEMP: &str = "__PocketHLE_MFC_CDC_ClearTemporary";
pub(super) const DETACH: &str = "__PocketHLE_MFC_CDC_Detach";
pub(super) const ATTACH: &str = "__PocketHLE_MFC_CDC_Attach";
pub(super) const DESTRUCT: &str = "__PocketHLE_MFC_CDC_Destruct";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1202, "?FromHandle@CDC@@SAPAV1@PAUHDC__@@@Z", from_handle),
    (1044, "?DeleteTempMap@CDC@@SAXXZ", clear_temporary),
    (76, "??0CDC@@QAA@XZ", construct),
    (230, "??1CDC@@UAA@XZ", destruct),
    (
        1457,
        "?GetRuntimeClass@CDC@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (799, "?Attach@CDC@@QAAHPAUHDC__@@@Z", attach),
    (1059, "?Detach@CDC@@QAAPAUHDC__@@XZ", detach),
    (1036, "?DeleteDC@CDC@@QAAHXZ", delete),
    (2312, "?SetAttribDC@CDC@@UAAXPAUHDC__@@@Z", set_attributes),
    (2387, "?SetOutputDC@CDC@@UAAXPAUHDC__@@@Z", set_output),
    (2166, "?ReleaseAttribDC@CDC@@UAAXXZ", release_attributes),
    (2169, "?ReleaseOutputDC@CDC@@UAAXXZ", release_output),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", ATTACH, attach);
    d.register_handler("coredll.dll", DESTRUCT, destruct);
    d.register_handler("coredll.dll", DETACH, detach);
    d.register_handler("coredll.dll", CLEAR_TEMP, clear_temporary);
}
fn root(ctx: &CallCtx<'_>, handle: u32) -> String {
    super::handle_maps::Kind::Dc.root(ctx, handle)
}
fn from_handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(
        super::handle_maps::from_handle(ctx, super::handle_maps::Kind::Dc, handle)?,
    ))
}
fn clear_temporary(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::handle_maps::clear_temporary(ctx, super::handle_maps::Kind::Dc)
}

fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    construct_base(ctx, object)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
pub(super) fn construct_base(ctx: &mut CallCtx<'_>, object: u32) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 20, pocket_cpu::Prot::WRITE)?;
    let table = super::classes::vtable(ctx, "CDC")?;
    ctx.cpu.write_mem(object, &[0; 20])?;
    put(ctx, object, table)
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CDC",
    )?))
}
fn attach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        return frame.finish(ctx, 1);
    }
    let object = frame.arg(ctx, 0)?;
    let handle = frame.arg(ctx, 1)?;
    let owner = super::application::owner(ctx)?;
    let key = root(ctx, handle);
    if handle == 0
        || word(ctx, object + 4)? != 0
        || ctx.kernel.gdi.dc(handle).is_none()
        || ctx.kernel.dynamic_exports[&owner].contains_key(&key)
    {
        return frame.finish(ctx, 0);
    }
    put(ctx, object + 4, handle)?;
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key, object);
    let target = virtual_function(ctx, object, 5)?;
    frame.call_args(ctx, 1, target, &[object, handle])
}
fn detach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    if !frame.fresh {
        let handle = frame.get(ctx, 0)?;
        put(ctx, object + 4, 0)?;
        return frame.finish(ctx, handle);
    }
    let handle = word(ctx, object + 4)?;
    frame.set(ctx, 0, handle)?;
    let owner = super::application::owner(ctx)?;
    let key = root(ctx, handle);
    if ctx.kernel.dynamic_exports[&owner].get(&key) == Some(&object) {
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .remove(&key);
    }
    let target = virtual_function(ctx, object, 7)?;
    frame.call_args(ctx, 1, target, &[object])
}
fn delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    dispose(ctx, DisposeMode::DeleteHandle)
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    dispose(ctx, DisposeMode::Destruct)
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    dispose(ctx, DisposeMode::DeleteObject)
}
fn finish_dispose(
    ctx: &mut CallCtx<'_>,
    frame: Frame,
    mode: DisposeMode,
    result: u32,
) -> Result<DispatchOutcome, KernelError> {
    if mode == DisposeMode::DeleteHandle {
        return frame.finish(ctx, result);
    }
    let object = frame.arg(ctx, 0)?;
    let table = super::classes::vtable(ctx, "CObject")?;
    put(ctx, object, table)?;
    if mode == DisposeMode::DeleteObject && frame.arg(ctx, 1)? & 1 != 0 {
        ctx.kernel.heap.free(object);
    }
    frame.finish(ctx, object)
}
fn dispose(ctx: &mut CallCtx<'_>, mode: DisposeMode) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    if frame.fresh {
        if mode != DisposeMode::DeleteHandle {
            // Base destruction replaces the derived vtable before Detach.
            let table = super::classes::vtable(ctx, "CDC")?;
            put(ctx, object, table)?;
        }
        if word(ctx, object + 4)? == 0 {
            return finish_dispose(ctx, frame, mode, 0);
        }
        let target = service(ctx, DETACH)?;
        return frame.call_args(ctx, 1, target, &[object]);
    }
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    match frame.phase(ctx)? {
        1 => {
            let target = service(ctx, "DeleteDC")?;
            frame.call_args(ctx, 2, target, &[returned])
        }
        2 => finish_dispose(ctx, frame, mode, returned),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC CDC disposal continuation".into(),
        )),
    }
}
fn set_field(
    ctx: &mut CallCtx<'_>,
    offset: u32,
    release: bool,
) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 20, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let handle = if release { 0 } else { ctx.arg_u32(1)? };
    put(ctx, object + offset, handle)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn set_attributes(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_field(ctx, 8, false)
}
fn set_output(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_field(ctx, 4, false)
}
fn release_attributes(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_field(ctx, 8, true)
}
fn release_output(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_field(ctx, 4, true)
}

#[cfg(test)]
mod tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    use pocket_kernel::DispatchOutcome;
    #[test]
    fn temporary_dc_identity_is_per_thread_and_cleanup_never_deletes_the_borrowed_dc() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        // Initialize shared class/module storage before measuring wrapper ownership.
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:76",
            &[0x2000],
        );
        let dc = k.gdi.create_memory_dc();
        let free = k.heap.free_bytes();
        let mut from = |cpu: &mut pocket_cpu::stub::StubCpu, k: &mut pocket_kernel::KernelState| {
            let DispatchOutcome::ReturnedR0(object) =
                call(cpu, k, &mut d, "mfcce300.dll", "ord:1202", &[dc])
            else {
                panic!("no wrapper")
            };
            object
        };
        let first = from(&mut cpu, &mut k);
        assert_eq!(from(&mut cpu, &mut k), first);
        k.current_thread = 2;
        let other = from(&mut cpu, &mut k);
        assert_ne!(first, other);
        k.current_thread = 0;
        cpu.write_arg(0, 0).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(1044), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert!(k.gdi.dc(dc).is_some());
        assert_eq!(cpu.read_u32_le(other + 4).unwrap(), dc);
        k.current_thread = 2;
        drive(&mut cpu, &mut k, &mut d, entry(1044), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert!(k.gdi.dc(dc).is_some());
        assert_eq!(k.heap.free_bytes(), free);
    }
    #[test]
    fn detach_preserves_the_real_dc_and_destruction_deletes_an_owned_reattached_dc() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:76",
            &[0x2000],
        );
        let dc = k.gdi.create_memory_dc();
        let available = k.heap.free_bytes();
        for (i, value) in [0x2000, dc].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(799), |pc, _| panic!(
                "Unexpected guest {pc:x}"
            )),
            1
        );
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), dc);
        assert_eq!(cpu.read_u32_le(0x2008).unwrap(), dc);
        cpu.write_arg(0, 0x2000).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1059), |pc, _| panic!(
                "Unexpected guest {pc:x}"
            )),
            dc
        );
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2008).unwrap(), 0);
        assert!(k.gdi.dc(dc).is_some());
        for (i, value) in [0x2000, dc].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        drive(&mut cpu, &mut k, &mut d, entry(799), |pc, _| {
            panic!("Unexpected guest {pc:x}")
        });
        cpu.write_arg(0, 0x2000).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(230), |pc, _| {
            panic!("Unexpected guest {pc:x}")
        });
        assert!(k.gdi.dc(dc).is_none());
        assert_eq!(k.heap.free_bytes(), available);
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|name| name.starts_with("__PocketHLE_MFC_CDC:")
                || name.starts_with("__PocketHLE_MFC_Frame:")));
    }
    #[test]
    fn deleting_virtual_destructor_closes_the_dc_before_releasing_the_cpp_object() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        let object = k.heap.alloc_cpp(20).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:76",
            &[object],
        );
        let dc = k.gdi.create_memory_dc();
        cpu.write_arg(0, object).unwrap();
        cpu.write_arg(1, dc).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(799), |pc, _| {
            panic!("Unexpected guest {pc:x}")
        });
        cpu.write_arg(0, object).unwrap();
        cpu.write_arg(1, 1).unwrap();
        let name = "__PocketHLE_MFC_Class:CDC:1";
        let thunk = pocket_kernel::Thunk {
            thunk_va: k.dynamic_exports[&1][name],
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name(name.into()),
            friendly_name: None,
        };
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, thunk, |pc, _| panic!(
                "Unexpected guest {pc:x}"
            )),
            object
        );
        assert_eq!(k.heap.msize(object), None);
        assert!(k.gdi.dc(dc).is_none());
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|name| name.starts_with("__PocketHLE_MFC_CDC:")
                || name.starts_with("__PocketHLE_MFC_Frame:")));
    }
}
