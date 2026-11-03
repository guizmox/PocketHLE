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
    #[serde(rename = "60")]
    Fps60,
    #[serde(rename = "unlimited")]
    Unlimited,
}

impl FrameRateLimit {
    pub const ALL: [Self; 7] = [
        Self::Fps10,
        Self::Fps15,
        Self::Fps20,
        Self::Fps30,
        Self::Fps45,
        Self::Fps60,
        Self::Unlimited,
    ];

    /// Some(0) disables implicit presentation pacing; None retains native LCD timing.
    pub fn fps(self) -> Option<u32> {
        match self {
            Self::Fps10 => Some(10),
            Self::Fps15 => Some(15),
            Self::Fps20 => Some(20),
            Self::Fps30 => Some(30),
            Self::Fps45 => Some(45),
            Self::Fps60 => Some(60),
            Self::Unlimited => Some(0),
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
            Self::Unlimited => "Unlimited",
        }
    }
}

/// Missing/null/inherit select the emulator setting; unlimited is an explicit override.
pub fn deserialize_game_limit<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<FrameRateLimit>, D::Error> {
    match Option::<String>::deserialize(deserializer)? {
        None => Ok(None),
        Some(value) if value == "inherit" => Ok(None),
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
    fn games_inherit_global_limit_or_choose_explicit_bounded_or_unlimited_override() {
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
            assert_eq!(explicit.frame_rate_limit, Some(FrameRateLimit::Unlimited));
            assert_eq!(explicit.effective_frame_rate_limit(&config), Some(0));
            let inherit: GameSettings =
                serde_json::from_str(r#"{"frame_rate_limit":null}"#).unwrap();
            assert_eq!(inherit.effective_frame_rate_limit(&config), global.fps());
        }
    }

    #[test]
    fn unlimited_is_explicit_and_default_stays_sixty() {
        assert_eq!(FrameRateLimit::default(), FrameRateLimit::Fps60);
        let default: crate::LauncherConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(default.frame_rate_limit, FrameRateLimit::Fps60);
        let config: crate::LauncherConfig =
            serde_json::from_str(r#"{"frame_rate_limit":"unlimited"}"#).unwrap();
        assert_eq!(config.frame_rate_limit, FrameRateLimit::Unlimited);
        assert_eq!(config.frame_rate_limit.fps(), Some(0));
        assert_eq!(
            serde_json::to_value(config).unwrap()["frame_rate_limit"],
            "unlimited"
        );
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
