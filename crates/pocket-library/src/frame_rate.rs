//! Optional per-game presentation ceiling; independent of video recording FPS.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FrameRateLimit {
    #[default]
    #[serde(rename = "unlimited")]
    Unlimited,
    #[serde(rename = "20")]
    Fps20,
    #[serde(rename = "30")]
    Fps30,
    #[serde(rename = "40")]
    Fps40,
    #[serde(rename = "50")]
    Fps50,
    #[serde(rename = "60")]
    Fps60,
}

impl FrameRateLimit {
    pub const ALL: [Self; 6] = [
        Self::Unlimited,
        Self::Fps20,
        Self::Fps30,
        Self::Fps40,
        Self::Fps50,
        Self::Fps60,
    ];

    pub fn fps(self) -> Option<u32> {
        match self {
            Self::Unlimited => None,
            Self::Fps20 => Some(20),
            Self::Fps30 => Some(30),
            Self::Fps40 => Some(40),
            Self::Fps50 => Some(50),
            Self::Fps60 => Some(60),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unlimited => "Unlimited",
            Self::Fps20 => "20 fps",
            Self::Fps30 => "30 fps",
            Self::Fps40 => "40 fps",
            Self::Fps50 => "50 fps",
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
