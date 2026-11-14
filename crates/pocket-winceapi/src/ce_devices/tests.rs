use super::*;
use crate::bluetooth::tests::{call, setup};
use pocket_cpu::{stub::StubCpu, Cpu};

fn put(cpu: &mut StubCpu, pointer: u32, value: &str) {
    cpu.write_mem(
        pointer,
        &value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

fn result(outcome: DispatchOutcome) -> u32 {
    let DispatchOutcome::ReturnedR0(value) = outcome else {
        panic!("unexpected continuation")
    };
    value
}

#[test]
fn password_and_afs_names_share_the_child_device_context_and_preserve_ownership() {
    let (mut cpu, mut parent, mut dispatcher) = setup();
    parent.thread_last_errors.insert(0, 123);
    assert_eq!(
        call(
            &mut cpu,
            &mut parent,
            &mut dispatcher,
            "coredll.dll",
            "GetPasswordActive",
            &[]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(parent.thread_last_errors[&0], 123);
    parent.ce.device.configure_password_active(true);
    let (handle, _) = parent.object_handles.new_child().unwrap();
    let context = parent.child_handle_context(handle).unwrap();
    let (mut child_cpu, mut child, mut child_dispatcher) = setup();
    child.attach_handle_context(context);
    assert_eq!(
        call(
            &mut child_cpu,
            &mut child,
            &mut child_dispatcher,
            "coredll.dll",
            "GetPasswordActive",
            &[]
        ),
        DispatchOutcome::ReturnedR0(1)
    );
    put(&mut cpu, 0x1400, "Services");
    let index = result(call(
        &mut cpu,
        &mut parent,
        &mut dispatcher,
        "coredll.dll",
        "RegisterAFSName",
        &[0x1400],
    ));
    assert!(index > 0 && index < i32::MAX as u32);
    put(&mut child_cpu, 0x1400, "SERVICES");
    assert_eq!(
        call(
            &mut child_cpu,
            &mut child,
            &mut child_dispatcher,
            "coredll.dll",
            "RegisterAFSName",
            &[0x1400]
        ),
        DispatchOutcome::ReturnedR0(u32::MAX)
    );
    assert_eq!(child.thread_last_errors[&0], 183);
    assert_eq!(
        call(
            &mut child_cpu,
            &mut child,
            &mut child_dispatcher,
            "coredll.dll",
            "DeregisterAFSName",
            &[index]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(child.thread_last_errors[&0], 5);
    parent.record_process_exit(0);
    let replacement = result(call(
        &mut child_cpu,
        &mut child,
        &mut child_dispatcher,
        "coredll.dll",
        "RegisterAFSName",
        &[0x1400],
    ));
    assert!(replacement > index);
    assert!(child.ce.device.password_active());
    assert_eq!(
        call(
            &mut child_cpu,
            &mut child,
            &mut child_dispatcher,
            "coredll.dll",
            "DeregisterAFSName",
            &[replacement]
        ),
        DispatchOutcome::ReturnedR0(1)
    );
}

#[test]
fn afs_registration_rejects_protected_names_and_does_not_advertise_a_mounted_driver() {
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    cpu.map_region(0x5000, 4096, Prot::NONE).unwrap();
    for pointer in [0, 0x5000, 0xffff_ffff] {
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "RegisterAFSName",
                &[pointer]
            ),
            DispatchOutcome::ReturnedR0(u32::MAX)
        );
        assert_eq!(kernel.thread_last_errors[&0], 87);
    }
    put(&mut cpu, 0x1400, "");
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "RegisterAFSName",
            &[0x1400]
        ),
        DispatchOutcome::ReturnedR0(u32::MAX)
    );
    put(&mut cpu, 0x1400, "Devices");
    let index = result(call(
        &mut cpu,
        &mut kernel,
        &mut dispatcher,
        "coredll.dll",
        "RegisterAFSName",
        &[0x1400],
    ));
    cpu.write_mem(0x1500, b"TEST").unwrap();
    cpu.write_mem(0x1600, &[0; 8]).unwrap();
    let provider = result(call(
        &mut cpu,
        &mut kernel,
        &mut dispatcher,
        "coredll.dll",
        "CreateAPISet",
        &[0x1500, 1, 0x1600, 0x1600],
    ));
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "RegisterAFSEx",
            &[index, provider, 0, 4, 0x101]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors[&0], 50);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "RegisterAFSEx",
            &[index, u32::MAX, 0, 4, 0x101]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors[&0], 6);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "DeregisterAFS",
            &[index]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors[&0], 1168);
    assert!(kernel.object_handles.get(provider).is_some());
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "DeregisterAFSName",
            &[index]
        ),
        DispatchOutcome::ReturnedR0(1)
    );
}

#[test]
fn activation_validates_regini_before_driver_lookup_and_keeps_registry_unchanged() {
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    cpu.map_region(0x5000, 4096, Prot::NONE).unwrap();
    put(&mut cpu, 0x1400, "Services");
    put(&mut cpu, 0x1500, "BusName");
    put(&mut cpu, 0x1600, "Services");
    let size = ("Services".encode_utf16().count() as u32 + 1) * 2;
    let regini = [0x1500u32, 0x1600, size, 1]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    cpu.write_mem(0x1700, &regini).unwrap();
    kernel.registry.set_value(
        "HKLM\\Services",
        "Dll",
        RegistryValue::Sz("servicesd.exe".into()),
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "ActivateDeviceEx",
            &[0x1400, 0x1700, 1, 0]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors[&0], 50);
    assert_eq!(
        kernel.registry.value("HKLM\\Services", "Dll"),
        Some(RegistryValue::Sz("servicesd.exe".into()))
    );
    assert_eq!(kernel.registry.value("HKLM\\Services", "BusName"), None);
    assert!(!kernel.registry.contains_key("HKLM\\Drivers\\Active"));
    for args in [
        [0x1400, 0x5000, 1, 0],
        [0x1400, 0x4ffc, 2, 0],
        [0x1400, 0, u32::MAX, 0],
        [0x5000, 0, 0, 0],
    ] {
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "ActivateDeviceEx",
                &args
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.thread_last_errors[&0], 87);
    }
    cpu.write_mem(0x1704, &0x5000u32.to_le_bytes()).unwrap();
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "ActivateDeviceEx",
            &[0x1400, 0x1700, 1, 0]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors[&0], 87);
    cpu.write_mem(0x1704, &0x1600u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x170c, &4u32.to_le_bytes()).unwrap();
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "ActivateDeviceEx",
            &[0x1400, 0x1700, 1, 0]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors[&0], 13);
    put(&mut cpu, 0x1400, "MissingDriver");
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "ActivateDeviceEx",
            &[0x1400, 0xffff_ffff, 0, 0]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(kernel.thread_last_errors[&0], 2);
}

#[test]
fn power_off_retries_the_original_call_until_resume_and_exit_cancels_it() {
    let (mut cpu, mut kernel, mut dispatcher) = setup();
    cpu.write_reg(ArmReg::Lr, 0x1234).unwrap();
    kernel.thread_last_errors.insert(0, 321);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "PowerOffSystem",
            &[]
        ),
        DispatchOutcome::JumpTo(0x70000000)
    );
    let generation = kernel.ce.device.suspended_generation().unwrap();
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "PowerOffSystem",
            &[]
        ),
        DispatchOutcome::JumpTo(0x70000000)
    );
    assert_eq!(kernel.ce.device.suspended_generation(), Some(generation));
    assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
    assert_eq!(kernel.thread_last_errors[&0], 321);
    kernel.ce.device.resume();
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "PowerOffSystem",
            &[]
        ),
        DispatchOutcome::ReturnedR0(0)
    );
    assert!(kernel.ce.power_calls.is_empty());
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "PowerOffSystem",
            &[]
        ),
        DispatchOutcome::JumpTo(0x70000000)
    );
    assert_ne!(kernel.ce.device.suspended_generation(), Some(generation));
    let shared = kernel.ce.device.clone();
    kernel.record_process_exit(5);
    assert!(kernel.ce.power_calls.is_empty());
    assert_eq!(shared.suspended_generation(), None);
}
