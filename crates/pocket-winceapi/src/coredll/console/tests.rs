use super::*;
use crate::bluetooth::tests::{call, setup};
use pocket_cpu::Cpu;
use pocket_kernel::Dispatcher;

fn wide(cpu: &mut dyn Cpu, address: u32, text: &str) {
    cpu.write_mem(
        address,
        &text
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
fn value(outcome: DispatchOutcome) -> u32 {
    match outcome {
        DispatchOutcome::ReturnedR0(value) => value,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn console_mouse_messages_cannot_block_committed_keyboard_input() {
    let (mut cpu, mut k, mut d) = setup();
    k.ensure_console_window();
    let device = k.vfs.console_device().unwrap();
    {
        let mut c = device.lock().unwrap();
        c.resize(16, 3);
        for row in 0..10 {
            c.write_text(&format!("row{row}\r\n"));
        }
    }
    k.pending_input.extend([
        InputEvent::TouchSteeringPosition { x: 1000, y: -1000 },
        InputEvent::StylusAim { x: 10, y: -10 },
        InputEvent::MouseMessage {
            message: 0x20a,
            x: 20,
            y: 60,
            data: 120,
        },
        InputEvent::MouseMessage {
            message: 0x204,
            x: 20,
            y: 60,
            data: 0,
        },
        InputEvent::MouseMessage {
            message: 0x205,
            x: 20,
            y: 60,
            data: 0,
        },
        InputEvent::Text { character: 'Ж' },
        InputEvent::KeyDown { vk: 13 },
        InputEvent::KeyUp { vk: 13 },
    ]);
    call(&mut cpu, &mut k, &mut d, "coredll.dll", "GetTickCount", &[]);
    assert!(k.pending_input.is_empty());
    assert_eq!(
        device.lock().unwrap().read_utf16(8, true),
        Some(vec!['Ж' as u16, 10])
    );
    assert!(device.lock().unwrap().snapshot().caret.is_some());
}

#[test]
fn console_child_can_claim_a_view_after_forced_owner_teardown() {
    let (_, mut parent, _) = setup();
    parent.ensure_console_window();
    let device = parent.vfs.console_device().unwrap();
    let context = parent.console_context();
    let (_, handles) = parent.object_handles.new_child().unwrap();
    let (_, mut child, _) = setup();
    child.object_handles = handles;
    child.attach_console_context(context);
    child.ensure_console_window();
    assert!(child.console.view.is_none());
    drop(parent);
    assert!(device.lock().unwrap().owner_process.is_none());
    child.ensure_console_window();
    assert!(child.console.view.is_some());
    assert_eq!(
        device.lock().unwrap().owner_process,
        Some(child.object_handles.process_id())
    );
    drop(child);
    assert!(device.lock().unwrap().owner_process.is_none());
}

#[test]
fn stale_console_view_cannot_release_a_newer_process_owner() {
    let (_, mut parent, _) = setup();
    parent.ensure_console_window();
    let device = parent.vfs.console_device().unwrap();
    let (_, handles) = parent.object_handles.new_child().unwrap();
    let newer_pid = handles.process_id();
    device.lock().unwrap().owner_process = Some(newer_pid);
    drop(parent);
    assert_eq!(device.lock().unwrap().owner_process, Some(newer_pid));
}

#[test]
fn cmd_directory_totals_use_word_aligned_64_bit_variadic_integers() {
    let (mut cpu, mut k, mut d) = setup();
    wide(
        &mut cpu,
        0x1000,
        "Found %u file(s). Total size %I64d bytes. %u\n",
    );
    let expected = "Found 1 file(s). Total size 8589934634 bytes. 7\n";
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fwprintf",
            &[STD_FILE_BASE + 1, 0x1000, 1, 42, 2, 7]
        )),
        expected.len() as u32
    );
    wide(&mut cpu, 0x1100, "%I64d|%I64u|%llX|%u");
    let values = [u32::MAX, u32::MAX, 0, 1, 0x55667788, 0x11223344, 9];
    cpu.write_mem(
        0x1800,
        &values
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let expected_va = "-1|4294967296|1122334455667788|9";
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "vfwprintf",
            &[STD_FILE_BASE + 1, 0x1100, 0x1800]
        )),
        expected_va.len() as u32
    );
    let snapshot = k.vfs.console_device().unwrap().lock().unwrap().snapshot();
    assert_eq!(
        snapshot.lines[0].iter().collect::<String>(),
        expected.trim_end()
    );
    assert_eq!(snapshot.lines[1].iter().collect::<String>(), expected_va);
}

#[test]
fn narrow_printf_uses_the_live_redirected_stdout_and_full_64_bit_values() {
    let dir = tempfile::tempdir().unwrap();
    let (mut cpu, mut k, mut d) = setup();
    k.vfs.mount("\\", dir.path());
    wide(&mut cpu, 0x1000, "\\stdout.txt");
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "SetStdioPathW",
            &[1, 0x1000]
        )),
        1
    );
    cpu.write_mem(0x1100, b"%I64u:%u:\xe9\0").unwrap();
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "printf",
            &[0x1100, 0, 1, 7]
        )),
        14
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fflush",
        &[STD_FILE_BASE + 1],
    );
    assert_eq!(
        std::fs::read(dir.path().join("stdout.txt")).unwrap(),
        b"4294967296:7:\xe9"
    );
}

#[test]
fn find_file_records_retain_times_and_do_not_consume_entries_on_invalid_output() {
    let dir = tempfile::tempdir().unwrap();
    let (mut cpu, mut k, mut d) = setup();
    k.vfs.mount("\\", dir.path());
    let time = 116_444_736_010_000_000u64;
    for name in ["a.txt", "b.txt"] {
        let h = k
            .vfs
            .open_file(&format!("\\{name}"), 3, 3, 2, false, 0)
            .unwrap()
            .handle;
        k.vfs
            .set_file_times(h, [Some(time), Some(time), Some(time)])
            .unwrap();
        k.vfs.close(h);
    }
    wide(&mut cpu, 0x1000, "\\*.txt");
    let handle = value(call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "FindFirstFileW",
        &[0x1000, 0x2000],
    ));
    assert_ne!(handle, u32::MAX);
    for index in 0..3 {
        assert_eq!(
            u64::from_le_bytes(
                cpu.read_mem(0x2004 + index * 8, 8)
                    .unwrap()
                    .try_into()
                    .unwrap()
            ),
            time
        );
    }
    k.vfs.set_default_dir("\\elsewhere");
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "FindNextFileW",
            &[handle, 0x9000]
        )),
        0
    );
    assert_eq!(k.thread_last_errors[&0], 87);
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "FindNextFileW",
            &[handle, 0x2000]
        )),
        1
    );
    assert_eq!(
        u64::from_le_bytes(cpu.read_mem(0x2014, 8).unwrap().try_into().unwrap()),
        time
    );
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "FindNextFileW",
            &[handle, 0x2000]
        )),
        0
    );
    assert_eq!(k.thread_last_errors[&0], 18);
    wide(&mut cpu, 0x1000, "\\a.txt");
    assert_ne!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "FindFirstFileW",
            &[0x1000, 0x2000]
        )),
        u32::MAX
    );
    assert_eq!(
        u64::from_le_bytes(cpu.read_mem(0x2014, 8).unwrap().try_into().unwrap()),
        time
    );
}

#[test]
fn pocket_cmd_console_ioctl_uses_descriptors_and_validates_buffers_before_mutation() {
    let (mut cpu, mut k, mut d) = setup();
    k.ce.version = Some((6, 0, 0));
    let args = [1, 0x01020024, 0, 0, 0x1200, 4, 0x1210, 0];
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DeviceIoControl",
            &args
        )),
        1
    );
    assert!(cpu.read_u32_le(0x1200).unwrap() > 20);
    assert_eq!(cpu.read_u32_le(0x1210).unwrap(), 4);
    let hwnd = k.console.view.as_ref().unwrap().hwnd;
    assert!(k.accepts_text_input());
    cpu.write_mem(0x1300, b"Pocket CMD v 6.00").unwrap();
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DeviceIoControl",
            &[1, 0x0102000c, 0x1300, 17, 0, 0, 0x1210, 0]
        )),
        1
    );
    k.composite_controls();
    assert_eq!(k.window_titles[&hwnd], "Pocket CMD v 6.00");
    cpu.write_mem(0x1200, &[0xa5; 4]).unwrap();
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DeviceIoControl",
            &[1, 0x01020024, 0, 0, 0x1200, 2, 0x1210, 0]
        )),
        0
    );
    assert_eq!(cpu.read_mem(0x1200, 4).unwrap(), [0xa5; 4]);
    assert_eq!(k.thread_last_errors[&0], 122);
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DeviceIoControl",
            &[1, 0x0102000c, 0x1300, 3, 0, 0, 0x9000, 0]
        )),
        0
    );
    assert_eq!(
        k.vfs.console_device().unwrap().lock().unwrap().title,
        "Pocket CMD v 6.00"
    );
    assert_eq!(k.thread_last_errors[&0], 87);
}

#[test]
fn console_stdin_yields_then_returns_edited_unicode_without_reporting_eof() {
    let (mut cpu, mut k, mut d) = setup();
    k.ce.version = Some((6, 0, 0));
    cpu.write_reg(ArmReg::Lr, 0x2345).unwrap();
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fgetws",
            &[0x1200, 64, STD_FILE_BASE]
        ),
        DispatchOutcome::JumpTo(pocket_kernel::THREAD_SCHEDULER_IDLE_VA)
    );
    assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 0x1200);
    assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 64);
    assert_eq!(cpu.read_reg(ArmReg::R2).unwrap(), STD_FILE_BASE);
    assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x2345);
    assert!(!k.vfs.console_device().unwrap().lock().unwrap().at_eof());
    assert_eq!(
        d.schedule_idle(&mut cpu, &mut k).unwrap(),
        DispatchOutcome::JumpTo(0x70000000)
    );
    for ch in "echo ЖXY".chars() {
        k.pending_input
            .push_back(InputEvent::Text { character: ch });
    }
    k.pending_input.extend([
        InputEvent::Text { character: '\x08' },
        InputEvent::KeyDown { vk: 8 },
        InputEvent::KeyUp { vk: 8 },
        InputEvent::Text { character: 'Я' },
        InputEvent::KeyDown { vk: 13 },
        InputEvent::KeyUp { vk: 13 },
    ]);
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fgetws",
            &[0x1200, 64, STD_FILE_BASE]
        )),
        0x1200
    );
    let bytes = cpu.read_mem(0x1200, 18).unwrap();
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|u| *u != 0)
        .collect();
    assert_eq!(String::from_utf16(&units).unwrap(), "echo ЖЯ\n");
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "feof",
            &[STD_FILE_BASE]
        )),
        0
    );
    k.pending_input.extend([
        InputEvent::KeyDown { vk: 0x11 },
        InputEvent::KeyDown { vk: 0x5a },
        InputEvent::KeyUp { vk: 0x5a },
        InputEvent::KeyUp { vk: 0x11 },
    ]);
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fgetws",
            &[0x1200, 64, STD_FILE_BASE]
        )),
        0
    );
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "feof",
            &[STD_FILE_BASE]
        )),
        1
    );
}

#[test]
fn stdio_redirection_and_child_output_share_position_but_keep_paths_process_local() {
    let dir = tempfile::tempdir().unwrap();
    let (mut cpu, mut k, mut d) = setup();
    k.vfs.mount("\\", dir.path());
    wide(&mut cpu, 0x1000, "\\cmd-output.txt");
    wide(&mut cpu, 0x1100, "w");
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "_wfreopen",
            &[0x1000, 0x1100, STD_FILE_BASE + 1]
        )),
        STD_FILE_BASE + 1
    );
    wide(&mut cpu, 0x1300, "%s:%04d\n");
    wide(&mut cpu, 0x1400, "CE");
    cpu.write_mem(
        0x1500,
        &[0x1400u32.to_le_bytes(), 6u32.to_le_bytes()].concat(),
    )
    .unwrap();
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "vfwprintf",
            &[STD_FILE_BASE + 1, 0x1300, 0x1500]
        )),
        8
    );
    let context = k.console_context();
    let (mut child_cpu, mut child, mut child_d) = setup();
    child.vfs.mount("\\", dir.path());
    child.attach_console_context(context);
    wide(&mut child_cpu, 0x1000, "child\n");
    assert_eq!(
        value(call(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "coredll.dll",
            "fputws",
            &[0x1000, STD_FILE_BASE + 1]
        )),
        0
    );
    wide(&mut cpu, 0x1600, "parent\n");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fputws",
        &[0x1600, STD_FILE_BASE + 1],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fflush",
        &[STD_FILE_BASE + 1],
    );
    assert_eq!(
        std::fs::read(dir.path().join("cmd-output.txt")).unwrap(),
        b"CE:0006\nchild\nparent\n"
    );
    wide(&mut child_cpu, 0x1100, "CON:");
    assert_eq!(
        value(call(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "coredll.dll",
            "SetStdioPathW",
            &[1, 0x1100]
        )),
        1
    );
    assert_eq!(k.console.paths[1], "\\cmd-output.txt");
    assert_eq!(child.console.paths[1], "CON:");
    assert!(child.console.view.is_none());
    cpu.write_mem(0x1210, &260u32.to_le_bytes()).unwrap();
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "GetStdioPathW",
            &[1, 0x1800, 0x1210]
        )),
        1
    );
    let path_length = cpu.read_u32_le(0x1210).unwrap();
    let path_bytes = cpu.read_mem(0x1800, path_length * 2).unwrap();
    let path: Vec<_> = path_bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|u| *u != 0)
        .collect();
    assert_eq!(String::from_utf16(&path).unwrap(), "\\cmd-output.txt");
}

#[test]
fn console_children_share_output_and_ctrl_c_signals_the_cmd_event() {
    let (mut cpu, mut k, mut d) = setup();
    k.ce.version = Some((6, 0, 0));
    wide(&mut cpu, 0x1000, "parent>");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fputws",
        &[0x1000, STD_FILE_BASE + 1],
    );
    let event = value(call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "CreateEventW",
        &[0, 1, 0, 0],
    ));
    cpu.write_mem(0x1200, &event.to_le_bytes()).unwrap();
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DeviceIoControl",
            &[1, 0x01020028, 0x1200, 4, 0, 0, 0, 0]
        )),
        1
    );
    let context = k.console_context();
    let hwnd = k.console.view.as_ref().unwrap().hwnd;
    let (mut child_cpu, mut child, mut child_d) = setup();
    child.attach_console_context(context);
    wide(&mut child_cpu, 0x1000, "child output\n");
    call(
        &mut child_cpu,
        &mut child,
        &mut child_d,
        "coredll.dll",
        "fputws",
        &[0x1000, STD_FILE_BASE + 1],
    );
    assert!(child.console.inherited);
    assert!(child.console.view.is_none());
    k.composite_controls();
    let snapshot = k.vfs.console_device().unwrap().lock().unwrap().snapshot();
    assert_eq!(
        snapshot.lines[0].iter().collect::<String>(),
        "parent>child output"
    );
    k.pending_input.extend([
        InputEvent::KeyDown { vk: 0x11 },
        InputEvent::KeyDown { vk: 0x43 },
        InputEvent::KeyUp { vk: 0x43 },
        InputEvent::KeyUp { vk: 0x11 },
    ]);
    call(&mut cpu, &mut k, &mut d, "coredll.dll", "GetTickCount", &[]);
    assert!(k.events.get(&k.console.control_event).unwrap().signalled);
    k.composite_controls();
    let before = k.framebuffer.frame_counter;
    k.composite_controls();
    assert_eq!(k.framebuffer.frame_counter, before);
    k.window_visibility.insert(hwnd, false);
    k.composite_controls();
    let hidden = k.framebuffer.frame_counter;
    wide(&mut cpu, 0x1000, "hidden output");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fputws",
        &[0x1000, STD_FILE_BASE + 1],
    );
    k.composite_controls();
    assert_eq!(k.framebuffer.frame_counter, hidden);
}

#[test]
fn nul_redirection_does_not_create_a_host_file_or_console_window() {
    let dir = tempfile::tempdir().unwrap();
    let (mut cpu, mut k, mut d) = setup();
    k.vfs.mount("\\", dir.path());
    wide(&mut cpu, 0x1000, "NUL:");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "SetStdioPathW",
        &[1, 0x1000],
    );
    wide(&mut cpu, 0x1100, "discarded");
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fputws",
            &[0x1100, STD_FILE_BASE + 1]
        )),
        0
    );
    assert!(k.console.view.is_none());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn redirected_wide_text_round_trips_through_the_ce_ansi_code_page() {
    let dir = tempfile::tempdir().unwrap();
    let (mut cpu, mut k, mut d) = setup();
    k.vfs.mount("\\", dir.path());
    wide(&mut cpu, 0x1000, "\\cmd-ansi.txt");
    wide(&mut cpu, 0x1100, "w");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "_wfreopen",
        &[0x1000, 0x1100, STD_FILE_BASE + 1],
    );
    wide(&mut cpu, 0x1300, "CE €œé\n");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fputws",
        &[0x1300, STD_FILE_BASE + 1],
    );
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fclose",
        &[STD_FILE_BASE + 1],
    );
    assert_eq!(
        std::fs::read(dir.path().join("cmd-ansi.txt")).unwrap(),
        b"CE \x80\x9c\xe9\n"
    );
    wide(&mut cpu, 0x1100, "r");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "_wfreopen",
        &[0x1000, 0x1100, STD_FILE_BASE],
    );
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fgetws",
            &[0x1500, 32, STD_FILE_BASE]
        )),
        0x1500
    );
    let bytes = cpu.read_mem(0x1500, 16).unwrap();
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|u| *u != 0)
        .collect();
    assert_eq!(String::from_utf16(&units).unwrap(), "CE €œé\n");
}

#[test]
fn console_read_rejects_invalid_output_without_losing_input_and_short_reads_are_not_errors() {
    let (mut cpu, mut k, mut d) = setup();
    k.ce.version = Some((6, 0, 0));
    wide(&mut cpu, 0x1000, ">");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fputws",
        &[0x1000, STD_FILE_BASE + 1],
    );
    for character in "ready".chars() {
        k.pending_input.push_back(InputEvent::Text { character });
    }
    k.pending_input
        .extend([InputEvent::KeyDown { vk: 13 }, InputEvent::KeyUp { vk: 13 }]);
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "ReadFile",
            &[0, 0x9000, 64, 0x1200, 0]
        )),
        0
    );
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fread",
            &[0x1300, 1, 64, STD_FILE_BASE]
        )),
        6
    );
    assert_eq!(cpu.read_mem(0x1300, 6).unwrap(), b"ready\n");
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "ferror",
            &[STD_FILE_BASE]
        )),
        0
    );
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "feof",
            &[STD_FILE_BASE]
        )),
        0
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fgets",
            &[0x1300, 64, STD_FILE_BASE]
        ),
        DispatchOutcome::JumpTo(pocket_kernel::THREAD_SCHEDULER_IDLE_VA)
    );
}

#[test]
fn held_console_editing_keys_repeat_without_a_guest_message_pump_and_stop_on_release() {
    let (mut cpu, mut k, mut d) = setup();
    k.ce.version = Some((6, 0, 0));
    wide(&mut cpu, 0x1000, ">");
    call(
        &mut cpu,
        &mut k,
        &mut d,
        "coredll.dll",
        "fputws",
        &[0x1000, STD_FILE_BASE + 1],
    );
    for character in "abcde".chars() {
        k.pending_input.push_back(InputEvent::Text { character });
    }
    k.pending_input.push_back(InputEvent::KeyDown { vk: 0x25 });
    call(&mut cpu, &mut k, &mut d, "coredll.dll", "GetTickCount", &[]);
    k.key_repeat_next_ms = Some(monotonic_ms());
    call(&mut cpu, &mut k, &mut d, "coredll.dll", "GetTickCount", &[]);
    k.pending_input.extend([
        InputEvent::KeyUp { vk: 0x25 },
        InputEvent::Text { character: 'X' },
        InputEvent::KeyDown { vk: 13 },
        InputEvent::KeyUp { vk: 13 },
    ]);
    assert_eq!(
        value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "fgetws",
            &[0x1300, 32, STD_FILE_BASE]
        )),
        0x1300
    );
    let bytes = cpu.read_mem(0x1300, 16).unwrap();
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|u| *u != 0)
        .collect();
    assert_eq!(String::from_utf16(&units).unwrap(), "abcXde\n");
    assert!(k.held_keys.is_empty());
}
