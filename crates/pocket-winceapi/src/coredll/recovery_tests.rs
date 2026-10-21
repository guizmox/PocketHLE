//! Focused regressions for the reconstructed SimCity fixes.
use super::*;
use pocket_cpu::Cpu;
#[test]
fn menu_resource_is_ordered_and_enumeration_ends() {
    let bytes: Vec<_> = [0u16, 0, 0x80, 123, 65, 0]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut cpu = tests::scanf_cpu();
    let mut k = tests::fresh_kernel();
    let thunk = tests::dummy_thunk();
    cpu.write_mem(0x1000, &bytes).unwrap();
    cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut k,
        thunk: &thunk,
    };
    let DispatchOutcome::ReturnedR0(h) = menu_api::indirect(&mut ctx).unwrap() else {
        panic!()
    };
    assert_ne!(h, 0);
    assert_eq!(ctx.kernel.menu_items[&h][0].id, 123);
    ctx.cpu.write_mem(0x1200, &[0u8; 44]).unwrap();
    ctx.cpu.write_mem(0x1200, &44u32.to_le_bytes()).unwrap();
    ctx.cpu.write_mem(0x1204, &2u32.to_le_bytes()).unwrap();
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip([h, 0, 1, 0x1200])
    {
        ctx.cpu.write_reg(r, v).unwrap();
    }
    assert_eq!(
        menu_api::get_info(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.cpu.read_u32_le(0x1210).unwrap(), 123);
    ctx.cpu.write_reg(ArmReg::R1, 1).unwrap();
    assert_eq!(
        menu_api::get_info(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
}
#[test]
fn native_parent_translation_reaches_control_hit_testing() {
    let mut k = tests::fresh_kernel();
    let parent = 0xdead1000;
    let view = 0xdead1001;
    k.window_rects.insert(parent, (20, 30, 240, 320));
    k.window_styles.insert(parent, (0x10000000, 0));
    k.window_rects.insert(view, (5, 40, 200, 200));
    k.window_styles.insert(view, (0x50000000, 0));
    k.window_parents.insert(view, parent);
    let bar = k.controls.create(
        view,
        ControlClass::ScrollBar,
        517,
        String::new(),
        0x50000001,
        3,
        7,
        16,
        100,
    );
    k.refresh_control_visibility();
    assert_eq!(k.window_client_origin(bar), Some((28, 77)));
    assert_eq!(k.controls.screen_rect(bar), Some((28, 77, 16, 100)));
    assert_eq!(k.controls.hit_test(29, 78), Some(bar));
}
#[test]
fn textmetric_height_matches_selected_renderer() {
    let f = pocket_kernel::gdi::Font::new(-16);
    let m = pocket_kernel::text::metrics_w(Some(&f));
    let l = pocket_kernel::text::layout(Some(&f), &[65]);
    assert_eq!(i32::from_le_bytes(m[..4].try_into().unwrap()), l.height);
    assert_eq!(m[55] & 4, 4);
}
