//! WM_NOTIFY uses the same child reflection and typed command-map ABI as USER.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const BASE: &str = "__PocketHLE_MFC_WindowNotify";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] =
    &[(1966, "?OnNotify@CWnd@@MAAHIJPAJ@Z", notify)];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", BASE, notify);
}
fn notify(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let header = f.arg(ctx, 2)?;
    let out = f.arg(ctx, 3)?;
    if f.fresh {
        ctx.cpu.check_guest_access(header, 12, Prot::READ)?;
        ctx.cpu
            .check_guest_access(out, 4, Prot::READ | Prot::WRITE)?;
        let hwnd = word(ctx, header)?;
        f.set(ctx, 0, hwnd)?;
        return f.call_service(ctx, 1, "GetDlgCtrlID", &[hwnd]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            f.set(ctx, 1, result & 0xffff)?;
            let state = super::application::current_thread_state(ctx)?;
            let hwnd = word(ctx, o + 0x20)?;
            if word(ctx, state + 0x120)? == hwnd {
                return f.finish(ctx, 1);
            }
            let from = f.get(ctx, 0)?;
            if let Some(child) = super::windows::permanent(ctx, from)? {
                let wp = f.arg(ctx, 1)?;
                return f.call_virtual(ctx, 2, child, 42, &[0x4e, wp, header, out]);
            }
            route(ctx, &f, o, header, out)
        }
        2 => {
            if result != 0 {
                return f.finish(ctx, 1);
            }
            route(ctx, &f, o, header, out)
        }
        3 => f.finish(ctx, result),
        _ => Err(KernelError::Dispatch(
            "invalid window notification continuation".into(),
        )),
    }
}
fn route(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    o: u32,
    header: u32,
    out: u32,
) -> Result<DispatchOutcome, KernelError> {
    let code = word(ctx, header + 8)? & 0xffff;
    let id = f.get(ctx, 1)?;
    put(ctx, f.address + 128, out)?;
    put(ctx, f.address + 132, header)?;
    f.call_virtual(ctx, 3, o, 5, &[id, 0x4e0000 | code, f.address + 128, 0])
}

#[cfg(test)]
mod tests {
    use crate::mfc::tests::framework_setup;
    use crate::mfc::window_dispatch::tests::{drive, drive_outcomes, entry};
    use pocket_cpu::Cpu;
    use pocket_kernel::DispatchOutcome;
    #[test]
    fn notify_reflects_to_the_real_child_then_routes_the_native_notify_pair_if_unhandled() {
        for reflected in [false, true] {
            let (mut cpu, mut k, mut d) = framework_setup();
            for (object, hwnd, copy, slot, pc) in [
                (0x2000u32, 7u32, 0x2400u32, 5u32, 0x6000u32),
                (0x2200, 9, 0x2600, 42, 0x6100),
            ] {
                cpu.write_arg(0, object).unwrap();
                drive(&mut cpu, &mut k, &mut d, entry(206), |pc, _| {
                    panic!("unexpected {pc:x}")
                });
                k.window_classes.insert(hwnd, "NotifyOwner".into());
                k.window_ids.insert(hwnd, 33);
                cpu.write_arg(0, object).unwrap();
                cpu.write_arg(1, hwnd).unwrap();
                drive(&mut cpu, &mut k, &mut d, entry(804), |pc, _| {
                    panic!("unexpected {pc:x}")
                });
                let table = cpu.read_u32_le(object).unwrap();
                let bytes = cpu.read_mem(table, 46 * 4).unwrap();
                cpu.write_mem(copy, &bytes).unwrap();
                cpu.write_mem(copy + slot * 4, &pc.to_le_bytes()).unwrap();
                cpu.write_mem(object, &copy.to_le_bytes()).unwrap();
            }
            for (i, value) in [9u32, 0xabcdef, 0xfffffff4].into_iter().enumerate() {
                cpu.write_mem(0x3000 + i as u32 * 4, &value.to_le_bytes())
                    .unwrap();
            }
            for (i, value) in [0x2000, 127, 0x3000, 0x3100].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            let mut reflection_calls = 0;
            let mut command_calls = 0;
            assert_eq!(
                drive_outcomes(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    entry(1966),
                    4096,
                    |pc, cpu, _, _| {
                        match pc {
                            0x6100 => {
                                reflection_calls += 1;
                                assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                                assert_eq!(cpu.read_arg(1).unwrap(), 0x4e);
                                assert_eq!(cpu.read_arg(3).unwrap(), 0x3000);
                                if reflected {
                                    cpu.write_mem(0x3100, &68u32.to_le_bytes()).unwrap();
                                }
                                DispatchOutcome::ReturnedR0(u32::from(reflected))
                            }
                            0x6000 => {
                                command_calls += 1;
                                assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                                assert_eq!(cpu.read_arg(1).unwrap(), 33);
                                assert_eq!(cpu.read_arg(2).unwrap(), 0x4efff4);
                                let pair = cpu.read_arg(3).unwrap();
                                assert_eq!(cpu.read_u32_le(pair).unwrap(), 0x3100);
                                assert_eq!(cpu.read_u32_le(pair + 4).unwrap(), 0x3000);
                                cpu.write_mem(0x3100, &68u32.to_le_bytes()).unwrap();
                                DispatchOutcome::ReturnedR0(1)
                            }
                            _ => panic!("unexpected guest {pc:x}"),
                        }
                    }
                ),
                1
            );
            assert_eq!(reflection_calls, 1);
            assert_eq!(command_calls, usize::from(!reflected));
            assert_eq!(cpu.read_u32_le(0x3100).unwrap(), 68);
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")));
        }
    }
}
