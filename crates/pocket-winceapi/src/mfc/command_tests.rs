//! Commands exercise real maps, menu handles and continuations with guest callbacks.
use super::tests::result;
use super::window_dispatch::tests::{drive, entry};
use super::*;
use crate::bluetooth::tests::call;
use pocket_cpu::{regs::ArmReg, Cpu};
fn args(cpu: &mut dyn Cpu, values: &[u32]) {
    for (i, &value) in values.iter().enumerate() {
        if i < 4 {
            cpu.write_arg(i as u8, value).unwrap();
        } else {
            let sp = cpu.read_reg(ArmReg::Sp).unwrap();
            cpu.write_mem(sp + (i as u32 - 4) * 4, &value.to_le_bytes())
                .unwrap();
        }
    }
}
fn map(cpu: &mut dyn Cpu, code: u32, signature: u32) {
    for (i, value) in [0x6200u32, 0x3000].into_iter().enumerate() {
        cpu.write_mem(0x2800 + i as u32 * 4, &value.to_le_bytes())
            .unwrap();
    }
    for (i, value) in [
        0x111u32, code, 100, 104, signature, 0x6300, 0, 0, 0, 0, 0, 0,
    ]
    .into_iter()
    .enumerate()
    {
        cpu.write_mem(0x3000 + i as u32 * 4, &value.to_le_bytes())
            .unwrap();
    }
}
fn wnd_table(cpu: &mut dyn Cpu, object: u32, output: u32) {
    let original = cpu.read_u32_le(object).unwrap();
    let bytes = cpu.read_mem(original, 46 * 4).unwrap();
    cpu.write_mem(output, &bytes).unwrap();
    cpu.write_mem(output + 12 * 4, &0x6100u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(object, &output.to_le_bytes()).unwrap();
}
#[test]
fn declared_mfc_exports_have_unique_ordinals_and_names() {
    let mut ordinals = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for table in EXPORT_TABLES {
        for &(ordinal, name, _) in *table {
            assert!(ordinals.insert(ordinal), "duplicate ordinal {ordinal}");
            assert!(names.insert(name), "duplicate method {name}");
        }
    }
}
#[test]
fn command_ranges_query_handlers_without_execution_and_ignore_void_return_registers() {
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2000],
    );
    wnd_table(&mut cpu, 0x2000, 0x2400);
    map(&mut cpu, 0, 13);
    let free = k.heap.free_bytes();
    for query in [false, true] {
        args(
            &mut cpu,
            &[0x2000, 102, 0, 0, if query { 0x3200 } else { 0 }],
        );
        let mut executed = 0;
        let result = drive(&mut cpu, &mut k, &mut d, entry(1829), |pc, cpu| match pc {
            0x6100 => 0x2800,
            0x6300 => {
                executed += 1;
                assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                assert_eq!(cpu.read_arg(1).unwrap(), 102);
                0xdeadbeef
            }
            other => panic!("unexpected command callback {other:x}"),
        });
        assert_eq!(result, 1);
        assert_eq!(executed, usize::from(!query));
        if query {
            assert_eq!(cpu.read_u32_le(0x3200).unwrap(), 0x2000);
            assert_eq!(cpu.read_u32_le(0x3204).unwrap(), 0x6300);
        }
    }
    args(&mut cpu, &[0x2000, 105, 0, 0, 0]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1829), |pc, _| match pc {
            0x6100 => 0x2800,
            0x6200 => 0,
            other => panic!("unexpected {other:x}"),
        }),
        0
    );
    assert_eq!(k.heap.free_bytes(), free);
}
#[test]
fn update_handlers_consume_continue_routing_once_and_keep_ordinary_extra_arguments() {
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2000],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:69",
        &[0x2200],
    );
    wnd_table(&mut cpu, 0x2000, 0x2400);
    map(&mut cpu, u32::MAX, 44);
    let free = k.heap.free_bytes();
    for continuing in [true, false] {
        args(&mut cpu, &[0x2000, 102, u32::MAX, 0x2200, 0]);
        let result = drive(&mut cpu, &mut k, &mut d, entry(1829), |pc, cpu| match pc {
            0x6100 => 0x2800,
            0x6300 => {
                assert_eq!(cpu.read_arg(1).unwrap(), 0x2200);
                cpu.write_mem(0x221c, &u32::from(continuing).to_le_bytes())
                    .unwrap();
                999
            }
            other => panic!("unexpected {other:x}"),
        });
        assert_eq!(result, u32::from(!continuing));
        assert_eq!(cpu.read_u32_le(0x221c).unwrap(), 0);
    }
    assert_eq!(k.heap.free_bytes(), free);
}
#[test]
fn temporary_menu_identity_cleanup_preserves_the_attached_owner_and_real_tree() {
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    let menu = crate::coredll::menu_api::alloc(
        &mut k,
        vec![pocket_kernel::MenuItem {
            id: 101,
            ..Default::default()
        }],
    );
    let owner = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:972",
        &[],
    ));
    let free = k.heap.free_bytes();
    let temp = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:1206",
        &[menu],
    ));
    assert_eq!(
        temp,
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1206",
            &[menu]
        ))
    );
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:803",
            &[owner, menu]
        )),
        1
    );
    assert_eq!(
        owner,
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1206",
            &[menu]
        ))
    );
    drive(&mut cpu, &mut k, &mut d, entry(1047), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert_eq!(cpu.read_u32_le(owner + 4).unwrap(), menu);
    assert!(k.menu_items.contains_key(&menu));
    assert_eq!(k.heap.free_bytes(), free);
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1064",
            &[owner]
        )),
        menu
    );
    k.heap.free(owner);
}
#[test]
fn command_ui_changes_real_menu_state_and_text_without_losing_checked_or_disabled_flags() {
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    let menu = crate::coredll::menu_api::alloc(
        &mut k,
        vec![pocket_kernel::MenuItem {
            id: 101,
            ..Default::default()
        }],
    );
    let wrapper = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:1206",
        &[menu],
    ));
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:69",
        &[0x2000],
    );
    cpu.write_mem(0x2004, &101u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x200c, &wrapper.to_le_bytes()).unwrap();
    let free = k.heap.free_bytes();
    args(&mut cpu, &[0x2000, 0]);
    drive(&mut cpu, &mut k, &mut d, entry(1121), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert_eq!(k.menus[&menu][&101] & 3, 1);
    assert_eq!(cpu.read_u32_le(0x2018).unwrap(), 1);
    args(&mut cpu, &[0x2000, 1]);
    drive(&mut cpu, &mut k, &mut d, entry(2395), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert_eq!(k.menus[&menu][&101] & 8, 8);
    let label: Vec<_> = "New label"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    cpu.write_mem(0x2200, &label).unwrap();
    args(&mut cpu, &[0x2000, 0x2200]);
    drive(&mut cpu, &mut k, &mut d, entry(2430), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert_eq!(
        String::from_utf16_lossy(&k.menu_items[&menu][0].text),
        "New label"
    );
    assert_eq!(k.menus[&menu][&101] & 0xb, 9);
    assert_eq!(k.heap.free_bytes(), free);
}
#[test]
fn automatic_disable_queries_a_command_without_executing_it() {
    for exists in [false, true] {
        let (mut cpu, mut k, mut d) = tests::framework_setup();
        let menu = crate::coredll::menu_api::alloc(
            &mut k,
            vec![pocket_kernel::MenuItem {
                id: 101,
                ..Default::default()
            }],
        );
        let wrapper = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1206",
            &[menu],
        ));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(original, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &bytes).unwrap();
        cpu.write_mem(0x2414, &0x6100u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:69",
            &[0x2200],
        );
        cpu.write_mem(0x2204, &101u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x220c, &wrapper.to_le_bytes()).unwrap();
        let mut calls = 0;
        args(&mut cpu, &[0x2200, 0x2000, 1]);
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1092), |pc, cpu| {
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_arg(1).unwrap(), 101);
                assert_eq!(cpu.read_arg(3).unwrap(), 0x2200);
                let code = cpu.read_arg(2).unwrap();
                calls += 1;
                if code == u32::MAX {
                    0
                } else {
                    assert_eq!(code, 0);
                    let sp = cpu.read_reg(ArmReg::Sp).unwrap();
                    assert_ne!(cpu.read_u32_le(sp).unwrap(), 0);
                    u32::from(exists)
                }
            }),
            0
        );
        assert_eq!(calls, 2);
        assert_eq!(k.menus[&menu][&101] & 3, u32::from(!exists));
    }
}
#[test]
fn command_update_veto_consumes_the_command_without_executing_it() {
    for enabled in [false, true] {
        let (mut cpu, mut k, mut d) = tests::framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(original, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &bytes).unwrap();
        cpu.write_mem(0x2414, &0x6100u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        let mut executed = 0;
        args(&mut cpu, &[0x2000, 0x10065, 0]);
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1840), |pc, cpu| {
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_arg(1).unwrap(), 101);
                if cpu.read_arg(2).unwrap() == u32::MAX {
                    let probe = cpu.read_arg(3).unwrap();
                    cpu.write_mem(probe + 0x28, &u32::from(enabled).to_le_bytes())
                        .unwrap();
                    1
                } else {
                    assert_eq!(cpu.read_arg(2).unwrap(), 0);
                    assert_eq!(cpu.read_arg(3).unwrap(), 0);
                    executed += 1;
                    1
                }
            }),
            1
        );
        assert_eq!(executed, usize::from(enabled));
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|s| s.starts_with("__PocketHLE_MFC_Frame:")));
    }
}
#[test]
fn frame_routes_active_view_then_own_map_then_application_and_restores_thread_context() {
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:201",
        &[0x2000, 0],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:103",
        &[0x2200],
    );
    let app_table = cpu.read_u32_le(0x2000).unwrap();
    let bytes = cpu.read_mem(app_table, 38 * 4).unwrap();
    cpu.write_mem(0x2800, &bytes).unwrap();
    cpu.write_mem(0x2814, &0x6300u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2600, &0x2700u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2714, &0x6100u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x22ac, &0x2600u32.to_le_bytes()).unwrap();
    let frame_table = cpu.read_u32_le(0x2200).unwrap();
    let bytes = cpu.read_mem(frame_table, 59 * 4).unwrap();
    cpu.write_mem(0x2c00, &bytes).unwrap();
    cpu.write_mem(0x2c30, &0x6200u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2200, &0x2c00u32.to_le_bytes()).unwrap();
    let thread = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:651",
        &[],
    ));
    cpu.write_mem(thread + 0x12c, &0xabcdefu32.to_le_bytes())
        .unwrap();
    let free = k.heap.free_bytes();
    let mut order = Vec::new();
    args(&mut cpu, &[0x2200, 101, 0, 0, 0]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1833), |pc, cpu| {
            order.push(pc);
            assert_eq!(cpu.read_u32_le(thread + 0x12c).unwrap(), 0x2200);
            match pc {
                0x6100 => {
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2600);
                    0
                }
                0x6200 => {
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                    0
                }
                0x6300 => {
                    assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                    1
                }
                other => panic!("unexpected {other:x}"),
            }
        }),
        1
    );
    assert_eq!(order, [0x6100, 0x6200, 0x6300]);
    assert_eq!(cpu.read_u32_le(thread + 0x12c).unwrap(), 0xabcdef);
    assert_eq!(k.heap.free_bytes(), free);
}
#[test]
fn popup_update_rechecks_live_item_count_after_a_handler_removes_its_own_row() {
    fn command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
        let id = ctx.arg_u32(1)?;
        let ui = ctx.arg_u32(3)?;
        assert_eq!(ctx.arg_u32(2)?, u32::MAX);
        let owner = application::owner(ctx)?;
        let key = format!("__TestPopup:{id}");
        assert!(
            ctx.kernel
                .dynamic_exports
                .get_mut(&owner)
                .unwrap()
                .insert(key, 1)
                .is_none(),
            "row updated twice"
        );
        put(ctx, ui + 0x18, 1)?;
        if id == 100 {
            let wrapper = word(ctx, ui + 0xc)?;
            let handle = word(ctx, wrapper + 4)?;
            // A guest update handler may remove its current item. Model that
            // mutation in the real shared namespace; the popup must requery it.
            let items = ctx.kernel.menu_items.get_mut(&handle).unwrap();
            let index = items.iter().position(|item| item.id == id).unwrap();
            items.remove(index);
            ctx.kernel.menus.get_mut(&handle).unwrap().remove(&id);
        }
        Ok(DispatchOutcome::ReturnedR0(1))
    }
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:103",
        &[0x2000],
    );
    let menu = crate::coredll::menu_api::alloc(
        &mut k,
        (100..=102)
            .map(|id| pocket_kernel::MenuItem {
                id,
                ..Default::default()
            })
            .collect(),
    );
    let wrapper = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:1206",
        &[menu],
    ));
    let original = cpu.read_u32_le(0x2000).unwrap();
    let bytes = cpu.read_mem(original, 59 * 4).unwrap();
    cpu.write_mem(0x2400, &bytes).unwrap();
    let method = k.dynamic_exports[&1]["GetTickCount"];
    cpu.write_mem(0x2414, &method.to_le_bytes()).unwrap();
    cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
    d.register_handler("coredll.dll", "GetTickCount", command);
    args(&mut cpu, &[0x2000, wrapper, 0, 0]);
    drive(&mut cpu, &mut k, &mut d, entry(1947), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    for id in 100..=102 {
        assert!(k.dynamic_exports[&1].contains_key(&format!("__TestPopup:{id}")));
    }
    assert_eq!(
        k.menu_items[&menu].iter().map(|i| i.id).collect::<Vec<_>>(),
        [101, 102]
    );
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|s| s.starts_with("__PocketHLE_MFC_Frame:")));
}

#[test]
fn protected_handler_query_output_is_rejected_before_virtual_lookup() {
    use pocket_kernel::Dispatcher;
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2000],
    );
    wnd_table(&mut cpu, 0x2000, 0x2400);
    cpu.map_region(0x8000, 0x1000, pocket_cpu::Prot::READ)
        .unwrap();
    args(&mut cpu, &[0x2000, 101, 0, 0, 0x8000]);
    assert!(d.dispatch(&mut cpu, &entry(1829), &mut k).is_err());
    assert_eq!(cpu.read_mem(0x8000, 8).unwrap(), [0; 8]);
}

#[test]
fn system_popup_returns_without_dereferencing_a_menu_or_building_command_ui() {
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:103",
        &[0x2000],
    );
    let free = k.heap.free_bytes();
    args(&mut cpu, &[0x2000, 0, 0, 1]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1947), |pc, _| panic!(
            "unexpected {pc:x}"
        )),
        0
    );
    assert!(!k.dynamic_exports[&1].contains_key("__PocketHLE_MFC_CommandUI_Table"));
    assert_eq!(k.heap.free_bytes(), free);
}

#[test]
fn command_probe_records_enable_without_changing_other_fields() {
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    cpu.write_mem(0x2000, &[0x5a; 44]).unwrap();
    let name = "__PocketHLE_MFC_CommandProbe_Enable";
    let thunk = pocket_kernel::Thunk {
        thunk_va: k.dynamic_exports[&1][name],
        iat_va: 0,
        dll: "coredll.dll".into(),
        binding: pocket_pe::ImportBinding::Name(name.into()),
        friendly_name: None,
    };
    args(&mut cpu, &[0x2000, 0]);
    drive(&mut cpu, &mut k, &mut d, thunk, |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert_eq!(cpu.read_u32_le(0x2028).unwrap(), 0);
    assert_eq!(cpu.read_u32_le(0x2018).unwrap(), 1);
    assert_eq!(cpu.read_mem(0x2000, 24).unwrap(), [0x5a; 24]);
    assert_eq!(cpu.read_mem(0x201c, 12).unwrap(), [0x5a; 12]);
}

#[test]
fn control_creation_reuses_win_ce_classes_caption_geometry_and_parent_identity() {
    use pocket_kernel::controls::ControlClass;
    for (ordinal, class, titled) in [
        (901, ControlClass::Button, true),
        (927, ControlClass::Static, true),
        (904, ControlClass::ComboBox, false),
        (907, ControlClass::Edit, false),
        (916, ControlClass::ListBox, false),
    ] {
        let (mut cpu, mut k, mut d) = tests::framework_setup();
        for object in [0x2000, 0x2200] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[object],
            );
        }
        cpu.write_mem(0x2220, &0xdead1000u32.to_le_bytes()).unwrap();
        for (i, value) in [7u32, 8, 107, 28].into_iter().enumerate() {
            cpu.write_mem(0x2800 + 4 * i as u32, &value.to_le_bytes())
                .unwrap();
        }
        let caption: Vec<_> = "Caption"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x2a00, &caption).unwrap();
        if titled {
            args(&mut cpu, &[0x2000, 0x2a00, 0x50000000, 0x2800, 0x2200, 123]);
        } else {
            args(&mut cpu, &[0x2000, 0x50000000, 0x2800, 0x2200, 123]);
        }
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(ordinal), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            1
        );
        let hwnd = cpu.read_u32_le(0x2020).unwrap();
        let control = k.controls.get(hwnd).unwrap();
        assert_eq!(control.class, class);
        assert_eq!(control.parent, 0xdead1000);
        assert_eq!(
            (control.x, control.y, control.w, control.h),
            (7, 8, 100, 20)
        );
        assert_eq!(control.text, if titled { "Caption" } else { "" });
        assert_eq!(k.window_ids[&hwnd], 123);
    }
}

#[test]
fn owner_drawn_tab_calls_the_guest_override_and_preserves_the_optional_result() {
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2000],
    );
    cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x24b8, &0x6100u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2200, &0x12345678u32.to_le_bytes()).unwrap();
    let free = k.heap.free_bytes();
    args(&mut cpu, &[0x2000, 0x2b, 0, 0x2800, 0x2200]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1822), |pc, cpu| {
            assert_eq!(pc, 0x6100);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
            assert_eq!(cpu.read_arg(1).unwrap(), 0x2800);
            0xdeadbeef
        }),
        1
    );
    assert_eq!(cpu.read_u32_le(0x2200).unwrap(), 0x12345678);
    assert_eq!(k.heap.free_bytes(), free);
}

#[test]
fn window_command_ui_uses_real_tab_focus_check_messages_and_control_text() {
    use pocket_kernel::controls::ControlClass;
    let (mut cpu, mut k, mut d) = tests::framework_setup();
    let parent = 0xdead1000;
    k.window_classes.insert(parent, "Dialog".into());
    k.window_styles.insert(parent, (0x90000000, 0));
    let check = k.controls.create(
        parent,
        ControlClass::Button,
        101,
        "Check".into(),
        0x50010003,
        0,
        0,
        30,
        20,
    );
    let edit = k.controls.create(
        parent,
        ControlClass::Edit,
        102,
        "Edit".into(),
        0x50010000,
        40,
        0,
        30,
        20,
    );
    let wrapper = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:1207",
        &[check],
    ));
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:69",
        &[0x2000],
    );
    cpu.write_mem(0x2014, &wrapper.to_le_bytes()).unwrap();
    let free = k.heap.free_bytes();
    args(&mut cpu, &[0x2000, 1]);
    drive(&mut cpu, &mut k, &mut d, entry(2326), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert!(k.controls.get(check).unwrap().checked);
    // SetCheck on an EDIT must honor its WM_GETDLGCODE result, not write
    // a checkbox field into an unrelated control.
    let edit_wrapper = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:1207",
        &[edit],
    ));
    cpu.write_mem(0x2014, &edit_wrapper.to_le_bytes()).unwrap();
    args(&mut cpu, &[0x2000, 1]);
    drive(&mut cpu, &mut k, &mut d, entry(2326), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert!(!k.controls.get(edit).unwrap().checked);
    cpu.write_mem(
        0x2200,
        &"Changed"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    args(&mut cpu, &[0x2000, 0x2200]);
    drive(&mut cpu, &mut k, &mut d, entry(2430), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert_eq!(k.controls.get(edit).unwrap().text, "Changed");
    cpu.write_mem(0x2014, &wrapper.to_le_bytes()).unwrap();
    k.controls.focus = check;
    args(&mut cpu, &[0x2000, 0]);
    drive(&mut cpu, &mut k, &mut d, entry(1121), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert!(!k.controls.get(check).unwrap().enabled);
    assert_eq!(k.controls.focus, edit);
    assert_eq!(cpu.read_u32_le(0x2018).unwrap(), 1);
    args(&mut cpu, &[0x2000, 1]);
    drive(&mut cpu, &mut k, &mut d, entry(1121), |pc, _| {
        panic!("unexpected {pc:x}")
    });
    assert!(k.controls.get(check).unwrap().enabled);
    // The edit FromHandle wrapper is the only persistent allocation above.
    assert!(k.heap.free_bytes() < free);
    let stable = k.heap.free_bytes();
    for _ in 0..3 {
        args(&mut cpu, &[0x2000, 0]);
        drive(&mut cpu, &mut k, &mut d, entry(1121), |pc, _| {
            panic!("unexpected {pc:x}")
        });
    }
    assert_eq!(k.heap.free_bytes(), stable);
    assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
}
