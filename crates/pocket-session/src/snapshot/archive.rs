//! Backend-neutral persistence and verified cold-launch targets.
use super::{Host, ProcessSnapshots, Reply};
use crate::LaunchContext;
use pocket_core::SessionSnapshot;
use std::{
    path::PathBuf,
    sync::mpsc::{channel, Receiver},
    time::Instant,
};

/// Fully decoded archive shared by both frontends and both execution engines.
pub(super) enum Archive {
    Native(SessionSnapshot),
    Managed(pocket_netcf::snapshot::ManagedSnapshot),
}
impl Archive {
    pub fn program_path(&self) -> &str {
        match self {
            Self::Native(s) => s.program_path(),
            Self::Managed(s) => s.program_path(),
        }
    }
    fn engine(&self) -> crate::compatibility::Engine {
        match self {
            Self::Native(_) => crate::compatibility::Engine::Arm,
            Self::Managed(_) => crate::compatibility::Engine::CompactFramework,
        }
    }
    fn environment(&self) -> &std::collections::BTreeMap<String, String> {
        match self {
            Self::Native(s) => s.environment(),
            Self::Managed(s) => s.environment(),
        }
    }
    fn identities(&self) -> Vec<(&str, [u8; 32])> {
        match self {
            Self::Native(s) => s.executable_identities(),
            Self::Managed(s) => vec![(s.program_path(), s.executable())],
        }
    }
    fn read(mut input: impl std::io::Read) -> std::io::Result<Self> {
        use std::io::Read;
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        let stream = std::io::Cursor::new(magic).chain(input);
        match &magic {
            b"PHSECT01" => SessionSnapshot::read(stream).map(Self::Native),
            b"PHCFSE01" => pocket_netcf::snapshot::ManagedSnapshot::read(stream).map(Self::Managed),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unsupported snapshot execution engine",
            )),
        }
    }
    fn write(&self, output: &mut dyn std::io::Write) -> std::io::Result<()> {
        match self {
            Self::Native(s) => s.write(output),
            Self::Managed(s) => s.write(output),
        }
    }
    pub fn install(self, process: &mut ProcessSnapshots) {
        match self {
            Self::Native(s) => process.restore = Some(s),
            Self::Managed(s) => process.managed_restore = Some(s),
        }
    }
}
/// No native handles enter this worker. Slot replacement and replies are shared.
pub(super) fn write_checkpoint(path: PathBuf, snapshot: Archive, replies: Vec<Reply>) {
    let worker_replies = replies.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("pockethle-snapshot".into())
        .spawn(move || {
            let started = Instant::now();
            let result =
                pocket_library::snapshot_slot::save(&path, |output| snapshot.write(output))
                    .map_err(|e| e.to_string());
            log::debug!(
                "Snapshot compression and write took {:?}",
                started.elapsed()
            );
            for reply in worker_replies {
                let _ = reply.send(result.clone());
            }
        })
    {
        for reply in replies {
            let _ = reply.send(Err(format!("Cannot start snapshot writer: {error}")));
        }
    }
}
/// The frontend only drains its inputs. All checkpoint request handling and
/// completion/error delivery stay in this shared coordinator.
pub fn queue_managed_captures(
    host: &mut impl Host,
    captures: &mut Vec<pocket_netcf::host::CaptureRequest>,
) {
    if host.snapshots().requests.is_empty() {
        return;
    }
    let replies = std::mem::take(&mut host.snapshots().requests);
    match host.launch_context() {
        Some(context) => captures.push(managed_request(context, replies)),
        None => {
            for reply in replies {
                let _ = reply.send(Err("Missing managed launch context".into()));
            }
        }
    }
}
/// Managed callbacks capture on their owning emulation thread, then use the
/// same background writer/atomic slot as native processes.
fn managed_request(
    context: LaunchContext,
    replies: Vec<Reply>,
) -> pocket_netcf::host::CaptureRequest {
    let environment =
        crate::compatibility::environment(&context, crate::compatibility::Engine::CompactFramework);
    let path = pocket_library::snapshot_slot::path_for(&context.library, &context.game);
    pocket_netcf::host::CaptureRequest::new(environment, move |captured| match captured {
        Ok(snapshot) => write_checkpoint(path, Archive::Managed(snapshot), replies),
        Err(error) => {
            for reply in replies {
                let _ = reply.send(Err(error.clone()));
            }
        }
    })
}
fn load(context: &LaunchContext) -> Result<Option<(u64, Archive)>, String> {
    let path = pocket_library::snapshot_slot::path_for(&context.library, &context.game);
    pocket_library::snapshot_slot::open(&path)
        .and_then(|slot| {
            slot.map(|(time, payload)| Archive::read(payload).map(|saved| (time, saved)))
                .transpose()
        })
        .map_err(|e| e.to_string())
}
/// Retain exactly the archive validated before stopping the running game.
pub struct PreparedRestore {
    timestamp: u64,
    archive: Archive,
    executable: PathBuf,
    context: LaunchContext,
}
pub struct RestoreTarget {
    pub timestamp: u64,
    pub executable: PathBuf,
    pub context: LaunchContext,
}
impl PreparedRestore {
    pub fn install(self, process: &mut ProcessSnapshots) -> RestoreTarget {
        process.launch_config = Some(self.context.config.clone());
        self.archive.install(process);
        RestoreTarget {
            timestamp: self.timestamp,
            executable: self.executable,
            context: self.context,
        }
    }
}
impl std::fmt::Debug for PreparedRestore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedRestore")
            .field("timestamp", &self.timestamp)
            .field("executable", &self.executable)
            .finish_non_exhaustive()
    }
}
pub fn prepare(context: &LaunchContext) -> Result<Option<PreparedRestore>, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let Some((timestamp, archive)) = load(context)? else {
        return Ok(None);
    };
    crate::compatibility::validate(
        archive.environment(),
        &crate::compatibility::environment(context, archive.engine()),
    )?;
    for (guest, expected) in archive.identities() {
        let path = super::resolve_program(&context.library, &context.game, &context.card, guest)?;
        let mut file = std::fs::File::open(&path)
            .map_err(|e| format!("Cannot read saved executable {guest}: {e}"))?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            digest.update(&buffer[..n]);
        }
        let actual: [u8; 32] = digest.finalize().into();
        if actual != expected {
            return Err(format!("Snapshot executable has changed: {guest}"));
        }
    }
    let executable = super::resolve_program(
        &context.library,
        &context.game,
        &context.card,
        archive.program_path(),
    )?;
    Ok(Some(PreparedRestore {
        timestamp,
        archive,
        executable,
        context: context.clone(),
    }))
}

/// Both GUIs validate off the UI/emulation threads. Dropping the receiver
/// cancels delivery; a completed archive owns bytes, never native resources.
pub type Preparation = Receiver<Result<Option<PreparedRestore>, String>>;
pub fn prepare_async(context: LaunchContext) -> Result<Preparation, String> {
    let (reply, receiver) = channel();
    std::thread::Builder::new()
        .name("pockethle-snapshot-verify".into())
        .spawn(move || {
            let _ = reply.send(prepare(&context));
        })
        .map_err(|e| format!("Cannot start snapshot verification: {e}"))?;
    Ok(receiver)
}
