//! Failure and ABI contracts for the shared EDIT/clipboard owner.
use super::*;
use pocket_cpu::{stub::StubCpu, Cpu};
use pocket_kernel::Thunk;
fn fixture() -> (StubCpu, KernelState, Thunk) {
    let mut cpu = tests::scanf_cpu();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    (cpu, tests::fresh_kernel(), tests::dummy_thunk())
}
#[test]
fn clipboard_oom_preserves_previous_formats_and_does_not_cut_selection() {
    let (mut cpu, mut kernel, thunk) = fixture();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let baseline = ctx.kernel.heap.free_bytes();
    assert!(clipboard::copy_text(&mut ctx, 0, "previous").unwrap());
    let pointer = ctx.kernel.clipboard.formats[&13];
    let mut exhausted = Vec::new();
    while let Some(p) = ctx.kernel.heap.alloc(1) {
        exhausted.push(p);
    }
    assert!(!clipboard::copy_text(&mut ctx, 0, "replacement").unwrap());
    assert_eq!(ctx.kernel.clipboard.formats[&13], pointer);
    assert_eq!(
        clipboard::text(&mut ctx).unwrap().as_deref(),
        Some("previous")
    );
    let h = ctx.kernel.controls.create(
        0,
        ControlClass::Edit,
        12,
        "cut text".into(),
        0x50000080,
        0,
        0,
        100,
        18,
    );
    ctx.kernel.controls.get_mut(h).unwrap().edit.selection = Some((0, 3));
    assert_eq!(control_message(&mut ctx, h, 0x300, 0, 0).unwrap(), 0);
    assert_eq!(ctx.kernel.controls.get(h).unwrap().text, "cut text");
    assert_eq!(
        ctx.kernel.controls.get(h).unwrap().edit.selection,
        Some((0, 3))
    );
    ctx.cpu.write_arg(0, 0).unwrap();
    assert_eq!(
        clipboard::open_clipboard(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    ctx.cpu.write_arg(0, 1).unwrap();
    assert_eq!(
        clipboard::get_clipboard_data(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert!(!ctx.kernel.clipboard.formats.contains_key(&1));
    assert_eq!(
        get_last_error(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(8)
    );
    clipboard::empty_clipboard(&mut ctx).unwrap();
    clipboard::close_clipboard(&mut ctx).unwrap();
    for p in exhausted {
        ctx.kernel.heap.free(p);
    }
    assert_eq!(ctx.kernel.heap.free_bytes(), baseline);
}
#[test]
fn edit_line_messages_read_capacity_without_terminating_or_overwriting_the_tail() {
    let (mut cpu, mut kernel, thunk) = fixture();
    let h = kernel.controls.create(
        7,
        ControlClass::Edit,
        12,
        "a\tb\r\n😀xy\r\nz".into(),
        0x50000084,
        0,
        0,
        100,
        18,
    );
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(control_message(&mut ctx, h, 0xba, 0, 0).unwrap(), 3);
    assert_eq!(control_message(&mut ctx, h, 0xbb, 1, 0).unwrap(), 5);
    assert_eq!(control_message(&mut ctx, h, 0xc1, 6, 0).unwrap(), 4);
    assert_eq!(control_message(&mut ctx, h, 0xc9, 9, 0).unwrap(), 1);
    assert_eq!(control_message(&mut ctx, h, 0xbb, 99, 0).unwrap(), u32::MAX);
    ctx.cpu
        .write_mem(0x2000, &[2, 0, 0xcc, 0xcc, 0xaa, 0xbb])
        .unwrap();
    assert_eq!(control_message(&mut ctx, h, 0xc4, 1, 0x2000).unwrap(), 2);
    assert_eq!(
        ctx.cpu.read_mem(0x2000, 6).unwrap(),
        [0x3d, 0xd8, 0, 0xde, 0xaa, 0xbb]
    );
    assert_eq!(control_message(&mut ctx, h, 0xb6, 0, 100).unwrap(), 1);
    assert_eq!(control_message(&mut ctx, h, 0xce, 0, 0).unwrap(), 2);
    let at = control_message(&mut ctx, h, 0xd7, 0, (2 << 16) | 3).unwrap();
    assert_eq!(at, (2 << 16) | 11);
    let text = ctx.kernel.controls.get(h).unwrap().text.clone();
    ctx.cpu.write_mem(0x2ffe, &[4, 0]).unwrap();
    ctx.cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
    assert!(control_message(&mut ctx, h, 0xc4, 1, 0x2ffe).is_err());
    assert_eq!(ctx.cpu.read_mem(0x2ffe, 2).unwrap(), [4, 0]);
    assert_eq!(ctx.kernel.controls.get(h).unwrap().text, text);
}
#[test]
fn suspended_edit_notifications_restore_without_repeating_mutation_or_losing_caller() {
    let (mut cpu, mut kernel, thunk) = fixture();
    let parent = 0xdead1000;
    let h = kernel.controls.create(
        parent,
        ControlClass::Edit,
        12,
        "".into(),
        0x50000084,
        0,
        0,
        100,
        18,
    );
    kernel.window_procs.insert(parent, 0x11000);
    kernel.window_classes.insert(parent, "FRAME".into());
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for i in 0..32 {
        for (r, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
            .into_iter()
            .zip([h, 0x102, 65, 0])
        {
            ctx.cpu.write_reg(r, value).unwrap();
        }
        ctx.cpu.write_reg(ArmReg::Lr, 0x12345678).unwrap();
        assert_eq!(
            def_window_proc_w(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(0x11000)
        );
        for code in [0x400, 0x300] {
            assert_eq!(ctx.cpu.read_reg(ArmReg::R2).unwrap(), (code << 16) | 12);
            assert_eq!(ctx.kernel.controls.get(h).unwrap().text, "A".repeat(i + 1));
            let saved = ctx.kernel.capture_snapshot().unwrap();
            saved.restore(ctx.kernel).unwrap();
            ctx.cpu.write_reg(ArmReg::R0, 0xbad0).unwrap();
            let returned = def_window_proc_w(&mut ctx).unwrap();
            assert_eq!(
                returned,
                if code == 0x400 {
                    DispatchOutcome::JumpTo(0x11000)
                } else {
                    DispatchOutcome::ReturnedR0(0)
                }
            );
        }
        assert!(ctx.kernel.control_notify_frames.is_empty());
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x12345678);
        assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x9000);
    }
}
