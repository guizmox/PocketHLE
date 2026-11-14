//! Direct CE application launch without importing or rewriting its directory.
use anyhow::{Context, Result};
use pocket_library::{
    GameEntry, GamePlatform, GameSettings, ScreenPref, WindowsCeMount, WindowsCeSettings,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CeLaunch {
    pub profile_name: String,
    pub executable: String,
    pub image_root: String,
    pub data_dir: String,
    pub app_prefix: String,
    pub command_line: String,
    pub screen: ScreenPref,
    pub classic_windows: bool,
    pub mounts: Vec<WindowsCeMount>,
}
impl Default for CeLaunch {
    fn default() -> Self {
        Self {
            profile_name: String::new(),
            executable: String::new(),
            image_root: String::new(),
            data_dir: String::new(),
            app_prefix: "\\Application\\".into(),
            command_line: String::new(),
            screen: ScreenPref::VgaLandscape,
            classic_windows: true,
            mounts: Vec::new(),
        }
    }
}

pub enum Action {
    PickExecutable,
    PickImage,
    PickData,
    SaveProfile,
    Run,
    Back,
}
impl CeLaunch {
    pub fn load(root: &Path) -> Self {
        match std::fs::read(root.join("windows-ce.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("Cannot read CE launcher settings: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        std::fs::write(
            root.join("windows-ce.json"),
            serde_json::to_vec_pretty(self)?,
        )
        .context("saving CE launch settings")
    }
    pub fn load_profile(root: &Path, source: &str) -> Result<Self> {
        if Path::new(source).is_file() {
            let path = Path::new(source).canonicalize()?;
            let mut profile: Self = serde_json::from_slice(&std::fs::read(&path)?)
                .context("reading Windows CE profile")?;
            let directory = path.parent().context("profile has no parent directory")?;
            for value in [
                &mut profile.executable,
                &mut profile.image_root,
                &mut profile.data_dir,
            ] {
                if !value.trim().is_empty() && Path::new(value.trim()).is_relative() {
                    *value = directory.join(value.trim()).to_string_lossy().into_owned();
                }
            }
            for mount in &mut profile.mounts {
                if mount.host_dir.is_relative() {
                    mount.host_dir = directory.join(&mount.host_dir);
                }
            }
            return Ok(profile);
        }
        CeProfiles::load(root)
            .entries
            .get(source)
            .cloned()
            .with_context(|| format!("Windows CE profile {source:?} was not found"))
    }
    pub fn entry(&self) -> Result<GameEntry> {
        let executable = Path::new(self.executable.trim())
            .canonicalize()
            .context("select a CE executable")?;
        anyhow::ensure!(executable.is_file(), "The executable path must name a file");
        let image_root = optional_path(&self.image_root);
        if let Some(path) = &image_root {
            anyhow::ensure!(path.is_dir(), "The CE image directory does not exist");
        }
        let app_prefix = app_prefix(&self.app_prefix)?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        executable.hash(&mut hash);
        self.profile_name.hash(&mut hash);
        image_root.hash(&mut hash);
        self.data_dir.hash(&mut hash);
        let display_name = executable
            .file_stem()
            .context("executable has no file name")?
            .to_string_lossy()
            .into_owned();
        Ok(GameEntry {
            platform: Some(GamePlatform::PocketPc),
            id: format!("ce-{:016x}", hash.finish()),
            display_name,
            provider: Some("Windows CE".into()),
            executable,
            source_cab: "direct-ce".into(),
            install_dir: None,
            install_dirs: Vec::new(),
            save_prefix: None,
            registry: Vec::new(),
            imported_at: 0,
            settings: GameSettings {
                windows_ce: Some(WindowsCeSettings {
                    profile_name: self.profile_name.clone(),
                    image_root,
                    data_dir: optional_path(&self.data_dir),
                    app_prefix,
                    command_line: self.command_line.clone(),
                    classic_windows: self.classic_windows,
                    mounts: self.mounts.clone(),
                }),
                screen: self.screen,
                max_slices: 0,
                ..GameSettings::default()
            },
            icon: None,
            companions: Vec::new(),
        })
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, profiles: &CeProfiles) -> Option<Action> {
        let mut action = None;
        ui.heading("Windows CE");
        ui.label("Run an ARM CE application from its existing folder.");
        ui.horizontal(|ui| {
            ui.label("Profile");
            egui::ComboBox::from_id_source("ce_profile")
                .selected_text(if self.profile_name.is_empty() {
                    "Custom"
                } else {
                    &self.profile_name
                })
                .show_ui(ui, |ui| {
                    for (name, profile) in &profiles.entries {
                        if crate::controller_ui::add_enabled(
                            ui,
                            true,
                            egui::SelectableLabel::new(self.profile_name == *name, name),
                        )
                        .clicked()
                        {
                            *self = profile.clone();
                        }
                    }
                });
            if crate::controller_ui::button(ui, "New profile").clicked() {
                *self = Self::default();
            }
        });
        ui.horizontal(|ui| {
            crate::controller_ui::text_edit(ui, &mut self.profile_name, "Profile name");
            if crate::controller_ui::add_enabled(
                ui,
                !self.profile_name.trim().is_empty(),
                egui::Button::new("Save profile"),
            )
            .clicked()
            {
                action = Some(Action::SaveProfile);
            }
        });
        ui.weak("Each profile keeps its own device files and registry settings.");
        ui.add_space(12.0);
        ui.label("Application EXE");
        ui.horizontal(|ui| {
            crate::controller_ui::text_edit(ui, &mut self.executable, "Path to the EXE");
            if crate::controller_ui::button(ui, "Browse…").clicked() {
                action = Some(Action::PickExecutable);
            }
        });
        ui.label("Extracted CE image (optional)");
        ui.horizontal(|ui| {
            crate::controller_ui::text_edit(
                ui,
                &mut self.image_root,
                "Folder containing Windows or fs/Windows",
            );
            if crate::controller_ui::button(ui, "Choose folder…").clicked() {
                action = Some(Action::PickImage);
            }
        });
        ui.weak("The image supplies system resources, fonts and registry settings.");
        ui.label("Device data folder");
        ui.horizontal(|ui| {
            crate::controller_ui::text_edit(
                ui,
                &mut self.data_dir,
                "Automatic: separate device data in the library",
            );
            if crate::controller_ui::button(ui, "Browse…").clicked() {
                action = Some(Action::PickData);
            }
        });
        ui.label("Application folder in CE");
        crate::controller_ui::text_edit(
            ui,
            &mut self.app_prefix,
            "\\Application\\ or \\Storage Card\\",
        );
        ui.weak("The EXE's folder and its companion DLLs appear at this path.");
        ui.label("Application arguments (optional)");
        crate::controller_ui::text_edit(
            ui,
            &mut self.command_line,
            "Command line passed to the CE application",
        );
        ui.label("Additional application folders");
        let mut remove = None;
        for (index, mount) in self.mounts.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    crate::controller_ui::text_edit(
                        ui,
                        &mut mount.guest_prefix,
                        "Folder in CE, for example \\CERF\\",
                    );
                    let mut host = mount.host_dir.to_string_lossy().into_owned();
                    crate::controller_ui::text_edit(ui, &mut host, "Folder on this computer");
                    mount.host_dir = PathBuf::from(host);
                    if crate::controller_ui::button(ui, "Remove").clicked() {
                        remove = Some(index);
                    }
                })
            });
        }
        if let Some(index) = remove {
            self.mounts.remove(index);
        }
        if crate::controller_ui::button(ui, "Add folder").clicked() {
            self.mounts.push(WindowsCeMount {
                guest_prefix: "\\CERF\\".into(),
                host_dir: PathBuf::new(),
            });
        }
        ui.weak("Programs share these folders. Changes are saved in the device data folder.");
        ui.add_space(8.0);
        crate::controller_ui::checkbox(
            ui,
            &mut self.classic_windows,
            "Classic CE window frames and caption buttons",
        );
        ui.horizontal(|ui| {
            ui.label("Screen");
            egui::ComboBox::from_id_source("ce_screen")
                .selected_text(self.screen.label())
                .show_ui(ui, |ui| {
                    for screen in [
                        ScreenPref::VgaLandscape,
                        ScreenPref::Landscape,
                        ScreenPref::Hpc,
                        ScreenPref::Qvga,
                        ScreenPref::Vga,
                    ] {
                        crate::controller_ui::selectable_value(
                            ui,
                            &mut self.screen,
                            screen,
                            screen.label(),
                        );
                    }
                });
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if crate::controller_ui::add_enabled(
                ui,
                !self.executable.trim().is_empty(),
                egui::Button::new("Run application"),
            )
            .clicked()
            {
                action = Some(Action::Run);
            }
            if crate::controller_ui::button(ui, "Back to library").clicked() {
                action = Some(Action::Back);
            }
        });
        action
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CeProfiles {
    pub entries: BTreeMap<String, CeLaunch>,
}
impl CeProfiles {
    pub fn load(root: &Path) -> Self {
        match std::fs::read(root.join("windows-ce-profiles.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                log::warn!("Cannot read CE profiles: {error}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
    pub fn remember(&mut self, root: &Path, launch: &mut CeLaunch) -> Result<()> {
        let name = launch.profile_name.trim().to_owned();
        anyhow::ensure!(!name.is_empty(), "Enter a profile name");
        launch.profile_name = name.clone();
        if launch.data_dir.trim().is_empty() {
            launch.data_dir = root
                .join("devices")
                .join("ce-profiles")
                .join(device_slug(&name))
                .to_string_lossy()
                .into_owned();
        }
        let previous = self.entries.insert(name.clone(), launch.clone());
        if let Err(error) = std::fs::write(
            root.join("windows-ce-profiles.json"),
            serde_json::to_vec_pretty(self)?,
        ) {
            match previous {
                Some(previous) => {
                    self.entries.insert(name, previous);
                }
                None => {
                    self.entries.remove(&name);
                }
            }
            return Err(error).context("saving Windows CE profiles");
        }
        Ok(())
    }
}
fn device_slug(name: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in name.as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    let label: String = name
        .chars()
        .take(32)
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("{}-{hash:016x}", label.trim_matches('-'))
}
fn optional_path(value: &str) -> Option<PathBuf> {
    (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()))
}
pub fn app_prefix(value: &str) -> Result<String> {
    let value = value.trim().replace('/', "\\");
    anyhow::ensure!(
        value.starts_with('\\')
            && !value.contains([':', '\0'])
            && !value.split('\\').any(|p| p == "." || p == ".."),
        "Use an absolute CE folder such as \\Application\\"
    );
    Ok(format!("{}\\", value.trim_end_matches('\\')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_keep_distinct_device_directories_and_saved_launch_settings() {
        let root = tempfile::tempdir().unwrap();
        let mut profiles = CeProfiles::default();
        let mut ce6 = CeLaunch {
            profile_name: "Windows CE 6".into(),
            image_root: "image/fs".into(),
            ..Default::default()
        };
        profiles.remember(root.path(), &mut ce6).unwrap();
        let ce6_data = ce6.data_dir.clone();
        let mut demo = CeLaunch {
            profile_name: "CERF demo".into(),
            ..Default::default()
        };
        profiles.remember(root.path(), &mut demo).unwrap();
        assert_ne!(ce6_data, demo.data_dir);
        let reopened = CeLaunch::load_profile(root.path(), "Windows CE 6").unwrap();
        assert_eq!(reopened, ce6);
        assert_eq!(reopened.image_root, "image/fs");
        assert!(reopened.classic_windows);
        assert!(!Path::new(&ce6_data).exists()); // Saving a profile does not boot it.
        assert!(CeLaunch::load_profile(root.path(), "Missing").is_err());
    }

    #[test]
    fn older_launch_settings_and_portable_profile_files_are_readable() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("windows-ce.json"),
            r#"{"executable":"test.exe"}"#,
        )
        .unwrap();
        let launch = CeLaunch::load(root.path());
        assert!(launch.profile_name.is_empty());
        assert_eq!(launch.executable, "test.exe");
        assert_eq!(launch.screen, ScreenPref::VgaLandscape);
        let file = root.path().join("ce6.json");
        std::fs::write(&file, serde_json::to_vec(&launch).unwrap()).unwrap();
        // Portable profiles resolve the EXE beside the JSON, independently of
        // the host working directory. Legacy launcher settings stay relative.
        let expected = CeLaunch {
            executable: root
                .path()
                .canonicalize()
                .unwrap()
                .join("test.exe")
                .to_string_lossy()
                .into_owned(),
            ..launch
        };
        assert_eq!(
            CeLaunch::load_profile(root.path(), file.to_str().unwrap()).unwrap(),
            expected
        );
        assert_ne!(device_slug("a/b"), device_slug("a\\b"));
        assert_ne!(device_slug("Демо"), device_slug("Другой"));
    }
}
