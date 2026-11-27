//! MFC control adapters retain the shared WinCE message/notification authority.
use super::callbacks::{service, Frame};
use super::*;

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1272, "?GetDlgCtrlID@CWnd@@QBAHXZ", control_id),
    (1273, "?GetDlgItem@CWnd@@QBAPAV1@H@Z", dialog_item),
    (1576, "?GetText@CListBox@@QBAXHAAVCString@@@Z", list_string),
    (1595, "?GetWindowTextLengthW@CWnd@@QBAHXZ", text_length),
    (1596, "?GetWindowTextW@CWnd@@QBAHPAGH@Z", text_buffer),
    (
        1597,
        "?GetWindowTextW@CWnd@@QBAXAAVCString@@@Z",
        text_string,
    ),
    (2443, "?SetWindowTextW@CWnd@@QAAXPBG@Z", window_text),
    (2397, "?SetRange@CSliderCtrl@@QAAXHHH@Z", slider_range),
    (847, "?CheckRadioButton@CWnd@@QAAXHHH@Z", check_radio),
    (1241, "?GetCheckedRadioButton@CWnd@@QAAHHH@Z", checked_radio),
];
fn dialog_item(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let id = f.arg(ctx, 1)?;
        let container = word(ctx, object + 0x3c)?;
        if container != 0 {
            let method = super::callbacks::virtual_function(ctx, container, 29)?;
            return f.call_args(ctx, 2, method, &[container, id]);
        }
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, "GetDlgItem", &[hwnd, id]);
    }
    let result = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
    let result = match f.phase(ctx)? {
        1 => super::windows::wrapper(ctx, result)?,
        2 => result,
        _ => {
            return Err(KernelError::Dispatch(
                "Invalid MFC dialog-item continuation".into(),
            ))
        }
    };
    f.finish(ctx, result)
}
fn control_id(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    control_call(ctx, "GetDlgCtrlID", 37, 0)
}
fn window_text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    control_call(ctx, "SetWindowTextW", 33, 1)
}
fn text_length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    control_call(ctx, "GetWindowTextLengthW", 36, 0)
}
fn text_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    control_call(ctx, "GetWindowTextW", 34, 2)
}
fn list_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let hwnd = word(ctx, object + 0x20)?;
        let index = f.arg(ctx, 1)?;
        let value = f.arg(ctx, 2)?;
        return f.call_service(
            ctx,
            1,
            super::data_exchange_text::GET_LIST,
            &[hwnd, index, value],
        );
    }
    f.finish(ctx, 0)
}
fn text_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let value = f.arg(ctx, 1)?;
        let site = word(ctx, object + 0x40)?;
        if site != 0 {
            let method = super::callbacks::virtual_function(ctx, site, 35)?;
            return f.call_args(ctx, 1, method, &[site, value]);
        }
        let hwnd = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, super::data_exchange_text::GET_TEXT, &[hwnd, value]);
    }
    f.finish(ctx, 0)
}
fn control_call(
    ctx: &mut CallCtx<'_>,
    service_name: &str,
    slot: u32,
    arguments: u8,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let site = word(ctx, object + 0x40)?;
        let (target, arg) = if site == 0 {
            (service(ctx, service_name)?, word(ctx, object + 0x20)?)
        } else {
            (super::callbacks::virtual_function(ctx, site, slot)?, site)
        };
        let mut args = vec![arg];
        for i in 1..=arguments {
            args.push(f.arg(ctx, i.into())?);
        }
        return f.call_args(ctx, 1, target, &args);
    }
    let id = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
    f.finish(ctx, id)
}

fn slider_range(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let phase = if f.fresh { 0 } else { f.phase(ctx)? };
    let (next, message, value) = match phase {
        0 => (1, 0x407, f.arg(ctx, 1)?), // TBM_SETRANGEMIN, signed 32-bit endpoint
        1 => (2, 0x408, f.arg(ctx, 2)?), // TBM_SETRANGEMAX
        2 => return f.finish(ctx, 0),
        _ => {
            return Err(KernelError::Dispatch(
                "Invalid MFC slider-range continuation".into(),
            ))
        }
    };
    let hwnd = word(ctx, o + 0x20)?;
    let redraw = f.arg(ctx, 3)?;
    let target = service(ctx, "SendMessageW")?;
    f.call_args(ctx, next, target, &[hwnd, message, redraw, value])
}

fn check_radio(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    if word(ctx, object + 0x3c)? != 0 {
        return Err(KernelError::Dispatch(
            "MFC OLE radio-button access is not implemented".into(),
        ));
    }
    let hwnd = word(ctx, object + 0x20)?;
    let first = f.arg(ctx, 1)?;
    let last = f.arg(ctx, 2)?;
    let checked = f.arg(ctx, 3)?;
    let target = service(ctx, "CheckRadioButton")?;
    f.call_args(ctx, 1, target, &[hwnd, first, last, checked])
}

fn checked_radio(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use pocket_cpu::regs::ArmReg;
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let first = f.arg(ctx, 1)? as i32;
    let last = f.arg(ctx, 2)? as i32;
    let current = if f.fresh {
        if word(ctx, object + 0x3c)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC OLE radio-button access is not implemented".into(),
            ));
        }
        if first > last {
            return f.finish(ctx, 0);
        }
        if i64::from(last) - i64::from(first) >= 65536 {
            return Err(KernelError::Dispatch(
                "MFC radio-button range exceeds its bound".into(),
            ));
        }
        first
    } else {
        let current = f.get(ctx, 0)? as i32;
        match f.phase(ctx)? {
            1 => {
                let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
                let target = service(ctx, "SendMessageW")?;
                return f.call_args(ctx, 2, target, &[hwnd, 0xf0, 0, 0]);
            }
            2 => {
                if ctx.cpu.read_reg(ArmReg::R0)? != 0 {
                    return f.finish(ctx, current as u32);
                }
                if current == last {
                    return f.finish(ctx, 0);
                }
                current + 1
            }
            _ => {
                return Err(KernelError::Dispatch(
                    "Invalid MFC radio-button continuation".into(),
                ))
            }
        }
    };
    // Look up every ID through USER; missing controls and guest procedures keep
    // the same BM_GETCHECK semantics as the original CWnd adapter.
    f.set(ctx, 0, current as u32)?;
    let hwnd = word(ctx, object + 0x20)?;
    let target = service(ctx, "GetDlgItem")?;
    f.call_args(ctx, 1, target, &[hwnd, current as u32])
}

#[cfg(test)]
mod control_id_tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn control_id_uses_user_metadata_or_the_real_ole_site_virtual() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let child = k.controls.create(
            7,
            pocket_kernel::controls::ControlClass::Button,
            103,
            "Test".into(),
            0x50000000,
            0,
            0,
            20,
            20,
        );
        cpu.write_mem(0x2020, &child.to_le_bytes()).unwrap();
        cpu.write_reg(ArmReg::R0, 0x2000).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1272), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            103
        );
        cpu.write_mem(0x2040, &0x2400u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2400, &0x2800u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2800 + 37 * 4, &0x6200u32.to_le_bytes())
            .unwrap();
        cpu.write_reg(ArmReg::R0, 0x2000).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1272), |pc, cpu| {
                assert_eq!(pc, 0x6200);
                assert_eq!(cpu.read_arg(0).unwrap(), 0x2400);
                109
            }),
            109
        );
    }
}

#[cfg(test)]
mod dialog_item_tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::controls::ControlClass;
    #[test]
    fn dialog_item_reuses_borrowed_window_wrappers_and_delegates_real_container_overrides() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        k.window_classes.insert(7, "Frame".into());
        let child = k.controls.create(
            7,
            ControlClass::ListBox,
            42,
            String::new(),
            0x50000000,
            0,
            0,
            80,
            50,
        );
        let mut get = |id| {
            cpu.write_arg(0, 0x2000).unwrap();
            cpu.write_arg(1, id).unwrap();
            drive(&mut cpu, &mut k, &mut d, entry(1273), |pc, _| {
                panic!("unexpected callback {pc:x}")
            })
        };
        let wrapper = get(42);
        assert_ne!(wrapper, 0);
        assert_eq!(get(41), 0);
        drop(get);
        assert_eq!(cpu.read_u32_le(wrapper + 0x20).unwrap(), child);
        let baseline = k.heap.free_bytes();
        for _ in 0..64 {
            cpu.write_arg(0, 0x2000).unwrap();
            cpu.write_arg(1, 42).unwrap();
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(1273), |pc, _| panic!(
                    "unexpected {pc:x}"
                )),
                wrapper
            );
            assert_eq!(k.heap.free_bytes(), baseline);
        }
        drive(&mut cpu, &mut k, &mut d, entry(1048), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert!(k.heap.msize(wrapper).is_none());
        assert!(k.controls.get(child).is_some());
        cpu.write_mem(0x203c, &0x3000u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x3000, &0x3200u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x3200 + 29 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 42).unwrap();
        let value = drive(&mut cpu, &mut k, &mut d, entry(1273), |pc, ctx| {
            assert_eq!(pc, 0x6000);
            assert_eq!(ctx.read_reg(ArmReg::R0).unwrap(), 0x3000);
            assert_eq!(ctx.read_reg(ArmReg::R1).unwrap(), 42);
            0x2600
        });
        assert_eq!(value, 0x2600);
    }
}

#[cfg(test)]
mod window_text_tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    use pocket_kernel::controls::ControlClass;
    #[test]
    fn text_adapter_updates_user_text_and_keeps_real_site_virtuals() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let child = k.controls.create(
            7,
            ControlClass::Static,
            3,
            String::new(),
            0x50000000,
            0,
            0,
            100,
            30,
        );
        cpu.write_mem(0x2020, &child.to_le_bytes()).unwrap();
        let text: Vec<_> = "KJFK – tour"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x3000, &text).unwrap();
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 0x3000).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(2443), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert_eq!(k.controls.get(child).unwrap().text, "KJFK – tour");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:181",
            &[0x2300],
        );
        cpu.write_arg(0, 0x2000).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1595), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            11
        );
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 0x3200).unwrap();
        cpu.write_arg(2, 5).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1596), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            4
        );
        assert_eq!(
            cpu.read_mem(0x3200, 10).unwrap(),
            [b'K', 0, b'J', 0, b'F', 0, b'K', 0, 0, 0]
        );
        let mut baseline = None;
        for _ in 0..32 {
            cpu.write_arg(0, 0x2000).unwrap();
            cpu.write_arg(1, 0x2300).unwrap();
            drive(&mut cpu, &mut k, &mut d, entry(1597), |pc, _| {
                panic!("unexpected {pc:x}")
            });
            let data = cpu.read_u32_le(0x2300).unwrap();
            assert_eq!(cpu.read_u32_le(data - 8).unwrap(), 11);
            assert_eq!(cpu.read_mem(data, text.len() as u32).unwrap(), text);
            if let Some(free) = baseline {
                assert_eq!(k.heap.free_bytes(), free);
            } else {
                baseline = Some(k.heap.free_bytes());
            }
        }
        let list = k.controls.create(
            7,
            ControlClass::ListBox,
            4,
            String::new(),
            0x50000000,
            0,
            0,
            100,
            80,
        );
        k.controls
            .get_mut(list)
            .unwrap()
            .list_items
            .push(("situation – été".into(), 0));
        cpu.write_mem(0x2020, &list.to_le_bytes()).unwrap();
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 0).unwrap();
        cpu.write_arg(2, 0x2300).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(1576), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        let data = cpu.read_u32_le(0x2300).unwrap();
        let expected: Vec<_> = "situation – été"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(cpu.read_mem(data, expected.len() as u32).unwrap(), expected);
        cpu.write_mem(0x2020, &child.to_le_bytes()).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2300],
        );
        cpu.write_mem(0x2040, &0x2400u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2400, &0x2800u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2800 + 33 * 4, &0x6100u32.to_le_bytes())
            .unwrap();
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 0x3000).unwrap();
        let result = drive(&mut cpu, &mut k, &mut d, entry(2443), |pc, cpu| {
            assert_eq!(pc, 0x6100);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2400);
            assert_eq!(cpu.read_arg(1).unwrap(), 0x3000);
            99
        });
        assert_eq!(result, 99);
    }
}
