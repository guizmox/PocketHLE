//! Read companion GLES export metadata without executing the native driver.
use pocket_pe::{ImportBinding, LoadedImage};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
pub type Tables = HashMap<String, HashMap<u16, String>>;

pub fn local_tables(executable: &Path, image: &LoadedImage) -> Result<Tables, String> {
    let mut tables = Tables::new();
    let directory = executable
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let dlls: HashSet<_> = image
        .imports
        .iter()
        .filter(|i| pocket_gles::ordinals::is_gles_dll(&i.dll))
        .map(|i| i.dll.to_ascii_lowercase())
        .collect();
    for dll in dlls {
        // Only companion files in the importing image's directory are evidence.
        if dll.contains(['/', '\\', ':']) {
            return Err("Invalid GLES import filename".into());
        }
        let mut candidates = Vec::new();
        for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(&dll)
                && entry.path().is_file()
            {
                candidates.push(entry.path());
            }
        }
        if candidates.is_empty() {
            continue;
        }
        if candidates.len() != 1 {
            return Err(format!("Ambiguous companion GLES DLL: {dll}"));
        }
        let path = &candidates[0];
        let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if metadata.len() > 16 * 1024 * 1024 {
            return Err(format!(
                "Companion GLES DLL is too large: {}",
                path.display()
            ));
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let mut driver = pocket_pe::load_bytes(&bytes)
            .map_err(|e| format!("Invalid companion {}: {e}", path.display()))?;
        let arm = |m| {
            matches!(
                m,
                pocket_pe::machine::ARM | pocket_pe::machine::THUMB | pocket_pe::machine::ARMNT
            )
        };
        if driver.machine != image.machine && !(arm(driver.machine) && arm(image.machine)) {
            return Err(format!(
                "Companion GLES DLL has a different CPU architecture: {}",
                path.display()
            ));
        }
        let base = driver.image_base;
        // At its original base this validates exports without relocating or running code.
        pocket_pe::prepare_runtime_module(&mut driver, &bytes, base)
            .map_err(|e| format!("Invalid companion GLES exports {}: {e}", path.display()))?;
        let mut table = HashMap::new();
        for (ordinal, rva) in &driver.exports {
            let Some(n) = ordinal
                .strip_prefix('#')
                .and_then(|s| s.parse::<u16>().ok())
            else {
                continue;
            };
            let names: Vec<_> = driver
                .exports
                .iter()
                .filter(|(name, target)| !name.starts_with('#') && *target == rva)
                .map(|(n, _)| n.clone())
                .collect();
            if names.len() == 1 {
                table.insert(n, names[0].clone());
            }
        }
        // An existing DLL is authoritative: missing/ambiguous ordinals must not
        // silently fall back to another vendor's incompatible ABI.
        for import in image
            .imports
            .iter()
            .filter(|i| i.dll.eq_ignore_ascii_case(&dll))
        {
            if let ImportBinding::Ordinal(n) = import.binding {
                if !table.contains_key(&n) {
                    return Err(format!(
                        "Missing or ambiguous export {dll}!#{n} in {}",
                        path.display()
                    ));
                }
            }
        }
        log::info!(
            "GLES ordinal metadata: {} ({} named ordinals; driver code is not loaded)",
            path.display(),
            table.len()
        );
        tables.insert(dll, table);
    }
    Ok(tables)
}

pub fn resolve(tables: &Tables, dll: &str, ordinal: u16, arch: pocket_cpu::Arch) -> Option<String> {
    match tables.get(&dll.to_ascii_lowercase()) {
        Some(table) => table.get(&ordinal).cloned(),
        None => crate::resolve_ordinal_for_arch(dll, ordinal, arch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DRIVER: &[u8] = include_bytes!("../tests/data/gles-export-metadata.pe");
    fn image(dll: &str, ordinal: u16) -> LoadedImage {
        let mut image = pocket_pe::load_bytes(DRIVER).unwrap();
        image.imports = vec![pocket_pe::ImportSymbol {
            dll: dll.into(),
            binding: ImportBinding::Ordinal(ordinal),
            iat_va: 0x1000,
        }];
        image
    }
    #[test]
    fn local_cm_and_cl_use_export_names_without_running_driver() {
        for dll in ["libGLES_CM.dll", "libGLES_CL.dll"] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(dll), DRIVER).unwrap();
            let tables = local_tables(&dir.path().join("game.exe"), &image(dll, 14)).unwrap();
            assert_eq!(
                resolve(&tables, dll, 14, pocket_cpu::Arch::Arm).as_deref(),
                Some("eglGetDisplay")
            );
            assert_eq!(
                resolve(&tables, dll, 17, pocket_cpu::Arch::Arm).as_deref(),
                Some("eglInitialize")
            );
            assert_eq!(resolve(&tables, dll, 99, pocket_cpu::Arch::Arm), None);
        }
    }
    #[test]
    fn no_local_dll_retains_existing_cm_and_cl_tables() {
        let dir = tempfile::tempdir().unwrap();
        for dll in ["libGLES_CM.dll", "libGLES_CL.dll"] {
            let tables = local_tables(&dir.path().join("game.exe"), &image(dll, 14)).unwrap();
            assert!(tables.is_empty());
            assert_eq!(
                resolve(&tables, dll, 14, pocket_cpu::Arch::Arm),
                crate::resolve_ordinal_for_arch(dll, 14, pocket_cpu::Arch::Arm)
            );
        }
    }
    #[test]
    fn malformed_missing_and_ambiguous_local_exports_never_guess() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("libGLES_CM.dll");
        let exe = dir.path().join("game.exe");
        std::fs::write(&path, b"not a DLL").unwrap();
        assert!(local_tables(&exe, &image("libGLES_CM.dll", 14)).is_err());
        std::fs::write(&path, DRIVER).unwrap();
        assert!(local_tables(&exe, &image("libGLES_CM.dll", 99)).is_err());
        let mut ambiguous = DRIVER.to_vec();
        ambiguous[0x344..0x348].copy_from_slice(&0x1000u32.to_le_bytes());
        std::fs::write(&path, ambiguous).unwrap();
        assert!(local_tables(&exe, &image("libGLES_CM.dll", 14)).is_err());
    }
}
