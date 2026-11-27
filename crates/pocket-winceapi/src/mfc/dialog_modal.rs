//! Modal dialogs reuse modeless creation, the CE thread pump and HWND owners.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
use pocket_pe::ResourceKey;
const PRE: &str = "__PocketHLE_MFC_PreModal";
const POST: &str = "__PocketHLE_MFC_PostModal";
const END: &str = "__PocketHLE_MFC_EndDialog";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1079, "?DoModal@CDialog@@UAAHXZ", modal),
    (1130, "?EndDialog@CDialog@@QAAXH@Z", end),
    (
        1637,
        "?InitModalIndirect@CDialog@@QAAHPAXPAVCWnd@@@Z",
        init_handle,
    ),
    (
        1638,
        "?InitModalIndirect@CDialog@@QAAHPBUDLGTEMPLATE@@PAVCWnd@@PAX@Z",
        init_template,
    ),
    (1805, "?OnCancel@CDialog@@MAAXXZ", cancel),
    (1969, "?OnOK@CDialog@@MAAXXZ", ok),
    (2100, "?PreModal@CDialog@@IAAPAUHWND__@@XZ", pre),
    (2083, "?PostModal@CDialog@@IAAXXZ", post),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", PRE, pre);
    d.register_handler("coredll.dll", POST, post);
    d.register_handler("coredll.dll", END, end);
}
fn init(ctx: &mut CallCtx<'_>, direct: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 124, Prot::READ | Prot::WRITE)?;
    let data = ctx.arg_u32(1)?;
    let parent = ctx.arg_u32(2)?;
    put(ctx, object + if direct { 0x68 } else { 0x64 }, data)?;
    if direct {
        let records = ctx.arg_u32(3)?;
        put(ctx, object + 0x6c, records)?;
    }
    if word(ctx, object + 0x70)? == 0 {
        put(ctx, object + 0x70, parent)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn init_handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    init(ctx, false)
}
fn init_template(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    init(ctx, true)
}
fn end(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let value = f.arg(ctx, 1)?;
    if f.fresh {
        if word(ctx, object + 0x2c)? & 0x18 != 0 {
            let target = virtual_function(ctx, object, 29)?;
            return f.call_args(ctx, 1, target, &[object, value]);
        }
    } else if f.phase(ctx)? == 2 {
        return f.finish(ctx, 0);
    }
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 2, "EndDialog", &[hwnd, value])
}
fn cancel(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    f.call_service(ctx, 1, END, &[object, 2])
}
fn ok(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::data_exchange::UPDATE, &[object, 1]);
    }
    if f.phase(ctx)? == 1 && ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        return f.call_service(ctx, 2, END, &[object, 1]);
    }
    f.finish(ctx, 0)
}
fn pre(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(object, 124, Prot::READ | Prot::WRITE)?;
        let module = super::application::state(ctx)?;
        let app = word(ctx, module + 4)?;
        if app != 0 {
            return f.call_service(ctx, 1, super::message_box::MODELESS, &[app, 0]);
        }
        return pre_owner(ctx, &f, object);
    }
    match f.phase(ctx)? {
        1 => pre_owner(ctx, &f, object),
        2 => {
            let owner = ctx.cpu.read_reg(ArmReg::R0)?;
            f.set(ctx, 0, owner)?;
            f.call_service(ctx, 3, super::window_creation::HOOK, &[object])
        }
        3 => {
            let owner = f.get(ctx, 0)?;
            f.finish(ctx, owner)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC PreModal continuation".into(),
        )),
    }
}
fn pre_owner(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    object: u32,
) -> Result<DispatchOutcome, KernelError> {
    let parent = word(ctx, object + 0x70)?;
    let hwnd = if parent == 0 {
        0
    } else {
        word(ctx, parent + 0x20)?
    };
    f.call_service(ctx, 2, super::safe_owner::SAFE, &[hwnd, object + 0x74])
}
fn post_app(ctx: &mut CallCtx<'_>, f: Frame, object: u32) -> Result<DispatchOutcome, KernelError> {
    put(ctx, object + 0x74, 0)?;
    let module = super::application::state(ctx)?;
    let app = word(ctx, module + 4)?;
    if app == 0 {
        return f.finish(ctx, 0);
    }
    f.call_service(ctx, 4, super::message_box::MODELESS, &[app, 1])
}
fn post(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::window_creation::UNHOOK, &[]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            super::window_handles::detach_value(ctx, object)?;
            put(ctx, object + 0x40, 0)?;
            let owner = word(ctx, object + 0x74)?;
            f.call_service(ctx, 2, "IsWindow", &[owner])
        }
        2 if result != 0 => {
            let owner = word(ctx, object + 0x74)?;
            f.call_service(ctx, 3, "EnableWindow", &[owner, 1])
        }
        2 | 3 => post_app(ctx, f, object),
        4 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC PostModal continuation".into(),
        )),
    }
}
fn hook_create(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    object: u32,
) -> Result<DispatchOutcome, KernelError> {
    super::exception_links::begin_catch(ctx, f, 160)?;
    f.set(ctx, 5, 1)?;
    f.call_service(ctx, 5, super::window_creation::HOOK, &[object])
}
fn hide(ctx: &mut CallCtx<'_>, f: Frame, object: u32) -> Result<DispatchOutcome, KernelError> {
    let hwnd = word(ctx, object + 0x20)?;
    if hwnd == 0 {
        return bar(ctx, f, object);
    }
    f.call_service(ctx, 9, "SetWindowPos", &[hwnd, 0, 0, 0, 0, 0, 0x97])
}
fn bar(ctx: &mut CallCtx<'_>, f: Frame, object: u32) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 5)? != 0 {
        super::exception_links::unlink(ctx, f.address + 160)?;
        f.set(ctx, 5, 0)?;
    }
    let bar = word(ctx, object + 0x4c)?;
    if bar == 0 {
        return enable_owner(ctx, f, object);
    }
    let target = virtual_function(ctx, bar, 23)?;
    f.call_args(ctx, 10, target, &[bar])
}
fn enable_owner(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    object: u32,
) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 4)? != 0 {
        let owner = f.get(ctx, 3)?;
        return f.call_service(ctx, 12, "EnableWindow", &[owner, 1]);
    }
    active_owner(ctx, f, object)
}
fn active_owner(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    object: u32,
) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 3)? != 0 {
        return f.call_service(ctx, 13, "GetActiveWindow", &[]);
    }
    destroy(ctx, &f, object)
}
fn destroy(ctx: &mut CallCtx<'_>, f: &Frame, object: u32) -> Result<DispatchOutcome, KernelError> {
    let target = virtual_function(ctx, object, 23)?;
    f.call_args(ctx, 15, target, &[object])
}
fn modal(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(object, 124, Prot::READ | Prot::WRITE)?;
        if word(ctx, object + 0x20)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC DoModal requires an unattached dialog".into(),
            ));
        }
        let module = super::application::state(ctx)?;
        if word(ctx, module + 0x2054)? != 0 || word(ctx, object + 0x78)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC OLE modal dialogs are not implemented".into(),
            ));
        }
        let mut instance = word(ctx, module + 12)?;
        let raw = word(ctx, object + 0x60)?;
        let template = if raw != 0 {
            let name = crate::coredll::read_wide_resource_key(ctx, raw)?;
            instance = super::resources::module_for(ctx, &name, &ResourceKey::Id(5))?;
            let Some((entry, base)) = crate::coredll::lookup_resource_in_module(
                ctx.kernel,
                instance,
                &ResourceKey::Id(5),
                &name,
            ) else {
                return f.finish(ctx, u32::MAX);
            };
            base.checked_add(entry.data_rva)
                .ok_or_else(|| KernelError::Dispatch("MFC modal resource overflow".into()))?
        } else {
            let handle = word(ctx, object + 0x64)?;
            if handle != 0 {
                handle
            } else {
                word(ctx, object + 0x68)?
            }
        };
        if crate::coredll::read_dialog_template(ctx, template).is_none() {
            return f.finish(ctx, u32::MAX);
        }
        f.set(ctx, 1, template)?;
        f.set(ctx, 2, instance)?;
        return f.call_service(ctx, 1, PRE, &[object]);
    }
    let phase = f.phase(ctx)?;
    if f.get(ctx, 5)? != 0 && word(ctx, f.address + 212)? != 0 {
        if phase == 30 {
            return bar(ctx, f, object);
        }
        put(ctx, object + 0x34, u32::MAX)?;
        let exception = word(ctx, f.address + 212)?;
        return f.call_service(ctx, 30, super::exception_links::DELETE, &[exception]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match phase {
        1 => {
            f.set(ctx, 3, result)?;
            f.call_service(ctx, 2, super::window_creation::UNHOOK, &[])
        }
        2 => {
            let owner = f.get(ctx, 3)?;
            f.call_service(ctx, 3, "IsWindowEnabled", &[owner])
        }
        3 if result != 0 => {
            f.set(ctx, 4, 1)?;
            let owner = f.get(ctx, 3)?;
            f.call_service(ctx, 4, "EnableWindow", &[owner, 0])
        }
        3 | 4 => hook_create(ctx, &f, object),
        5 => {
            let template = f.get(ctx, 1)?;
            let instance = f.get(ctx, 2)?;
            let owner = f.get(ctx, 3)?;
            let parent = super::windows::wrapper(ctx, owner)?;
            let records = word(ctx, object + 0x6c)?;
            f.call_service(
                ctx,
                6,
                super::dialog_creation::CREATE_INDIRECT,
                &[object, template, parent, records, instance],
            )
        }
        6 if result != 0 && word(ctx, object + 0x2c)? & 0x10 != 0 => {
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 7, "GetWindowLongW", &[hwnd, (-16i32) as u32])
        }
        6 => hide(ctx, f, object),
        7 => f.call_service(
            ctx,
            8,
            super::modal_loop::RUN,
            &[object, 4 | u32::from(result & 0x100 != 0)],
        ),
        8 => hide(ctx, f, object),
        9 => bar(ctx, f, object),
        10 => {
            let bar = word(ctx, object + 0x4c)?;
            if bar == 0 {
                return enable_owner(ctx, f, object);
            }
            let target = virtual_function(ctx, bar, 1)?;
            f.call_args(ctx, 11, target, &[bar, 1])
        }
        11 => {
            put(ctx, object + 0x4c, 0)?;
            enable_owner(ctx, f, object)
        }
        12 => active_owner(ctx, f, object),
        13 => {
            let hwnd = word(ctx, object + 0x20)?;
            if result == hwnd && hwnd != 0 {
                let owner = f.get(ctx, 3)?;
                return f.call_service(ctx, 14, "SetActiveWindow", &[owner]);
            }
            destroy(ctx, &f, object)
        }
        14 => destroy(ctx, &f, object),
        15 => f.call_service(ctx, 16, POST, &[object]),
        16 => {
            let value = word(ctx, object + 0x34)?;
            f.finish(ctx, value)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC DoModal continuation".into(),
        )),
    }
}
