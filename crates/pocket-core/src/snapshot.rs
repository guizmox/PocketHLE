//! Experimental persistent checkpoint of an ARM WinCE execution session.

use super::*;
use bincode::Options;
use pocket_cpu::unicorn::{ArmExecutionSnapshot, UnicornCpu};
use pocket_kernel::{guest_clock::GuestClockSnapshot, KernelSnapshot, ResumePoint};
use pocket_snapshot::{codec, PAYLOAD_LIMIT};
use pocket_winceapi::gles::EglSessionSnapshot;
use serde::Serialize;
use std::io::{Read, Write};

#[path = "device_snapshot.rs"]
mod device;
pub use device::{DeviceArchive, DeviceRestoreGroup, ProcessCheckpoint};

const SECTION_MAGIC: &[u8; 8] = b"PHSECT01";
const SECTION_REQUIRED: u16 = 1 << 15;
const SESSION: u16 = SECTION_REQUIRED | 1;
const ARM: u16 = SECTION_REQUIRED | 2;
const KERNEL: u16 = SECTION_REQUIRED | 3;
const KERNEL_VERSION: u16 = 12;
const EGL_FIXED: u16 = SECTION_REQUIRED | 4;
const EGL_FIXED_VERSION: u16 = 2;
const MEDIA: u16 = SECTION_REQUIRED | 5;
const EGL_PROGRAMMABLE: u16 = SECTION_REQUIRED | 6;
const EGL_PROGRAMMABLE_VERSION: u16 = 6;
const HSS: u16 = SECTION_REQUIRED | 7;
const INPUT_RELEASES: u16 = SECTION_REQUIRED | 8;
const DEVICE: u16 = SECTION_REQUIRED | 9;
const SECTION_HEADER_SIZE: u64 = 12;

pub struct SessionSnapshot {
    device: Option<DeviceArchive>,
    executable: [u8; 32],
    environment: std::collections::BTreeMap<String, String>,
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
    pub(super) resumed: Option<std::time::Instant>,
    clock: GuestClockSnapshot,
    point: ResumePoint,
}

impl SessionSnapshot {
    /// Called by a frame hook at an explicitly requested kernel boundary.
    pub fn capture(cpu: &mut dyn Cpu, process: &mut Process, point: ResumePoint) -> Result<Self> {
        Self::capture_process(
            cpu,
            process,
            point,
            false,
            crate::checkpoint_domain::CheckpointTime::now(),
        )
    }

    fn capture_process(
        cpu: &mut dyn Cpu,
        process: &mut Process,
        point: ResumePoint,
        coordinated: bool,
        at: crate::checkpoint_domain::CheckpointTime,
    ) -> Result<Self> {
        let cpu = (cpu as &mut dyn std::any::Any)
            .downcast_mut::<UnicornCpu>()
            .context("snapshot requires the Unicorn ARM backend")?;
        let media = pocket_winceapi::capture_snapshot_media(at.host).map_err(anyhow::Error::msg)?;
        let programmable =
            pocket_winceapi::gles2::capture_snapshot().map_err(anyhow::Error::msg)?;
        let executable = process
            .executable_fingerprint
            .context("missing executable identity")?;
        let clock = at.clock;
        let hss = process.state.hss.capture_snapshot();
        let started = std::time::Instant::now();
        let kernel = if coordinated {
            process.state.capture_device_process(at.host)
        } else {
            process.state.capture_checkpoint_at(at.host)
        }
        .map_err(anyhow::Error::msg)?;
        let kernel_time = started.elapsed();
        let egl = pocket_winceapi::gles::capture_egl_session().map_err(anyhow::Error::msg)?;
        let graphics_time = started.elapsed() - kernel_time;
        let cpu = cpu.capture_arm_execution_with_mappings(
            point.pc,
            &process.state.mapped_views.snapshot_ranges(),
        )?;
        log::debug!(
            "Snapshot capture: kernel {kernel_time:?}, EGL {graphics_time:?}, ARM {:?}",
            started.elapsed() - kernel_time - graphics_time
        );
        Ok(Self {
            device: None,
            environment: Default::default(),
            executable,
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

    pub fn write(&self, output: impl Write) -> std::io::Result<()> {
        pocket_snapshot::write_stream(output, SECTION_MAGIC, |out| self.write_sections(out))
    }

    fn write_sections(&self, mut buffered: &mut (impl Write + ?Sized)) -> std::io::Result<()> {
        let count = 5
            + u16::from(self.programmable.is_some())
            + u16::from(self.hss.is_some())
            + u16::from(!self.input_releases.is_empty())
            + u16::from(self.device.is_some());
        buffered.write_all(&count.to_le_bytes())?;
        let mut remaining = PAYLOAD_LIMIT;
        write_section(
            &mut buffered,
            &mut remaining,
            SESSION,
            &(
                &self.executable,
                &self.environment,
                &self.clock,
                &self.point,
                &self.thunks,
                &self.thunk_by_va,
            ),
        )?;
        write_section(&mut buffered, &mut remaining, ARM, &self.cpu)?;
        let kernel = pocket_snapshot::fields::Plan::new(&self.kernel)?;
        consume_budget(&mut remaining, kernel.encoded_len())?;
        buffered.write_all(&KERNEL.to_le_bytes())?;
        buffered.write_all(&KERNEL_VERSION.to_le_bytes())?;
        buffered.write_all(&kernel.encoded_len().to_le_bytes())?;
        kernel.write(&mut buffered)?;
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
        if let Some(device) = &self.device {
            write_section(&mut buffered, &mut remaining, DEVICE, device)?;
        }
        Ok(())
    }

    pub fn read(input: impl Read) -> std::io::Result<Self> {
        pocket_snapshot::read_stream(input, SECTION_MAGIC, |input| Self::read_sections(input))
    }

    fn read_sections(input: &mut (impl Read + ?Sized)) -> std::io::Result<Self> {
        Self::read_sections_mode(input, true)
    }

    fn read_sections_mode(
        input: &mut (impl Read + ?Sized),
        allow_device: bool,
    ) -> std::io::Result<Self> {
        let mut count = [0; 2];
        input.read_exact(&mut count)?;
        let mut remaining = PAYLOAD_LIMIT;
        let (mut session, mut cpu, mut kernel, mut egl, mut media) = (None, None, None, None, None);
        let mut programmable = None;
        let mut device = None;
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
                (SESSION, 3) => session = Some(read_section(&mut section)?),
                (ARM, 1) => cpu = Some(read_section(&mut section)?),
                (KERNEL, KERNEL_VERSION) => {
                    use pocket_snapshot::fields::FieldSet;
                    kernel = Some(KernelSnapshot::read_archive(&mut section)?);
                }
                (KERNEL, _) => {
                    return Err(invalid(format!(
                        "unsupported WinCE snapshot container {version}; create a new snapshot"
                    )))
                }
                (EGL_FIXED, EGL_FIXED_VERSION) => egl = Some(read_section(&mut section)?),
                (MEDIA, 2) => media = Some(read_section(&mut section)?),
                (EGL_PROGRAMMABLE, EGL_PROGRAMMABLE_VERSION) => {
                    programmable = Some(read_section(&mut section)?)
                }
                (DEVICE, 1) if allow_device => device = Some(read_section(&mut section)?),
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
        let (executable, environment, clock, point, thunks, thunk_by_va) =
            session.ok_or_else(|| invalid("missing session section"))?;
        let snapshot = Self {
            device,
            executable,
            environment,
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
        };
        let pc = snapshot
            .cpu
            .validate()
            .map_err(|e| invalid(e.to_string()))?;
        if pc != snapshot.point.pc {
            return Err(invalid("snapshot continuation mismatch"));
        }
        if snapshot
            .thunk_by_va
            .values()
            .any(|&index| index >= snapshot.thunks.len())
        {
            return Err(invalid("invalid snapshot thunk table"));
        }
        if allow_device && snapshot.kernel.is_device_process() != snapshot.device.is_some() {
            return Err(invalid("process checkpoint is missing its device domain"));
        }
        if let Some(device) = &snapshot.device {
            device.validate(&snapshot).map_err(invalid)?;
        }
        Ok(snapshot)
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
    output: &mut (impl Write + ?Sized),
    remaining: &mut u64,
    id: u16,
    value: &impl Serialize,
) -> std::io::Result<()> {
    let len = codec()
        .serialized_size(value)
        .map_err(std::io::Error::other)?;
    consume_budget(remaining, len)?;
    output.write_all(&id.to_le_bytes())?;
    let version = if id == EGL_FIXED {
        EGL_FIXED_VERSION
    } else if id == EGL_PROGRAMMABLE {
        EGL_PROGRAMMABLE_VERSION
    } else if id == SESSION {
        3
    } else if id == MEDIA {
        2
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
        anyhow::ensure!(
            !saved.has_device() && !saved.kernel.is_device_process(),
            "device checkpoint requires the coordinated restore path"
        );
        let temporary_files = saved
            .kernel
            .prepare_temporary_files(
                &mut self
                    .process
                    .as_mut()
                    .context("missing restore process")?
                    .state
                    .vfs,
            )
            .map_err(anyhow::Error::msg)?;
        let (restored, contents) = self.restore_snapshot_parts(saved, None)?;
        let state = &mut self
            .process
            .as_mut()
            .context("missing restored process")?
            .state;
        state
            .vfs
            .finish_snapshot_restore()
            .map_err(anyhow::Error::msg)?;
        let files = state
            .vfs
            .install_snapshot_files()
            .map_err(anyhow::Error::msg)?;
        if let Some(contents) = contents {
            contents.install().map_err(anyhow::Error::msg)?;
        }
        files.commit();
        temporary_files.commit();
        Ok(restored)
    }

    fn restore_snapshot_parts(
        &mut self,
        saved: SessionSnapshot,
        domain: Option<&pocket_kernel::DeviceRestoreDomain>,
    ) -> Result<(
        RestoredSession,
        Option<pocket_kernel::file_mapping::MappingContents>,
    )> {
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
        if let Some(domain) = domain {
            domain.prepare_process(&mut process.state);
        }
        let mappings = saved
            .kernel
            .restore_with_domain(&mut process.state, domain)
            .map_err(anyhow::Error::msg)?;
        let (mut cpu, pc) = saved.cpu.restore(process.state.heap.image_page_budget())?;
        anyhow::ensure!(pc == saved.point.pc, "snapshot continuation mismatch");
        let contents = mappings
            .map(|mappings| {
                mappings.restore_with_domain(
                    &mut process.state,
                    &mut cpu,
                    domain.map(|domain| &domain.sections),
                )
            })
            .transpose()
            .map_err(anyhow::Error::msg)?;
        saved.egl.restore().map_err(anyhow::Error::msg)?;
        pocket_winceapi::gles2::restore_snapshot(saved.programmable).map_err(anyhow::Error::msg)?;
        saved
            .media
            .restore(&mut process.state)
            .map_err(anyhow::Error::msg)?;
        if let Some(hss) = saved.hss {
            process.state.hss = hss.restore().map_err(anyhow::Error::msg)?;
        }
        process
            .state
            .restore_snapshot_input(saved.input_releases)
            .map_err(anyhow::Error::msg)?;
        if process.state.sdl.key_ptr != 0 {
            cpu.write_mem(process.state.sdl.key_ptr, &process.state.sdl.keys)?;
        }
        // Host presentation pacing is a current preference, not guest state.
        // Retain saved explicit waits and their remaining time.
        process.state.framebuffer.directdraw.frame_rate_limit = self.requested_frame_rate_limit;
        process.thunks = saved.thunks;
        process.thunk_by_va = saved.thunk_by_va;
        self.cpu = Box::new(cpu);
        Ok((
            RestoredSession {
                resumed: None,
                clock: saved.clock,
                point: saved.point,
            },
            contents,
        ))
    }

    pub fn run_restored(
        &mut self,
        restored: RestoredSession,
        hook: &mut dyn FrameHook,
    ) -> Result<()> {
        let process = self.process.as_mut().context("no restored process")?;
        restored
            .clock
            .run_at(
                restored.resumed.unwrap_or_else(std::time::Instant::now),
                || {
                    // A restored synchronous launcher must not execute even one
                    // guest instruction before its saved foreground child exits.
                    if process.state.suspended_process_launch.is_some() {
                        hook.on_checkpoint(self.cpu.as_mut(), process, restored.point);
                        anyhow::ensure!(
                            process.state.suspended_process_launch.is_none()
                                || process.state.should_stop,
                            "frontend did not resume the suspended child-process wait"
                        );
                    }
                    if process.state.should_stop {
                        return Ok(());
                    }
                    pocket_kernel::resume_main_loop(
                        self.cpu.as_mut(),
                        process,
                        &mut self.dispatcher,
                        self.instruction_budget_per_slice,
                        self.max_slices,
                        Some(hook),
                        restored.point,
                    )
                    .map_err(anyhow::Error::from)
                },
            )
            .context("restored emulator loop")
    }
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
    fn device_checkpoint_cold_restores_shared_sections_and_wait_objects() {
        use pocket_kernel::{file_mapping::FileMapping, handles::HandleObject, EventObject};
        use std::sync::{Arc, Barrier};
        let mut parent = fixture(3);
        let mut child = fixture(3);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("open.bin");
        std::fs::write(&path, b"saved").unwrap();
        let state = &mut parent.process.as_mut().unwrap().state;
        state.vfs.mount_save_dir(r"\Data\", directory.path());
        state
            .vfs
            .open_file(r"\Data\open.bin", 3, 3, 3, false, 0)
            .unwrap();
        let (child_handle, _) = state.object_handles.new_child().unwrap();
        child
            .process
            .as_mut()
            .unwrap()
            .state
            .attach_handle_context(state.child_handle_context(child_handle).unwrap());
        let section_handle = state.object_handles.reserve().unwrap();
        let section = FileMapping::anonymous(section_handle, 4096, None).unwrap();
        state.object_handles.insert_mapping(
            section_handle,
            Some("device-section".into()),
            section.clone(),
        );
        let event_handle = state.object_handles.reserve().unwrap();
        state
            .object_handles
            .bind(event_handle, HandleObject::Event(event_handle));
        state.events.insert(
            event_handle,
            EventObject {
                manual_reset: true,
                signalled: false,
            },
        );
        let parent_address = unsafe {
            state
                .mapped_views
                .map(parent.cpu.as_mut(), section.clone(), 2, 0, 0)
        }
        .unwrap();
        let child_state = &mut child.process.as_mut().unwrap().state;
        let child_section_handle = child_state.object_handles.reserve().unwrap();
        child_state
            .object_handles
            .insert_mapping(child_section_handle, None, section);
        let child_address = unsafe {
            child_state.mapped_views.map(
                child.cpu.as_mut(),
                child_state
                    .object_handles
                    .mapping(child_section_handle)
                    .unwrap(),
                2,
                0,
                0,
            )
        }
        .unwrap();
        parent.cpu.write_mem(parent_address, b"saved").unwrap();
        for emulator in [&mut parent, &mut child] {
            emulator.process.as_mut().unwrap().state.publish_handles();
        }
        let at = crate::checkpoint_domain::CheckpointTime::now();
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: 0x11000,
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
        let saved = SessionSnapshot::assemble_device(vec![(1, first), (2, second)], 2).unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(parent);
        drop(child);
        std::fs::write(&path, b"later").unwrap();
        let mut rejected = SessionSnapshot::read(bytes.as_slice()).unwrap();
        let (shared, foreground, mut others) = rejected.take_device().unwrap().into_parts();
        let mut destination = fixture(3);
        destination
            .process
            .as_mut()
            .unwrap()
            .state
            .vfs
            .mount_save_dir(r"\Data\", directory.path());
        let failed_group = Arc::new(
            DeviceRestoreGroup::new(
                shared,
                foreground,
                &destination.process.as_ref().unwrap().state,
                std::time::Duration::from_secs(10),
            )
            .unwrap(),
        );
        let remote_group = failed_group.clone();
        let remote = std::thread::spawn(move || {
            let mut wrong_executable = fixture(4);
            remote_group
                .restore_process(&mut wrong_executable, others.remove(0))
                .err()
                .unwrap()
        });
        assert!(failed_group
            .restore_process(&mut destination, rejected)
            .is_err());
        assert!(remote
            .join()
            .unwrap()
            .to_string()
            .contains("different executable"));
        drop(destination);
        drop(failed_group);
        assert_eq!(std::fs::read(&path).unwrap(), b"later");
        let mut root = SessionSnapshot::read(bytes.as_slice()).unwrap();
        assert!(root.has_device());
        let (shared, foreground, mut children) = root.take_device().unwrap().into_parts();
        let reference = fixture(3);
        let group = Arc::new(
            DeviceRestoreGroup::new(
                shared,
                foreground,
                &reference.process.as_ref().unwrap().state,
                std::time::Duration::from_secs(10),
            )
            .unwrap(),
        );
        drop(reference);
        let barrier = Arc::new(Barrier::new(2));
        let peer_group = group.clone();
        let peer_barrier = barrier.clone();
        let (epoch_tx, epoch_rx) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let mut child = fixture(3);
            let restored = peer_group
                .restore_process(&mut child, children.remove(0))
                .unwrap();
            epoch_tx.send(restored.resumed.unwrap()).unwrap();
            assert_eq!(child.cpu.read_mem(child_address, 5).unwrap(), b"saved");
            peer_barrier.wait();
            peer_barrier.wait();
            assert_eq!(child.cpu.read_mem(child_address, 5).unwrap(), b"again");
            assert!(
                child
                    .process
                    .as_ref()
                    .unwrap()
                    .state
                    .events
                    .get(&event_handle)
                    .unwrap()
                    .signalled
            );
            child
                .run_restored(restored, &mut StopAfterOneFrame)
                .unwrap();
        });
        let mut parent = fixture(3);
        parent
            .process
            .as_mut()
            .unwrap()
            .state
            .vfs
            .mount_save_dir(r"\Data\", directory.path());
        let restored = group.restore_process(&mut parent, root).unwrap();
        assert_eq!(epoch_rx.recv().unwrap(), restored.resumed.unwrap());
        assert_eq!(
            parent
                .process
                .as_ref()
                .unwrap()
                .state
                .object_handles
                .process_id(),
            1
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"saved");
        barrier.wait();
        parent.cpu.write_mem(parent_address, b"again").unwrap();
        parent
            .process
            .as_mut()
            .unwrap()
            .state
            .events
            .get_mut(&event_handle)
            .unwrap()
            .signalled = true;
        barrier.wait();
        peer.join().unwrap();
        parent
            .run_restored(restored, &mut StopAfterOneFrame)
            .unwrap();
    }

    struct StopAfterOneFrame;
    impl FrameHook for StopAfterOneFrame {
        fn on_frame(&mut self, _: &mut KernelState) -> FrameAction {
            FrameAction::Stop
        }
    }

    #[test]
    fn mapped_file_checkpoint_restores_aliases_and_defers_writeback_until_validated() {
        use pocket_cpu::Prot;
        use pocket_kernel::file_mapping::FileMapping;
        use pocket_kernel::handles::HandleObject;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("section.bin");
        std::fs::write(&path, vec![0u8; 8192]).unwrap();
        let mut source = fixture(3);
        let state = &mut source.process.as_mut().unwrap().state;
        state.vfs.mount_save_dir("\\Game\\", directory.path());
        let file = state
            .vfs
            .open_file("\\Game\\section.bin", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        state.vfs.mark_for_mapping(file);
        let handle = state.object_handles.reserve().unwrap();
        let mapping = FileMapping::file(
            handle,
            8192,
            true,
            state.vfs.mapping_backing(file, 8192, true).unwrap(),
        );
        state
            .object_handles
            .insert_mapping(handle, Some("shared-section".into()), mapping.clone());
        let alias = state
            .object_handles
            .duplicate(HandleObject::Mapping(handle))
            .unwrap();
        state.vfs.attach_section_file(handle, file);
        let first = unsafe {
            state
                .mapped_views
                .map(source.cpu.as_mut(), mapping.clone(), 2, 0, 0)
        }
        .unwrap();
        let second = unsafe {
            state
                .mapped_views
                .map(source.cpu.as_mut(), mapping, 4, 4096, 0)
        }
        .unwrap();
        source.cpu.write_mem(first + 4096, b"checkpoint").unwrap();
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: 0x11000,
        };
        let snapshot =
            SessionSnapshot::capture(source.cpu.as_mut(), source.process.as_mut().unwrap(), point)
                .unwrap();
        source.cpu.write_mem(first + 4096, b"later data").unwrap();
        source
            .process
            .as_ref()
            .unwrap()
            .state
            .mapped_views
            .flush(first + 4096, 10)
            .unwrap();
        drop(source);

        let mut encoded = Vec::new();
        snapshot.write(&mut encoded).unwrap();
        assert_eq!(&std::fs::read(&path).unwrap()[4096..4106], b"later data");
        let mut invalid = SessionSnapshot::read(encoded.as_slice()).unwrap();
        invalid
            .input_releases
            .push(pocket_kernel::InputEvent::KeyDown { vk: 0x27 });
        let mut rejected = fixture(3);
        rejected
            .process
            .as_mut()
            .unwrap()
            .state
            .vfs
            .mount_save_dir("\\Game\\", directory.path());
        assert!(rejected.restore_snapshot(invalid).is_err());
        drop(rejected);
        assert_eq!(&std::fs::read(&path).unwrap()[4096..4106], b"later data");

        let mut destination = fixture(3);
        destination
            .process
            .as_mut()
            .unwrap()
            .state
            .vfs
            .mount_save_dir("\\Game\\", directory.path());
        destination
            .restore_snapshot(SessionSnapshot::read(encoded.as_slice()).unwrap())
            .unwrap();
        assert_eq!(destination.cpu.read_mem(second, 10).unwrap(), b"checkpoint");
        assert!(destination
            .cpu
            .check_guest_access(second, 1, Prot::WRITE)
            .is_err());
        let state = &mut destination.process.as_mut().unwrap().state;
        let mapping = state.object_handles.mapping(handle).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &mapping,
            &state.object_handles.mapping(alias).unwrap()
        ));
        assert!(std::sync::Arc::ptr_eq(
            &mapping,
            &state
                .object_handles
                .named_mapping("shared-section")
                .unwrap()
        ));
        assert_eq!(state.vfs.take_section_file(handle), Some(file));
        assert!(state.object_handles.close(handle).is_some());
        assert!(state.vfs.close(file));
        state
            .mapped_views
            .unmap(destination.cpu.as_mut(), first)
            .unwrap();
        assert_eq!(destination.cpu.read_mem(second, 10).unwrap(), b"checkpoint");
        assert_eq!(&std::fs::read(path).unwrap()[4096..4106], b"checkpoint");
    }

    #[test]
    fn ram_mapped_checkpoint_preserves_orphan_backing_and_store_accounting() {
        use pocket_kernel::{file_mapping::FileMapping, memory_division::MemoryDivision};
        let mut source = fixture(3);
        source.set_memory_division(Some(MemoryDivision::new(64 * 1024 * 1024, 4096).unwrap()));
        let state = &mut source.process.as_mut().unwrap().state;
        assert!(state.vfs.create_dir(r"\RAM"));
        let file = state
            .vfs
            .open_file(r"\RAM\section", 3, 3, 2, false, 0)
            .unwrap()
            .handle;
        state.vfs.write(file, &vec![0; 8192]).unwrap();
        state.vfs.mark_for_mapping(file);
        let handle = state.object_handles.reserve().unwrap();
        let mapping = FileMapping::file(
            handle,
            8192,
            true,
            state.vfs.mapping_backing(file, 8192, true).unwrap(),
        );
        state
            .object_handles
            .insert_mapping(handle, Some("ram-section".into()), mapping.clone());
        let first = unsafe {
            state
                .mapped_views
                .map(source.cpu.as_mut(), mapping.clone(), 2, 0, 0)
        }
        .unwrap();
        let second = unsafe {
            state
                .mapped_views
                .map(source.cpu.as_mut(), mapping, 4, 4096, 0)
        }
        .unwrap();
        // A mapped file description remains alive even after CloseHandle.
        assert!(state.vfs.close(file));
        source
            .cpu
            .write_mem(first + 4096, b"RAM checkpoint")
            .unwrap();
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: 0x11000,
        };
        let saved =
            SessionSnapshot::capture(source.cpu.as_mut(), source.process.as_mut().unwrap(), point)
                .unwrap();
        let used = source
            .process
            .as_ref()
            .unwrap()
            .state
            .memory_division
            .as_ref()
            .unwrap()
            .snapshot()
            .store_used;
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut restored = fixture(3);
        restored.set_memory_division(Some(MemoryDivision::new(64 * 1024 * 1024, 4096).unwrap()));
        restored
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        assert_eq!(
            restored.cpu.read_mem(second, 14).unwrap(),
            b"RAM checkpoint"
        );
        restored.cpu.write_mem(second, b"same").unwrap();
        assert_eq!(restored.cpu.read_mem(first + 4096, 4).unwrap(), b"same");
        let state = &mut restored.process.as_mut().unwrap().state;
        assert_eq!(
            state
                .memory_division
                .as_ref()
                .unwrap()
                .snapshot()
                .store_used,
            used
        );
        let file = state
            .vfs
            .open_file(r"\RAM\section", 1, 3, 3, false, 0)
            .unwrap()
            .handle;
        state
            .vfs
            .seek(file, 4096, pocket_kernel::vfs::SeekKind::Begin);
        let mut data = [0; 4];
        state.vfs.read(file, &mut data).unwrap();
        assert_eq!(&data, b"same");
    }

    #[test]
    fn checkpoint_restores_incremental_crypto_dom_aliases_and_sdl_surfaces() {
        use pocket_kernel::crypto::{Digest as _, Sha1};
        use pocket_kernel::{
            crypto::{CryptoHash, CryptoKey},
            msxml::{ComKind, ComObject},
            sdl::Surface,
        };
        let mut source = fixture(3);
        let state = &mut source.process.as_mut().unwrap().state;
        let provider = state.crypto.allocate().unwrap();
        state.crypto.providers.insert(provider);
        let hash = state.crypto.allocate().unwrap();
        let mut digest = Sha1::new();
        digest.update([0x37; 127]);
        state.crypto.hashes.insert(
            hash,
            CryptoHash {
                provider,
                digest,
                finished: false,
                value: None,
            },
        );
        let key = state.crypto.allocate().unwrap();
        state.crypto.keys.insert(
            key,
            CryptoKey {
                provider,
                bytes: [7; 8],
                ecb: true,
            },
        );
        let document = state
            .msxml
            .insert_document(
                pocket_kernel::msxml::parse("<root><child>saved text</child></root>", false)
                    .unwrap(),
            )
            .unwrap();
        let root = state.msxml.doc(document).unwrap().root_element.unwrap();
        for (address, kind) in [
            (0x500800, ComKind::Document),
            (0x500820, ComKind::Node(root)),
            (0x500840, ComKind::ChildList(root)),
        ] {
            state.msxml.objects.insert(
                address,
                ComObject {
                    doc: document,
                    kind,
                    iter_pos: 0,
                    refs: 1,
                    preserve_whitespace: false,
                },
            );
        }
        state.sdl.initialized = 0x20;
        state.sdl.screen = 0x500700;
        state.sdl.key_ptr = 0x500000;
        state.sdl.keys[273] = 1;
        state.sdl.events.push_back([24; 20]);
        state.sdl.surfaces.insert(
            0x500700,
            Surface {
                width: 4,
                height: 4,
                pitch: 8,
                bytes: 2,
                pixels: 0x500400,
                format: 0x500600,
                masks: [0xf800, 0x7e0, 0x1f, 0],
            },
        );
        source.cpu.write_mem(0x500000, &state.sdl.keys).unwrap();
        source.cpu.write_mem(0x500400, &[0x37; 32]).unwrap();
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: 0x11000,
        };
        let saved =
            SessionSnapshot::capture(source.cpu.as_mut(), source.process.as_mut().unwrap(), point)
                .unwrap();
        assert_eq!(source.process.as_ref().unwrap().state.sdl.keys[273], 1);
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        let state = &mut destination.process.as_mut().unwrap().state;
        let digest = &mut state.crypto.hashes.get_mut(&hash).unwrap().digest;
        digest.update(b" continued");
        let mut expected = Sha1::new();
        expected.update([0x37; 127]);
        expected.update(b" continued");
        assert_eq!(digest.clone().finalize(), expected.finalize());
        assert_eq!(state.crypto.keys[&key].bytes, [7; 8]);
        assert_eq!(state.msxml.text_of(document, root), "saved text");
        state.msxml.objects.remove(&0x500800);
        state.msxml.collect_document(document);
        assert!(state.msxml.doc(document).is_some());
        state.msxml.objects.remove(&0x500820);
        state.msxml.objects.remove(&0x500840);
        state.msxml.collect_document(document);
        assert!(state.msxml.doc(document).is_none());
        assert_eq!(state.sdl.surfaces[&state.sdl.screen].pixels, 0x500400);
        assert_eq!(destination.cpu.read_mem(0x500400, 32).unwrap(), [0x37; 32]);
        assert!(destination
            .cpu
            .read_mem(0x500000, 323)
            .unwrap()
            .iter()
            .all(|&v| v == 0));
        assert_eq!(state.sdl.events[0], [24; 20]);
        assert!(state
            .sdl
            .events
            .iter()
            .any(|event| event[0] == 3
                && u32::from_le_bytes(event[8..12].try_into().unwrap()) == 273));
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
    fn kernel_records_evolve_inside_the_same_native_session_envelope() {
        let mut source = fixture(3);
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11004,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write_sections(&mut bytes).unwrap();
        let mut at = 2;
        while u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) != KERNEL {
            at += 12 + u64::from_le_bytes(bytes[at + 4..at + 12].try_into().unwrap()) as usize;
        }
        let len = u64::from_le_bytes(bytes[at + 4..at + 12].try_into().unwrap()) as usize;
        let body = at + 12;
        let count = u16::from_le_bytes(bytes[body..body + 2].try_into().unwrap());
        let mut extended = bytes.clone();
        let mut field = Vec::new();
        pocket_snapshot::fields::write_value(
            &mut field,
            pocket_snapshot::fields::Header {
                id: 0x7f01,
                version: 1,
                len: 4,
            },
            &42u32,
        )
        .unwrap();
        extended.splice(body + len..body + len, field);
        extended[body..body + 2].copy_from_slice(&(count + 1).to_le_bytes());
        extended[at + 4..at + 12].copy_from_slice(&((len + 16) as u64).to_le_bytes());
        let restored = SessionSnapshot::read_sections(&mut extended.as_slice()).unwrap();
        let mut destination = fixture(3);
        destination.restore_snapshot(restored).unwrap();
        extended[body + len..body + len + 2].copy_from_slice(&0xff01u16.to_le_bytes());
        assert!(SessionSnapshot::read_sections(&mut extended.as_slice())
            .err()
            .unwrap()
            .to_string()
            .contains("required state record"));
        bytes[at + 2..at + 4].copy_from_slice(&11u16.to_le_bytes());
        assert!(SessionSnapshot::read_sections(&mut bytes.as_slice())
            .err()
            .unwrap()
            .to_string()
            .contains("WinCE snapshot container 11"));
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
    fn read_rejects_a_validly_compressed_but_inconsistent_continuation() {
        let mut source = fixture(3);
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: 0x11004,
        };
        let mut saved =
            SessionSnapshot::capture(source.cpu.as_mut(), source.process.as_mut().unwrap(), point)
                .unwrap();
        saved.point.pc = 0x11000;
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        let error = SessionSnapshot::read(bytes.as_slice()).err().unwrap();
        assert!(error.to_string().contains("continuation mismatch"));
    }

    #[test]
    fn current_host_pacing_keeps_saved_guest_waits() {
        let mut source = fixture(3);
        source.set_frame_rate_limit(Some(30));
        source
            .process_mut()
            .unwrap()
            .state
            .framebuffer
            .directdraw
            .pending
            .insert(
                (0, 1, 2),
                std::time::Instant::now() + std::time::Duration::from_secs(1),
            );
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: 0x11004,
        };
        let saved =
            SessionSnapshot::capture(source.cpu.as_mut(), source.process.as_mut().unwrap(), point)
                .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination.set_frame_rate_limit(Some(60));
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        let timing = &destination.process().unwrap().state.framebuffer.directdraw;
        assert_eq!(timing.frame_rate_limit, Some(60));
        assert_eq!(timing.pending.len(), 1);
    }

    #[test]
    fn frozen_wince12_process_reference_restores_guest_state() {
        use pocket_snapshot::fields::FieldSet;
        let bytes = include_bytes!("../tests/fixtures/wince12-process-v1.bin");
        let saved = pocket_snapshot::read_stream(bytes.as_slice(), b"PHKW12G1", |input| {
            KernelSnapshot::read_archive(input)
        })
        .unwrap();
        let mut destination = fixture(3);
        saved
            .restore(&mut destination.process.as_mut().unwrap().state)
            .unwrap();
        let state = &destination.process().unwrap().state;
        assert_eq!(
            state.gdi.pending_window_updates[&0xdead0001],
            vec![[0, 0, 320, 240]]
        );
        assert!(state.gdi.generated_window_paints.contains(&0xdead0001));
        assert_eq!(state.message_frames[&0][0].message, 0x000f);
        assert_eq!(state.image_base, 0x10000);
        assert!(state.gdi.shell_toolbars.is_empty());
    }

    #[test]
    fn pending_mfc_ddx_cold_restores_and_catches_a_real_nonlocal_return() {
        use pocket_kernel::{DispatchOutcome, Dispatcher, Thunk};
        use pocket_pe::ImportBinding;
        fn invoke(
            e: &mut Emulator,
            dll: &str,
            name: &str,
            va: u32,
            args: &[u32],
        ) -> DispatchOutcome {
            for (i, &v) in args.iter().enumerate() {
                e.cpu.write_arg(i as u8, v).unwrap();
            }
            let thunk = Thunk {
                thunk_va: va,
                iat_va: 0,
                dll: dll.into(),
                binding: ImportBinding::Name(name.into()),
                friendly_name: None,
            };
            e.dispatcher
                .dispatch(
                    e.cpu.as_mut(),
                    &thunk,
                    &mut e.process.as_mut().unwrap().state,
                )
                .unwrap()
        }
        let mut source = fixture(3);
        source
            .cpu
            .map_region(
                0x60000000,
                0x10000,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )
            .unwrap();
        source.cpu.write_reg(ArmReg::Sp, 0x6000fff0).unwrap();
        let owner = source
            .process()
            .unwrap()
            .state
            .dynamic_exports
            .iter()
            .find_map(|(&owner, exports)| {
                exports
                    .contains_key("__PocketHLE_MFC_UpdateData")
                    .then_some(owner)
            })
            .unwrap();
        invoke(
            &mut source,
            "mfcce300.dll",
            "ord:206",
            0x72000000,
            &[0x60000000],
        );
        let table = source.cpu.read_u32_le(0x60000000).unwrap();
        let bytes = source.cpu.read_mem(table, 40 * 4).unwrap();
        source.cpu.write_mem(0x60000100, &bytes).unwrap();
        source
            .cpu
            .write_mem(0x60000100 + 33 * 4, &0x11000u32.to_le_bytes())
            .unwrap();
        source
            .cpu
            .write_mem(0x60000000, &0x60000100u32.to_le_bytes())
            .unwrap();
        source
            .cpu
            .write_mem(0x60000020, &7u32.to_le_bytes())
            .unwrap();
        let DispatchOutcome::ReturnedR0(ts) = invoke(
            &mut source,
            "mfcce300.dll",
            "?AfxGetThreadState@@YAPAV_AFX_THREAD_STATE@@XZ",
            0x72000000,
            &[],
        ) else {
            panic!()
        };
        source
            .cpu
            .write_mem(ts + 0x120, &73u32.to_le_bytes())
            .unwrap();
        source.cpu.write_reg(ArmReg::Lr, 0x11008).unwrap();
        source.cpu.write_reg(ArmReg::R4, 0xcafe).unwrap();
        let update =
            source.process().unwrap().state.dynamic_exports[&owner]["__PocketHLE_MFC_UpdateData"];
        assert_eq!(
            invoke(
                &mut source,
                "coredll.dll",
                "__PocketHLE_MFC_UpdateData",
                update,
                &[0x60000000, 1]
            ),
            DispatchOutcome::JumpTo(0x11000)
        );
        let link = source.cpu.read_u32_le(ts + 0x10).unwrap();
        assert_ne!(link, 0);
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11000,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        assert_eq!(destination.cpu.read_u32_le(ts + 0x10).unwrap(), link);
        destination.cpu.write_reg(ArmReg::R4, 0).unwrap();
        let user = destination.process().unwrap().state.dynamic_exports[&owner]
            ["__PocketHLE_MFC_ThrowUser"];
        assert_eq!(
            invoke(
                &mut destination,
                "coredll.dll",
                "__PocketHLE_MFC_ThrowUser",
                user,
                &[]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        destination.cpu.write_reg(ArmReg::R0, 1).unwrap();
        let mut pc = destination.cpu.read_reg(ArmReg::Lr).unwrap();
        for _ in 0..256 {
            if pc == 0x11008 {
                break;
            }
            let name = destination.process().unwrap().state.dynamic_exports[&owner]
                .iter()
                .find_map(|(n, &v)| (v == pc).then_some(n.clone()))
                .unwrap();
            pc = match invoke(&mut destination, "coredll.dll", &name, pc, &[]) {
                DispatchOutcome::JumpTo(p) => p,
                DispatchOutcome::ReturnedR0(v) => {
                    destination.cpu.write_reg(ArmReg::R0, v).unwrap();
                    destination.cpu.read_reg(ArmReg::Lr).unwrap()
                }
                other => panic!("unexpected {other:?}"),
            };
        }
        assert_eq!(pc, 0x11008);
        assert_eq!(destination.cpu.read_reg(ArmReg::R0).unwrap(), 0);
        assert_eq!(destination.cpu.read_reg(ArmReg::R4).unwrap(), 0xcafe);
        assert_eq!(destination.cpu.read_u32_le(ts + 0x120).unwrap(), 73);
        assert_eq!(destination.cpu.read_u32_le(ts + 0x10).unwrap(), 0);
        assert!(
            !destination.process().unwrap().state.dynamic_exports[&owner]
                .keys()
                .any(|k| k.starts_with("__PocketHLE_MFC_Frame:"))
        );
    }

    #[test]
    fn pending_mfc_window_callback_cold_restores_the_try_and_saved_msg() {
        use pocket_kernel::{DispatchOutcome, Dispatcher, Thunk};
        use pocket_pe::ImportBinding;
        fn invoke(
            e: &mut Emulator,
            dll: &str,
            name: &str,
            va: u32,
            args: &[u32],
        ) -> DispatchOutcome {
            for (i, &v) in args.iter().enumerate() {
                if i < 4 {
                    e.cpu.write_arg(i as u8, v).unwrap();
                } else {
                    let sp = e.cpu.read_reg(ArmReg::Sp).unwrap();
                    e.cpu
                        .write_mem(
                            sp + e.cpu.stack_arg_offset() + (i as u32 - 4) * 4,
                            &v.to_le_bytes(),
                        )
                        .unwrap();
                }
            }
            let thunk = Thunk {
                thunk_va: va,
                iat_va: 0,
                dll: dll.into(),
                binding: ImportBinding::Name(name.into()),
                friendly_name: None,
            };
            e.dispatcher
                .dispatch(
                    e.cpu.as_mut(),
                    &thunk,
                    &mut e.process.as_mut().unwrap().state,
                )
                .unwrap()
        }
        let mut source = fixture(3);
        source
            .cpu
            .map_region(
                0x60000000,
                0x10000,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )
            .unwrap();
        source.cpu.write_reg(ArmReg::Sp, 0x6000fff0).unwrap();
        let owner = source
            .process()
            .unwrap()
            .state
            .dynamic_exports
            .iter()
            .find_map(|(&owner, exports)| {
                exports
                    .contains_key("__PocketHLE_MFC_CallWndProc")
                    .then_some(owner)
            })
            .unwrap();
        invoke(
            &mut source,
            "mfcce300.dll",
            "ord:206",
            0x72000000,
            &[0x60000000],
        );
        let table = source.cpu.read_u32_le(0x60000000).unwrap();
        let bytes = source.cpu.read_mem(table, 46 * 4).unwrap();
        source.cpu.write_mem(0x60000100, &bytes).unwrap();
        source
            .cpu
            .write_mem(0x60000100 + 38 * 4, &0x11000u32.to_le_bytes())
            .unwrap();
        source
            .cpu
            .write_mem(0x60000000, &0x60000100u32.to_le_bytes())
            .unwrap();
        source
            .cpu
            .write_mem(0x60000020, &7u32.to_le_bytes())
            .unwrap();
        let DispatchOutcome::ReturnedR0(ts) = invoke(
            &mut source,
            "mfcce300.dll",
            "?AfxGetThreadState@@YAPAV_AFX_THREAD_STATE@@XZ",
            0x72000000,
            &[],
        ) else {
            panic!()
        };
        source.cpu.write_mem(ts + 0x3c, &[0x5a; 28]).unwrap();
        source.cpu.write_reg(ArmReg::Lr, 0x11008).unwrap();
        source.cpu.write_reg(ArmReg::R4, 0xcafe).unwrap();
        let update =
            source.process().unwrap().state.dynamic_exports[&owner]["__PocketHLE_MFC_CallWndProc"];
        assert_eq!(
            invoke(
                &mut source,
                "coredll.dll",
                "__PocketHLE_MFC_CallWndProc",
                update,
                &[0x60000000, 7, 0x400, 42, 0x1234]
            ),
            DispatchOutcome::JumpTo(0x11000)
        );
        let link = source.cpu.read_u32_le(ts + 0x10).unwrap();
        assert_ne!(link, 0);
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11000,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        assert_eq!(destination.cpu.read_u32_le(ts + 0x10).unwrap(), link);
        // Resume the real WindowProc return into its saved TRY-owned frame.
        destination.cpu.write_reg(ArmReg::R0, 55).unwrap();
        let mut pc = destination.cpu.read_reg(ArmReg::Lr).unwrap();
        for _ in 0..256 {
            if pc == 0x11008 {
                break;
            }
            let name = destination.process().unwrap().state.dynamic_exports[&owner]
                .iter()
                .find_map(|(n, &v)| (v == pc).then_some(n.clone()))
                .unwrap();
            pc = match invoke(&mut destination, "coredll.dll", &name, pc, &[]) {
                DispatchOutcome::JumpTo(p) => p,
                DispatchOutcome::ReturnedR0(v) => {
                    destination.cpu.write_reg(ArmReg::R0, v).unwrap();
                    destination.cpu.read_reg(ArmReg::Lr).unwrap()
                }
                other => panic!("unexpected {other:?}"),
            };
        }
        assert_eq!(pc, 0x11008);
        assert_eq!(destination.cpu.read_reg(ArmReg::R0).unwrap(), 55);
        assert_eq!(destination.cpu.read_reg(ArmReg::R4).unwrap(), 0xcafe);
        assert_eq!(destination.cpu.read_mem(ts + 0x3c, 28).unwrap(), [0x5a; 28]);
        assert_eq!(destination.cpu.read_u32_le(ts + 0x10).unwrap(), 0);
        assert!(
            !destination.process().unwrap().state.dynamic_exports[&owner]
                .keys()
                .any(|k| k.starts_with("__PocketHLE_MFC_Frame:"))
        );
    }

    #[test]
    fn editor_clipboard_and_document_list_owners_survive_cold_restore() {
        use pocket_cpu::Prot;
        let mut source = fixture(3);
        let state = &mut source.process.as_mut().unwrap().state;
        let hwnd = state.controls.create(
            7,
            pocket_kernel::controls::ControlClass::Edit,
            10,
            "12345\r\nsecond\r\nthird".into(),
            0x50010004,
            1,
            1,
            100,
            20,
        );
        let edit = &mut state.controls.get_mut(hwnd).unwrap().edit;
        edit.selection = Some((4, 1));
        edit.first_line = 2;
        edit.scroll_x = 47;
        edit.limit = 9;
        edit.modified = true;
        edit.undo = Some(("previous".into(), Some((0, 3))));
        edit.tab_stops = vec![4, 8];
        let list = state.controls.create(
            7,
            pocket_kernel::controls::ControlClass::ListView,
            11,
            "".into(),
            0x50000000,
            0,
            30,
            100,
            60,
        );
        state.controls.get_mut(list).unwrap().list_items = vec![("note.txt".into(), 0)];
        state.controls.doc_lists.insert(
            list,
            pocket_kernel::controls::doc_list::DocList {
                directory: "\\".into(),
                filters: vec![vec!["*.txt".into()]],
                filter_index: 1,
                paths: vec!["\\note.txt".into()],
                updates_enabled: true,
                descending: false,
                recursive: false,
            },
        );
        let clipboard = state.heap.alloc(6).unwrap();
        state.clipboard.formats.insert(13, clipboard);
        state.clipboard.opened_by = Some(0);
        if source
            .cpu
            .check_guest_access(clipboard, 6, Prot::WRITE)
            .is_err()
        {
            source
                .cpu
                .map_region(clipboard & !4095, 4096, Prot::READ | Prot::WRITE)
                .unwrap();
        }
        source
            .cpu
            .write_mem(clipboard, &[65, 0, 66, 0, 0, 0])
            .unwrap();
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11004,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        for _ in 0..3 {
            let mut destination = fixture(3);
            destination
                .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
                .unwrap();
            let control = destination
                .process()
                .unwrap()
                .state
                .controls
                .get(hwnd)
                .unwrap();
            assert_eq!(control.text, "12345\r\nsecond\r\nthird");
            assert_eq!(control.edit.selection, Some((4, 1)));
            assert_eq!(control.edit.first_line, 2);
            assert_eq!(control.edit.scroll_x, 47);
            assert_eq!(control.edit.limit, 9);
            assert!(control.edit.modified);
            assert_eq!(control.edit.undo.as_ref().unwrap().0, "previous");
            assert_eq!(control.edit.tab_stops, [4, 8]);
            let state = &destination.process().unwrap().state;
            assert_eq!(state.clipboard.opened_by, Some(0));
            assert_eq!(state.clipboard.formats[&13], clipboard);
            assert!(state.heap.owns_global_block(clipboard));
            assert_eq!(state.controls.doc_lists[&list].paths, ["\\note.txt"]);
            assert_eq!(
                destination.cpu.read_mem(clipboard, 6).unwrap(),
                [65, 0, 66, 0, 0, 0]
            );
            let control = destination
                .process
                .as_mut()
                .unwrap()
                .state
                .controls
                .get_mut(hwnd)
                .unwrap();
            assert!(control.edit.undo(&mut control.text));
            assert_eq!(control.text, "previous");
        }
    }

    #[test]
    fn archive_identity_map_cold_restores_and_keeps_its_borrowed_vfs_description() {
        use pocket_kernel::{DispatchOutcome, Dispatcher, Thunk};
        fn invoke(e: &mut Emulator, ordinal: u16, args: &[u32]) -> u32 {
            for (i, &v) in args.iter().enumerate() {
                e.cpu.write_arg(i as u8, v).unwrap();
            }
            e.cpu.write_reg(ArmReg::Lr, 0x11008).unwrap();
            drive(e, ordinal, 0x72000000, 0x11008);
            e.cpu.read_reg(ArmReg::R0).unwrap()
        }
        fn drive(e: &mut Emulator, ordinal: u16, mut pc: u32, stop: u32) {
            let start = Thunk {
                thunk_va: 0x72000000,
                iat_va: 0,
                dll: "mfcce300.dll".into(),
                binding: pocket_pe::ImportBinding::Ordinal(ordinal),
                friendly_name: None,
            };
            for _ in 0..16384 {
                if pc == stop {
                    return;
                }
                let thunk = if pc == start.thunk_va {
                    start.clone()
                } else {
                    let name = e
                        .process()
                        .unwrap()
                        .state
                        .dynamic_exports
                        .values()
                        .find_map(|exports| {
                            exports
                                .iter()
                                .find_map(|(name, &va)| (va == pc).then_some(name.clone()))
                        })
                        .unwrap_or_else(|| panic!("unexpected archive guest callback {pc:08x}"));
                    Thunk {
                        thunk_va: pc,
                        iat_va: 0,
                        dll: "coredll.dll".into(),
                        binding: pocket_pe::ImportBinding::Name(name),
                        friendly_name: None,
                    }
                };
                pc = match e
                    .dispatcher
                    .dispatch(
                        e.cpu.as_mut(),
                        &thunk,
                        &mut e.process.as_mut().unwrap().state,
                    )
                    .unwrap()
                {
                    DispatchOutcome::JumpTo(target) => target,
                    DispatchOutcome::ReturnedR0(value) => {
                        e.cpu.write_reg(ArmReg::R0, value).unwrap();
                        e.cpu.read_reg(ArmReg::Lr).unwrap()
                    }
                    other => panic!("unexpected archive outcome {other:?}"),
                };
            }
            panic!("archive callback budget exceeded")
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("graph.bin");
        std::fs::write(&path, []).unwrap();
        let mut source = fixture(3);
        source
            .cpu
            .map_region(
                0x60000000,
                0x10000,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )
            .unwrap();
        source.cpu.write_reg(ArmReg::Sp, 0x6000fff0).unwrap();
        let state = &mut source.process.as_mut().unwrap().state;
        state.vfs.mount_save_dir(r"\Data\", directory.path());
        let handle = state
            .vfs
            .open(
                r"\Data\graph.bin",
                pocket_kernel::vfs::Access::ReadWrite,
                false,
            )
            .unwrap();
        invoke(&mut source, 97, &[0x60000000]);
        source
            .cpu
            .write_mem(0x60000004, &handle.to_le_bytes())
            .unwrap();
        source
            .cpu
            .write_mem(0x6000fff0, &0u32.to_le_bytes())
            .unwrap();
        invoke(&mut source, 38, &[0x60000100, 0x60000000, 0, 128]);
        for n in 1..=150 {
            invoke(&mut source, 1754, &[0x60000100, 0x60001000 + n * 4]);
        }
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11004,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination
            .process
            .as_mut()
            .unwrap()
            .state
            .vfs
            .mount_save_dir(r"\Data\", directory.path());
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        assert_eq!(destination.cpu.read_u32_le(0x60000130).unwrap(), 151);
        invoke(&mut destination, 2590, &[0x60000100, 0x60001000 + 150 * 4]);
        // Park with real CFile::Write arguments prepared, before the borrowed
        // file is touched. Capture the actual archive/stream continuation chain.
        let file_table = destination.cpu.read_u32_le(0x60000000).unwrap();
        let write_target = destination.cpu.read_u32_le(file_table + 15 * 4).unwrap();
        destination.cpu.write_arg(0, 0x60000100).unwrap();
        destination.cpu.write_reg(ArmReg::Lr, 0x11008).unwrap();
        drive(&mut destination, 851, 0x72000000, write_target);
        assert_eq!(destination.cpu.read_arg(0).unwrap(), 0x60000000);
        assert_eq!(destination.cpu.read_arg(2).unwrap(), 2);
        assert!(std::fs::read(&path).unwrap().is_empty());
        let saved = SessionSnapshot::capture(
            destination.cpu.as_mut(),
            destination.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: write_target,
            },
        )
        .unwrap();
        let mut pending_bytes = Vec::new();
        saved.write(&mut pending_bytes).unwrap();
        drop(saved);
        drop(destination);
        let mut destination = fixture(3);
        destination
            .process
            .as_mut()
            .unwrap()
            .state
            .vfs
            .mount_save_dir(r"\Data\", directory.path());
        destination
            .restore_snapshot(SessionSnapshot::read(pending_bytes.as_slice()).unwrap())
            .unwrap();
        drive(&mut destination, 851, write_target, 0x11008);

        assert_eq!(std::fs::read(&path).unwrap(), [150, 0]);
        assert!(destination.process().unwrap().state.vfs.is_open(handle));
        assert_eq!(
            destination.process.as_mut().unwrap().state.vfs.seek(
                handle,
                0,
                pocket_kernel::vfs::SeekKind::Current
            ),
            Some(2)
        );
        assert!(!destination
            .process()
            .unwrap()
            .state
            .dynamic_exports
            .values()
            .any(|exports| exports
                .keys()
                .any(|key| key.starts_with("__PocketHLE_MFC_Frame:"))));
        invoke(&mut destination, 212, &[0x60000100]);
        assert_eq!(destination.cpu.read_u32_le(0x60000134).unwrap(), 0);
    }

    #[test]
    fn three_state_check_survives_cold_restore_without_changing_the_required_control_record() {
        let mut source = fixture(3);
        let state = &mut source.process.as_mut().unwrap().state;
        let hwnd = state.controls.create(
            7,
            pocket_kernel::controls::ControlClass::Button,
            11,
            "Three states".into(),
            0x50000006,
            1,
            1,
            100,
            20,
        );
        state.controls.get_mut(hwnd).unwrap().set_check_state(2);
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11004,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        let mut destination = fixture(3);
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        let controls = &mut destination.process.as_mut().unwrap().state.controls;
        assert_eq!(controls.get(hwnd).unwrap().check_state(), 2);
        controls.focus = hwnd;
        controls.key_down(0x20);
        assert_eq!(controls.get(hwnd).unwrap().check_state(), 0);
    }

    #[test]
    fn persisted_snapshot_keeps_shell_toolbar_records_and_owned_bitmap_strips() {
        let mut source = fixture(3);
        let state = &mut source.process.as_mut().unwrap().state;
        let hwnd = 0xdead0b01;
        let bitmap = state.gdi.create_compatible_bitmap(16, 16);
        state.gdi.bitmap_mut(bitmap).unwrap().pixels[..2].copy_from_slice(&0x07e0u16.to_le_bytes());
        state.menu_items.insert(
            10,
            vec![pocket_kernel::MenuItem {
                id: 81,
                ..Default::default()
            }],
        );
        state.gdi.shell_menus.bars.insert(
            hwnd,
            pocket_kernel::shell_menu::Bar {
                owner: 0xdead0001,
                menu: 10,
                sip: false,
                background: 0xc618,
            },
        );
        let mut toolbar = pocket_kernel::shell_menu::Toolbar::default();
        toolbar.state.buttons.push([0, 81, 4, 0x1234, 0]);
        toolbar.state.bitmaps.push((bitmap, 1));
        toolbar.owned_bitmaps.push(bitmap);
        toolbar.strings.push("Go".encode_utf16().collect());
        state.gdi.shell_toolbars.insert(hwnd, toolbar);
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11004,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        let state = &destination.process().unwrap().state;
        assert_eq!(
            state.gdi.shell_toolbars[&hwnd].state.buttons,
            vec![[0, 81, 4, 0x1234, 0]]
        );
        assert_eq!(state.gdi.shell_toolbars[&hwnd].owned_bitmaps, vec![bitmap]);
        assert_eq!(
            state.gdi.shell_toolbars[&hwnd].strings,
            vec!["Go".encode_utf16().collect::<Vec<_>>()]
        );
        assert_eq!(
            state.gdi.bitmap(bitmap).unwrap().pixels[..2],
            0x07e0u16.to_le_bytes()
        );
    }

    #[test]
    fn persisted_snapshot_keeps_window_update_regions_and_dispatched_message_identity() {
        let mut source = fixture(3);
        let frame =
            pocket_kernel::DispatchMessageFrame::capture(source.cpu.as_mut(), 0x000f).unwrap();
        let state = &mut source.process.as_mut().unwrap().state;
        state
            .gdi
            .pending_window_updates
            .insert(0xdead0001, vec![[0, 0, 320, 240]]);
        state.gdi.generated_window_paints.insert(0xdead0001);
        state.message_frames.insert(0, vec![frame]);
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11004,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        let state = &destination.process().unwrap().state;
        assert!(state.gdi.pending_window_updates.contains_key(&0xdead0001));
        assert!(state.gdi.generated_window_paints.contains(&0xdead0001));
        assert_eq!(state.message_frames[&0][0].message, 0x000f);
        assert_eq!(state.message_frames[&0][0].args, frame.args);
    }

    #[test]
    fn persisted_snapshot_retains_nested_window_creation_scratch_owners() {
        use pocket_kernel::{CreateStage, GuestCallFrame, WindowCallbackFrame};
        let mut source = fixture(3);
        let state = &mut source.process.as_mut().unwrap().state;
        let outer = state.heap.alloc(64).unwrap();
        let inner = state.heap.alloc(64).unwrap();
        let callback = |sp| WindowCallbackFrame {
            thread: 0,
            frame: GuestCallFrame {
                args: [0; 4],
                lr: 0x11004,
                sp,
            },
        };
        state
            .create_frame_stack
            .push((callback(0x1f000), CreateStage::Create, 0xdead1000));
        state.create_frame = Some(callback(0x1ef00));
        state.create_frame_hwnd = 0xdead1001;
        state.create_stage = CreateStage::Create;
        state
            .window_create_scratch
            .insert((0, 0x1f000, 0xdead1000), outer);
        state
            .window_create_scratch
            .insert((0, 0x1ef00, 0xdead1001), inner);
        source.cpu.write_mem(outer, &[0xa5; 64]).unwrap();
        source.cpu.write_mem(inner, &[0x5a; 64]).unwrap();
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11004,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        saved.write(&mut bytes).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination
            .restore_snapshot(SessionSnapshot::read(bytes.as_slice()).unwrap())
            .unwrap();
        let state = &destination.process.as_ref().unwrap().state;
        assert_eq!(
            state.window_create_scratch.get(&(0, 0x1f000, 0xdead1000)),
            Some(&outer)
        );
        assert_eq!(
            state.window_create_scratch.get(&(0, 0x1ef00, 0xdead1001)),
            Some(&inner)
        );
        assert_eq!(state.heap.msize(outer), Some(64));
        assert_eq!(state.heap.msize(inner), Some(64));
        assert_eq!(destination.cpu.read_mem(outer, 64).unwrap(), [0xa5; 64]);
        assert_eq!(destination.cpu.read_mem(inner, 64).unwrap(), [0x5a; 64]);
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
    #[test]
    fn checkpoint_cold_restores_an_active_decompressor_without_input_replay() {
        let payload: Vec<u8> = (0..200_000).map(|i| ((i / 7) % 251) as u8).collect();
        let encoded = pocket_kernel::zlib::ZlibStreams::compress(&payload).unwrap();
        let mut source = fixture(3);
        source.cpu.write_reg(ArmReg::R0, 40).unwrap();
        let streams = &mut source.process.as_mut().unwrap().state.zlib_streams;
        let handle = streams.init().unwrap();
        let mut prefix = [0; 17];
        let first = streams.inflate(handle, &encoded, &mut prefix, 0).unwrap();
        assert_eq!(first.status, 0);
        let checkpoint = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11004,
            },
        )
        .unwrap();
        let mut file = Vec::new();
        checkpoint.write(&mut file).unwrap();
        drop(checkpoint);
        drop(source);

        let mut destination = fixture(3);
        let resumed = destination
            .restore_snapshot(SessionSnapshot::read(file.as_slice()).unwrap())
            .unwrap();
        let streams = &mut destination.process.as_mut().unwrap().state.zlib_streams;
        let mut input = first.consumed;
        let mut decoded = prefix[..first.written].to_vec();
        loop {
            let mut row = [0; 94];
            let step = streams
                .inflate(handle, &encoded[input..], &mut row, 0)
                .unwrap();
            input += step.consumed;
            decoded.extend_from_slice(&row[..step.written]);
            if step.status == 1 {
                break;
            }
            assert_eq!(step.status, 0);
        }
        assert_eq!(input, encoded.len());
        assert_eq!(decoded, payload);
        assert!(streams.end(handle));
        assert!(!streams.contains(handle));
        assert_ne!(streams.init().unwrap(), handle);
        destination
            .run_restored(resumed, &mut |_state: &mut KernelState| {
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
    #[test]
    fn checkpoint_cold_restores_native_rect_code_mid_operation() {
        use pocket_kernel::native_thunks::{self, SMALL_COPY_HELPER};
        let mut source = fixture(3);
        let rect = 0x60000000;
        source
            .cpu
            .map_region(
                rect,
                0x1000,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )
            .unwrap();
        // Resume between the left/top and right/bottom stores of InflateRect.
        // There is no host-side pending operation to serialize or reconstruct.
        let before: Vec<_> = [7i32, 16, 40, 60]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        source.cpu.write_mem(rect, &before).unwrap();
        source.cpu.write_reg(ArmReg::R0, rect).unwrap();
        source.cpu.write_reg(ArmReg::R1, 3).unwrap();
        source.cpu.write_reg(ArmReg::R2, 4).unwrap();
        source.cpu.write_reg(ArmReg::Lr, 0x11004).unwrap();
        let continuation: Vec<_> = [0xe51ff004, pocket_kernel::PROCESS_EXIT_TRAMPOLINE_VA]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        source.cpu.write_mem(0x11004, &continuation).unwrap();
        let point = ResumePoint {
            main_entry: 0x11000,
            pc: SMALL_COPY_HELPER + 0x300 + 32,
        };
        let saved =
            SessionSnapshot::capture(source.cpu.as_mut(), source.process.as_mut().unwrap(), point)
                .unwrap();
        let mut encoded = Vec::new();
        saved.write(&mut encoded).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        // Poison the freshly loaded helper. Correct restoration must supply the
        // captured machine code, not depend on reinstalling the current helper.
        destination
            .cpu
            .write_mem(SMALL_COPY_HELPER + 0x300, &[0; 64])
            .unwrap();
        let restored = destination
            .restore_snapshot(SessionSnapshot::read(encoded.as_slice()).unwrap())
            .unwrap();
        assert!(native_thunks::native_thunk_at(
            "coredll.dll",
            "InflateRect",
            pocket_kernel::THUNK_REGION_BASE
        )
        .is_some());
        destination
            .run_restored(restored, &mut |_state: &mut KernelState| {
                FrameAction::Continue
            })
            .unwrap();
        let expected: Vec<_> = [7i32, 16, 43, 64]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        assert_eq!(destination.cpu.read_mem(rect, 16).unwrap(), expected);
        assert_eq!(
            destination
                .process
                .as_ref()
                .unwrap()
                .state
                .process_exit_code,
            Some(1)
        );
    }
    #[test]
    fn cold_restore_reconciles_a_pending_remote_file_closure() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("pending.bin"), b"content").unwrap();
        let mut source = fixture(3);
        let state = &mut source.process.as_mut().unwrap().state;
        state.vfs.mount_save_dir(r"\Data\", directory.path());
        let handle = state
            .vfs
            .open_file(r"\Data\pending.bin", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        state.publish_handles();
        state.sync_transferred_handles();
        let pid = state.object_handles.process_id();
        assert!(state.object_handles.close_in(pid, handle).is_some());
        assert!(state.vfs.is_open(handle));
        let saved = SessionSnapshot::capture(
            source.cpu.as_mut(),
            source.process.as_mut().unwrap(),
            ResumePoint {
                main_entry: 0x11000,
                pc: 0x11000,
            },
        )
        .unwrap();
        let mut encoded = Vec::new();
        saved.write(&mut encoded).unwrap();
        drop(saved);
        drop(source);
        let mut destination = fixture(3);
        destination
            .process
            .as_mut()
            .unwrap()
            .state
            .vfs
            .mount_save_dir(r"\Data\", directory.path());
        destination
            .restore_snapshot(SessionSnapshot::read(encoded.as_slice()).unwrap())
            .unwrap();
        let state = &mut destination.process.as_mut().unwrap().state;
        assert!(state.object_handles.is_closed(handle));
        state.sync_transferred_handles();
        assert!(!state.vfs.is_open(handle));
        assert_eq!(
            std::fs::read(directory.path().join("pending.bin")).unwrap(),
            b"content"
        );
    }
}

#[cfg(test)]
#[path = "snapshot_launcher_tests.rs"]
mod launcher_tests;
