//! Frame status messages preserve virtual overrides and use the WinCE text owner.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1928, "?OnHelpPromptAddr@CFrameWnd@@IAAJIJ@Z", help_prompt),
    (2012, "?OnSetFocus@CFrameWnd@@IAAXPAVCWnd@@@Z", on_focus),
    (2015, "?OnSetMessageString@CFrameWnd@@IAAJIJ@Z", set_message),
    (
        1349,
        "?GetMessageBar@CFrameWnd@@UAAPAVCWnd@@XZ",
        message_bar,
    ),
    (
        1381,
        "?GetMessageString@CFrameWnd@@UBAXIAAVCString@@@Z",
        message_string,
    ),
];
fn message_bar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let hwnd = word(ctx, object + 0x20)?;
    Ok(DispatchOutcome::ReturnedR0(
        super::descendants::find_permanent(ctx, hwnd, 0xe801)?,
    ))
}
fn message_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let id = ctx.arg_u32(1)?;
    let string = ctx.arg_u32(2)?;
    let mut text = super::resources::string(ctx, id)?;
    text.truncate(255);
    let end = text.iter().position(|&ch| ch == 10).unwrap_or(text.len());
    super::strings::replace(ctx, string, &text[..end])?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn finish(ctx: &mut CallCtx<'_>, frame: Frame) -> Result<DispatchOutcome, KernelError> {
    let object = frame.arg(ctx, 0)?;
    let id = frame.get(ctx, 1)?;
    put(ctx, object + 0xa4, id)?;
    put(ctx, object + 0xa8, id)?;
    if frame.get(ctx, 2)? != 0 {
        release_string(ctx, frame.address + 128)?;
    }
    let previous = frame.get(ctx, 0)?;
    frame.finish(ctx, previous)
}
fn set_bar_text(
    ctx: &mut CallCtx<'_>,
    frame: &Frame,
    text: u32,
) -> Result<DispatchOutcome, KernelError> {
    let bar = frame.get(ctx, 3)?;
    if word(ctx, bar + 0x40)? != 0 {
        return Err(KernelError::Dispatch(
            "MFC OLE window text is not implemented".into(),
        ));
    }
    let hwnd = word(ctx, bar + 0x20)?;
    let target = service(ctx, "SetWindowTextW")?;
    frame.call_args(ctx, 3, target, &[hwnd, text])
}
fn set_message(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        let flags = word(ctx, object + 0x2c)?;
        put(ctx, object + 0x2c, flags & !0x40)?;
        let previous = word(ctx, object + 0xa8)?;
        frame.set(ctx, 0, previous)?;
        let id = frame.arg(ctx, 1)?;
        frame.set(ctx, 1, id)?;
        let target = virtual_function(ctx, object, 52)?;
        return frame.call_args(ctx, 1, target, &[object]);
    }
    match frame.phase(ctx)? {
        1 => {
            if result == 0 {
                return finish(ctx, frame);
            }
            frame.set(ctx, 3, result)?;
            new_string(ctx, frame.address + 128, &[])?;
            frame.set(ctx, 2, 1)?;
            let text = frame.arg(ctx, 2)?;
            let id = frame.get(ctx, 1)?;
            if text != 0 || id == 0 {
                return set_bar_text(ctx, &frame, text);
            }
            let id = if id == 0xef06 && word(ctx, object + 0xb0)? != 0 {
                0xf005
            } else {
                id
            };
            frame.set(ctx, 1, id)?;
            let target = virtual_function(ctx, object, 49)?;
            let string = frame.address + 128;
            frame.call_args(ctx, 2, target, &[object, id, string])
        }
        2 => {
            let text = word(ctx, frame.address + 128)?;
            set_bar_text(ctx, &frame, text)
        }
        3 => {
            let bar = frame.get(ctx, 3)?;
            let hwnd = word(ctx, bar + 0x20)?;
            frame.set(ctx, 4, hwnd)?;
            let target = service(ctx, "GetParent")?;
            frame.call_args(ctx, 4, target, &[hwnd])
        }
        4 => {
            if result == 0 {
                return finish(ctx, frame);
            }
            let visits = frame.get(ctx, 5)?;
            if visits >= 128 {
                return Err(KernelError::Dispatch(
                    "MFC parent-frame search exceeds its bound".into(),
                ));
            }
            frame.set(ctx, 5, visits + 1)?;
            frame.set(ctx, 4, result)?;
            let parent = super::windows::wrapper(ctx, result)?;
            frame.set(ctx, 6, parent)?;
            let target = virtual_function(ctx, parent, 44)?;
            frame.call_args(ctx, 5, target, &[parent])
        }
        5 => {
            if result != 0 {
                let parent = frame.get(ctx, 6)?;
                let id = frame.get(ctx, 1)?;
                put(ctx, parent + 0xa4, id)?;
                put(ctx, parent + 0xa8, id)?;
                return finish(ctx, frame);
            }
            let hwnd = frame.get(ctx, 4)?;
            let target = service(ctx, "GetParent")?;
            frame.call_args(ctx, 4, target, &[hwnd])
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC status-message continuation".into(),
        )),
    }
}

fn on_focus(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        return frame.finish(ctx, 0);
    }
    let object = frame.arg(ctx, 0)?;
    let view = word(ctx, object + 0xac)?;
    let (name, argument) = if view == 0 {
        ("__PocketHLE_MFC_WindowDefault", object)
    } else {
        if word(ctx, view + 0x40)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC OLE view focus is not implemented".into(),
            ));
        }
        ("SetFocus", word(ctx, view + 0x20)?)
    };
    let target = service(ctx, name)?;
    frame.call_args(ctx, 1, target, &[argument])
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn status_messages_update_real_window_text_and_frame_ids_without_leaking_temporaries() {
        for with_bar in [false, true] {
            let (mut cpu, mut k, mut d) = framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:103",
                &[0x2000],
            );
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[0x2200],
            );
            for (hwnd, object) in [(0xdead1000u32, 0x2000u32), (0xdead1001, 0x2200)] {
                k.window_classes.insert(hwnd, "StatusTest".into());
                k.dynamic_exports
                    .get_mut(&1)
                    .unwrap()
                    .insert(format!("__PocketHLE_MFC_Window:{hwnd:08x}"), object);
                cpu.write_mem(object + 0x20, &hwnd.to_le_bytes()).unwrap();
            }
            k.window_parents.insert(0xdead1001, 0xdead1000);
            k.window_styles.insert(0xdead1001, (0x50000000, 0));
            k.window_ids
                .insert(0xdead1001, if with_bar { 0xe801 } else { 7 });
            cpu.write_mem(0x202c, &0x40u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x20a8, &33u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x1000, &[82, 0, 101, 0, 97, 0, 100, 0, 121, 0, 0, 0])
                .unwrap();
            for (i, value) in [0x2000, 77, 0x1000].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            let baseline = k.heap.free_bytes();
            let previous = drive(&mut cpu, &mut k, &mut d, entry(2015), |pc, _| {
                panic!("Unexpected guest callback {pc:x}")
            });
            assert_eq!(previous, 33);
            assert_eq!(cpu.read_u32_le(0x20a8).unwrap(), 77);
            assert_eq!(cpu.read_u32_le(0x20a4).unwrap(), 77);
            assert_eq!(cpu.read_u32_le(0x202c).unwrap() & 0x40, 0);
            assert_eq!(
                k.window_titles.get(&0xdead1001).map(String::as_str),
                with_bar.then_some("Ready")
            );
            assert_eq!(k.heap.free_bytes(), baseline);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        }
    }
}

fn help_prompt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(
        object + 0x64,
        4,
        pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
    )?;
    Ok(DispatchOutcome::ReturnedR0(object + 0x64))
}
