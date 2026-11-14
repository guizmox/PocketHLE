use super::*;
use crate::bluetooth::tests::{call, setup};
use pocket_cpu::Cpu;

#[test]
fn nested_dialog_pages_follow_parent_client_coordinates_and_moves() {
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    kernel.ce.windows.enabled = true;
    kernel.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
    let parent = 0xdead0080;
    kernel.window_classes.insert(parent, "Frame".into());
    kernel.window_styles.insert(parent, (0x10c00000, 0));
    kernel.window_rects.insert(parent, (80, 60, 400, 300));
    kernel.active_window = parent;
    let mut template = [0u8; 64];
    template[..4].copy_from_slice(&0x50000000u32.to_le_bytes());
    template[14..16].copy_from_slice(&80i16.to_le_bytes());
    template[16..18].copy_from_slice(&60i16.to_le_bytes());
    cpu.write_mem(0x1800, &template).unwrap();
    let create = |cpu: &mut _, kernel: &mut _, dispatcher: &mut _, owner| {
        let DispatchOutcome::ReturnedR0(hwnd) = call(
            cpu,
            kernel,
            dispatcher,
            "coredll.dll",
            "CreateDialogIndirectParamW",
            &[0, 0x1800, owner, 0, 0],
        ) else {
            panic!("modeless dialog did not return its handle")
        };
        assert_ne!(hwnd, 0);
        hwnd
    };
    let page = create(&mut cpu, &mut kernel, &mut dispatcher, parent);
    assert_eq!(kernel.window_rects[&page].1, 0);
    call(
        &mut cpu,
        &mut kernel,
        &mut dispatcher,
        "coredll.dll",
        "MoveWindow",
        &[page, 9, 0, 180, 100, 0],
    );
    assert_eq!(kernel.window_rects[&page], (9, 0, 180, 100));
    assert_eq!(kernel.window_client_origin(page), Some((92, 83)));
    let button = kernel.controls.create(
        page,
        ControlClass::Button,
        42,
        "Option".into(),
        0x50000000,
        5,
        7,
        30,
        16,
    );
    kernel.refresh_control_visibility();
    assert_eq!(kernel.controls.screen_rect(button), Some((97, 90, 30, 16)));
    assert_eq!(kernel.controls.hit_test(100, 94), Some(button));

    let nested = create(&mut cpu, &mut kernel, &mut dispatcher, page);
    call(
        &mut cpu,
        &mut kernel,
        &mut dispatcher,
        "coredll.dll",
        "SetWindowPos",
        &[nested, 0, (-3i32) as u32, (-4i32) as u32, 80, 50, 0x14],
    );
    assert_eq!(kernel.window_rects[&nested], (-3, -4, 80, 50));
    assert_eq!(kernel.window_client_origin(nested), Some((89, 79)));

    call(
        &mut cpu,
        &mut kernel,
        &mut dispatcher,
        "coredll.dll",
        "MoveWindow",
        &[parent, 130, 100, 400, 300, 0],
    );
    kernel.refresh_control_visibility();
    assert_eq!(kernel.window_client_origin(page), Some((142, 123)));
    assert_eq!(kernel.window_client_origin(nested), Some((139, 119)));
    assert_eq!(kernel.window_rects[&page], (9, 0, 180, 100));
    assert_eq!(kernel.window_rects[&nested], (-3, -4, 80, 50));
    assert_eq!(
        kernel.controls.screen_rect(button),
        Some((147, 130, 30, 16))
    );
    let frame_count = kernel.framebuffer.frame_counter;
    for (hwnd, expected) in [
        (page, [142i32, 123, 322, 223]),
        (nested, [139i32, 119, 219, 169]),
        (button, [147i32, 130, 177, 146]),
    ] {
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "GetWindowRect",
                &[hwnd, 0x1900]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            cpu.read_mem(0x1900, 16).unwrap(),
            expected
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(kernel.framebuffer.frame_counter, frame_count);
    assert_eq!(kernel.active_window, parent);
}
