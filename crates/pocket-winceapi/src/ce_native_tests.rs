//! Execute real ARM/Thumb callbacks, rather than simulating their return in Rust.
use super::*;
use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu, Prot, StopReason};
const RETURN: u32 = 0x70000000;

#[test]
fn focus_callbacks_execute_arm_and_thumb_before_returning_to_the_original_caller() {
    // Store message and other HWND at offsets selected by the receiving HWND.
    let arm: Vec<_> = [
        0xe59f3010u32,
        0xe080c003,
        0xe58c1000,
        0xe58c2004,
        0xe3a00000,
        0xe12fff1e,
        0x1000,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let thumb: Vec<_> = [0x4b02u16, 0x181b, 0x6019, 0x605a, 0x2000, 0x4770]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .chain(0x1000u32.to_le_bytes())
        .collect();
    // Use small synthetic HWND values to select distinct result records.
    for (code, entry) in [(&arm, 0x2000), (&thumb, 0x2001)] {
        let (mut cpu, mut kernel, mut dispatcher) = setup(code);
        for hwnd in [0x100, 0x200] {
            kernel.window_procs.insert(hwnd, entry);
            kernel.window_threads.insert(hwnd, 0);
        }
        kernel.controls.focus = 0x100;
        args(&mut cpu, [0x200, 1, 2, 3]);
        let t = thunk("SetFocus");
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::JumpTo(entry)
        );
        assert_eq!(
            cpu.run_until_hook(entry, 100).unwrap(),
            StopReason::Hook(RETURN)
        );
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::JumpTo(entry)
        );
        assert_eq!(
            cpu.run_until_hook(entry, 100).unwrap(),
            StopReason::Hook(RETURN)
        );
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::ReturnedR0(0x100)
        );
        assert_eq!(cpu.read_u32_le(0x1100).unwrap(), 8);
        assert_eq!(cpu.read_u32_le(0x1104).unwrap(), 0x200);
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 7);
        assert_eq!(cpu.read_u32_le(0x1204).unwrap(), 0x100);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
        assert!(kernel.ce.focus_changes.is_empty());
    }
}

#[test]
fn native_com_factories_execute_both_entry_modes_and_release_before_returning_the_create_result() {
    let arm: Vec<_> = [0xe59f3008u32, 0xe5823000, 0xe3a00000, 0xe12fff1e, 0x3000]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut thumb: Vec<_> = [0x4b01u16, 0x6013, 0x2000, 0x4770]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    thumb.extend(0x3000u32.to_le_bytes());
    for (code, entry) in [(&arm, 0x2000), (&thumb, 0x2001)] {
        for result in [0, 0x80004002u32] {
            let (mut cpu, mut kernel, mut dispatcher) = setup(code);
            kernel.heap = pocket_kernel::Heap::new(0x50000000, 0x10000);
            let create: Vec<_> = [
                0xe59fc008u32,
                0xe583c000,
                0xe59f0004,
                0xe12fff1e,
                if result == 0 { 0x3300 } else { 0 },
                result,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            let release: Vec<_> = [
                0xe59f1010u32,
                0xe5912000,
                0xe2822001,
                0xe5812000,
                0xe3a00021,
                0xe12fff1e,
                0x3400,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            cpu.write_mem(0x2100, &create).unwrap();
            cpu.write_mem(0x2200, &release).unwrap();
            cpu.write_mem(0x3000, &0x3100u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x3108, &0x2200u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x310c, &0x2100u32.to_le_bytes()).unwrap();
            let class = [0x42u8; 16];
            let iid = [0x24u8; 16];
            cpu.write_mem(0x1100, &class).unwrap();
            cpu.write_mem(0x1200, &iid).unwrap();
            cpu.write_mem(0x4000, &0x1300u32.to_le_bytes()).unwrap();
            kernel.registry.set_value(
                &format!(
                    r"HKCR\CLSID\{}\InprocServer32",
                    crate::ole32::guid_string(&class)
                ),
                "default",
                pocket_kernel::registry::RegistryValue::Sz("fixture.dll".into()),
            );
            kernel
                .ce
                .com
                .servers
                .insert("fixture.dll".into(), 0x31000000);
            kernel.dynamic_exports.insert(
                0x31000000,
                [("DllGetClassObject".to_owned(), entry)]
                    .into_iter()
                    .collect(),
            );
            args(&mut cpu, [0x1100, 0, 3, 0x1200]);
            let mut t = thunk("CoCreateInstance");
            t.dll = "ole32.dll".into();
            let outcome = dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap();
            assert_eq!(outcome, DispatchOutcome::JumpTo(entry));
            let scratch = kernel.ce.com.activations[0].scratch;
            // The factory receives IClassFactory rather than the requested
            // application interface. The latter reaches CreateInstance.
            assert_eq!(
                cpu.read_mem(scratch + 16, 16).unwrap(),
                [1, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]
            );
            cpu.write_reg(ArmReg::Sp, 0x3ff0).unwrap();
            assert_eq!(
                crate::native_com::resume(&mut CallCtx {
                    cpu: &mut cpu,
                    kernel: &mut kernel,
                    thunk: &t
                })
                .unwrap(),
                None
            );
            cpu.write_reg(ArmReg::Sp, 0x4000).unwrap();
            assert_eq!(
                cpu.run_until_hook(entry, 100).unwrap(),
                StopReason::Hook(RETURN)
            );
            assert_eq!(
                dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
                DispatchOutcome::JumpTo(0x2100)
            );
            assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 0);
            let requested_iid = cpu.read_reg(ArmReg::R2).unwrap();
            assert_eq!(cpu.read_mem(requested_iid, 16).unwrap(), iid);
            assert_eq!(
                cpu.run_until_hook(0x2100, 100).unwrap(),
                StopReason::Hook(RETURN)
            );
            assert_eq!(
                dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
                DispatchOutcome::JumpTo(0x2200)
            );
            assert_eq!(
                cpu.run_until_hook(0x2200, 100).unwrap(),
                StopReason::Hook(RETURN)
            );
            assert_eq!(
                dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
                DispatchOutcome::ReturnedR0(result)
            );
            assert_eq!(
                cpu.read_u32_le(0x1300).unwrap(),
                if result == 0 { 0x3300 } else { 0 }
            );
            assert_eq!(cpu.read_u32_le(0x3400).unwrap(), 1);
            assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert!(kernel.ce.com.activations.is_empty());
            assert_eq!(kernel.heap.msize(scratch), None);
        }
    }
}

#[test]
fn bind_context_retains_native_objects_restores_callbacks_and_releases_every_owned_reference() {
    fn call(
        cpu: &mut UnicornCpu,
        kernel: &mut pocket_kernel::KernelState,
        dispatcher: &mut WinCeDispatcher,
        name: &str,
        values: [u32; 4],
    ) -> u32 {
        args(cpu, values);
        cpu.write_reg(ArmReg::Sp, 0x4000).unwrap();
        cpu.write_reg(ArmReg::Lr, 0x1234).unwrap();
        let mut t = thunk(name);
        t.dll = "ole32.dll".into();
        t.thunk_va = kernel.dynamic_exports[&pocket_kernel::OLE32_MODULE_HANDLE][name];
        let mut interleaved = false;
        loop {
            match dispatcher.dispatch(cpu, &t, kernel).unwrap() {
                DispatchOutcome::JumpTo(entry) => {
                    if !interleaved {
                        cpu.write_reg(ArmReg::Sp, 0x3ff0).unwrap();
                        assert_eq!(
                            crate::native_com::resume_references(&mut CallCtx {
                                cpu,
                                kernel,
                                thunk: &t
                            })
                            .unwrap(),
                            None
                        );
                        cpu.write_reg(ArmReg::Sp, 0x4000).unwrap();
                        // Another thread may yield inside its own COM callback.
                        // Its newer frame must not hide this thread's return.
                        kernel
                            .ce
                            .com
                            .callbacks
                            .push(pocket_kernel::ce::ComCallback {
                                thread: 1,
                                thunk: t.thunk_va,
                                frame: pocket_kernel::GuestCallFrame {
                                    args: [0; 4],
                                    sp: 0x4000,
                                    lr: 0x5678,
                                },
                                calls: Vec::new(),
                                result: 0,
                                free: Vec::new(),
                            });
                        interleaved = true;
                    }
                    assert_eq!(
                        cpu.run_until_hook(entry, 100).unwrap(),
                        StopReason::Hook(t.thunk_va)
                    );
                }
                DispatchOutcome::ReturnedR0(value) => {
                    if interleaved {
                        assert_eq!(kernel.ce.com.callbacks.len(), 1);
                        assert_eq!(kernel.ce.com.callbacks[0].thread, 1);
                        kernel.ce.com.callbacks.clear();
                    }
                    assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
                    assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
                    return value;
                }
                other => panic!("unexpected binding result: {other:?}"),
            }
        }
    }
    let add: Vec<_> = [
        0xe5901004u32,
        0xe2811001,
        0xe5801004,
        0xe1a00001,
        0xe12fff1e,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let release: Vec<_> = [
        0xe5901004u32,
        0xe2411001,
        0xe5801004,
        0xe1a00001,
        0xe12fff1e,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let (mut cpu, mut kernel, mut dispatcher) = setup(&add);
    kernel.heap = pocket_kernel::Heap::new(0x50000000, 0x10000);
    cpu.write_mem(0x2100, &release).unwrap();
    let names = [
        "CreateBindCtx",
        "bind_query",
        "bind_add_ref",
        "bind_release",
        "bind_register_bound",
        "bind_revoke_bound",
        "bind_release_bound",
        "bind_set_options",
        "bind_get_options",
        "bind_get_rot",
        "bind_register_param",
        "bind_get_param",
        "bind_enum_params",
        "bind_revoke_param",
    ];
    let mut exports = std::collections::HashMap::new();
    for (i, name) in names.into_iter().enumerate() {
        let address = RETURN + i as u32 * 32;
        cpu.write_mem(address, &0xe12fff1eu32.to_le_bytes())
            .unwrap();
        if i != 0 {
            cpu.add_code_hook(address).unwrap();
        }
        exports.insert(name.into(), address);
    }
    kernel
        .dynamic_exports
        .insert(pocket_kernel::OLE32_MODULE_HANDLE, exports);
    for object in [0x3000, 0x3020] {
        cpu.write_mem(object, &0x3100u32.to_le_bytes()).unwrap();
        cpu.write_mem(object + 4, &1u32.to_le_bytes()).unwrap();
    }
    cpu.write_mem(0x3104, &0x2000u32.to_le_bytes()).unwrap();
    cpu.write_mem(0x3108, &0x2100u32.to_le_bytes()).unwrap();
    let key: Vec<_> = "BrowserClient\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    cpu.write_mem(0x1600, &key).unwrap();
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "CreateBindCtx",
            [0, 0x1300, 0, 0]
        ),
        0
    );
    let context = cpu.read_u32_le(0x1300).unwrap();
    let vtable = cpu.read_u32_le(context).unwrap();
    let opts: Vec<_> = [16u32, 4, 1, 500]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    cpu.write_mem(0x1500, &opts).unwrap();
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_set_options",
            [context, 0x1500, 0, 0]
        ),
        0
    );
    cpu.write_mem(0x1500, &32u32.to_le_bytes()).unwrap();
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_get_options",
            [context, 0x1500, 0, 0]
        ),
        0
    );
    assert_eq!(
        cpu.read_mem(0x1500, 32).unwrap(),
        [32u32, 4, 1, 500, 0, 5, 0x409, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    cpu.write_mem(0x1500, &12u32.to_le_bytes()).unwrap();
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_set_options",
            [context, 0x1500, 0, 0]
        ),
        0x80070057
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_register_bound",
            [context, 0x3000, 0, 0]
        ),
        0
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_register_param",
            [context, 0x1600, 0x3000, 0]
        ),
        0
    );
    assert_eq!(cpu.read_u32_le(0x3004).unwrap(), 3);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_get_param",
            [context, 0x1600, 0x1304, 0]
        ),
        0
    );
    assert_eq!(cpu.read_u32_le(0x1304).unwrap(), 0x3000);
    assert_eq!(cpu.read_u32_le(0x3004).unwrap(), 4);
    args(&mut cpu, [0x3000, 0, 0, 0]);
    cpu.write_reg(ArmReg::Lr, RETURN).unwrap();
    assert_eq!(
        cpu.run_until_hook(0x2100, 100).unwrap(),
        StopReason::Hook(RETURN)
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_register_param",
            [context, 0x1600, 0x3000, 0]
        ),
        0
    );
    assert_eq!(cpu.read_u32_le(0x3004).unwrap(), 3);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_register_param",
            [context, 0x1600, 0x3020, 0]
        ),
        0
    );
    assert_eq!(cpu.read_u32_le(0x3004).unwrap(), 2);
    assert_eq!(cpu.read_u32_le(0x3024).unwrap(), 2);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_release_bound",
            [context, 0, 0, 0]
        ),
        0
    );
    assert_eq!(cpu.read_u32_le(0x3004).unwrap(), 1);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_revoke_bound",
            [context, 0x3000, 0, 0]
        ),
        0x800401e9
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_revoke_param",
            [context, 0x1600, 0, 0]
        ),
        0
    );
    assert_eq!(cpu.read_u32_le(0x3024).unwrap(), 1);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_get_param",
            [context, 0x1600, 0x1304, 0]
        ),
        0x80004005
    );
    assert_eq!(cpu.read_u32_le(0x1304).unwrap(), 0);
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_register_param",
            [context, 0x1600, 0x3020, 0]
        ),
        0
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "bind_release",
            [context, 0, 0, 0]
        ),
        0
    );
    assert_eq!(cpu.read_u32_le(0x3024).unwrap(), 1);
    assert!(kernel.ce.com.bind_contexts.is_empty());
    assert!(kernel.ce.com.callbacks.is_empty());
    assert_eq!(kernel.heap.msize(context), None);
    assert_eq!(kernel.heap.msize(vtable), None);
}

#[test]
fn ole_run_executes_arm_and_thumb_query_run_release_and_preserves_the_result() {
    let arm_query: Vec<_> = [0xe59f3008u32, 0xe5823000, 0xe3a00000, 0xe12fff1e, 0x3000]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let thumb_query: Vec<_> = [0x4b01u16, 0x6013, 0x2000, 0x4770]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .chain(0x3000u32.to_le_bytes())
        .collect();
    for thumb in [false, true] {
        for result in [0u32, 0x8004_0112] {
            let (code, entry) = if thumb {
                (&thumb_query, 0x2001)
            } else {
                (&arm_query, 0x2000)
            };
            let (mut cpu, mut k, mut d) = setup(code);
            k.heap = pocket_kernel::Heap::new(0x50000000, 0x10000);
            let (run, release) = if thumb {
                let run: Vec<_> = [0x4a01u16, 0x6011, 0x4801, 0x4770]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .chain(0x3300u32.to_le_bytes())
                    .chain(result.to_le_bytes())
                    .collect();
                let release: Vec<_> = [0x4b02u16, 0x681a, 0x3201, 0x601a, 0x2021, 0x4770]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .chain(0x3304u32.to_le_bytes())
                    .collect();
                (run, release)
            } else {
                let run = [
                    0xe59f2008u32,
                    0xe5821000,
                    0xe59f0004,
                    0xe12fff1e,
                    0x3300,
                    result,
                ]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
                let release = [
                    0xe59f1010u32,
                    0xe5912000,
                    0xe2822001,
                    0xe5812000,
                    0xe3a00021,
                    0xe12fff1e,
                    0x3304,
                ]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
                (run, release)
            };
            cpu.write_mem(0x2240, &run).unwrap();
            cpu.write_mem(0x2280, &release).unwrap();
            let mode = u32::from(thumb);
            cpu.write_mem(0x3000, &0x3100u32.to_le_bytes()).unwrap();
            for (slot, address) in [(0, entry), (2, 0x2280 | mode), (4, 0x2240 | mode)] {
                cpu.write_mem(0x3100 + slot * 4, &address.to_le_bytes())
                    .unwrap();
            }
            cpu.write_mem(0x3300, &99u32.to_le_bytes()).unwrap();
            args(&mut cpu, [0x3000, 11, 22, 33]);
            let mut t = thunk("OleRun");
            t.dll = "ole32.dll".into();
            for address in [entry, 0x2240 | mode, 0x2280 | mode] {
                assert_eq!(
                    d.dispatch(&mut cpu, &t, &mut k).unwrap(),
                    DispatchOutcome::JumpTo(address)
                );
                assert_eq!(
                    cpu.run_until_hook(address, 100).unwrap(),
                    StopReason::Hook(RETURN)
                );
            }
            assert_eq!(
                d.dispatch(&mut cpu, &t, &mut k).unwrap(),
                DispatchOutcome::ReturnedR0(result)
            );
            assert_eq!(cpu.read_u32_le(0x3300).unwrap(), 0); // Run(NULL)
            assert_eq!(cpu.read_u32_le(0x3304).unwrap(), 1); // Release exactly once
            assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 11);
            assert!(k.ce.com.runs.is_empty());
        }
    }
}

fn thunk(name: &str) -> pocket_kernel::Thunk {
    pocket_kernel::Thunk {
        thunk_va: RETURN,
        iat_va: 0,
        dll: "coredll.dll".into(),
        binding: pocket_pe::ImportBinding::Name(name.into()),
        friendly_name: Some(name.into()),
    }
}
fn setup(code: &[u8]) -> (UnicornCpu, pocket_kernel::KernelState, WinCeDispatcher) {
    let mut cpu = UnicornCpu::new().unwrap();
    cpu.map_region(0x1000, 0x4000, Prot::ALL).unwrap();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    cpu.map_region(RETURN, 0x1000, Prot::ALL).unwrap();
    cpu.write_mem(RETURN, &0xe12fff1eu32.to_le_bytes()).unwrap();
    cpu.add_code_hook(RETURN).unwrap();
    cpu.write_mem(0x2000, code).unwrap();
    cpu.write_reg(ArmReg::Sp, 0x4000).unwrap();
    cpu.write_reg(ArmReg::Lr, 0x1234).unwrap();
    (
        cpu,
        crate::gx::tests::fresh_kernel(),
        WinCeDispatcher::new(),
    )
}
fn args(cpu: &mut dyn Cpu, values: [u32; 4]) {
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(values)
    {
        cpu.write_reg(r, v).unwrap();
    }
}
#[test]
fn ce_font_callback_executes_arm_and_thumb_and_restores_original_continuation() {
    let arm = [0xe590c01cu32, 0xe583c000, 0xe3a00000, 0xe12fff1e]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let thumb = [0x69c0u16, 0x6018, 0x2000, 0x4770]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    for (code, entry) in [(&arm, 0x2000), (&thumb, 0x2001)] {
        let (mut cpu, mut kernel, mut dispatcher) = setup(code);
        args(
            &mut cpu,
            [pocket_kernel::gdi::GDI_SCREEN_DC, 0, entry, 0x1300],
        );
        let t = thunk("EnumFontFamiliesW");
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::JumpTo(entry)
        );
        assert_eq!(
            cpu.run_until_hook(entry, 100).unwrap(),
            StopReason::Hook(RETURN)
        );
        // The first callback receives Arial's UTF-16 LOGFONT face name.
        assert_eq!(cpu.read_u32_le(0x1300).unwrap(), 0x00720041);
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert!(kernel.gdi.font_enum_frames.is_empty());
    }
}
#[test]
fn ce_notify_callback_executes_arm_and_thumb_and_ignores_callback_result() {
    let arm = [0xe5831000u32, 0xe3a00000, 0xe12fff1e]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let thumb = [0x6019u16, 0x2000, 0x4770]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    for (code, entry) in [(&arm, 0x2000), (&thumb, 0x2001)] {
        let (mut cpu, mut kernel, mut dispatcher) = setup(code);
        kernel.window_procs.insert(0x111, entry);
        kernel.window_threads.insert(0x111, 0);
        args(&mut cpu, [0x111, 0x400, 0, 0x1300]);
        let t = thunk("SendNotifyMessageW");
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::JumpTo(entry)
        );
        assert_eq!(
            cpu.run_until_hook(entry, 100).unwrap(),
            StopReason::Hook(RETURN)
        );
        assert_eq!(cpu.read_u32_le(0x1300).unwrap(), 0x400);
        assert_eq!(
            dispatcher.dispatch(&mut cpu, &t, &mut kernel).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert!(kernel.notify_message_frames.is_empty());
    }
}
