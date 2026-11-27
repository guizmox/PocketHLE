//! Walk real CE3 guest message maps; never invent a handler for an unknown ABI.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const ON_MESSAGE: &str = "__PocketHLE_MFC_OnWndMsg";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", ON_MESSAGE, on_message);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2060, "?OnWndMsg@CWnd@@MAAHIIJPAJ@Z", on_message),
    (
        1932,
        "?OnHScroll@CWnd@@IAAXIIPAVCScrollBar@@@Z",
        scroll_base,
    ),
    (
        2058,
        "?OnVScroll@CWnd@@IAAXIIPAVCScrollBar@@@Z",
        scroll_base,
    ),
    (
        1929,
        "?OnHScroll@CFrameWnd@@IAAXIIPAVCScrollBar@@@Z",
        horizontal_frame,
    ),
    (
        2055,
        "?OnVScroll@CFrameWnd@@IAAXIIPAVCScrollBar@@@Z",
        vertical_frame,
    ),
];
const HISTORY: u32 = 96;
const MAX_MAPS: u32 = 32;

fn handled(
    ctx: &mut CallCtx<'_>,
    frame: Frame,
    value: u32,
) -> Result<DispatchOutcome, KernelError> {
    let destination = frame.get(ctx, 0)?;
    if destination != 0 {
        put(ctx, destination, value)?;
    }
    frame.finish(ctx, 1)
}
fn on_message(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let message = frame.arg(ctx, 1)?;
    let wparam = frame.arg(ctx, 2)?;
    let lparam = frame.arg(ctx, 3)?;
    if frame.fresh {
        let result = ctx.arg_u32(4)?;
        if result != 0 {
            ctx.cpu
                .check_guest_access(result, 4, pocket_cpu::Prot::WRITE)?;
        }
        frame.set(ctx, 0, result)?;
        if message == 0x111 {
            let target = virtual_function(ctx, object, 30)?;
            return frame.call_args(ctx, 4, target, &[object, wparam, lparam]);
        }
        if message == 0x4e {
            if word(ctx, lparam)? == 0 {
                return frame.finish(ctx, 0);
            }
            put(ctx, frame.address + 88, 0)?;
            let target = virtual_function(ctx, object, 31)?;
            return frame.call_args(
                ctx,
                5,
                target,
                &[object, wparam, lparam, frame.address + 88],
            );
        }
        if message == 6 {
            let other = super::windows::wrapper(ctx, lparam)?;
            let target = service(ctx, super::activation::ACTIVATE)?;
            return frame.call_args(ctx, 6, target, &[object, wparam, other]);
        }
        if message == 0x20 {
            let target = service(ctx, super::activation::CURSOR)?;
            return frame.call_args(
                ctx,
                7,
                target,
                &[object, (lparam as i16 as i32) as u32, lparam >> 16],
            );
        }
        let target = virtual_function(ctx, object, 12)?;
        return frame.call_args(ctx, 1, target, &[object]);
    }
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    match frame.phase(ctx)? {
        3 => {
            let value = match frame.get(ctx, 2)? {
                1 => returned,
                2 => 1,
                _ => 0,
            };
            return handled(ctx, frame, value);
        }
        4 => {
            return if returned != 0 {
                handled(ctx, frame, 1)
            } else {
                frame.finish(ctx, 0)
            }
        }
        5 => {
            let value = word(ctx, frame.address + 88)?;
            return if returned != 0 {
                handled(ctx, frame, value)
            } else {
                frame.finish(ctx, 0)
            };
        }
        7 if returned != 0 => return handled(ctx, frame, 1),
        6 | 7 => {
            let target = virtual_function(ctx, object, 12)?;
            return frame.call_args(ctx, 1, target, &[object]);
        }
        1 | 2 => {}
        _ => {
            return Err(KernelError::Dispatch(
                "Invalid MFC message-map continuation".into(),
            ))
        }
    }
    let map = returned;
    if map == 0 {
        return frame.finish(ctx, 0);
    }
    ctx.cpu.check_guest_access(map, 8, pocket_cpu::Prot::READ)?;
    let count = frame.get(ctx, 1)?;
    if count >= MAX_MAPS {
        return Err(KernelError::Dispatch(
            "MFC message-map inheritance exceeds its bound".into(),
        ));
    }
    for index in 0..count {
        if word(ctx, frame.address + HISTORY + index * 4)? == map {
            return Err(KernelError::Dispatch(
                "Cyclic MFC message-map inheritance".into(),
            ));
        }
    }
    put(ctx, frame.address + HISTORY + count * 4, map)?;
    frame.set(ctx, 1, count + 1)?;
    let entries = word(ctx, map + 4)?;
    let Some(fields) = super::message_maps::find_window_entry(ctx, entries, message)? else {
        let base = word(ctx, map)?;
        return frame.call_args(ctx, 2, base, &[]);
    };
    let signature = if fields[2] == 0x1a { 47 } else { fields[4] };
    let Some((args, result_policy)) =
        marshal(ctx, object, signature, wparam, lparam, message >= 0xc000)?
    else {
        // Native CE3's unused signature slots skip invocation and leave zero
        // LRESULT while reporting a matched map entry.
        return handled(ctx, frame, 0);
    };
    frame.set(ctx, 2, result_policy)?;
    frame.call_args(ctx, 3, fields[5], &args)
}

fn low_signed(value: u32) -> u32 {
    value as i16 as i32 as u32
}
fn high_signed(value: u32) -> u32 {
    (value >> 16) as i16 as i32 as u32
}
// Returns native LRESULT policy: zero for void, actual result, or constant one.
// The complete 1..49 table is verified against the CE3 ARM OnWndMsg switch.
fn marshal(
    ctx: &mut CallCtx<'_>,
    object: u32,
    signature: u32,
    w: u32,
    l: u32,
    registered: bool,
) -> Result<Option<(Vec<u32>, u32)>, KernelError> {
    if registered {
        return Ok(Some((vec![object, w, l], 1)));
    }
    let win = |ctx: &mut CallCtx<'_>, h| super::windows::wrapper(ctx, h);
    let menu = |ctx: &mut CallCtx<'_>, h| super::menus::wrapper(ctx, h);
    let dc = |ctx: &mut CallCtx<'_>, h| {
        super::handle_maps::from_handle(ctx, super::handle_maps::Kind::Dc, h)
    };
    let (tail, result) = match signature {
        1 => (vec![dc(ctx, w)?], 1),
        2 => (vec![w], 1),
        3 | 8 => (vec![win(ctx, w)?, low_signed(l), l >> 16], 1),
        4 | 5 => {
            let h = word(ctx, l + 4)?;
            let kind = word(ctx, l + 8)?;
            let wrapper = dc(ctx, h)?;
            if signature == 4 {
                let h = word(ctx, l)?;
                (vec![wrapper, win(ctx, h)?, kind], 1)
            } else {
                (vec![wrapper, kind], 1)
            }
        }
        6 => (vec![w & 0xffff, win(ctx, l)?, w >> 16], 1),
        7 => (vec![w & 0xffff, w >> 16], 1),
        9 | 42 => (vec![l], 1),
        10 | 33 => (vec![w, l], 1),
        11 => (vec![w & 0xffff, w >> 16, menu(ctx, l)?], 1),
        12 => (vec![], 0),
        13 => (vec![w], 0),
        14 | 18 | 37 | 47 => (vec![w, l], 0),
        15 => (vec![low_signed(l), high_signed(l)], 0),
        16 | 17 => (vec![w, l & 0xffff, l >> 16], 0),
        19 => (
            vec![
                u32::from(word(ctx, object + 0x20)? == l),
                win(ctx, l)?,
                win(ctx, w)?,
            ],
            0,
        ),
        20 => (vec![dc(ctx, w)?], 0),
        21 => (vec![menu(ctx, w)?], 0),
        22 => (vec![menu(ctx, w)?, l & 0xffff, l >> 16], 0),
        23 => (vec![win(ctx, w)?], 0),
        24 => (vec![win(ctx, w)?, l & 0xffff, l >> 16], 0),
        25 => (vec![win(ctx, w)?, low_signed(l), high_signed(l)], 0),
        26 => (vec![win(ctx, w)?, l], 0),
        27 => (vec![w, win(ctx, l)?], 0),
        28 => (vec![w & 0xffff, win(ctx, l)?, w >> 16], 0),
        29 => (vec![low_signed(w), high_signed(w), win(ctx, l)?], 0),
        30 => (vec![low_signed(w), high_signed(w)], 0),
        31 | 36 => (vec![l], 0),
        32 | 43 => (vec![w, l], 2),
        34 => (vec![low_signed(l), high_signed(l)], 1),
        35 => (vec![], 1),
        38..=41 | 46 => return Ok(None),
        44 => (vec![win(ctx, l)?], 0),
        45 => (vec![win(ctx, w)?, l], 1),
        48 => (vec![w & 0xffff, w >> 16, l], 0),
        49 => (vec![w, low_signed(l), high_signed(l)], 0),
        _ => {
            return Err(KernelError::Dispatch(format!(
                "Invalid CE3 MFC message signature {signature}"
            )))
        }
    };
    let mut args = Vec::with_capacity(tail.len() + 1);
    args.push(object);
    args.extend(tail);
    Ok(Some((args, result)))
}

fn scroll_base(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let child = f.arg(ctx, 3)?;
        if child != 0 {
            let msg = super::application::current_thread_state(ctx)? + 0x3c;
            let message = word(ctx, msg + 4)?;
            let wp = word(ctx, msg + 8)?;
            let lp = word(ctx, msg + 12)?;
            let target = virtual_function(ctx, child, 42)?;
            return f.call_args(ctx, 1, target, &[child, message, wp, lp, 0]);
        }
    } else if f.phase(ctx)? == 2 || ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        return f.finish(ctx, 0);
    }
    let target = service(ctx, "__PocketHLE_MFC_WindowDefault")?;
    f.call_args(ctx, 2, target, &[object])
}
fn horizontal_frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll_frame(ctx, 0x114)
}
fn vertical_frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    scroll_frame(ctx, 0x115)
}
fn scroll_frame(ctx: &mut CallCtx<'_>, message: u32) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let view = word(ctx, object + 0xac)?;
        if view != 0 {
            let hwnd = word(ctx, view + 0x20)?;
            let msg = super::application::current_thread_state(ctx)? + 0x3c;
            let wp = word(ctx, msg + 8)?;
            let lp = word(ctx, msg + 12)?;
            let target = service(ctx, "SendMessageW")?;
            return f.call_args(ctx, 1, target, &[hwnd, message, wp, lp]);
        }
    }
    f.finish(ctx, 0)
}

#[cfg(test)]
mod tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn protected_message_result_is_rejected_before_calling_any_guest_handler() {
        use pocket_kernel::Dispatcher;
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        cpu.map_region(0x8000, 0x1000, pocket_cpu::Prot::READ)
            .unwrap();
        for (i, value) in [0x2000, 0x132, 0, 0].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        cpu.write_mem(0x4000, &0x8000u32.to_le_bytes()).unwrap();
        // The object intentionally has no vtable: output validation precedes
        // virtual lookup and any guest callback, even for reflected messages.
        let result = d.dispatch(&mut cpu, &entry(2060), &mut k);
        assert!(result.is_err());
        assert_eq!(cpu.read_mem(0x8000, 4).unwrap(), [0; 4]);
    }

    #[test]
    fn inherited_guest_maps_convert_signed_coordinates_and_ignore_void_callback_registers() {
        for (message, signature, lparam, expected_args, expected_result) in [
            (
                0x201,
                49,
                0xfffefffdu32,
                vec![0x2000, 7, 0xfffffffd, 0xfffffffe],
                0,
            ),
            (1, 9, 0x12345678, vec![0x2000, 0x12345678], 0xfffffffe),
            (
                0x8001,
                10,
                0x12345678,
                vec![0x2000, 7, 0x12345678],
                0xfffffffe,
            ),
        ] {
            let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[0x2000],
            );
            let table = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(table, 46 * 4).unwrap();
            cpu.write_mem(0x2400, &bytes).unwrap();
            cpu.write_mem(0x2400 + 12 * 4, &0x6000u32.to_le_bytes())
                .unwrap();
            cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
            for (address, value) in [
                (0x3000, 0x6100u32),
                (0x3004, 0x3100),
                (0x3200, 0x6300),
                (0x3204, 0x3300),
            ] {
                cpu.write_mem(address, &value.to_le_bytes()).unwrap();
            }
            cpu.write_mem(0x3100, &[0; 24]).unwrap();
            for (index, value) in [message as u32, 0, 0, 0, signature, 0x6200]
                .into_iter()
                .enumerate()
            {
                cpu.write_mem(0x3300 + index as u32 * 4, &value.to_le_bytes())
                    .unwrap();
            }
            cpu.write_mem(0x3318, &[0; 24]).unwrap();
            for (i, value) in [0x2000, message, 7, lparam].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            cpu.write_mem(0x4000, &0x2800u32.to_le_bytes()).unwrap();
            let available = k.heap.free_bytes();
            let mut called = 0;
            let result = drive(&mut cpu, &mut k, &mut d, entry(2060), |pc, cpu| match pc {
                0x6000 => 0x3000,
                0x6100 => 0x3200,
                0x6200 => {
                    called += 1;
                    for (i, &value) in expected_args.iter().enumerate() {
                        assert_eq!(cpu.read_arg(i as u8).unwrap(), value);
                    }
                    0xfffffffe
                }
                _ => panic!("Unexpected guest function {pc:x}"),
            });
            assert_eq!(result, 1);
            assert_eq!(called, 1);
            assert_eq!(cpu.read_u32_le(0x2800).unwrap(), expected_result);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert_eq!(k.heap.free_bytes(), available);
        }
    }
    #[test]
    fn command_messages_use_the_virtual_command_router_without_searching_window_maps() {
        for handled_command in [false, true] {
            let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[0x2000],
            );
            let table = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(table, 46 * 4).unwrap();
            cpu.write_mem(0x2400, &bytes).unwrap();
            cpu.write_mem(0x2400 + 30 * 4, &0x6000u32.to_le_bytes())
                .unwrap();
            cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
            for (i, value) in [0x2000, 0x111, 0x12340001, 0xdead1000]
                .into_iter()
                .enumerate()
            {
                cpu.write_arg(i as u8, value).unwrap();
            }
            cpu.write_mem(0x2800, &0xabcdefu32.to_le_bytes()).unwrap();
            cpu.write_mem(0x4000, &0x2800u32.to_le_bytes()).unwrap();
            let available = k.heap.free_bytes();
            let result = drive(&mut cpu, &mut k, &mut d, entry(2060), |pc, cpu| {
                assert_eq!(pc, 0x6000);
                for (i, value) in [0x2000, 0x12340001, 0xdead1000].into_iter().enumerate() {
                    assert_eq!(cpu.read_arg(i as u8).unwrap(), value);
                }
                u32::from(handled_command)
            });
            assert_eq!(result, u32::from(handled_command));
            assert_eq!(
                cpu.read_u32_le(0x2800).unwrap(),
                if handled_command { 1 } else { 0xabcdef }
            );
            assert_eq!(k.heap.free_bytes(), available);
        }
    }
}

#[cfg(test)]
#[path = "scroll_tests.rs"]
mod scroll_tests;

#[cfg(test)]
mod signature_tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn complete_scalar_signature_table_preserves_ce3_narrowing_and_return_policy() {
        let w = 0xfffe8001u32;
        let l = 0xfffd8002u32;
        let cases: [(u32, Vec<u32>, u32); 23] = [
            (2, vec![w], 0xdeadbeef),
            (7, vec![0x8001, 0xfffe], 0xdeadbeef),
            (9, vec![l], 0xdeadbeef),
            (10, vec![w, l], 0xdeadbeef),
            (12, vec![], 0),
            (13, vec![w], 0),
            (14, vec![w, l], 0),
            (15, vec![0xffff8002, 0xfffffffd], 0),
            (16, vec![w, 0x8002, 0xfffd], 0),
            (17, vec![w, 0x8002, 0xfffd], 0),
            (18, vec![w, l], 0),
            (30, vec![0xffff8001, 0xfffffffe], 0),
            (31, vec![l], 0),
            (32, vec![w, l], 1),
            (33, vec![w, l], 0xdeadbeef),
            (34, vec![0xffff8002, 0xfffffffd], 0xdeadbeef),
            (35, vec![], 0xdeadbeef),
            (36, vec![l], 0),
            (37, vec![w, l], 0),
            (42, vec![l], 0xdeadbeef),
            (43, vec![w, l], 1),
            (48, vec![0x8001, 0xfffe, l], 0),
            (49, vec![w, 0xffff8002, 0xfffffffd], 0),
        ];
        for (sig, expected, output) in cases {
            let (mut cpu, mut k, mut d) = framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[0x2000],
            );
            let table = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(table, 46 * 4).unwrap();
            cpu.write_mem(0x2400, &bytes).unwrap();
            cpu.write_mem(0x2400 + 12 * 4, &0x6000u32.to_le_bytes())
                .unwrap();
            cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x3000, &0x6100u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x3004, &0x3100u32.to_le_bytes()).unwrap();
            for (i, value) in [0x400u32, 0, 0, 0, sig, 0x6200].into_iter().enumerate() {
                cpu.write_mem(0x3100 + i as u32 * 4, &value.to_le_bytes())
                    .unwrap();
            }
            cpu.write_mem(0x3118, &[0; 24]).unwrap();
            for (i, value) in [0x2000, 0x400, w, l].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            cpu.write_mem(0x4000, &0x2800u32.to_le_bytes()).unwrap();
            let free = k.heap.free_bytes();
            let mut calls = 0;
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(2060), |pc, cpu| match pc {
                    0x6000 => 0x3000,
                    0x6200 => {
                        calls += 1;
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                        for (i, value) in expected.iter().enumerate() {
                            assert_eq!(
                                cpu.read_arg((i + 1) as u8).unwrap(),
                                *value,
                                "signature {sig}"
                            );
                        }
                        0xdeadbeef
                    }
                    _ => panic!("unexpected {pc:x}"),
                }),
                1
            );
            assert_eq!(calls, 1);
            assert_eq!(cpu.read_u32_le(0x2800).unwrap(), output, "signature {sig}");
            assert_eq!(k.heap.free_bytes(), free);
        }
    }
}
