//! Native 40-byte CCmdUI updates the shared menu, without a second menu model.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const UPDATE: &str = "__PocketHLE_MFC_UpdateCommandUI";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (69, "??0CCmdUI@@QAA@XZ", construct),
    (1092, "?DoUpdate@CCmdUI@@QAAHPAVCCmdTarget@@H@Z", update),
    (1121, "?Enable@CCmdUI@@UAAXH@Z", enable),
    (2326, "?SetCheck@CCmdUI@@UAAXH@Z", check),
    (2395, "?SetRadio@CCmdUI@@UAAXH@Z", radio),
    (2430, "?SetText@CCmdUI@@UAAXPBG@Z", text),
];
const VIRTUALS: &[(&str, crate::Handler)] = &[
    ("__PocketHLE_MFC_CommandUI_Enable", enable),
    ("__PocketHLE_MFC_CommandUI_Check", check),
    ("__PocketHLE_MFC_CommandUI_Radio", radio),
    ("__PocketHLE_MFC_CommandUI_Text", text),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    for &(name, handler) in VIRTUALS {
        d.register_handler("coredll.dll", name, handler);
    }
    d.register_handler(
        "coredll.dll",
        "__PocketHLE_MFC_CommandProbe_Enable",
        probe_enable,
    );
    d.register_handler(
        "coredll.dll",
        "__PocketHLE_MFC_CommandProbe_Decoration",
        probe_decoration,
    );
    d.register_handler("coredll.dll", UPDATE, update);
}
pub(super) fn initialize(ctx: &mut CallCtx<'_>, object: u32) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 40, Prot::READ | Prot::WRITE)?;
    let table = table(ctx, false)?;
    ctx.cpu.write_mem(object, &[0; 40])?;
    put(ctx, object, table)
}
fn table(ctx: &mut CallCtx<'_>, probe: bool) -> Result<u32, KernelError> {
    let owner = super::application::owner(ctx)?;
    let key = if probe {
        "__PocketHLE_MFC_CommandProbe_Table"
    } else {
        "__PocketHLE_MFC_CommandUI_Table"
    };
    if let Some(&table) = ctx.kernel.dynamic_exports[&owner].get(key) {
        return Ok(table);
    }
    let mut methods = [0; 4];
    for (index, &(name, _)) in VIRTUALS.iter().enumerate() {
        let name = if !probe {
            name
        } else if index == 0 {
            "__PocketHLE_MFC_CommandProbe_Enable"
        } else {
            "__PocketHLE_MFC_CommandProbe_Decoration"
        };
        methods[index] = service(ctx, name)?;
    }
    let table = alloc(ctx, 16)?;
    for (index, method) in methods.into_iter().enumerate() {
        put(ctx, table + index as u32 * 4, method)?;
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key.into(), table);
    Ok(table)
}
pub(super) fn initialize_probe(ctx: &mut CallCtx<'_>, object: u32) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 44, Prot::READ | Prot::WRITE)?;
    initialize(ctx, object)?;
    let vtable = table(ctx, true)?;
    put(ctx, object, vtable)?;
    put(ctx, object + 0x28, 1)
}
fn probe_enable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let value = ctx.arg_u32(1)?;
    ctx.cpu.check_guest_access(object, 44, Prot::WRITE)?;
    put(ctx, object + 0x28, value)?;
    put(ctx, object + 0x18, 1)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn probe_decoration(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // The retail CTestCmdUI's three decoration virtuals have empty bodies:
    // this query object only records Enable and must never change live UI.
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    initialize(ctx, object)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn update(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let target = f.arg(ctx, 1)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(object, 40, Prot::READ | Prot::WRITE)?;
        let id = word(ctx, object + 4)?;
        if id == 0 || id & 0xffff == 0xffff {
            return f.finish(ctx, 1);
        }
        put(ctx, object + 0x18, 0)?;
        let method = virtual_function(ctx, target, 5)?;
        return f.call_args(ctx, 1, method, &[target, id, u32::MAX, object, 0]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            f.set(ctx, 0, result)?;
            if f.arg(ctx, 2)? != 0 && word(ctx, object + 0x18)? == 0 {
                put(ctx, f.address + 128, 0)?;
                let id = word(ctx, object + 4)?;
                let method = virtual_function(ctx, target, 5)?;
                return f.call_args(ctx, 2, method, &[target, id, 0, object, f.address + 128]);
            }
            f.finish(ctx, result)
        }
        2 => {
            let method = virtual_function(ctx, object, 0)?;
            f.call_args(ctx, 3, method, &[object, result])
        }
        3 => {
            let value = f.get(ctx, 0)?;
            f.finish(ctx, value)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC command UI update continuation".into(),
        )),
    }
}
fn enable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    menu_state(ctx, true)
}
fn check(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    menu_state(ctx, false)
}
fn menu_state(ctx: &mut CallCtx<'_>, enabling: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if !f.fresh {
        let phase = f.phase(ctx)?;
        if (10..=13).contains(&phase) {
            return window_enable(ctx, f, phase);
        }
        if phase == 20 {
            if ctx.cpu.read_reg(ArmReg::R0)? & 0x2000 != 0 {
                let window = word(ctx, object + 0x14)?;
                let hwnd = word(ctx, window + 0x20)?;
                let value = f.arg(ctx, 1)?;
                return f.call_service(ctx, 1, "SendMessageW", &[hwnd, 0xf1, value, 0]);
            }
            return f.finish(ctx, 0);
        }
        if phase != 1 {
            return Err(KernelError::Dispatch(
                "Invalid MFC command UI setter continuation".into(),
            ));
        }
        if enabling {
            put(ctx, object + 0x18, 1)?;
        }
        return f.finish(ctx, 0);
    }
    ctx.cpu
        .check_guest_access(object, 40, Prot::READ | Prot::WRITE)?;
    let menu = word(ctx, object + 0xc)?;
    if menu == 0 {
        let window = word(ctx, object + 0x14)?;
        if window == 0 {
            return f.finish(ctx, 0);
        }
        let hwnd = word(ctx, window + 0x20)?;
        if enabling {
            if f.arg(ctx, 1)? == 0 {
                return f.call_service(ctx, 10, "GetFocus", &[]);
            }
            let value = f.arg(ctx, 1)?;
            return f.call_service(ctx, 1, super::window_position::ENABLE, &[window, value]);
        }
        return f.call_service(ctx, 20, "SendMessageW", &[hwnd, 0x87, 0, 0]);
    }
    // Native base setters deliberately leave submenu owners unchanged.
    if word(ctx, object + 0x10)? != 0 {
        return f.finish(ctx, 0);
    }
    let handle = word(ctx, menu + 4)?;
    let index = word(ctx, object + 8)?;
    let value = f.arg(ctx, 1)?;
    let (name, flags) = if enabling {
        ("EnableMenuItem", u32::from(value == 0))
    } else {
        ("CheckMenuItem", if value != 0 { 8 } else { 0 })
    };
    f.call_service(ctx, 1, name, &[handle, index, 0x400 | flags])
}
fn radio(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    let value = u32::from(f.arg(ctx, 1)? != 0);
    let method = virtual_function(ctx, object, 1)?;
    f.call_args(ctx, 1, method, &[object, value])
}
fn text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu.check_guest_access(object, 40, Prot::READ)?;
        let menu = word(ctx, object + 0xc)?;
        if menu == 0 {
            let window = word(ctx, object + 0x14)?;
            if window == 0 {
                return f.finish(ctx, 0);
            }
            let hwnd = word(ctx, window + 0x20)?;
            let label = f.arg(ctx, 1)?;
            return f.call_service(ctx, 2, "SetWindowTextW", &[hwnd, label]);
        }
        if word(ctx, object + 0x10)? != 0 {
            return f.finish(ctx, 0);
        }
        let handle = word(ctx, menu + 4)?;
        let index = word(ctx, object + 8)?;
        return f.call_service(ctx, 1, "GetMenuState", &[handle, index, 0x400]);
    }
    if f.phase(ctx)? == 1 {
        let state = ctx.cpu.read_reg(ArmReg::R0)? & 0xfffff6fb;
        let menu = word(ctx, object + 0xc)?;
        let handle = word(ctx, menu + 4)?;
        let index = word(ctx, object + 8)?;
        let id = word(ctx, object + 4)?;
        let label = f.arg(ctx, 1)?;
        return f.call_service(
            ctx,
            2,
            "ModifyMenuW",
            &[handle, index, 0x400 | state, id, label],
        );
    }
    if f.phase(ctx)? != 2 {
        return Err(KernelError::Dispatch(
            "Invalid MFC menu text continuation".into(),
        ));
    }
    f.finish(ctx, 0)
}

/// Native CCmdUI moves focus to the next tab stop before disabling its target.
/// Use USER traversal and actual callbacks, not a second list of MFC controls.
fn window_enable(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    phase: u32,
) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let window = word(ctx, object + 0x14)?;
    let hwnd = word(ctx, window + 0x20)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match phase {
        10 if result == hwnd => f.call_service(ctx, 11, "GetParent", &[hwnd]),
        11 if result != 0 => f.call_service(ctx, 12, "GetNextDlgTabItem", &[result, hwnd, 0]),
        12 if result != 0 => f.call_service(ctx, 13, "SetFocus", &[result]),
        _ => {
            let value = f.arg(ctx, 1)?;
            f.call_service(ctx, 1, super::window_position::ENABLE, &[window, value])
        }
    }
}
