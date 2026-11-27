//! Writable SD-card backing shared by the desktop and Android launchers.
use crate::GameEntry;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

fn relative(prefix: &str) -> Option<PathBuf> {
    let prefix = prefix.replace('\\', "/");
    let mut parts = prefix.split('/').filter(|p| !p.is_empty());
    if !matches!(
        parts.next()?.to_ascii_lowercase().as_str(),
        "sd card" | "storage card"
    ) {
        return None;
    }
    let suffix = parts.collect::<Vec<_>>().join("/");
    if suffix.is_empty() {
        Some(PathBuf::new())
    } else {
        crate::backup::safe_path(&suffix).ok()
    }
}
fn normalized(prefix: &str) -> String {
    prefix
        .replace('\\', "/")
        .trim_matches('/')
        .to_ascii_lowercase()
}

impl GameEntry {
    /// Native Gizmondo games may save on the removable card as well as Flash
    /// Disk. SSX writes options.dat and ssxtmp.sp at the SD-card root; mounting
    /// only Flash Disk writable makes it report "Game deck memory full".
    /// Mirror each card alias so specific read-only install mounts cannot
    /// hide a modified file behind a broader writable overlay. Original card
    /// files remain unchanged; both SD-card spellings share persistent data.
    pub fn gizmondo_card_save_mounts(&self, library_root: &Path) -> Vec<(String, PathBuf)> {
        if !crate::is_gizmondo_game(self, library_root) {
            return Vec::new();
        }
        let card = self.gizmondo_storage_card_root(library_root);
        let extracted = self.extracted_dir(library_root);
        let saves = self.save_dir(library_root);
        let overlay = saves.join("sd-card");
        let mut aliases = vec![
            ("\\SD Card\\".into(), card.clone()),
            ("\\Storage Card\\".into(), card.clone()),
        ];
        if self.settings.storage_card_dir.is_none() {
            aliases.push(("\\SD Card\\Game\\".into(), extracted.clone()));
            aliases.push(("\\Storage Card\\Game\\".into(), extracted));
        }
        aliases.extend(self.install_mounts(library_root));
        // Retain the established save backing of explicit SD-card installs.
        let explicit = self.guest_launch_paths(library_root).and_then(|paths| {
            relative(&paths.save_prefix)?;
            aliases
                .iter()
                .find(|(p, _)| normalized(p) == normalized(&paths.save_prefix))
                .map(|(_, p)| p.clone())
        });
        let mut seen = BTreeSet::new();
        aliases
            .into_iter()
            .filter_map(|(prefix, lower)| {
                let suffix = relative(&prefix)?;
                if !seen.insert(normalized(&prefix)) {
                    return None;
                }
                let upper = explicit
                    .as_ref()
                    .and_then(|base| lower.strip_prefix(base).ok())
                    .map(|rel| saves.join(rel))
                    .unwrap_or_else(|| overlay.join(lower.strip_prefix(&card).unwrap_or(&suffix)));
                Some((prefix, upper))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GamePlatform, GameSettings};
    use pocket_kernel::vfs::Vfs;
    fn game() -> GameEntry {
        serde_json::from_value(serde_json::json!({
            "platform":"gizmondo", "id":"card", "display_name":"Card",
            "executable":"extracted/ssx.exe", "source_cab":"card.vhd", "imported_at":0,
            "settings": GameSettings::default()
        }))
        .unwrap()
    }
    #[test]
    fn card_root_writes_persist_through_aliases_without_changing_assets() {
        let root = tempfile::tempdir().unwrap();
        let game = game();
        let card = game.extracted_dir(root.path());
        std::fs::create_dir_all(&card).unwrap();
        std::fs::write(card.join("options.dat"), b"original").unwrap();
        let mut v = Vfs::default();
        for prefix in [
            "\\SD Card\\",
            "\\Storage Card\\",
            "\\SD Card\\Game\\",
            "\\Storage Card\\Game\\",
        ] {
            v.mount_read_only(prefix, &card);
        }
        assert!(v
            .open_file("\\SD Card\\ssxtmp.sp", 2, 3, 2, false, 0)
            .is_err());
        for (prefix, dir) in game.gizmondo_card_save_mounts(root.path()) {
            v.mount_save_dir(&prefix, dir);
        }
        for path in ["\\SD Card\\ssxtmp.sp", "\\Storage Card\\Game\\options.dat"] {
            let handle = v.open_file(path, 2, 3, 2, false, 0).unwrap().handle;
            assert_eq!(v.write(handle, b"saved"), Some(5));
            assert!(v.close(handle));
        }
        for path in [
            "\\Storage Card\\ssxtmp.sp",
            "\\SD Card\\options.dat",
            "\\SD Card\\Game\\options.dat",
        ] {
            let handle = v.open_file(path, 1, 3, 3, false, 0).unwrap().handle;
            let mut bytes = [0; 5];
            assert_eq!(v.read(handle, &mut bytes), Some(5));
            assert_eq!(&bytes, b"saved");
            assert!(v.close(handle));
        }
        assert_eq!(
            std::fs::read(card.join("options.dat")).unwrap(),
            b"original"
        );
        assert!(!card.join("ssxtmp.sp").exists());
        assert_eq!(
            std::fs::read(game.save_dir(root.path()).join("sd-card/options.dat")).unwrap(),
            b"saved"
        );
        assert!(v.disk_space("\\SD Card\\").unwrap().1 > 0);
    }
    #[test]
    fn custom_cards_and_explicit_save_paths_remain_separate() {
        let root = tempfile::tempdir().unwrap();
        let mut game = game();
        let external = root.path().join("external");
        std::fs::create_dir_all(&external).unwrap();
        game.settings.storage_card_dir = Some(external);
        let mounts = game.gizmondo_card_save_mounts(root.path());
        assert!(!mounts
            .iter()
            .any(|(p, _)| p.to_ascii_lowercase().contains("\\game\\")));
        assert_eq!(mounts[0].1, mounts[1].1);
        game.settings.storage_card_dir = None;
        game.install_dir = Some("\\SD Card\\".into());
        let mounts = game.gizmondo_card_save_mounts(root.path());
        assert_eq!(mounts[0].1, game.save_dir(root.path()));
        assert_eq!(mounts[1].1, game.save_dir(root.path()));
        game.platform = Some(GamePlatform::PocketPc);
        assert!(game.gizmondo_card_save_mounts(root.path()).is_empty());
    }
}
