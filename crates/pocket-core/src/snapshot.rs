//! Experimental persistent checkpoint of an ARM WinCE execution session.

use super::*;
use bincode::Options;
use pocket_cpu::unicorn::{ArmExecutionSnapshot, UnicornCpu};
use pocket_kernel::{guest_clock::GuestClockSnapshot, KernelSnapshot, ResumePoint};
use pocket_winceapi::gles::EglSessionSnapshot;
use serde::Serialize;
use std::io::{BufReader, BufWriter, Read, Write};

const SECTION_MAGIC: &[u8; 8] = b"PHSECT01";
const SECTION_REQUIRED: u16 = 1 << 15;
const SESSION: u16 = SECTION_REQUIRED | 1;
const ARM: u16 = SECTION_REQUIRED | 2;
const KERNEL: u16 = SECTION_REQUIRED | 3;
const EGL_FIXED: u16 = SECTION_REQUIRED | 4;
const MEDIA: u16 = SECTION_REQUIRED | 5;
const EGL_PROGRAMMABLE: u16 = SECTION_REQUIRED | 6;
const EGL_PROGRAMMABLE_VERSION: u16 = 3;
const HSS: u16 = SECTION_REQUIRED | 7;
const INPUT_RELEASES: u16 = SECTION_REQUIRED | 8;
const SECTION_HEADER_SIZE: u64 = 12;
const IO_BUFFER_SIZE: usize = 64 * 1024;
const PAYLOAD_LIMIT: u64 = 512 * 1024 * 1024;

pub struct SessionSnapshot {
    executable: [u8; 32],
    card_program: Option<String>,
    cpu: ArmExecutionSnapshot,
    kernel: KernelSnapshot,
    egl: EglSessionSnapshot,
    programmable: Option<pocket_winceapi::gles2::ProgrammableSnapshot>,
    media: pocket_winceapi::MediaSnapshot,
    hss: Option<pocket_kernel::HssSnapshot>,
    input_releases: Vec<pocket_kernel::InputEvent>,
    clock: GuestClockSnapshot,
    point: ResumePoint,
    thunks: Vec<pocket_kernel::Thunk>,
    thunk_by_va: std::collections::HashMap<u32, usize>,
}

#[derive(Clone, Copy)]
pub struct RestoredSession {
    clock: GuestClockSnapshot,
    point: ResumePoint,
}

impl SessionSnapshot {
    /// Called by a frame hook at an explicitly requested kernel boundary.
    pub fn capture(cpu: &mut dyn Cpu, process: &mut Process, point: ResumePoint) -> Result<Self> {
        Self::capture_process(cpu, process, point, false)
    }

    pub fn capture_foreground(
        cpu: &mut dyn Cpu,
        process: &mut Process,
        point: ResumePoint,
    ) -> Result<Self> {
        Self::capture_process(cpu, process, point, true)
    }

    fn capture_process(
        cpu: &mut dyn Cpu,
        process: &mut Process,
        point: ResumePoint,
        detached: bool,
    ) -> Result<Self> {
        let cpu = (cpu as &mut dyn std::any::Any)
            .downcast_mut::<UnicornCpu>()
            .context("snapshot requires the Unicorn ARM backend")?;
        let media = pocket_winceapi::capture_snapshot_media().map_err(anyhow::Error::msg)?;
        let programmable =
            pocket_winceapi::gles2::capture_snapshot().map_err(anyhow::Error::msg)?;
        let executable = process
            .executable_fingerprint
            .context("missing executable identity")?;
        let clock = GuestClockSnapshot::capture();
        let hss = process.state.hss.capture_snapshot();
        let started = std::time::Instant::now();
        let kernel = process
            .state
            .capture_snapshot_with_suspended_parents(detached)
            .map_err(anyhow::Error::msg)?;
        let kernel_time = started.elapsed();
        let egl = pocket_winceapi::gles::capture_egl_session().map_err(anyhow::Error::msg)?;
        let graphics_time = started.elapsed() - kernel_time;
        let cpu = cpu.capture_arm_execution(point.pc)?;
        log::debug!(
            "Snapshot capture: kernel {kernel_time:?}, EGL {graphics_time:?}, ARM {:?}",
            started.elapsed() - kernel_time - graphics_time
        );
        Ok(Self {
            executable,
            card_program: detached.then(|| process.state.module_path.clone()),
            cpu,
            kernel,
            egl,
            programmable,
            media,
            hss,
            input_releases: process.state.snapshot_input_releases(),
            clock,
            point,
            thunks: process.thunks.clone(),
            thunk_by_va: process.thunk_by_va.clone(),
        })
    }

    pub fn card_program(&self) -> Option<&str> {
        self.card_program.as_deref()
    }

    pub fn write(&self, mut output: impl Write) -> std::io::Result<()> {
        output.write_all(SECTION_MAGIC)?;
        let compressed = flate2::write::ZlibEncoder::new(output, flate2::Compression::fast());
        let mut buffered = BufWriter::with_capacity(IO_BUFFER_SIZE, compressed);
        let count = 5
            + u16::from(self.programmable.is_some())
            + u16::from(self.hss.is_some())
            + u16::from(!self.input_releases.is_empty());
        buffered.write_all(&count.to_le_bytes())?;
        let mut remaining = PAYLOAD_LIMIT;
        write_section(
            &mut buffered,
            &mut remaining,
            SESSION,
            &(
                &self.executable,
                &self.card_program,
                &self.clock,
                &self.point,
                &self.thunks,
                &self.thunk_by_va,
            ),
        )?;
        write_section(&mut buffered, &mut remaining, ARM, &self.cpu)?;
        write_section(&mut buffered, &mut remaining, KERNEL, &self.kernel)?;
        write_section(&mut buffered, &mut remaining, EGL_FIXED, &self.egl)?;
        write_section(&mut buffered, &mut remaining, MEDIA, &self.media)?;
        if let Some(programmable) = &self.programmable {
            write_section(
                &mut buffered,
                &mut remaining,
                EGL_PROGRAMMABLE,
                programmable,
            )?;
        }
        if let Some(hss) = &self.hss {
            write_section(&mut buffered, &mut remaining, HSS, hss)?;
        }
        if !self.input_releases.is_empty() {
            write_section(
                &mut buffered,
                &mut remaining,
                INPUT_RELEASES,
                &self.input_releases,
            )?;
        }
        buffered.into_inner()?.finish()?;
        Ok(())
    }

    pub fn read(mut input: impl Read) -> std::io::Result<Self> {
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        if &magic != SECTION_MAGIC {
            return Err(invalid("unsupported snapshot format"));
        }
        let compressed = flate2::read::ZlibDecoder::new(input);
        let mut buffered = BufReader::with_capacity(IO_BUFFER_SIZE, compressed);
        let snapshot = Self::read_sections(&mut buffered)?;
        if buffered.read(&mut [0; 1])? != 0 {
            return Err(invalid("trailing snapshot data"));
        }
        Ok(snapshot)
    }

    fn read_sections(input: &mut impl Read) -> std::io::Result<Self> {
        let mut count = [0; 2];
        input.read_exact(&mut count)?;
        let mut remaining = PAYLOAD_LIMIT;
        let (mut session, mut cpu, mut kernel, mut egl, mut media) = (None, None, None, None, None);
        let mut programmable = None;
        let mut hss = None;
        let mut input_releases = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..u16::from_le_bytes(count) {
            let mut header = [0; SECTION_HEADER_SIZE as usize];
            input.read_exact(&mut header)?;
            let id = u16::from_le_bytes([header[0], header[1]]);
            let version = u16::from_le_bytes([header[2], header[3]]);
            let len = u64::from_le_bytes(header[4..].try_into().unwrap());
            consume_budget(&mut remaining, len)?;
            if !seen.insert(id & !SECTION_REQUIRED) {
                return Err(invalid("duplicate snapshot section"));
            }
            let mut section = input.take(len);
            match (id, version) {
                (SESSION, 1) => session = Some(read_section(&mut section)?),
                (ARM, 1) => cpu = Some(read_section(&mut section)?),
                (KERNEL, 1) => kernel = Some(read_section(&mut section)?),
                (EGL_FIXED, 1) => egl = Some(read_section(&mut section)?),
                (MEDIA, 1) => media = Some(read_section(&mut section)?),
                (EGL_PROGRAMMABLE, EGL_PROGRAMMABLE_VERSION) => {
                    programmable = Some(read_section(&mut section)?)
                }
                (HSS, 1) => hss = Some(read_section(&mut section)?),
                (INPUT_RELEASES, 1) => input_releases = read_section(&mut section)?,
                _ if id & SECTION_REQUIRED == 0 => {
                    std::io::copy(&mut section, &mut std::io::sink())?;
                }
                _ => {
                    return Err(invalid(format!(
                        "unsupported required snapshot section {} version {version}",
                        id & !SECTION_REQUIRED
                    )))
                }
            }
            if section.limit() != 0 {
                return Err(invalid("snapshot section length mismatch"));
            }
        }
        let (executable, card_program, clock, point, thunks, thunk_by_va) =
            session.ok_or_else(|| invalid("missing session section"))?;
        Ok(Self {
            executable,
            card_program,
            clock,
            point,
            thunks,
            thunk_by_va,
            cpu: cpu.ok_or_else(|| invalid("missing ARM section"))?,
            kernel: kernel.ok_or_else(|| invalid("missing WinCE kernel section"))?,
            egl: egl.ok_or_else(|| invalid("missing fixed-function EGL section"))?,
            programmable,
            media: media.ok_or_else(|| invalid("missing media section"))?,
            hss,
            input_releases,
        })
    }
}

fn invalid(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

fn consume_budget(remaining: &mut u64, len: u64) -> std::io::Result<()> {
    *remaining = remaining
        .checked_sub(SECTION_HEADER_SIZE)
        .and_then(|budget| budget.checked_sub(len))
        .ok_or_else(|| invalid("snapshot sections exceed the size limit"))?;
    Ok(())
}

fn write_section(
    output: &mut impl Write,
    remaining: &mut u64,
    id: u16,
    value: &impl Serialize,
) -> std::io::Result<()> {
    let len = codec()
        .serialized_size(value)
        .map_err(std::io::Error::other)?;
    consume_budget(remaining, len)?;
    output.write_all(&id.to_le_bytes())?;
    let version = if id == EGL_PROGRAMMABLE {
        EGL_PROGRAMMABLE_VERSION
    } else {
        1
    };
    output.write_all(&version.to_le_bytes())?;
    output.write_all(&len.to_le_bytes())?;
    codec()
        .with_no_limit()
        .serialize_into(output, value)
        .map_err(std::io::Error::other)
}

fn read_section<T: serde::de::DeserializeOwned>(
    section: &mut std::io::Take<impl Read>,
) -> std::io::Result<T> {
    codec()
        .with_limit(section.limit())
        .deserialize_from(section)
        .map_err(std::io::Error::other)
}

impl Emulator {
    /// Restore in a fresh, normally configured launch. The old session is
    /// separate from this emulator. Discard this launch on any failure.
    pub fn restore_snapshot(&mut self, saved: SessionSnapshot) -> Result<RestoredSession> {
        let process = self
            .process
            .as_mut()
            .context("load the saved executable before restoring")?;
        anyhow::ensure!(
            process.executable_fingerprint == Some(saved.executable),
            "snapshot is for a different executable"
        );
        anyhow::ensure!(
            saved
                .thunk_by_va
                .values()
                .all(|&index| index < saved.thunks.len()),
            "invalid snapshot thunk table"
        );

        // Release resident bootstrap image pages before charging the restored
        // image and heap to the configured guest RAM pool.
        self.cpu = Box::new(StubCpu::new());
        saved
            .kernel
            .restore(&mut process.state)
            .map_err(anyhow::Error::msg)?;
        let (cpu, pc) = saved.cpu.restore(process.state.heap.image_page_budget())?;
        anyhow::ensure!(pc == saved.point.pc, "snapshot continuation mismatch");
        saved.egl.restore().map_err(anyhow::Error::msg)?;
        pocket_winceapi::gles2::restore_snapshot(saved.programmable).map_err(anyhow::Error::msg)?;
        saved.media.restore().map_err(anyhow::Error::msg)?;
        if let Some(hss) = saved.hss {
            process.state.hss = hss.restore().map_err(anyhow::Error::msg)?;
        }
        process
            .state
            .restore_snapshot_input(saved.input_releases)
            .map_err(anyhow::Error::msg)?;
        process.thunks = saved.thunks;
        process.thunk_by_va = saved.thunk_by_va;
        self.cpu = Box::new(cpu);
        Ok(RestoredSession {
            clock: saved.clock,
            point: saved.point,
        })
    }

    pub fn run_restored(
        &mut self,
        restored: RestoredSession,
        hook: &mut dyn FrameHook,
    ) -> Result<()> {
        let process = self.process.as_mut().context("no restored process")?;
        restored
            .clock
            .run(|| {
                pocket_kernel::resume_main_loop(
                    self.cpu.as_mut(),
                    process,
                    &mut self.dispatcher,
                    self.instruction_budget_per_slice,
                    self.max_slices,
                    Some(hook),
                    restored.point,
                )
            })
            .context("restored emulator loop")
    }
}

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(PAYLOAD_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::regs::ArmReg;
    use pocket_kernel::{FrameAction, KernelState};
    use pocket_pe::{LoadedImage, LoadedSection};
    use sha2::{Digest, Sha256};

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

    #[test]
    fn gles2_section_is_present_only_for_programmable_sessions() {
        let mut emulator = fixture(3);
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: 0x11000,
        };
        for enabled in [false, true] {
            pocket_winceapi::gles2::set_enabled(enabled);
            let saved = SessionSnapshot::capture(
                emulator.cpu.as_mut(),
                emulator.process.as_mut().unwrap(),
                point,
            )
            .unwrap();
            let mut bytes = Vec::new();
            saved.write(&mut bytes).unwrap();
            let saved = SessionSnapshot::read(bytes.as_slice()).unwrap();
            assert_eq!(saved.programmable.is_some(), enabled);
        }
        pocket_winceapi::gles2::set_enabled(false);
    }

    #[test]
    fn unknown_required_sections_are_rejected_and_optional_sections_are_skipped() {
        let section = |id: u16, len: u64, body: &[u8]| {
            let mut bytes = 1u16.to_le_bytes().to_vec();
            bytes.extend(id.to_le_bytes());
            bytes.extend(2u16.to_le_bytes());
            bytes.extend(len.to_le_bytes());
            bytes.extend(body);
            bytes
        };
        let error = |bytes: Vec<u8>| {
            SessionSnapshot::read_sections(&mut bytes.as_slice())
                .err()
                .unwrap()
                .to_string()
        };
        assert_eq!(error(section(35, 3, b"abc")), "missing session section");
        assert_eq!(
            error(section(SECTION_REQUIRED | 35, 3, b"abc")),
            "unsupported required snapshot section 35 version 2"
        );
        assert_eq!(
            error(section(35, 4, b"abc")),
            "snapshot section length mismatch"
        );
        assert_eq!(
            error(section(35, PAYLOAD_LIMIT, b"")),
            "snapshot sections exceed the size limit"
        );
    }

    #[test]
    fn persisted_snapshot_restores_a_cold_session_and_resumes_guest_code() {
        let mut source = fixture(3);
        source.cpu.write_reg(ArmReg::R0, 40).unwrap();
        let state = &mut source.process.as_mut().unwrap().state;
        state.random_seed = 12345;
        state.held_keys.push(0x27);
        state.pressed_keys[0x27] = true;
        state.pressed_keys[1] = true;
        state.controls.cursor_position = (23, 42);
        state
            .pending_input
            .push_back(pocket_kernel::InputEvent::KeyDown { vk: 0x28 });
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: 0x11004,
        };
        let checkpoint =
            SessionSnapshot::capture(source.cpu.as_mut(), source.process.as_mut().unwrap(), point)
                .unwrap();
        assert!(source.process.as_ref().unwrap().state.pressed_keys[1]);
        let mut file = Vec::new();
        checkpoint.write(&mut file).unwrap();

        drop(checkpoint);
        drop(source);

        let mut different = fixture(7);
        let mismatch = different.restore_snapshot(SessionSnapshot::read(file.as_slice()).unwrap());
        assert!(mismatch.is_err());
        drop(different);

        let mut destination = fixture(3);
        let restored = destination
            .restore_snapshot(SessionSnapshot::read(file.as_slice()).unwrap())
            .unwrap();
        assert_eq!(
            destination.process.as_ref().unwrap().state.random_seed,
            12345
        );
        let state = &destination.process.as_ref().unwrap().state;
        assert!(state.held_keys.is_empty());
        assert!(!state.async_keys.is_down(0x27));
        assert_eq!(
            state.pending_input.iter().copied().collect::<Vec<_>>(),
            [
                pocket_kernel::InputEvent::KeyUp { vk: 0x27 },
                pocket_kernel::InputEvent::PointerUp { x: 23, y: 42 },
            ],
        );
        destination
            .run_restored(restored, &mut |_state: &mut KernelState| {
                FrameAction::Continue
            })
            .unwrap();
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
}
