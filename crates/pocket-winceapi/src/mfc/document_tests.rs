//! Framework tests exercise actual callbacks, file bytes and ownership transitions.
use super::tests::{framework_setup, result};
use super::window_dispatch::tests::{drive, drive_outcomes, entry};
use crate::{bluetooth::tests::call, WinCeDispatcher};
use pocket_cpu::{regs::ArmReg, Cpu};
use pocket_kernel::{DispatchOutcome, Dispatcher, KernelState, Thunk};
fn arguments(cpu: &mut dyn Cpu, args: &[u32]) {
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
}
fn run(
    cpu: &mut dyn Cpu,
    k: &mut KernelState,
    d: &mut WinCeDispatcher,
    ordinal: u16,
    args: &[u32],
) -> u32 {
    arguments(cpu, args);
    drive(cpu, k, d, entry(ordinal), |pc, _| {
        panic!("unexpected guest {pc:x}")
    })
}
fn wide(cpu: &mut dyn Cpu, p: u32, s: &str) {
    cpu.write_mem(
        p,
        &s.encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
fn assert_no_frames(k: &KernelState) {
    assert!(!k.dynamic_exports[&1]
        .keys()
        .any(|name| name.starts_with("__PocketHLE_MFC_Frame:")));
}
#[test]
fn deferred_static_templates_are_adopted_but_remain_borrowed_at_application_teardown() {
    let (mut cpu, mut k, mut d) = framework_setup();
    wide(&mut cpu, 0x1000, "Application");
    assert_eq!(
        run(&mut cpu, &mut k, &mut d, 162, &[0x2000, 100, 0, 0, 0]),
        0x2000
    );
    assert_eq!(cpu.read_u32_le(0x2020).unwrap(), 0);
    run(&mut cpu, &mut k, &mut d, 201, &[0x2200, 0x1000]);
    assert_eq!(run(&mut cpu, &mut k, &mut d, 1622, &[0x2200]), 1);
    let manager = cpu.read_u32_le(0x2280).unwrap();
    assert_ne!(manager, 0);
    let position = run(&mut cpu, &mut k, &mut d, 1301, &[0x2200]);
    assert_ne!(position, 0);
    cpu.write_mem(0x2800, &position.to_le_bytes()).unwrap();
    assert_eq!(
        run(&mut cpu, &mut k, &mut d, 1393, &[0x2200, 0x2800]),
        0x2000
    );
    assert_eq!(cpu.read_u32_le(0x2800).unwrap(), 0);
    run(&mut cpu, &mut k, &mut d, 85, &[0x2600]);
    run(&mut cpu, &mut k, &mut d, 574, &[0x2000, 0x2600]);
    assert_eq!(cpu.read_u32_le(0x2628).unwrap(), 0x2000);
    assert_eq!(run(&mut cpu, &mut k, &mut d, 1299, &[0x2000]), u32::MAX);
    run(&mut cpu, &mut k, &mut d, 236, &[0x2600]);
    assert_eq!(cpu.read_u32_le(0x2068).unwrap(), 0);
    run(&mut cpu, &mut k, &mut d, 310, &[0x2200]);
    assert!(k.heap.msize(manager).is_none());
    // The static template is still alive, with its CString owned until its caller's destructor.
    assert_ne!(cpu.read_u32_le(0x2064).unwrap(), 0);
    run(&mut cpu, &mut k, &mut d, 287, &[0x2000]);
    assert_eq!(cpu.read_u32_le(0x2064).unwrap(), 0);
    assert_no_frames(&k);
}
#[test]
fn dynamic_templates_are_deleted_once_by_the_application_owned_manager() {
    let (mut cpu, mut k, mut d) = framework_setup();
    wide(&mut cpu, 0x1000, "Application");
    run(&mut cpu, &mut k, &mut d, 201, &[0x2200, 0x1000]);
    run(&mut cpu, &mut k, &mut d, 1622, &[0x2200]);
    let template = result(call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:315",
        &[108],
    ));
    run(&mut cpu, &mut k, &mut d, 162, &[template, 100, 0, 0, 0]);
    assert_eq!(cpu.read_u32_le(template + 0x20).unwrap(), 1);
    run(&mut cpu, &mut k, &mut d, 572, &[0x2200, template]);
    let manager = cpu.read_u32_le(0x2280).unwrap();
    run(&mut cpu, &mut k, &mut d, 310, &[0x2200]);
    assert!(k.heap.msize(template).is_none());
    assert!(k.heap.msize(manager).is_none());
    assert_no_frames(&k);
}
#[test]
fn template_confidence_and_document_paths_use_native_extension_and_existing_identity() {
    let (mut cpu, mut k, mut d) = framework_setup();
    wide(&mut cpu, 0x1000, "Application");
    run(&mut cpu, &mut k, &mut d, 201, &[0x2200, 0x1000]);
    run(&mut cpu, &mut k, &mut d, 1622, &[0x2200]);
    run(&mut cpu, &mut k, &mut d, 162, &[0x2000, 100, 0, 0, 0]);
    wide(
        &mut cpu,
        0x1100,
        "Window\nLevel\nNew Level\nLevel files\n.ext\nLevel.Type\nLevel data",
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "mfcce300.dll",
        "ord:324",
        &[0x2064, 0x1100],
    );
    run(&mut cpu, &mut k, &mut d, 85, &[0x2600]);
    run(&mut cpu, &mut k, &mut d, 574, &[0x2000, 0x2600]);
    wide(&mut cpu, 0x1300, "\\Data\\Level.EXT");
    run(&mut cpu, &mut k, &mut d, 2392, &[0x2600, 0x1300, 0]);
    // CE3's GetFileTitle preserves the extension; it is not the desktop shell's hidden-extension policy.
    let title = cpu.read_u32_le(0x2620).unwrap();
    let bytes = cpu.read_mem(title, 20).unwrap();
    assert_eq!(
        String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .take_while(|&c| c != 0)
                .collect::<Vec<_>>()
        )
        .unwrap(),
        "Level.EXT"
    );
    for (input, confidence, doc) in [
        ("\\data\\level.ext", 5, 0x2600),
        ("\\Data\\Other.EXT", 4, 0),
        ("\\Data\\Other.bin", 3, 0),
    ] {
        wide(&mut cpu, 0x1400, input);
        assert_eq!(
            run(&mut cpu, &mut k, &mut d, 1755, &[0x2000, 0x1400, 0x2800]),
            confidence
        );
        assert_eq!(cpu.read_u32_le(0x2800).unwrap(), doc);
        assert_no_frames(&k);
    }
    run(&mut cpu, &mut k, &mut d, 236, &[0x2600]);
    run(&mut cpu, &mut k, &mut d, 287, &[0x2000]);
    run(&mut cpu, &mut k, &mut d, 310, &[0x2200]);
}
fn api(ordinal: u16) -> Thunk {
    Thunk {
        thunk_va: 0x7000f000 + u32::from(ordinal) * 4,
        iat_va: 0,
        dll: "mfcce300.dll".into(),
        binding: pocket_pe::ImportBinding::Ordinal(ordinal),
        friendly_name: None,
    }
}
#[test]
fn document_save_load_reuses_real_archives_and_mirror_files_without_leaking_or_sticking_wait_cursor(
) {
    let (mut cpu, mut k, mut d) = framework_setup();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("state.bin"), [1, 2, 3]).unwrap();
    k.vfs.mount("\\Data\\", dir.path());
    wide(&mut cpu, 0x1000, "Application");
    wide(&mut cpu, 0x1200, "\\Data\\state.bin");
    run(&mut cpu, &mut k, &mut d, 201, &[0x2200, 0x1000]);
    run(&mut cpu, &mut k, &mut d, 85, &[0x2000]);
    let v = cpu.read_u32_le(0x2000).unwrap();
    let bytes = cpu.read_mem(v, 45 * 4).unwrap();
    cpu.write_mem(0x2800, &bytes).unwrap();
    cpu.write_mem(0x2800 + 8, &0x6100u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x2800 + 28 * 4, &0x6200u32.to_le_bytes())
        .unwrap();
    cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
    let expected = [4u8, 8, 15, 16, 23, 42, 0, 255];
    cpu.write_mem(0x1600, &expected).unwrap();
    let arrow = pocket_kernel::controls::system_cursor_handle(32512).unwrap();
    k.controls.cursor_handle = arrow;
    let mut baseline = None;
    for _ in 0..32 {
        for (saving, ordinal) in [(true, 2001), (false, 1973)] {
            arguments(&mut cpu, &[0x2000, 0x1200]);
            let mut serialized = 0;
            let mut deleted = 0;
            assert_eq!(
                drive_outcomes(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    entry(ordinal),
                    1024,
                    |pc, cpu, k, d| {
                        let io = api(if saving { 2582 } else { 2135 });
                        if pc == io.thunk_va {
                            return d.dispatch(cpu, &io, k).unwrap();
                        }
                        match pc {
                            0x6100 => {
                                serialized += 1;
                                assert_eq!(
                                    k.controls.cursor_handle,
                                    pocket_kernel::controls::system_cursor_handle(32514).unwrap()
                                );
                                let archive = cpu.read_arg(1).unwrap();
                                assert_eq!(cpu.read_u32_le(archive).unwrap(), 0x2000);
                                arguments(cpu, &[archive, if saving { 0x1600 } else { 0x1700 }, 8]);
                                // Serialize tail-calls archive I/O; the real saved LR
                                // returns to document I/O after every virtual refill.
                                d.dispatch(cpu, &api(if saving { 2582 } else { 2135 }), k)
                                    .unwrap()
                            }
                            0x6200 => {
                                deleted += 1;
                                DispatchOutcome::ReturnedR0(0)
                            }
                            _ => panic!("unexpected {pc:x}"),
                        }
                    }
                ),
                1
            );
            assert_eq!(serialized, 1);
            assert_eq!(deleted, u32::from(!saving));
            assert_eq!(k.controls.cursor_handle, arrow);
            assert_eq!(cpu.read_u32_le(0x229c).unwrap(), 0);
            assert_eq!(cpu.read_u32_le(0x2048).unwrap(), 0);
            if !saving {
                assert_eq!(cpu.read_mem(0x1700, 8).unwrap(), expected);
            }
            assert_no_frames(&k);
        }
        assert_eq!(
            std::fs::read(dir.path().join("state.bin")).unwrap(),
            expected
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        if let Some(n) = baseline {
            assert_eq!(k.heap.free_bytes(), n);
        }
        baseline = Some(k.heap.free_bytes());
    }
    run(&mut cpu, &mut k, &mut d, 236, &[0x2000]);
    run(&mut cpu, &mut k, &mut d, 310, &[0x2200]);
}
#[test]
fn interrupted_document_save_aborts_temporary_data_and_reports_the_real_caught_exception() {
    let (mut cpu, mut k, mut d) = framework_setup();
    let dir = tempfile::tempdir().unwrap();
    let original = [9u8, 8, 7];
    std::fs::write(dir.path().join("state.bin"), original).unwrap();
    k.vfs.mount("\\Data\\", dir.path());
    wide(&mut cpu, 0x1000, "Application");
    wide(&mut cpu, 0x1200, "\\Data\\state.bin");
    run(&mut cpu, &mut k, &mut d, 201, &[0x2200, 0x1000]);
    run(&mut cpu, &mut k, &mut d, 85, &[0x2000]);
    let v = cpu.read_u32_le(0x2000).unwrap();
    let bytes = cpu.read_mem(v, 45 * 4).unwrap();
    cpu.write_mem(0x2800, &bytes).unwrap();
    for (slot, pc) in [(2, 0x6100u32), (33, 0x6300)] {
        cpu.write_mem(0x2800 + slot * 4, &pc.to_le_bytes()).unwrap();
    }
    cpu.write_mem(0x2000, &0x2800u32.to_le_bytes()).unwrap();
    let mut baseline = None;
    for _ in 0..3 {
        arguments(&mut cpu, &[0x2000, 0x1200]);
        let mut reports = 0;
        assert_eq!(
            drive_outcomes(
                &mut cpu,
                &mut k,
                &mut d,
                entry(2001),
                1024,
                |pc, cpu, k, d| match pc {
                    0x6100 => {
                        arguments(cpu, &[3, 0x1200]);
                        d.dispatch(cpu, &api(705), k).unwrap()
                    }
                    0x6300 => {
                        reports += 1;
                        let exception = cpu.read_arg(2).unwrap();
                        assert_eq!(cpu.read_u32_le(exception + 8).unwrap(), 3);
                        assert_eq!(cpu.read_arg(3).unwrap(), 1);
                        DispatchOutcome::ReturnedR0(0)
                    }
                    _ => panic!("unexpected {pc:x}"),
                }
            ),
            0
        );
        assert_eq!(reports, 1);
        assert_eq!(
            std::fs::read(dir.path().join("state.bin")).unwrap(),
            original
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(cpu.read_u32_le(0x229c).unwrap(), 0);
        assert_no_frames(&k);
        if let Some(n) = baseline {
            assert_eq!(k.heap.free_bytes(), n);
        }
        baseline = Some(k.heap.free_bytes());
    }
    run(&mut cpu, &mut k, &mut d, 236, &[0x2000]);
    run(&mut cpu, &mut k, &mut d, 310, &[0x2200]);
}

#[test]
fn document_list_open_reuses_background_view_and_rolls_back_rejected_new_documents() {
    for accept in [false, true] {
        let (mut cpu, mut k, mut d) = framework_setup();
        wide(&mut cpu, 0x1000, "Application");
        run(&mut cpu, &mut k, &mut d, 201, &[0x2200, 0x1000]);
        run(&mut cpu, &mut k, &mut d, 103, &[0x3000]);
        run(&mut cpu, &mut k, &mut d, 200, &[0x3200]);
        cpu.write_mem(0x2220, &0x3000u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x30ac, &0x3200u32.to_le_bytes()).unwrap();
        run(&mut cpu, &mut k, &mut d, 181, &[0x2800]);
        run(&mut cpu, &mut k, &mut d, 181, &[0x2804]);
        let base_class = run(&mut cpu, &mut k, &mut d, 1462, &[0]);
        let descriptor = cpu.read_mem(base_class, 24).unwrap();
        cpu.write_mem(0x3600, &descriptor).unwrap();
        cpu.write_mem(0x360c, &0x6300u32.to_le_bytes()).unwrap();
        let document_class = 0x3600;
        run(
            &mut cpu,
            &mut k,
            &mut d,
            3553,
            &[0x2400, 0, document_class, 0, 0, 0x2800, 0x2804],
        );
        // Preserve actual constructors/destructors/AddDocument/AddView. Only the
        // application's OnNewDocument and InitialUpdateFrame virtuals are supplied.
        let owned_document = k.heap.alloc_cpp(88).unwrap();
        run(&mut cpu, &mut k, &mut d, 85, &[owned_document]);
        let doc_table = cpu.read_u32_le(owned_document).unwrap();
        cpu.write_mem(doc_table + 29 * 4, &0x6200u32.to_le_bytes())
            .unwrap();
        let template_table = cpu.read_u32_le(0x2400).unwrap();
        cpu.write_mem(template_table + 30 * 4, &0x6100u32.to_le_bytes())
            .unwrap();
        arguments(&mut cpu, &[0x2400, 0, 1]);
        let mut initial = 0;
        let document = drive(&mut cpu, &mut k, &mut d, entry(2769), |pc, cpu| {
            if pc == 0x6300 {
                owned_document
            } else if pc == 0x6200 {
                assert_eq!(cpu.read_u32_le(0x3248).unwrap(), cpu.read_arg(0).unwrap());
                u32::from(accept)
            } else {
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_arg(1).unwrap(), 0x3000);
                initial += 1;
                0
            }
        });
        assert_eq!(document != 0, accept);
        assert_eq!(initial, u32::from(accept));
        assert_eq!(cpu.read_u32_le(0x2220).unwrap(), 0x3000);
        assert_eq!(cpu.read_u32_le(0x30ac).unwrap(), 0x3200);
        assert_eq!(cpu.read_u32_le(0x2468).unwrap(), document);
        assert_eq!(cpu.read_u32_le(0x3248).unwrap(), document);
        if document != 0 {
            cpu.write_mem(document + 0x54, &1u32.to_le_bytes()).unwrap();
            let delete = cpu.read_u32_le(doc_table + 4).unwrap();
            let thunk = k.dynamic_exports[&1]
                .iter()
                .find(|(_, value)| **value == delete)
                .unwrap()
                .0
                .clone();
            arguments(&mut cpu, &[document, 1]);
            drive(
                &mut cpu,
                &mut k,
                &mut d,
                Thunk {
                    thunk_va: delete,
                    iat_va: 0,
                    dll: "coredll.dll".into(),
                    binding: pocket_pe::ImportBinding::Name(thunk),
                    friendly_name: None,
                },
                |pc, _| panic!("unexpected {pc:x}"),
            );
        }
        assert_eq!(cpu.read_u32_le(0x3248).unwrap(), 0);
        run(&mut cpu, &mut k, &mut d, 2698, &[0x2400]);
        assert!(k.heap.msize(owned_document).is_none());
        assert_no_frames(&k);
    }
}
