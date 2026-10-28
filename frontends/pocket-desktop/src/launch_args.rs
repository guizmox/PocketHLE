//! Optional direct library launch. No settings are written by argument parsing.
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchArgs {
    pub game: String,
    pub fullscreen: Option<bool>,
    pub filter: Option<usize>,
}
pub const HELP: &str = "Usage: pockethle-gui [\"Game name or ID\" [0|1 [filter-index]]]\n\n0 = fullscreen, 1 = windowed. Omitted options keep saved preferences.\nFilters: 0 reconstruction, 1 SMAA + reconstruction, 2 SMAA + bilinear,\n3 xBRZ, 4 bicubic, 5 Lanczos, 6 bilinear, 7 nearest.\nThe Steam Deck executable accepts the same arguments.";

pub fn parse(args: Vec<String>) -> Result<Option<LaunchArgs>, String> {
    if args.is_empty() {
        return Ok(None);
    }
    if args.len() > 3 || args[0].trim().is_empty() {
        return Err(HELP.into());
    }
    let fullscreen = match args.get(1).map(String::as_str) {
        None => None,
        Some("0") => Some(true),
        Some("1") => Some(false),
        _ => {
            return Err(format!(
                "Invalid display mode: use 0 (fullscreen) or 1 (windowed).\n{HELP}"
            ))
        }
    };
    let filter = args
        .get(2)
        .map(|s| {
            s.parse::<usize>()
                .ok()
                .filter(|n| *n < 8)
                .ok_or_else(|| format!("Invalid filter index: use 0 to 7.\n{HELP}"))
        })
        .transpose()?;
    Ok(Some(LaunchArgs {
        game: args[0].clone(),
        fullscreen,
        filter,
    }))
}

/// Prefer an exact stable ID; otherwise require a unique case-insensitive name.
pub fn find_game<'a>(
    query: &str,
    games: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<String, String> {
    let games: Vec<_> = games.into_iter().collect();
    if let Some((id, _)) = games.iter().find(|(id, _)| *id == query) {
        return Ok((*id).into());
    }
    let matches: Vec<_> = games
        .iter()
        .filter(|(_, name)| name.to_lowercase() == query.to_lowercase())
        .collect();
    match matches.as_slice() {
        [(id, _)] => Ok((*id).into()),
        [] => Err(format!("Game not found in the PocketHLE library: {query}")),
        _ => Err(format!(
            "Several games are named {query}. Use an ID: {}",
            matches
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn no_args_preserves_launcher() {
        assert_eq!(parse(vec![]).unwrap(), None);
    }
    #[test]
    fn display_modes_and_optional_filter() {
        let p = parse(args(&["Sim City", "0", "7"])).unwrap().unwrap();
        assert_eq!(
            (p.game.as_str(), p.fullscreen, p.filter),
            ("Sim City", Some(true), Some(7))
        );
        assert_eq!(
            parse(args(&["Game", "1"])).unwrap().unwrap().fullscreen,
            Some(false)
        );
        let p = parse(args(&["Game"])).unwrap().unwrap();
        assert_eq!((p.fullscreen, p.filter), (None, None));
    }
    #[test]
    fn rejects_invalid_arguments() {
        for a in [
            &["Game", "2"][..],
            &["Game", "0", "8"],
            &["Game", "0", "-1"],
            &["Game", "0", "x"],
            &["Game", "0", "0", "extra"],
            &[""],
        ] {
            assert!(parse(args(a)).is_err());
        }
    }
    #[test]
    fn matches_id_or_unique_name_and_rejects_ambiguity() {
        let games = [("one", "Game"), ("two", "Game"), ("three", "Other")];
        assert_eq!(find_game("one", games).unwrap(), "one");
        assert_eq!(find_game("OTHER", games).unwrap(), "three");
        assert!(find_game("Game", games).unwrap_err().contains("one, two"));
        assert!(find_game("Missing", games).is_err());
    }
}
