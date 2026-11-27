//! DDX exchanges guest values with real WinCE controls and preserves UI routing.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const UPDATE: &str = "__PocketHLE_MFC_UpdateData";
const ATTACH_SITE: &str = "__PocketHLE_MFC_AttachControlSite";
pub(super) const PREPARE_EDIT: &str = "__PocketHLE_MFC_PrepareEditControl";
pub(super) const PREPARE: &str = "__PocketHLE_MFC_PrepareControl";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (807, "?AttachControlSite@CWnd@@QAAXPAV1@@Z", attach_site),
    (75, "??0CDataExchange@@QAA@PAVCWnd@@H@Z", construct),
    (2495, "?UpdateData@CWnd@@QAAHH@Z", update),
    (
        2101,
        "?PrepareCtrl@CDataExchange@@QAAPAUHWND__@@H@Z",
        prepare,
    ),
    (
        2102,
        "?PrepareEditCtrl@CDataExchange@@QAAPAUHWND__@@H@Z",
        prepare_edit,
    ),
    (1003, "?DDX_Check@@YAXPAVCDataExchange@@HAAH@Z", check),
    (
        1004,
        "?DDX_Control@@YAXPAVCDataExchange@@HAAVCWnd@@@Z",
        control,
    ),
    (
        1074,
        "?DoDataExchange@CWnd@@MAAXPAVCDataExchange@@@Z",
        empty_exchange,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", UPDATE, update);
    d.register_handler("coredll.dll", ATTACH_SITE, attach_site);
    d.register_handler("coredll.dll", PREPARE, prepare);
    d.register_handler("coredll.dll", PREPARE_EDIT, prepare_edit);
}
fn initialize(
    ctx: &mut CallCtx<'_>,
    exchange: u32,
    window: u32,
    save: u32,
) -> Result<(), KernelError> {
    ctx.cpu.check_guest_access(exchange, 12, Prot::WRITE)?;
    for (offset, value) in [(0, save), (4, window), (8, 0)] {
        put(ctx, exchange + offset, value)?;
    }
    Ok(())
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let exchange = ctx.arg_u32(0)?;
    let window = ctx.arg_u32(1)?;
    let save = ctx.arg_u32(2)?;
    initialize(ctx, exchange, window, save)?;
    Ok(DispatchOutcome::ReturnedR0(exchange))
}
fn empty_exchange(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(object, 72, Prot::READ)?;
    // The release CWnd base virtual is empty; UpdateData always executes the
    // object's actual slot 33, so a derived DDX method cannot be skipped.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn update(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let save = f.arg(ctx, 1)?;
        initialize(ctx, f.address + 128, object, save)?;
        let slot = super::application::current_thread_state(ctx)? + 0x120;
        let previous = word(ctx, slot)?;
        f.set(ctx, 0, slot)?;
        f.set(ctx, 1, previous)?;
        let hwnd = word(ctx, object + 0x20)?;
        let target = virtual_function(ctx, object, 33)?;
        put(ctx, slot, hwnd)?;
        // A guest TRY link lives inside the existing snapshot-owned frame.
        super::exception_links::begin_catch(ctx, &f, 160)?;
        // Restore notification suppression if a replacement exception escapes
        // this catch (for example from a guest ReportError override).
        put(ctx, f.address + 144, slot)?;
        put(ctx, f.address + 148, previous)?;
        put(ctx, f.address + 44, f.address + 144)?;
        return f.call_args(ctx, 1, target, &[object, f.address + 128]);
    }
    let exception = word(ctx, f.address + 160 + 52)?;
    match f.phase(ctx)? {
        1 if exception != 0 => {
            let class = super::classes::runtime(ctx, "CUserException")?;
            f.call_service(ctx, 2, super::class_queries::KIND, &[exception, class])
        }
        1 => {
            f.set(ctx, 4, 1)?;
            finish_update(ctx, f)
        }
        2 if ctx.cpu.read_reg(ArmReg::R0)? == 0 => {
            let target = virtual_function(ctx, exception, 6)?;
            f.call_args(ctx, 3, target, &[exception, 0x30, 0xf108])
        }
        2 | 3 => f.call_service(ctx, 4, super::exception_links::DELETE, &[exception]),
        4 => finish_update(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC data exchange continuation".into(),
        )),
    }
}

fn finish_update(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    super::exception_links::unlink(ctx, f.address + 160)?;
    let slot = f.get(ctx, 0)?;
    let previous = f.get(ctx, 1)?;
    put(ctx, slot, previous)?;
    let answer = f.get(ctx, 4)?;
    f.finish(ctx, answer)
}
fn prepare(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    prepare_impl(ctx, false)
}
fn prepare_edit(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    prepare_impl(ctx, true)
}
fn prepare_impl(ctx: &mut CallCtx<'_>, edit: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let exchange = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(exchange, 16, Prot::READ | Prot::WRITE)?;
        let object = word(ctx, exchange + 4)?;
        if word(ctx, object + 0x3c)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC OLE data exchange is not implemented".into(),
            ));
        }
        let hwnd = word(ctx, object + 0x20)?;
        let id = f.arg(ctx, 1)?;
        return f.call_service(ctx, 1, "GetDlgItem", &[hwnd, id]);
    }
    let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
    if hwnd == 0 {
        return Err(KernelError::Dispatch(
            "MFC data exchange control was not found".into(),
        ));
    }
    put(ctx, exchange + 8, hwnd)?;
    put(ctx, exchange + 12, u32::from(edit))?;
    f.finish(ctx, hwnd)
}
fn check(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let exchange = f.arg(ctx, 0)?;
    let value = f.arg(ctx, 2)?;
    if f.fresh {
        let save = word(ctx, exchange)?;
        ctx.cpu.check_guest_access(
            value,
            4,
            if save != 0 {
                Prot::WRITE
            } else {
                Prot::READ | Prot::WRITE
            },
        )?;
        f.set(ctx, 0, save)?;
        let id = f.arg(ctx, 1)?;
        return f.call_service(ctx, 1, PREPARE, &[exchange, id]);
    }
    match f.phase(ctx)? {
        1 => {
            let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
            if f.get(ctx, 0)? != 0 {
                return f.call_service(ctx, 2, "SendMessageW", &[hwnd, 0xf0, 0, 0]);
            }
            let mut state = word(ctx, value)?;
            if state > 2 {
                state = 0;
                put(ctx, value, state)?;
            }
            f.call_service(ctx, 3, "SendMessageW", &[hwnd, 0xf1, state, 0])
        }
        2 => {
            let state = ctx.cpu.read_reg(ArmReg::R0)?;
            put(ctx, value, state)?;
            f.finish(ctx, 0)
        }
        3 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC checkbox exchange continuation".into(),
        )),
    }
}
fn control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 2)?;
    if f.fresh {
        if word(ctx, object + 0x20)? != 0 {
            return f.finish(ctx, 0);
        }
        let exchange = f.arg(ctx, 0)?;
        let id = f.arg(ctx, 1)?;
        return f.call_service(ctx, 1, PREPARE, &[exchange, id]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => f.call_service(ctx, 2, super::window_handles::SUBCLASS, &[object, result]),
        2 if result == 0 => Err(KernelError::Dispatch(
            "MFC DDX control subclass failed".into(),
        )),
        2 => {
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 3, "GetParent", &[hwnd])
        }
        3 => {
            let exchange = f.arg(ctx, 0)?;
            let parent = word(ctx, exchange + 4)?;
            if word(ctx, parent + 0x20)? != result {
                return f.call_service(ctx, 4, ATTACH_SITE, &[object, parent]);
            }
            f.finish(ctx, 0)
        }
        4 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC control exchange continuation".into(),
        )),
    }
}

fn attach_site(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let parent = ctx.arg_u32(1)?;
    ctx.cpu.check_guest_access(object, 72, Prot::READ)?;
    if word(ctx, object + 0x40)? == 0 && parent != 0 && word(ctx, parent + 0x3c)? != 0 {
        return Err(KernelError::Dispatch(
            "MFC OLE control-site attachment is not implemented".into(),
        ));
    }
    // Retail AttachControlSite changes only OLE site links. Ordinary HWND
    // parentage is untouched, including a DDX control in a nested container.
    Ok(DispatchOutcome::ReturnedR0(0))
}
