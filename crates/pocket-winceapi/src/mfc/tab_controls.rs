//! Tab notification and image-list detachment use the shared WinCE tab owner.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1673, "?InsertItem@CTabCtrl@@QAAHIHPBGHJ@Z", insert),
    (1674, "?InsertItem@CTabCtrl@@QAAHIHPBGHJKK@Z", insert_state),
    (
        1822,
        "?OnChildNotify@CTabCtrl@@MAAHIIJPAJ@Z",
        super::control_notifications::owner_draw_notify,
    ),
    (1868, "?OnDestroy@CTabCtrl@@IAAXXZ", destroy),
    (
        1112,
        "?DrawItem@CTabCtrl@@UAAXPAUtagDRAWITEMSTRUCT@@@Z",
        draw_item,
    ),
];
fn draw_item(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // The retail base virtual is empty; owner-drawn subclasses override slot 46.
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, "SendMessageW", &[hwnd, 0x1302, 0, 0]);
    }
    if f.phase(ctx)? == 1 && ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        // TCM_SETIMAGELIST returns the borrowed old list. Detach it; do not
        // delete it when destroying the tab, which never owned that resource.
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 2, "SendMessageW", &[hwnd, 0x1303, 0, 0]);
    }
    if matches!(f.phase(ctx)?, 1 | 2) {
        return f.call_service(ctx, 3, super::window_lifetime::ON_DESTROY, &[object]);
    }
    if f.phase(ctx)? != 3 {
        return Err(KernelError::Dispatch(
            "Invalid MFC tab destruction continuation".into(),
        ));
    }
    f.finish(ctx, 0)
}

fn insert(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    insert_impl(ctx, false)
}
fn insert_state(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    insert_impl(ctx, true)
}
fn insert_impl(ctx: &mut CallCtx<'_>, state: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        if f.phase(ctx)? != 1 {
            return Err(KernelError::Dispatch(
                "Invalid MFC tab insertion continuation".into(),
            ));
        }
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    let object = f.arg(ctx, 0)?;
    let mask = f.arg(ctx, 1)?;
    let index = f.arg(ctx, 2)?;
    let text = f.arg(ctx, 3)?;
    let image = ctx.arg_u32(4)?;
    let data = ctx.arg_u32(5)?;
    let (state, state_mask) = if state {
        (ctx.arg_u32(6)?, ctx.arg_u32(7)?)
    } else {
        (0, 0)
    };
    let item = f.address + 128;
    for (i, value) in [mask, state, state_mask, text, 0, image, data]
        .into_iter()
        .enumerate()
    {
        put(ctx, item + i as u32 * 4, value)?;
    }
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 1, "SendMessageW", &[hwnd, 0x133e, index, item])
}
