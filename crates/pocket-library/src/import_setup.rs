//! One-time, post-import launcher setup. Never changes installed game data.
use crate::{
    GameEntry, LauncherConfig, LauncherProfile, PocketPcControlsSize, PocketPcTouchLayout,
};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Gizmondo,
    PocketPc,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Completed {
    #[serde(default)]
    pub gizmondo: bool,
    #[serde(default)]
    pub pocket_pc: bool,
}
impl Completed {
    pub fn contains(&self, platform: Platform) -> bool {
        match platform {
            Platform::Gizmondo => self.gizmondo,
            Platform::PocketPc => self.pocket_pc,
        }
    }
    pub fn requests(&self, before: Presence, imported: Presence) -> Vec<Platform> {
        [
            (Platform::Gizmondo, before.gizmondo, imported.gizmondo),
            (Platform::PocketPc, before.pocket_pc, imported.pocket_pc),
        ]
        .into_iter()
        .filter_map(|(p, had, added)| (!had && added && !self.contains(p)).then_some(p))
        .collect()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Presence {
    pub gizmondo: bool,
    pub pocket_pc: bool,
}
impl Presence {
    pub fn read(entries: &[GameEntry], root: &Path) -> Self {
        let mut result = Self::default();
        for entry in entries {
            if crate::is_gizmondo_game(entry, root) {
                result.gizmondo = true;
            } else {
                result.pocket_pc = true;
            }
        }
        result
    }
}

/// Missing choices mean "keep current settings". Android-only fields are not
/// emitted by desktop. Saving re-reads the latest file, not the dialog snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub enum Choices {
    Gizmondo {
        #[serde(default)]
        supersampling: Option<bool>,
        #[serde(default)]
        camera: Option<bool>,
        #[serde(default)]
        bluetooth: Option<bool>,
        #[serde(default)]
        location: Option<bool>,
        #[serde(default)]
        data: Option<bool>,
        #[serde(default)]
        controls_size: Option<PocketPcControlsSize>,
        #[serde(default)]
        overlap: Option<bool>,
    },
    PocketPc {
        #[serde(default)]
        layout: Option<PocketPcTouchLayout>,
        #[serde(default)]
        controls_size: Option<PocketPcControlsSize>,
    },
}
impl Choices {
    pub fn platform(&self) -> Platform {
        match self {
            Self::Gizmondo { .. } => Platform::Gizmondo,
            Self::PocketPc { .. } => Platform::PocketPc,
        }
    }
    fn apply(&self, config: &mut LauncherConfig) {
        match self {
            Self::Gizmondo {
                supersampling,
                camera,
                bluetooth,
                location,
                data,
                controls_size,
                overlap,
            } => {
                if let Some(enabled) = supersampling {
                    if *enabled {
                        config.gpu_renderer = true;
                        // Preserve an already chosen 3x/4x policy.
                        if config.gles_internal_resolution <= 1 {
                            config.gles_internal_resolution = 3;
                        }
                    } else {
                        config.gles_internal_resolution = 1;
                    }
                }
                if let Some(v) = camera {
                    config.camera_enabled = *v;
                }
                if let Some(v) = bluetooth {
                    config.bluetooth_enabled = *v;
                }
                if let Some(v) = location {
                    config.gps_enabled = *v;
                }
                if let Some(v) = data {
                    config.gprs_enabled = *v;
                }
                if let Some(v) = controls_size {
                    config.android_gizmondo_controls_size = *v;
                }
                if let Some(v) = overlap {
                    config.android_gizmondo_maximize_screen = *v;
                }
                config.first_import_setup.gizmondo = true;
            }
            Self::PocketPc {
                layout,
                controls_size,
            } => {
                if let Some(v) = layout {
                    config.android_pocketpc_touch_layout = *v;
                }
                if let Some(v) = controls_size {
                    config.android_pocketpc_controls_size = *v;
                }
                config.first_import_setup.pocket_pc = true;
            }
        }
    }
}

/// Atomically replace settings only, preserving unrelated/unknown JSON members.
/// A repeated completion cannot replay stale choices after an Activity restart.
pub fn save(
    root: &Path,
    profile: LauncherProfile,
    choice: &Choices,
) -> anyhow::Result<LauncherConfig> {
    let path = root.join(profile.config_file_name());
    let mut raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    let mut config: LauncherConfig = serde_json::from_value(raw.clone())?;
    if config.first_import_setup.contains(choice.platform()) {
        return Ok(config);
    }
    let before = serde_json::to_value(&config)?;
    choice.apply(&mut config);
    let after = serde_json::to_value(&config)?;
    for (key, value) in after
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid settings object"))?
    {
        if key != "first_import_setup" && before.get(key) != Some(value) {
            raw[key] = value.clone();
        }
    }
    let field = match choice.platform() {
        Platform::Gizmondo => "gizmondo",
        Platform::PocketPc => "pocket_pc",
    };
    if !raw["first_import_setup"].is_object() {
        raw["first_import_setup"] = serde_json::json!({});
    }
    raw["first_import_setup"][field] = true.into();
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer_pretty(file.as_file_mut(), &raw)?;
    file.as_file_mut().flush()?;
    file.as_file().sync_all()?;
    file.persist(&path).map_err(|error| error.error)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_successful_batch_requests_once_per_new_platform() {
        let done = Completed::default();
        let both = Presence {
            gizmondo: true,
            pocket_pc: true,
        };
        assert_eq!(
            done.requests(Presence::default(), both),
            vec![Platform::Gizmondo, Platform::PocketPc]
        );
        assert!(
            done.requests(both, both).is_empty(),
            "existing titles and updates do not start setup"
        );
        assert!(
            done.requests(Presence::default(), Presence::default())
                .is_empty(),
            "cancelled/empty imports"
        );
        let done = Completed {
            gizmondo: true,
            pocket_pc: false,
        };
        assert_eq!(
            done.requests(Presence::default(), both),
            vec![Platform::PocketPc]
        );
    }
    #[test]
    fn completion_preserves_unknown_settings_and_does_not_touch_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        let mut library = crate::Library::open(dir.path()).unwrap();
        library.config_mut().gles_internal_resolution = 5;
        library.save().unwrap();
        let path = dir.path().join("config.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["future_setting"] = serde_json::json!({"keep": 42});
        value["first_import_setup"]["future_platform"] = true.into();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let catalogue = std::fs::read(dir.path().join("library.json")).unwrap();
        let choice = serde_json::from_value(
            serde_json::json!({"platform":"gizmondo", "supersampling":true,"camera":false}),
        )
        .unwrap();
        let config = save(dir.path(), LauncherProfile::Standard, &choice).unwrap();
        assert_eq!(config.gles_internal_resolution, 5);
        assert!(!config.camera_enabled);
        assert!(config.first_import_setup.gizmondo);
        let updated: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(updated["future_setting"], value["future_setting"]);
        assert_eq!(updated["first_import_setup"]["future_platform"], true);
        assert_eq!(
            catalogue,
            std::fs::read(dir.path().join("library.json")).unwrap()
        );
        let stale =
            serde_json::from_value(serde_json::json!({"platform":"gizmondo","camera":true}))
                .unwrap();
        assert!(
            !save(dir.path(), LauncherProfile::Standard, &stale)
                .unwrap()
                .camera_enabled
        );
    }
    #[test]
    fn keep_current_and_android_choices_are_profile_scoped() {
        let dir = tempfile::tempdir().unwrap();
        crate::Library::open(dir.path()).unwrap();
        crate::Library::open_with_profile(dir.path(), LauncherProfile::SteamDeck).unwrap();
        let before = crate::Library::open(dir.path()).unwrap().config().clone();
        let keep = serde_json::from_value(serde_json::json!({"platform":"gizmondo"})).unwrap();
        let after = save(dir.path(), LauncherProfile::Standard, &keep).unwrap();
        assert_eq!(
            after.gles_internal_resolution,
            before.gles_internal_resolution
        );
        assert_eq!(after.camera_enabled, before.camera_enabled);
        assert!(
            !crate::Library::open_with_profile(dir.path(), LauncherProfile::SteamDeck)
                .unwrap()
                .config()
                .first_import_setup
                .gizmondo
        );
        let mobile = serde_json::from_value(
            serde_json::json!({"platform":"pocket_pc","layout":"controller","controls_size":"big"}),
        )
        .unwrap();
        let after = save(dir.path(), LauncherProfile::Standard, &mobile).unwrap();
        assert_eq!(
            after.android_pocketpc_touch_layout,
            PocketPcTouchLayout::Controller
        );
        assert_eq!(
            after.android_pocketpc_controls_size,
            PocketPcControlsSize::Big
        );
        assert!(after.first_import_setup.pocket_pc);
        assert_eq!(
            after.android_gizmondo_controls_size,
            before.android_gizmondo_controls_size
        );
    }
}
