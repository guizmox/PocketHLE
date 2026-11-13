//! Per-game PocketPC GPU compatibility and the single post-import settings hint.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuCompatibilityProfile {
    #[default]
    Generic,
    NvidiaGoforce5500,
    Intel2700g,
    #[serde(alias = "qualcomm_snapdragon")]
    Adreno200,
}
impl GpuCompatibilityProfile {
    pub const ALL: [Self; 4] = [
        Self::Generic,
        Self::NvidiaGoforce5500,
        Self::Intel2700g,
        Self::Adreno200,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Generic => "Generic OpenGL ES (1.1, Common)",
            Self::NvidiaGoforce5500 => "NVIDIA GoForce 5500 (OpenGL ES 1.1, Common)",
            Self::Intel2700g => "Intel 2700G (OpenGL ES 1.0, Common-Lite)",
            Self::Adreno200 => "Generic OpenGL ES (2.0, Common)",
        }
    }
    pub fn nvidia_combiners(self) -> bool {
        self == Self::NvidiaGoforce5500
    }
    pub fn intel_depth_bias(self) -> bool {
        self == Self::Intel2700g
    }

    pub fn programmable_gles(self) -> bool {
        self == Self::Adreno200
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ImportAdvice {
    pub may_run_fast: bool,
    pub choose_gpu_profile: bool,
    pub show_display_advice: bool,
}
impl ImportAdvice {
    pub fn for_game(
        game: &crate::GameEntry,
        root: &std::path::Path,
        show_display_advice: bool,
    ) -> Option<Self> {
        if crate::is_gizmondo_game(game, root) {
            return None;
        }
        let path = game.launch_path(root);
        let label = crate::analysis::detect_graphics_api_cached(&path);
        let choose_gpu_profile = matches!(label, Some("OpenGL ES" | "OpenGL ES CM" | "OpenGL ES 1.1 CM"))
            && !pocket_pe::load_file(&path)
                .ok()
                .is_some_and(|image| crate::uses_programmable_gles(&image));
        (show_display_advice || choose_gpu_profile).then(|| Self {
            may_run_fast: show_display_advice && crate::frame_pacing::may_run_fast(&path),
            choose_gpu_profile,
            show_display_advice,
        })
    }
    pub fn message(self) -> String {
        let mut paragraphs = Vec::new();
        if self.show_display_advice {
            paragraphs.push("If the image is sideways, stretched, or cut off, adjust Screen and Rotate display in the game settings.");
        }
        if self.choose_gpu_profile {
            paragraphs.push("This executable uses OpenGL ES. Keep the current GPU compatibility profile if you are unsure which one the game needs.");
        }
        if self.may_run_fast {
            paragraphs.push("This game may run too fast at higher frame rates. If that happens, try setting an FPS limit in the game settings.");
        }
        paragraphs.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_round_trip_and_keep_vendor_behaviors_exclusive() {
        let default: crate::GameSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            default.gpu_compatibility_profile,
            GpuCompatibilityProfile::Generic
        );
        for (profile, flags) in GpuCompatibilityProfile::ALL.into_iter().zip([
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ]) {
            assert_eq!(
                (
                    profile.nvidia_combiners(),
                    profile.intel_depth_bias(),
                    profile.programmable_gles(),
                ),
                flags
            );
            let settings = crate::GameSettings {
                gpu_compatibility_profile: profile,
                ..Default::default()
            };
            let restored: crate::GameSettings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored, settings);
            assert!(!(profile.nvidia_combiners() && profile.intel_depth_bias()));
            assert_eq!(
                settings.has_custom_settings(&crate::LauncherConfig::default()),
                profile != GpuCompatibilityProfile::Generic
            );
        }
    }
    #[test]
    fn retired_global_options_are_not_saved_or_inherited() {
        let config: crate::LauncherConfig = serde_json::from_str(
            r#"{"pocketpc_intel2700g_depth_bias":true,"pocketpc_nv_combiners":true}"#,
        )
        .unwrap();
        let json = serde_json::to_value(config).unwrap();
        assert!(json.get("pocketpc_intel2700g_depth_bias").is_none());
        assert!(json.get("pocketpc_nv_combiners").is_none());
        let ids = ["generic", "nvidia_goforce5500", "intel2700g", "adreno200"];
        for (profile, id) in GpuCompatibilityProfile::ALL.into_iter().zip(ids) {
            assert_eq!(serde_json::to_value(profile).unwrap(), id);
        }
    }

    #[test]
    fn import_message_keeps_display_advice_and_adds_gpu_choice_when_needed() {
        for may_run_fast in [false, true] {
            for uses_opengl_es in [false, true] {
                let text = ImportAdvice {
                    may_run_fast,
                    choose_gpu_profile: uses_opengl_es,
                    show_display_advice: true,
                }
                .message();
                assert_eq!(text.contains("GPU compatibility profile"), uses_opengl_es);
                assert_eq!(text.contains("FPS limit"), may_run_fast);
                assert!(text.contains("Rotate display"));
            }
        }
    }
}
