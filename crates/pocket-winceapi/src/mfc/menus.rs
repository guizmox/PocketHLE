//! CMenu wrappers own attached shared MENU handles, independently of toolbars.
use super::*;
pub(super) const CLEAR_TEMP: &str = "__PocketHLE_MFC_Menu_ClearTemporary";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", CLEAR_TEMP, clear_temporary);
}
const PREFIX: &str = "__PocketHLE_MFC_Menu:";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1206,
        "?FromHandle@CMenu@@SAPAV1@PAUHMENU__@@@Z",
        from_handle,
    ),
    (1047, "?DeleteTempMap@CMenu@@SAXXZ", clear_temporary),
    (803, "?Attach@CMenu@@QAAHPAUHMENU__@@@Z", attach),
    (1064, "?Detach@CMenu@@QAAPAUHMENU__@@XZ", detach),
    (1054, "?DestroyMenu@CMenu@@QAAHXZ", destroy),
    (972, "?CreateObject@CMenu@@SAPAVCObject@@XZ", create),
    (
        1497,
        "?GetRuntimeClass@CMenu@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
];
pub(super) fn permanent(ctx: &CallCtx<'_>, handle: u32) -> Result<Option<u32>, KernelError> {
    let owner = super::application::owner(ctx)?;
    Ok(ctx.kernel.dynamic_exports[&owner]
        .get(&format!("{PREFIX}{handle:08x}"))
        .copied())
}
pub(super) fn wrapper(ctx: &mut CallCtx<'_>, handle: u32) -> Result<u32, KernelError> {
    super::handle_maps::from_handle(ctx, super::handle_maps::Kind::Menu, handle)
}
fn from_handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(wrapper(ctx, handle)?))
}
fn clear_temporary(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::handle_maps::clear_temporary(ctx, super::handle_maps::Kind::Menu)
}
pub(super) fn new(ctx: &mut CallCtx<'_>, menu: u32) -> Result<u32, KernelError> {
    let table = super::classes::vtable(ctx, "CMenu")?;
    let o = alloc_cpp(ctx, 8)?;
    let result = (|| {
        put(ctx, o, table)?;
        put(ctx, o + 4, 0)?;
        if menu != 0 && !associate(ctx, o, menu)? {
            return Err(KernelError::Dispatch(
                "MFC CMenu handle already attached".into(),
            ));
        }
        Ok::<_, KernelError>(())
    })();
    if let Err(e) = result {
        ctx.kernel.heap.free(o);
        return Err(e);
    }
    Ok(o)
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(new(ctx, 0)?))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CMenu",
    )?))
}
fn associate(ctx: &mut CallCtx<'_>, o: u32, menu: u32) -> Result<bool, KernelError> {
    ctx.cpu
        .check_guest_access(o, 8, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let owner = super::application::owner(ctx)?;
    let key = format!("{PREFIX}{menu:08x}");
    if menu == 0
        || word(ctx, o + 4)? != 0
        || !ctx.kernel.menu_items.contains_key(&menu)
        || ctx.kernel.dynamic_exports[&owner].contains_key(&key)
    {
        return Ok(false);
    }
    put(ctx, o + 4, menu)?;
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key, o);
    Ok(true)
}
fn attach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let menu = ctx.arg_u32(1)?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(associate(
        ctx, o, menu,
    )?)))
}
pub(super) fn detach_value(ctx: &mut CallCtx<'_>, o: u32) -> Result<u32, KernelError> {
    ctx.cpu
        .check_guest_access(o, 8, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let h = word(ctx, o + 4)?;
    let owner = super::application::owner(ctx)?;
    let key = format!("{PREFIX}{h:08x}");
    if let Some(&other) = ctx.kernel.dynamic_exports[&owner].get(&key) {
        if other != o {
            return Err(KernelError::Dispatch(
                "MFC CMenu belongs to another wrapper".into(),
            ));
        }
    }
    put(ctx, o + 4, 0)?;
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .remove(&key);
    Ok(h)
}
fn detach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(detach_value(ctx, o)?))
}
fn destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let h = detach_value(ctx, o)?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(
        crate::coredll::menu_api::destroy_owned(ctx.kernel, h),
    )))
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    let h = detach_value(ctx, o)?;
    if h != 0 {
        crate::coredll::menu_api::destroy_owned(ctx.kernel, h);
    }
    if flags & 1 != 0 {
        ctx.kernel.heap.free(o);
    }
    Ok(DispatchOutcome::ReturnedR0(o))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn menu_attachment_is_unique_detach_borrows_and_destroy_releases_the_real_tree() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let submenu = crate::coredll::menu_api::alloc(&mut k, Vec::new());
        let root = crate::coredll::menu_api::alloc(
            &mut k,
            vec![pocket_kernel::MenuItem {
                submenu,
                ..Default::default()
            }],
        );
        let wrapper = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:972",
            &[],
        ));
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:803",
                &[wrapper, root]
            )),
            1
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:803",
                &[0x2400, root]
            )),
            0
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1064",
                &[wrapper]
            )),
            root
        );
        assert!(k.menu_items.contains_key(&root));
        assert!(k.menu_items.contains_key(&submenu));
        assert_eq!(cpu.read_u32_le(wrapper + 4).unwrap(), 0);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:803",
            &[wrapper, root],
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1054",
                &[wrapper]
            )),
            1
        );
        assert!(!k.menu_items.contains_key(&root));
        assert!(!k.menu_items.contains_key(&submenu));
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|key| key.starts_with(super::PREFIX)));
        k.heap.free(wrapper);
    }
}
