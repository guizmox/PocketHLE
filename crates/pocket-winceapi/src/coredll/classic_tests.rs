use super::*;
use pocket_cpu::{stub::StubCpu, Cpu};
use pocket_kernel::{controls::ControlClass, framebuffer::Framebuffer, windowing};

const HWND: u32 = 0xdead0080;

#[test]
fn intersecting_a_clip_preserves_saved_regions_and_never_publishes_pixels() {
    let mut cpu = tests::scanf_cpu();
    let mut kernel = tests::fresh_kernel();
    let dc = kernel.gdi.create_memory_dc();
    let bitmap = kernel.gdi.create_compatible_bitmap(100, 80);
    kernel.gdi.dc_mut(dc).unwrap().selected_bitmap = Some(bitmap);
    let regions = vec![[0, 0, 25, 30], [60, 40, 100, 80]];
    kernel.gdi.dc_mut(dc).unwrap().clip_rects = Some(regions.clone());
    let saved = kernel.gdi.save_dc(dc).unwrap() as i32;
    let before = kernel.framebuffer.authority_epoch();
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for (i, value) in [dc, 10, 10, 75].into_iter().enumerate() {
        ctx.cpu.write_arg(i as u8, value).unwrap();
    }
    let sp = ctx.cpu.read_reg(ArmReg::Sp).unwrap();
    ctx.cpu.write_mem(sp, &60u32.to_le_bytes()).unwrap();
    assert_eq!(
        ce_gdi::intersect_clip_rect(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(3)
    );
    assert_eq!(
        ctx.kernel.gdi.dc(dc).unwrap().clip_rects,
        Some(vec![[10, 10, 25, 30], [60, 40, 75, 60]])
    );
    ctx.cpu.write_arg(1, 80).unwrap();
    assert_eq!(
        ce_gdi::intersect_clip_rect(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.kernel.gdi.dc(dc).unwrap().clip_rects, Some(vec![]));
    assert!(ctx.kernel.gdi.restore_dc(dc, saved));
    assert_eq!(ctx.kernel.gdi.dc(dc).unwrap().clip_rects, Some(regions));
    assert_eq!(ctx.kernel.framebuffer.authority_epoch(), before);
    ctx.cpu.write_arg(0, 0xabcdef).unwrap();
    assert_eq!(
        ce_gdi::intersect_clip_rect(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
}

#[test]
fn control_panel_dynamic_icons_use_cpl_resources_and_survive_image_list_copying() {
    use crate::bluetooth::tests::{call, setup};
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    let base = 0x31000000;
    cpu.map_region(base, 0x1000, Prot::READ | Prot::WRITE)
        .unwrap();
    // CE6 control.exe obtains a caller-owned HICON in NEWCPLINFOW, copies it
    // into both image lists, then destroys it. The CPL has its own RT_ICON scope.
    let mut dib = vec![0u8; 48];
    for (offset, value) in [(0, 40u32), (4, 1), (8, 2)] {
        dib[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    dib[40..44].copy_from_slice(&[0, 0, 255, 0]);
    let mut group = vec![0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0];
    group.extend_from_slice(&(dib.len() as u32).to_le_bytes());
    group.extend_from_slice(&9u16.to_le_bytes());
    cpu.write_mem(base + 0x100, &group).unwrap();
    cpu.write_mem(base + 0x200, &dib).unwrap();
    kernel.modules.push(LoadedModule {
        data_file: false,
        dependencies: Vec::new(),
        attached: true,
        satellites: Vec::new(),
        resident_regions: vec![(base, 0x1000)],
        module_slot: Some(base),
        handle: base,
        name: "cplmain.cpl".into(),
        guest_path: r"\Windows\cplmain.cpl".into(),
        base,
        image_size: 0x1000,
        image_entry: 0,
        refcount: 1,
        resources: vec![
            ResourceEntry {
                ty: ResourceKey::Id(14),
                name: ResourceKey::Id(5040),
                data_rva: 0x100,
                native_leaves: Vec::new(),
                size: group.len() as u32,
                codepage: 0,
            },
            ResourceEntry {
                ty: ResourceKey::Id(3),
                name: ResourceKey::Id(9),
                data_rva: 0x200,
                native_leaves: Vec::new(),
                size: dib.len() as u32,
                codepage: 0,
            },
        ],
    });
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "cplmain.cpl",
            "CPlApplet",
            &[0, 8, 0, 0x2000]
        ),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(cpu.read_u32_le(0x2000).unwrap(), 468);
    let icon = cpu.read_u32_le(0x2010).unwrap();
    assert_ne!(icon, 0);
    let DispatchOutcome::ReturnedR0(list) = call(
        &mut cpu,
        &mut kernel,
        &mut dispatcher,
        "coredll.dll",
        "ImageList_Create",
        &[16, 16, 1, 1, 1],
    ) else {
        panic!("missing image list")
    };
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "ImageList_ReplaceIcon",
            &[list, u32::MAX, icon]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "DestroyIcon",
            &[icon]
        ),
        DispatchOutcome::ReturnedR0(1)
    );
    assert!(!kernel.gdi.icons.contains_key(&icon));
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "ImageList_Draw",
            &[list, 0, GDI_SCREEN_DC, 10, 12, 0]
        ),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(pixel(&kernel, 10, 12), 0xf800);
    assert_eq!(pixel(&kernel, 25, 27), 0xf800);
}

#[test]
fn is_child_walks_native_and_hle_children_without_following_popup_owners() {
    let mut cpu = StubCpu::new();
    let mut state = framed_kernel();
    let child = HWND + 1;
    let popup = HWND + 2;
    state.window_classes.insert(child, "DefShellView".into());
    state.window_styles.insert(child, (0x50000000, 0));
    state.window_parents.insert(child, HWND);
    state.window_classes.insert(popup, "Popup".into());
    state.window_styles.insert(popup, (0x90000000, 0));
    state.window_parents.insert(popup, HWND);
    let edit = state.controls.create(
        child,
        ControlClass::Edit,
        1,
        String::new(),
        0x50000000,
        0,
        0,
        40,
        20,
    );
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    for (parent, descendant, expected) in [
        (HWND, child, 1),
        (HWND, edit, 1),
        (child, edit, 1),
        (HWND, popup, 0),
        (child, HWND, 0),
        (HWND, HWND, 0),
        (0, child, 0),
        (HWND, 0, 0),
        (HWND, 0x9999, 0),
    ] {
        ctx.cpu.write_arg(0, parent).unwrap();
        ctx.cpu.write_arg(1, descendant).unwrap();
        assert_eq!(
            is_child(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(expected)
        );
    }
    ctx.kernel.window_parents.insert(child, child);
    ctx.cpu.write_arg(0, HWND).unwrap();
    ctx.cpu.write_arg(1, edit).unwrap();
    assert_eq!(is_child(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
}

#[test]
fn native_caret_is_clipped_to_the_client_and_hidden_or_unchanged_carets_do_not_publish() {
    let mut state = framed_kernel();
    state.controls.focus = HWND;
    state.ce.carets.blink_ms = u32::MAX;
    let origin = state.window_client_origin(HWND).unwrap();
    state.composite_controls();
    let base = pixel(&state, origin.0 as usize, origin.1 as usize);
    let caption = pixel(&state, origin.0 as usize, origin.1 as usize - 1);
    let before = state.framebuffer.frame_counter;
    state.ce.carets.threads.insert(
        0,
        pocket_kernel::caret::Caret {
            hwnd: HWND,
            position: (-1, -1),
            size: (2, 2),
            hidden: 1,
            gray: false,
            mask: None,
            epoch: std::time::Instant::now(),
        },
    );
    state.composite_controls();
    assert_eq!(state.framebuffer.frame_counter, before);
    state.ce.carets.threads.get_mut(&0).unwrap().hidden = 0;
    state.composite_controls();
    assert_eq!(
        pixel(&state, origin.0 as usize, origin.1 as usize),
        base ^ u16::MAX
    );
    assert_eq!(
        pixel(&state, origin.0 as usize, origin.1 as usize - 1),
        caption
    );
    let shown = state.framebuffer.frame_counter;
    state.composite_controls();
    assert_eq!(state.framebuffer.frame_counter, shown);
    state.ce.carets.threads.get_mut(&0).unwrap().hidden = 1;
    state.composite_controls();
    assert_eq!(pixel(&state, origin.0 as usize, origin.1 as usize), base);
    state.discard_window_metadata(&HWND);
    assert!(state.ce.carets.threads.is_empty());
}

#[test]
fn native_rich_edit_text_input_requires_the_active_enabled_visible_edit() {
    let mut state = framed_kernel();
    state.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
    let edit = HWND + 1;
    state.window_classes.insert(edit, "RichEdit20W".into());
    state.window_procs.insert(edit, 0x2000);
    state.window_styles.insert(edit, (0x50000000, 0));
    state.window_parents.insert(edit, HWND);
    state.controls.focus = edit;
    assert_eq!(state.text_input_target(), Some(edit));
    state.window_styles.get_mut(&edit).unwrap().0 |= 0x800;
    assert!(!state.accepts_text_input());
    state.window_styles.get_mut(&edit).unwrap().0 &= !0x800;
    state.window_visibility.insert(HWND, false);
    assert!(!state.accepts_text_input());
    state.window_visibility.insert(HWND, true);
    state.window_styles.get_mut(&HWND).unwrap().0 |= 0x08000000;
    assert!(!state.accepts_text_input());
    state.window_styles.get_mut(&HWND).unwrap().0 &= !0x08000000;
    state.active_window = HWND + 2;
    assert!(!state.accepts_text_input());
}

#[test]
fn ole_rectangle_scaling_rounds_signed_ties_and_rejects_long_overflow() {
    let mut cpu = tests::scanf_cpu();
    let mut kernel = tests::fresh_kernel();
    kernel.thread_last_errors.insert(0, 123);
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for (a, b, divisor, expected) in [
        (5, 1, 2, 3),
        (-5, 1, 2, -3),
        (5, 1, -2, -3),
        (-5, -1, -2, -3),
        (4, 1, 3, 1),
        (i32::MIN, 1, 1, i32::MIN),
        (i32::MIN, i32::MIN, i32::MIN, i32::MIN),
        (i32::MAX, 2, 2, i32::MAX),
        (i32::MAX, 2, 1, -1),
        (1, 1, 0, -1),
    ] {
        message(&mut ctx, a as u32, b as u32, divisor as u32, 0);
        assert_eq!(
            mul_div(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(expected as u32)
        );
    }
    assert_eq!(ctx.kernel.thread_last_errors.get(&0), Some(&123));
}

#[test]
fn caption_uses_class_and_window_icons_with_transparency_and_repaints_on_change() {
    let mut state = framed_kernel();
    let mut icon = pocket_kernel::imaging::Image::new(2, 2).unwrap();
    icon.pixels.fill(0xf800);
    icon.alpha[0] = 0;
    let class_icon = state.gdi.register_icon(icon.clone());
    state
        .gdi
        .class_values
        .insert(("test".into(), -14), class_icon);
    state.composite_controls();
    let rect = state.window_rects[&HWND];
    let metrics = state.window_metrics(HWND);
    let x = (rect.0 + metrics.border + 4) as usize;
    let y = (rect.1 + metrics.border + (metrics.caption - 16) / 2) as usize;
    assert_eq!(
        pixel(&state, x, y),
        pocket_kernel::framebuffer::colorref_to_rgb565(windowing::system_color(3))
    );
    assert_eq!(pixel(&state, x + 15, y + 15), 0xf800);
    icon.pixels.fill(0x07e0);
    let window_icon = state.gdi.register_icon(icon);
    state.ce.window_icons.insert(HWND, [window_icon, 0]);
    state.composite_controls();
    assert_eq!(pixel(&state, x + 15, y + 15), 0x07e0);
}

#[test]
fn control_panel_reserves_pending_input_for_its_parked_applet_call() {
    let mut cpu = tests::scanf_cpu();
    let mut kernel = framed_kernel();
    let thunk = tests::dummy_thunk();
    kernel.ce.control_panel = Some(pocket_kernel::ce::ControlPanel {
        hwnd: HWND,
        thread: 1,
        thunk: thunk.thunk_va,
        frame: GuestCallFrame {
            args: [0; 4],
            sp: 0,
            lr: 0,
        },
        applet: 0,
        previous_active: 0,
        previous_focus: 0,
        fields: Vec::new(),
        sound_schemes: Vec::new(),
        tab: 0,
        pages: Vec::new(),
        selected_page: 0,
        status: 0,
    });
    let click = InputEvent::PointerDown { x: 60, y: 100 };
    kernel.pending_input.push_back(click);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(take_pending_input(&mut ctx), None);
    assert_eq!(ctx.kernel.pending_input.front(), Some(&click));
    ctx.kernel.current_thread = 1;
    assert_eq!(take_pending_input(&mut ctx), Some(click));
}

#[test]
fn start_tracking_reserves_input_from_other_ui_pumps_and_filtered_peeks() {
    use pocket_kernel::shell_menu::TrackingMenu;
    let mut cpu = StubCpu::new();
    cpu.map_region(0x2000, 0x1000, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut kernel = framed_kernel();
    kernel.window_threads.insert(HWND, 0);
    let thunk = tests::dummy_thunk();
    kernel.gdi.shell_menus.begin_tracking(
        TrackingMenu {
            thread: 1,
            thunk: thunk.thunk_va,
            frame: GuestCallFrame {
                args: [0; 4],
                sp: 0,
                lr: 0,
            },
            owner: HWND,
            flags: 0,
            point: (0, 455),
            result: None,
            callback: None,
            scratch: 0,
            paints: Vec::new(),
        },
        10,
    );
    let click = InputEvent::PointerDown { x: 60, y: 100 };
    kernel.pending_input.push_back(click);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(take_pending_input(&mut ctx), None);
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, 0, 0x200, 0x202, 3).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(ctx.kernel.pending_input.front(), Some(&click));
    ctx.kernel.current_thread = 1;
    ctx.kernel.gdi.shell_menus.tracking.as_mut().unwrap().thunk += 32;
    assert_eq!(take_pending_input(&mut ctx), None); // nested owner callback
    ctx.kernel.gdi.shell_menus.tracking.as_mut().unwrap().thunk = thunk.thunk_va;
    assert_eq!(take_pending_input(&mut ctx), Some(click));
    assert!(ctx.kernel.pending_input.is_empty());
}

#[test]
fn ce_resource_pointers_terminate_standard_strings_and_expire_with_their_module() {
    use crate::bluetooth::tests::{call, setup};
    use pocket_kernel::LoadedModule;
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    let base = 0x30000000;
    cpu.map_region(base, 0x1000, Prot::READ | Prot::WRITE)
        .unwrap();
    let id = 0x8a08;
    let mut block = Vec::new();
    let mut terminated_va = 0;
    for index in 0..16 {
        let units: Vec<_> = match index {
            8 => "E&xit Explorer".encode_utf16().collect(),
            9 => "Programs".encode_utf16().chain(Some(0)).collect(),
            _ => Vec::new(),
        };
        block.extend_from_slice(&(units.len() as u16).to_le_bytes());
        if index == 9 {
            terminated_va = base + 0x100 + block.len() as u32;
        }
        block.extend(units.into_iter().flat_map(u16::to_le_bytes));
    }
    cpu.write_mem(base + 0x100, &block).unwrap();
    kernel.modules.push(LoadedModule {
        data_file: true,
        dependencies: Vec::new(),
        attached: false,
        satellites: Vec::new(),
        resident_regions: vec![(base, 0x1000)],
        module_slot: Some(base),
        handle: base,
        name: "strings.dll".into(),
        guest_path: r"\Windows\strings.dll".into(),
        base,
        image_size: 0x1000,
        image_entry: 0,
        refcount: 1,
        resources: vec![ResourceEntry {
            ty: ResourceKey::Id(6),
            name: ResourceKey::Id((id >> 4) + 1),
            data_rva: 0x100,
            native_leaves: Vec::new(),
            size: block.len() as u32,
            codepage: 0,
        }],
    });
    let DispatchOutcome::ReturnedR0(copy) = call(
        &mut cpu,
        &mut kernel,
        &mut dispatcher,
        "coredll.dll",
        "LoadStringW",
        &[base, id, 0, 0],
    ) else {
        panic!("expected resource pointer");
    };
    assert_ne!(copy, 0);
    let expected: Vec<_> = "E&xit Explorer"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(cpu.read_mem(copy, expected.len() as u32).unwrap(), expected);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "LoadStringW",
            &[base, id, 0, 0]
        ),
        DispatchOutcome::ReturnedR0(copy)
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "LoadStringW",
            &[base, id + 1, 0, 0]
        ),
        DispatchOutcome::ReturnedR0(terminated_va)
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "LoadStringW",
            &[base, id + 2, 0, 0]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "LoadStringW",
            &[base, id, 0x2000, 5]
        ),
        DispatchOutcome::ReturnedR0(4)
    );
    assert_eq!(
        cpu.read_mem(0x2000, 10).unwrap(),
        "E&xi"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        cpu.read_mem(base + 0x100, block.len() as u32).unwrap(),
        block
    );
    let other_copy = kernel.heap.alloc(16).unwrap();
    kernel.gdi.resource_strings.insert(0x31000100, other_copy);
    kernel.unload_runtime_module(&mut cpu, base).unwrap();
    assert_eq!(kernel.heap.msize(copy), None);
    assert!(!kernel
        .gdi
        .resource_strings
        .values()
        .any(|&pointer| pointer == copy));
    assert_eq!(
        kernel.gdi.resource_strings.get(&0x31000100),
        Some(&other_copy)
    );
    assert_eq!(kernel.heap.msize(other_copy), Some(16));
    assert!(cpu.read_mem(base + 0x100, 2).is_err());
}

#[test]
fn list_double_click_notification_carries_the_client_point_and_item_data() {
    let mut cpu = StubCpu::new();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut kernel = tests::fresh_kernel();
    let list = kernel.controls.create(
        HWND,
        ControlClass::ListView,
        7,
        String::new(),
        0x50000001,
        20,
        30,
        160,
        100,
    );
    kernel
        .controls
        .get_mut(list)
        .unwrap()
        .list_items
        .push(("Folder".into(), 0x12345678));
    kernel.controls.cursor_position = (31, 43);
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let Some(Some((parent, message, id, pointer))) = control_action_message(
        &mut ctx,
        ControlAction::Notify {
            parent: HWND,
            id: 7,
            hwnd: list,
            code: (-3i32) as u32,
            index: 0,
        },
    ) else {
        panic!("expected WM_NOTIFY");
    };
    assert_eq!((parent, message, id), (HWND, 0x4e, 7));
    assert_eq!(ctx.cpu.read_u32_le(pointer).unwrap(), list);
    assert_eq!(ctx.cpu.read_u32_le(pointer + 8).unwrap(), (-3i32) as u32);
    assert_eq!(ctx.cpu.read_u32_le(pointer + 12).unwrap(), 0);
    assert_eq!(ctx.cpu.read_u32_le(pointer + 32).unwrap(), 11);
    assert_eq!(ctx.cpu.read_u32_le(pointer + 36).unwrap(), 13);
    assert_eq!(ctx.cpu.read_u32_le(pointer + 40).unwrap(), 0x12345678);
}

#[test]
fn ce_image_shell_folders_override_mobile_defaults_and_create_through_the_vfs() {
    use crate::bluetooth::tests::{call, setup};
    use pocket_kernel::registry::RegistryValue;
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    let key = r"HKLM\System\Explorer\Shell Folders";
    let output = 0x2000;
    let query =
        |cpu: &mut StubCpu, kernel: &mut KernelState, dispatcher: &mut WinCeDispatcher, create| {
            call(
                cpu,
                kernel,
                dispatcher,
                "coredll.dll",
                "SHGetSpecialFolderPath",
                &[0, output, 2, create],
            )
        };
    let read_path = |cpu: &mut dyn Cpu| {
        let bytes = cpu.read_mem(output, 520).unwrap();
        String::from_utf16_lossy(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                .take_while(|&u| u != 0)
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        query(&mut cpu, &mut kernel, &mut dispatcher, 0),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(read_path(&mut cpu), r"\Windows\Start Menu\Programs");
    kernel.registry.set_value(
        key,
        "Programs",
        RegistryValue::Sz(r"\Windows\Programs".into()),
    );
    assert_eq!(
        query(&mut cpu, &mut kernel, &mut dispatcher, 0),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(read_path(&mut cpu), r"\Windows\Programs");
    let writable = std::env::temp_dir().join(format!(
        "pockethle-shell-folder-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    kernel.vfs.mount_save_dir(r"\", &writable);
    assert_eq!(
        query(&mut cpu, &mut kernel, &mut dispatcher, 1),
        DispatchOutcome::ReturnedR0(1)
    );
    // CE keeps the registry spelling while its VFS resolves case-insensitive
    // shell folder names, including on Linux and Android host filesystems.
    let programs = kernel.vfs.resolve(r"\wINDows\pROgrams").unwrap();
    assert!(programs.is_dir());
    assert!(programs
        .canonicalize()
        .unwrap()
        .starts_with(writable.canonicalize().unwrap()));
    std::fs::write(writable.join("Blocked"), b"keep").unwrap();
    kernel.registry.set_value(
        key,
        "Programs",
        RegistryValue::Sz(r"\Blocked\Programs".into()),
    );
    assert_eq!(
        query(&mut cpu, &mut kernel, &mut dispatcher, 1),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(read_path(&mut cpu), r"\Windows\Programs");
    assert_eq!(std::fs::read(writable.join("Blocked")).unwrap(), b"keep");
    std::fs::remove_dir_all(writable).unwrap();
}

fn framed_kernel() -> KernelState {
    let mut state = tests::fresh_kernel();
    state.ce.windows.enabled = true;
    state.framebuffer =
        pocket_kernel::framebuffer::deferred::DeferredFramebuffer::new(Framebuffer::new(640, 480))
            .unwrap();
    state.window_rects.insert(HWND, (40, 60, 240, 180));
    state.window_styles.insert(HWND, (0x10cf0000, 0));
    state.window_visibility.insert(HWND, true);
    state.window_procs.insert(HWND, 0x1234);
    state.window_classes.insert(HWND, "test".into());
    state.window_titles.insert(HWND, "CE window".into());
    state.ce.windows.z_order.push(HWND);
    state.active_window = HWND;
    state
}

fn message(ctx: &mut CallCtx<'_>, hwnd: u32, msg: u32, wp: u32, lp: u32) {
    for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip([hwnd, msg, wp, lp])
    {
        ctx.cpu.write_reg(reg, value).unwrap();
    }
}

fn paint(state: &mut KernelState, dc: u32, color: u16) {
    let handle = state.gdi.dc(dc).unwrap().selected_bitmap.unwrap();
    let bitmap = state.gdi.bitmap_mut(handle).unwrap();
    let (w, h) = (bitmap.width, bitmap.height);
    Surface::Bitmap(bitmap).fill_rect(0, 0, w as i32, h as i32, color);
}

fn pixel(state: &KernelState, x: usize, y: usize) -> u16 {
    let at = (y * state.framebuffer.width as usize + x) * 2;
    u16::from_le_bytes(
        state.framebuffer.resident_cpu().unwrap().pixels[at..at + 2]
            .try_into()
            .unwrap(),
    )
}

#[test]
fn classic_client_dc_and_child_clipping_preserve_the_caption() {
    let mut cpu = StubCpu::new();
    let mut state = framed_kernel();
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, HWND);
    assert_eq!(ctx.kernel.window_client_origin(HWND), Some((44, 84)));
    assert_eq!(ctx.kernel.window_client_size(HWND), Some((232, 152)));
    let bitmap = ctx
        .kernel
        .gdi
        .bitmap(ctx.kernel.gdi.dc(dc).unwrap().selected_bitmap.unwrap())
        .unwrap();
    assert_eq!((bitmap.width, bitmap.height), (232, 152));
    paint(ctx.kernel, dc, 0x07e0);
    let child = HWND + 1;
    ctx.kernel.window_rects.insert(child, (-10, -30, 300, 240));
    ctx.kernel.window_styles.insert(child, (0x50000000, 0));
    ctx.kernel.window_visibility.insert(child, true);
    ctx.kernel.window_parents.insert(child, HWND);
    let child_dc = window_dc(&mut ctx, child);
    paint(ctx.kernel, child_dc, 0xf800);
    ctx.kernel.composite_controls();
    assert_eq!(
        pixel(ctx.kernel, 100, 65),
        colorref_to_rgb565(windowing::system_color(3))
    );
    assert_eq!(pixel(ctx.kernel, 60, 90), 0xf800);
    assert_eq!(
        pixel(ctx.kernel, 39, 90),
        colorref_to_rgb565(windowing::system_color(1))
    );
    let counter = ctx.kernel.framebuffer.frame_counter;
    let original = ctx
        .kernel
        .framebuffer
        .resident_cpu()
        .unwrap()
        .pixels
        .clone();
    let epoch = ctx.kernel.framebuffer.authority_epoch();
    ctx.kernel.composite_controls();
    assert_eq!(ctx.kernel.framebuffer.frame_counter, counter);
    assert_eq!(
        ctx.kernel.framebuffer.resident_cpu().unwrap().pixels,
        original
    );
    assert_eq!(ctx.kernel.framebuffer.authority_epoch(), epoch);
    ctx.kernel.window_rects.get_mut(&HWND).unwrap().2 = 300;
    resize_window_dc(ctx.kernel, HWND);
    assert_eq!(window_dc(&mut ctx, HWND), dc);
    let bitmap = ctx
        .kernel
        .gdi
        .bitmap(ctx.kernel.gdi.dc(dc).unwrap().selected_bitmap.unwrap())
        .unwrap();
    assert_eq!((bitmap.width, bitmap.height), (292, 152));
    assert_eq!(
        u16::from_le_bytes(bitmap.pixels[..2].try_into().unwrap()),
        0x07e0
    );
}

#[test]
fn classic_close_delivers_system_command_and_wm_close_before_destruction() {
    for style in [0x10cf0000, windowing::classic_style(0x10400000)] {
        let mut cpu = StubCpu::new();
        let mut state = framed_kernel();
        state.window_styles.insert(HWND, (style, 0));
        let thunk = tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        let rect = ctx.kernel.window_rects[&HWND];
        let close = windowing::caption_buttons(
            rect,
            ctx.kernel.window_styles[&HWND].0,
            ctx.kernel.window_metrics(HWND),
        )
        .into_iter()
        .find(|b| b.hit == 20)
        .unwrap();
        let (x, y) = (
            (close.rect[0] + close.rect[2]) as u16 / 2,
            (close.rect[1] + close.rect[3]) as u16 / 2,
        );
        let down = classic_window::take_input(&mut ctx, InputEvent::PointerDown { x, y })
            .unwrap()
            .unwrap();
        assert_eq!(down.1, 0xa1);
        message(&mut ctx, down.0, down.1, down.2, down.3);
        classic_window::default_message(&mut ctx, HWND).unwrap();
        let up = classic_window::take_input(&mut ctx, InputEvent::PointerUp { x, y })
            .unwrap()
            .unwrap();
        message(&mut ctx, up.0, up.1, up.2, up.3);
        classic_window::default_message(&mut ctx, HWND).unwrap();
        assert!(ctx
            .kernel
            .posted_messages
            .iter()
            .any(|m| m.0 == HWND && m.1 == 0x112 && m.2 == 0xf060));
        message(&mut ctx, HWND, 0x112, 0xf060, up.3);
        classic_window::default_message(&mut ctx, HWND).unwrap();
        assert!(ctx.kernel.posted_messages.contains(&(HWND, 0x10, 0, 0)));
        assert!(!ctx.kernel.posted_messages.iter().any(|m| m.1 == 2));
        assert!(ctx.kernel.window_rects.contains_key(&HWND));
    }
}

#[test]
fn pointer_probes_check_the_whole_range_and_permissions_without_changing_error_state() {
    let mut cpu = tests::scanf_cpu();
    cpu.map_region(0x700000, 0x1000, Prot::READ).unwrap();
    let mut kernel = framed_kernel();
    kernel.thread_last_errors.insert(0, 1234);
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for (address, size, read_bad, write_bad) in [
        (0, 0, 0, 0),
        (0, 1, 1, 1),
        (0x700000, 0x1000, 0, 1),
        (0x700fff, 2, 1, 1),
        (0xfffffff0, 32, 1, 1),
        (0x1400, 4, 0, 0),
    ] {
        ctx.cpu.write_reg(ArmReg::R0, address).unwrap();
        ctx.cpu.write_reg(ArmReg::R1, size).unwrap();
        assert_eq!(
            is_bad_read_ptr(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(read_bad)
        );
        assert_eq!(
            is_bad_write_ptr(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(write_bad)
        );
    }
    assert_eq!(ctx.kernel.thread_last_errors.get(&0), Some(&1234));
}

#[test]
fn process_version_distinguishes_image_module_and_remote_process_and_rejects_stale_ids() {
    let mut cpu = tests::scanf_cpu();
    let mut kernel = framed_kernel();
    kernel.image_version = 0x0006_0001;
    kernel.ce.module_versions.insert(0x31000000, 0x0005_0002);
    let (_, child) = kernel.object_handles.new_child().unwrap();
    child.set_process_version(0x0004_0003);
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for (id, version) in [
        (0, 0x0006_0001),
        (1, 0x0006_0001),
        (PROCESS_INSTANCE_HANDLE, 0x0006_0001),
        (0x31000000, 0x0005_0002),
        (child.process_id(), 0x0004_0003),
    ] {
        ctx.cpu.write_reg(ArmReg::R0, id).unwrap();
        assert_eq!(
            get_process_version(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(version)
        );
    }
    child.mark_inactive();
    ctx.cpu.write_reg(ArmReg::R0, child.process_id()).unwrap();
    assert_eq!(
        get_process_version(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(ctx.kernel.thread_last_errors.get(&0), Some(&87));
}

#[test]
fn classic_drag_maximize_minimize_and_restore_keep_window_identity() {
    let mut cpu = StubCpu::new();
    let mut state = framed_kernel();
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, HWND);
    let down = classic_window::take_input(&mut ctx, InputEvent::PointerDown { x: 100, y: 70 })
        .unwrap()
        .unwrap();
    assert_eq!(down.2, 2);
    message(&mut ctx, down.0, down.1, down.2, down.3);
    classic_window::default_message(&mut ctx, HWND).unwrap();
    classic_window::take_input(&mut ctx, InputEvent::PointerMove { x: 130, y: 95 });
    classic_window::take_input(&mut ctx, InputEvent::PointerUp { x: 130, y: 95 });
    let moved = (70, 85, 240, 180);
    assert_eq!(ctx.kernel.window_rects[&HWND], moved);
    assert!(ctx.kernel.ce.windows.drag.is_none());
    assert!(classic_window::show_state(ctx.kernel, HWND, 0xf030));
    assert_eq!(ctx.kernel.window_rects[&HWND], (0, 0, 640, 480));
    assert!(ctx
        .kernel
        .posted_messages
        .iter()
        .any(|m| m.1 == WM_SIZE && m.2 == 2));
    assert!(classic_window::show_state(ctx.kernel, HWND, 0xf120));
    assert_eq!(ctx.kernel.window_rects[&HWND], moved);
    assert!(classic_window::show_state(ctx.kernel, HWND, 0xf020));
    assert_eq!(ctx.kernel.window_client_size(HWND), Some((0, 0)));
    assert!(ctx
        .kernel
        .posted_messages
        .iter()
        .any(|m| m.1 == WM_SIZE && m.2 == 1 && m.3 == 0));
    assert!(classic_window::show_state(ctx.kernel, HWND, 0xf120));
    assert_eq!(ctx.kernel.window_rects[&HWND], moved);
    assert_eq!(ctx.kernel.window_dcs[&HWND], dc);
}

#[test]
fn classic_presentation_still_schedules_guest_owner_draw() {
    let mut state = framed_kernel();
    let button = state.controls.create(
        HWND,
        ControlClass::Button,
        1001,
        "More".into(),
        0x5000000b,
        10,
        20,
        80,
        24,
    );
    state.composite_controls();
    assert!(state
        .posted_messages
        .iter()
        .any(|m| m.0 == button && m.1 == WM_PAINT));
}

#[test]
fn frame_control_uses_the_requested_dc_and_preserves_application_clipping() {
    let mut cpu = StubCpu::new();
    cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut state = framed_kernel();
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, HWND);
    paint(ctx.kernel, dc, 0x1357);
    ctx.kernel.gdi.dc_mut(dc).unwrap().clip_rects = Some(vec![[8, 8, 12, 12]]);
    ctx.cpu
        .write_mem(
            0x1000,
            &[4i32, 4, 20, 20]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let before = ctx.kernel.framebuffer.frame_counter;
    message(&mut ctx, dc, 0x1000, 3, 1);
    assert_eq!(
        classic_gdi::frame_control(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    let bitmap = ctx
        .kernel
        .gdi
        .bitmap(ctx.kernel.gdi.dc(dc).unwrap().selected_bitmap.unwrap())
        .unwrap();
    let at = |x, y| {
        u16::from_le_bytes(
            bitmap.pixels
                [(y * bitmap.width as usize + x) * 2..(y * bitmap.width as usize + x) * 2 + 2]
                .try_into()
                .unwrap(),
        )
    };
    assert_eq!(at(4, 4), 0x1357);
    assert_ne!(at(8, 8), 0x1357);
    assert_eq!(at(12, 8), 0x1357);
    assert_eq!(ctx.kernel.framebuffer.frame_counter, before);
}

#[test]
fn classic_control_repaints_keep_background_controls_below_captions_and_status_below_its_owner() {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    let desktop = HWND + 10;
    state.window_rects.insert(desktop, (0, 0, 640, 480));
    state.window_styles.insert(desktop, (0x90000000, 0));
    state.window_visibility.insert(desktop, true);
    let list = state.controls.create(
        desktop,
        ControlClass::ListView,
        0,
        String::new(),
        0x50000000,
        0,
        0,
        640,
        480,
    );
    state.controls.get_mut(list).unwrap().list_background = windowing::system_color(1);
    let taskbar = HWND + 11;
    state.window_rects.insert(taskbar, (0, 454, 640, 26));
    state.window_styles.insert(taskbar, (0x90000000, 0));
    state.window_visibility.insert(taskbar, true);
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, taskbar);
    paint(ctx.kernel, dc, 0x07e0);
    ctx.cpu
        .write_mem(
            0x1000,
            &"Files\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    message(&mut ctx, 0x50000000, 0x1000, HWND, 5);
    let DispatchOutcome::ReturnedR0(bar) = status_control::create(&mut ctx).unwrap() else {
        panic!()
    };
    assert_ne!(bar, FAKE_STATUSBAR_HWND);
    assert_eq!(ctx.kernel.controls.get(bar).unwrap().parent, HWND);
    assert_eq!(
        status_control::message(&mut ctx, bar, 0x40b, 0, 0x1000).unwrap(),
        Some(1)
    );
    repaint_controls(&mut ctx);
    assert_eq!(
        pixel(ctx.kernel, 100, 65),
        colorref_to_rgb565(windowing::system_color(3))
    );
    assert_eq!(pixel(ctx.kernel, 20, 466), 0x07e0);
    assert!(ctx.kernel.status_bar.is_none());
}

#[test]
fn combobox_window_rect_reports_the_closed_field_used_by_command_bands() {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    let combo = state.controls.create(
        HWND,
        ControlClass::ComboBox,
        24,
        String::new(),
        0x50000000,
        5,
        10,
        120,
        110,
    );
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    message(&mut ctx, combo, 0x1000, 0, 0);
    assert_eq!(
        get_window_rect(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    let top = ctx.cpu.read_u32_le(0x1004).unwrap();
    let bottom = ctx.cpu.read_u32_le(0x100c).unwrap();
    assert_eq!(
        bottom - top,
        ctx.kernel
            .controls
            .get(combo)
            .unwrap()
            .choice_closed_height() as u32
    );
    assert!(bottom - top < 110);
}

#[test]
fn taskbar_input_reaches_its_worker_even_without_classic_frames() {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    state.ce.windows.enabled = false;
    let taskbar = HWND + 1;
    state.window_rects.insert(taskbar, (0, 454, 640, 26));
    state.window_styles.insert(taskbar, (0x90000000, 0));
    state.window_visibility.insert(taskbar, true);
    state.window_procs.insert(taskbar, 0x2100);
    state.window_threads.insert(taskbar, 1);
    let mut worker = GuestThread::new(0x2100, 0, 0x9000, 0x1000, 0x2200, 0, 0xdead7c00, [0; 17]);
    worker.started = true;
    worker.parked_in_pump = true;
    worker.sleep_until_ms = u64::MAX;
    state.threads.push(worker);
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    ctx.kernel
        .pending_input
        .push_back(InputEvent::PointerDown { x: 25, y: 465 });
    ctx.kernel
        .pending_input
        .push_back(InputEvent::PointerUp { x: 25, y: 465 });
    assert_eq!(next_input_message(&mut ctx), None);
    assert_eq!(
        ctx.kernel.posted_messages.back(),
        Some(&(taskbar, WM_LBUTTONDOWN, 1, 25 | (11 << 16)))
    );
    assert_eq!(ctx.kernel.pending_input.len(), 1);
    assert_eq!(ctx.kernel.threads[0].sleep_until_ms, 0);
    assert!(worker_is_ready(ctx.kernel, 0, &ctx.kernel.threads[0], 0));
    ctx.kernel.current_thread = 1;
    message(&mut ctx, 0x2000, 0, 0, 0);
    assert_eq!(
        get_message_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), taskbar);
    assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap(), WM_LBUTTONDOWN);
    ctx.kernel.controls.pointer_capture.insert(1, taskbar);
    let captured = addressed_input(ctx.kernel, InputEvent::PointerUp { x: 400, y: 100 }).unwrap();
    assert_eq!(
        captured,
        (
            taskbar,
            WM_LBUTTONUP,
            0,
            400 | ((-354i32 as u32 & 65535) << 16)
        )
    );
}

#[test]
fn parked_taskbar_receives_raw_input_without_a_primary_thread_message_pump() {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    let taskbar = HWND + 1;
    state.window_rects.insert(taskbar, (0, 454, 640, 26));
    state.window_styles.insert(taskbar, (0x90000000, 0));
    state.window_visibility.insert(taskbar, true);
    state.window_procs.insert(taskbar, 0x2100);
    state.window_threads.insert(taskbar, 1);
    state.active_window = taskbar;
    let mut worker = GuestThread::new(0x2100, 0, 0x9000, 0x1000, 0x2200, 0, 0xdead7c00, [0; 17]);
    worker.started = true;
    worker.worker_saved = true;
    worker.parked_in_pump = true;
    worker.sleep_until_ms = u64::MAX;
    worker.worker_regs[0] = 0x2000;
    worker.worker_regs[13] = 0x8800;
    worker.worker_regs[14] = 0x2200;
    worker.worker_regs[15] = 0x70000300;
    state.threads.push(worker);
    let main_thunk = tests::dummy_thunk();
    let mut pump_thunk = tests::dummy_thunk();
    pump_thunk.thunk_va = 0x70000300;
    cpu.write_reg(ArmReg::R0, 60000).unwrap();
    cpu.write_reg(ArmReg::R1, 0xabcdef).unwrap();
    cpu.write_reg(ArmReg::Lr, 0x12340).unwrap();
    state
        .pending_input
        .push_back(InputEvent::PointerDown { x: 25, y: 465 });
    state
        .pending_input
        .push_back(InputEvent::PointerUp { x: 25, y: 465 });
    assert!(worker_is_ready(&state, 0, &state.threads[0], 0));
    assert_eq!(
        resume_worker_reenter(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &main_thunk,
        })
        .unwrap(),
        Some(DispatchOutcome::JumpTo(pump_thunk.thunk_va))
    );
    assert_eq!(state.current_thread, 1);
    assert_eq!(state.threads[0].saved_regs[0], 60000);
    assert_eq!(state.threads[0].saved_regs[1], 0xabcdef);
    assert_eq!(state.threads[0].saved_regs[14], 0x12340);
    assert_eq!(state.threads[0].saved_regs[15], main_thunk.thunk_va);
    for (message, buttons) in [(WM_LBUTTONDOWN, 1), (WM_LBUTTONUP, 0)] {
        cpu.write_reg(ArmReg::R0, 0x2000).unwrap();
        assert_eq!(
            get_message_w(&mut CallCtx {
                cpu: &mut cpu,
                kernel: &mut state,
                thunk: &pump_thunk,
            })
            .unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(), taskbar);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), message);
        assert_eq!(cpu.read_u32_le(0x2008).unwrap(), buttons);
        assert_eq!(cpu.read_u32_le(0x200c).unwrap(), 25 | (11 << 16));
        assert_eq!(cpu.read_u32_le(0x2018).unwrap(), 465);
    }
    assert!(state.pending_input.is_empty());
    // A real Sleep remains blocked even if unrelated input arrives.
    state.threads[0].sleep_until_ms = u64::MAX;
    state.threads[0].parked_in_pump = false;
    state
        .pending_input
        .push_back(InputEvent::PointerDown { x: 25, y: 465 });
    assert!(!worker_is_ready(&state, 0, &state.threads[0], 0));
}

fn park_combined_message_wait(
    mask: u32,
    all: bool,
    timeout: u32,
) -> (StubCpu, KernelState, pocket_kernel::Thunk, [u32; 2]) {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    state.window_threads.insert(HWND, 1);
    state.active_window = HWND;
    let mut main = [0; 17];
    main[0] = 0x2000;
    main[1] = 0x1234;
    main[13] = 0x8700;
    main[14] = 0x2700;
    main[15] = 0x2800;
    let mut worker = GuestThread::new(0x2100, 0, 0x9000, 0x1000, 0x2200, 0, 0xdead7c00, main);
    worker.started = true;
    state.threads.push(worker);
    state.current_thread = 1;
    let handles = [0xdead7001u32, 0xdead7002];
    for handle in handles {
        state.events.insert(
            handle,
            pocket_kernel::EventObject {
                manual_reset: false,
                signalled: false,
            },
        );
    }
    cpu.write_mem(
        0x1400,
        &handles
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for (reg, value) in [
        (ArmReg::R0, 2),
        (ArmReg::R1, 0x1400),
        (ArmReg::R2, timeout),
        (ArmReg::R3, mask),
        (ArmReg::Sp, 0x8800),
        (ArmReg::Lr, 0x2600),
    ] {
        cpu.write_reg(reg, value).unwrap();
    }
    cpu.write_mem(0x8800, &u32::from(all).to_le_bytes())
        .unwrap();
    let wait = tests::dummy_thunk();
    assert_eq!(
        msg_wait_for_multiple_objects(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &wait,
        })
        .unwrap(),
        DispatchOutcome::JumpTo(0x2800)
    );
    assert_eq!(state.current_thread, 0);
    (cpu, state, wait, handles)
}

#[test]
fn unsatisfied_combined_wait_stays_parked_until_an_object_is_signalled() {
    let (mut cpu, mut state, wait, handles) = park_combined_message_wait(4, false, u32::MAX);
    let mut main = tests::dummy_thunk();
    main.thunk_va += 32;
    assert_eq!(
        resume_worker_reenter(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &main
        })
        .unwrap(),
        None
    );
    state.threads[0].messages.push_back((WM_KEYDOWN, 0x41, 0)); // excluded mask
    state.posted_messages.push_back((0, WM_LBUTTONDOWN, 1, 0)); // another owner
    assert_eq!(
        resume_worker_reenter(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &main
        })
        .unwrap(),
        None
    );
    state.events.get_mut(&handles[0]).unwrap().signalled = true;
    assert_eq!(
        resume_worker_reenter(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &main
        })
        .unwrap(),
        Some(DispatchOutcome::JumpTo(wait.thunk_va))
    );
    assert!(state.events.get(&handles[0]).unwrap().signalled); // readiness does not acquire
    assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 2);
    assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 0x1400);
    assert_eq!(cpu.read_reg(ArmReg::R2).unwrap(), u32::MAX);
    assert_eq!(cpu.read_reg(ArmReg::R3).unwrap(), 4);
    assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x8800);
    assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x2600);
    assert_eq!(
        msg_wait_for_multiple_objects(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &wait
        })
        .unwrap(),
        DispatchOutcome::JumpTo(main.thunk_va)
    );
    assert!(!state.events.get(&handles[0]).unwrap().signalled);
    assert_eq!(state.threads[0].worker_regs[0], 0);
    assert_eq!(state.threads[0].worker_regs[15], 0x2600);
}

#[test]
fn combined_wait_wakes_on_owned_raw_input_without_consuming_the_click() {
    let (mut cpu, mut state, wait, handles) = park_combined_message_wait(4, false, u32::MAX);
    state
        .pending_input
        .push_back(InputEvent::PointerMove { x: 60, y: 100 });
    assert!(!worker_is_ready(
        &state,
        0,
        &state.threads[0],
        monotonic_ms()
    ));
    state
        .pending_input
        .push_back(InputEvent::PointerDown { x: 60, y: 100 });
    assert!(worker_is_ready(
        &state,
        0,
        &state.threads[0],
        monotonic_ms()
    ));
    let main = tests::dummy_thunk();
    assert!(resume_worker_reenter(&mut CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &main
    })
    .unwrap()
    .is_some());
    assert_eq!(
        msg_wait_for_multiple_objects(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &wait
        })
        .unwrap(),
        DispatchOutcome::JumpTo(main.thunk_va)
    );
    assert_eq!(state.threads[0].worker_regs[0], 2);
    assert_eq!(state.pending_input.len(), 2);
    assert!(handles
        .iter()
        .all(|h| !state.events.get(h).unwrap().signalled));
}

#[test]
fn combined_wait_all_requires_matching_traffic_and_every_object() {
    let (mut cpu, mut state, wait, handles) = park_combined_message_wait(8, true, u32::MAX);
    state.threads[0].messages.push_back((0x401, 0, 0));
    state.events.get_mut(&handles[0]).unwrap().signalled = true;
    assert!(!worker_is_ready(
        &state,
        0,
        &state.threads[0],
        monotonic_ms()
    ));
    state.events.get_mut(&handles[1]).unwrap().signalled = true;
    let message = state.threads[0].messages.pop_front().unwrap();
    assert!(!worker_is_ready(
        &state,
        0,
        &state.threads[0],
        monotonic_ms()
    ));
    state.threads[0].messages.push_back(message);
    let main = tests::dummy_thunk();
    assert!(resume_worker_reenter(&mut CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &main
    })
    .unwrap()
    .is_some());
    assert!(handles
        .iter()
        .all(|h| state.events.get(h).unwrap().signalled));
    msg_wait_for_multiple_objects(&mut CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &wait,
    })
    .unwrap();
    assert_eq!(state.threads[0].worker_regs[0], 2);
    assert!(handles
        .iter()
        .all(|h| !state.events.get(h).unwrap().signalled));
    assert_eq!(state.threads[0].messages.front(), Some(&message));
}

#[test]
fn combined_wait_deadline_and_owned_timer_wake_without_polling_idle_objects() {
    let (mut cpu, mut state, wait, _) = park_combined_message_wait(0, false, 60000);
    let key = (1, wait.thunk_va, 0x8800);
    let deadline = state.wait_deadlines[&key];
    state.threads[0].messages.push_back((0x401, 0, 0));
    assert!(!worker_is_ready(&state, 0, &state.threads[0], deadline - 1));
    assert!(worker_is_ready(&state, 0, &state.threads[0], deadline));
    state.wait_deadlines.insert(key, 0);
    let main = tests::dummy_thunk();
    assert!(resume_worker_reenter(&mut CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &main
    })
    .unwrap()
    .is_some());
    msg_wait_for_multiple_objects(&mut CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &wait,
    })
    .unwrap();
    assert_eq!(state.threads[0].worker_regs[0], 0x102);
    assert!(!state.wait_deadlines.contains_key(&key));

    let (_, mut state, _, _) = park_combined_message_wait(0x10, false, u32::MAX);
    state.timers.insert((0, HWND, 1), (50, 0, 0));
    assert!(!worker_is_ready(&state, 0, &state.threads[0], 1));
    state.timers.insert((1, HWND, 2), (50, 10, 0));
    assert!(!worker_is_ready(&state, 0, &state.threads[0], 9));
    assert!(worker_is_ready(&state, 0, &state.threads[0], 10));
    assert_eq!(state.timers[&(1, HWND, 2)].1, 10); // preview does not reschedule
}

#[test]
fn messagebox_caption_drag_keeps_the_call_open_and_clears_both_covered_regions() {
    use pocket_kernel::msgbox::{id, mb};
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, HWND);
    paint(ctx.kernel, dc, 0x07e0);
    ctx.kernel.composite_controls();
    let backdrop = ctx
        .kernel
        .framebuffer
        .resident_cpu()
        .unwrap()
        .pixels
        .clone();
    for (addr, text) in [(0x1000, "Move this box"), (0x1100, "MessageBox")] {
        ctx.cpu
            .write_mem(
                addr,
                &text
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    message(&mut ctx, HWND, 0x1000, 0x1100, mb::YESNO);
    assert_eq!(
        message_box_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(thunk.thunk_va)
    );
    let modal = ctx.kernel.modal.as_ref().unwrap();
    let (x, y) = (modal.x, modal.y);
    ctx.kernel.composite_controls();
    assert_eq!(
        pixel(ctx.kernel, (x + 2) as usize, (y + 2) as usize),
        0x0010
    );
    for event in [
        InputEvent::PointerDown {
            x: (x + 10) as u16,
            y: (y + 5) as u16,
        },
        InputEvent::PointerMove {
            x: (x + 150) as u16,
            y: (y + 85) as u16,
        },
        InputEvent::PointerUp {
            x: (x + 150) as u16,
            y: (y + 85) as u16,
        },
    ] {
        ctx.kernel.pending_input.push_back(event);
    }
    assert_eq!(
        message_box_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(thunk.thunk_va)
    );
    let modal = ctx.kernel.modal.as_ref().unwrap();
    assert_eq!((modal.x, modal.y, modal.result), (x + 140, y + 80, None));
    let no = modal.buttons[1].clone();
    ctx.kernel.composite_controls();
    let original_at = ((y + 2) as usize * 640 + (x + 2) as usize) * 2;
    assert_eq!(
        pixel(ctx.kernel, (x + 2) as usize, (y + 2) as usize),
        u16::from_le_bytes(backdrop[original_at..original_at + 2].try_into().unwrap())
    );
    for event in [
        InputEvent::PointerDown {
            x: (no.x + no.w / 2) as u16,
            y: (no.y + no.h / 2) as u16,
        },
        InputEvent::PointerUp {
            x: (no.x + no.w / 2) as u16,
            y: (no.y + no.h / 2) as u16,
        },
    ] {
        ctx.kernel.pending_input.push_back(event);
    }
    assert_eq!(
        message_box_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(id::NO)
    );
    assert!(ctx.kernel.modal.is_none());
    ctx.kernel.composite_controls();
    assert_eq!(
        ctx.kernel.framebuffer.resident_cpu().unwrap().pixels,
        backdrop
    );
    assert!(ctx.kernel.pending_input.is_empty());
    assert!(!ctx.kernel.pressed_keys[1]);
}

#[test]
fn filtered_worker_mouse_preview_preserves_events_and_client_coordinates() {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    state.window_threads.insert(HWND, 1);
    state.threads.push(GuestThread::new(
        0x2100, 0, 0x9000, 0x1000, 0x2200, 0, 0xdead7c00, [0; 17],
    ));
    state.current_thread = 1;
    state
        .pending_input
        .push_back(InputEvent::KeyDown { vk: 0x41 });
    state
        .pending_input
        .push_back(InputEvent::PointerDown { x: 60, y: 100 });
    state
        .pending_input
        .push_back(InputEvent::PointerUp { x: 60, y: 100 });
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    for _ in 0..2 {
        assert_eq!(
            message_queue::peek(&mut ctx, 0x2000, HWND, WM_LBUTTONDOWN, WM_LBUTTONDOWN, 2).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), HWND);
        assert_eq!(ctx.cpu.read_u32_le(0x200c).unwrap(), 16 | (16 << 16));
        assert_eq!(ctx.kernel.pending_input.len(), 3);
        assert!(!ctx.kernel.pressed_keys[1]);
    }
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, HWND, WM_LBUTTONDOWN, WM_LBUTTONDOWN, 3).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert!(ctx.kernel.pressed_keys[1]);
    assert_eq!(ctx.kernel.pending_input.len(), 2);
    assert_eq!(
        ctx.kernel.pending_input.front(),
        Some(&InputEvent::KeyDown { vk: 0x41 })
    );
    ctx.kernel.current_thread = 0;
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, HWND, WM_LBUTTONUP, WM_LBUTTONUP, 3).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(ctx.kernel.pending_input.len(), 2);
    ctx.kernel.current_thread = 1;
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, HWND, WM_LBUTTONUP, WM_LBUTTONUP, 3).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert!(!ctx.kernel.pressed_keys[1]);
}

#[test]
fn classic_filtered_paint_queue_drains_without_manufacturing_invalidations() {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    state.synthetic_paint_next_ms = 0;
    let revision = state.framebuffer.frame_counter;
    let epoch = state.framebuffer.authority_epoch();
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    assert_eq!(next_message_if_due(&mut ctx), None);
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, 0, WM_PAINT, WM_PAINT, 3).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    queue_window_paint(ctx.kernel, HWND);
    queue_window_paint(ctx.kernel, HWND);
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, 0, WM_PAINT, WM_PAINT, 3).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), HWND);
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, 0, WM_PAINT, WM_PAINT, 3).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(ctx.kernel.framebuffer.frame_counter, revision);
    assert_eq!(ctx.kernel.framebuffer.authority_epoch(), epoch);
}

#[test]
fn legacy_filtered_paint_has_an_idle_opportunity_without_losing_real_paints() {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    state.ce.windows.enabled = false;
    state.synthetic_paint_next_ms = 0;
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, HWND, WM_PAINT, WM_PAINT, 3).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), HWND);
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, HWND, 0x10, 0x10, 2).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    ctx.kernel.synthetic_paint_next_ms = 0;
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, HWND, WM_PAINT, WM_PAINT, 3).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    queue_window_paint(ctx.kernel, HWND);
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, HWND, WM_PAINT, WM_PAINT, 3).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), HWND);
}

#[test]
fn window_icons_keep_separate_small_and_large_handles() {
    let mut cpu = tests::scanf_cpu();
    let mut state = framed_kernel();
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    message(&mut ctx, HWND, 0x80, 1, 0xabc);
    assert_eq!(
        def_window_proc_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    message(&mut ctx, HWND, 0x7f, 1, 0);
    assert_eq!(
        def_window_proc_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0xabc)
    );
    message(&mut ctx, HWND, 0x7f, 0, 0);
    assert_eq!(
        def_window_proc_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    message(&mut ctx, HWND, 0x80, 1, 0);
    assert_eq!(
        def_window_proc_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0xabc)
    );
}

#[test]
fn overlay_mutation_repaints_visible_icons_without_repainting_unchanged_lists() {
    let mut state = framed_kernel();
    let control = state.controls.create(
        HWND,
        ControlClass::ListView,
        1,
        String::new(),
        0x50000000,
        10,
        10,
        200,
        100,
    );
    let handle = state.gdi.create_image_list(16, 16);
    let mut base = pocket_kernel::imaging::Image::new(16, 16).unwrap();
    base.pixels.fill(0x07e0);
    let mut overlay = base.clone();
    overlay.pixels.fill(0xf800);
    let list = state.gdi.image_lists.get_mut(&handle).unwrap();
    list.replace(-1, &base).unwrap();
    list.replace(-1, &overlay).unwrap();
    let c = state.controls.get_mut(control).unwrap();
    c.list_image_lists[0] = handle;
    c.list_image_sizes[0] = (16, 16);
    c.list_items.push(("Shortcut".into(), 0));
    c.list_images.push(0);
    c.list_states.push(0x100);
    let icon_x = 44 + 10 + (c.list_spacing.0 - 16) / 2 + 8;
    let icon_y = 84 + 10 + 2 + 8;
    state.composite_controls();
    assert_eq!(pixel(&state, icon_x as usize, icon_y), 0x07e0);
    assert!(state
        .gdi
        .image_lists
        .get_mut(&handle)
        .unwrap()
        .set_overlay(1, 1));
    state.composite_controls();
    assert_eq!(pixel(&state, icon_x as usize, icon_y), 0xf800);
    let epoch = state.framebuffer.authority_epoch();
    assert!(state
        .gdi
        .image_lists
        .get_mut(&handle)
        .unwrap()
        .set_overlay(1, 1));
    state.composite_controls();
    assert_eq!(state.framebuffer.authority_epoch(), epoch);
}

#[test]
fn interleaved_list_paints_restore_each_threads_call_and_scratch() {
    let mut cpu = StubCpu::new();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut state = tests::fresh_kernel();
    let mut windows = Vec::new();
    for thread in 0..2 {
        let parent = HWND + thread as u32;
        state
            .window_procs
            .insert(parent, 0x6000 + thread as u32 * 0x1000);
        let hwnd = state.controls.create(
            parent,
            ControlClass::ListView,
            10 + thread as u32,
            String::new(),
            0x50000000,
            0,
            0,
            80,
            60,
        );
        state
            .controls
            .get_mut(hwnd)
            .unwrap()
            .list_items
            .push(("Item".into(), 0));
        windows.push(hwnd);
    }
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    let mut scratch = Vec::new();
    let mut dcs = Vec::new();
    for (thread, &hwnd) in windows.iter().enumerate() {
        ctx.kernel.current_thread = thread;
        message(&mut ctx, hwnd, WM_PAINT, 0, 17 + thread as u32);
        ctx.cpu
            .write_reg(ArmReg::Sp, 0x1800 + thread as u32 * 0x1000)
            .unwrap();
        ctx.cpu
            .write_reg(ArmReg::Lr, 0x8000 + thread as u32 * 0x1000)
            .unwrap();
        assert_eq!(
            list_paint::paint(&mut ctx, hwnd).unwrap(),
            DispatchOutcome::JumpTo(0x6000 + thread as u32 * 0x1000)
        );
        let pointer = ctx.cpu.read_reg(ArmReg::R3).unwrap();
        scratch.push(pointer);
        let dc = ctx.cpu.read_u32_le(pointer + 16).unwrap();
        dcs.push(dc);
        paint(ctx.kernel, dc, 0x1234 + thread as u16);
    }
    // Explorer's folder thread returns while the desktop's later paint is
    // still parked. A global last() would miss the folder continuation.
    ctx.kernel.current_thread = 0;
    ctx.cpu.write_reg(ArmReg::Sp, 0x17f0).unwrap();
    assert_eq!(list_paint::resume(&mut ctx).unwrap(), None);
    assert_eq!(ctx.kernel.gdi.list_paint_frames.len(), 2);
    for thread in 0..2 {
        ctx.kernel.current_thread = thread;
        ctx.cpu
            .write_reg(ArmReg::Sp, 0x1800 + thread as u32 * 0x1000)
            .unwrap();
        ctx.cpu.write_reg(ArmReg::R0, 4).unwrap(); // CDRF_SKIPDEFAULT
        assert_eq!(
            list_paint::resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::ReturnedR0(0))
        );
        for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
            .into_iter()
            .zip([windows[thread], WM_PAINT, 0, 17 + thread as u32])
        {
            assert_eq!(ctx.cpu.read_reg(reg).unwrap(), value);
        }
        assert_eq!(
            ctx.cpu.read_reg(ArmReg::Sp).unwrap(),
            0x1800 + thread as u32 * 0x1000
        );
        assert_eq!(
            ctx.cpu.read_reg(ArmReg::Lr).unwrap(),
            0x8000 + thread as u32 * 0x1000
        );
        assert_eq!(ctx.kernel.heap.msize(scratch[thread]), None);
        let bitmap = ctx
            .kernel
            .gdi
            .bitmap(
                ctx.kernel
                    .gdi
                    .dc(dcs[thread])
                    .unwrap()
                    .selected_bitmap
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(
            u16::from_le_bytes(bitmap.pixels[..2].try_into().unwrap()),
            0x1234 + thread as u16
        );
        if thread == 0 {
            assert_eq!(ctx.kernel.gdi.list_paint_frames.len(), 1);
            assert_eq!(ctx.kernel.gdi.list_paint_frames[0].thread, 1);
            assert_eq!(ctx.kernel.heap.msize(scratch[1]), Some(2108));
        }
    }
    assert!(ctx.kernel.gdi.list_paint_frames.is_empty());
}

#[test]
fn interleaved_control_color_callbacks_keep_the_other_threads_dc_alive() {
    let mut cpu = StubCpu::new();
    let mut state = tests::fresh_kernel();
    let mut windows = Vec::new();
    for thread in 0..2 {
        let parent = HWND + thread as u32;
        state
            .window_procs
            .insert(parent, 0x6000 + thread as u32 * 0x1000);
        windows.push(state.controls.create(
            parent,
            ControlClass::Static,
            thread as u32,
            "Label".into(),
            0x50000000,
            0,
            0,
            80,
            20,
        ));
    }
    let brushes = [
        state.gdi.create_solid_brush(0x123456),
        state.gdi.create_solid_brush(0x654321),
    ];
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    let mut dcs = Vec::new();
    for (thread, &hwnd) in windows.iter().enumerate() {
        ctx.kernel.current_thread = thread;
        message(&mut ctx, hwnd, WM_PAINT, 0, 0);
        ctx.cpu
            .write_reg(ArmReg::Sp, 0x1800 + thread as u32 * 0x1000)
            .unwrap();
        ctx.cpu
            .write_reg(ArmReg::Lr, 0x8000 + thread as u32 * 0x1000)
            .unwrap();
        assert_eq!(
            control_color::paint(&mut ctx, hwnd, false).unwrap(),
            DispatchOutcome::JumpTo(0x6000 + thread as u32 * 0x1000)
        );
        let dc = ctx.cpu.read_reg(ArmReg::R2).unwrap();
        ctx.kernel.gdi.dc_mut(dc).unwrap().text_color = 0x99 + thread as u32;
        dcs.push(dc);
    }
    for thread in 0..2 {
        ctx.kernel.current_thread = thread;
        ctx.cpu
            .write_reg(ArmReg::Sp, 0x1800 + thread as u32 * 0x1000)
            .unwrap();
        ctx.cpu.write_reg(ArmReg::R0, brushes[thread]).unwrap();
        assert_eq!(
            control_color::resume(&mut ctx).unwrap(),
            Some(DispatchOutcome::ReturnedR0(0))
        );
        let control = ctx.kernel.controls.get(windows[thread]).unwrap();
        assert_eq!(control.text_color, 0x99 + thread as u32);
        assert_eq!(control.background_color, Some([0x123456, 0x654321][thread]));
        assert!(ctx.kernel.gdi.dc(dcs[thread]).is_none());
        assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), windows[thread]);
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), WM_PAINT);
        assert_eq!(
            ctx.cpu.read_reg(ArmReg::Lr).unwrap(),
            0x8000 + thread as u32 * 0x1000
        );
        if thread == 0 {
            assert_eq!(ctx.kernel.gdi.control_color_frames.len(), 1);
            assert!(ctx.kernel.gdi.dc(dcs[1]).is_some());
        }
    }
    assert!(ctx.kernel.gdi.control_color_frames.is_empty());
}

#[test]
fn finished_folder_thread_releases_its_surfaces_and_reveals_the_desktop() {
    let mut cpu = StubCpu::new();
    let mut state = framed_kernel();
    let desktop = HWND + 10;
    state.window_rects.insert(desktop, (0, 0, 640, 480));
    state.window_styles.insert(desktop, (0x90000000, 0));
    state.window_visibility.insert(desktop, true);
    state.window_threads.insert(desktop, 0);
    state.window_classes.insert(desktop, "Desktop".into());
    state.ce.windows.z_order.insert(0, desktop);
    state.window_threads.insert(HWND, 1);
    let mut worker = GuestThread::new(0x11000, 0, 0x9000, 0x1000, 0x12000, 0, 0xdead7c00, [0; 17]);
    worker.started = true;
    state.threads.push(worker);
    let child = state.controls.create(
        HWND,
        ControlClass::Static,
        1,
        "Child".into(),
        0x50000000,
        0,
        0,
        80,
        30,
    );
    state.window_threads.insert(child, 1);
    state.posted_messages.push_back((child, WM_PAINT, 0, 0));
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    let background = colorref_to_rgb565(windowing::system_color(1));
    let desktop_dc = window_dc(&mut ctx, desktop);
    paint(ctx.kernel, desktop_dc, background);
    let folder_dc = window_dc(&mut ctx, HWND);
    let folder_bitmap = ctx
        .kernel
        .gdi
        .dc(folder_dc)
        .unwrap()
        .selected_bitmap
        .unwrap();
    paint(ctx.kernel, folder_dc, 0xf800);
    let child_dc = window_dc(&mut ctx, child);
    let child_bitmap = ctx
        .kernel
        .gdi
        .dc(child_dc)
        .unwrap()
        .selected_bitmap
        .unwrap();
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 160, 160), 0xf800);
    ctx.kernel.threads[0].finished = true;
    ctx.kernel.threads[0].exit_code = Some(7);
    ctx.kernel.reclaim_finished_stacks(ctx.cpu).unwrap();
    assert_eq!(pixel(ctx.kernel, 160, 160), background);
    assert!(!ctx.kernel.window_rects.contains_key(&HWND));
    assert!(!ctx.kernel.window_threads.contains_key(&child));
    assert!(!ctx.kernel.window_dcs.contains_key(&child));
    assert!(ctx.kernel.controls.get(child).is_none());
    assert!(ctx.kernel.gdi.dc(folder_dc).is_none());
    assert!(ctx.kernel.gdi.bitmap(folder_bitmap).is_none());
    assert!(ctx.kernel.gdi.bitmap(child_bitmap).is_none());
    assert!(ctx.kernel.posted_messages.iter().all(|m| m.0 != child));
    assert_eq!(ctx.kernel.window_dcs.get(&desktop), Some(&desktop_dc));
    let epoch = ctx.kernel.framebuffer.authority_epoch();
    ctx.kernel.reclaim_finished_stacks(ctx.cpu).unwrap();
    ctx.kernel.composite_controls();
    assert_eq!(ctx.kernel.framebuffer.authority_epoch(), epoch);
}

#[test]
fn interleaved_window_destruction_keeps_each_threads_notifications_and_return() {
    let mut cpu = StubCpu::new();
    let mut state = tests::fresh_kernel();
    for thread in 0..2 {
        let hwnd = HWND + thread as u32;
        state.window_classes.insert(hwnd, "Window".into());
        state
            .window_procs
            .insert(hwnd, 0x6000 + thread as u32 * 0x1000);
    }
    let thunk = tests::dummy_thunk();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut state,
        thunk: &thunk,
    };
    for thread in 0..2 {
        ctx.kernel.current_thread = thread;
        message(&mut ctx, HWND + thread as u32, 101, 102, 103);
        ctx.cpu
            .write_reg(ArmReg::Sp, 0x1800 + thread as u32 * 0x1000)
            .unwrap();
        ctx.cpu
            .write_reg(ArmReg::Lr, 0x8000 + thread as u32 * 0x1000)
            .unwrap();
        assert_eq!(
            destroy_window(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(0x6000 + thread as u32 * 0x1000)
        );
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), WM_DESTROY);
    }
    for thread in 0..2 {
        ctx.kernel.current_thread = thread;
        ctx.cpu
            .write_reg(ArmReg::Sp, 0x1800 + thread as u32 * 0x1000)
            .unwrap();
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            destroy_window(&mut ctx).unwrap(),
            DispatchOutcome::JumpTo(0x6000 + thread as u32 * 0x1000)
        );
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), 0x82);
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            destroy_window(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            ctx.cpu.read_reg(ArmReg::Lr).unwrap(),
            0x8000 + thread as u32 * 0x1000
        );
        assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), HWND + thread as u32);
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), 101);
        if thread == 0 {
            assert!(ctx.kernel.window_classes.contains_key(&(HWND + 1)));
            assert_eq!(ctx.kernel.window_destroy_frames.len(), 1);
        }
    }
    assert!(ctx.kernel.window_destroy_frames.is_empty());
}
