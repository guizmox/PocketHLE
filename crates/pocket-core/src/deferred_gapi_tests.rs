//! Native CPU + GLES regression, separate from game assets and visual tests.
use super::*;
use pocket_cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu};
use pocket_kernel::{DispatchOutcome, Dispatcher, Thunk};
use pocket_pe::{ImportBinding, LoadedImage, LoadedSection};
use std::sync::{atomic::AtomicBool, Arc};
fn api(
    d: &mut WinCeDispatcher,
    cpu: &mut dyn Cpu,
    k: &mut pocket_kernel::KernelState,
    dll: &str,
    name: &str,
    args: &[u32],
) -> u32 {
    for (i, reg) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .enumerate()
    {
        cpu.write_reg(reg, args.get(i).copied().unwrap_or(0))
            .unwrap();
    }
    let thunk = Thunk {
        thunk_va: 0x70000000,
        iat_va: 0,
        dll: dll.into(),
        binding: ImportBinding::Name(name.into()),
        friendly_name: None,
    };
    match d.dispatch(cpu, &thunk, k).unwrap() {
        DispatchOutcome::ReturnedR0(v) => v,
        other => panic!("{name}: {other:?}"),
    }
}
fn image() -> LoadedImage {
    LoadedImage {
        source_path: "deferred-memory-test".into(),
        machine: pe::machine::ARM,
        subsystem: pe::subsystem::WINDOWS_CE_GUI,
        subsystem_version: (4, 0),
        image_base: 0x10000,
        stack_reserve: 0x10000,
        size_of_image: 0x2000,
        entry_point: 0x1000,
        sections: vec![LoadedSection {
            name: ".text".into(),
            virtual_address: 0x1000,
            virtual_size: 0x1000,
            characteristics: 0x60000020,
            data: vec![0; 4096],
        }],
        imports: vec![],
        exports: Default::default(),
        resources: vec![],
        managed_runtime: None,
    }
}
#[test]
#[ignore = "requires native shared GL and Unicorn"]
fn native_gapi_swap_defers_real_arm_and_hle_access_and_preserves_gdi_changes() {
    for columns in [false, true] {
        for scale in [0, 5] {
            let ui = pocket_gles::gpu::Renderer::new(8, 8).unwrap();
            let source = ui.shared_context_source();
            let mut cpu = UnicornCpu::new().unwrap();
            let mut d = WinCeDispatcher::new();
            let mut process = Process::map_into(image(), &mut cpu, &|_, _| None, &d).unwrap();
            let k = &mut process.state;
            k.framebuffer.resize(3, 2);
            k.gapi_column_major = columns;
            pocket_winceapi::gles::with_shared_context_source(Some(source), || {
                pocket_winceapi::gles::with_shared_frame_export(
                    Arc::new(AtomicBool::new(true)),
                    || {
                        pocket_winceapi::gles::set_internal_resolution(scale);
                        let display =
                            api(&mut d, &mut cpu, k, "libGLES_CM.dll", "eglGetDisplay", &[0]);
                        assert_eq!(
                            api(
                                &mut d,
                                &mut cpu,
                                k,
                                "libGLES_CM.dll",
                                "eglInitialize",
                                &[display, 0, 0]
                            ),
                            1
                        );
                        let config = 0x45470002;
                        let surface = api(
                            &mut d,
                            &mut cpu,
                            k,
                            "libGLES_CM.dll",
                            "eglCreateWindowSurface",
                            &[display, config, 1, 0],
                        );
                        let context = api(
                            &mut d,
                            &mut cpu,
                            k,
                            "libGLES_CM.dll",
                            "eglCreateContext",
                            &[display, config, 0, 0],
                        );
                        assert_eq!(
                            api(
                                &mut d,
                                &mut cpu,
                                k,
                                "libGLES_CM.dll",
                                "eglMakeCurrent",
                                &[display, surface, surface, context]
                            ),
                            1
                        );
                        pocket_winceapi::gles::set_gpu_renderer(true);
                        assert_eq!(
                            api(
                                &mut d,
                                &mut cpu,
                                k,
                                "gx.dll",
                                "?GXOpenDisplay@@YAHPAUHWND__@@K@Z",
                                &[1, 0]
                            ),
                            1
                        );
                        for color in [[65536, 0, 0, 65536], [0, 65536, 0, 65536]] {
                            api(
                                &mut d,
                                &mut cpu,
                                k,
                                "libGLES_CM.dll",
                                "glClearColorx",
                                &color,
                            );
                            api(&mut d, &mut cpu, k, "libGLES_CM.dll", "glClear", &[0x4000]);
                            k.framebuffer.directdraw.last_present = None;
                            k.framebuffer.directdraw.pending.clear();
                            assert_eq!(
                                api(
                                    &mut d,
                                    &mut cpu,
                                    k,
                                    "libGLES_CM.dll",
                                    "eglSwapBuffers",
                                    &[display, surface]
                                ),
                                1
                            );
                            assert!(cpu.deferred_memory_pending(
                                pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE
                            ));
                            assert!(!k.framebuffer.cpu_resident());
                            assert!(pocket_winceapi::gles::take_shared_window_frame(
                                &k.framebuffer
                            )
                            .is_some());
                            // No-op window completion still cannot force materialization.
                            k.present_window_surfaces();
                            assert!(cpu.deferred_memory_pending(
                                pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE
                            ));
                            assert!(!k.framebuffer.cpu_resident());
                            if color[0] == 65536 {
                                assert_eq!(
                                    cpu.read_mem(pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE, 2)
                                        .unwrap(),
                                    [0, 0xf8]
                                );
                                assert!(!cpu.deferred_memory_pending(
                                    pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE
                                ));
                                assert!(k.framebuffer.gpu_source().is_some());
                            }
                        }
                        // ARM LDR through a pointer retained across both swaps sees green.
                        cpu.write_mem(
                            0x11040,
                            &[0xe5901000u32.to_le_bytes(), 0xe12fff1eu32.to_le_bytes()].concat(),
                        )
                        .unwrap();
                        cpu.add_code_hook(0x11080).unwrap();
                        cpu.write_reg(ArmReg::R0, pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE)
                            .unwrap();
                        cpu.write_reg(ArmReg::Lr, 0x11080).unwrap();
                        cpu.run_until_hook(0x11040, 100).unwrap();
                        assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 0x07e007e0);
                        assert!(
                            !cpu.deferred_memory_pending(pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE)
                        );
                        assert!(k.framebuffer.gpu_source().is_some());
                        // Another swap, followed by a real GDI write before guest resume.
                        api(
                            &mut d,
                            &mut cpu,
                            k,
                            "libGLES_CM.dll",
                            "glClearColorx",
                            &[65536, 0, 0, 65536],
                        );
                        api(&mut d, &mut cpu, k, "libGLES_CM.dll", "glClear", &[0x4000]);
                        k.framebuffer.directdraw.last_present = None;
                        k.framebuffer.directdraw.pending.clear();
                        api(
                            &mut d,
                            &mut cpu,
                            k,
                            "libGLES_CM.dll",
                            "eglSwapBuffers",
                            &[display, surface],
                        );
                        k.framebuffer.fill(0x001f);
                        assert!(k.framebuffer.gpu_source().is_none());
                    },
                );
            });
            // A one-slice real kernel resume must mirror the blue CPU-authoritative
            // image before the ARM load; an old lazy red snapshot must not win.
            cpu.write_mem(
                0x11000,
                &[
                    0xe59f0004u32.to_le_bytes(),
                    0xe5901000u32.to_le_bytes(),
                    0xe12fff1eu32.to_le_bytes(),
                    pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE.to_le_bytes(),
                ]
                .concat(),
            )
            .unwrap();
            pocket_kernel::run_main_loop(&mut cpu, &mut process, &mut d, 1000, 1).unwrap();
            assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 0x001f001f);
            assert!(!cpu.deferred_memory_pending(pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE));
            assert_eq!(
                cpu.read_mem(pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE, 4)
                    .unwrap(),
                [0x1f, 0, 0x1f, 0]
            );
        }
    }
}
