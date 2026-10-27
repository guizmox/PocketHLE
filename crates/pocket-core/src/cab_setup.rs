//! Deferred CAB registry setup, executed by the cabinet's own ARM DLL.
//! Only registry/time installers are supported here. Installers needing file
//! mutation, GUI, services or child processes remain unsupported, rather than
//! running partially against the read-only installed package (IonFx resources
//! have their separate extraction path in pocket-cab).
use crate::Emulator;
use anyhow::{ensure, Context, Result};
use pocket_kernel::registry::RegistryValue;
use pocket_pe::{ImportBinding, LoadedImage};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MARKERS: &str = r"HKCU\Software\PocketHLE\CabRegistrySetup";

fn supported(image: &LoadedImage) -> bool {
    image.machine == 0x1c0
        && image.imports.iter().all(|import| {
            if !import.dll.eq_ignore_ascii_case("coredll.dll") {
                return false;
            }
            let name = match &import.binding {
                ImportBinding::Name(name) => Some(name.clone()),
                ImportBinding::Ordinal(ord) => crate::winceapi::resolve_ordinal(&import.dll, *ord),
            };
            matches!(
                name.as_deref(),
                Some(
                    "RegCloseKey"
                        | "RegCreateKeyExW"
                        | "RegOpenKeyExW"
                        | "RegQueryValueExW"
                        | "RegSetValueExW"
                        | "RegDeleteValueW"
                        | "GetLocalTime"
                        | "GetSystemTime"
                        | "SystemTimeToFileTime"
                        | "FileTimeToSystemTime"
                        | "GetTickCount"
                        | "GetLastError"
                        | "SetLastError"
                        | "memcpy"
                        | "memset"
                        | "memcmp"
                        | "wcslen"
                )
            )
        })
}

fn setup_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("999"))
        {
            ensure!(
                entry.metadata()?.len() <= 8 * 1024 * 1024,
                "CAB setup DLL too large"
            );
            files.push(entry.path());
        }
    }
    files.sort();
    ensure!(files.len() <= 16, "Too many CAB setup DLL candidates");
    Ok(files)
}

impl Emulator {
    /// Complete a supported CAB's registry initialization before the first game
    /// instruction. Frontends call only for CAB imports, never Gizmondo/ZIPs.
    /// Returns the number of newly initialized setup DLLs. Registry changes
    /// commit only after successful callbacks; the DLL hash + install directory
    /// marker lives in the same persistent device registry as those changes.
    pub fn complete_cab_registry_setup(&mut self, root: &Path, install_dir: &str) -> Result<usize> {
        let mut completed = self.complete_cab_registry_setup_dir(root, install_dir)?;
        // The importer keeps add-on setup callbacks outside the guest mount.
        // Only its bounded, hash-named package directories are eligible here.
        let packages = root.parent().map(|parent| parent.join(".cab-setups"));
        if let Some(packages) =
            packages.filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()))
        {
            let order_path = packages.join("order.txt");
            let metadata = std::fs::symlink_metadata(&order_path)?;
            ensure!(
                metadata.is_file() && metadata.len() <= 8192,
                "Invalid add-on setup order metadata"
            );
            let order = std::fs::read_to_string(order_path)?;
            let mut entries = Vec::new();
            for name in order.lines() {
                ensure!(
                    name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit()),
                    "Invalid add-on setup identity"
                );
                let path = packages.join(name);
                ensure!(
                    std::fs::symlink_metadata(&path)?.is_dir(),
                    "Invalid add-on setup package directory"
                );
                ensure!(!entries.contains(&path), "Duplicate add-on setup package");
                entries.push(path);
                ensure!(entries.len() <= 64, "Too many add-on setup packages");
            }
            for directory in entries {
                let path = directory.join("install-dir.txt");
                let metadata = std::fs::symlink_metadata(&path)?;
                ensure!(
                    metadata.is_file() && metadata.len() <= 8192,
                    "Invalid add-on setup directory metadata"
                );
                let install_dir = std::fs::read_to_string(&path)?;
                ensure!(
                    install_dir.starts_with('\\')
                        && !install_dir.contains(['\0', '/'])
                        && install_dir
                            .split('\\')
                            .all(|c| !matches!(c, "." | "..") && !c.contains(':')),
                    "Invalid add-on WinCE installation directory"
                );
                completed += self.complete_cab_registry_setup_dir(&directory, &install_dir)?;
            }
        }
        Ok(completed)
    }

    fn complete_cab_registry_setup_dir(&mut self, root: &Path, install_dir: &str) -> Result<usize> {
        #[allow(unused_mut)]
        let mut completed = 0;
        for path in setup_files(root)? {
            let bytes = std::fs::read(&path)?;
            let Ok(image) = pocket_pe::load_bytes(&bytes) else {
                continue;
            };
            if !image.exports.contains_key("Install_Exit") {
                continue;
            }
            if !supported(&image) {
                log::debug!(
                    "CAB setup {} needs an unsupported installation capability",
                    path.display()
                );
                continue;
            }
            let mut hash = Sha256::new();
            hash.update(&bytes);
            hash.update(
                install_dir
                    .trim_end_matches('\\')
                    .to_ascii_lowercase()
                    .as_bytes(),
            );
            let marker = hash
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let registry = &self
                .process
                .as_ref()
                .context("No process loaded")?
                .state
                .registry;
            if registry.value(MARKERS, &marker) == Some(RegistryValue::Dword(1)) {
                continue;
            }
            #[cfg(not(feature = "unicorn"))]
            anyhow::bail!("CAB registry setup requires the Unicorn CPU backend");
            #[cfg(feature = "unicorn")]
            {
                // Loading a helper resets thread-local media backends. Preserve
                // the already loaded application's host session around it.
                let _session = crate::suspend_host_session();
                let baseline = registry.isolated_copy();
                let mut helper = Emulator::with_unicorn_cpu()?;
                helper.load_pe(&path)?;
                helper.process.as_mut().unwrap().state.registry = baseline.isolated_copy();
                helper.set_halt_on_unimplemented(true);
                helper.run_setup_callbacks(install_dir)?;
                let staged = &mut helper.process.as_mut().unwrap().state.registry;
                ensure!(
                    staged.set_value(MARKERS, &marker, RegistryValue::Dword(1)),
                    "CAB setup marker out of memory"
                );
                let registry = &mut self.process.as_mut().unwrap().state.registry;
                ensure!(
                    registry.commit_isolated(&baseline, staged),
                    "CAB setup registry conflict or insufficient object-store space"
                );
                registry
                    .flush()
                    .context("Saving CAB registry initialization")?;
                completed += 1;
                log::info!("Completed CAB registry setup {}", path.display());
            }
        }
        Ok(completed)
    }

    #[cfg(feature = "unicorn")]
    fn run_setup_callbacks(&mut self, install_dir: &str) -> Result<()> {
        use pocket_cpu::{regs::ArmReg, Prot};
        use pocket_kernel::{FrameAction, KernelState};
        // A separate mapped host harness calls the original DLL without changing
        // any commercial code/data. ARMv4 interworking call sequence, aligned SP,
        // all seven Install_Exit arguments (three on stack).
        const HARNESS: u32 = 0x6000_0000;
        const RESULT: u32 = HARNESS + 0x1000;
        const DIRECTORY: u32 = HARNESS + 0x1100;
        ensure!(
            install_dir.encode_utf16().count() < 1024,
            "CAB install directory too long"
        );
        self.cpu
            .map_region(HARNESS, 0x2000, Prot::READ | Prot::WRITE | Prot::EXEC)?;
        let process = self.process.as_mut().unwrap();
        let image = &process.image;
        let attach = (image.entry_point != 0).then(|| image.entry_va());
        let init = image
            .exports
            .get("Install_Init")
            .map(|rva| image.image_base + rva);
        let exit = image.image_base + image.exports["Install_Exit"];
        let code = harness(RESULT, DIRECTORY, image.image_base, attach, init, exit);
        self.cpu.write_mem(HARNESS, &code)?;
        self.cpu.write_mem(RESULT, &[0; 8])?;
        let directory: Vec<_> = install_dir
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        self.cpu.write_mem(DIRECTORY, &directory)?;
        process.image.entry_point = HARNESS.wrapping_sub(image.image_base);
        self.cpu.write_reg(ArmReg::Sp, process.stack_top - 16)?;
        self.cpu
            .write_reg(ArmReg::Lr, pocket_kernel::PROCESS_EXIT_TRAMPOLINE_VA)?;
        self.instruction_budget_per_slice = 50_000;
        self.max_slices = 512;
        let start = std::time::Instant::now();
        self.run_with_hook(&mut |_state: &mut KernelState| {
            if start.elapsed() > std::time::Duration::from_secs(2) {
                FrameAction::Stop
            } else {
                FrameAction::Continue
            }
        })?;
        let result = self.cpu.read_mem(RESULT, 4)?;
        ensure!(
            result == 1u32.to_le_bytes(),
            "CAB setup cancelled, failed or exceeded execution budget"
        );
        Ok(())
    }
}

// The bridge's result is published only after Install_Exit returns DONE and detach completes. Failure or
// cancellation still calls DLL_PROCESS_DETACH when attach succeeded.
#[cfg(feature = "unicorn")]
fn harness(
    result: u32,
    directory: u32,
    base: u32,
    attach: Option<u32>,
    init: Option<u32>,
    exit: u32,
) -> Vec<u8> {
    struct Arm(Vec<u32>);
    impl Arm {
        fn literal(&mut self, reg: u32, value: u32) {
            self.0.extend([0xe59f0000 | (reg << 12), 0xea000000, value]);
        }
        fn call(&mut self, address: u32, args: &[u32]) {
            for (index, &value) in args.iter().enumerate() {
                self.literal(if index < 4 { index as u32 } else { 12 }, value);
                if index >= 4 {
                    self.0.push(0xe58dc000 | ((index as u32 - 4) * 4));
                }
            }
            self.literal(12, address);
            self.0.extend([0xe1a0e00f, 0xe12fff1c]); // mov lr,pc; bx ip
        }
        fn jump(&mut self, condition: u32) -> usize {
            let at = self.0.len();
            self.0.push(condition | 0x0a000000);
            at
        }
        fn target(&mut self, at: usize, target: usize) {
            self.0[at] |= ((target as i32 - at as i32 - 2) as u32) & 0xffffff;
        }
    }
    let mut a = Arm(vec![0xe92d4010, 0xe24dd010]); // push {r4,lr}; sub sp,#16
    a.literal(4, result);
    let attach_failed = attach.map(|entry| {
        a.call(entry, &[base, 1, 0]);
        a.0.push(0xe3500000);
        a.jump(0) // eq: no detach
    });
    let init_failed = init.map(|entry| {
        a.call(entry, &[0, 1, 0, directory]);
        a.0.push(0xe3500000);
        a.jump(0x10000000)
    });
    a.call(exit, &[0, directory, 0, 0, 0, 0, 0]);
    a.0.extend([0xe3500000, 0x03a00001, 0x05840004]); // DONE -> result=1
    let detach = a.0.len();
    if let Some(at) = init_failed {
        a.target(at, detach);
    }
    if let Some(entry) = attach {
        a.call(entry, &[base, 0, 0]);
    }
    let end = a.0.len();
    if let Some(at) = attach_failed {
        a.target(at, end);
    }
    a.0.extend([0xe5940004, 0xe5840000, 0xe3a00000, 0xe28dd010, 0xe8bd8010]);
    a.0.into_iter().flat_map(u32::to_le_bytes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addon_setup_metadata_keeps_package_directory_and_rejects_bad_paths() {
        let root=std::env::temp_dir().join(format!("pockethle-addon-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let extracted=root.join("extracted");std::fs::create_dir_all(&extracted).unwrap();
        let mut emu=Emulator::with_stub_cpu();
        assert_eq!(emu.complete_cab_registry_setup(&extracted,r"\Program Files\Example").unwrap(),0);
        let id="0123456789abcdef0123456789abcdef";
        let packages=root.join(".cab-setups");let directory=packages.join(id);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(packages.join("order.txt"),format!("{id}\n")).unwrap();
        std::fs::write(directory.join("install-dir.txt"),r"\Program Files\Example\Tracks").unwrap();
        assert_eq!(emu.complete_cab_registry_setup(&extracted,r"\Program Files\Example").unwrap(),0);
        std::fs::write(directory.join("install-dir.txt"),r"\Program Files\Example\..\Other").unwrap();
        assert!(emu.complete_cab_registry_setup(&extracted,r"\Program Files\Example").is_err());
        std::fs::write(directory.join("install-dir.txt"),r"\Program Files\Example\Tracks").unwrap();
        std::fs::write(packages.join("order.txt"),"../escape\n").unwrap();
        assert!(emu.complete_cab_registry_setup(&extracted,r"\Program Files\Example").is_err());
        std::fs::write(packages.join("order.txt"),format!("{id}\n{id}\n")).unwrap();
        assert!(emu.complete_cab_registry_setup(&extracted,r"\Program Files\Example").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn fixture() -> LoadedImage {
        LoadedImage {
            source_path: "synthetic-setup".into(),
            machine: 0x1c0,
            subsystem: 9,
            image_base: 0x10000,
            size_of_image: 0x2000,
            stack_reserve: 0x10000,
            entry_point: 0x1000,
            sections: vec![pocket_pe::LoadedSection {
                name: ".text".into(),
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                characteristics: 0x60000020,
                data: vec![0; 0x1000],
            }],
            imports: vec![],
            exports: Default::default(),
            resources: vec![],
            managed_runtime: None,
        }
    }
    #[cfg(feature = "unicorn")]
    fn callback_fixture(init_result: u32, attach_result: u32) -> Emulator {
        let mut emu = Emulator::with_unicorn_cpu().unwrap();
        let mut image = fixture();
        image.exports.insert("Install_Init".into(), 0x1020);
        image.exports.insert("Install_Exit".into(), 0x1040);
        // DllMain returns supplied status; Init returns supplied enum. Exit
        // checks the seventh argument at sp+8 is zero, and returns it. Also
        // writes a sentinel proving it really executed.
        for (offset, words) in [
            (0usize, vec![0xe3a00000 | attach_result, 0xe12fff1e]),
            (0x20, vec![0xe3a00000 | init_result, 0xe12fff1e]),
            (
                0x40,
                vec![
                    0xe59d0008, 0xe59fc008, 0xe3a01077, 0xe58c1000, 0xe12fff1e, 0x60001008,
                ],
            ),
        ] {
            for (i, word) in words.iter().enumerate() {
                image.sections[0].data[offset + i * 4..offset + i * 4 + 4]
                    .copy_from_slice(&word.to_le_bytes());
            }
        }
        emu.process = Some(
            pocket_kernel::Process::map_into(
                image,
                emu.cpu.as_mut(),
                &|dll, ord| crate::winceapi::resolve_ordinal(dll, ord),
                &emu.dispatcher,
            )
            .unwrap(),
        );
        emu
    }
    #[cfg(feature = "unicorn")]
    #[test]
    fn arm_callbacks_exit_stack_success_cancel_and_attach_failure() {
        let mut emu = callback_fixture(0, 1);
        emu.run_setup_callbacks(r"\Program Files\Example").unwrap();
        assert_eq!(
            emu.read_guest_memory(0x60001008, 4).unwrap(),
            0x77u32.to_le_bytes()
        );
        for (init, attach) in [(1, 1), (0, 0)] {
            let mut emu = callback_fixture(init, attach);
            assert!(emu.run_setup_callbacks(r"\Program Files\Example").is_err());
            assert_eq!(emu.read_guest_memory(0x60001008, 4).unwrap(), [0; 4]);
        }
    }
    #[cfg(feature = "unicorn")]
    #[test]
    fn stalled_detach_cannot_commit_an_unfinished_setup() {
        let mut emu = callback_fixture(0, 1);
        // DllMain: attach succeeds; detach loops. Install_Exit succeeded,
        // but the host must still reject this incomplete lifecycle.
        let words: [u32; 4] = [0xe3510000, 0x0afffffe, 0xe3a00001, 0xe12fff1e];
        emu.write_guest_memory(
            0x11000,
            &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(emu.run_setup_callbacks(r"\Program Files\Example").is_err());
        assert_eq!(
            emu.read_guest_memory(0x60001004, 4).unwrap(),
            1u32.to_le_bytes()
        );
        assert_eq!(emu.read_guest_memory(0x60001000, 4).unwrap(), [0; 4]);
    }
    #[test]
    fn capability_gate_requires_registry_only_arm_setup() {
        // Fixtures are synthetic; no commercial binaries in unit tests.
        let mut image = fixture();
        assert!(supported(&image));
        image.imports.push(pocket_pe::ImportSymbol {
            dll: "coredll.dll".into(),
            binding: ImportBinding::Name("CreateFileW".into()),
            iat_va: 0,
        });
        assert!(!supported(&image));
    }
}
