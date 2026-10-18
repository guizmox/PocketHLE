//! Static title suggestions from the maintainer's Liste_Jeux.xlsx (Builds/Temp),
//! supplemented with exact GZGA200 identifiers from the supplied MultiBoot launcher.
//! Local 700/800/900 IDs carry user-requested alternative-version labels.
//! Official IDs identify titles; local 700/800/900 IDs distinguish supplied variants.
//! No runtime network lookup.
use std::{collections::BTreeMap, sync::OnceLock};
static TITLES: OnceLock<BTreeMap<String, String>> = OnceLock::new();

/// Local variant IDs use GZGAx00xxx; normal game IDs use GZGA200xxx.
pub fn is_alternative(id: &str) -> bool {
    original_card_id(id).is_some()
}

/// The executable keeps its SDK card ID when the catalogue assigns an ALT ID.
/// Keep the numeric suffix, replacing only the local variant digit with 2.
/// No catalogue entry or mutable display name is required. Official 270/280
/// card IDs use a different scheme and must not be conflated with local ALTs.
pub fn original_card_id(id: &str) -> Option<String> {
    let id = id.to_ascii_uppercase();
    let bytes = id.as_bytes();
    if bytes.len() == 10
        && id.starts_with("GZGA")
        && bytes[4..].iter().all(u8::is_ascii_digit)
        && bytes[4] != b'2'
        && bytes[5..7] == *b"00"
    {
        Some(format!("GZGA2{}", &id[5..]))
    } else {
        None
    }
}

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
        assert_eq!(lookup("GZAP200001"), Some("Gizmondo Navigator (UK)"));
        assert_eq!(
            lookup("gzap250006"),
            Some("Gizmondo Navigator (USA West Coast)")
        );
        assert_eq!(lookup("GZGA200096"), Some("Skystriker"));
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
        assert_eq!(lookup("GZGA900056"), Some("Agaju (E3 demo)"));
        assert_eq!(lookup("GZGA900069"), Some("Catapult (august 2005)"));
        assert_eq!(lookup("GZGA800056"), Some("Agaju (august 2005)"));
        assert_eq!(lookup("GZGA200098"), Some("Alien Hominid"));
        assert_eq!(lookup("GZGA900016"), Some("Chicane (licensed cars)"));
        assert_eq!(lookup("GZGA800016"), Some("Chicane (E3 demo)"));
        assert_eq!(lookup("GZGA900015"), Some("Colors (early demo)"));
        assert_eq!(lookup("GZGA200030"), Some("Motocross 2005"));
        assert_eq!(lookup("GZGA200041"), Some("Furious Phil"));
        assert_eq!(
            lookup("GZGA900013"),
            Some("Richard Burns Rally (april 2005)")
        );
        assert_eq!(lookup("GZGA900049"), Some("Hit & Myth (may 2005)"));
        assert_eq!(lookup("GZGA200007"), None);
        assert_eq!(lookup("GZGA222222"), None);
        assert_eq!(lookup("GZGA22222"), None);
    }
}
