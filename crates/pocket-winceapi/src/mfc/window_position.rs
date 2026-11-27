//! Window placement delegates geometry, ownership queries and moves to WinCE.
use super::callbacks::service;
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const ENABLE: &str = "__PocketHLE_MFC_EnableWindow";
pub(super) const FOCUS: &str = "__PocketHLE_MFC_SetWindowFocus";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", FOCUS, set_focus);
    d.register_handler("coredll.dll", ENABLE, enable);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1129, "?EnableWindow@CWnd@@QAAHH@Z", enable),
    (840, "?CenterWindow@CWnd@@QAAXPAV1@@Z", center),
    (2348, "?SetFocus@CWnd@@QAAPAV1@XZ", set_focus),
    (1768, "?MoveWindow@CWnd@@QAAXHHHHH@Z", move_window),
    (2446, "?ShowWindow@CWnd@@QAAHH@Z", show),
];
fn target_rect(ctx: &mut CallCtx<'_>, frame: &Frame) -> Result<DispatchOutcome, KernelError> {
    let hwnd = frame.get(ctx, 0)?;
    frame.call_service(ctx, 3, "GetWindowRect", &[hwnd, frame.address + 128])
}
fn place(ctx: &mut CallCtx<'_>, frame: Frame) -> Result<DispatchOutcome, KernelError> {
    let mut rect = |offset: u32| -> Result<[i32; 4], KernelError> {
        let mut result = [0; 4];
        for (i, value) in result.iter_mut().enumerate() {
            *value = word(ctx, frame.address + offset + i as u32 * 4)? as i32;
        }
        Ok(result)
    };
    let window = rect(128)?;
    let work = rect(144)?;
    let center = rect(160)?;
    let width = i64::from(window[2]) - i64::from(window[0]);
    let height = i64::from(window[3]) - i64::from(window[1]);
    let axis = |start: i32, end: i32, length: i64, low: i32, high: i32| {
        let value = (i64::from(start) + i64::from(end)) / 2 - length / 2;
        if value < i64::from(low) {
            low
        } else if value + length > i64::from(high) {
            (i64::from(high) - length) as i32
        } else {
            value as i32
        }
    };
    let x = axis(center[0], center[2], width, work[0], work[2]);
    let y = axis(center[1], center[3], height, work[1], work[3]);
    let hwnd = frame.get(ctx, 0)?;
    frame.call_service(
        ctx,
        8,
        "SetWindowPos",
        &[hwnd, 0, x as u32, y as u32, u32::MAX, u32::MAX, 0x15],
    )
}
fn center(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        let object = frame.arg(ctx, 0)?;
        let parent = frame.arg(ctx, 1)?;
        let hwnd = word(ctx, object + 0x20)?;
        let parent = if parent == 0 {
            0
        } else {
            word(ctx, parent + 0x20)?
        };
        frame.set(ctx, 0, hwnd)?;
        frame.set(ctx, 1, parent)?;
        return frame.call_service(ctx, 1, "GetWindowLongW", &[hwnd, (-16i32) as u32]);
    }
    match frame.phase(ctx)? {
        1 => {
            frame.set(ctx, 2, u32::from(returned & 0x40000000 != 0))?;
            if frame.get(ctx, 1)? != 0 {
                return target_rect(ctx, &frame);
            }
            let hwnd = frame.get(ctx, 0)?;
            if frame.get(ctx, 2)? != 0 {
                frame.call_service(ctx, 2, "GetParent", &[hwnd])
            } else {
                frame.call_service(ctx, 2, "GetWindow", &[hwnd, 4])
            }
        }
        2 => {
            frame.set(ctx, 1, returned)?;
            if returned == 0 {
                return target_rect(ctx, &frame);
            }
            frame.call_service(ctx, 9, "SendMessageW", &[returned, 0x36b, 0, 0])
        }
        9 => {
            if returned != 0 {
                frame.set(ctx, 1, returned)?;
            }
            target_rect(ctx, &frame)
        }
        3 => {
            if returned == 0 {
                return frame.finish(ctx, 0);
            }
            if frame.get(ctx, 2)? != 0 {
                let hwnd = frame.get(ctx, 0)?;
                return frame.call_service(ctx, 10, "GetParent", &[hwnd]);
            }
            let parent = frame.get(ctx, 1)?;
            if parent == 0 {
                frame.call_service(
                    ctx,
                    5,
                    "SystemParametersInfoW",
                    &[0x30, 0, frame.address + 144, 0],
                )
            } else {
                frame.call_service(ctx, 4, "GetWindowLongW", &[parent, (-16i32) as u32])
            }
        }
        4 => {
            if returned & 0x10000000 == 0 {
                frame.set(ctx, 1, 0)?;
            }
            frame.call_service(
                ctx,
                5,
                "SystemParametersInfoW",
                &[0x30, 0, frame.address + 144, 0],
            )
        }
        5 => {
            if returned == 0 {
                return frame.finish(ctx, 0);
            }
            let parent = frame.get(ctx, 1)?;
            if parent != 0 {
                return frame.call_service(ctx, 6, "GetWindowRect", &[parent, frame.address + 160]);
            }
            let work = ctx.cpu.read_mem(frame.address + 144, 16)?;
            ctx.cpu.write_mem(frame.address + 160, &work)?;
            place(ctx, frame)
        }
        6 => {
            if returned == 0 {
                frame.finish(ctx, 0)
            } else {
                place(ctx, frame)
            }
        }
        10 => {
            frame.set(ctx, 3, returned)?;
            frame.call_service(ctx, 11, "GetClientRect", &[returned, frame.address + 144])
        }
        11 if returned == 0 => frame.finish(ctx, 0),
        11 => {
            let center = frame.get(ctx, 1)?;
            frame.call_service(ctx, 12, "GetClientRect", &[center, frame.address + 160])
        }
        12 if returned == 0 => frame.finish(ctx, 0),
        12 => {
            let center = frame.get(ctx, 1)?;
            let parent = frame.get(ctx, 3)?;
            frame.call_service(
                ctx,
                13,
                "MapWindowPoints",
                &[center, parent, frame.address + 160, 2],
            )
        }
        13 => place(ctx, frame), // zero means no translation, not failure
        8 => frame.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC CenterWindow continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn top_level_centering_uses_real_owner_geometry_and_keeps_the_window_size() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
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
        cpu.write_mem(0x2020, &0xdead1000u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2220, &0xdead1001u32.to_le_bytes()).unwrap();
        k.window_classes.insert(0xdead1000, "CenterTarget".into());
        k.window_classes.insert(0xdead1001, "CenterOwner".into());
        k.window_rects.insert(0xdead1000, (10, 20, 100, 80));
        k.window_rects.insert(0xdead1001, (40, 50, 160, 140));
        k.window_styles.insert(0xdead1000, (0x90000000, 0));
        k.window_styles.insert(0xdead1001, (0x90000000, 0));
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 0x2200).unwrap();
        let available = k.heap.free_bytes();
        drive(&mut cpu, &mut k, &mut d, entry(840), |pc, _| {
            panic!("Unexpected guest callback {pc:x}")
        });
        assert_eq!(k.window_rects[&0xdead1000], (70, 80, 100, 80));
        assert_eq!(k.heap.free_bytes(), available);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
}

fn show(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return frame.finish(ctx, result);
    }
    let object = frame.arg(ctx, 0)?;
    let command = frame.arg(ctx, 1)?;
    let ole = word(ctx, object + 0x40)?;
    if ole != 0 {
        let target = super::callbacks::virtual_function(ctx, ole, 41)?;
        return frame.call_args(ctx, 1, target, &[ole, command]);
    }
    let hwnd = word(ctx, object + 0x20)?;
    frame.call_service(ctx, 1, "ShowWindow", &[hwnd, command])
}

fn move_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        return frame.finish(ctx, 0);
    }
    let mut args = [0; 6];
    for (i, value) in args.iter_mut().enumerate() {
        *value = ctx.arg_u32(i as u8)?;
    }
    let ole = word(ctx, args[0] + 0x40)?;
    let target = if ole == 0 {
        args[0] = word(ctx, args[0] + 0x20)?;
        service(ctx, "MoveWindow")?
    } else {
        args[0] = ole;
        super::callbacks::virtual_function(ctx, ole, 39)?
    };
    frame.call_args(ctx, 1, target, &args)
}

fn set_focus(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        let value = if frame.phase(ctx)? == 1 {
            super::windows::wrapper(ctx, result)?
        } else {
            result
        };
        return frame.finish(ctx, value);
    }
    let object = frame.arg(ctx, 0)?;
    let ole = word(ctx, object + 0x40)?;
    if ole != 0 {
        let target = super::callbacks::virtual_function(ctx, ole, 44)?;
        return frame.call_args(ctx, 2, target, &[ole]);
    }
    let hwnd = word(ctx, object + 0x20)?;
    frame.call_service(ctx, 1, "SetFocus", &[hwnd])
}

// Keep the control-site override as a real guest call. HWND controls use the
// same USER owner as non-MFC applications, including WM_ENABLE notifications.
fn enable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    let object = f.arg(ctx, 0)?;
    let value = f.arg(ctx, 1)?;
    let site = word(ctx, object + 0x40)?;
    if site != 0 {
        let target = super::callbacks::virtual_function(ctx, site, 43)?;
        return f.call_args(ctx, 1, target, &[site, value]);
    }
    let hwnd = word(ctx, object + 0x20)?;
    f.call_service(ctx, 1, "EnableWindow", &[hwnd, value])
}

#[cfg(test)]
mod child_tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn child_centering_translates_a_sibling_center_into_parent_client_coordinates() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (object, hwnd) in [(0x2000, 7u32), (0x2200, 8)] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[object],
            );
            cpu.write_mem(object + 0x20, &hwnd.to_le_bytes()).unwrap();
            k.window_classes.insert(hwnd, "Child".into());
            k.window_styles.insert(hwnd, (0x50000000, 0));
            k.window_parents.insert(hwnd, 9);
        }
        k.window_classes.insert(9, "Parent".into());
        k.window_styles.insert(9, (0x90000000, 0));
        k.window_rects.insert(9, (20, 30, 200, 160));
        k.window_rects.insert(7, (0, 0, 40, 20));
        k.window_rects.insert(8, (60, 50, 100, 60));
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, 0x2200).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(840), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert_eq!(k.window_rects[&7], (90, 70, 40, 20));
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
}
