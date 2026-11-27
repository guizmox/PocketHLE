//! CE's base pointer handler shares the cancellable host-input gesture service.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
const GESTURE: &str = "__PocketHLE_MFC_RecognizeGesture";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] =
    &[(2755, "?OnLButtonDown@CWnd@@IAAXIVCPoint@@@Z", left_down)];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", GESTURE, crate::aygshell::gesture::recognize);
}
fn left_down(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let info = f.address + 128;
    if f.fresh {
        let hwnd = word(ctx, object + 0x20)?;
        let x = f.arg(ctx, 2)?;
        let y = f.arg(ctx, 3)?;
        for (i, value) in [20, hwnd, x, y, 1].into_iter().enumerate() {
            put(ctx, info + i as u32 * 4, value)?;
        }
        return f.call_service(ctx, 1, GESTURE, &[info]);
    }
    match f.phase(ctx)? {
        1 if ctx.cpu.read_reg(ArmReg::R0)? == 1000 => {
            put(ctx, info + 16, 2)?;
            f.call_service(ctx, 2, GESTURE, &[info])
        }
        1 => f.call_service(ctx, 2, "__PocketHLE_MFC_WindowDefault", &[object]),
        2 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC pointer gesture continuation".into(),
        )),
    }
}
