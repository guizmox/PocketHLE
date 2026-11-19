//! Capture only between complete IL callbacks; restore never re-executes Main.
use super::*;
use crate::vm::ObjectGraph;
use pocket_kernel::framebuffer::PixelSnapshot;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, Instant},
};
const MAGIC: &[u8; 8] = b"PHCFSE01";
#[derive(Serialize, Deserialize)]
pub struct ManagedSnapshot {
    version: u16,
    executable: [u8; 32],
    environment: BTreeMap<String, String>,
    program_path: String,
    elapsed_ms: u64,
    machine: vm::MachineSnapshot,
    forms: forms::Forms,
    graphics: graphics::GraphicsSnapshot,
    system: system::Services,
    sensors: sensors::Sensors,
    strings: Vec<String>,
    streams: Vec<Option<(usize, usize)>>,
    storage: storage::StorageSnapshot,
    tables: BTreeMap<usize, BTreeSet<u32>>,
    resource_types: BTreeMap<usize, u32>,
    pen_widths: BTreeMap<usize, u32>,
    drawing_colors: BTreeMap<usize, u16>,
    fmod: crate::fmod::Fmod,
    audio: pocket_kernel::audio::AudioSnapshot,
    data_slots: Vec<(String, vm::Value)>,
    forms_frame: Option<(u64, PixelSnapshot)>,
    forms_canvas: Option<PixelSnapshot>,
    next_collection: u64,
    uptime: Duration,
}
fn identity(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}
impl ManagedSnapshot {
    pub fn write(&self, output: impl Write) -> std::io::Result<()> {
        pocket_snapshot::write_document(output, MAGIC, self)
    }
    pub fn read(input: impl Read) -> std::io::Result<Self> {
        let saved: Self = pocket_snapshot::read_document(input, MAGIC)?;
        if saved.version != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unsupported managed checkpoint version",
            ));
        }
        saved
            .validate()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(saved)
    }
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
    pub fn executable(&self) -> [u8; 32] {
        self.executable
    }
    pub fn program_path(&self) -> &str {
        &self.program_path
    }
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }
    fn validate(&self) -> Result<(), String> {
        self.machine.validate()?;
        self.graphics.validate()?;
        if !self.system.validate_checkpoint(&self.machine)
            || !self.storage.validate_checkpoint(&self.machine)
            || self
                .tables
                .keys()
                .chain(self.resource_types.keys())
                .chain(self.pen_widths.keys())
                .chain(self.drawing_colors.keys())
                .any(|&id| !self.machine.live_object(id))
        {
            return Err("Invalid managed service identity".into());
        }
        let unique: BTreeSet<_> = self.strings.iter().collect();
        if unique.len() != self.strings.len()
            || self
                .strings
                .iter()
                .try_fold(0usize, |n, s| n.checked_add(s.len()))
                .is_none_or(|n| n > 64 * 1024 * 1024)
        {
            return Err("Invalid managed string table".into());
        }
        let valid_value = |value| match value {
            vm::Value::Image(id) => self.forms.valid_image(id),
            vm::Value::HostString(id) => id < self.strings.len(),
            vm::Value::ResourceStream(id) => id < self.streams.len(),
            vm::Value::DataSlot(id) => id < self.data_slots.len(),
            _ => self.machine.validate_value(value),
        };
        self.forms.validate_checkpoint(&self.machine, valid_value)?;
        if !self.machine.all_values(valid_value)
            || !self.data_slots.iter().all(|(_, v)| valid_value(*v))
        {
            return Err("Invalid managed service reference".into());
        }
        if self.streams.iter().flatten().any(|&(offset, len)| {
            offset
                .checked_add(len)
                .is_none_or(|end| end > 32 * 1024 * 1024)
        }) {
            return Err("Invalid managed resource range".into());
        }
        Ok(())
    }
    pub fn restore<'a>(
        self,
        bytes: &'a [u8],
        size: (u32, u32),
        save_root: Option<PathBuf>,
        asset_root: Option<PathBuf>,
        gpu: (bool, u8),
    ) -> Result<Session<'a>, String> {
        if self.executable != identity(bytes) {
            return Err("Managed checkpoint executable has changed".into());
        }
        let assembly = Assembly::read(bytes).ok_or("Invalid managed assembly")?;
        self.validate()?;
        let machine = self.machine.restore()?;
        let mut strings = Vec::new();
        let mut string_ids = BTreeMap::new();
        let mut string_bytes = 0usize;
        for text in self.strings {
            string_bytes = string_bytes
                .checked_add(text.len())
                .filter(|&n| n <= 64 * 1024 * 1024)
                .ok_or("Managed string budget exceeded")?;
            let text: std::rc::Rc<str> = text.into();
            if string_ids.insert(text.clone(), strings.len()).is_some() {
                return Err("Duplicate managed string identity".into());
            }
            strings.push(text);
        }
        let streams = self
            .streams
            .into_iter()
            .map(|range| {
                range
                    .map(|(offset, len)| {
                        bytes
                            .get(
                                offset
                                    ..offset
                                        .checked_add(len)
                                        .ok_or("Invalid managed resource range")?,
                            )
                            .ok_or("Invalid managed resource range")
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut assets = pocket_kernel::vfs::Vfs::new();
        if let Some((root, (directory, _))) =
            asset_root.zip(self.program_path.rsplit_once(['\\', '/']))
        {
            assets.mount_read_only(directory, root);
        }
        let started = Instant::now()
            .checked_sub(self.uptime)
            .ok_or("Invalid managed uptime")?;
        let graphics = self.graphics.restore()?;
        if graphics.display_size() != size || graphics.render_policy() != gpu {
            return Err("Managed checkpoint graphics settings differ".into());
        }
        let forms_frame = self
            .forms_frame
            .map(|(generation, image)| Ok::<_, String>((generation, image.restore()?)))
            .transpose()?;
        let forms_canvas = self.forms_canvas.map(PixelSnapshot::restore).transpose()?;
        Ok(Session {
            startup: Startup {
                assembly,
                unresolved: None,
                unresolved_type: None,
                streams,
                system: self.system,
                forms: self.forms,
                graphics,
                sensors: self.sensors,
                strings,
                string_ids,
                string_bytes,
                program_path: self.program_path,
                storage: self.storage.restore(save_root)?,
                started,
                tables: self.tables,
                resource_types: self.resource_types,
                pen_widths: self.pen_widths,
                drawing_colors: self.drawing_colors,
                audio: self.audio.restore()?,
                fmod: self.fmod,
                assets,
                data_slots: self.data_slots,
            },
            forms_frame,
            forms_canvas,
            next_collection: self.next_collection,
            machine,
        })
    }
}
impl Session<'_> {
    pub fn checkpoint(
        &self,
        elapsed_ms: u64,
        environment: BTreeMap<String, String>,
    ) -> Result<ManagedSnapshot, String> {
        let startup = &self.startup;
        let bytes = startup.assembly.image();
        let base = bytes.as_ptr() as usize;
        let streams = startup
            .streams
            .iter()
            .map(|slice| {
                slice
                    .map(|slice| {
                        let offset = (slice.as_ptr() as usize)
                            .checked_sub(base)
                            .ok_or("Managed resource is outside its assembly")?;
                        if offset
                            .checked_add(slice.len())
                            .is_none_or(|end| end > bytes.len())
                        {
                            return Err("Managed resource is outside its assembly");
                        }
                        Ok((offset, slice.len()))
                    })
                    .transpose()
            })
            .collect::<Result<_, _>>()?;
        let saved = ManagedSnapshot {
            version: 1,
            executable: identity(bytes),
            environment,
            program_path: startup.program_path.clone(),
            elapsed_ms,
            machine: self.machine.checkpoint()?,
            forms: startup.forms.clone(),
            graphics: startup.graphics.checkpoint()?,
            system: startup.system.clone(),
            sensors: startup.sensors.clone(),
            strings: startup.strings.iter().map(|s| s.to_string()).collect(),
            streams,
            storage: startup.storage.checkpoint()?,
            tables: startup.tables.clone(),
            resource_types: startup.resource_types.clone(),
            pen_widths: startup.pen_widths.clone(),
            drawing_colors: startup.drawing_colors.clone(),
            fmod: startup.fmod.clone(),
            audio: startup.audio.capture_snapshot()?,
            data_slots: startup.data_slots.clone(),
            forms_frame: self
                .forms_frame
                .as_ref()
                .map(|(generation, frame)| (*generation, PixelSnapshot::capture(frame))),
            forms_canvas: self.forms_canvas.as_ref().map(PixelSnapshot::capture),
            next_collection: self.next_collection,
            uptime: startup.started.elapsed(),
        };
        saved.validate()?;
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::Options;
    #[test]
    #[ignore = "requires supplied POCKETHLE_DIAMOND_BEER_EXE and a native GL driver"]
    fn diamond_beer_cold_checkpoint_continues_without_replaying_main() {
        let es = std::env::var_os("POCKETHLE_NETCF_TEST_HOST_ES")
            .map(|_| pocket_gles::gpu::ProgrammableHost::new_programmable().unwrap());
        if let Some(host) = &es {
            assert!(host.es, "the test requires a genuine ES source");
        }
        let source = es.as_ref().map(|h| h.shared_source());
        pocket_gles::gpu::with_shared_context_source(source, cold_checkpoint);
    }
    fn cold_checkpoint() {
        let executable = PathBuf::from(std::env::var_os("POCKETHLE_DIAMOND_BEER_EXE").unwrap());
        let bytes = std::fs::read(&executable).unwrap();
        let saves = tempfile::tempdir().unwrap();
        if let Some(settings) = std::env::var_os("POCKETHLE_DIAMOND_BEER_SETTINGS") {
            std::fs::copy(settings, saves.path().join("Settings.xml")).unwrap();
        }
        let gpu = std::env::var_os("POCKETHLE_NETCF_TEST_GPU").is_some();
        let mut session = Session::start_in(
            &bytes,
            (240, 320),
            500_000,
            Some(saves.path().to_path_buf()),
            None,
            (gpu, 1),
            executable.parent().map(PathBuf::from),
        )
        .unwrap();
        if gpu {
            assert!(
                session.startup.graphics.gpu_active(),
                "the game must use its native GPU context"
            );
        }
        session.set_acceleration([1.0, 9.0, 2.0]);
        for t in (0..2500).step_by(25) {
            session.tick(t, 500_000).unwrap();
        }
        let environment = BTreeMap::from([("Device".into(), "Pocket PC".into())]);
        let mut before = session.frame().expect("live game frame").2.to_vec();
        assert!(before.chunks_exact(2).any(|p| p != [0, 0]));
        let mut vm = pocket_snapshot::codec()
            .serialize(&session.machine.checkpoint().unwrap())
            .unwrap();
        let mut services = pocket_snapshot::codec()
            .serialize(&session.startup.system)
            .unwrap();
        let mut encoded = Vec::new();
        session
            .checkpoint(2500, environment.clone())
            .unwrap()
            .write(&mut encoded)
            .unwrap();
        assert!(ManagedSnapshot::read(&encoded[..encoded.len() / 2]).is_err());
        drop(session);
        for cycle in 0..2 {
            let saved = ManagedSnapshot::read(encoded.as_slice()).unwrap();
            assert_eq!(saved.environment(), &environment);
            let mut resumed = saved
                .restore(
                    &bytes,
                    (240, 320),
                    Some(saves.path().to_path_buf()),
                    executable.parent().map(PathBuf::from),
                    (gpu, 1),
                )
                .unwrap();
            assert_eq!(
                resumed.frame().unwrap().2,
                before.as_slice(),
                "frame after cold restore {cycle}"
            );
            assert_eq!(
                pocket_snapshot::codec()
                    .serialize(&resumed.machine.checkpoint().unwrap())
                    .unwrap(),
                vm,
                "VM after cold restore {cycle}"
            );
            assert_eq!(
                pocket_snapshot::codec()
                    .serialize(&resumed.startup.system)
                    .unwrap(),
                services
            );
            resumed.set_acceleration([-3.0, 7.5, 1.0]);
            for t in (2500 + cycle * 500..3000 + cycle * 500).step_by(25) {
                resumed.tick(t, 500_000).unwrap();
            }
            assert!(resumed.running());
            assert!(resumed.frame().is_some());
            // Save the restored state again, not the initial image or a replay log.
            let mut again = Vec::new();
            resumed
                .checkpoint(3000 + cycle * 500, environment.clone())
                .unwrap()
                .write(&mut again)
                .unwrap();
            let recaptured = ManagedSnapshot::read(again.as_slice()).unwrap();
            assert_eq!(recaptured.executable(), identity(&bytes));
            before = resumed.frame().unwrap().2.to_vec();
            vm = pocket_snapshot::codec()
                .serialize(&resumed.machine.checkpoint().unwrap())
                .unwrap();
            services = pocket_snapshot::codec()
                .serialize(&resumed.startup.system)
                .unwrap();
            encoded = again;
        }
    }
}
