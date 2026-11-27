//! Real USER/GDI/callback tests for the common splitter engines.
use super::tests::{framework_setup, result};
use super::window_dispatch::tests::{drive_with, entry};
use super::*;
use crate::bluetooth::tests::call;
use pocket_cpu::{regs::ArmReg, Cpu};
fn run(
    cpu: &mut dyn Cpu,
    k: &mut pocket_kernel::KernelState,
    d: &mut WinCeDispatcher,
    n: u16,
    args: &[u32],
) -> u32 {
    let sp = cpu.read_reg(ArmReg::Sp).unwrap();
    let home = cpu.stack_arg_offset();
    for (i, &v) in args.iter().enumerate() {
        if i < 4 {
            cpu.write_arg(i as u8, v).unwrap();
        } else {
            cpu.write_mem(sp + home + (i as u32 - 4) * 4, &v.to_le_bytes())
                .unwrap();
        }
    }
    drive_with(cpu, k, d, entry(n), 4096, |pc, _, _, _| {
        panic!("unexpected guest {pc:x}")
    })
}
fn fixture(
    rows: u32,
    cols: u32,
    style: u32,
) -> (
    pocket_cpu::stub::StubCpu,
    pocket_kernel::KernelState,
    WinCeDispatcher,
) {
    let (mut cpu, mut k, mut d) = framework_setup();
    run(&mut cpu, &mut k, &mut d, 103, &[0x2000]);
    k.window_classes.insert(7, "Frame".into());
    k.window_rects.insert(7, (0, 0, 240, 320));
    k.window_styles.insert(7, (0x10000000, 0));
    run(&mut cpu, &mut k, &mut d, 804, &[0x2000, 7]);
    run(&mut cpu, &mut k, &mut d, 169, &[0x2400]);
    assert_eq!(
        run(
            &mut cpu,
            &mut k,
            &mut d,
            985,
            &[0x2400, 0x2000, rows, cols, style, 0xe900]
        ),
        1
    );
    let splitter = cpu.read_u32_le(0x2420).unwrap();
    k.window_rects.insert(splitter, (0, 0, 240, 300));
    for r in 0..rows {
        for c in 0..cols {
            let i = r * cols + c;
            let o = 0x2800 + i * 80;
            let h = 0xdead1001 + i;
            run(&mut cpu, &mut k, &mut d, 206, &[o]);
            k.window_classes.insert(h, "Pane".into());
            k.window_rects.insert(h, (0, 0, 1, 1));
            k.window_parents.insert(h, splitter);
            k.window_ids.insert(h, 0xe900 + r * 16 + c);
            k.window_styles.insert(h, (0x50000000, 0));
            run(&mut cpu, &mut k, &mut d, 804, &[o, h]);
        }
    }
    for r in 0..rows {
        run(&mut cpu, &mut k, &mut d, 2402, &[0x2400, r, 100, 1]);
    }
    for c in 0..cols {
        run(&mut cpu, &mut k, &mut d, 2332, &[0x2400, c, 80, 1]);
    }
    run(&mut cpu, &mut k, &mut d, 2154, &[0x2400]);
    (cpu, k, d)
}
#[test]
fn tracker_cancel_restores_pixels_capture_and_dimensions_and_commit_changes_native_ideal() {
    let (mut cpu, mut k, mut d) = fixture(2, 1, 0x50000000);
    let splitter = cpu.read_u32_le(0x2420).unwrap();
    let rp = cpu.read_u32_le(0x2488).unwrap();
    let initial = cpu.read_u32_le(rp + 4).unwrap();
    let dc = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "GetDC",
        &[splitter],
    ));
    let bitmap = k.gdi.dc(dc).unwrap().selected_bitmap.unwrap();
    let pixels = k.gdi.bitmap(bitmap).unwrap().pixels.clone();
    let previous_brush = k.gdi.dc(dc).unwrap().selected_brush;
    for accept in [0, 1] {
        assert_eq!(run(&mut cpu, &mut k, &mut d, 1616, &[0x2400, 20, 102]), 101);
        run(&mut cpu, &mut k, &mut d, 2455, &[0x2400, 101]);
        assert_eq!(cpu.read_u32_le(0x248c).unwrap(), 1);
        assert_eq!(
            k.controls
                .pointer_capture
                .get(&k.current_thread)
                .copied()
                .unwrap_or(0),
            splitter
        );
        let before = cpu.read_u32_le(0x24b0).unwrap();
        run(&mut cpu, &mut k, &mut d, 1957, &[0x2400, 1, 20, 150]);
        assert_ne!(cpu.read_u32_le(0x24b0).unwrap(), before);
        run(&mut cpu, &mut k, &mut d, 2457, &[0x2400, accept]);
        assert_eq!(cpu.read_u32_le(0x248c).unwrap(), 0);
        assert_eq!(
            k.controls
                .pointer_capture
                .get(&k.current_thread)
                .copied()
                .unwrap_or(0),
            0
        );
        assert_eq!(k.gdi.dc(dc).unwrap().selected_brush, previous_brush);
        if accept == 0 {
            assert_eq!(cpu.read_u32_le(rp + 4).unwrap(), initial);
            assert_eq!(k.gdi.bitmap(bitmap).unwrap().pixels, pixels);
        } else {
            assert_ne!(cpu.read_u32_le(rp + 4).unwrap(), initial);
        }
    }
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")));
}
#[test]
fn solid_and_bevel_rectangles_use_real_dc_pixels_and_native_background_color_state() {
    let (mut cpu, mut k, mut d) = fixture(1, 1, 0x50000000);
    let splitter = cpu.read_u32_le(0x2420).unwrap();
    let dc = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "GetDC",
        &[splitter],
    ));
    // A CDC borrows the live window DC; no synthetic pixel renderer for the test.
    cpu.write_mem(0x3104, &dc.to_le_bytes()).unwrap();
    let area: Vec<_> = [10i32, 10, 20, 20]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect();
    cpu.write_mem(0x3200, &area).unwrap();
    run(
        &mut cpu,
        &mut k,
        &mut d,
        1153,
        &[0x3100, 0x3200, 0x000000ff],
    );
    assert_eq!(k.gdi.dc(dc).unwrap().bk_color, 0xff);
    let bitmap = k.gdi.dc(dc).unwrap().selected_bitmap.unwrap();
    let at = |k: &pocket_kernel::KernelState, x: u32, y: u32| {
        let image = k.gdi.bitmap(bitmap).unwrap();
        let i = ((y * image.width + x) * 2) as usize;
        u16::from_le_bytes([image.pixels[i], image.pixels[i + 1]])
    };
    assert_eq!(at(&k, 15, 15), 0xf800);
    run(
        &mut cpu,
        &mut k,
        &mut d,
        1098,
        &[0x3100, 0x3200, 0x0000ff00, 0x00ff0000],
    );
    assert_eq!(at(&k, 10, 10), 0x07e0);
    assert_eq!(at(&k, 19, 19), 0x001f);
    assert_eq!(at(&k, 15, 15), 0xf800);
    assert_eq!(k.gdi.dc(dc).unwrap().bk_color, 0xff0000);
}
#[test]
fn splitter_shared_scrollbars_are_positioned_using_the_live_client_metrics() {
    let (mut cpu, k, mut d) = fixture(2, 2, 0x50300000);
    let mut k = k;
    let splitter = cpu.read_u32_le(0x2420).unwrap();
    let inside = [1u32, 1, 224, 284];
    run(&mut cpu, &mut k, &mut d, 1321, &[0x2400, 0x3200]);
    for (i, v) in inside.into_iter().enumerate() {
        assert_eq!(cpu.read_u32_le(0x3200 + i as u32 * 4).unwrap(), v);
    }
    for h in k.controls.children_of(splitter) {
        let c = k.controls.get(h).unwrap();
        assert!(c.w > 0 && c.h > 0);
        if c.id == 0xea00 {
            assert_eq!(c.y, 285);
        }
        if c.id == 0xea10 {
            assert_eq!(c.x, 225);
        }
        if c.id == 0xea20 {
            assert_eq!((c.x, c.y, c.w, c.h), (225, 285, 14, 14));
        }
    }
}

#[test]
fn splitter_hit_boundaries_and_tracking_rects_match_executed_retail_ce3_reference() {
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("testdata/splitter-ce3.json")).unwrap();
    let (mut cpu, mut k, mut d) = fixture(2, 2, 0x50000000);
    for sample in oracle["hits"].as_array().unwrap() {
        let x = sample[0].as_u64().unwrap() as u32;
        let y = sample[1].as_u64().unwrap() as u32;
        let expected = sample[2].as_u64().unwrap() as u32;
        assert_eq!(
            run(&mut cpu, &mut k, &mut d, 1616, &[0x2400, x, y]),
            expected,
            "native hit mismatch at {x},{y}"
        );
    }
    for sample in oracle["rects"].as_array().unwrap() {
        for (i, v) in [1u32, 1, 239, 299].into_iter().enumerate() {
            cpu.write_mem(0x249c + i as u32 * 4, &v.to_le_bytes())
                .unwrap();
        }
        let hit = sample["hit"].as_u64().unwrap() as u32;
        run(&mut cpu, &mut k, &mut d, 1313, &[0x2400, hit, 0x3300]);
        for (name, base) in [("rect", 0x3300), ("limit", 0x249c), ("offset", 0x2494)] {
            for (i, v) in sample[name].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    cpu.read_u32_le(base + i as u32 * 4).unwrap() as i32,
                    v.as_i64().unwrap() as i32,
                    "native {name} mismatch for hit {hit}"
                );
            }
        }
    }
}

#[test]
fn dynamic_splitter_uses_real_view_factories_and_rolls_back_a_partly_created_column() {
    use super::window_dispatch::tests::drive_outcomes;
    use pocket_kernel::{DispatchOutcome, Dispatcher};
    fn args(cpu: &mut dyn Cpu, values: &[u32]) {
        let sp = cpu.read_reg(ArmReg::Sp).unwrap();
        let home = cpu.stack_arg_offset();
        for (i, &v) in values.iter().enumerate() {
            if i < 4 {
                cpu.write_arg(i as u8, v).unwrap();
            } else {
                cpu.write_mem(sp + home + (i as u32 - 4) * 4, &v.to_le_bytes())
                    .unwrap();
            }
        }
    }
    fn direct(
        cpu: &mut dyn Cpu,
        k: &mut pocket_kernel::KernelState,
        d: &mut WinCeDispatcher,
        n: u16,
        values: &[u32],
    ) -> u32 {
        args(cpu, values);
        let DispatchOutcome::ReturnedR0(v) = d.dispatch(cpu, &entry(n), k).unwrap() else {
            panic!("base constructor must return directly")
        };
        v
    }
    let (mut cpu, mut k, mut d) = framework_setup();
    run(&mut cpu, &mut k, &mut d, 103, &[0x2000]);
    k.window_classes.insert(7, "Frame".into());
    k.window_rects.insert(7, (0, 0, 240, 320));
    k.window_styles.insert(7, (0x10000000, 0));
    run(&mut cpu, &mut k, &mut d, 804, &[0x2000, 7]);
    run(&mut cpu, &mut k, &mut d, 169, &[0x2400]);
    cpu.write_mem(0x3300, &[0; 24]).unwrap();
    for (off, v) in [(0, 0x3400u32), (4, 76), (8, 0xffff), (12, 0x6100)] {
        cpu.write_mem(0x3300 + off, &v.to_le_bytes()).unwrap();
    }
    cpu.write_mem(0x3400, b"TestView\0").unwrap();
    cpu.write_mem(0x3500, &[0; 20]).unwrap();
    cpu.write_mem(0x3500, &0x3300u32.to_le_bytes()).unwrap();
    let mut factories = 0;
    let invoke = |cpu: &mut dyn Cpu,
                  k: &mut pocket_kernel::KernelState,
                  d: &mut WinCeDispatcher,
                  n,
                  values: &[u32],
                  factories: &mut u32,
                  reject: Option<u32>| {
        args(cpu, values);
        drive_outcomes(cpu, k, d, entry(n), 16384, |pc, cpu, k, d| {
            assert_eq!(pc, 0x6100);
            *factories += 1;
            if reject == Some(*factories) {
                return DispatchOutcome::ReturnedR0(0);
            }
            let object = direct(cpu, k, d, 315, &[76]);
            assert_ne!(object, 0);
            assert_eq!(direct(cpu, k, d, 200, &[object]), object);
            DispatchOutcome::ReturnedR0(object)
        })
    };
    assert_eq!(
        invoke(
            &mut cpu,
            &mut k,
            &mut d,
            926,
            &[0x2400, 0x2000, 2, 2, 80, 100, 0x3500, 0x50000001, 0xe900],
            &mut factories,
            None
        ),
        1
    );
    assert_eq!(factories, 1);
    let splitter = cpu.read_u32_le(0x2420).unwrap();
    k.window_rects.insert(splitter, (0, 0, 240, 300));
    run(&mut cpu, &mut k, &mut d, 2154, &[0x2400]);
    let original = run(&mut cpu, &mut k, &mut d, 1406, &[0x2400, 0, 0]);
    let original_hwnd = cpu.read_u32_le(original + 0x20).unwrap();
    assert!(k.heap.msize(original).is_some());
    assert_eq!(
        invoke(
            &mut cpu,
            &mut k,
            &mut d,
            2453,
            &[0x2400, 120],
            &mut factories,
            None
        ),
        1
    );
    assert_eq!(cpu.read_u32_le(0x2474).unwrap(), 2);
    run(&mut cpu, &mut k, &mut d, 1048, &[]);
    let before = k.heap.free_bytes();
    let before_windows = k.window_classes.len();
    // The first new column pane is real; the second factory fails. Rollback must
    // destroy the first pane through its native view/window deleting owner.
    let reject = factories + 2;
    assert_eq!(
        invoke(
            &mut cpu,
            &mut k,
            &mut d,
            2452,
            &[0x2400, 100],
            &mut factories,
            Some(reject)
        ),
        0
    );
    assert_eq!(cpu.read_u32_le(0x2478).unwrap(), 1);
    assert_eq!(k.window_classes.len(), before_windows);
    // FromHandle may borrow a temporary wrapper during focus restoration.
    // Native idle cleanup, rather than pane deletion, owns that wrapper.
    run(&mut cpu, &mut k, &mut d, 1048, &[]);
    assert_eq!(k.heap.free_bytes(), before);
    assert_eq!(
        invoke(
            &mut cpu,
            &mut k,
            &mut d,
            2452,
            &[0x2400, 100],
            &mut factories,
            None
        ),
        1
    );
    assert_eq!(cpu.read_u32_le(0x2478).unwrap(), 2);
    run(&mut cpu, &mut k, &mut d, 1032, &[0x2400, 1]);
    run(&mut cpu, &mut k, &mut d, 1042, &[0x2400, 1]);
    assert_eq!(cpu.read_u32_le(0x2478).unwrap(), 1);
    assert_eq!(cpu.read_u32_le(0x2474).unwrap(), 1);
    assert_eq!(
        run(&mut cpu, &mut k, &mut d, 1406, &[0x2400, 0, 0]),
        original
    );
    assert!(k.window_classes.contains_key(&original_hwnd));
    run(&mut cpu, &mut k, &mut d, 292, &[0x2400]);
    assert!(!k.window_classes.contains_key(&original_hwnd));
    assert!(k.heap.msize(original).is_none());
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")));
}

#[test]
fn keyboard_tracking_and_capture_loss_restore_the_same_guest_owned_state() {
    let (mut cpu, mut k, mut d) = fixture(2, 1, 0x50000000);
    let rp = cpu.read_u32_le(0x2488).unwrap();
    let original = cpu.read_u32_le(rp + 4).unwrap();
    assert_eq!(run(&mut cpu, &mut k, &mut d, 1076, &[0x2400]), 1);
    assert_eq!(cpu.read_u32_le(0x248c).unwrap(), 1);
    run(&mut cpu, &mut k, &mut d, 1949, &[0x2400, 0x1b, 1, 0]);
    assert_eq!(cpu.read_u32_le(0x248c).unwrap(), 0);
    assert_eq!(cpu.read_u32_le(rp + 4).unwrap(), original);
    run(&mut cpu, &mut k, &mut d, 2455, &[0x2400, 101]);
    let release = pocket_kernel::Thunk {
        thunk_va: k.dynamic_exports[&1]["ReleaseCapture"],
        iat_va: 0,
        dll: "coredll.dll".into(),
        binding: pocket_pe::ImportBinding::Name("ReleaseCapture".into()),
        friendly_name: None,
    };
    drive_with(&mut cpu, &mut k, &mut d, release, 4096, |pc, _, _, _| {
        panic!("unexpected guest {pc:x}")
    });
    run(&mut cpu, &mut k, &mut d, 1957, &[0x2400, 0, 20, 150]);
    assert_eq!(cpu.read_u32_le(0x248c).unwrap(), 0);
    assert_eq!(cpu.read_u32_le(rp + 4).unwrap(), original);
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")));
}

#[test]
fn user_child_queries_exclude_popup_owners_and_cursor_motion_uses_the_input_queue() {
    let (mut cpu, mut k, mut d) = fixture(2, 1, 0x50000000);
    let splitter = cpu.read_u32_le(0x2420).unwrap();
    let pane = run(&mut cpu, &mut k, &mut d, 1406, &[0x2400, 0, 0]);
    let hwnd = cpu.read_u32_le(pane + 0x20).unwrap();
    for (parent, child, expected) in [
        (7, hwnd, 1),
        (splitter, hwnd, 1),
        (hwnd, hwnd, 0),
        (hwnd, 7, 0),
    ] {
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "IsChild",
                &[parent, child]
            )),
            expected
        );
    }
    k.window_classes.insert(8, "OwnedPopup".into());
    k.window_styles.insert(8, (0x80000000, 0));
    k.window_parents.insert(8, 7);
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "IsChild",
            &[7, 8]
        )),
        0
    );
    k.pending_input.clear();
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SetCursorPos",
            &[30, 40]
        )),
        1
    );
    assert_eq!(k.controls.cursor_position, (30, 40));
    assert!(matches!(
        k.pending_input.back(),
        Some(pocket_kernel::InputEvent::PointerMove { x: 30, y: 40 })
    ));
    result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "SetCursorPos",
        &[30, 40],
    ));
    assert_eq!(k.pending_input.len(), 1);
    result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "GetCursorPos",
        &[0x3300],
    ));
    assert_eq!(cpu.read_u32_le(0x3300).unwrap(), 30);
    assert_eq!(cpu.read_u32_le(0x3304).unwrap(), 40);
}

#[test]
fn view_scroll_fanout_preserves_each_axis_and_rewinds_shared_bar_before_siblings() {
    use super::window_dispatch::tests::drive_outcomes;
    use pocket_kernel::DispatchOutcome;
    fn user(
        cpu: &mut dyn Cpu,
        k: &mut pocket_kernel::KernelState,
        d: &mut WinCeDispatcher,
        name: &str,
        args: &[u32],
    ) -> u32 {
        use pocket_kernel::Dispatcher;
        for (i, &value) in args.iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x6400,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name(name.into()),
            friendly_name: None,
        };
        result(d.dispatch(cpu, &thunk, k).unwrap())
    }
    for ordinal in [1090, 1091] {
        let (mut cpu, mut k, mut d) = fixture(2, 2, 0x50300000);
        let splitter = cpu.read_u32_le(0x2420).unwrap();
        let table = cpu.read_u32_le(0x2800).unwrap();
        let bytes = cpu.read_mem(table, 46 * 4).unwrap();
        cpu.write_mem(0x3700, &[0; 55 * 4]).unwrap();
        cpu.write_mem(0x3700, &bytes).unwrap();
        for (slot, pc) in [(26u32, 0x6300u32), (47, 0x6100), (48, 0x6200)] {
            cpu.write_mem(0x3700 + slot * 4, &pc.to_le_bytes()).unwrap();
        }
        for object in [0x2800u32, 0x2850, 0x28a0, 0x28f0] {
            cpu.write_mem(object, &0x3700u32.to_le_bytes()).unwrap();
        }
        let horizontal = k
            .controls
            .children_of(splitter)
            .into_iter()
            .find(|h| k.controls.get(*h).unwrap().id == 0xea00)
            .unwrap();
        let vertical = k
            .controls
            .children_of(splitter)
            .into_iter()
            .find(|h| k.controls.get(*h).unwrap().id == 0xea10)
            .unwrap();
        for (object, hwnd) in [(0x3100u32, vertical), (0x3200, horizontal)] {
            run(&mut cpu, &mut k, &mut d, 206, &[object]);
            cpu.write_mem(object + 0x20, &hwnd.to_le_bytes()).unwrap();
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SetScrollRange",
                &[hwnd, 2, 0, 1000, 0],
            ));
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SetScrollPos",
                &[hwnd, 2, 10, 0],
            ));
        }
        let values: &[u32] = if ordinal == 1090 {
            &[0x2400, 0x2800, 0x0304, 1]
        } else {
            &[0x2400, 0x2800, 7, 9, 1]
        };
        let sp = cpu.read_reg(ArmReg::Sp).unwrap();
        let home = cpu.stack_arg_offset();
        for (i, &v) in values.iter().enumerate() {
            if i < 4 {
                cpu.write_arg(i as u8, v).unwrap();
            } else {
                cpu.write_mem(sp + home + (i as u32 - 4) * 4, &v.to_le_bytes())
                    .unwrap();
            }
        }
        let mut calls = Vec::new();
        assert_eq!(
            drive_outcomes(
                &mut cpu,
                &mut k,
                &mut d,
                entry(ordinal),
                4096,
                |pc, cpu, k, d| {
                    if pc == 0x6300 {
                        return DispatchOutcome::ReturnedR0(if cpu.read_arg(1).unwrap() == 1 {
                            0x3100
                        } else {
                            0x3200
                        });
                    }
                    assert_eq!(pc, if ordinal == 1090 { 0x6100 } else { 0x6200 });
                    let object = cpu.read_arg(0).unwrap();
                    let x = cpu.read_arg(1).unwrap();
                    let y = cpu.read_arg(2).unwrap();
                    assert_eq!(cpu.read_arg(3).unwrap(), 1);
                    calls.push((object, x, y));
                    for hwnd in if object == 0x2800 {
                        vec![horizontal, vertical]
                    } else if object == 0x2850 {
                        vec![vertical]
                    } else {
                        vec![horizontal]
                    } {
                        assert_eq!(user(cpu, k, d, "GetScrollPos", &[hwnd, 2]), 10);
                        user(cpu, k, d, "SetScrollPos", &[hwnd, 2, 20, 0]);
                    }
                    DispatchOutcome::ReturnedR0(1)
                }
            ),
            1
        );
        assert_eq!(
            calls,
            if ordinal == 1090 {
                vec![
                    (0x2800, 0x0304, 0),
                    (0x2850, 0x03ff, 0),
                    (0x28a0, 0xff04, 0),
                ]
            } else {
                vec![(0x2800, 7, 9), (0x2850, 0, 9), (0x28a0, 7, 0)]
            }
        );
        for hwnd in [horizontal, vertical] {
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "GetScrollPos",
                    &[hwnd, 2]
                )),
                20
            );
        }
    }
}

#[test]
fn toolbar_destruction_deletes_its_bitmap_and_invokes_the_real_owned_object_deleter() {
    use super::window_dispatch::tests::drive_outcomes;
    use pocket_kernel::DispatchOutcome;
    let (mut cpu, mut k, mut d) = framework_setup();
    for _ in 0..2 {
        run(&mut cpu, &mut k, &mut d, 195, &[0x2400]);
        cpu.write_mem(0x3000, &[0x55, 0xaa]).unwrap();
        let bitmap = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreateBitmap",
            &[8, 1, 1, 1, 0x3000],
        ));
        assert_ne!(bitmap, 0);
        let owner = run(&mut cpu, &mut k, &mut d, 315, &[8]);
        cpu.write_mem(owner, &0x3300u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x3304, &0x6100u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2484, &bitmap.to_le_bytes()).unwrap();
        cpu.write_mem(0x24a0, &owner.to_le_bytes()).unwrap();
        let mut deletions = 0;
        cpu.write_arg(0, 0x2400).unwrap();
        drive_outcomes(
            &mut cpu,
            &mut k,
            &mut d,
            entry(304),
            4096,
            |pc, cpu, k, _| {
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_arg(0).unwrap(), owner);
                assert_eq!(cpu.read_arg(1).unwrap(), 1);
                deletions += 1;
                assert!(k.gdi.get(bitmap).is_none());
                k.heap.free(owner);
                DispatchOutcome::ReturnedR0(owner)
            },
        );
        assert_eq!(deletions, 1);
        assert_eq!(cpu.read_u32_le(0x2484).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x24a0).unwrap(), 0);
        assert!(k.gdi.get(bitmap).is_none());
        assert!(k.heap.msize(owner).is_none());
    }
}
