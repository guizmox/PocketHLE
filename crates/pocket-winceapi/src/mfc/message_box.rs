//! MFC message boxes use the same real modal/input service as MessageBoxW.
//! Resource text is owned only across the guest virtual call; no response is
//! synthesized here and guest CWinApp overrides remain observable.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const TEXT: &str = "__PocketHLE_MFC_TextMessageBox";
pub(super) const MODELESS: &str = "__PocketHLE_MFC_EnableModeless";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", MODELESS, enable_modeless);
    d.register_handler("coredll.dll", TEXT, text_box);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (673, "?AfxMessageBox@@YAHIII@Z", resource_box),
    (674, "?AfxMessageBox@@YAHPBGII@Z", text_box),
    (1077, "?DoMessageBox@CWinApp@@UAAHPBGII@Z", do_message_box),
    (1125, "?EnableModeless@CWinApp@@QAAXH@Z", enable_modeless),
    (1760, "?MessageBoxW@CWnd@@QAAHPBG0I@Z", window_box),
    (1093, "?DoWaitCursor@CWinApp@@UAAXH@Z", wait_cursor),
];
fn resource_box(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    afx_box(ctx, true)
}
fn text_box(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    afx_box(ctx, false)
}
fn afx_box(ctx: &mut CallCtx<'_>, resource: bool) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if frame.fresh {
        let source = frame.arg(ctx, 0)?;
        let style = frame.arg(ctx, 1)?;
        let mut help = frame.arg(ctx, 2)?;
        let state = super::application::state(ctx)?;
        let app = word(ctx, state + 4)?;
        if app == 0 {
            return Err(KernelError::Dispatch(
                "MFC message box requires an initialized CWinApp".into(),
            ));
        }
        let text = if resource {
            let instance = word(ctx, state + 12)?;
            let text = crate::coredll::resource_string(ctx, instance, source);
            let p = super::application::duplicate_wide(ctx, &text)?;
            frame.own(ctx, p)?;
            if help == u32::MAX {
                help = source;
            }
            p
        } else {
            source
        };
        let f = virtual_function(ctx, app, 35)?;
        return frame.call(ctx, 1, f, [app, text, style, help]);
    }
    if frame.phase(ctx)? != 1 {
        return Err(KernelError::Dispatch(
            "Invalid MFC message-box continuation".into(),
        ));
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    frame.finish(ctx, result)
}

fn enable_modeless(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::pretranslation::MAIN, &[]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 1 && result != 0 {
        // The retail CE method invokes IsFrameWnd here, not the desktop
        // Begin/EndModalState graph. Preserve the real guest virtual call.
        let enabled = f.arg(ctx, 1)?;
        let target = virtual_function(ctx, result, 44)?;
        return f.call_args(ctx, 2, target, &[result, enabled]);
    }
    f.finish(ctx, 0)
}
fn window_box(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    let object = f.arg(ctx, 0)?;
    let text = f.arg(ctx, 1)?;
    let caption = f.arg(ctx, 2)?;
    let style = f.arg(ctx, 3)?;
    let hwnd = if object == 0 {
        0
    } else {
        word(ctx, object + 0x20)?
    };
    let caption = if caption == 0 {
        let module = super::application::state(ctx)?;
        word(ctx, module + 0x10)?
    } else {
        caption
    };
    f.call_service(ctx, 1, "MessageBoxW", &[hwnd, text, caption, style])
}
fn prepare_box(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    prompt: u32,
) -> Result<DispatchOutcome, KernelError> {
    let app = f.arg(ctx, 0)?;
    let pointer = if prompt == 0 { app + 0x98 } else { prompt };
    ctx.cpu
        .check_guest_access(pointer, 4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let old = word(ctx, pointer)?;
    f.set(ctx, 0, pointer)?;
    f.set(ctx, 1, old)?;
    let help = f.arg(ctx, 3)?;
    if help != 0 {
        put(ctx, pointer, help.wrapping_add(0x30000))?;
    }
    let text = f.arg(ctx, 1)?;
    let caption = word(ctx, app + 0x78)?;
    let mut style = f.arg(ctx, 2)?;
    if style & 0xf0 == 0 && matches!(style & 0xf,0..=1|3..=4) {
        style |= 0x30;
    }
    let owner = f.get(ctx, 2)?;
    f.call_service(
        ctx,
        5,
        "MessageBoxW",
        &[owner, text, caption, style | 0x10000],
    )
}
fn do_message_box(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let app = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(app, 0xb4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
        let target = virtual_function(ctx, app, 36)?;
        return f.call_args(ctx, 1, target, &[app, 0x80000000]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => f.call_service(ctx, 2, MODELESS, &[app, 0]),
        2 => {
            let output = f.address + 128;
            put(ctx, output, 0)?;
            f.call_service(ctx, 3, super::safe_owner::SAFE, &[0, output])
        }
        3 => {
            f.set(ctx, 2, result)?;
            if result != 0 {
                return f.call_service(ctx, 4, "SendMessageW", &[result, 0x376, 0, 0]);
            }
            prepare_box(ctx, f, 0)
        }
        4 => prepare_box(ctx, f, result),
        5 => {
            f.set(ctx, 3, result)?;
            let pointer = f.get(ctx, 0)?;
            let old = f.get(ctx, 1)?;
            put(ctx, pointer, old)?;
            let top = word(ctx, f.address + 128)?;
            if top != 0 {
                return f.call_service(ctx, 7, "EnableWindow", &[top, 1]);
            }
            f.call_service(ctx, 6, MODELESS, &[app, 1])
        }
        7 => f.call_service(ctx, 6, MODELESS, &[app, 1]),
        6 => {
            let answer = f.get(ctx, 3)?;
            f.finish(ctx, answer)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC application message box continuation".into(),
        )),
    }
}

fn wait_cursor(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let app = ctx.arg_u32(0)?;
    let change = ctx.arg_u32(1)? as i32;
    ctx.cpu
        .check_guest_access(app, 0xb4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let old = word(ctx, app + 0x9c)? as i32;
    let count = old.wrapping_add(change);
    let selected = if count > 0 {
        pocket_kernel::controls::system_cursor_handle(32514).unwrap()
    } else {
        word(ctx, app + 0xa0)?
    };
    ctx.cpu.write_arg(0, selected)?;
    let previous = crate::coredll::set_cursor(ctx)?;
    if count > 0 {
        put(ctx, app + 0x9c, count as u32)?;
        if change > 0 && count == 1 {
            let DispatchOutcome::ReturnedR0(previous) = previous else {
                return Err(KernelError::Dispatch("SetCursor did not return".into()));
            };
            put(ctx, app + 0xa0, previous)?;
        }
    } else {
        put(ctx, app + 0x9c, 0)?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    use pocket_kernel::{Dispatcher, InputEvent, Thunk};
    #[test]
    fn resource_message_box_preserves_virtual_calls_waits_for_real_input_and_releases_frames() {
        for (key, expected) in [(13, 6), (27, 7)] {
            let (mut cpu, mut k, mut d) = framework_setup();
            let caption: Vec<_> = "Test app"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            cpu.write_mem(0x1000, &caption).unwrap();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:201",
                &[0x2000, 0x1000],
            );
            let mut words = vec![0u16; 15];
            let text: Vec<_> = "Continue?".encode_utf16().collect();
            words.push(text.len() as u16);
            words.extend(text);
            let bytes: Vec<_> = words.into_iter().flat_map(u16::to_le_bytes).collect();
            cpu.write_mem(0x1800, &bytes).unwrap();
            k.resources.push(pocket_pe::ResourceEntry {
                ty: pocket_pe::ResourceKey::Id(6),
                name: pocket_pe::ResourceKey::Id(10),
                data_rva: 0x1800,
                native_leaves: vec![],
                size: bytes.len() as u32,
                codepage: 0,
            });
            // Safe-owner selection retains per-thread framework state. Warm
            // that persistent owner before checking temporary frame cleanup.
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:651", &[]);
            // FromHandle's temporary active-window identity is retained until
            // idle cleanup, separately from the message-box continuation.
            super::super::window_dispatch::tests::drive(
                &mut cpu,
                &mut k,
                &mut d,
                super::super::window_dispatch::tests::entry(2633),
                |pc, _| panic!("unexpected main-window callback {pc:x}"),
            );
            let baseline = k.heap.free_bytes();
            let start = 0x72000000;
            let end = 0x12340000;
            let entry = Thunk {
                thunk_va: start,
                iat_va: 0,
                dll: "mfcce300.dll".into(),
                binding: pocket_pe::ImportBinding::Ordinal(673),
                friendly_name: None,
            };
            let mut thunks: std::collections::HashMap<_, _> = k.dynamic_exports[&1]
                .iter()
                .map(|(name, va)| {
                    (
                        *va,
                        Thunk {
                            thunk_va: *va,
                            iat_va: 0,
                            dll: "coredll.dll".into(),
                            binding: pocket_pe::ImportBinding::Name(name.clone()),
                            friendly_name: None,
                        },
                    )
                })
                .collect();
            thunks.insert(start, entry);
            for (r, v) in [
                (ArmReg::R0, 159),
                (ArmReg::R1, 4),
                (ArmReg::R2, u32::MAX),
                (ArmReg::Lr, end),
            ] {
                cpu.write_reg(r, v).unwrap();
            }
            let mut pc = start;
            let mut answered = false;
            for _ in 0..128 {
                if pc == end {
                    break;
                }
                let outcome = d.dispatch(&mut cpu, &thunks[&pc], &mut k).unwrap();
                if !answered {
                    if let Some(modal) = k.modal.as_ref() {
                        assert_eq!(modal.caption, "Test app");
                        assert_eq!(modal.lines.join(" "), "Continue?");
                        assert_eq!(
                            modal.buttons.iter().map(|b| b.result).collect::<Vec<_>>(),
                            [6, 7]
                        );
                        assert_eq!(cpu.read_u32_le(0x2098).unwrap(), 0x30000 + 159);
                        k.pending_input.push_back(InputEvent::KeyDown { vk: key });
                        answered = true;
                    }
                }
                pc = match outcome {
                    DispatchOutcome::JumpTo(target) => target,
                    DispatchOutcome::ReturnedR0(result) => {
                        cpu.write_reg(ArmReg::R0, result).unwrap();
                        cpu.read_reg(ArmReg::Lr).unwrap()
                    }
                    other => panic!("Unexpected modal outcome {other:?}"),
                };
            }
            assert!(answered);
            assert_eq!(pc, end);
            assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), expected);
            assert!(k.modal.is_none());
            assert_eq!(cpu.read_u32_le(0x2098).unwrap(), 0);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert_eq!(k.heap.free_bytes(), baseline);
            assert!(!k
                .dynamic_exports
                .values()
                .any(|m| m.keys().any(|n| n.starts_with("__PocketHLE_MFC_Frame:"))));
        }
    }
}
