//! Portable library backups. Call only while games/imports/settings writes are stopped.
//! Restore validates and builds a complete sibling tree before publishing it.
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::{write::FileOptions, ZipArchive, ZipWriter};

const MARKER: &str = "__pockethle_backup__.json";
const MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreMode {
    MergeNoOverwrite,
    MergeOverwrite,
    Replace,
}

pub fn suggested_filename() -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("PocketHLE-backup-{timestamp}.zip")
}

/// Write atomically; a failed export never leaves a truncated destination ZIP.
pub fn save(root: &Path, destination: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    validate_library(&root)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize()?;
    if parent.starts_with(&root) {
        bail!("Save the backup outside the PocketHLE library");
    }
    let mut output = tempfile::NamedTempFile::new_in(&parent)?;
    {
        let mut zip = ZipWriter::new(output.as_file_mut());
        zip.start_file(MARKER, options())?;
        zip.write_all(b"{\"format\":\"PocketHLE\",\"version\":1}")?;
        pack(&mut zip, &root, &root)?;
        zip.finish()?;
    }
    output.as_file_mut().sync_all()?;
    output.persist(destination).map_err(|e| e.error)?;
    Ok(())
}

fn options() -> FileOptions {
    FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600)
}

fn pack<W: Write + std::io::Seek>(zip: &mut ZipWriter<W>, root: &Path, dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        let relative = path.strip_prefix(root)?;
        let name = relative
            .components()
            .map(|component| {
                let name = component
                    .as_os_str()
                    .to_str()
                    .context("Backup paths must be UTF-8")?;
                if name.contains('\\') {
                    bail!("Non-portable filename: {name:?}");
                }
                Ok(name)
            })
            .collect::<Result<Vec<_>>>()?
            .join("/");
        safe_path(&name)?;
        if name == MARKER {
            bail!("Reserved backup filename: {MARKER}");
        }
        if ty.is_symlink() {
            bail!("Cannot back up symbolic link: {}", path.display());
        }
        if ty.is_dir() {
            zip.add_directory(format!("{name}/"), options())?;
            pack(zip, root, &path)?;
        } else if ty.is_file() {
            zip.start_file(name, options())?;
            std::io::copy(&mut File::open(path)?, zip)?;
        } else {
            bail!("Unsupported file: {}", path.display());
        }
    }
    Ok(())
}

/// Accept only portable relative paths, independently of the host platform.
fn safe_path(name: &str) -> Result<PathBuf> {
    let name = name.strip_suffix('/').unwrap_or(name);
    if name.is_empty() || name.contains(['\\', ':', '\0']) {
        bail!("Invalid backup path: {name:?}");
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' ']) {
            bail!("Unsafe backup path: {name:?}");
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            bail!("Non-portable backup path: {name:?}");
        }
    }
    Ok(PathBuf::from(name))
}

fn unpack(archive: &Path, target: &Path) -> Result<()> {
    let mut zip = ZipArchive::new(File::open(archive)?)?;
    if zip.len() > MAX_ENTRIES {
        bail!("Too many backup entries");
    }
    let mut paths = HashSet::new();
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let relative = safe_path(entry.name())?;
        let key = relative.to_string_lossy().to_lowercase();
        if !paths.insert(key) {
            bail!("Duplicate backup path: {}", entry.name());
        }
        if let Some(mode) = entry.unix_mode() {
            let ty = mode & 0o170000;
            if ty != 0 && ty != 0o100000 && ty != 0o040000 {
                bail!("Links and special files are not supported");
            }
        }
        total = total
            .checked_add(entry.size())
            .context("Backup too large")?;
        if total > MAX_BYTES {
            bail!("Backup exceeds 64 GiB uncompressed");
        }
        if entry.name() == MARKER {
            let mut text = String::new();
            entry.take(4097).read_to_string(&mut text)?;
            let marker: serde_json::Value = serde_json::from_str(&text)?;
            if marker["format"] != "PocketHLE" || marker["version"] != 1 {
                bail!("Unsupported backup format");
            }
            continue;
        }
        let path = target.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(path)?;
        } else {
            fs::create_dir_all(path.parent().context("Missing parent")?)?;
            let mut output = File::options().write(true).create_new(true).open(path)?;
            let expected = entry.size();
            // Reading to EOF also verifies ZIP CRCs. Bound dishonest ZIP sizes.
            let copied = std::io::copy(
                &mut entry.by_ref().take(expected.saturating_add(1)),
                &mut output,
            )?;
            if copied != expected {
                bail!("Invalid ZIP entry size");
            }
        }
    }
    validate_library(target)
}

fn validate_library(root: &Path) -> Result<()> {
    let library: super::LibraryFile =
        serde_json::from_reader(File::open(root.join("library.json"))?)
            .context("Not a PocketHLE library backup")?;
    // LibraryFile derives Default: a library created through Library::open
    // therefore uses schema 0 until explicitly versioned. Serde's missing-field
    // default uses schema 1. Both are existing PocketHLE catalogue formats.
    if !matches!(library.schema_version, 0 | 1) {
        bail!("Unsupported library schema: {}", library.schema_version);
    }
    let mut ids = HashSet::new();
    for game in library.games {
        if game.id.is_empty()
            || game.id == "."
            || game.id == ".."
            || !game
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
            || !ids.insert(game.id.to_lowercase())
        {
            bail!("Invalid or duplicate game ID");
        }
        for path in std::iter::once(&game.executable)
            .chain(game.icon.iter())
            .chain(game.companions.iter())
        {
            safe_path(
                &path
                    .to_str()
                    .context("Invalid game path")?
                    .replace('\\', "/"),
            )?;
        }
    }
    for config in ["config.json", "config-steamdeck.json"] {
        if root.join(config).exists() {
            let _: super::LauncherConfig = serde_json::from_reader(File::open(root.join(config))?)?;
        }
    }
    Ok(())
}

// PathBuf JSON written on Windows contains backslashes. Convert only host
// relative metadata paths, never guest WinCE registry/install paths or game data.
fn normalize_metadata(root: &Path) -> Result<()> {
    fn normalize_game(value: &mut serde_json::Value) -> Result<()> {
        for key in ["executable", "icon"] {
            if let Some(path) = value
                .get_mut(key)
                .and_then(|v| v.as_str().map(str::to_owned))
            {
                let path = path.replace('\\', "/");
                safe_path(&path)?;
                value[key] = path.into();
            }
        }
        if let Some(paths) = value.get_mut("companions").and_then(|v| v.as_array_mut()) {
            for path in paths {
                let normalized = path
                    .as_str()
                    .context("Invalid companion path")?
                    .replace('\\', "/");
                safe_path(&normalized)?;
                *path = normalized.into();
            }
        }
        Ok(())
    }
    let path = root.join("library.json");
    let mut catalog: serde_json::Value = serde_json::from_reader(File::open(&path)?)?;
    for game in catalog["games"]
        .as_array_mut()
        .context("Missing game catalogue")?
    {
        normalize_game(game)?;
        let id = game["id"].as_str().context("Missing game ID")?;
        let manifest = root.join("games").join(id).join("game.json");
        if manifest.is_file() {
            let mut value: serde_json::Value = serde_json::from_reader(File::open(&manifest)?)?;
            normalize_game(&mut value)?;
            super::write_json(&manifest, &value)?;
        }
    }
    super::write_json(&path, &catalog)?;
    Ok(())
}

fn copy_tree(source: &Path, target: &Path, overwrite: bool) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest = target.join(entry.file_name());
        if ty.is_symlink() {
            bail!("Cannot restore over symbolic links");
        }
        if ty.is_dir() {
            if dest.is_file() {
                if !overwrite {
                    continue;
                }
                fs::remove_file(&dest)?;
            }
            copy_tree(&entry.path(), &dest, overwrite)?;
        } else if ty.is_file() {
            if dest.exists() && !overwrite {
                continue;
            }
            if dest.is_dir() {
                fs::remove_dir_all(&dest)?;
            }
            fs::copy(entry.path(), dest)?;
        } else {
            bail!("Unsupported file in library");
        }
    }
    Ok(())
}

fn merge_catalogue(current: &Path, incoming: &Path, target: &Path, overwrite: bool) -> Result<()> {
    let mut old: super::LibraryFile =
        serde_json::from_reader(File::open(current.join("library.json"))?)?;
    let new: super::LibraryFile =
        serde_json::from_reader(File::open(incoming.join("library.json"))?)?;
    for game in new.games {
        match old.games.iter_mut().find(|existing| existing.id == game.id) {
            Some(existing) if overwrite => *existing = game,
            Some(_) => {}
            None => old.games.push(game),
        }
    }
    super::write_json(&target.join("library.json"), &old)?;
    Ok(())
}

pub fn restore(root: &Path, archive: &Path, mode: RestoreMode) -> Result<()> {
    if fs::symlink_metadata(root)?.file_type().is_symlink() {
        bail!("Library root cannot be a symbolic link");
    }
    let root = root.canonicalize()?;
    let parent = root.parent().context("Cannot replace a filesystem root")?;
    let staging = tempfile::Builder::new()
        .prefix(".pockethle-restore-")
        .tempdir_in(parent)?;
    let incoming = staging.path().join("incoming");
    fs::create_dir(&incoming)?;
    unpack(archive, &incoming)?;
    normalize_metadata(&incoming)?;
    let next = staging.path().join("next");
    if mode == RestoreMode::Replace {
        fs::rename(&incoming, &next)?;
    } else {
        validate_library(&root)?;
        copy_tree(&root, &next, false)?;
        let overwrite = mode == RestoreMode::MergeOverwrite;
        copy_tree(&incoming, &next, overwrite)?;
        merge_catalogue(&root, &incoming, &next, overwrite)?;
        validate_library(&next)?;
    }
    let previous = staging.path().join("previous");
    fs::rename(&root, &previous).context("Could not move current library; nothing was replaced")?;
    if let Err(error) = fs::rename(&next, &root) {
        if let Err(rollback) = fs::rename(&previous, &root) {
            let retained = staging.keep();
            bail!("Restore failed ({error}); rollback failed ({rollback}). Original library retained at {}", retained.join("previous").display());
        }
        return Err(error).context("Restore failed; original library restored");
    }
    // A cleanup failure must not turn a successful commit into an apparent failure.
    if let Err(error) = staging.close() {
        log::warn!("Backup cleanup failed: {error}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(root: &Path, id: &str, content: &str) {
        fs::create_dir_all(root.join(format!("games/{id}/extracted"))).unwrap();
        fs::write(root.join(format!("games/{id}/extracted/game.exe")), content).unwrap();
        let game = crate::GameEntry {
            platform: None,
            id: id.into(),
            display_name: id.into(),
            provider: None,
            executable: "extracted/game.exe".into(),
            source_cab: "test.cab".into(),
            install_dir: None,
            install_dirs: Vec::new(),
            save_prefix: None,
            registry: Vec::new(),
            imported_at: 0,
            settings: crate::GameSettings::default(),
            icon: None,
            companions: Vec::new(),
        };
        let mut value = serde_json::to_value(game).unwrap();
        value["id"] = id.into();
        value["executable"] = "extracted/game.exe".into();
        fs::write(
            root.join("library.json"),
            serde_json::to_vec(&serde_json::json!({"schema_version":1,"games":[value]})).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("config.json"),
            serde_json::to_vec(&crate::LauncherConfig::default()).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn all_restore_modes_preserve_expected_files_and_catalogue() {
        for schema in [0, 1] {
            for mode in [
                RestoreMode::MergeNoOverwrite,
                RestoreMode::MergeOverwrite,
                RestoreMode::Replace,
            ] {
                let temp = tempfile::tempdir().unwrap();
                let source = temp.path().join("source");
                let target = temp.path().join("target");
                library(&source, "incoming", "new game");
                library(&target, "existing", "old game");
                for root in [&source, &target] {
                    let file = root.join("library.json");
                    let mut catalog: serde_json::Value =
                        serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
                    catalog["schema_version"] = schema.into();
                    fs::write(file, serde_json::to_vec(&catalog).unwrap()).unwrap();
                }
                fs::write(source.join("shared-save"), "backup save").unwrap();
                fs::write(target.join("shared-save"), "current save").unwrap();
                fs::create_dir(source.join("empty-directory")).unwrap();
                let archive = temp.path().join("backup.zip");
                save(&source, &archive).unwrap();
                restore(&target, &archive, mode).unwrap();
                assert_eq!(
                    fs::read_to_string(target.join("games/incoming/extracted/game.exe")).unwrap(),
                    "new game"
                );
                assert_eq!(
                    fs::read_to_string(target.join("shared-save")).unwrap(),
                    if mode == RestoreMode::MergeNoOverwrite {
                        "current save"
                    } else {
                        "backup save"
                    }
                );
                assert!(target.join("empty-directory").is_dir());
                assert!(!target.join(MARKER).exists());
                let catalog: crate::LibraryFile =
                    serde_json::from_slice(&fs::read(target.join("library.json")).unwrap())
                        .unwrap();
                assert_eq!(
                    catalog.games.len(),
                    if mode == RestoreMode::Replace { 1 } else { 2 }
                );
                assert_eq!(
                    target.join("games/existing").exists(),
                    mode != RestoreMode::Replace
                );
            }
        }
    }

    #[test]
    fn conflicting_game_metadata_follows_merge_policy() {
        for (mode, expected) in [
            (RestoreMode::MergeNoOverwrite, "Current"),
            (RestoreMode::MergeOverwrite, "Backup"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let source = temp.path().join("source");
            let target = temp.path().join("target");
            library(&source, "same", "new");
            library(&target, "same", "old");
            for (root, name) in [(&source, "Backup"), (&target, "Current")] {
                let mut catalog: crate::LibraryFile =
                    serde_json::from_slice(&fs::read(root.join("library.json")).unwrap()).unwrap();
                catalog.games[0].display_name = name.into();
                crate::write_json(&root.join("library.json"), &catalog).unwrap();
            }
            let archive = temp.path().join("backup.zip");
            save(&source, &archive).unwrap();
            restore(&target, &archive, mode).unwrap();
            let catalog: crate::LibraryFile =
                serde_json::from_slice(&fs::read(target.join("library.json")).unwrap()).unwrap();
            assert_eq!(catalog.games[0].display_name, expected);
        }
    }

    #[test]
    fn malformed_zip_leaves_original_library_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        library(&root, "original", "original bytes");
        let archive = temp.path().join("invalid.zip");
        fs::write(&archive, b"not a ZIP").unwrap();
        let before = fs::read(root.join("library.json")).unwrap();
        assert!(restore(&root, &archive, RestoreMode::Replace).is_err());
        assert_eq!(fs::read(root.join("library.json")).unwrap(), before);
        assert_eq!(
            fs::read(root.join("games/original/extracted/game.exe")).unwrap(),
            b"original bytes"
        );
    }

    #[test]
    fn unsafe_or_duplicate_entries_do_not_change_target() {
        for names in [
            vec!["../escape"],
            vec!["/absolute"],
            vec!["C:/escape"],
            vec!["..\\escape"],
            vec!["duplicate", "duplicate"],
            vec!["Case", "case"],
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("library");
            library(&root, "original", "keep");
            let archive = temp.path().join("invalid.zip");
            let mut zip = ZipWriter::new(File::create(&archive).unwrap());
            for name in names {
                zip.start_file(name, options()).unwrap();
                zip.write_all(b"attack").unwrap();
            }
            zip.finish().unwrap();
            assert!(restore(&root, &archive, RestoreMode::Replace).is_err());
            assert!(root.join("games/original/extracted/game.exe").exists());
            assert!(!temp.path().join("escape").exists());
        }
    }

    #[test]
    fn windows_catalogue_paths_are_portable_on_restore() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        library(&source, "windows", "game bytes");
        library(&target, "old", "old");
        let path = source.join("library.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["games"][0]["executable"] = "extracted\\game.exe".into();
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let archive = temp.path().join("backup.zip");
        save(&source, &archive).unwrap();
        restore(&target, &archive, RestoreMode::Replace).unwrap();
        let catalog: crate::LibraryFile =
            serde_json::from_slice(&fs::read(target.join("library.json")).unwrap()).unwrap();
        assert_eq!(
            catalog.games[0].executable.to_string_lossy(),
            "extracted/game.exe"
        );
        assert!(catalog.games[0].executable_path(&target).is_file());
    }

    #[test]
    fn catalogue_created_by_library_api_can_be_backed_up_and_restored() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        // Use production initialization and persistence, not a schema-1 fixture.
        crate::Library::open(&source).unwrap().save().unwrap();
        fs::write(source.join("save.dat"), b"keep these saves").unwrap();
        let archive = temp.path().join("backup.zip");
        save(&source, &archive).unwrap();
        for mode in [
            RestoreMode::MergeNoOverwrite,
            RestoreMode::MergeOverwrite,
            RestoreMode::Replace,
        ] {
            crate::Library::open(&target).unwrap().save().unwrap();
            restore(&target, &archive, mode).unwrap();
            assert!(crate::Library::open(&target).unwrap().games().is_empty());
            assert_eq!(
                fs::read(target.join("save.dat")).unwrap(),
                b"keep these saves"
            );
        }
    }

    #[test]
    fn unsupported_schema_error_identifies_version_without_modifying_library() {
        let temp = tempfile::tempdir().unwrap();
        library(temp.path(), "game", "bytes");
        let file = temp.path().join("library.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        value["schema_version"] = 99.into();
        fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
        let before = fs::read(&file).unwrap();
        let output = temp
            .path()
            .parent()
            .unwrap()
            .join(format!("unsupported-schema-{}.zip", std::process::id()));
        assert!(save(temp.path(), &output)
            .unwrap_err()
            .to_string()
            .contains("99"));
        assert_eq!(fs::read(file).unwrap(), before);
        assert!(!output.exists());
    }

    #[test]
    fn export_inside_library_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        library(temp.path(), "game", "bytes");
        assert!(save(temp.path(), &temp.path().join("recursive.zip")).is_err());
        assert!(!temp.path().join("recursive.zip").exists());
    }
}
