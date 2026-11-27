//! CE dialog initialization executes real virtuals, DDX and shell services.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
const OWNED_BAR: &str = "__PocketHLE_MFC_DialogBar:";
use super::dialog_records::{resource_data, DIRECT, EXECUTE};
const NEW_BAR: &str = "__PocketHLE_MFC_NewDialogBar";
const SHELL_INIT: &str = "__PocketHLE_MFC_InitShellDialog";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1858,
        "?OnCtlColor@CDialog@@IAAPAUHBRUSH__@@PAVCDC@@PAVCWnd@@I@Z",
        super::control_notifications::color,
    ),
    (1600, "?HandleInitDialog@CDialog@@IAAJIJ@Z", handle_init),
    (1939, "?OnInitDialog@CDialog@@UAAHXZ", on_init),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", NEW_BAR, super::control_bars::new_command);
    d.register_handler("coredll.dll", SHELL_INIT, crate::aygshell::sh_init_dialog);
}
fn handle_init(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let module = super::application::state(ctx)?;
        if word(ctx, module + 0x2054)? != 0 || word(ctx, object + 0x78)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC OLE dialog initialization is not implemented".into(),
            ));
        }
        let target = virtual_function(ctx, object, 51)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    match f.phase(ctx)? {
        1 if word(ctx, object + 0x2c)? & 0x10 != 0 => {
            let bar = alloc_cpp(ctx, 224)?;
            f.set(ctx, 0, bar)?;
            let owner = super::application::owner(ctx)?;
            ctx.kernel
                .dynamic_exports
                .get_mut(&owner)
                .unwrap()
                .insert(format!("{OWNED_BAR}{object:08x}"), bar);
            put(ctx, object + 0x4c, bar)?;
            f.call_service(ctx, 2, NEW_BAR, &[bar])
        }
        1 => f.call_service(ctx, 4, "__PocketHLE_MFC_WindowDefault", &[object]),
        2 => {
            let bar = f.get(ctx, 0)?;
            let target = virtual_function(ctx, bar, 56)?;
            f.call_args(
                ctx,
                3,
                target,
                &[bar, object, 0x800, 0x50002000, 0, 0, 0, 0, 0xe800],
            )
        }
        3 => f.call_service(ctx, 4, "__PocketHLE_MFC_WindowDefault", &[object]),
        4 => {
            let result = ctx.cpu.read_reg(ArmReg::R0)?;
            if result != 0 && word(ctx, object + 0x2c)? & 0x100 != 0 {
                f.set(ctx, 1, result)?;
                let hwnd = word(ctx, object + 0x20)?;
                return f.call_service(ctx, 5, "GetNextDlgTabItem", &[hwnd, 0, 0]);
            }
            f.finish(ctx, result)
        }
        5 => {
            let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
            if let Some(window) = super::windows::permanent(ctx, hwnd)? {
                return f.call_service(ctx, 6, super::window_position::FOCUS, &[window]);
            }
            let result = f.get(ctx, 1)?;
            f.finish(ctx, result)
        }
        6 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC dialog initializer continuation".into(),
        )),
    }
}
fn on_init(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let data = word(ctx, object + 0x6c)?;
        if data != 0 {
            return f.call_service(ctx, 1, DIRECT, &[object, data]);
        }
        let raw = word(ctx, object + 0x60)?;
        let (data, size) = resource_data(ctx, raw)?;
        if data == 0 {
            return f.call_service(ctx, 1, DIRECT, &[object, 0]);
        }
        return f.call_service(ctx, 1, EXECUTE, &[object, data, size]);
    }
    match f.phase(ctx)? {
        1 => {
            if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
                return Err(KernelError::Dispatch(
                    "MFC dialog initialization records failed".into(),
                ));
            }
            f.call_service(ctx, 2, super::data_exchange::UPDATE, &[object, 0])
        }
        2 => {
            if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
                return Err(KernelError::Dispatch(
                    "MFC initial data exchange failed".into(),
                ));
            }
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 3, "GetDlgItem", &[hwnd, 0xe146])
        }
        3 => {
            let help = ctx.cpu.read_reg(ArmReg::R0)?;
            if help != 0 {
                f.set(ctx, 0, help)?;
                return f.call_service(ctx, 5, super::dialog_help::ENABLED, &[]);
            }
            shell_init(ctx, f, object)
        }
        4 => f.finish(ctx, 1),
        5 => {
            let show = if ctx.cpu.read_reg(ArmReg::R0)? != 0 {
                5
            } else {
                0
            };
            let help = f.get(ctx, 0)?;
            f.call_service(ctx, 6, "ShowWindow", &[help, show])
        }
        6 => shell_init(ctx, f, object),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC OnInitDialog continuation".into(),
        )),
    }
}
fn shell_init(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    object: u32,
) -> Result<DispatchOutcome, KernelError> {
    if word(ctx, object + 0x48)? == 0 {
        return f.finish(ctx, 1);
    }
    let info = f.address + 128;
    let hwnd = word(ctx, object + 0x20)?;
    for (offset, value) in [(0, 1), (4, hwnd), (8, 13)] {
        put(ctx, info + offset, value)?;
    }
    f.call_service(ctx, 4, SHELL_INIT, &[info])
}

pub(super) fn take_owned_bar(
    ctx: &mut CallCtx<'_>,
    dialog: u32,
) -> Result<Option<u32>, KernelError> {
    let owner = super::application::owner(ctx)?;
    Ok(ctx
        .kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .remove(&format!("{OWNED_BAR}{dialog:08x}")))
}
pub(super) fn forget_owned_bar(ctx: &mut CallCtx<'_>, bar: u32) -> Result<(), KernelError> {
    let owner = super::application::owner(ctx)?;
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .retain(|name, value| !name.starts_with(OWNED_BAR) || *value != bar);
    Ok(())
}
