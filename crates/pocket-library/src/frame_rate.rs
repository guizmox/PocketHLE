//! Global and optional per-game presentation ceiling; independent of video recording FPS.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FrameRateLimit {
    #[serde(rename = "10")]
    Fps10,
    #[serde(rename = "15")]
    Fps15,
    #[serde(rename = "20")]
    Fps20,
    #[serde(rename = "30")]
    Fps30,
    #[serde(rename = "45")]
    Fps45,
    #[default]
    #[serde(rename = "60", alias = "unlimited")]
    Fps60,
}

impl FrameRateLimit {
    pub const ALL: [Self; 6] = [
        Self::Fps10,
        Self::Fps15,
        Self::Fps20,
        Self::Fps30,
        Self::Fps45,
        Self::Fps60,
    ];

    pub fn fps(self) -> Option<u32> {
        match self {
            Self::Fps10 => Some(10),
            Self::Fps15 => Some(15),
            Self::Fps20 => Some(20),
            Self::Fps30 => Some(30),
            Self::Fps45 => Some(45),
            Self::Fps60 => Some(60),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fps10 => "10 fps",
            Self::Fps15 => "15 fps",
            Self::Fps20 => "20 fps",
            Self::Fps30 => "30 fps",
            Self::Fps45 => "45 fps",
            Self::Fps60 => "60 fps",
        }
    }
}

/// Old explicit unlimited game values now inherit the emulator's bounded limit.
pub fn deserialize_game_limit<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<FrameRateLimit>, D::Error> {
    match Option::<String>::deserialize(deserializer)? {
        None => Ok(None),
        Some(value) if value == "unlimited" || value == "inherit" => Ok(None),
        Some(value) => FrameRateLimit::deserialize(
            serde::de::value::StringDeserializer::<D::Error>::new(value),
        )
        .map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameSettings;

    #[test]
    fn old_settings_keep_the_existing_presentation_policy() {
        let old: GameSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.frame_rate_limit, None);
        assert_eq!(
            old.effective_frame_rate_limit(&crate::LauncherConfig::default()),
            Some(60)
        );
    }

    #[test]
    fn all_games_inherit_the_global_limit_unless_they_choose_a_bounded_override() {
        for global in FrameRateLimit::ALL {
            let mut config = crate::LauncherConfig::default();
            config.frame_rate_limit = global;
            let mut settings: GameSettings = serde_json::from_str("{}").unwrap();
            assert_eq!(settings.effective_frame_rate_limit(&config), global.fps());
            for value in FrameRateLimit::ALL {
                settings.frame_rate_limit = Some(value);
                assert_eq!(settings.effective_frame_rate_limit(&config), value.fps());
            }
            let explicit: GameSettings =
                serde_json::from_str(r#"{"frame_rate_limit":"unlimited"}"#).unwrap();
            assert_eq!(explicit.frame_rate_limit, None);
            assert_eq!(explicit.effective_frame_rate_limit(&config), global.fps());
            let inherit: GameSettings =
                serde_json::from_str(r#"{"frame_rate_limit":null}"#).unwrap();
            assert_eq!(inherit.effective_frame_rate_limit(&config), global.fps());
        }
    }

    #[test]
    fn global_default_and_legacy_unlimited_are_capped_at_sixty() {
        assert_eq!(FrameRateLimit::default(), FrameRateLimit::Fps60);
        for legacy in ["{}", r#"{"frame_rate_limit":"unlimited"}"#] {
            let config: crate::LauncherConfig = serde_json::from_str(legacy).unwrap();
            assert_eq!(config.frame_rate_limit, FrameRateLimit::Fps60);
            assert_eq!(
                serde_json::to_value(config).unwrap()["frame_rate_limit"],
                "60"
            );
        }
        for value in FrameRateLimit::ALL {
            assert!(value.fps().is_some_and(|fps| fps <= 60));
        }
    }

    #[test]
    fn legacy_halt_setting_cannot_be_reenabled_by_old_game_json() {
        let settings: GameSettings =
            serde_json::from_str(r#"{"halt_on_unimplemented":true}"#).unwrap();
        assert!(serde_json::to_value(settings)
            .unwrap()
            .get("halt_on_unimplemented")
            .is_none());
    }

    #[test]
    fn per_game_limits_round_trip_without_losing_other_options() {
        for value in FrameRateLimit::ALL {
            let mut settings = GameSettings::default();
            settings.frame_rate_limit = Some(value);
            settings.pocketpc_owner_name = "Example owner".into();
            let saved = serde_json::to_string(&settings).unwrap();
            let restored: GameSettings = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.frame_rate_limit, Some(value));
            assert_eq!(restored.pocketpc_owner_name, "Example owner");
        }
        assert!(serde_json::from_str::<GameSettings>(r#"{"frame_rate_limit":"0"}"#).is_err());
    }
}
