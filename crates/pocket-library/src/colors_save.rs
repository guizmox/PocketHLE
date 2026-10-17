//! Explicit Colors menu unlocks. Solo/online money is not granted by this action.
use crate::{is_colors_game, GameEntry};
use anyhow::{bail, Context, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const KEY: [u8; 32] = [
    41, 35, 190, 132, 225, 108, 214, 174, 82, 144, 73, 241, 241, 187, 233, 235, 179, 166, 219, 60,
    135, 12, 62, 153, 36, 94, 13, 28, 6, 183, 71, 222,
];
const ROTATIONS: [u8; 32] = [
    3, 2, 5, 0, 3, 3, 3, 6, 7, 3, 2, 5, 1, 0, 5, 7, 5, 4, 3, 4, 6, 5, 5, 3, 3, 5, 1, 2, 4, 3, 6, 2,
];
const FEEDBACK: [u8; 256] = [
    32, 154, 80, 238, 64, 120, 54, 253, 18, 73, 50, 246, 158, 125, 220, 173, 79, 20, 242, 68, 102,
    208, 107, 196, 48, 183, 59, 161, 34, 145, 157, 225, 139, 31, 218, 176, 202, 153, 2, 185, 114,
    44, 128, 126, 197, 213, 233, 178, 234, 201, 204, 83, 191, 103, 214, 45, 142, 131, 239, 87, 97,
    255, 105, 143, 205, 209, 30, 156, 22, 230, 29, 240, 132, 74, 119, 215, 232, 57, 203, 51, 116,
    12, 244, 212, 159, 164, 89, 53, 207, 211, 144, 72, 117, 217, 42, 229, 192, 247, 43, 129, 135,
    14, 95, 0, 141, 190, 123, 5, 21, 7, 130, 24, 112, 146, 100, 84, 206, 177, 133, 62, 248, 70,
    106, 4, 150, 115, 47, 104, 118, 250, 187, 17, 174, 36, 136, 121, 254, 82, 219, 37, 67, 60, 69,
    216, 40, 11, 245, 96, 61, 151, 39, 138, 194, 8, 165, 193, 140, 169, 241, 149, 155, 168, 167,
    134, 181, 231, 35, 85, 56, 108, 94, 78, 113, 226, 180, 71, 101, 137, 122, 99, 38, 223, 41, 109,
    98, 224, 52, 166, 63, 227, 3, 65, 15, 27, 243, 235, 160, 6, 77, 127, 170, 91, 19, 252, 184, 13,
    58, 16, 76, 236, 49, 66, 124, 90, 222, 228, 33, 147, 179, 175, 111, 9, 1, 23, 86, 198, 200,
    249, 28, 55, 189, 110, 92, 195, 163, 152, 199, 93, 182, 81, 25, 46, 188, 148, 75, 88, 210, 172,
    26, 10, 162, 237, 251, 221, 186, 171,
];

// Reconstructed from Colors' byte transform; the same format wraps config and ISV.
fn transform(input: &[u8], decrypt: bool) -> Vec<u8> {
    let mut key = KEY;
    let mut previous = 0u8;
    let mut output = Vec::with_capacity(input.len());
    for (i, &byte) in input.iter().enumerate() {
        let value = if decrypt {
            byte.wrapping_sub(previous) ^ key[i % 32]
        } else {
            (byte ^ key[i % 32]).wrapping_add(previous)
        };
        previous = FEEDBACK[usize::from(if decrypt { byte } else { value })];
        output.push(value);
        if i % 32 == 31 {
            let shift = ROTATIONS[(i / 32) % 32];
            if shift != 0 {
                let old = key;
                for j in 0..32 {
                    key[j] = (old[j] << shift) | (old[(j + 1) % 32] >> (8 - shift));
                }
            }
        }
    }
    output
}

fn unlock_config(input: &[u8]) -> Result<Vec<u8>> {
    let text = String::from_utf8(transform(input, true)).context("Unknown Colors config format")?;
    let mut turf_found = false;
    let mut tag_found = false;
    let mut output = String::new();
    for line in text.split_inclusive('\n') {
        let clean = line.trim_end_matches(['\r', '\n']);
        if clean.is_empty() {
            output.push_str(line);
            continue;
        }
        let (key, value) = clean
            .split_once('=')
            .context("Invalid Colors config; no files changed")?;
        if key.is_empty()
            || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || value.contains('\0')
        {
            bail!("Invalid Colors config; no files changed");
        }
        if matches!(key, "TurfWarUnlocked" | "TagEditorUnlocked") {
            let found = if key == "TurfWarUnlocked" {
                &mut turf_found
            } else {
                &mut tag_found
            };
            if *found || !matches!(value, "0" | "1") {
                bail!("Ambiguous Colors unlock setting: {key}");
            }
            *found = true;
            output.push_str(key);
            output.push_str("=1");
            output.push_str(&line[clean.len()..]);
        } else {
            output.push_str(line);
        }
    }
    for (key, found) in [
        ("TurfWarUnlocked", turf_found),
        ("TagEditorUnlocked", tag_found),
    ] {
        if !found {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(key);
            output.push_str("=1\n");
        }
    }
    Ok(transform(output.as_bytes(), false))
}

fn validate_save(input: &[u8]) -> Result<(u32, u32)> {
    let text = String::from_utf8(transform(input, true)).context("Unknown Colors save format")?;
    let mut stack: Vec<&str> = Vec::new();
    let mut pending = None;
    let mut money = None;
    let mut server = None;
    for line in text.split_inclusive('\n') {
        let clean = line.trim();
        if clean == "{" {
            stack.push(pending.take().context("Invalid Colors save section")?);
        } else if clean == "}" {
            if pending.is_some() || stack.pop().is_none() {
                bail!("Invalid Colors save nesting");
            }
        } else if !clean.is_empty() {
            if pending.is_some() {
                bail!("Invalid Colors save section");
            }
            if let Some(index) = clean.find(char::is_whitespace) {
                let name = &clean[..index];
                let value = clean[index..].trim();
                if stack == ["Save", "Character"] && matches!(name, "Money" | "ServerMoney") {
                    let number: u32 = value.parse().context("Invalid Colors balance")?;
                    if number > i32::MAX as u32 {
                        bail!("Invalid Colors balance");
                    }
                    let slot = if name == "Money" {
                        &mut money
                    } else {
                        &mut server
                    };
                    if slot.replace(number).is_some() {
                        bail!("Duplicate Colors balance");
                    }
                }
            } else {
                pending = Some(clean);
            }
        }
    }
    if !stack.is_empty() || pending.is_some() {
        bail!("Incomplete Colors save");
    }
    Ok((
        money.context("Colors player balance not found")?,
        server.context("Colors server balance not found")?,
    ))
}

fn child(parent: &Path, name: &str) -> Result<PathBuf> {
    let mut matches = Vec::new();
    if parent.exists() {
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
            {
                matches.push(entry.path());
            }
        }
    }
    if matches.len() > 1 {
        bail!("Ambiguous Colors save path: {name}");
    }
    let path = matches.pop().unwrap_or_else(|| parent.join(name));
    if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        bail!("Colors save symlinks are not supported");
    }
    Ok(path)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::metadata(path) {
        Ok(meta) => {
            if !meta.is_file() || meta.len() > 4 * 1024 * 1024 {
                bail!("Unsupported Colors save file");
            }
            Ok(Some(fs::read(path)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing save directory")?)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// The game must be stopped. Backups are retained; unknown formats fail before writes.
pub fn unlock_turf_wars(game: &GameEntry, root: &Path) -> Result<String> {
    if !is_colors_game(game, root) {
        bail!("This action is only available for Colors");
    }
    unlock_in_flash(&game.gizmondo_flash_dir(root))
}

fn unlock_in_flash(flash: &Path) -> Result<String> {
    let games = child(flash, "MyGames")?;
    let directory = child(&games, "GZGA200015 Colors")?;
    let config = child(&directory, "config.cfg")?;
    let save_directory = child(&directory, "Save")?;
    let save = child(&save_directory, "SaveFile.isv")?;
    let old_config = read_optional(&config)?;
    let old_save = read_optional(&save)?;
    let new_config = unlock_config(old_config.as_deref().unwrap_or_default())?;
    if let Some(bytes) = old_save.as_ref() {
        validate_save(bytes)?;
    }
    let config_changed = old_config.as_deref() != Some(new_config.as_slice());
    if !config_changed {
        return Ok("Turf Wars and Tag Editor already unlocked. Player credits unchanged.".into());
    }
    // Validate both files before touching either. Keep unique original copies.
    fs::create_dir_all(&directory)?;
    if let Some(bytes) = old_config.as_ref() {
        let mut backup = tempfile::Builder::new()
            .prefix("config.cfg-before-turf-")
            .suffix(".bak")
            .tempfile_in(&directory)?;
        backup.write_all(bytes)?;
        backup.as_file().sync_all()?;
        backup.keep().map_err(|error| error.error)?;
        log::info!("Colors original config backed up before Turf Wars unlock");
    }
    replace(&config, &new_config)?;
    Ok("Turf Wars and Tag Editor unlocked. Player credits unchanged; online funds remain server-managed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Vec<u8> {
        transform(
            b"MainVolume=128\nLanguage=english\nTurfWarUnlocked=0\n",
            false,
        )
    }
    fn save(money: u32, server: u32) -> Vec<u8> {
        transform(format!("Save\n{{\n Character\n {{\n  Money {money}\n  ServerMoney {server}\n  Health 280\n }}\n Mission\n {{\n  Tutorial\n  {{\n   Status 1\n  }}\n }}\n}}\n").as_bytes(), false)
    }
    #[test]
    fn codec_round_trips_all_bytes_and_rotates_over_multiple_key_cycles() {
        let input: Vec<u8> = (0..8192).map(|i| i as u8).collect();
        assert_eq!(transform(&transform(&input, false), true), input);
        // Independent output of the supplied game's ARM transform, initial key bytes.
        assert_eq!(
            transform(b"MainVolume=128\n", false),
            vec![100, 201, 50, 182, 224, 89, 141, 23, 3, 227, 109, 199, 66, 161, 139]
        );
    }
    #[test]
    fn balances_are_validated_without_granting_or_changing_them() {
        for money in [0, 525, 20_000, 74_999, 75_000, 75_001, 100_000] {
            for server in [525, 80_000] {
                let original = save(money, server);
                assert_eq!(validate_save(&original).unwrap(), (money, server));
            }
        }
    }
    #[test]
    fn config_unlock_is_idempotent_and_keeps_unknown_settings() {
        let original = config();
        let changed = unlock_config(&original).unwrap();
        assert_eq!(
            String::from_utf8(transform(&changed, true)).unwrap(),
            "MainVolume=128\nLanguage=english\nTurfWarUnlocked=1\nTagEditorUnlocked=1\n"
        );
        assert_eq!(unlock_config(&changed).unwrap(), changed);
        assert!(unlock_config(b"corrupt").is_err());
    }
    #[test]
    fn previously_unlocked_turf_wars_can_still_unlock_tag_editor() {
        // Simulate a player who clicked the old Turf Wars-only button.
        let old = transform(
            b"MainVolume=128\nTurfWarUnlocked=1\nTagEditorUnlocked=0\n",
            false,
        );
        let changed = unlock_config(&old).unwrap();
        assert_eq!(
            transform(&changed, true),
            b"MainVolume=128\nTurfWarUnlocked=1\nTagEditorUnlocked=1\n".to_vec()
        );
        assert_eq!(unlock_config(&changed).unwrap(), changed);

        // The old config may not contain the newer Tag Editor key at all.
        let old_without_tag = transform(b"TurfWarUnlocked=1\n", false);
        assert_eq!(
            transform(&unlock_config(&old_without_tag).unwrap(), true),
            b"TurfWarUnlocked=1\nTagEditorUnlocked=1\n".to_vec()
        );
    }
    #[test]
    fn tag_unlock_preserves_existing_player_credits_and_never_changes_save() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("MyGames/GZGA200015 Colors");
        fs::create_dir_all(directory.join("Save")).unwrap();
        let config_path = directory.join("config.cfg");
        fs::write(
            &config_path,
            transform(b"TurfWarUnlocked=1\nTagEditorUnlocked=0\n", false),
        )
        .unwrap();
        let save_path = directory.join("Save/SaveFile.isv");
        for (money, server_money) in [(525, 525), (1500, 525), (98000, 90000)] {
            let original = save(money, server_money);
            fs::write(&save_path, &original).unwrap();
            unlock_in_flash(temp.path()).unwrap();
            assert_eq!(fs::read(&save_path).unwrap(), original);
            assert_eq!(
                validate_save(&fs::read(&save_path).unwrap()).unwrap(),
                (money, server_money)
            );
        }
        assert_eq!(
            transform(&fs::read(&config_path).unwrap(), true),
            b"TurfWarUnlocked=1\nTagEditorUnlocked=1\n"
        );
        let backups = fs::read_dir(&directory)
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "bak"))
            .count();
        assert_eq!(backups, 1);
    }
    #[test]
    fn invalid_or_duplicate_tag_flags_are_rejected_without_modification() {
        for setting in [
            "TagEditorUnlocked=2\n",
            "TagEditorUnlocked=0\nTagEditorUnlocked=1\n",
        ] {
            let cfg = transform(format!("TurfWarUnlocked=1\n{setting}").as_bytes(), false);
            assert!(unlock_config(&cfg).is_err());
        }
    }
    #[test]
    fn files_are_backed_up_once_and_mixed_case_paths_are_reused() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("mygames/GZGA200015 Colors");
        fs::create_dir_all(directory.join("save")).unwrap();
        let old_config = config();
        let old_save = save(525, 525);
        fs::write(directory.join("Config.CFG"), &old_config).unwrap();
        fs::write(directory.join("save/savefile.isv"), &old_save).unwrap();
        unlock_in_flash(temp.path()).unwrap();
        let backups: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "bak"))
            .map(|entry| fs::read(entry.path()).unwrap())
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(backups.contains(&old_config));
        assert!(!backups.contains(&old_save));
        assert_eq!(
            fs::read(directory.join("save/savefile.isv")).unwrap(),
            old_save
        );
        unlock_in_flash(temp.path()).unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 3);
        // Inspect directory entries: on Windows, exists("config.cfg") also
        // resolves the existing "Config.CFG" and cannot detect a duplicate.
        let config_names: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().eq_ignore_ascii_case("config.cfg"))
            .collect();
        assert_eq!(config_names, vec![std::ffi::OsString::from("Config.CFG")]);
    }
    #[test]
    fn repeated_unlock_never_changes_campaign_gains_or_sync_checkpoint() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("MyGames/GZGA200015 Colors");
        fs::create_dir_all(directory.join("Save")).unwrap();
        fs::write(directory.join("config.cfg"), config()).unwrap();
        let path = directory.join("Save/SaveFile.isv");
        // Different checkpoints cover pending solo gains and solo spending.
        for (money, checkpoint) in [(525, 0), (1525, 525), (1000, 1525), (98000, 90000)] {
            let original = save(money, checkpoint);
            fs::write(&path, &original).unwrap();
            for _ in 0..20 {
                unlock_in_flash(temp.path()).unwrap();
                assert_eq!(fs::read(&path).unwrap(), original);
            }
        }
        let backups = fs::read_dir(&directory)
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "bak"))
            .count();
        assert_eq!(backups, 1);
    }
    #[test]
    fn missing_save_is_not_invented_and_corrupt_save_aborts_config_change() {
        let temp = tempfile::tempdir().unwrap();
        unlock_in_flash(temp.path()).unwrap();
        let directory = temp.path().join("MyGames/GZGA200015 Colors");
        assert!(!directory.join("Save").exists());
        fs::write(directory.join("config.cfg"), config()).unwrap();
        fs::create_dir(directory.join("Save")).unwrap();
        fs::write(directory.join("Save/SaveFile.isv"), b"corrupt").unwrap();
        assert!(unlock_in_flash(temp.path()).is_err());
        assert_eq!(fs::read(directory.join("config.cfg")).unwrap(), config());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
    }
    #[test]
    fn malformed_or_ambiguous_balances_are_rejected() {
        for plain in [
            "Save\n{\n}\n",
            "Save\n{\nCharacter\n{\nMoney -1\nServerMoney 0\n}\n}\n",
            "Save\n{\nCharacter\n{\nMoney 1\nMoney 2\nServerMoney 0\n}\n}\n",
        ] {
            assert!(validate_save(&transform(plain.as_bytes(), false)).is_err());
        }
    }
}
