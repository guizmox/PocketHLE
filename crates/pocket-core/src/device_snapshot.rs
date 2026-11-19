//! Device checkpoints reuse the normal per-process section codec. Child payloads
//! are uncompressed sections inside the one outer compressed device stream.
use super::*;
use pocket_kernel::KernelState;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::OnceLock;

pub struct ProcessCheckpoint {
    session: SessionSnapshot,
    shared: Option<pocket_kernel::DeviceSnapshot>,
}
#[derive(Serialize, Deserialize)]
pub struct DeviceArchive {
    shared: pocket_kernel::DeviceSnapshot,
    foreground: u32,
    others: Vec<ChildSession>,
}
struct ChildSession {
    session: SessionSnapshot,
    encoded: OnceLock<std::result::Result<Vec<u8>, String>>,
}
impl Serialize for ChildSession {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let bytes = self
            .encoded
            .get_or_init(|| {
                let mut bytes = Vec::new();
                self.session
                    .write_sections(&mut bytes)
                    .map_err(|error| error.to_string())?;
                Ok(bytes)
            })
            .as_ref()
            .map_err(serde::ser::Error::custom)?;
        serializer.serialize_bytes(bytes)
    }
}
impl<'de> Deserialize<'de> for ChildSession {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> std::result::Result<Self, D::Error> {
        let bytes: serde_bytes::ByteBuf = Deserialize::deserialize(decoder)?;
        let mut input = bytes.as_ref();
        let session = SessionSnapshot::read_sections_mode(&mut input, false)
            .map_err(serde::de::Error::custom)?;
        if !input.is_empty() {
            return Err(serde::de::Error::custom("trailing process checkpoint data"));
        }
        Ok(Self {
            session,
            encoded: OnceLock::new(),
        })
    }
}
impl DeviceArchive {
    pub(super) fn validate(&self, root: &SessionSnapshot) -> std::result::Result<(), String> {
        let mut expected = self.shared.process_ids();
        let mut actual = vec![root.process_id()];
        if self.others.len() >= 128 {
            return Err("device checkpoint has too many processes".into());
        }
        for child in &self.others {
            if child.session.device.is_some() || !child.session.kernel.is_device_process() {
                return Err("invalid nested device checkpoint".into());
            }
            actual.push(child.session.process_id());
        }
        expected.sort_unstable();
        actual.sort_unstable();
        if expected != actual
            || !actual.contains(&self.foreground)
            || actual.windows(2).any(|v| v[0] == v[1])
        {
            return Err("device checkpoint process manifest is inconsistent".into());
        }
        let mut waits = std::collections::BTreeMap::new();
        for session in std::iter::once(root).chain(self.others.iter().map(|child| &child.session)) {
            if let Some(handle) = session.kernel.suspended_child_handle()? {
                let child = self
                    .shared
                    .child_process_id(session.process_id(), handle)
                    .ok_or("suspended caller references a missing active child")?;
                waits.insert(session.process_id(), child);
            }
        }
        for &pid in waits.keys() {
            let mut visited = std::collections::BTreeSet::new();
            let mut next = pid;
            while let Some(&child) = waits.get(&next) {
                if !visited.insert(next) {
                    return Err("cyclic suspended launcher chain".into());
                }
                next = child;
            }
        }
        Ok(())
    }
    pub fn into_parts(self) -> (pocket_kernel::DeviceSnapshot, u32, Vec<SessionSnapshot>) {
        (
            self.shared,
            self.foreground,
            self.others.into_iter().map(|child| child.session).collect(),
        )
    }
}
impl SessionSnapshot {
    pub fn environment(&self) -> &std::collections::BTreeMap<String, String> {
        &self.environment
    }
    pub fn set_environment(&mut self, environment: std::collections::BTreeMap<String, String>) {
        self.environment = environment;
    }
    /// Identity of each saved process, including parked launchers and children.
    pub fn executable_identities(&self) -> Vec<(&str, [u8; 32])> {
        let mut all = vec![(self.program_path(), self.executable)];
        if let Some(device) = &self.device {
            all.extend(
                device
                    .others
                    .iter()
                    .map(|child| (child.session.program_path(), child.session.executable)),
            );
        }
        all
    }

    pub fn process_id(&self) -> u32 {
        self.kernel.process_id()
    }
    pub fn program_path(&self) -> &str {
        self.kernel.program_path()
    }
    pub fn take_device(&mut self) -> Option<DeviceArchive> {
        self.device.take()
    }
    pub fn has_device(&self) -> bool {
        self.device.is_some()
    }
    pub fn capture_device_process(
        cpu: &mut dyn Cpu,
        process: &mut Process,
        point: ResumePoint,
        leader: bool,
        at: crate::checkpoint_domain::CheckpointTime,
    ) -> Result<ProcessCheckpoint> {
        let session = Self::capture_process(cpu, process, point, true, at)?;
        let shared = leader
            .then(|| process.state.capture_device_objects())
            .transpose()
            .map_err(anyhow::Error::msg)?;
        Ok(ProcessCheckpoint { session, shared })
    }
    pub fn assemble_device(
        mut processes: Vec<(u32, ProcessCheckpoint)>,
        foreground: u32,
    ) -> Result<Self> {
        processes.sort_unstable_by_key(|(pid, _)| *pid);
        anyhow::ensure!(
            !processes.is_empty() && processes.len() <= 128,
            "invalid checkpoint participant count"
        );
        let mut shared = None;
        for (pid, process) in &mut processes {
            anyhow::ensure!(
                *pid == process.session.process_id(),
                "checkpoint participant changed process identity"
            );
            if let Some(domain) = process.shared.take() {
                anyhow::ensure!(shared.is_none(), "multiple device checkpoint leaders");
                shared = Some(domain);
            }
        }
        let mut shared = shared.context("missing device checkpoint leader")?;
        let shared_audio = shared.shared_audio_keys();
        for (_, process) in &mut processes {
            shared.include_process_audio(process.session.kernel.take_device_audio(&shared_audio));
            process.session.kernel.omit_device_wait_objects();
        }
        let (_, first) = processes.remove(0);
        let mut root = first.session;
        let archive = DeviceArchive {
            shared,
            foreground,
            others: processes
                .into_iter()
                .map(|(_, process)| ChildSession {
                    session: process.session,
                    encoded: OnceLock::new(),
                })
                .collect(),
        };
        archive.validate(&root).map_err(anyhow::Error::msg)?;
        root.device = Some(archive);
        Ok(root)
    }
}

use std::{
    collections::BTreeSet,
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};
struct RestoreProgress {
    prepared: BTreeSet<u32>,
    restored: BTreeSet<u32>,
    adopted: BTreeSet<u32>,
    objects_ready: bool,
    committed: bool,
    resumed: Option<Instant>,
    error: Option<String>,
}
pub struct DeviceRestoreGroup {
    domain: pocket_kernel::DeviceRestoreDomain,
    progress: Mutex<RestoreProgress>,
    changed: Condvar,
    mappings: Mutex<Vec<pocket_kernel::file_mapping::MappingContents>>,
    deadline: Instant,
    leader: u32,
}
impl DeviceRestoreGroup {
    pub fn new(
        shared: pocket_kernel::DeviceSnapshot,
        foreground: u32,
        reference: &KernelState,
        timeout: Duration,
    ) -> Result<Self> {
        let domain = pocket_kernel::DeviceRestoreDomain::new(shared, reference)
            .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            domain.process_ids().contains(&foreground),
            "foreground process is missing from checkpoint"
        );
        let leader = *domain.process_ids().first().context("empty saved device")?;
        Ok(Self {
            domain,
            progress: Mutex::new(RestoreProgress {
                prepared: BTreeSet::new(),
                restored: BTreeSet::new(),
                adopted: BTreeSet::new(),
                objects_ready: false,
                committed: false,
                resumed: None,
                error: None,
            }),
            changed: Condvar::new(),
            mappings: Mutex::new(Vec::new()),
            deadline: Instant::now() + timeout,
            leader,
        })
    }
    pub fn process_table(&self, pid: u32) -> Option<pocket_kernel::handles::HandleTable> {
        self.domain.process_table(pid)
    }
    pub fn cancel(&self, error: impl Into<String>) {
        let mut state = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        if !state.committed {
            state.error.get_or_insert(error.into());
        }
        self.changed.notify_all();
    }
    fn wait(&self, phase: &str, ready: impl Fn(&RestoreProgress) -> bool) -> Result<()> {
        let mut state = self
            .progress
            .lock()
            .map_err(|_| anyhow::anyhow!("device restore coordinator poisoned"))?;
        loop {
            if let Some(error) = &state.error {
                anyhow::bail!("{error}");
            }
            if ready(&state) {
                return Ok(());
            }
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let error = format!("timed out while {phase} for all restored processes");
                state.error = Some(error.clone());
                self.changed.notify_all();
                anyhow::bail!(error);
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| anyhow::anyhow!("device restore coordinator poisoned"))?
                .0;
        }
    }
    pub fn restore_process(
        &self,
        emulator: &mut Emulator,
        saved: SessionSnapshot,
    ) -> Result<RestoredSession> {
        let result = self.restore_inner(emulator, saved);
        if let Err(error) = &result {
            self.cancel(format!("{error:#}"));
        }
        result
    }
    fn restore_inner(
        &self,
        emulator: &mut Emulator,
        saved: SessionSnapshot,
    ) -> Result<RestoredSession> {
        let pid = saved.process_id();
        anyhow::ensure!(
            self.domain.process_ids().contains(&pid)
                && saved.device.is_none()
                && saved.kernel.is_device_process(),
            "invalid process in device checkpoint"
        );
        let process = emulator
            .process
            .as_ref()
            .context("load the saved executable before restoring")?;
        saved
            .kernel
            .prime_device_restore(&process.state.vfs, &self.domain)
            .map_err(anyhow::Error::msg)?;
        {
            let mut state = self
                .progress
                .lock()
                .map_err(|_| anyhow::anyhow!("device restore coordinator poisoned"))?;
            anyhow::ensure!(state.prepared.insert(pid), "process entered restore twice");
            self.changed.notify_all();
        }
        self.wait("preparing mounts", |s| {
            s.prepared.len() == self.domain.process_ids().len()
        })?;
        let restored = {
            let _gate = self
                .domain
                .serial
                .lock()
                .map_err(|_| anyhow::anyhow!("device reconstruction gate poisoned"))?;
            self.wait("starting reconstruction", |_| true)?;
            let (restored, contents) =
                emulator.restore_snapshot_parts(saved, Some(&self.domain))?;
            emulator
                .process
                .as_mut()
                .unwrap()
                .state
                .vfs
                .defer_device_snapshot_files()
                .map_err(anyhow::Error::msg)?;
            if let Some(contents) = contents {
                self.mappings
                    .lock()
                    .map_err(|_| anyhow::anyhow!("pending section writes poisoned"))?
                    .push(contents);
            }
            restored
        };
        {
            let mut state = self
                .progress
                .lock()
                .map_err(|_| anyhow::anyhow!("device restore coordinator poisoned"))?;
            state.restored.insert(pid);
            self.changed.notify_all();
        }
        if pid == self.leader {
            self.wait("reconstructing resources", |s| {
                s.restored.len() == self.domain.process_ids().len()
            })?;
            self.domain.finish_objects().map_err(anyhow::Error::msg)?;
            let mut state = self
                .progress
                .lock()
                .map_err(|_| anyhow::anyhow!("device restore coordinator poisoned"))?;
            state.objects_ready = true;
            self.changed.notify_all();
        } else {
            self.wait("binding shared objects", |s| s.objects_ready)?;
        }
        self.domain
            .adopt_process(pid, &mut emulator.process.as_mut().unwrap().state)
            .map_err(anyhow::Error::msg)?;
        {
            let mut state = self
                .progress
                .lock()
                .map_err(|_| anyhow::anyhow!("device restore coordinator poisoned"))?;
            state.adopted.insert(pid);
            self.changed.notify_all();
        }
        if pid == self.leader {
            self.wait("adopting the shared domain", |s| {
                s.adopted.len() == self.domain.process_ids().len()
            })?;
            // Hold the phase lock through commit. A late cancellation cannot
            // release a peer midway through applying shared backing contents.
            let mut state = self
                .progress
                .lock()
                .map_err(|_| anyhow::anyhow!("device restore coordinator poisoned"))?;
            if let Some(error) = &state.error {
                anyhow::bail!("{error}");
            }
            let commit = (|| -> Result<()> {
                self.domain
                    .release_reconstruction_references()
                    .map_err(anyhow::Error::msg)?;
                let files = emulator
                    .process
                    .as_mut()
                    .unwrap()
                    .state
                    .vfs
                    .install_device_snapshot_files(&self.domain.files)
                    .map_err(anyhow::Error::msg)?;
                for contents in std::mem::take(
                    &mut *self
                        .mappings
                        .lock()
                        .map_err(|_| anyhow::anyhow!("pending section writes poisoned"))?,
                ) {
                    contents.install().map_err(anyhow::Error::msg)?;
                }
                files.commit();
                Ok(())
            })();
            match &commit {
                Ok(()) => {
                    state.resumed = Some(Instant::now());
                    state.committed = true;
                }
                Err(error) => {
                    state.error = Some(format!("{error:#}"));
                }
            }
            self.changed.notify_all();
            drop(state);
            commit?;
        } else {
            self.wait("committing the device checkpoint", |s| s.committed)?;
        }
        let mut restored = restored;
        restored.resumed = self
            .progress
            .lock()
            .map_err(|_| anyhow::anyhow!("device restore coordinator poisoned"))?
            .resumed;
        Ok(restored)
    }
}
