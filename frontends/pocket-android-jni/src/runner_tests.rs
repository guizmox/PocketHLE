//! Exercise the Android runner with real ARM diagnostics, without JNI or sensors.
use super::*;
use std::{fs, path::Path, time::SystemTime};

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("pockethle-android-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn report(root: &Path, name: &str) -> String {
    let path = fs::read_dir(root.join("flash"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
        .unwrap();
    fs::read_to_string(path).unwrap()
}
fn run_diagnostic(root: &Path, id: &str) {
    let session = start(root.to_path_buf(), id.to_string()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    while session.is_running() && Instant::now() < deadline {
        // Dismiss only the diagnostic's final modal; input is routed by the runner.
        session.send_input(InputCommand::Input(InputEvent::KeyDown { vk: 0x0d }));
        session.send_input(InputCommand::Input(InputEvent::KeyUp { vk: 0x0d }));
        std::thread::sleep(Duration::from_millis(20));
    }
    let completed = !session.is_running();
    let summary = session.finish();
    assert!(completed, "diagnostic timed out: {summary}");
    assert!(summary.contains("Emulator exited cleanly."), "{summary}");
}

#[test]
fn android_runner_ramtest_processes_and_orphan_complete() {
    let files = Files::new();
    let mut lib = Library::open(&files.0).unwrap();
    let id = lib
        .import_zip(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/ramtest/dist/PocketHLE-RAMTEST.zip"),
        )
        .unwrap()
        .id
        .clone();
    run_diagnostic(&files.0, &id);
    for (name, result) in [
        (
            "RAMTEST.TXT",
            "RAMTEST_RESULT PASS checks=0x00000084 failures=0x00000000",
        ),
        ("PROCTEST.TXT", "PROCTEST_RESULT PASS"),
        ("ORPHANTEST.TXT", "ORPHANTEST_RESULT PASS"),
        ("DLLTEST.TXT", "DLLTEST_RESULT PASS"),
        ("DEPTEST.TXT", "DEPTEST_RESULT PASS"),
    ] {
        let text = report(&files.0, name);
        println!("{text}");
        assert!(text.contains(result) && !text.contains("FAIL"), "{text}");
    }
}

#[test]
fn android_runner_vfstest_sharing_rename_quota_and_repeat_cleanup() {
    let files = Files::new();
    let mut lib = Library::open(&files.0).unwrap();
    let id = lib
        .import_zip(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/vfstest/dist/PocketHLE-VFSTEST.zip"),
        )
        .unwrap()
        .id
        .clone();
    for _ in 0..2 {
        run_diagnostic(&files.0, &id);
        let text = report(&files.0, "VFSTEST.TXT");
        println!("{text}");
        assert!(
            text.contains("VFSTEST_RESULT PASS checks=0x00000067 failures=0x00000000"),
            "{text}"
        );
        assert!(!text.lines().any(|l| l.starts_with("FAIL ")), "{text}");
        assert_eq!(
            fs::read_dir(files.0.join("flash")).unwrap().count(),
            1,
            "probe files, directories and quota allocations must be removed"
        );
    }
}

#[test]
fn disabled_process_launch_reproduces_android_vfs_report() {
    let files = Files::new();
    let mut lib = Library::open(&files.0).unwrap();
    let entry = lib
        .import_zip(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/vfstest/dist/PocketHLE-VFSTEST.zip"),
        )
        .unwrap()
        .clone();
    let mut emu = Emulator::with_unicorn_cpu().unwrap();
    emu.max_slices = u64::MAX;
    emu.load_pe(entry.launch_path(&files.0)).unwrap();
    assert!(emu.set_memory_division(Some(MemoryDivision::gizmondo_sdk_default())));
    emu.mount_read_only_dir("\\SD Card\\", entry.extracted_dir(&files.0));
    emu.mount_save_dir("\\Flash Disk\\", files.0.join("flash"));
    emu.set_module_path("\\SD Card\\AUTORUN.EXE");
    emu.set_default_dir("\\SD Card\\");
    emu.set_synthetic_message_budget(0);
    let deadline = Instant::now() + Duration::from_secs(45);
    emu.run_with_hook(&mut |kernel: &mut KernelState| {
        assert!(
            Instant::now() < deadline,
            "disabled-runner reproduction timed out"
        );
        if kernel.modal.is_some() {
            kernel
                .pending_input
                .push_back(InputEvent::KeyDown { vk: 0x0d });
        }
        FrameAction::Continue
    })
    .unwrap();
    let text = report(&files.0, "VFSTEST.TXT");
    println!("{text}");
    assert!(
        text.contains("VFSTEST_RESULT FAIL checks=0x00000067 failures=0x00000018"),
        "{text}"
    );
}
