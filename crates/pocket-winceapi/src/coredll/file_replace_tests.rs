use super::tests::{dummy_thunk, fresh_kernel, scanf_cpu};
use super::*;
use pocket_cpu::Cpu;
use pocket_kernel::Dispatcher;
use pocket_pe::ImportBinding;

#[test]
fn ce_document_replacement_uses_destination_first_and_reports_failures() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("old.pwd"), b"old").unwrap();
    std::fs::write(root.path().join("new.tmp"), b"new document").unwrap();
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.vfs.mount_save_dir("\\", root.path());
    let destination = 0x3000;
    let source = 0x3100;
    for (address, name) in [(destination, "\\old.pwd"), (source, "\\new.tmp")] {
        let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        cpu.write_mem(address, &wide_to_bytes(&wide)).unwrap();
    }
    let mut dispatcher = WinCeDispatcher::new();
    let mut thunk = dummy_thunk();
    thunk.dll = "coredll.dll".into();
    thunk.binding = ImportBinding::Name("DeleteAndRenameFile".into());
    thunk.friendly_name = Some("DeleteAndRenameFile".into());
    cpu.write_reg(ArmReg::R0, destination).unwrap();
    cpu.write_reg(ArmReg::R1, 0).unwrap();
    assert_eq!(
        dispatcher.dispatch(&mut cpu, &thunk, &mut kernel).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors.get(&0), Some(&87));
    assert_eq!(std::fs::read(root.path().join("old.pwd")).unwrap(), b"old");
    cpu.write_reg(ArmReg::R0, destination).unwrap();
    cpu.write_reg(ArmReg::R1, source).unwrap();
    assert_eq!(
        dispatcher.dispatch(&mut cpu, &thunk, &mut kernel).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(
        std::fs::read(root.path().join("old.pwd")).unwrap(),
        b"new document"
    );
    assert!(!root.path().join("new.tmp").exists());
    cpu.write_reg(ArmReg::R0, destination).unwrap();
    cpu.write_reg(ArmReg::R1, source).unwrap();
    assert_eq!(
        dispatcher.dispatch(&mut cpu, &thunk, &mut kernel).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors.get(&0), Some(&2));
    assert_eq!(
        std::fs::read(root.path().join("old.pwd")).unwrap(),
        b"new document"
    );
}
