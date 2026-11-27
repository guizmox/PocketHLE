//! CE CWnd's +0x44 flag is ShowDoneButton ownership, not tooltip storage.
//! The top-level frame has one owner (+0xc8); styling and input use shell/USER.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const SHOW: &str = "__PocketHLE_MFC_ShowDoneButton";
pub(super) const SHELL: &str = "__PocketHLE_MFC_ShellDoneButton";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] =
    &[(2784, "?ShowDoneButton@CWnd@@QAAHH@Z", show)];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", SHOW, show);
    d.register_handler("coredll.dll", SHELL, crate::aygshell::done_button);
}
fn show(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let enabled = f.arg(ctx, 1)? != 0;
    if f.fresh {
        return f.call_service(ctx, 1, super::hierarchy::TOP, &[o]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 0, result)?;
            if enabled {
                let previous = word(ctx, result + 0xc8)?;
                if previous != 0 {
                    put(ctx, previous + 0x44, 0)?;
                }
                let hwnd = word(ctx, result + 0x20)?;
                return f.call_service(ctx, 4, SHELL, &[hwnd, 1]);
            }
            f.call_service(ctx, 2, super::pretranslation::MAIN, &[])
        }
        2 => {
            let hwnd = if result == 0 {
                0
            } else {
                word(ctx, result + 0x20)?
            };
            f.call_service(ctx, 3, "SetForegroundWindow", &[hwnd])
        }
        3 => {
            let frame = f.get(ctx, 0)?;
            put(ctx, o + 0x44, 0)?;
            put(ctx, frame + 0xc8, 0)?;
            let hwnd = word(ctx, frame + 0x20)?;
            f.call_service(ctx, 4, SHELL, &[hwnd, 2])
        }
        4 => {
            if result != 0 && enabled {
                let frame = f.get(ctx, 0)?;
                put(ctx, frame + 0xc8, o)?;
                put(ctx, o + 0x44, 1)?;
            }
            f.finish(ctx, u32::from(result != 0))
        }
        _ => Err(KernelError::Dispatch(
            "invalid ShowDoneButton continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::framework_setup;
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use pocket_cpu::Cpu;
    #[test]
    fn done_button_moves_between_views_and_destroy_unregisters_actual_caption_owner() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let run = |cpu: &mut dyn pocket_cpu::Cpu,
                   k: &mut pocket_kernel::KernelState,
                   d: &mut WinCeDispatcher,
                   n,
                   args: &[u32]| {
            for (i, &v) in args.iter().enumerate() {
                cpu.write_arg(i as u8, v).unwrap();
            }
            drive(cpu, k, d, entry(n), |pc, _| {
                panic!("unexpected guest {pc:x}")
            })
        };
        run(&mut cpu, &mut k, &mut d, 103, &[0x2000]);
        for (hwnd, object) in [(7, 0x2000), (8, 0x2400), (9, 0x2500)] {
            if hwnd != 7 {
                run(&mut cpu, &mut k, &mut d, 206, &[object]);
                k.window_parents.insert(hwnd, 7);
            }
            k.window_classes.insert(hwnd, "Window".into());
            k.window_styles
                .insert(hwnd, (if hwnd == 7 { 0 } else { 0x40000000 }, 0));
            assert_eq!(run(&mut cpu, &mut k, &mut d, 804, &[object, hwnd]), 1);
        }
        assert_eq!(run(&mut cpu, &mut k, &mut d, 2784, &[0x2400, 1]), 1);
        assert_eq!(cpu.read_u32_le(0x2044).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2444).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(0x20c8).unwrap(), 0x2400);
        assert_ne!(k.window_styles[&7].1 & 0x80000000, 0);
        assert_eq!(run(&mut cpu, &mut k, &mut d, 2784, &[0x2500, 1]), 1);
        assert_eq!(cpu.read_u32_le(0x2444).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2544).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(0x20c8).unwrap(), 0x2500);
        run(&mut cpu, &mut k, &mut d, 1872, &[0x2500]);
        assert_eq!(cpu.read_u32_le(0x2544).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x20c8).unwrap(), 0);
        assert_eq!(k.window_styles[&7].1 & 0x80000000, 0);
    }
}
