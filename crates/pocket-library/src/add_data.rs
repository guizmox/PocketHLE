//! Per-game data additions. Prepare first, confirm conflicts, then publish while
//! sessions are stopped. Staging never modifies the installed game.
use anyhow::{bail, ensure, Context, Result};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::Library;

const MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    ApplicationFolder,
    MyDocuments,
}

#[derive(Debug)]
pub struct PreparedData {
    game_id: String,
    target: PathBuf,
    destination: Destination,
    staging: tempfile::TempDir,
    conflicts: Vec<String>,
    executables: Vec<String>,
    executable: Option<String>,
}

impl PreparedData {
    pub fn executables(&self) -> &[String] {
        &self.executables
    }

    pub fn choose_executable(&mut self, executable: Option<&str>) -> Result<()> {
        if let Some(path) = executable {
            ensure!(
                self.executables.iter().any(|candidate| candidate == path),
                "Executable is not part of this addition"
            );
        }
        self.executable = executable.map(str::to_owned);
        Ok(())
    }

    pub fn selected_game_id(&self) -> Option<&str> {
        self.executable.as_ref().map(|_| self.game_id.as_str())
    }

    pub fn destination(&self) -> &Path {
        &self.target
    }

    pub fn conflicts(&self) -> &[String] {
        &self.conflicts
    }

    /// No game writes occur until this explicit commit. A changed conflict set
    /// invalidates consent rather than silently overwriting additional files.
    pub fn apply(self, library: &mut Library, overwrite: bool) -> Result<()> {
        ensure!(
            target(library, &self.game_id, self.destination)? == self.target,
            "Game directory changed; select the data again"
        );
        let content = self.staging.path().join("content");
        let conflicts = find_conflicts(&content, &self.target)?;
        ensure!(
            conflicts == self.conflicts,
            "Game files changed; select the data again"
        );
        ensure!(
            overwrite || conflicts.is_empty(),
            "Existing files require overwrite confirmation"
        );
        let backup = self.staging.path().join("backup");
        fs::create_dir(&backup)?;
        let mut journal = Vec::new();
        let result = (|| -> Result<Option<crate::LibraryFile>> {
            fs::create_dir_all(&self.target)?;
            publish(&content, &self.target, &backup, &mut journal)?;
            let Some(selected) = &self.executable else {
                return Ok(None);
            };
            let mut executable = self.target.clone();
            for component in crate::backup::safe_path(selected)?.components() {
                executable = child(&executable, component.as_os_str())?;
            }
            ensure!(executable.is_file(), "Selected executable is missing");
            let game_directory = self.target.parent().context("Missing game directory")?;
            let relative = executable.strip_prefix(game_directory)?.to_path_buf();
            let mut catalogue = library.library.clone();
            let game = catalogue
                .games
                .iter_mut()
                .find(|game| game.id == self.game_id)
                .context("Game not found")?;
            game.executable = relative;
            game.settings.import_executable_selected = true;
            crate::apply_import_gpu_default(game, game_directory);
            let game_json = self.staging.path().join("game.json");
            fs::write(&game_json, serde_json::to_vec_pretty(&game)?)?;
            let library_json = self.staging.path().join("library.json");
            fs::write(&library_json, serde_json::to_vec_pretty(&catalogue)?)?;
            let game_destination = child(game_directory, std::ffi::OsStr::new("game.json"))?;
            ensure!(
                !game_destination.exists() || game_destination.is_file(),
                "Invalid game manifest"
            );
            replace(&game_json, &game_destination, &backup, &mut journal)?;
            let library_destination = child(library.root(), std::ffi::OsStr::new("library.json"))?;
            ensure!(library_destination.is_file(), "Invalid library catalogue");
            replace(&library_json, &library_destination, &backup, &mut journal)?;
            Ok(Some(catalogue))
        })();
        match result {
            Ok(Some(catalogue)) => library.library = catalogue,
            Ok(None) => {}
            Err(error) => {
                if let Err(rollback) = rollback(&mut journal) {
                    let recovery = self.staging.keep();
                    bail!(
                        "{error:#}; rollback failed: {rollback:#}. Recovery files retained at {}",
                        recovery.display()
                    );
                }
                return Err(error);
            }
        }
        Ok(())
    }
}

/// `name` is the original filename, independently of an Android cache path.
/// ZIP extraction preserves every archive directory; it never strips a wrapper.
pub fn prepare(
    library: &Library,
    id: &str,
    source: &Path,
    name: &str,
    extract: bool,
) -> Result<PreparedData> {
    prepare_at(
        library,
        id,
        source,
        name,
        extract,
        Destination::ApplicationFolder,
    )
}

pub fn prepare_at(
    library: &Library,
    id: &str,
    source: &Path,
    name: &str,
    extract: bool,
    destination: Destination,
) -> Result<PreparedData> {
    let target = target(library, id, destination)?;
    let game = library.get(id).context("Game not found")?;
    let directory = library.root().join(game.relative_dir());
    let staging = tempfile::Builder::new()
        .prefix(".add-data-")
        .tempdir_in(directory)?;
    let content = staging.path().join("content");
    fs::create_dir(&content)?;
    if extract {
        unpack(source, &content)?;
    } else {
        let relative = crate::backup::safe_path(name)?;
        ensure!(
            relative.components().count() == 1,
            "Select a single filename"
        );
        ensure!(fs::metadata(source)?.is_file(), "Select a file");
        fs::copy(source, content.join(relative))?;
    }
    let conflicts = find_conflicts(&content, &target)?;
    fn collect(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<()> {
        for entry in sorted_entries(dir)? {
            if entry.file_type()?.is_dir() {
                collect(&entry.path(), root, out)?;
            } else if entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            {
                out.push(
                    entry
                        .path()
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        Ok(())
    }
    let mut executables = Vec::new();
    if destination == Destination::ApplicationFolder {
        collect(&content, &content, &mut executables)?;
    }
    Ok(PreparedData {
        game_id: id.into(),
        target,
        destination,
        staging,
        conflicts,
        executables,
        executable: None,
    })
}

pub fn is_zip(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

fn target(library: &Library, id: &str, destination: Destination) -> Result<PathBuf> {
    let game = library.get(id).context("Game not found")?;
    let games = library.root().join("games");
    let directory = library.root().join(game.relative_dir());
    let extracted = game.extracted_dir(library.root());
    for path in [&games, &directory, &extracted] {
        let ty = fs::symlink_metadata(path)?.file_type();
        ensure!(
            ty.is_dir() && !ty.is_symlink(),
            "Game directory must not be a link: {}",
            path.display()
        );
    }
    ensure!(
        directory.parent() == Some(games.as_path()),
        "Invalid game directory"
    );
    if destination == Destination::ApplicationFolder {
        return Ok(extracted.canonicalize()?);
    }
    let directory = directory.canonicalize()?;
    let saves = child(&directory, std::ffi::OsStr::new("saves"))?;
    ensure!(!saves.exists() || saves.is_dir(), "Invalid saves directory");
    if !game.my_documents_uses_separate_directory(library.root()) {
        return Ok(saves);
    }
    let documents = child(&saves, std::ffi::OsStr::new("my-documents"))?;
    ensure!(
        !documents.exists() || documents.is_dir(),
        "Invalid My Documents directory"
    );
    Ok(documents)
}

/// Resolve like the guest's case-insensitive filesystem on every host. Refuse
/// ambiguous host trees and links instead of following them outside the game.
fn child(parent: &Path, name: &std::ffi::OsStr) -> Result<PathBuf> {
    let mut matches = Vec::new();
    if parent.is_dir() {
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(&name.to_string_lossy())
            {
                matches.push(entry.path());
            }
        }
    }
    ensure!(
        matches.len() <= 1,
        "Ambiguous filename in {}",
        parent.display()
    );
    let path = matches.pop().unwrap_or_else(|| parent.join(name));
    match fs::symlink_metadata(&path) {
        Ok(meta) => ensure!(
            meta.is_dir() || meta.is_file(),
            "Links and special files are not supported: {}",
            path.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(path)
}

fn unpack(source: &Path, content: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(source)?)?;
    ensure!(zip.len() <= MAX_ENTRIES, "Too many ZIP entries");
    let mut seen = std::collections::HashSet::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name().replace('\\', "/");
        let relative = crate::backup::safe_path(&name)?;
        ensure!(
            seen.insert(name.trim_end_matches('/').to_ascii_lowercase()),
            "Duplicate ZIP path: {name}"
        );
        if let Some(mode) = entry.unix_mode() {
            ensure!(
                matches!(mode & 0o170000, 0 | 0o100000 | 0o040000),
                "ZIP links and special files are not supported"
            );
        }
        total = total.checked_add(entry.size()).context("ZIP too large")?;
        ensure!(total <= MAX_BYTES, "ZIP exceeds 64 GiB uncompressed");
        let mut path = content.to_path_buf();
        let parts: Vec<_> = relative.components().collect();
        for (i, part) in parts.iter().enumerate() {
            path = child(&path, part.as_os_str())?;
            if i + 1 < parts.len() || entry.is_dir() {
                fs::create_dir_all(&path)?;
            }
        }
        if !entry.is_dir() {
            let expected = entry.size();
            let mut output = File::options().write(true).create_new(true).open(&path)?;
            let copied = std::io::copy(
                &mut entry.by_ref().take(expected.saturating_add(1)),
                &mut output,
            )?;
            ensure!(copied == expected, "Invalid ZIP entry size: {name}");
        }
    }
    Ok(())
}

fn sorted_entries(path: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    Ok(entries)
}

fn find_conflicts(content: &Path, target: &Path) -> Result<Vec<String>> {
    fn visit(source: &Path, dest: &Path, root: &Path, out: &mut Vec<String>) -> Result<()> {
        for entry in sorted_entries(source)? {
            let path = child(dest, &entry.file_name())?;
            if path.exists() {
                if entry.file_type()?.is_dir() && path.is_dir() {
                    visit(&entry.path(), &path, root, out)?;
                } else {
                    out.push(path.strip_prefix(root)?.to_string_lossy().into_owned());
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(content, target, target, &mut out)?;
    out.sort();
    Ok(out)
}

struct Change {
    target: PathBuf,
    backup: Option<PathBuf>,
}

fn publish(source: &Path, dest: &Path, backups: &Path, journal: &mut Vec<Change>) -> Result<()> {
    for entry in sorted_entries(source)? {
        let path = child(dest, &entry.file_name())?;
        if entry.file_type()?.is_dir() && path.is_dir() {
            publish(&entry.path(), &path, backups, journal)?;
            continue;
        }
        replace(&entry.path(), &path, backups, journal)?;
    }
    Ok(())
}

fn replace(source: &Path, path: &Path, backups: &Path, journal: &mut Vec<Change>) -> Result<()> {
    let backup = if path.exists() {
        let saved = backups.join(journal.len().to_string());
        fs::rename(path, &saved)?;
        Some(saved)
    } else {
        None
    };
    journal.push(Change {
        target: path.to_path_buf(),
        backup,
    });
    fs::rename(source, path)?;
    Ok(())
}

fn rollback(journal: &mut Vec<Change>) -> Result<()> {
    for change in journal.iter().rev() {
        if change.target.is_dir() {
            fs::remove_dir_all(&change.target)?;
        } else if change.target.exists() {
            fs::remove_file(&change.target)?;
        }
        if let Some(backup) = &change.backup {
            fs::rename(backup, &change.target)?;
        }
    }
    journal.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn library() -> (tempfile::TempDir, Library, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("games/test/extracted");
        fs::create_dir_all(&dir).unwrap();
        let entry = serde_json::json!({"id":"test", "display_name":"Test", "executable":"extracted/game.exe", "source_cab":"game.cab"});
        fs::write(
            root.path().join("library.json"),
            serde_json::json!({"games":[entry]}).to_string(),
        )
        .unwrap();
        let library = Library::open(root.path()).unwrap();
        (root, library, dir)
    }

    fn zip(root: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let source = root.join("data.zip");
        let mut zip = zip::ZipWriter::new(File::create(&source).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::FileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        source
    }

    #[test]
    fn file_is_staged_and_cancel_or_missing_consent_never_writes_game() {
        let (root, mut lib, dir) = library();
        fs::write(dir.join("Data.bin"), b"old").unwrap();
        let source = root.path().join("source");
        fs::write(&source, b"new").unwrap();
        let pending = prepare(&lib, "test", &source, "data.bin", false).unwrap();
        assert_eq!(pending.conflicts(), &["Data.bin"]);
        assert_eq!(fs::read(dir.join("Data.bin")).unwrap(), b"old");
        drop(pending); // Cancel.
        assert_eq!(fs::read(dir.join("Data.bin")).unwrap(), b"old");
        assert!(prepare(&lib, "test", &source, "data.bin", false)
            .unwrap()
            .apply(&mut lib, false)
            .is_err());
        assert_eq!(fs::read(dir.join("Data.bin")).unwrap(), b"old");
        prepare(&lib, "test", &source, "data.bin", false)
            .unwrap()
            .apply(&mut lib, true)
            .unwrap();
        assert_eq!(fs::read(dir.join("Data.bin")).unwrap(), b"new");
        assert!(!dir.join("data.bin").exists());
    }

    #[test]
    fn my_documents_preserves_zip_folders_without_offering_executables() {
        let (root, mut lib, extracted) = library();
        let source = zip(
            root.path(),
            &[("test/game.exe", b"data"), ("test/racer.gba", b"rom")],
        );
        let pending = prepare_at(
            &lib,
            "test",
            &source,
            "data.zip",
            true,
            Destination::MyDocuments,
        )
        .unwrap();
        let documents = extracted.parent().unwrap().join("saves/my-documents");
        assert!(pending.executables().is_empty());
        assert!(!documents.exists());
        pending.apply(&mut lib, false).unwrap();
        assert_eq!(fs::read(documents.join("test/racer.gba")).unwrap(), b"rom");
        assert_eq!(fs::read(documents.join("test/game.exe")).unwrap(), b"data");
        assert!(!extracted.join("test").exists());
        let pending = prepare_at(
            &lib,
            "test",
            &source,
            "data.zip",
            true,
            Destination::MyDocuments,
        )
        .unwrap();
        assert_eq!(pending.conflicts().len(), 2);
        assert!(pending.apply(&mut lib, false).is_err());
    }

    #[test]
    fn zip_copy_and_extraction_preserve_structure_and_merge_unrelated_files() {
        let (root, mut lib, dir) = library();
        fs::create_dir(dir.join("assets")).unwrap();
        fs::write(dir.join("assets/keep"), b"keep").unwrap();
        fs::write(dir.join("assets/old"), b"old").unwrap();
        let source = zip(
            root.path(),
            &[("assets/old", b"new"), ("assets/sub/new", b"added")],
        );
        let pending = prepare(&lib, "test", &source, "DATA.ZIP", true).unwrap();
        assert_eq!(pending.conflicts(), &["assets/old"]);
        assert!(!dir.join("assets/sub").exists());
        pending.apply(&mut lib, true).unwrap();
        assert_eq!(fs::read(dir.join("assets/keep")).unwrap(), b"keep");
        assert_eq!(fs::read(dir.join("assets/old")).unwrap(), b"new");
        assert_eq!(fs::read(dir.join("assets/sub/new")).unwrap(), b"added");
        prepare(&lib, "test", &source, "DATA.ZIP", false)
            .unwrap()
            .apply(&mut lib, false)
            .unwrap();
        assert_eq!(
            fs::read(dir.join("DATA.ZIP")).unwrap(),
            fs::read(source).unwrap()
        );
    }

    #[test]
    fn file_directory_conflicts_require_consent_before_any_installation() {
        let (root, mut lib, dir) = library();
        fs::write(dir.join("assets"), b"file").unwrap();
        fs::create_dir(dir.join("replace")).unwrap();
        fs::write(dir.join("replace/old"), b"old").unwrap();
        let source = zip(root.path(), &[("assets/new", b"new"), ("replace", b"file")]);
        let pending = prepare(&lib, "test", &source, "data.zip", true).unwrap();
        assert_eq!(pending.conflicts(), &["assets", "replace"]);
        pending.apply(&mut lib, true).unwrap();
        assert_eq!(fs::read(dir.join("assets/new")).unwrap(), b"new");
        assert_eq!(fs::read(dir.join("replace")).unwrap(), b"file");
    }

    #[test]
    fn changed_conflicts_invalidate_confirmation_including_new_collisions() {
        let (root, mut lib, dir) = library();
        let source = zip(root.path(), &[("one", b"1"), ("two", b"2")]);
        let pending = prepare(&lib, "test", &source, "data.zip", true).unwrap();
        fs::write(dir.join("two"), b"other").unwrap();
        assert!(pending.apply(&mut lib, true).is_err());
        assert!(!dir.join("one").exists());
        assert_eq!(fs::read(dir.join("two")).unwrap(), b"other");
    }

    #[test]
    fn unsafe_or_ambiguous_zip_never_writes_destination() {
        for names in [
            vec!["../outside"],
            vec!["/absolute"],
            vec!["C:/drive"],
            vec!["NUL"],
            vec!["a", "A"],
            vec!["a", "a/b"],
        ] {
            let (root, lib, dir) = library();
            let entries: Vec<_> = names
                .iter()
                .map(|name| (*name, b"data".as_slice()))
                .collect();
            let source = zip(root.path(), &entries);
            assert!(
                prepare(&lib, "test", &source, "data.zip", true).is_err(),
                "{names:?}"
            );
            assert_eq!(fs::read_dir(dir).unwrap().count(), 0);
        }
    }

    #[test]
    fn failed_publish_rolls_back_files_and_replaced_directories() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("dest");
        let source = root.path().join("source");
        let backup = root.path().join("backup");
        for path in [&dest, &source, &backup] {
            fs::create_dir(path).unwrap();
        }
        fs::create_dir(dest.join("a")).unwrap();
        fs::write(dest.join("a/old"), b"old").unwrap();
        fs::write(source.join("a"), b"new").unwrap();
        let mut journal = Vec::new();
        publish(&source, &dest, &backup, &mut journal).unwrap();
        rollback(&mut journal).unwrap();
        assert_eq!(fs::read(dest.join("a/old")).unwrap(), b"old");
    }

    #[test]
    fn executable_choice_updates_both_manifests_and_reopens() {
        let (root, mut lib, dir) = library();
        let source = zip(
            root.path(),
            &[("bin/new.exe", b"exe"), ("other.EXE", b"other")],
        );
        let mut pending = prepare(&lib, "test", &source, "data.zip", true).unwrap();
        assert_eq!(pending.executables(), &["bin/new.exe", "other.EXE"]);
        assert!(pending.choose_executable(Some("outside.exe")).is_err());
        pending.choose_executable(Some("bin/new.exe")).unwrap();
        pending.apply(&mut lib, false).unwrap();
        assert_eq!(
            lib.get("test").unwrap().executable,
            PathBuf::from("extracted/bin/new.exe")
        );
        let manifest: crate::GameEntry =
            serde_json::from_slice(&fs::read(dir.parent().unwrap().join("game.json")).unwrap())
                .unwrap();
        assert_eq!(manifest.executable, lib.get("test").unwrap().executable);
        let reopened = Library::open(root.path()).unwrap();
        assert_eq!(
            reopened.get("test").unwrap().executable,
            manifest.executable
        );
    }

    #[test]
    fn single_executable_is_optional_and_preserves_original_filename() {
        let (root, mut lib, dir) = library();
        let source = root.path().join("source.tmp");
        fs::write(&source, b"exe").unwrap();
        let pending = prepare(&lib, "test", &source, "My Game.EXE", false).unwrap();
        assert_eq!(pending.executables(), &["My Game.EXE"]);
        pending.apply(&mut lib, false).unwrap();
        assert_eq!(
            lib.get("test").unwrap().executable,
            PathBuf::from("extracted/game.exe")
        );
        assert_eq!(fs::read(dir.join("My Game.EXE")).unwrap(), b"exe");
        let mut pending = prepare(&lib, "test", &source, "New.EXE", false).unwrap();
        pending.choose_executable(Some("New.EXE")).unwrap();
        pending.apply(&mut lib, false).unwrap();
        assert_eq!(
            lib.get("test").unwrap().executable,
            PathBuf::from("extracted/New.EXE")
        );
    }

    #[test]
    fn metadata_failure_restores_game_data_and_first_manifest() {
        let (root, mut lib, dir) = library();
        let game_json = dir.parent().unwrap().join("game.json");
        fs::write(&game_json, b"original manifest").unwrap();
        fs::write(dir.join("new.exe"), b"old exe").unwrap();
        let source = zip(root.path(), &[("new.exe", b"new exe"), ("added", b"new")]);
        let mut pending = prepare(&lib, "test", &source, "data.zip", true).unwrap();
        pending.choose_executable(Some("new.exe")).unwrap();
        // Force rejection at the second metadata write, after the first has been
        // published. This also verifies rollback of both replaced and new files.
        fs::remove_file(root.path().join("library.json")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            root.path().join("missing"),
            root.path().join("library.json"),
        )
        .unwrap();
        #[cfg(not(unix))]
        {
            fs::create_dir(root.path().join("library.json")).unwrap();
            fs::write(root.path().join("library.json/no-remove"), b"x").unwrap();
        }
        // A directory is not a valid manifest either; production rejects it.
        assert!(pending.apply(&mut lib, true).is_err());
        assert_eq!(fs::read(&game_json).unwrap(), b"original manifest");
        assert_eq!(fs::read(dir.join("new.exe")).unwrap(), b"old exe");
        assert!(!dir.join("added").exists());
        assert_eq!(
            lib.get("test").unwrap().executable,
            PathBuf::from("extracted/game.exe")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_destinations_are_rejected() {
        let (root, lib, dir) = library();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.join("link")).unwrap();
        let source = zip(root.path(), &[("link/new", b"new")]);
        assert!(prepare(&lib, "test", &source, "data.zip", true).is_err());
        assert!(!outside.path().join("new").exists());
    }
}
