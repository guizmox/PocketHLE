//! Modeless CE dialogs share WinCE templates, HWNDs and MFC subclass routing.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
use pocket_pe::ResourceKey;
const PREPARE: &str = "__PocketHLE_MFC_PrepareDialog";
const INITIAL: &str = "__PocketHLE_MFC_InitialDialogProc";
pub(super) const CREATE_INDIRECT: &str = "__PocketHLE_MFC_CreateDialogIndirect";
const PROCEDURE: &str = "__PocketHLE_MFC_DialogProc";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (905, "?Create@CDialog@@QAAHPBGPAVCWnd@@@Z", create_resource),
    (
        947,
        "?CreateIndirect@CDialog@@QAAHPAXPAVCWnd@@@Z",
        create_indirect,
    ),
    (
        948,
        "?CreateIndirect@CDialog@@QAAHPBUDLGTEMPLATE@@PAVCWnd@@PAX@Z",
        create_indirect_init,
    ),
    (2099, "?PreInitDialog@CDialog@@MAAXXZ", pre_init),
    (2013, "?OnSetFont@CDialog@@UAAXPAVCFont@@@Z", set_font),
    (
        2379,
        "?SetOccDialogInfo@CDialog@@MAAHPAU_AFX_OCC_DIALOG_INFO@@@Z",
        set_occ,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", INITIAL, initial);
    d.register_handler("coredll.dll", PREPARE, prepare);
    d.register_handler("coredll.dll", CREATE_INDIRECT, create_in_module);
    d.register_handler("coredll.dll", PROCEDURE, procedure);
}
fn pre_init(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    empty_hook(ctx)
}
fn set_font(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    empty_hook(ctx)
}
fn empty_hook(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(object, 124, Prot::READ)?;
    // Verified retail base virtuals have an empty body; guest overrides are
    // called by the dialog message path, never substituted with these hooks.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn set_occ(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let info = ctx.arg_u32(1)?;
    put(ctx, object + 0x78, info)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn create_resource(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create(ctx, true, false, false)
}
fn create_indirect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create(ctx, false, false, false)
}
fn create_indirect_init(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create(ctx, false, true, false)
}
fn create_in_module(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create(ctx, false, true, true)
}
fn create(
    ctx: &mut CallCtx<'_>,
    resource: bool,
    init: bool,
    instance_argument: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return match f.phase(ctx)? {
            1 if result == 0 => {
                let target = virtual_function(ctx, object, 41)?;
                f.call_args(ctx, 2, target, &[object])
            }
            1 => {
                if !crate::coredll::window_exists(ctx.kernel, result) {
                    return f.finish(ctx, 0);
                }
                if word(ctx, object + 0x2c)? & 0x10 == 0 {
                    return f.call_service(ctx, 2, "DestroyWindow", &[result]);
                }
                let attached = word(ctx, object + 0x20)? == result;
                f.finish(ctx, u32::from(attached))
            }
            2 => f.finish(ctx, 0),
            3 => start_dialog(ctx, f, result),
            _ => Err(KernelError::Dispatch(
                "Invalid MFC dialog creation continuation".into(),
            )),
        };
    }
    ctx.cpu
        .check_guest_access(object, 124, Prot::READ | Prot::WRITE)?;
    if word(ctx, object + 0x20)? != 0 {
        return Err(KernelError::Dispatch(
            "MFC dialog creation on an attached window".into(),
        ));
    }
    let module = super::application::state(ctx)?;
    if word(ctx, module + 0x2054)? != 0 || word(ctx, object + 0x78)? != 0 {
        return Err(KernelError::Dispatch(
            "MFC OLE dialog creation is not implemented".into(),
        ));
    }
    let argument = f.arg(ctx, 1)?;
    let (instance, template) = if resource {
        put(ctx, object + 0x60, argument)?;
        if argument >> 16 == 0 && word(ctx, object + 0x5c)? == 0 {
            put(ctx, object + 0x5c, argument)?;
        }
        let name = crate::coredll::read_wide_resource_key(ctx, argument)?;
        let instance = super::resources::module_for(ctx, &name, &ResourceKey::Id(5))?;
        let Some((entry, base)) = crate::coredll::lookup_resource_in_module(
            ctx.kernel,
            instance,
            &ResourceKey::Id(5),
            &name,
        ) else {
            return f.finish(ctx, 0);
        };
        let template = base
            .checked_add(entry.data_rva)
            .ok_or_else(|| KernelError::Dispatch("MFC dialog resource overflow".into()))?;
        (instance, template)
    } else {
        (
            if instance_argument {
                ctx.arg_u32(4)?
            } else {
                word(ctx, module + 8)?
            },
            argument,
        )
    };
    if crate::coredll::read_dialog_template(ctx, template).is_none() {
        return f.finish(ctx, 0);
    }
    let init = if init { f.arg(ctx, 3)? } else { 0 };
    put(ctx, object + 0x6c, init)?;
    f.set(ctx, 0, instance)?;
    f.set(ctx, 1, template)?;
    let parent = f.arg(ctx, 2)?;
    if parent == 0 {
        return f.call_service(ctx, 3, super::pretranslation::MAIN, &[]);
    }
    start_dialog(ctx, f, parent)
}
fn start_dialog(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    parent: u32,
) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let parent_hwnd = if parent == 0 {
        0
    } else {
        word(ctx, parent + 0x20)?
    };
    let instance = f.get(ctx, 0)?;
    let template = f.get(ctx, 1)?;
    put(ctx, object + 0x34, u32::MAX)?;
    let flags = word(ctx, object + 0x2c)?;
    put(ctx, object + 0x2c, flags | 0x10)?;
    let initial = service(ctx, INITIAL)?;
    f.call_service(
        ctx,
        1,
        "CreateDialogIndirectParamW",
        &[instance, template, parent_hwnd, initial, object],
    )
}

/// USER constructs custom children before WM_INITDIALOG. Associate the parent
/// first, so an outstanding AfxHookWindowCreate cannot attach it to a child.
pub(super) fn preparation(ctx: &CallCtx<'_>, procedure: u32) -> Option<u32> {
    ctx.kernel.dynamic_exports.values().find_map(|exports| {
        (exports.get(INITIAL) == Some(&procedure))
            .then(|| exports.get(PREPARE).copied())
            .flatten()
    })
}
fn prepare(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let hwnd = f.arg(ctx, 0)?;
    let object = f.arg(ctx, 1)?;
    if f.fresh {
        super::window_creation::consume_hook(ctx, object)?;
        return f.call_service(ctx, 1, super::window_handles::SUBCLASS, &[object, hwnd]);
    }
    match f.phase(ctx)? {
        1 => {
            if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
                return Err(KernelError::Dispatch(
                    "Could not attach the MFC dialog".into(),
                ));
            }
            let target = service(ctx, PROCEDURE)?;
            f.call_service(ctx, 2, "SetWindowLongW", &[hwnd, 4, target])
        }
        2 => f.finish(ctx, 1),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC dialog preparation continuation".into(),
        )),
    }
}
fn initial(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let hwnd = f.arg(ctx, 0)?;
    let object = f.arg(ctx, 3)?;
    if f.fresh {
        if f.arg(ctx, 1)? != 0x110 {
            return Err(KernelError::Dispatch(
                "Unexpected initial MFC dialog message".into(),
            ));
        }
        let attached = word(ctx, object + 0x20)? == hwnd
            && super::windows::permanent(ctx, hwnd)? == Some(object);
        if !attached {
            return f.call_service(ctx, 1, PREPARE, &[hwnd, object]);
        }
    } else if f.phase(ctx)? == 2 {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    } else if f.phase(ctx)? != 1 {
        return Err(KernelError::Dispatch(
            "Invalid MFC initial dialog continuation".into(),
        ));
    }
    // The bridge-only creation cookie never becomes WM_INITDIALOG lParam.
    let target = super::windows::procedure(ctx)?;
    let focus = f.arg(ctx, 2)?;
    f.call_args(ctx, 2, target, &[hwnd, 0x110, focus, 0])
}

fn procedure(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    if f.arg(ctx, 1)? != 0x110 {
        return f.finish(ctx, 0);
    }
    let hwnd = f.arg(ctx, 0)?;
    let object = super::windows::wrapper(ctx, hwnd)?;
    let target = virtual_function(ctx, object, 47)?;
    f.call_args(ctx, 1, target, &[object])
}

#[cfg(test)]
mod tests {
    use crate::guest_callbacks::WINCE;
    const INIT: &str = "__PocketHLE_DialogCustomInit";
    use crate::mfc::tests::framework_setup;
    use crate::mfc::window_dispatch::tests::drive_with;
    use pocket_cpu::Cpu;
    #[test]
    fn registered_template_children_exist_before_init_and_use_real_creation_callbacks() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let mut template = Vec::new();
        template.extend(0x10000000u32.to_le_bytes());
        template.extend(0u32.to_le_bytes());
        for v in [1u16, 0, 0, 100, 80, 0, 0, 0] {
            template.extend(v.to_le_bytes());
        }
        while template.len() % 4 != 0 {
            template.push(0);
        }
        template.extend(0x50000000u32.to_le_bytes());
        template.extend(0u32.to_le_bytes());
        for v in [4u16, 4, 80, 12, 1026] {
            template.extend(v.to_le_bytes());
        }
        for s in ["TestRegisteredChild", "Airport"] {
            template.extend(s.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes));
        }
        template.extend(0u16.to_le_bytes());
        cpu.write_mem(0x2000, &template).unwrap();
        k.window_classes.insert(0xdead1000, "#32770".into());
        k.window_styles.insert(0xdead1000, (0x10000000, 0));
        k.window_rects.insert(0xdead1000, (0, 0, 240, 294));
        k.window_class_procs
            .insert("TestRegisteredChild".into(), 0x6100);
        for (i, value) in [0xdead1000, 0x6200, 0x2000, 73].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        let init = pocket_kernel::Thunk {
            thunk_va: 0x700ff000,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name(INIT.into()),
            friendly_name: None,
        };
        let free = k.heap.free_bytes();
        let mut created = 0;
        let mut initialized = 0;
        let result = drive_with(&mut cpu, &mut k, &mut d, init, 1024, |pc, cpu, k, _| {
            let hwnd = cpu.read_arg(0).unwrap();
            let message = cpu.read_arg(1).unwrap();
            if pc == 0x6100 {
                assert_eq!(k.window_ids[&hwnd], 1026);
                if message == 1 {
                    created += 1;
                }
                0
            } else {
                assert_eq!(pc, 0x6200);
                assert_eq!(message, 0x110);
                assert_eq!(cpu.read_arg(3).unwrap(), 73);
                assert_eq!(created, 1);
                let child = *k.window_ids.iter().find(|(_, id)| **id == 1026).unwrap().0;
                assert_eq!(k.window_parents[&child], hwnd);
                assert_eq!(k.window_titles[&child], "Airport");
                initialized += 1;
                1
            }
        });
        assert_eq!((result, initialized), (1, 1));
        assert_eq!(k.heap.free_bytes(), free);
        assert!(!k
            .dynamic_exports
            .values()
            .any(|exports| exports.keys().any(|key| key.starts_with(WINCE.root))));
    }
}
