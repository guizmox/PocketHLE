//! Reproduce live waveOut mixer starvation with actual ARM CPU-only code.
#![cfg(feature = "unicorn")]

use pocket_core::{
    cpu::{regs::ArmReg, unicorn::UnicornCpu, Cpu},
    kernel::{self, Dispatcher, GuestThread, KernelError, KernelState, Process, Thunk},
    pe::{self, ImportBinding, ImportSymbol, LoadedImage, LoadedSection},
    winceapi::WinCeDispatcher,
};

struct ApiOnlyDispatcher(WinCeDispatcher);
impl Dispatcher for ApiOnlyDispatcher {
    fn dispatch(
        &mut self,
        cpu: &mut dyn Cpu,
        thunk: &Thunk,
        kernel: &mut KernelState,
    ) -> Result<kernel::DispatchOutcome, KernelError> {
        self.0.dispatch(cpu, thunk, kernel)
    }
}

fn run_mixer(dispatcher: &mut dyn Dispatcher, thumb: bool) -> [i16; 4] {
    const DATA: u32 = 0x0050_0000;
    const HANDLE: u32 = 0xdead4100;
    let mut text = vec![0; 0x1000];
    // Main sets two sentinels and flags, then unloads indefinitely without APIs.
    for (index, instruction) in [0xe3a05055u32, 0xe3a06066, 0xe1550006, 0xeafffffe]
        .into_iter()
        .enumerate()
    {
        text[index * 4..index * 4 + 4].copy_from_slice(&instruction.to_le_bytes());
    }
    if thumb {
        for (index, instruction) in [0x2555u16, 0x2666, 0x42b5, 0xe7fe].into_iter().enumerate() {
            text[index * 2..index * 2 + 2].copy_from_slice(&instruction.to_le_bytes());
        }
    }
    let image = LoadedImage {
        source_path: "audio_cpu_slice_regression".into(),
        machine: if thumb {
            pe::machine::THUMB
        } else {
            pe::machine::ARM
        },
        subsystem: pe::subsystem::WINDOWS_CE_GUI,
        image_base: 0x10000,
        size_of_image: 0x2000,
        stack_reserve: 0x10000,
        entry_point: 0x1000,
        sections: vec![LoadedSection {
            name: ".text".into(),
            virtual_address: 0x1000,
            virtual_size: 0x1000,
            characteristics: 0xe0000020,
            data: text,
        }],
        imports: vec![ImportSymbol {
            dll: "coredll.dll".into(),
            binding: ImportBinding::Name("Sleep".into()),
            iat_va: 0x11800,
        }],
        exports: Default::default(),
        resources: vec![],
        managed_runtime: None,
    };
    let mut cpu = UnicornCpu::new().unwrap();
    let mut process = Process::map_into(image, &mut cpu, &|_, _| None, dispatcher).unwrap();
    let sleep = process.thunks[0].thunk_va;
    // Mixer clears the ring, publishes it through Sleep(1), then idles.
    let code = [
        0xe59f1018u32, // ldr r1, [pc, #24]
        0xe3a00000,    // mov r0, #0
        0xe5810000,    // str r0, [r1]
        0xe5810004,    // str r0, [r1, #4]
        0xe59fc00c,    // ldr ip, [pc, #12]
        0xe3a00001,    // mov r0, #1
        0xe12fff3c,    // blx ip
        0xeafffffc,    // b back to mov r0, #1 / Sleep
        DATA,
        sleep,
    ];
    let bytes: Vec<_> = code.into_iter().flat_map(u32::to_le_bytes).collect();
    cpu.write_mem(0x11100, &bytes).unwrap();
    cpu.write_mem(DATA, &[0xe8, 3, 0xe8, 3, 0xe8, 3, 0xe8, 3])
        .unwrap();
    let format = kernel::audio::GuestFormat {
        sample_rate: 48000,
        channels: 1,
        bits_per_sample: 16,
    };
    process.state.audio.open_wave_stream(HANDLE, format);
    assert!(process
        .state
        .audio
        .queue_wave_loop(HANDLE, vec![1000; 4], u32::MAX));
    process.state.wave_out.devices.insert(
        HANDLE,
        kernel::WaveOutDevice {
            format,
            callback_kind: kernel::WaveCallbackKind::None,
            callback_target: 0,
            instance: 0,
            owner_thread: 0,
            paused: false,
        },
    );
    process.state.wave_out.live_loops.insert(
        HANDLE,
        kernel::LiveWaveLoop {
            hdr: DATA + 0x100,
            data: DATA,
            bytes: 8,
        },
    );
    let mut worker = GuestThread::new(
        0x11100,
        0,
        process.stack_top - 0x1000,
        0x1000,
        0,
        0,
        0xdead7c00,
        [0; 17],
    );
    worker.started = true;
    worker.worker_saved = true;
    worker.worker_regs[13] = worker.stack_top;
    worker.worker_regs[15] = worker.entry;
    worker.worker_regs[16] = 0x10;
    process.state.threads.push(worker);
    let tap = process.state.audio.tap();
    let mut out = [0; 4];
    tap.drain_into(&mut out);
    assert_eq!(out, [1000; 4]);
    kernel::run_main_loop(&mut cpu, &mut process, dispatcher, 100, 2).unwrap();
    assert_eq!(process.state.current_thread, 0);
    assert_eq!(cpu.read_reg(ArmReg::R5).unwrap(), 0x55);
    assert_eq!(cpu.read_reg(ArmReg::R6).unwrap(), 0x66);
    assert_eq!(
        cpu.read_reg(ArmReg::Pc).unwrap(),
        if thumb { 0x11006 } else { 0x1100c }
    );
    assert_eq!(
        cpu.read_reg(ArmReg::Cpsr).unwrap() & 0x20,
        if thumb { 0x20 } else { 0 }
    );
    assert_eq!(cpu.read_reg(ArmReg::Cpsr).unwrap() & 0xf0000000, 0x80000000);
    tap.drain_into(&mut out);
    out
}

#[test]
fn native_arm_audio_mixer_runs_during_main_cpu_only_unload() {
    // The old API-only policy leaves the last buffer playing indefinitely.
    assert_eq!(
        run_mixer(&mut ApiOnlyDispatcher(WinCeDispatcher::new()), false),
        [1000; 4]
    );
    // Slice scheduling lets the mixer publish silence, preserving main state.
    assert_eq!(run_mixer(&mut WinCeDispatcher::new(), false), [0; 4]);
}

#[test]
fn native_thumb_audio_mixer_preserves_main_mode_and_flags() {
    assert_eq!(run_mixer(&mut WinCeDispatcher::new(), true), [0; 4]);
}
