//! Regressions for CERF SampleApp's startup and the CE6 common-control ABI.
use super::*;
use pocket_cpu::Cpu;
use pocket_kernel::controls::{TREE_FIRST, TREE_LAST, TREE_ROOT};

fn args(ctx: &mut CallCtx<'_>, values: &[u32]) {
    for (i, value) in values.iter().copied().enumerate() {
        if i < 4 {
            ctx.cpu.write_arg(i as u8, value).unwrap();
        } else {
            let sp = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::Sp).unwrap();
            let at = sp + ctx.cpu.stack_arg_offset() + (i as u32 - 4) * 4;
            ctx.cpu.write_mem(at, &value.to_le_bytes()).unwrap();
        }
    }
}
fn words(ctx: &mut CallCtx<'_>, ptr: u32, values: &[u32]) {
    ctx.cpu
        .write_mem(
            ptr,
            &values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn wide(ctx: &mut CallCtx<'_>, ptr: u32, value: &str) {
    ctx.cpu
        .write_mem(
            ptr,
            &value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn tree_item(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    parent: u32,
    after: u32,
    label: &str,
    data: u32,
) -> u32 {
    wide(ctx, 0x1800, label);
    words(
        ctx,
        0x1700,
        &[parent, after, 5, 0, 0, 0, 0x1800, 64, 0, 0, 0, data],
    );
    message(ctx, hwnd, 0x1132, 0, 0x1700).unwrap().unwrap()
}

#[test]
fn sampleapp_common_classes_do_not_reenter_main_create_and_unknown_ce_class_fails() {
    let mut cpu = super::super::tests::scanf_cpu();
    let mut kernel = super::super::tests::fresh_kernel();
    kernel.ce.windows.enabled = true;
    kernel.wnd_proc = 0x11a88;
    let thunk = super::super::tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for (name, expected) in [
        ("systreeview32", ControlClass::TreeView),
        ("SysHeader32", ControlClass::Header),
        ("MSCTLS_UPDOWN32", ControlClass::UpDown),
    ] {
        wide(&mut ctx, 0x1800, name);
        args(
            &mut ctx,
            &[
                0, 0x1800, 0, 0x50000000, 3, 4, 80, 100, FAKE_HWND, 201, 0, 0,
            ],
        );
        let DispatchOutcome::ReturnedR0(hwnd) = create_window_ex_w(&mut ctx).unwrap() else {
            panic!("system class entered guest WM_CREATE");
        };
        assert_ne!(hwnd, 0);
        assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().class, expected);
        assert!(!ctx.kernel.window_procs.contains_key(&hwnd));
        assert!(ctx.kernel.create_frame.is_none());
    }
    let count = ctx.kernel.controls.children_of(FAKE_HWND).len();
    wide(&mut ctx, 0x1800, "UnsupportedSampleControl");
    args(
        &mut ctx,
        &[0, 0x1800, 0, 0x50000000, 0, 0, 10, 10, FAKE_HWND, 202, 0, 0],
    );
    assert_eq!(
        create_window_ex_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(ctx.kernel.thread_last_errors.get(&0), Some(&1407));
    assert_eq!(ctx.kernel.controls.children_of(FAKE_HWND).len(), count);
}

#[test]
fn tree_unicode_hierarchy_visible_navigation_and_deleted_handles() {
    let mut cpu = super::super::tests::scanf_cpu();
    let mut kernel = super::super::tests::fresh_kernel();
    let hwnd = kernel.controls.create(
        FAKE_HWND,
        ControlClass::TreeView,
        9,
        String::new(),
        0x50000001,
        0,
        0,
        200,
        80,
    );
    let thunk = super::super::tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let root = tree_item(&mut ctx, hwnd, TREE_ROOT, TREE_LAST, "Windows", 0x11223344);
    let second = tree_item(&mut ctx, hwnd, root, TREE_LAST, "Бета", 0x55667788);
    let first = tree_item(&mut ctx, hwnd, root, TREE_FIRST, "Альфа", 0x99);
    assert_eq!(message(&mut ctx, hwnd, 0x1105, 0, 0).unwrap(), Some(3));
    assert_eq!(
        message(&mut ctx, hwnd, 0x110a, 4, root).unwrap(),
        Some(first)
    );
    assert_eq!(
        message(&mut ctx, hwnd, 0x110a, 1, first).unwrap(),
        Some(second)
    );
    assert_eq!(
        ctx.kernel.controls.get(hwnd).unwrap().tree.visible(),
        vec![(root, 0)]
    );
    assert_eq!(
        dispatch(&mut ctx, hwnd, 0x1102, 2, root, false).unwrap(),
        Some(DispatchOutcome::ReturnedR0(1))
    );
    assert_eq!(
        message(&mut ctx, hwnd, 0x110a, 6, root).unwrap(),
        Some(first)
    );
    words(&mut ctx, 0x1700, &[5, second, 0, 0, 0x1800, 64, 0, 0, 0, 0]);
    assert_eq!(message(&mut ctx, hwnd, 0x113e, 0, 0x1700).unwrap(), Some(1));
    assert_eq!(ctx.cpu.read_u32_le(0x1724).unwrap(), 0x55667788);
    assert_eq!(
        String::from_utf16_lossy(&read_wstr(&mut ctx, 0x1800, 64).unwrap()),
        "Бета"
    );
    words(&mut ctx, 0x1900, &[second, 0, 0, 0]);
    assert_eq!(message(&mut ctx, hwnd, 0x1104, 0, 0x1900).unwrap(), Some(1));
    assert_eq!(ctx.cpu.read_u32_le(0x1904).unwrap(), 40);
    assert_eq!(message(&mut ctx, hwnd, 0x1101, 0, root).unwrap(), Some(1));
    assert_eq!(message(&mut ctx, hwnd, 0x1105, 0, 0).unwrap(), Some(0));
    assert_eq!(message(&mut ctx, hwnd, 0x113e, 0, 0x1700).unwrap(), Some(0));
    assert_ne!(
        tree_item(&mut ctx, hwnd, TREE_ROOT, TREE_LAST, "New", 0),
        root
    );
}

#[test]
fn tree_selection_veto_notifications_and_reentrant_callback_restore_original_api() {
    for veto in [false, true] {
        let mut cpu = super::super::tests::scanf_cpu();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let mut kernel = super::super::tests::fresh_kernel();
        kernel.window_procs.insert(FAKE_HWND, 0x11480);
        let hwnd = kernel.controls.create(
            FAKE_HWND,
            ControlClass::TreeView,
            201,
            String::new(),
            0x50000001,
            0,
            0,
            200,
            80,
        );
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let item = tree_item(&mut ctx, hwnd, TREE_ROOT, TREE_LAST, "Files", 0x11223344);
        let original = [hwnd, 0x110b, 9, item];
        args(&mut ctx, &original);
        ctx.cpu.write_reg(ArmReg::Lr, 0x8888).unwrap();
        assert_eq!(
            dispatch(&mut ctx, hwnd, 0x110b, 9, item, false).unwrap(),
            Some(DispatchOutcome::JumpTo(0x11480))
        );
        let ptr = ctx.cpu.read_reg(ArmReg::R3).unwrap();
        assert_eq!(ctx.cpu.read_u32_le(ptr + 8).unwrap(), (-450i32) as u32);
        assert_eq!(ctx.cpu.read_u32_le(ptr + 56 + 4).unwrap(), item);
        assert_eq!(ctx.cpu.read_u32_le(ptr + 56 + 36).unwrap(), 0x11223344);
        // A nested SendMessage from the parent cannot consume the outer frame.
        ctx.cpu.write_reg(ArmReg::Sp, 0x8fc0).unwrap();
        assert_eq!(resume(&mut ctx).unwrap(), None);
        ctx.cpu.write_reg(ArmReg::Sp, 0x9000).unwrap();
        ctx.cpu.write_reg(ArmReg::R0, u32::from(veto)).unwrap();
        let outcome = resume(&mut ctx).unwrap();
        if veto {
            assert_eq!(outcome, Some(DispatchOutcome::ReturnedR0(0)));
            assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().tree.selected, 0);
        } else {
            assert_eq!(outcome, Some(DispatchOutcome::JumpTo(0x11480)));
            assert_eq!(ctx.cpu.read_u32_le(ptr + 8).unwrap(), (-451i32) as u32);
            assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().tree.selected, item);
            ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
            assert_eq!(
                resume(&mut ctx).unwrap(),
                Some(DispatchOutcome::ReturnedR0(1))
            );
        }
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x8888);
        for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
            .into_iter()
            .zip(original)
        {
            assert_eq!(ctx.cpu.read_reg(reg).unwrap(), value);
        }
        assert!(ctx.kernel.controls.common_control_frames.is_empty());
    }
}

#[test]
fn header_order_and_updown_signed_range_buddy_and_veto() {
    let mut cpu = super::super::tests::scanf_cpu();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut kernel = super::super::tests::fresh_kernel();
    let header = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Header,
        203,
        String::new(),
        0x50000000,
        0,
        0,
        200,
        20,
    );
    let buddy = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Edit,
        204,
        String::new(),
        0x50000000,
        0,
        30,
        100,
        20,
    );
    let updown = kernel.controls.create(
        FAKE_HWND,
        ControlClass::UpDown,
        205,
        String::new(),
        0x50000023,
        100,
        30,
        20,
        20,
    );
    let thunk = super::super::tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for (index, width) in [(0, 60), (1, 80)] {
        wide(&mut ctx, 0x1800, "Name");
        words(&mut ctx, 0x1700, &[3, width, 0x1800, 0, 64, 0, 0]);
        assert_eq!(
            message(&mut ctx, header, 0x120a, index, 0x1700).unwrap(),
            Some(index)
        );
    }
    words(&mut ctx, 0x1900, &[1, 0]);
    assert_eq!(
        message(&mut ctx, header, 0x1212, 2, 0x1900).unwrap(),
        Some(1)
    );
    assert_eq!(
        message(&mut ctx, header, 0x1207, 0, 0x1900).unwrap(),
        Some(1)
    );
    assert_eq!(ctx.cpu.read_u32_le(0x1900).unwrap(), 80);
    words(&mut ctx, 0x1900, &[0, 0]);
    assert_eq!(
        message(&mut ctx, header, 0x1212, 2, 0x1900).unwrap(),
        Some(0)
    );
    assert_eq!(message(&mut ctx, header, 0x120f, 0, 0).unwrap(), Some(1));
    message(&mut ctx, updown, 0x469, buddy, 0).unwrap();
    message(&mut ctx, updown, 0x46f, (-5i32) as u32, 5).unwrap();
    message(&mut ctx, updown, 0x471, 0, 5).unwrap();
    assert_eq!(ctx.kernel.controls.get(buddy).unwrap().text, "5");
    assert_eq!(
        dispatch(&mut ctx, updown, CHANGE, (-722i32) as u32, 1, false).unwrap(),
        Some(DispatchOutcome::ReturnedR0(0))
    );
    assert_eq!(ctx.kernel.controls.get(buddy).unwrap().text, "-5");
    ctx.kernel.window_procs.insert(FAKE_HWND, 0x11480);
    args(&mut ctx, &[updown, CHANGE, (-722i32) as u32, 1]);
    ctx.cpu.write_reg(ArmReg::Lr, 0x9999).unwrap();
    assert_eq!(
        dispatch(&mut ctx, updown, CHANGE, (-722i32) as u32, 1, false).unwrap(),
        Some(DispatchOutcome::JumpTo(0x11480))
    );
    let ptr = ctx.cpu.read_reg(ArmReg::R3).unwrap();
    assert_eq!(ctx.cpu.read_u32_le(ptr + 12).unwrap() as i32, -5);
    assert_eq!(ctx.cpu.read_u32_le(ptr + 16).unwrap(), 1);
    ctx.cpu.write_reg(ArmReg::R0, 1).unwrap();
    assert_eq!(
        resume(&mut ctx).unwrap(),
        Some(DispatchOutcome::ReturnedR0(0))
    );
    assert_eq!(ctx.kernel.controls.get(buddy).unwrap().text, "-5");
    assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x9999);
    ctx.kernel.controls.get_mut(buddy).unwrap().text = "invalid".into();
    message(&mut ctx, updown, 0x472, 0, 0x1900).unwrap();
    assert_eq!(ctx.cpu.read_u32_le(0x1900).unwrap(), 1);
}
