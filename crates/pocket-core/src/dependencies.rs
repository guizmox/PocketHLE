//! Read-only PocketPC import diagnostics. This is not a compatibility verdict:
//! static imports may be unused, and GetProcAddress imports are only known at run time.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        (entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
            && entry.path().is_file())
        .then(|| entry.path())
    })
}

/// Uses the real dispatcher registrations, never a separate supported-API list.
/// Native DLL presence is reported separately from verified exported entry points.
pub fn information(executable: &Path, library_root: &Path) -> Vec<(String, String)> {
    let image = match pocket_pe::load_file(executable) {
        Ok(image) => image,
        Err(error) => {
            return vec![(
                "Dependencies".into(),
                format!("Cannot inspect executable: {error}"),
            )]
        }
    };
    let registry = pocket_winceapi::WinCeDispatcher::capability_registry();
    let shared = library_root.join("runtimes").join("pocketpc");
    let mut imports = BTreeMap::<String, Vec<_>>::new();
    for import in &image.imports {
        imports
            .entry(import.dll.to_ascii_lowercase())
            .or_default()
            .push(&import.binding);
    }
    let mut rows = vec![
        ("Shared PocketPC runtimes".into(), shared.display().to_string()),
        ("Scope".into(), "Static imports only; unused imports may be listed. Native DLL dependencies and dynamically requested APIs are checked when used. This is not a compatibility verdict.".into()),
    ];
    for (dll, bindings) in imports {
        let missing: Vec<_> = bindings
            .iter()
            .filter(|binding| !registry.supports_import(&dll, binding))
            .collect();
        let local = executable.parent().and_then(|dir| find_file(dir, &dll));
        let native = local.or_else(|| find_file(&shared, &dll));
        let mut status = format!(
            "{}/{} imports registered in PocketHLE",
            bindings.len() - missing.len(),
            bindings.len()
        );
        if let Some(path) = native {
            status.push_str(&format!(
                ". Native file: {} (loaded and validated when used)",
                path.display()
            ));
        } else if !missing.is_empty() {
            let names = missing
                .iter()
                .take(12)
                .map(|b| b.to_string_short())
                .collect::<Vec<_>>()
                .join(", ");
            status.push_str(&format!(
                ". No native file found. Unregistered imports: {names}"
            ));
            if missing.len() > 12 {
                status.push_str(&format!(" (+{} more)", missing.len() - 12));
            }
        }
        rows.push((dll, status));
    }
    rows
}
