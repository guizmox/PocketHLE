//! Optional per-game presentation ceiling; independent of video recording FPS.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FrameRateLimit {
    #[default]
    #[serde(rename = "unlimited")]
    Unlimited,
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
    #[serde(rename = "60")]
    Fps60,
}

impl FrameRateLimit {
    pub const ALL: [Self; 7] = [
        Self::Unlimited,
        Self::Fps10,
        Self::Fps15,
        Self::Fps20,
        Self::Fps30,
        Self::Fps45,
        Self::Fps60,
    ];

    pub fn fps(self) -> Option<u32> {
        match self {
            Self::Unlimited => None,
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
            Self::Unlimited => "Unlimited",
            Self::Fps10 => "10 fps",
            Self::Fps15 => "15 fps",
            Self::Fps20 => "20 fps",
            Self::Fps30 => "30 fps",
            Self::Fps45 => "45 fps",
            Self::Fps60 => "60 fps",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameSettings;

    #[test]
    fn old_settings_keep_the_existing_presentation_policy() {
        let old: GameSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.frame_rate_limit, FrameRateLimit::Unlimited);
        assert_eq!(old.frame_rate_limit.fps(), None);
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
            settings.frame_rate_limit = value;
            settings.pocketpc_owner_name = "Example owner".into();
            let saved = serde_json::to_string(&settings).unwrap();
            let restored: GameSettings = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.frame_rate_limit, value);
            assert_eq!(restored.pocketpc_owner_name, "Example owner");
        }
        assert!(serde_json::from_str::<GameSettings>(r#"{"frame_rate_limit":"0"}"#).is_err());
    }
}
