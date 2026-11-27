//! Dialog regressions exercise shared HWND/control owners and real continuations.
use super::tests::{framework_setup, result};
use super::window_dispatch::tests::{drive, entry};
use crate::bluetooth::tests::call;
use pocket_cpu::{regs::ArmReg, Cpu};
use pocket_kernel::controls::ControlClass;
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
fn template() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&0x50000000u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    for n in [1u16, 0, 0, 100, 60, 0, 0, 0] {
        b.extend_from_slice(&n.to_le_bytes());
    }
    while b.len() % 4 != 0 {
        b.push(0);
    }
    b.extend_from_slice(&0x50010003u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    for n in [5u16, 5, 50, 10, 10, 0xffff, 0x80] {
        b.extend_from_slice(&n.to_le_bytes());
    }
    for n in "Check".encode_utf16().chain([0, 0]) {
        b.extend_from_slice(&n.to_le_bytes());
    }
    b
}
#[test]
fn modeless_creation_binds_before_virtual_initializers_and_releases_its_framework_bar() {
    let (mut cpu, mut k, mut d) = framework_setup();
    // Empty initialization retains the existing direct service; old snapshots
    // need not contain the newly added resource-bounded entry to reopen a dialog.
    k.dynamic_exports
        .get_mut(&1)
        .unwrap()
        .remove(super::dialog_records::EXECUTE);
    k.window_classes.insert(7, "Parent".into());
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2200],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:804",
        &[0x2200, 7],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:79",
        &[0x2000],
    );
    let original = cpu.read_u32_le(0x2000).unwrap();
    let bytes = cpu.read_mem(original, 52 * 4).unwrap();
    cpu.write_mem(0x2400, &bytes).unwrap();
    for (slot, callback) in [(21, 0x6300u32), (51, 0x6100), (33, 0x6200)] {
        cpu.write_mem(0x2400 + slot * 4, &callback.to_le_bytes())
            .unwrap();
    }
    cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2048, &0u32.to_le_bytes()).unwrap(); // caller opts out of shell resizing
    cpu.write_mem(0x2800, &template()).unwrap();
    args(&mut cpu, &[0x2000, 0x2800, 0x2200]);
    let mut callbacks = Vec::new();
    let value = drive(&mut cpu, &mut k, &mut d, entry(947), |pc, cpu| {
        callbacks.push(pc);
        assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
        assert_ne!(cpu.read_u32_le(0x2020).unwrap(), 0);
        if pc == 0x6200 {
            let dx = cpu.read_arg(1).unwrap();
            assert_eq!(cpu.read_u32_le(dx).unwrap(), 0);
            assert_eq!(cpu.read_u32_le(dx + 4).unwrap(), 0x2000);
        } else {
            assert!(matches!(pc, 0x6100 | 0x6300));
        }
        0xdeadbeef // all three callbacks return void
    });
    assert_eq!(value, 1);
    assert_eq!(callbacks, [0x6300, 0x6100, 0x6200]);
    let hwnd = cpu.read_u32_le(0x2020).unwrap();
    assert_ne!(hwnd, 1);
    assert_eq!(k.window_parents[&hwnd], 7);
    let child = k
        .controls
        .children_of(hwnd)
        .into_iter()
        .find(|h| k.controls.get(*h).is_some_and(|c| c.id == 10))
        .unwrap();
    assert_eq!(k.controls.get(child).unwrap().text, "Check");
    let bar = cpu.read_u32_le(0x204c).unwrap();
    assert_eq!(k.heap.msize(bar), Some(224));
    assert_eq!(k.controls.get(child).unwrap().parent, hwnd);
    args(&mut cpu, &[0x2000]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(231), |pc, _| panic!(
            "unexpected teardown callback {pc:x}"
        )),
        0x2000
    );
    assert_eq!(k.heap.msize(bar), None);
    assert!(k.controls.get(child).is_none());
    assert_eq!(cpu.read_u32_le(0x2220).unwrap(), 7);
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|key| key.starts_with("__PocketHLE_MFC_DialogBar:")));
    assert!(k.dialog_frame.is_none());
    assert!(k.dialog_default_frames.is_empty());
}
#[test]
fn checkbox_exchange_uses_live_control_state_and_retains_the_native_constructor_tail() {
    let (mut cpu, mut k, mut d) = framework_setup();
    k.window_classes.insert(7, "Window".into());
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
        "ord:804",
        &[0x2000, 7],
    );
    let child = k.controls.create(
        7,
        ControlClass::Button,
        10,
        "Check".into(),
        0x50010003,
        0,
        0,
        50,
        20,
    );
    cpu.write_mem(0x220c, &0xabcdef12u32.to_le_bytes()).unwrap();
    assert_eq!(
        result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:75",
            &[0x2200, 0x2000, 0]
        )),
        0x2200
    );
    assert_eq!(cpu.read_u32_le(0x220c).unwrap(), 0xabcdef12);
    for (state, expected) in [(1u32, true), (9, false)] {
        cpu.write_mem(0x2400, &state.to_le_bytes()).unwrap();
        args(&mut cpu, &[0x2200, 10, 0x2400]);
        drive(&mut cpu, &mut k, &mut d, entry(1003), |pc, _| {
            panic!("unexpected callback {pc:x}")
        });
        assert_eq!(k.controls.get(child).unwrap().checked, expected);
        assert_eq!(cpu.read_u32_le(0x2400).unwrap(), u32::from(expected));
    }
    k.controls.get_mut(child).unwrap().checked = true;
    cpu.write_mem(0x2200, &1u32.to_le_bytes()).unwrap();
    args(&mut cpu, &[0x2200, 10, 0x2400]);
    drive(&mut cpu, &mut k, &mut d, entry(1003), |pc, _| {
        panic!("unexpected callback {pc:x}")
    });
    assert_eq!(cpu.read_u32_le(0x2400).unwrap(), 1);
    assert_eq!(cpu.read_u32_le(0x2208).unwrap(), child);
    args(&mut cpu, &[0x2200, 10]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(2102), |pc, _| panic!(
            "unexpected callback {pc:x}"
        )),
        child
    );
    assert_eq!(cpu.read_u32_le(0x220c).unwrap(), 1);
}
#[test]
fn update_data_executes_the_override_ignores_void_r0_and_restores_notification_suppression() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2000],
    );
    let table = cpu.read_u32_le(0x2000).unwrap();
    let bytes = cpu.read_mem(table, 46 * 4).unwrap();
    cpu.write_mem(0x2400, &bytes).unwrap();
    cpu.write_mem(0x2484, &0x6100u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
    let thread = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:651",
        &[],
    ));
    cpu.write_mem(thread + 0x120, &0x55u32.to_le_bytes())
        .unwrap();
    let free = k.heap.free_bytes();
    for saving in [0u32, 1] {
        args(&mut cpu, &[0x2000, saving]);
        let mut count = 0;
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(2495), |pc, cpu| {
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_u32_le(thread + 0x120).unwrap(), 7);
                count += 1;
                let dx = cpu.read_arg(1).unwrap();
                assert_eq!(cpu.read_u32_le(dx).unwrap(), saving);
                assert_eq!(cpu.read_u32_le(dx + 4).unwrap(), 0x2000);
                0
            }),
            1
        );
        assert_eq!(count, 1);
        assert_eq!(cpu.read_u32_le(thread + 0x120).unwrap(), 0x55);
        assert_eq!(k.heap.free_bytes(), free);
    }
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|s| s.starts_with("__PocketHLE_MFC_Frame:")));
}
#[test]
fn missing_dialog_template_fails_without_publishing_a_window_or_a_bar() {
    let (mut cpu, mut k, mut d) = framework_setup();
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:79",
        &[0x2000],
    );
    let windows = k.window_classes.len();
    let controls = k.controls.children_of(7).len();
    args(&mut cpu, &[0x2000, 0x7777, 0]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(905), |pc, _| panic!(
            "unexpected callback {pc:x}"
        )),
        0
    );
    assert_eq!(cpu.read_u32_le(0x2020).unwrap(), 0);
    assert_eq!(k.window_classes.len(), windows);
    assert_eq!(k.controls.children_of(7).len(), controls);
}

#[test]
fn checked_radio_scans_live_controls_in_id_order_including_missing_ids() {
    let (mut cpu, mut k, mut d) = framework_setup();
    k.window_classes.insert(7, "Parent".into());
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
        "ord:804",
        &[0x2000, 7],
    );
    let first = k.controls.create(
        7,
        ControlClass::Button,
        10,
        "First".into(),
        0x50000004,
        0,
        0,
        20,
        20,
    );
    let second = k.controls.create(
        7,
        ControlClass::Button,
        12,
        "Second".into(),
        0x50000004,
        20,
        0,
        20,
        20,
    );
    for (a, b, expected) in [(false, true, 12), (true, true, 10), (false, false, 0)] {
        k.controls.get_mut(first).unwrap().checked = a;
        k.controls.get_mut(second).unwrap().checked = b;
        args(&mut cpu, &[0x2000, 10, 12]);
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1241), |pc, _| panic!(
                "unexpected callback {pc:x}"
            )),
            expected
        );
    }
    args(&mut cpu, &[0x2000, 12, 10]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1241), |pc, _| panic!(
            "unexpected callback {pc:x}"
        )),
        0
    );
}

fn init_record(blob: &mut Vec<u8>, id: u16, message: u16, payload: &[u8]) {
    blob.extend_from_slice(&id.to_le_bytes());
    blob.extend_from_slice(&message.to_le_bytes());
    blob.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    blob.extend_from_slice(payload);
}
fn init_wide(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}
#[test]
fn dlginit_fills_real_list_and_combo_skips_unknown_unaligned_records_and_frees_copies() {
    let (mut cpu, mut k, mut d) = framework_setup();
    k.window_classes.insert(7, "Dialog".into());
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
        "ord:804",
        &[0x2000, 7],
    );
    let list = k.controls.create(
        7,
        ControlClass::ListBox,
        10,
        "".into(),
        0x50000000,
        0,
        0,
        80,
        40,
    );
    let combo = k.controls.create(
        7,
        ControlClass::ComboBox,
        11,
        "".into(),
        0x50000003,
        0,
        40,
        80,
        40,
    );
    let mut blob = Vec::new();
    init_record(&mut blob, 10, 0x401, &init_wide("Été"));
    init_record(&mut blob, 10, 0x7777, &[1, 2, 3]);
    init_record(&mut blob, 11, 0x403, &init_wide("Second"));
    init_record(&mut blob, 10, 0x180, &init_wide("Third"));
    blob.extend_from_slice(&0u16.to_le_bytes());
    cpu.write_mem(0x2800, &blob).unwrap();
    let free = k.heap.free_bytes();
    args(&mut cpu, &[0x2000, 0x2800]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1142), |pc, _| panic!(
            "unexpected {pc:x}"
        )),
        1
    );
    assert_eq!(
        k.controls
            .get(list)
            .unwrap()
            .list_items
            .iter()
            .map(|v| v.0.as_str())
            .collect::<Vec<_>>(),
        ["Été", "Third"]
    );
    assert_eq!(k.controls.get(combo).unwrap().list_items[0].0, "Second");
    assert_eq!(k.heap.free_bytes(), free);
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|k| k.starts_with("__PocketHLE_MFC_Frame:")));
}
#[test]
fn dlginit_control_failure_returns_false_without_leaking_its_copy_or_sending_initial_update() {
    let (mut cpu, mut k, mut d) = framework_setup();
    k.window_classes.insert(7, "Dialog".into());
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
        "ord:804",
        &[0x2000, 7],
    );
    let child = k.controls.create(
        7,
        ControlClass::ListBox,
        10,
        "".into(),
        0x50000000,
        0,
        0,
        80,
        40,
    );
    k.window_procs.insert(child, 0x6100);
    let mut blob = Vec::new();
    init_record(&mut blob, 10, 0x401, &init_wide("Item"));
    blob.extend_from_slice(&0u16.to_le_bytes());
    cpu.write_mem(0x2800, &blob).unwrap();
    let free = k.heap.free_bytes();
    let mut messages = Vec::new();
    args(&mut cpu, &[0x2000, 0x2800]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1142), |pc, cpu| {
            assert_eq!(pc, 0x6100);
            messages.push(cpu.read_arg(1).unwrap());
            u32::MAX
        }),
        0
    );
    assert_eq!(messages, [0x180]);
    assert_eq!(k.heap.free_bytes(), free);
}
#[test]
fn dlginit_checks_the_resource_extent_before_reading_header_or_payload() {
    use pocket_kernel::Dispatcher;
    for size in [1, 7, 9] {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x2800, &[10, 0, 1, 4, 4, 0, 0, 0, 65, 0, 0, 0])
            .unwrap();
        args(&mut cpu, &[0x2000, 0x2800, size]);
        let name = super::dialog_records::EXECUTE;
        let thunk = pocket_kernel::Thunk {
            thunk_va: k.dynamic_exports[&1][name],
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name(name.into()),
            friendly_name: None,
        };
        let error = d
            .dispatch(&mut cpu, &thunk, &mut k)
            .unwrap_err()
            .to_string();
        assert!(error.contains("DLGINIT"), "{error}");
    }
}
#[test]
fn attach_control_site_does_not_reparent_ordinary_windows() {
    let (mut cpu, mut k, mut d) = framework_setup();
    for (object, hwnd) in [(0x2000, 7), (0x2200, 8)] {
        k.window_classes.insert(hwnd, "Window".into());
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[object],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:804",
            &[object, hwnd],
        );
    }
    k.window_parents.insert(8, 9);
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:807",
        &[0x2200, 0x2000],
    );
    assert_eq!(k.window_parents[&8], 9);
    assert_eq!(cpu.read_u32_le(0x2240).unwrap(), 0);
}
#[test]
fn initial_focus_redirection_selects_a_real_permanent_tab_window() {
    let (mut cpu, mut k, mut d) = framework_setup();
    k.window_classes.insert(7, "Dialog".into());
    k.window_styles.insert(7, (0x10000000, 0));
    k.window_visibility.insert(7, true);
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:79",
        &[0x2000],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:804",
        &[0x2000, 7],
    );
    let child = k.controls.create(
        7,
        ControlClass::Button,
        10,
        "Focus".into(),
        0x50010000,
        0,
        0,
        40,
        20,
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:206",
        &[0x2200],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:804",
        &[0x2200, child],
    );
    let original = cpu.read_u32_le(0x2000).unwrap();
    let bytes = cpu.read_mem(original, 52 * 4).unwrap();
    cpu.write_mem(0x2400, &bytes).unwrap();
    cpu.write_mem(0x2400 + 40 * 4, &0x6100u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x202c, &0x100u32.to_le_bytes()).unwrap();
    args(&mut cpu, &[0x2000, 0, 0]);
    assert_eq!(
        drive(&mut cpu, &mut k, &mut d, entry(1600), |pc, _| {
            assert_eq!(pc, 0x6100);
            1
        }),
        0
    );
    assert_eq!(k.controls.focus, child);
}

#[test]
fn help_button_visibility_queries_real_virtual_handlers_with_handler_info() {
    for available in [false, true] {
        let (mut cpu, mut k, mut d) = framework_setup();
        k.window_classes.insert(7, "Dialog".into());
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
            "ord:79",
            &[0x2400],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:804",
            &[0x2400, 7],
        );
        cpu.write_mem(0x2020, &0x2400u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2448, &0u32.to_le_bytes()).unwrap();
        for (object, table, length, callback) in [
            (0x2000, 0x3000, 40, 0x6200u32),
            (0x2400, 0x3200, 52, 0x6100u32),
        ] {
            let original = cpu.read_u32_le(object).unwrap();
            let bytes = cpu.read_mem(original, length * 4).unwrap();
            cpu.write_mem(table, &bytes).unwrap();
            cpu.write_mem(table + 5 * 4, &callback.to_le_bytes())
                .unwrap();
            cpu.write_mem(object, &table.to_le_bytes()).unwrap();
        }
        let help = k.controls.create(
            7,
            ControlClass::Button,
            0xe146,
            "Help".into(),
            0x50000000,
            0,
            0,
            40,
            20,
        );
        args(&mut cpu, &[0x2400]);
        let mut queries = Vec::new();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1939), |pc, cpu| {
                assert!(matches!(pc, 0x6100 | 0x6200));
                assert_eq!(cpu.read_arg(1).unwrap(), 0xe146);
                assert_eq!(cpu.read_arg(2).unwrap(), 0);
                assert_eq!(cpu.read_arg(3).unwrap(), 0);
                let sp = cpu.read_reg(ArmReg::Sp).unwrap();
                let info = cpu.read_u32_le(sp).unwrap();
                assert_ne!(
                    info, 0,
                    "Help availability must query, not execute a command"
                );
                queries.push(pc);
                u32::from(pc == 0x6100 && available)
            }),
            1
        );
        assert_eq!(k.controls.get(help).unwrap().visible, available);
        assert_eq!(
            queries,
            if available {
                vec![0x6100]
            } else {
                vec![0x6100, 0x6200]
            }
        );
    }
}
