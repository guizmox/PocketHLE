//! Suspended CPU continuations use the ordinary device serializer and barrier.
use super::*;
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{
    handles::HandleObject, ChildProcess, FrameAction, FrameHook, KernelState, ProcessLaunch,
};
use pocket_pe::{LoadedImage, LoadedSection};
use sha2::{Digest, Sha256};
use std::time::Duration;
fn fixture(initial_value: u32) -> Emulator {
    let mut emulator = Emulator::with_unicorn_cpu().unwrap();
    let mut code = [0xe3a00000u32 | initial_value, 0xe2800002, 0xe12fff1e]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    code.resize(4096, 0);
    let fingerprint = Sha256::digest(&code).into();
    let image = LoadedImage {
        source_path: "checkpoint-fixture".into(),
        machine: pe::machine::ARM,
        subsystem: pe::subsystem::WINDOWS_CE_GUI,
        image_base: 0x10000,
        stack_reserve: 0x10000,
        size_of_image: 0x2000,
        entry_point: 0x1000,
        sections: vec![LoadedSection {
            name: ".text".into(),
            virtual_address: 0x1000,
            virtual_size: 0x1000,
            characteristics: 0x60000020,
            data: code,
        }],
        imports: vec![],
        exports: Default::default(),
        resources: vec![],
        managed_runtime: None,
    };
    emulator.process = Some(
        Process::map_into(
            image,
            emulator.cpu.as_mut(),
            &|_, _| None,
            &emulator.dispatcher,
        )
        .unwrap(),
    );
    emulator.process.as_mut().unwrap().executable_fingerprint = Some(fingerprint);
    emulator.max_slices = 8;
    emulator
}

fn suspend(parent: &mut Emulator, child: &mut Emulator, directory: &std::path::Path) -> u32 {
    let state = &mut parent.process.as_mut().unwrap().state;
    state.vfs.mount_read_only(r"\SD Card\", directory);
    state.module_path = r"\SD Card\parent.exe".into();
    let (handle, table) = state.object_handles.new_child().unwrap();
    state
        .object_handles
        .bind(handle, HandleObject::ChildProcess(handle));
    state
        .object_handles
        .bind(handle + 1, HandleObject::ChildThread(handle));
    state.child_processes.insert(
        handle,
        ChildProcess {
            thread_handle: handle + 1,
            process_handle_closed: false,
            thread_handle_closed: false,
            exit_code: None,
        },
    );
    state.suspended_process_launch = Some(ProcessLaunch {
        concurrent: false,
        command_line: String::new(),
        creation_flags: 0,
        call_key: Some((0, 0x70001000, 0x60000000)),
        executable: directory.join("child.exe"),
        guest_path: r"\SD Card\child.exe".into(),
        process_handle: handle,
        thread_handle: handle + 1,
    });
    child
        .process
        .as_mut()
        .unwrap()
        .state
        .attach_handle_context(state.child_handle_context(handle).unwrap());
    child.process.as_mut().unwrap().state.module_path = r"\SD Card\child.exe".into();
    table.process_id()
}
#[test]
fn cold_restore_waits_for_the_saved_child_before_executing_any_parent_instruction() {
    let directory = tempfile::tempdir().unwrap();
    for name in ["parent.exe", "child.exe"] {
        std::fs::write(directory.path().join(name), b"fixture").unwrap();
    }
    let mut parent = fixture(3);
    let mut child = fixture(3);
    let child_pid = suspend(&mut parent, &mut child, directory.path());
    parent.cpu.write_reg(ArmReg::R0, 40).unwrap();
    child.cpu.write_reg(ArmReg::R0, 80).unwrap();
    let at = crate::checkpoint_domain::CheckpointTime::now();
    let point = ResumePoint {
        main_entry: 0x11000,
        pc: 0x11004,
    };
    let first = SessionSnapshot::capture_device_process(
        parent.cpu.as_mut(),
        parent.process.as_mut().unwrap(),
        point,
        true,
        at,
    )
    .unwrap();
    let second = SessionSnapshot::capture_device_process(
        child.cpu.as_mut(),
        child.process.as_mut().unwrap(),
        point,
        false,
        at,
    )
    .unwrap();
    let saved =
        SessionSnapshot::assemble_device(vec![(1, first), (child_pid, second)], child_pid).unwrap();
    let mut bytes = Vec::new();
    saved.write(&mut bytes).unwrap();
    drop(saved);
    drop(child);
    drop(parent);
    let mut saved = SessionSnapshot::read(bytes.as_slice()).unwrap();
    let (shared, foreground, mut children) = saved.take_device().unwrap().into_parts();
    let mut destination = fixture(3);
    destination
        .process
        .as_mut()
        .unwrap()
        .state
        .vfs
        .mount_read_only(r"\SD Card\", directory.path());
    let group = std::sync::Arc::new(
        DeviceRestoreGroup::new(
            shared,
            foreground,
            &destination.process.as_ref().unwrap().state,
            Duration::from_secs(5),
        )
        .unwrap(),
    );
    let remote = group.clone();
    let child_saved = children.remove(0);
    let path = directory.path().to_path_buf();
    let (release, wait) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut child = fixture(3);
        child
            .process
            .as_mut()
            .unwrap()
            .state
            .vfs
            .mount_read_only(r"\SD Card\", path);
        let restored = remote.restore_process(&mut child, child_saved).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        child
            .run_restored(restored, &mut |_: &mut KernelState| FrameAction::Continue)
            .unwrap();
        let state = &child.process.as_ref().unwrap().state;
        assert_eq!(state.process_exit_code, Some(82));
        let table = state.object_handles.clone();
        drop(child);
        table.complete_process_exit(82);
    });
    let restored = group.restore_process(&mut destination, saved).unwrap();
    struct WaitHook {
        release: Option<std::sync::mpsc::Sender<()>>,
        child: u32,
        calls: usize,
    }
    impl FrameHook for WaitHook {
        fn on_frame(&mut self, _: &mut KernelState) -> FrameAction {
            FrameAction::Continue
        }
        fn on_checkpoint(&mut self, cpu: &mut dyn Cpu, process: &mut Process, _: ResumePoint) {
            self.calls += 1;
            assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 40);
            assert!(process.state.suspended_process_launch.is_some());
            self.release.take().unwrap().send(()).unwrap();
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while process
                .state
                .object_handles
                .exit_code(self.child, None)
                .is_none()
            {
                assert!(std::time::Instant::now() < until);
                std::thread::yield_now();
            }
            process.state.suspended_process_launch = None;
        }
    }
    let mut hook = WaitHook {
        release: Some(release),
        child: child_pid,
        calls: 0,
    };
    destination.run_restored(restored, &mut hook).unwrap();
    worker.join().unwrap();
    assert_eq!(hook.calls, 1);
    assert_eq!(
        destination
            .process
            .as_ref()
            .unwrap()
            .state
            .process_exit_code,
        Some(42)
    );
}

#[test]
fn device_capture_rejects_a_cyclic_suspended_launcher_chain() {
    let directory = tempfile::tempdir().unwrap();
    for name in ["parent.exe", "child.exe"] {
        std::fs::write(directory.path().join(name), b"fixture").unwrap();
    }
    let mut parent = fixture(3);
    let mut child = fixture(3);
    let child_pid = suspend(&mut parent, &mut child, directory.path());
    let root = parent
        .process
        .as_ref()
        .unwrap()
        .state
        .object_handles
        .process_id();
    let state = &mut child.process.as_mut().unwrap().state;
    state.vfs.mount_read_only(r"\SD Card\", directory.path());
    let handle = 0xd1800000;
    state
        .object_handles
        .bind(handle, HandleObject::RemoteProcess(root));
    state.child_processes.insert(
        handle,
        ChildProcess {
            thread_handle: handle + 1,
            process_handle_closed: false,
            thread_handle_closed: false,
            exit_code: None,
        },
    );
    state.suspended_process_launch = Some(ProcessLaunch {
        concurrent: false,
        command_line: String::new(),
        creation_flags: 0,
        call_key: None,
        executable: directory.path().join("parent.exe"),
        guest_path: r"\SD Card\parent.exe".into(),
        process_handle: handle,
        thread_handle: handle + 1,
    });
    let at = crate::checkpoint_domain::CheckpointTime::now();
    let point = ResumePoint {
        main_entry: 0x11000,
        pc: 0x11004,
    };
    let first = SessionSnapshot::capture_device_process(
        parent.cpu.as_mut(),
        parent.process.as_mut().unwrap(),
        point,
        true,
        at,
    )
    .unwrap();
    let second = SessionSnapshot::capture_device_process(
        child.cpu.as_mut(),
        child.process.as_mut().unwrap(),
        point,
        false,
        at,
    )
    .unwrap();
    let error =
        SessionSnapshot::assemble_device(vec![(root, first), (child_pid, second)], child_pid)
            .err()
            .expect("cyclic device checkpoint must fail");
    assert!(
        error
            .to_string()
            .contains("cyclic suspended launcher chain"),
        "{error:#}"
    );
}
