//! Static title suggestions from the maintainer's Liste_Jeux.xlsx (Builds/Temp).
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
        assert_eq!(lookup("GZGA200030"), Some("Motocross 2005"));
        assert_eq!(lookup("GZGA200041"), Some("Furious Phil"));
        assert_eq!(lookup("GZGA200007"), None);
        assert_eq!(lookup("GZGA222222"), None);
    }
}
