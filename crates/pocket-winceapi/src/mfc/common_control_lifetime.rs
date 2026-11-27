//! Derived CWnd destruction keeps its own vtable during shared HWND teardown.
use super::callbacks::{service, Frame};
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (254, "??1CListBox@@UAA@XZ", listbox_destruct),
    (
        1485,
        "?GetRuntimeClass@CListBox@@UBAPAUCRuntimeClass@@XZ",
        listbox_runtime,
    ),
    (293, "??1CStatic@@UAA@XZ", static_destruct),
    (239, "??1CEdit@@UAA@XZ", edit_destruct),
    (255, "??1CListCtrl@@UAA@XZ", list_destruct),
    (
        1524,
        "?GetRuntimeClass@CStatic@@UBAPAUCRuntimeClass@@XZ",
        static_runtime,
    ),
    (
        1465,
        "?GetRuntimeClass@CEdit@@UBAPAUCRuntimeClass@@XZ",
        edit_runtime,
    ),
    (217, "??1CButton@@UAA@XZ", button_destruct),
    (
        1443,
        "?GetRuntimeClass@CButton@@UBAPAUCRuntimeClass@@XZ",
        button_runtime,
    ),
    (301, "??1CTabCtrl@@UAA@XZ", tab_destruct),
    (
        1531,
        "?GetRuntimeClass@CTabCtrl@@UBAPAUCRuntimeClass@@XZ",
        tab_runtime,
    ),
    (276, "??1CProgressCtrl@@UAA@XZ", progress_destruct),
    (288, "??1CSliderCtrl@@UAA@XZ", slider_destruct),
    (
        1507,
        "?GetRuntimeClass@CProgressCtrl@@UBAPAUCRuntimeClass@@XZ",
        progress_runtime,
    ),
    (
        1519,
        "?GetRuntimeClass@CSliderCtrl@@UBAPAUCRuntimeClass@@XZ",
        slider_runtime,
    ),
];
fn tab_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CTabCtrl",
    )?))
}
fn tab_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CTabCtrl", false)
}
fn progress_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CProgressCtrl",
    )?))
}
fn slider_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CSliderCtrl",
    )?))
}
fn progress_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CProgressCtrl", false)
}
fn slider_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CSliderCtrl", false)
}
pub(super) fn destruct(
    ctx: &mut CallCtx<'_>,
    class: &str,
    deleting: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu.check_guest_access(
            object,
            super::classes::object_size(class)?,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )?;
        let flags = f.arg(ctx, 1)?;
        if deleting && flags & 2 != 0 {
            return Err(KernelError::Dispatch(
                "MFC vector destruction is not implemented".into(),
            ));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let table = super::classes::vtable(ctx, class)?;
        put(ctx, object, table)?;
        let target = service(ctx, super::window_lifetime::DESTROY)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    if f.phase(ctx)? == 1 && class == "CDialog" {
        if let Some(bar) = super::dialog_init::take_owned_bar(ctx, object)? {
            put(ctx, object + 0x4c, 0)?;
            let target = super::callbacks::virtual_function(ctx, bar, 1)?;
            return f.call_args(ctx, 3, target, &[bar, 1]);
        }
    }
    if matches!(f.phase(ctx)?, 1 | 3) {
        let target = service(ctx, super::window_lifetime::DESTRUCT)?;
        return f.call_args(ctx, 2, target, &[object]);
    }
    if f.phase(ctx)? != 2 {
        return Err(KernelError::Dispatch(
            "Invalid MFC derived-window destruction continuation".into(),
        ));
    }
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(object);
    }
    f.finish(ctx, object)
}

fn button_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CButton",
    )?))
}
fn button_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CButton", false)
}

fn static_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CStatic", false)
}
fn edit_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CEdit", false)
}
fn list_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CListCtrl", false)
}
fn static_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CStatic",
    )?))
}
fn edit_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CEdit",
    )?))
}

fn listbox_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CListBox", false)
}
fn listbox_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CListBox",
    )?))
}
