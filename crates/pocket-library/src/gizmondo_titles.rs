//! Static title suggestions from the maintainer's Liste_Jeux.xlsx (Builds/Temp),
//! supplemented with exact GZGA200 identifiers from the supplied MultiBoot launcher.
//! GZGA900056/900069 are user-requested local alternative-version IDs.
//! IDs identify games, not individual E3/retail builds. No runtime network lookup.
use std::{collections::BTreeMap, sync::OnceLock};
static TITLES: OnceLock<BTreeMap<String, String>> = OnceLock::new();

pub fn lookup(id: &str) -> Option<&'static str> {
    TITLES
        .get_or_init(|| {
            serde_json::from_str(include_str!("../data/gizmondo-titles.json"))
                .expect("validated built-in Gizmondo title table")
        })
        .get(&id.to_ascii_uppercase())
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_ids_and_user_correction_are_exact_and_unknown_ids_are_not_guessed() {
        assert_eq!(lookup("gzga200016"), Some("Chicane"));
        assert_eq!(lookup("GZGA270016"), Some("Chicane"));
        assert_eq!(lookup("GZGA200015"), Some("Colors"));
        assert_eq!(lookup("GZGA270015"), Some("Colors"));
        assert_eq!(lookup("GZGA200011"), Some("Sega Classics"));
        assert_eq!(lookup("GZGA200033"), Some("Conflict Desert Storm II"));
        assert_eq!(lookup("GZGA200054"), Some("Speedgun Stadium"));
        assert_eq!(lookup("GZGA200055"), Some("Supernaturals"));
        assert_eq!(lookup("GZGA200099"), Some("Supernaturals"));
        assert_eq!(lookup("GZGA280045"), Some("Ball Busters"));
        assert_eq!(lookup("GZGA200069"), Some("Catapult"));
        assert_eq!(lookup("GZGA900056"), Some("Agaju (alternative)"));
        assert_eq!(lookup("GZGA900069"), Some("Catapult (alternative)"));
        assert_eq!(lookup("GZGA200030"), Some("Motocross 2005"));
        assert_eq!(lookup("GZGA200041"), Some("Furious Phil"));
        assert_eq!(lookup("GZGA200007"), None);
        assert_eq!(lookup("GZGA222222"), None);
        assert_eq!(lookup("GZGA22222"), None);
    }
}
