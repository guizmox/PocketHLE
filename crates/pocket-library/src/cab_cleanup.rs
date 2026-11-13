use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pocket_cab::{installed_cab_aliases, plain_path_below};
use serde::Deserialize;

#[derive(Deserialize)]
struct GameManifest {
    executable: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backs_up_identical_aliases_and_preserves_the_launch_target_and_modified_files() {
        let library = tempfile::tempdir().unwrap();
        let root = library.path();
        let game = root.join("games").join("sample");
        let extracted = game.join("extracted");
        fs::create_dir_all(&extracted).unwrap();
        fs::write(
            game.join("game.json"),
            br#"{"executable":"extracted/launch.003"}"#,
        )
        .unwrap();
        fs::write(
            extracted.join("_setup.xml"),
            br#"<wap-provisioningdoc>
<characteristic type="FileOperation">
<characteristic type="%CE1%\Sample" translation="install">
<characteristic type="asset.dat" translation="install">
<characteristic type="Extract"><parm name="Source" value="asset.001" /></characteristic>
</characteristic>
<characteristic type="game.exe" translation="install">
<characteristic type="Extract"><parm name="Source" value="game.002" /></characteristic>
</characteristic>
<characteristic type="launch.exe" translation="install">
<characteristic type="Extract"><parm name="Source" value="launch.003" /></characteristic>
</characteristic>
</characteristic>
</characteristic>
</wap-provisioningdoc>"#,
        )
        .unwrap();
        for (name, bytes) in [
            ("asset.001", "same"),
            ("asset.dat", "same"),
            ("game.002", "original"),
            ("game.exe", "modified"),
            ("launch.003", "launch"),
            ("launch.exe", "launch"),
        ] {
            fs::write(extracted.join(name), bytes).unwrap();
        }

        let report = run(root).unwrap();
        assert!(report.starts_with("Moved 1 verified CAB aliases"));
        let backup = fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("cab-alias-backup-")
            })
            .unwrap();
        assert_eq!(
            fs::read(backup.join("sample").join("extracted").join("asset.001")).unwrap(),
            b"same"
        );
        assert!(!extracted.join("asset.001").exists());
        assert!(extracted.join("asset.dat").is_file());
        assert!(extracted.join("game.002").is_file());
        assert!(extracted.join("launch.003").is_file());
        assert_eq!(run(root).unwrap(), "No verified CAB aliases found.");
    }
}

/// Move verified aliases out of installed game directories and report the backup location.
pub fn run(root: &Path) -> io::Result<String> {
    let games = root.join("games");
    if !plain_path_below(root, &games, true) {
        return Err(io::Error::other(
            "The games directory is missing or is a link.",
        ));
    }
    let mut aliases = Vec::new();
    let mut skipped = 0;
    for game in fs::read_dir(&games)? {
        let game = game?.path();
        if !plain_path_below(&games, &game, true) {
            continue;
        }
        let extracted = game.join("extracted");
        let manifest = game.join("game.json");
        if !plain_path_below(&game, &extracted, true) || !plain_path_below(&game, &manifest, false)
        {
            continue;
        }
        let executable = fs::File::open(manifest)
            .ok()
            .and_then(|file| serde_json::from_reader::<_, GameManifest>(file).ok())
            .filter(|manifest| !manifest.executable.as_os_str().is_empty())
            .map(|manifest| game.join(manifest.executable));
        let Some(executable) = executable else {
            skipped += 1;
            continue;
        };
        match installed_cab_aliases(&extracted, &executable) {
            Ok(paths) => aliases.extend(paths),
            Err(error) => {
                log::warn!("CAB cleanup skipped {}: {error}", game.display());
                skipped += 1;
            }
        }
    }
    let skipped_notice = if skipped == 0 {
        String::new()
    } else {
        format!(" Skipped {skipped} game folders.")
    };
    if aliases.is_empty() {
        return Ok(format!("No verified CAB aliases found.{skipped_notice}"));
    }

    let backup = tempfile::Builder::new()
        .prefix("cab-alias-backup-")
        .tempdir_in(root)?
        .keep();
    let mut moved = 0;
    let mut failed = 0;
    for source in aliases {
        let relative = source.strip_prefix(&games).expect("alias is beneath games");
        let destination = backup.join(relative);
        let result = fs::create_dir_all(destination.parent().expect("alias has a parent"))
            .and_then(|()| fs::rename(&source, &destination));
        match result {
            Ok(()) => moved += 1,
            Err(error) => {
                log::warn!("Could not back up CAB alias {}: {error}", source.display());
                failed += 1;
            }
        }
    }
    let failed_notice = if failed == 0 {
        String::new()
    } else {
        format!(" Could not move {failed} files. See the log.")
    };
    Ok(format!(
        "Moved {moved} verified CAB aliases to {}.{skipped_notice}{failed_notice}",
        backup.display()
    ))
}
