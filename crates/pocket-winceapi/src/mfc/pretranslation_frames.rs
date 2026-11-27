//! Frame keyboard accelerators and dropdown cancellation use shared WinCE APIs.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const CANCEL: &str = "__PocketHLE_MFC_CancelModes";
const COMBO: &str = "__PocketHLE_MFC_ComboStyle";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2107,
        "?PreTranslateMessage@CFrameWnd@@UAAHPAUtagMSG@@@Z",
        translate,
    ),
    (607, "?AfxCancelModes@@YAXPAUHWND__@@@Z", cancel),
    (
        1261,
        "?GetDefaultAccelerator@CFrameWnd@@UAAPAUHACCEL__@@XZ",
        accelerator,
    ),
    (
        1260,
        "?GetDefaultAccelerator@CDocument@@UAAPAUHACCEL__@@XZ",
        document_accelerator,
    ),
    (
        1211,
        "?GetActiveDocument@CFrameWnd@@UAAPAVCDocument@@XZ",
        active_document,
    ),
    (
        1216,
        "?GetActiveView@CFrameWnd@@QBAPAVCView@@XZ",
        active_view,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", CANCEL, cancel);
    d.register_handler("coredll.dll", COMBO, combo);
}
fn active_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(word(ctx, frame + 0xac)?))
}
fn active_document(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = ctx.arg_u32(0)?;
    let view = word(ctx, frame + 0xac)?;
    Ok(DispatchOutcome::ReturnedR0(if view == 0 {
        0
    } else {
        word(ctx, view + 0x48)?
    }))
}
fn document_accelerator(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // The retail base document has no default table. Derived slot 43 is still
    // called by the frame and may return its own actual accelerator resource.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn accelerator(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let handle = word(ctx, object + 0x60)?;
        f.set(ctx, 0, handle)?;
        let target = virtual_function(ctx, object, 47)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    match f.phase(ctx)? {
        1 if value != 0 => {
            let target = virtual_function(ctx, value, 43)?;
            f.call_args(ctx, 2, target, &[value])
        }
        1 | 2 => {
            let fallback = f.get(ctx, 0)?;
            let document_result = f.phase(ctx)? == 2 && value != 0;
            f.finish(ctx, if document_result { value } else { fallback })
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC accelerator selection continuation".into(),
        )),
    }
}
fn translate_next(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    let message = f.arg(ctx, 1)?;
    let kind = word(ctx, message + 4)?;
    // The nonvirtual CE CWnd base pretranslator is FALSE. The actual frame
    // virtual was already selected by WalkPreTranslateTree before this method.
    if !(0x100..=0x108).contains(&kind) {
        return f.finish(ctx, 0);
    }
    let target = virtual_function(ctx, object, 57)?;
    f.call_args(ctx, 2, target, &[object])
}
fn translate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let message = f.arg(ctx, 1)?;
        if word(ctx, message + 4)? == 0x201 {
            let hwnd = word(ctx, message)?;
            return f.call_service(ctx, 1, CANCEL, &[hwnd]);
        }
        return translate_next(ctx, f);
    }
    match f.phase(ctx)? {
        1 => translate_next(ctx, f),
        2 if value == 0 => f.finish(ctx, 0),
        2 => {
            let object = f.arg(ctx, 0)?;
            let message = f.arg(ctx, 1)?;
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 3, "TranslateAcceleratorW", &[hwnd, value, message])
        }
        3 => f.finish(ctx, u32::from(value != 0)),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame pretranslation continuation".into(),
        )),
    }
}
fn combo(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let hwnd = f.arg(ctx, 0)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        if hwnd == 0 {
            return f.finish(ctx, 0);
        }
        return f.call_service(ctx, 1, "GetWindowLongW", &[hwnd, (-16i32) as u32]);
    }
    match f.phase(ctx)? {
        1 if value & 0xf != f.arg(ctx, 1)? => f.finish(ctx, 0),
        1 => {
            let buffer = f.address + 128;
            f.call_service(ctx, 2, "GetClassNameW", &[hwnd, buffer, 10])
        }
        2 => {
            let text = crate::coredll::read_wstr(ctx, f.address + 128, 10)?;
            let text = String::from_utf16_lossy(&text);
            f.finish(
                ctx,
                u32::from(text.trim_end_matches('\0').eq_ignore_ascii_case("combobox")),
            )
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC combo-style continuation".into(),
        )),
    }
}
fn close_combo(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let hwnd = f.get(ctx, 0)?;
    f.call_service(ctx, 7, "SendMessageW", &[hwnd, 0x14f, 0, 0])
}
fn check_recipient(ctx: &mut CallCtx<'_>, f: &Frame) -> Result<DispatchOutcome, KernelError> {
    let recipient = f.arg(ctx, 0)?;
    if recipient == 0 {
        return close_combo(ctx, f);
    }
    f.call_service(ctx, 5, "GetWindowLongW", &[recipient, (-16i32) as u32])
}
fn cancel(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    let recipient = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, "GetFocus", &[]);
    }
    match f.phase(ctx)? {
        1 if value == 0 || value == recipient => f.finish(ctx, 0),
        1 => {
            f.set(ctx, 0, value)?;
            f.call_service(ctx, 2, COMBO, &[value, 3])
        }
        2 if value != 0 => check_recipient(ctx, &f),
        2 => {
            let focus = f.get(ctx, 0)?;
            f.call_service(ctx, 3, "GetParent", &[focus])
        }
        3 if value == recipient => f.finish(ctx, 0),
        3 => {
            f.set(ctx, 0, value)?;
            f.call_service(ctx, 4, COMBO, &[value, 2])
        }
        4 if value == 0 => f.finish(ctx, 0),
        4 => check_recipient(ctx, &f),
        5 if value & 0x40000000 != 0 => f.call_service(ctx, 6, "GetParent", &[recipient]),
        5 => close_combo(ctx, &f),
        6 if value == 0 => f.finish(ctx, 0),
        6 => close_combo(ctx, &f),
        7 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC cancel-modes continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn frame_accelerator_calls_document_virtual_and_preserves_fallback_handle() {
        for document_result in [0u32, 0x12345678] {
            let (mut cpu, mut k, mut d) = framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:103",
                &[0x2000],
            );
            let original = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(original, 59 * 4).unwrap();
            cpu.write_mem(0x2600, &bytes).unwrap();
            cpu.write_mem(0x2600 + 47 * 4, &0x6000u32.to_le_bytes())
                .unwrap();
            cpu.write_mem(0x2000, &0x2600u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2060, &0xabcdefu32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2200, &0x2800u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2800 + 43 * 4, &0x6100u32.to_le_bytes())
                .unwrap();
            cpu.write_arg(0, 0x2000).unwrap();
            let available = k.heap.free_bytes();
            let value = drive(&mut cpu, &mut k, &mut d, entry(1261), |pc, cpu| {
                if pc == 0x6000 {
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                    0x2200
                } else {
                    assert_eq!(pc, 0x6100);
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                    document_result
                }
            });
            assert_eq!(
                value,
                if document_result == 0 {
                    0xabcdef
                } else {
                    document_result
                }
            );
            assert_eq!(k.heap.free_bytes(), available);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        }
    }
}
