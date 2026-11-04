//! Execute real ARM/Thumb callbacks, rather than simulating their return in Rust.
use super::*;
use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};
const RETURN: u32 = 0x70000000;

fn thunk(name: &str) -> pocket_kernel::Thunk {
    pocket_kernel::Thunk {
        thunk_va: RETURN,
        iat_va: 0,
        dll: "coredll.dll".into(),
        binding: pocket_pe::ImportBinding::Name(name.into()),
        friendly_name: Some(name.into()),
    }
}
fn setup(code: &[u8]) -> (UnicornCpu, pocket_kernel::KernelState, WinCeDispatcher) {
    let mut cpu = UnicornCpu::new().unwrap();
    cpu.map_region(0x1000, 0x4000, Prot::ALL).unwrap();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    cpu.map_region(RETURN, 0x1000, Prot::ALL).unwrap();
    cpu.write_mem(RETURN, &0xe12fff1eu32.to_le_bytes()).unwrap();
    cpu.add_code_hook(RETURN).unwrap();
    cpu.write_mem(0x2000, code).unwrap();
    cpu.write_reg(ArmReg::Sp, 0x4000).unwrap();
    cpu.write_reg(ArmReg::Lr, 0x1234).unwrap();
    (
        cpu,
        crate::gx::tests::fresh_kernel(),
        WinCeDispatcher::new(),
    )
}
fn args(cpu: &mut dyn Cpu, values: [u32; 4]) {
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(values)
    {
        cpu.write_reg(r, v).unwrap();
    }
}
#[test]
fn ce_font_callback_executes_arm_and_thumb_and_restores_original_continuation() {
    let arm = [0xe590c01cu32, 0xe583c000, 0xe3a00000, 0xe12fff1e]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let thumb = [0x69c0u16, 0x6018, 0x2000, 0x4770]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    for (code, entry) in [(&arm, 0x2000), (&thumb, 0x2001)] {
        let (mut cpu, mut kernel, mut dispatcher) = setup(code);
        args(
            &mut cpu,
            [pocket_kernel::gdi::GDI_SCREEN_DC, 0, entry, 0x1300],
        );
        let t = thunk("EnumFontFamiliesW");
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::JumpTo(entry)
        );
        assert_eq!(
            cpu.run_until_hook(entry, 100).unwrap(),
            StopReason::Hook(RETURN)
        );
        // The first callback receives Arial's UTF-16 LOGFONT face name.
        assert_eq!(cpu.read_u32_le(0x1300).unwrap(), 0x00720041);
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert!(kernel.gdi.font_enum_frames.is_empty());
    }
}
#[test]
fn ce_notify_callback_executes_arm_and_thumb_and_ignores_callback_result() {
    let arm = [0xe5831000u32, 0xe3a00000, 0xe12fff1e]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let thumb = [0x6019u16, 0x2000, 0x4770]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    for (code, entry) in [(&arm, 0x2000), (&thumb, 0x2001)] {
        let (mut cpu, mut kernel, mut dispatcher) = setup(code);
        kernel.window_procs.insert(0x111, entry);
        kernel.window_threads.insert(0x111, 0);
        args(&mut cpu, [0x111, 0x400, 0, 0x1300]);
        let t = thunk("SendNotifyMessageW");
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::JumpTo(entry)
        );
        assert_eq!(
            cpu.run_until_hook(entry, 100).unwrap(),
            StopReason::Hook(RETURN)
        );
        assert_eq!(cpu.read_u32_le(0x1300).unwrap(), 0x400);
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert!(kernel.notify_message_frames.is_empty());
    }
}
