//! Native-layout and archive contracts for the additional CE3 container families.
use super::archive::tests::Fixture;
use pocket_cpu::Cpu;
fn wide(f: &mut Fixture, at: u32, text: &str) {
    f.cpu
        .write_mem(
            at,
            &text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn text(f: &mut Fixture, cell: u32) -> Vec<u16> {
    let at = f.word(cell);
    let length = f.word(at - 8);
    f.cpu
        .read_mem(at, length * 2)
        .unwrap()
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
fn rewind(f: &mut Fixture) {
    f.call("ord:851", &[0x2300]);
    f.call("ord:212", &[0x2300]);
    f.call("ord:2251", &[0x2000, 0, 0]);
    f.construct(1, 0);
}
#[test]
fn dword_self_insert_copy_compact_and_raw_archive_preserve_values_and_release_storage() {
    let mut f = Fixture::new(&[]);
    f.call("ord:87", &[0x2400]);
    f.call("ord:2649", &[]);
    let baseline = f.kernel.heap.free_bytes();
    f.call("ord:2305", &[0x2400, 0, 0xfedcba98]);
    f.call("ord:2305", &[0x2400, 1, 0x76543210]);
    f.call("ord:1650", &[0x2400, 1, 0x2400]);
    let data = f.word(0x2404);
    let expected: Vec<_> = [0xfedcba98u32, 0xfedcba98, 0x76543210, 0x76543210]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert_eq!(f.cpu.read_mem(data, 16).unwrap(), expected);
    f.call("ord:1189", &[0x2400]);
    assert_eq!(f.word(0x240c), 4);
    f.construct(0, 0);
    f.call("ord:2284", &[0x2400, 0x2300]);
    rewind(&mut f);
    assert_eq!(
        std::fs::read(f.directory.path().join("archive.bin")).unwrap(),
        [vec![4, 0], expected].concat()
    );
    f.call("ord:2284", &[0x2400, 0x2300]);
    assert_eq!(f.word(0x2408), 4);
    f.call("ord:212", &[0x2300]);
    f.call("ord:237", &[0x2400]);
    assert_eq!(f.kernel.heap.free_bytes(), baseline);
}
#[test]
fn text_maps_own_both_strings_and_load_merge_while_word_maps_keep_borrowed_objects() {
    for string in [true, false] {
        let mut f = Fixture::new(&[]);
        let (ctor, subscript, serial, dtor) = if string {
            (125, 356, 2287, 261)
        } else {
            (126, 357, 2288, 262)
        };
        f.call(&format!("ord:{ctor}"), &[0x2400, 2]);
        f.call("ord:2649", &[]);
        let baseline = f.kernel.heap.free_bytes();
        wide(&mut f, 0x2800, "key");
        wide(&mut f, 0x2900, "value");
        let slot = f.call(
            &format!("ord:{subscript}"),
            &[0x2400, if string { 0x2800 } else { 0xabc0 }],
        );
        if string {
            f.call("ord:324", &[slot, 0x2900]);
        }
        // Null CObject serializes through the graph codec without a factory.
        else {
            f.cpu.write_mem(slot, &0u32.to_le_bytes()).unwrap();
        }
        let node = if string { slot - 12 } else { slot - 8 };
        if !string {
            assert_eq!(
                f.cpu.read_mem(node + 4, 2).unwrap(),
                0xabc0u16.to_le_bytes()
            );
            let buckets = f.word(0x2404);
            assert_eq!(f.word(buckets + ((0xabc0 >> 4) % 17) * 4), node);
        }
        f.construct(0, 0);
        f.call(&format!("ord:{serial}"), &[0x2400, 0x2300]);
        rewind(&mut f);
        f.call(&format!("ord:{serial}"), &[0x2400, 0x2300]);
        assert_eq!(f.word(0x240c), 1);
        if string {
            assert_eq!(
                text(&mut f, slot),
                "value".encode_utf16().collect::<Vec<_>>()
            );
        }
        f.call("ord:212", &[0x2300]);
        f.call(&format!("ord:{dtor}"), &[0x2400]);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
    }
}
#[test]
fn string_and_object_lists_share_plex_storage_append_on_load_and_release_only_owned_values() {
    for string in [true, false] {
        let mut f = Fixture::new(&[]);
        let (ctor, add, serial, dtor) = if string {
            (183, 597, 2293, 299)
        } else {
            (135, 592, 2291, 270)
        };
        f.call(&format!("ord:{ctor}"), &[0x2400, 2]);
        f.call("ord:2649", &[]);
        let baseline = f.kernel.heap.free_bytes();
        wide(&mut f, 0x2800, "first");
        wide(&mut f, 0x2900, "second");
        for arg in if string { [0x2800, 0x2900] } else { [0, 0] } {
            f.call(&format!("ord:{add}"), &[0x2400, arg]);
        }
        f.construct(0, 0);
        f.call(&format!("ord:{serial}"), &[0x2400, 0x2300]);
        rewind(&mut f);
        f.call(&format!("ord:{serial}"), &[0x2400, 0x2300]);
        assert_eq!(f.word(0x240c), 4);
        let first = f.word(0x2404);
        if string {
            assert_eq!(
                text(&mut f, first + 8),
                "first".encode_utf16().collect::<Vec<_>>()
            );
            let last = f.word(0x2408);
            assert_eq!(
                text(&mut f, last + 8),
                "second".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(f.call("ord:2202", &[0x2400, 0x2700]), 0x2700);
            assert_eq!(
                text(&mut f, 0x2700),
                "first".encode_utf16().collect::<Vec<_>>()
            );
            f.call("ord:297", &[0x2700]);
        }
        f.call("ord:212", &[0x2300]);
        f.call(&format!("ord:{dtor}"), &[0x2400]);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
    }
}
#[test]
fn edit_view_uses_actual_edit_text_selection_undo_tabs_and_utf16_archive() {
    use pocket_kernel::controls::ControlClass;
    let mut f = Fixture::new(&[]);
    f.call("ord:91", &[0x2400]);
    let h = f.kernel.controls.create(
        7,
        ControlClass::Edit,
        10,
        "original".into(),
        4,
        0,
        0,
        120,
        100,
    );
    f.cpu.write_mem(0x2420, &h.to_le_bytes()).unwrap();
    let buffer = f.call("ord:1732", &[0x2400]);
    assert_eq!(
        f.cpu.read_mem(buffer, 18).unwrap(),
        "original\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    f.call("SendMessageW", &[h, 0xb1, 0, u32::MAX]);
    wide(&mut f, 0x2800, "changed é😀");
    f.call("SendMessageW", &[h, 0xc2, 1, 0x2800]);
    assert_eq!(f.call("SendMessageW", &[h, 0xc6, 0, 0]), 1);
    f.call("ord:1890", &[0x2400]);
    assert_eq!(f.kernel.controls.get(h).unwrap().text, "original");
    f.call("ord:1890", &[0x2400]);
    assert_eq!(f.kernel.controls.get(h).unwrap().text, "changed é😀");
    f.call("ord:2427", &[0x2400, 24]);
    assert_eq!(f.kernel.controls.get(h).unwrap().edit.tab_stops, [24]);
    f.construct(0, 0);
    f.call("ord:2285", &[0x2400, 0x2300]);
    rewind(&mut f);
    let bytes = std::fs::read(f.directory.path().join("archive.bin")).unwrap();
    assert_eq!(&bytes[..4], &11u32.to_le_bytes());
    f.call("ord:1034", &[0x2400]);
    assert!(f.kernel.controls.get(h).unwrap().text.is_empty());
    f.call("ord:2285", &[0x2400, 0x2300]);
    assert_eq!(f.kernel.controls.get(h).unwrap().text, "changed é😀");
    f.call("ord:212", &[0x2300]);
    f.call("ord:240", &[0x2400]);
    assert!(f.kernel.controls.get(h).is_none());
}
#[test]
fn clipboard_transfers_global_blocks_converts_text_and_releases_all_owned_memory() {
    let mut f = Fixture::new(&[]);
    let baseline = f.kernel.heap.free_bytes();
    let memory = f.call("LocalAlloc", &[0, 8]);
    f.cpu.write_mem(memory, &[0x80, 0xe9, 0, 0]).unwrap();
    assert_eq!(f.call("SetClipboardData", &[1, memory]), 0);
    assert_eq!(f.call("OpenClipboard", &[0]), 1);
    assert_eq!(f.call("OpenClipboard", &[0]), 0);
    assert_eq!(f.call("SetClipboardData", &[1, memory]), memory);
    assert_eq!(f.call("IsClipboardFormatAvailable", &[13]), 1);
    let unicode = f.call("GetClipboardData", &[13]);
    assert_ne!(unicode, memory);
    assert_eq!(
        f.cpu.read_mem(unicode, 6).unwrap(),
        [0xac, 0x20, 0xe9, 0, 0, 0]
    );
    assert_eq!(f.call("GetClipboardData", &[13]), unicode);
    let checkpoint = f.kernel.capture_snapshot().unwrap();
    // The existing kernel checkpoint must preserve clipboard-owned guest handles.
    let (_, mut restored, _) = super::tests::framework_setup();
    restored.vfs.mount("\\", f.directory.path());
    checkpoint.restore(&mut restored).unwrap();
    f.kernel = restored;
    assert_eq!(f.call("GetClipboardData", &[13]), unicode);
    assert_eq!(f.call("EmptyClipboard", &[]), 1);
    assert_eq!(f.call("CloseClipboard", &[]), 1);
    assert!(f.kernel.heap.msize(memory).is_none());
    assert!(f.kernel.heap.msize(unicode).is_none());
    assert_eq!(f.kernel.heap.free_bytes(), baseline);
}
#[test]
fn document_lists_use_mounted_files_filters_selection_and_real_vfs_delete() {
    let mut f = Fixture::new(&[]);
    std::fs::write(f.directory.path().join("first.txt"), b"one").unwrap();
    std::fs::write(f.directory.path().join("second.txt"), b"two").unwrap();
    std::fs::write(f.directory.path().join("image.png"), b"image").unwrap();
    f.call("ord:206", &[0x3000]);
    let parent = 0xdead1234u32;
    f.cpu.write_mem(0x3020, &parent.to_le_bytes()).unwrap();
    f.kernel.window_classes.insert(parent, "FRAME".into());
    f.kernel.window_rects.insert(parent, (0, 0, 240, 320));
    f.call("ord:2693", &[0x2400]);
    wide(&mut f, 0x2800, "\\");
    wide(&mut f, 0x2900, "Text|*.txt|All|*.*||");
    assert_eq!(f.call("ord:2715", &[0x2400, 0x3000, 0x2900, 0x2800, 0]), 1);
    assert_eq!(f.call("ord:2726", &[0x2400]), 4);
    assert_eq!(f.call("ord:2725", &[0x2400]), 1);
    f.call("ord:2777", &[0x2400, 2]);
    assert_eq!(f.call("ord:2726", &[0x2400]), 2);
    wide(&mut f, 0x2800, "first.txt");
    f.call("ord:2780", &[0x2400, 0x2800]);
    assert_eq!(f.call("ord:2740", &[0x2400]), 1);
    assert_eq!(f.call("ord:2741", &[0x2400, 0x2c00, 260]), 1);
    assert_eq!(
        f.cpu.read_mem(0x2c00, 22).unwrap(),
        "\\first.txt\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(f.call("ord:2720", &[0x2400]), 1);
    assert!(!f.directory.path().join("first.txt").exists());
    assert!(f.directory.path().join("second.txt").exists());
    assert_eq!(f.call("ord:2726", &[0x2400]), 1);
    f.call("ord:2777", &[0x2400, 1]);
    assert_eq!(f.call("ord:2726", &[0x2400]), 3);
    let h = f.word(0x2420);
    let snapshot = f.kernel.capture_snapshot().unwrap();
    let (_, mut restored, _) = super::tests::framework_setup();
    restored.vfs.mount("\\", f.directory.path());
    snapshot.restore(&mut restored).unwrap();
    f.kernel = restored;
    assert_eq!(f.kernel.controls.doc_lists[&h].paths.len(), 3);
    let documents = f.directory.path().join("documents");
    std::fs::create_dir_all(documents.join("nested")).unwrap();
    std::fs::write(documents.join("nested/note.txt"), b"note").unwrap();
    f.kernel.vfs.mount("\\My Documents", &documents);
    wide(&mut f, 0x2800, "All Folders");
    f.call("ord:2778", &[0x2400, 0x2800]);
    assert_eq!(
        f.kernel.controls.doc_lists[&h].paths,
        ["\\My Documents\\nested\\note.txt"]
    );
    assert!(f.kernel.controls.doc_lists[&h].recursive);
    f.call("ord:2697", &[0x2400]);
    assert!(!f.kernel.controls.doc_lists.contains_key(&h));
    assert!(f.kernel.controls.get(h).is_none());
}
#[test]
fn editor_modeless_replace_dialog_routes_actions_and_survives_kernel_restore() {
    use pocket_kernel::controls::ControlClass;
    let mut f = Fixture::new(&[]);
    f.call("ord:91", &[0x2400]);
    let h = f.kernel.controls.create(
        7,
        ControlClass::Edit,
        10,
        "cat CAT cat".into(),
        4,
        0,
        0,
        240,
        240,
    );
    f.cpu.write_mem(0x2420, &h.to_le_bytes()).unwrap();
    f.call("ord:1888", &[0x2400]);
    let state = f.kernel.dynamic_exports[&1]["__PocketHLE_MFC_EditFindState:0"];
    let dialog = f.word(state + 20);
    assert_ne!(dialog, 0);
    let find = f.kernel.controls.by_id(dialog, 1001).unwrap().hwnd;
    let replacement = f.kernel.controls.by_id(dialog, 1002).unwrap().hwnd;
    wide(&mut f, 0x2800, "cat");
    f.call("SendMessageW", &[find, 0xc, 0, 0x2800]);
    wide(&mut f, 0x2800, "dog");
    f.call("SendMessageW", &[replacement, 0xc, 0, 0x2800]);
    f.call("SendMessageW", &[dialog, 0x111, 1006, 0]);
    assert_eq!(f.kernel.controls.get(h).unwrap().text, "dog dog dog");
    assert!(f.kernel.controls.get(h).unwrap().edit.undo.is_some());
    let snapshot = f.kernel.capture_snapshot().unwrap();
    let (_, mut restored, _) = super::tests::framework_setup();
    restored.vfs.mount("\\", f.directory.path());
    snapshot.restore(&mut restored).unwrap();
    f.kernel = restored;
    assert_eq!(f.word(state + 20), dialog);
    assert!(f.kernel.controls.by_id(dialog, 1002).is_some());
    f.call("ord:3563", &[0x2400]);
    assert_eq!(f.word(state + 20), 0);
    assert!(f.kernel.controls.panel(dialog).is_none());
    f.call("ord:1890", &[0x2400]);
    assert_eq!(f.kernel.controls.get(h).unwrap().text, "cat CAT cat");
    f.call("ord:1888", &[0x2400]);
    let dialog = f.word(state + 20);
    assert_ne!(dialog, 0);
    f.call("__PocketHLE_MFC_EditFindClear", &[]);
    assert!(f.kernel.heap.msize(state).is_none());
    assert!(!f.kernel.dynamic_exports[&1].contains_key("__PocketHLE_MFC_EditFindState:0"));
    assert!(f.kernel.controls.panel(dialog).is_none());
}
#[test]
fn document_list_template_owns_its_list_and_retires_borrowed_deferred_registration() {
    let mut f = Fixture::new(&[]);
    wide(&mut f, 0x2900, "\\");
    wide(&mut f, 0x2a00, "Text|*.txt||");
    f.call("ord:181", &[0x2800]);
    f.call("ord:181", &[0x2804]);
    f.call("ord:324", &[0x2800, 0x2900]);
    f.call("ord:324", &[0x2804, 0x2a00]);
    assert_eq!(
        f.call("ord:3553", &[0x2400, 0, 0, 0, 0, 0x2804, 0x2800]),
        0x2400
    );
    let list = f.word(0x2470);
    assert!(f.kernel.heap.msize(list).is_some());
    assert_eq!(f.word(list + 0x48), 0x2400);
    assert_eq!(
        text(&mut f, 0x246c),
        "Text|*.txt||".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(text(&mut f, 0x2478), vec![b'\\' as u16]);
    let registration = *f.kernel.dynamic_exports[&1]
        .get("__PocketHLE_MFC_TemplateRegistration")
        .unwrap_or_else(|| {
            panic!(
                "missing registration: {:?}",
                f.kernel.dynamic_exports[&1]
                    .keys()
                    .filter(|key| key.contains("Registration") || key.contains("Frame:"))
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(f.word(registration + 20), 1);
    f.call("ord:2698", &[0x2400]);
    assert!(f.kernel.heap.msize(list).is_none());
    assert_eq!(f.word(registration + 20), 0);
    f.call("ord:297", &[0x2800]);
    f.call("ord:297", &[0x2804]);
}

#[test]
fn repeated_scalar_insert_moves_overlapping_tails_in_bounded_chunks() {
    let mut f = Fixture::new(&[]);
    f.call("ord:87", &[0x2400]);
    f.call("ord:1649", &[0x2400, 0, 0x76543210, 2048]);
    f.call("ord:1649", &[0x2400, 1, 0xfedcba98, 3]);
    let data = f.word(0x2404);
    assert_eq!(f.word(0x2408), 2051);
    for index in 0..2051 {
        assert_eq!(
            f.word(data + index * 4),
            if (1..4).contains(&index) {
                0xfedcba98
            } else {
                0x76543210
            }
        );
    }
    f.call("ord:237", &[0x2400]);
}

#[test]
fn protected_map_associations_recycle_owned_strings_and_reuse_pool_nodes() {
    for (ctor, new, free, dtor, cells) in [(125, 1775, 1183, 261, 2), (126, 1776, 1184, 262, 0)] {
        let mut f = Fixture::new(&[]);
        f.call(&format!("ord:{ctor}"), &[0x2400, 2]);
        f.call("ord:2649", &[]);
        let baseline = f.kernel.heap.free_bytes();
        let node = f.call(&format!("ord:{new}"), &[0x2400]);
        wide(&mut f, 0x2800, "owned association");
        for offset in 0..cells {
            f.call("ord:324", &[node + 8 + offset * 4, 0x2800]);
        }
        f.call(&format!("ord:{free}"), &[0x2400, node]);
        assert_eq!(f.call(&format!("ord:{new}"), &[0x2400]), node);
        f.call(&format!("ord:{free}"), &[0x2400, node]);
        f.call(&format!("ord:{dtor}"), &[0x2400]);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
    }
}

#[test]
fn scalar_array_families_follow_a_sequence_model_and_release_storage_on_every_cycle() {
    for (ctor, set, insert, remove, append, copy, compact, dtor, stride) in [
        (87, 2305, 1649, 2186, 787, 891, 1189, 237, 4u32),
        (207, 2311, 1660, 2194, 792, 896, 1195, 314, 2),
        (134, 2306, 1652, 2187, 788, 892, 1190, 269, 4),
    ] {
        let mut f = Fixture::new(&[]);
        f.call(&format!("ord:{ctor}"), &[0x2400]);
        f.call(&format!("ord:{ctor}"), &[0x2500]);
        f.call("ord:2649", &[]);
        let baseline = f.kernel.heap.free_bytes();
        let mut seed = 0xabc98231u32;
        for cycle in 0..32 {
            let mut model = Vec::<u32>::new();
            for _ in 0..64 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let value = if stride == 2 { seed & 0xffff } else { seed };
                let at = (seed as usize >> 8) % (model.len() + 3);
                match seed % 4 {
                    0 => {
                        f.call(&format!("ord:{set}"), &[0x2400, at as u32, value]);
                        model.resize(model.len().max(at + 1), 0);
                        model[at] = value;
                    }
                    1 => {
                        f.call(&format!("ord:{insert}"), &[0x2400, at as u32, value, 2]);
                        model.resize(model.len().max(at), 0);
                        model.splice(at..at, [value, value]);
                    }
                    2 if !model.is_empty() => {
                        let at = at.min(model.len() - 1);
                        f.call(&format!("ord:{remove}"), &[0x2400, at as u32, 1]);
                        model.remove(at);
                    }
                    _ => {
                        f.call(&format!("ord:{compact}"), &[0x2400]);
                    }
                }
                let data = f.word(0x2404);
                assert_eq!(f.word(0x2408) as usize, model.len(), "cycle {cycle}");
                let actual = if model.is_empty() {
                    Vec::new()
                } else {
                    f.cpu.read_mem(data, model.len() as u32 * stride).unwrap()
                };
                let expected: Vec<_> = model
                    .iter()
                    .flat_map(|v| v.to_le_bytes().into_iter().take(stride as usize))
                    .collect();
                assert_eq!(actual, expected);
            }
            f.call(&format!("ord:{copy}"), &[0x2500, 0x2400]);
            assert_eq!(
                f.call(&format!("ord:{append}"), &[0x2500, 0x2500]),
                model.len() as u32
            );
            let data = f.word(0x2504);
            let bytes = model.len() as u32 * stride;
            assert_eq!(
                f.cpu.read_mem(data, bytes).unwrap(),
                f.cpu.read_mem(data + bytes, bytes).unwrap()
            );
            for object in [0x2400, 0x2500] {
                f.call(&format!("ord:{dtor}"), &[object]);
            }
            assert_eq!(
                f.kernel.heap.free_bytes(),
                baseline,
                "family {ctor}, cycle {cycle}"
            );
            for object in [0x2400, 0x2500] {
                f.call(&format!("ord:{ctor}"), &[object]);
            }
        }
    }
}
#[test]
fn compacting_string_cells_transfers_references_and_resolved_byte_destructor_releases_storage() {
    let mut f = Fixture::new(&[]);
    f.call("ord:182", &[0x2400]);
    f.call("ord:2649", &[]);
    f.call("ord:47", &[0x2600]);
    f.call("ord:218", &[0x2600]);
    let baseline = f.kernel.heap.free_bytes();
    wide(&mut f, 0x2800, "owned 😀");
    for _ in 0..128 {
        f.call("ord:2309", &[0x2400, 0, 0x2800]);
        let data = f.word(0x2404);
        let string = f.word(data);
        f.call("ord:2417", &[0x2400, 20, u32::MAX]);
        f.call("ord:2417", &[0x2400, 1, u32::MAX]);
        f.call("ord:1193", &[0x2400]);
        let data = f.word(0x2404);
        assert_eq!(f.word(data), string);
        assert_eq!(f.word(string - 12), 1);
        f.call("ord:298", &[0x2400]);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
        f.call("ord:182", &[0x2400]);
        f.call("ord:47", &[0x2600]);
        f.call("ord:2413", &[0x2600, 1024, u32::MAX]);
        assert_eq!(
            {
                use pocket_kernel::Dispatcher;
                f.cpu.write_arg(0, 0x2600).unwrap();
                let thunk = pocket_kernel::Thunk {
                    friendly_name: crate::ordinals::lookup("mfcce300.dll", 218),
                    ..super::window_dispatch::tests::entry(218)
                };
                f.dispatcher
                    .dispatch(&mut f.cpu, &thunk, &mut f.kernel)
                    .unwrap()
            },
            pocket_kernel::DispatchOutcome::ReturnedR0(0x2600)
        );
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
    }
}

#[test]
fn initialized_mfc_owner_stays_hle_when_a_dll_appears_and_after_checkpoint_restore() {
    let mut f = Fixture::new(&[]);
    f.kernel.module_search_dirs = vec![f.directory.path().to_path_buf()];
    let thunk = super::window_dispatch::tests::entry(2406);
    assert!(!crate::coredll::native_import_preferred(&f.kernel, &thunk));
    f.call("ord:644", &[]);
    let saved = f.kernel.capture_snapshot().unwrap();
    std::fs::write(f.directory.path().join("MFCCE300.DLL"), []).unwrap();
    assert!(f.kernel.find_module_file("mfcce300.dll").is_some());
    assert!(!crate::coredll::native_import_available(&f.kernel, &thunk));
    assert!(!crate::coredll::native_import_preferred(&f.kernel, &thunk));
    let (_, mut restored, _) = super::tests::framework_setup();
    restored.vfs.mount("\\", f.directory.path());
    restored.module_search_dirs = f.kernel.module_search_dirs.clone();
    saved.restore(&mut restored).unwrap();
    f.kernel = restored;
    assert!(f.kernel.find_module_file("mfcce300.dll").is_some());
    assert!(!crate::coredll::native_import_available(&f.kernel, &thunk));
    assert!(!crate::coredll::native_import_preferred(&f.kernel, &thunk));
    use pocket_kernel::Dispatcher;
    let unsupported = pocket_kernel::Thunk {
        binding: pocket_pe::ImportBinding::Name("FutureMissingMfcApi".into()),
        ..thunk
    };
    let error = f
        .dispatcher
        .dispatch(&mut f.cpu, &unsupported, &mut f.kernel)
        .unwrap_err();
    assert!(error.to_string().contains("Unsupported MFC API"));
    assert!(f.kernel.modules.is_empty());
}

#[test]
fn resource_format_and_pointer_format_share_the_variadic_engine_and_release_replaced_storage() {
    let mut f = Fixture::new(&[]);
    f.call("ord:644", &[]);
    let format = "item=%s, value=%d, hex=%04x";
    let mut units = vec![0, format.encode_utf16().count() as u16];
    units.extend(format.encode_utf16());
    let bytes: Vec<_> = units.into_iter().flat_map(u16::to_le_bytes).collect();
    f.cpu.write_mem(0x1000, &bytes).unwrap();
    f.kernel.resources.push(pocket_pe::ResourceEntry {
        ty: pocket_pe::ResourceKey::Id(6),
        name: pocket_pe::ResourceKey::Id(2),
        data_rva: 0x1000,
        native_leaves: vec![],
        size: bytes.len() as u32,
        codepage: 0,
    });
    wide(&mut f, 0x2800, "object");
    wide(&mut f, 0x2900, format);
    f.call("ord:181", &[0x2400]);
    let mut baseline = None;
    for _ in 0..32 {
        for (ordinal, source) in [(1172, 17), (1173, 0x2900)] {
            f.call(
                &format!("ord:{ordinal}"),
                &[0x2400, source, 0x2800, (-42i32) as u32, 0xbeef],
            );
            assert_eq!(
                String::from_utf16(&text(&mut f, 0x2400)).unwrap(),
                "item=object, value=-42, hex=beef"
            );
            let free = f.kernel.heap.free_bytes();
            assert_eq!(*baseline.get_or_insert(free), free);
        }
    }
    f.call("ord:1172", &[0x2400, 999]);
    assert!(text(&mut f, 0x2400).is_empty());
    f.call("ord:297", &[0x2400]);
}

#[test]
fn solid_brush_owns_the_real_gdi_resource_and_repeated_delete_retires_the_permanent_mapping() {
    let mut f = Fixture::new(&[]);
    let mut baseline = None;
    for _ in 0..128 {
        assert_eq!(f.call("ord:43", &[0x2400, 0x00_12_34_56]), 0x2400);
        let brush = f.word(0x2404);
        assert_eq!(f.kernel.gdi.brush(brush).unwrap().color, 0x00_12_34_56);
        assert_eq!(f.call("ord:409", &[]), f.call("ord:1476", &[0x2400]));
        assert_eq!(f.call("ord:1040", &[0x2400]), 1);
        assert_eq!(f.word(0x2404), 0);
        assert!(f.kernel.gdi.get(brush).is_none());
        assert!(!f.kernel.dynamic_exports.values().any(|exports| exports
            .keys()
            .any(|key| key.ends_with(&format!(":{brush:08x}")))));
        let free = f.kernel.heap.free_bytes();
        assert_eq!(*baseline.get_or_insert(free), free);
    }
}
