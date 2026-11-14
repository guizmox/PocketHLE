use super::tests::{dummy_thunk, fresh_kernel, scanf_cpu};
use super::*;
use pocket_cpu::Cpu;

#[test]
fn run_combo_committed_text_reaches_the_edit_and_notifies_its_native_dialog() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
    let parent = 0xdead1000;
    kernel.active_window = parent;
    kernel.window_rects.insert(parent, (0, 0, 240, 200));
    kernel.window_styles.insert(parent, (0x10000000, 0));
    kernel.window_procs.insert(parent, 0x12000);
    let hwnd = kernel.controls.create(
        parent,
        ControlClass::ComboBox,
        101,
        String::new(),
        0x50010002,
        10,
        100,
        200,
        120,
    );
    kernel.controls.focus = hwnd;
    assert!(kernel.accepts_text_input());
    let history: Vec<_> = "\\Previous".encode_utf16().chain(Some(0)).collect();
    cpu.write_mem(0x3000, &wide_to_bytes(&history)).unwrap();
    let thunk = dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        control_message(&mut ctx, hwnd, 0x143, 0, 0x3000).unwrap(),
        0
    );
    assert_eq!(control_message(&mut ctx, hwnd, 0x14e, 0, 0).unwrap(), 0);
    assert_eq!(
        ctx.kernel.controls.get(hwnd).unwrap().window_text(),
        "\\Previous"
    );
    ctx.kernel
        .pending_input
        .push_back(InputEvent::Text { character: 'Я' });
    assert_eq!(
        next_input_message(&mut ctx),
        Some((hwnd, 0x102, 'Я' as u32, 1))
    );
    let original_sp = 0x8000;
    let original_lr = 0x11223344;
    ctx.cpu.write_reg(ArmReg::Sp, original_sp).unwrap();
    ctx.cpu.write_reg(ArmReg::Lr, original_lr).unwrap();
    for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip([hwnd, 0x102, 'Я' as u32, 1])
    {
        ctx.cpu.write_reg(reg, value).unwrap();
    }
    for code in [6, 5] {
        assert_eq!(
            def_window_proc_w(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(0x12000)
        );
        assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), parent);
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), WM_COMMAND);
        assert_eq!(ctx.cpu.read_reg(ArmReg::R2).unwrap(), (code << 16) | 101);
        assert_eq!(ctx.cpu.read_reg(ArmReg::R3).unwrap(), hwnd);
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
    }
    assert_eq!(
        def_window_proc_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), original_sp);
    assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), original_lr);
    assert!(ctx.kernel.control_notify_frames.is_empty());
    assert_eq!(
        ctx.kernel.controls.get(hwnd).unwrap().window_text(),
        "\\PreviousЯ"
    );
    assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().list_selected, None);
    assert_eq!(
        control_message(&mut ctx, hwnd, 0x87, 0x0d, 0).unwrap() & 4,
        0
    );
    ctx.kernel.controls.get_mut(hwnd).unwrap().choice_open = true;
    assert_eq!(
        control_message(&mut ctx, hwnd, 0x87, 0x0d, 0).unwrap() & 4,
        4
    );
    ctx.cpu.write_arg(0, hwnd).unwrap();
    ctx.cpu.write_arg(1, 0x4000).unwrap();
    ctx.cpu.write_arg(2, 64).unwrap();
    assert_eq!(
        get_window_text_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(10)
    );
    assert_eq!(
        ctx.cpu.read_mem(0x4000, 22).unwrap(),
        wide_to_bytes(
            &"\\PreviousЯ"
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>()
        )
    );
}

#[test]
fn combo_text_intent_respects_kind_disabled_ancestors_and_active_window() {
    let mut kernel = fresh_kernel();
    kernel.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
    let parent = 0xdead1000;
    kernel.active_window = parent;
    kernel.window_styles.insert(parent, (0x10000000, 0));
    let hwnd = kernel.controls.create(
        parent,
        ControlClass::ComboBox,
        1,
        String::new(),
        0x50010002,
        0,
        0,
        120,
        80,
    );
    kernel.controls.focus = hwnd;
    assert_eq!(kernel.text_input_target(), Some(hwnd));
    kernel.controls.get_mut(hwnd).unwrap().style |= 3;
    assert_eq!(kernel.text_input_target(), None);
    kernel.controls.get_mut(hwnd).unwrap().style &= !1;
    kernel.controls.get_mut(hwnd).unwrap().enabled = false;
    assert_eq!(kernel.text_input_target(), None);
    kernel.controls.get_mut(hwnd).unwrap().enabled = true;
    kernel.window_styles.get_mut(&parent).unwrap().0 |= 0x08000000;
    assert_eq!(kernel.text_input_target(), None);
    kernel.window_styles.get_mut(&parent).unwrap().0 &= !0x08000000;
    kernel.window_visibility.insert(parent, false);
    assert_eq!(kernel.text_input_target(), None);
    kernel.window_visibility.insert(parent, true);
    kernel.active_window = 0xdead2000;
    assert_eq!(kernel.text_input_target(), None);
    kernel.active_window = parent;
    assert_eq!(kernel.text_input_target(), Some(hwnd));
}
