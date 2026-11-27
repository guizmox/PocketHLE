//! CE control-color translation and notification reflection preserve real callbacks.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const CHILD: &str = "__PocketHLE_MFC_ChildNotify";
const REFLECT: &str = "__PocketHLE_MFC_ReflectChildNotify";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1817, "?OnChildNotify@CListBox@@MAAHIIJPAJ@Z", list_notify),
    (
        1106,
        "?DrawItem@CListBox@@UAAXPAUtagDRAWITEMSTRUCT@@@Z",
        empty_draw_item,
    ),
    (
        1757,
        "?MeasureItem@CListBox@@UAAXPAUtagMEASUREITEMSTRUCT@@@Z",
        empty_draw_item,
    ),
    (
        1039,
        "?DeleteItem@CListBox@@UAAXPAUtagDELETITEMSTRUCT@@@Z",
        empty_draw_item,
    ),
    (
        875,
        "?CompareItem@CListBox@@UAAHPAUtagCOMPAREITEMSTRUCT@@@Z",
        empty_draw_item,
    ),
    (
        2502,
        "?VKeyToItem@CListBox@@UAAHII@Z",
        super::window_dispatch::last_message_default,
    ),
    (
        842,
        "?CharToItem@CListBox@@UAAHII@Z",
        super::window_dispatch::last_message_default,
    ),
    (
        1814,
        "?OnChildNotify@CButton@@MAAHIIJPAJ@Z",
        owner_draw_notify,
    ),
    (
        1103,
        "?DrawItem@CButton@@UAAXPAUtagDRAWITEMSTRUCT@@@Z",
        empty_draw_item,
    ),
    (1967, "?OnNTCtlColor@CWnd@@IAAJIJ@Z", nt_color),
    (
        1861,
        "?OnCtlColor@CWnd@@IAAPAUHBRUSH__@@PAVCDC@@PAV1@I@Z",
        color,
    ),
    (1823, "?OnChildNotify@CWnd@@MAAHIIJPAJ@Z", child_notify),
    (2161, "?ReflectChildNotify@CWnd@@IAAHIIJPAJ@Z", reflect),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", CHILD, child_notify);
    d.register_handler("coredll.dll", REFLECT, reflect);
}
fn nt_color(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let r = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, r);
    }
    let o = f.arg(ctx, 0)?;
    let dc = f.arg(ctx, 1)?;
    let hwnd = f.arg(ctx, 2)?;
    let ts = super::application::current_thread_state(ctx)?;
    let message = word(ctx, ts + 0x40)?;
    if !(0x132..=0x138).contains(&message) {
        return Err(KernelError::Dispatch(
            "MFC control-color translation requires its current native message".into(),
        ));
    }
    for (i, v) in [hwnd, dc, message - 0x132].into_iter().enumerate() {
        put(ctx, f.address + 128 + i as u32 * 4, v)?;
    }
    let target = virtual_function(ctx, o, 38)?;
    f.call_args(ctx, 1, target, &[o, 0x19, 0, f.address + 128])
}
pub(super) fn color(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let child = f.arg(ctx, 2)?;
        if child == 0 {
            return f.call_service(ctx, 2, "__PocketHLE_MFC_WindowDefault", &[o]);
        }
        let ts = super::application::current_thread_state(ctx)?;
        let message = word(ctx, ts + 0x40)?;
        let wp = word(ctx, ts + 0x44)?;
        let lp = word(ctx, ts + 0x48)?;
        put(ctx, f.address + 128, 0)?;
        let target = virtual_function(ctx, child, 42)?;
        return f.call_args(ctx, 1, target, &[child, message, wp, lp, f.address + 128]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 1 {
        if result != 0 {
            let brush = word(ctx, f.address + 128)?;
            return f.finish(ctx, brush);
        }
        return f.call_service(ctx, 2, "__PocketHLE_MFC_WindowDefault", &[o]);
    }
    f.finish(ctx, result)
}
fn child_notify(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let msg = f.arg(ctx, 1)?;
    if f.fresh {
        let result = ctx.arg_u32(4)?;
        f.set(ctx, 0, result)?;
        let wp = f.arg(ctx, 2)?;
        let lp = f.arg(ctx, 3)?;
        if word(ctx, o + 0x40)? != 0 {
            let h = word(ctx, o + 0x20)?;
            return f.call_service(
                ctx,
                1,
                "SendMessageW",
                &[h, msg.wrapping_add(0x2000), wp, lp],
            );
        }
        return f.call_service(ctx, 2, REFLECT, &[o, msg, wp, lp, result]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 2 {
        return f.finish(ctx, result);
    }
    if (0x132..=0x138).contains(&msg) && result == 0 {
        return f.finish(ctx, 0);
    }
    let destination = f.get(ctx, 0)?;
    if destination != 0 {
        put(ctx, destination, result)?;
    }
    f.finish(ctx, 1)
}
fn reflect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let msg = f.arg(ctx, 1)?;
    if !f.fresh {
        let handled = ctx.cpu.read_reg(ArmReg::R0)?;
        let result = f.get(ctx, 0)?;
        if f.phase(ctx)? == 2 && handled != 0 && result != 0 {
            put(ctx, result, 1)?;
        }
        let success = handled != 0 && (!(0x132..=0x138).contains(&msg) || word(ctx, result)? != 0);
        return f.finish(ctx, u32::from(success));
    }
    let result = ctx.arg_u32(4)?;
    f.set(ctx, 0, result)?;
    let wp = f.arg(ctx, 2)?;
    let lp = f.arg(ctx, 3)?;
    if (0x132..=0x138).contains(&msg) {
        ctx.cpu
            .check_guest_access(result, 4, Prot::READ | Prot::WRITE)?;
        put(ctx, f.address + 128, 0)?;
        put(ctx, f.address + 132, wp)?;
        put(ctx, f.address + 136, msg - 0x132)?;
        return f.call_service(
            ctx,
            1,
            super::window_messages::ON_MESSAGE,
            &[o, 0xbc19, 0, f.address + 128, result],
        );
    }
    if matches!(msg, 0x2b..=0x2f | 0x39 | 0x114 | 0x115) {
        return f.call_service(
            ctx,
            1,
            super::window_messages::ON_MESSAGE,
            &[o, msg + 0xbc00, wp, lp, result],
        );
    }
    if msg == 0x111 {
        let target = virtual_function(ctx, o, 5)?;
        return f.call_args(ctx, 2, target, &[o, 0, 0xbd110000 | (wp >> 16), 0, 0]);
    }
    if msg == 0x4e {
        let code = word(ctx, lp + 8)? & 0xffff;
        put(ctx, f.address + 128, result)?;
        put(ctx, f.address + 132, lp)?;
        let target = virtual_function(ctx, o, 5)?;
        return f.call_args(
            ctx,
            3,
            target,
            &[o, 0, 0xbc4e0000 | code, f.address + 128, 0],
        );
    }
    // These messages have no native MFC reflection protocol.
    f.finish(ctx, 0)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::Thunk;
    #[test]
    fn native_control_color_reflects_through_real_cdc_and_child_map_and_restores_message_context() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let hwnd = 0xdead1000;
        let child = 0xdead1001;
        for (o, h) in [(0x2000, hwnd), (0x2200, child)] {
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:206", &[o]);
            k.window_classes.insert(h, "ColorWindow".into());
            k.window_procs
                .insert(h, k.dynamic_exports[&1]["DefWindowProcW"]);
        }
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, hwnd).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(2460), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:804",
            &[0x2200, child],
        );
        let table = cpu.read_u32_le(0x2200).unwrap();
        let bytes = cpu.read_mem(table, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &bytes).unwrap();
        cpu.write_mem(0x2400 + 12 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2200, &0x2400u32.to_le_bytes()).unwrap();
        for (p, v) in [(0x3000, 0x6100u32), (0x3004, 0x3100)] {
            cpu.write_mem(p, &v.to_le_bytes()).unwrap();
        }
        for (i, v) in [0xbc19u32, 0, 0, 0, 5, 0x6200].into_iter().enumerate() {
            cpu.write_mem(0x3100 + i as u32 * 4, &v.to_le_bytes())
                .unwrap();
        }
        cpu.write_mem(0x3118, &[0; 24]).unwrap();
        let hdc = k.gdi.create_memory_dc();
        let brush = k.gdi.create_solid_brush(0x123456);
        for (i, v) in [hwnd, 0x138, hdc, child].into_iter().enumerate() {
            cpu.write_arg(i as u8, v).unwrap();
        }
        let send = Thunk {
            thunk_va: 0x70000020,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("SendMessageW".into()),
            friendly_name: None,
        };
        let mut reflected = 0;
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, send, |pc, cpu| match pc {
                0x6000 => 0x3000,
                0x6100 => 0,
                0x6200 => {
                    reflected += 1;
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                    let wrapper = cpu.read_arg(1).unwrap();
                    assert_eq!(cpu.read_u32_le(wrapper + 4).unwrap(), hdc);
                    assert_eq!(cpu.read_arg(2).unwrap(), 6);
                    brush
                }
                _ => panic!("unexpected {pc:x}"),
            }),
            brush
        );
        assert_eq!(reflected, 1);
        let ts = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:651",
            &[],
        ));
        assert_eq!(cpu.read_mem(ts + 0x3c, 28).unwrap(), [0; 28]);
        drive(&mut cpu, &mut k, &mut d, entry(1044), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert!(k.gdi.dc(hdc).is_some() && k.gdi.get(brush).is_some());
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
}

pub(super) fn owner_draw_notify(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    owner_notify(ctx, false)
}
fn list_notify(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    owner_notify(ctx, true)
}
fn owner_notify(ctx: &mut CallCtx<'_>, list: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        let phase = f.phase(ctx)?;
        if !matches!(phase, 1 | 2 | 3) {
            return Err(KernelError::Dispatch(
                "Invalid MFC owner-draw notification continuation".into(),
            ));
        }
        if phase == 3 {
            let output = f.get(ctx, 0)?;
            put(ctx, output, result)?;
        }
        return f.finish(ctx, if phase == 2 { result } else { 1 });
    }
    let object = f.arg(ctx, 0)?;
    let message = f.arg(ctx, 1)?;
    let lp = f.arg(ctx, 3)?;
    let wp = f.arg(ctx, 2)?;
    let result = ctx.arg_u32(4)?;
    let slot = match message {
        0x2b => Some(46),
        0x2c if list => Some(47),
        0x39 if list => Some(48),
        0x2d if list => Some(49),
        0x2e if list => Some(50),
        0x2f if list => Some(51),
        _ => None,
    };
    if let Some(slot) = slot {
        let returns_value = matches!(slot, 48 | 50 | 51);
        if returns_value {
            ctx.cpu.check_guest_access(result, 4, Prot::WRITE)?;
            f.set(ctx, 0, result)?;
        }
        let target = virtual_function(ctx, object, slot)?;
        let args = if slot >= 50 {
            [object, wp & 0xffff, wp >> 16]
        } else {
            [object, lp, 0]
        };
        return f.call_args(
            ctx,
            if returns_value { 3 } else { 1 },
            target,
            &args[..if slot >= 50 { 3 } else { 2 }],
        );
    }
    f.call_service(
        ctx,
        2,
        super::control_notifications::CHILD,
        &[object, message, wp, lp, result],
    )
}
fn empty_draw_item(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Retail Draw/Measure/Delete bases are empty and CompareItem returns zero.
    // Real owner-drawn subclasses still run their actual virtual callbacks.
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod list_tests {
    use super::super::archive::tests::Fixture;
    use super::super::window_dispatch::tests::{drive_with, entry};
    use pocket_cpu::Cpu;
    #[test]
    fn list_reflection_calls_real_virtual_slots_and_preserves_the_fifth_argument_across_callbacks()
    {
        let mut f = Fixture::new(&[]);
        f.call("ord:206", &[0x2400]);
        let table = f.word(0x2400);
        let base = f.cpu.read_mem(table, 46 * 4).unwrap();
        f.cpu.write_mem(0x3000, &[0; 52 * 4]).unwrap();
        f.cpu.write_mem(0x3000, &base).unwrap();
        for slot in 46..52 {
            f.cpu
                .write_mem(0x3000 + slot * 4, &(0x6100u32 + slot * 4).to_le_bytes())
                .unwrap();
        }
        f.cpu.write_mem(0x2400, &0x3000u32.to_le_bytes()).unwrap();
        for (msg, slot, returns) in [
            (0x2bu32, 46, false),
            (0x2c, 47, false),
            (0x2d, 49, false),
            (0x39, 48, true),
            (0x2e, 50, true),
            (0x2f, 51, true),
        ] {
            f.cpu
                .write_mem(0x2800, &0x12345678u32.to_le_bytes())
                .unwrap();
            for (i, v) in [0x2400, msg, 0xfffe1234, 0x2900].into_iter().enumerate() {
                f.cpu.write_arg(i as u8, v).unwrap();
            }
            let sp = f.cpu.read_reg(pocket_cpu::regs::ArmReg::Sp).unwrap();
            let offset = f.cpu.stack_arg_offset();
            f.cpu
                .write_mem(sp + offset, &0x2800u32.to_le_bytes())
                .unwrap();
            let mut calls = 0;
            let result = drive_with(
                &mut f.cpu,
                &mut f.kernel,
                &mut f.dispatcher,
                entry(1817),
                256,
                |pc, cpu, _, _| {
                    assert_eq!(pc, 0x6100 + slot * 4);
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2400);
                    assert_eq!(
                        cpu.read_arg(1).unwrap(),
                        if slot >= 50 { 0x1234 } else { 0x2900 }
                    );
                    if slot >= 50 {
                        assert_eq!(cpu.read_arg(2).unwrap(), 0xfffe);
                    }
                    cpu.write_mem(sp + offset, &0xdead0000u32.to_le_bytes())
                        .unwrap();
                    calls += 1;
                    0xfffffff9
                },
            );
            assert_eq!(result, 1);
            assert_eq!(calls, 1);
            assert_eq!(
                f.word(0x2800),
                if returns { 0xfffffff9 } else { 0x12345678 }
            );
        }
    }
}
