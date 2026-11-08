//! Optional direct library launch. No settings are written by argument parsing.
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchArgs {
    pub game: String,
    pub fullscreen: Option<bool>,
    pub filter: Option<usize>,
}
pub const HELP: &str = "Usage: pockethle-gui [--steam-deck] [\"Game name or ID\" [0|1 [filter-index]]]\n\n0 = fullscreen, 1 = windowed. Omitted options keep saved preferences (Deck profile starts fullscreen).\nFilters: 0 no filter, 1 reconstruction, 2 SMAA, 3 SMAA Soft,\n4 xBRZ, 5 bicubic, 6 Lanczos, 7 bilinear, 8 nearest.\n--steam-deck selects the Proton/Deck profile, separate settings and fullscreen by default.";

#[derive(Debug, PartialEq)]
pub struct StartupArgs {
    pub profile: pocket_library::LauncherProfile,
    pub help: bool,
    pub launch: Option<LaunchArgs>,
}

pub fn parse(mut args: Vec<String>) -> Result<StartupArgs, String> {
    use pocket_library::LauncherProfile;
    let mut profile = LauncherProfile::Standard;
    let mut help = false;
    while let Some(arg) = args.first() {
        match arg.as_str() {
            "--steam-deck" if profile == LauncherProfile::Standard => {
                profile = LauncherProfile::SteamDeck
            }
            "--help" | "-h" => help = true,
            "--" => {
                args.remove(0);
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("Unknown or duplicate option: {value}\n{HELP}"))
            }
            _ => break,
        }
        args.remove(0);
    }
    let launch = if help { None } else { parse_launch(args)? };
    Ok(StartupArgs {
        profile,
        help,
        launch,
    })
}

/// Startup policy is applied after native-window restoration as well as in its builder.
pub fn startup_fullscreen(deck: bool, saved: bool, explicit: Option<bool>) -> bool {
    explicit.unwrap_or(deck || saved)
}

fn parse_launch(args: Vec<String>) -> Result<Option<LaunchArgs>, String> {
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
                .filter(|n| *n < 9)
                .ok_or_else(|| format!("Invalid filter index: use 0 to 8.\n{HELP}"))
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
    fn runtime_profile_can_launch_the_library_or_a_game() {
        use pocket_library::LauncherProfile;
        let standard = super::parse(vec![]).unwrap();
        assert_eq!(standard.profile, LauncherProfile::Standard);
        assert!(standard.launch.is_none());
        let deck = super::parse(args(&["--steam-deck"])).unwrap();
        assert_eq!(deck.profile, LauncherProfile::SteamDeck);
        assert!(deck.launch.is_none());
        let game = super::parse(args(&["--steam-deck", "Game", "1", "7"])).unwrap();
        assert_eq!(game.launch.unwrap().fullscreen, Some(false));
        assert!(super::parse(args(&["--steam-deck", "--steam-deck"])).is_err());
        assert!(super::parse(args(&["--unknown"])).is_err());
        assert!(
            super::parse(args(&["--steam-deck", "--help"]))
                .unwrap()
                .help
        );
    }
    #[test]
    fn no_args_preserves_launcher() {
        assert_eq!(parse_launch(vec![]).unwrap(), None);
    }
    #[test]
    fn display_modes_and_optional_filter() {
        let p = parse_launch(args(&["Sim City", "0", "7"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            (p.game.as_str(), p.fullscreen, p.filter),
            ("Sim City", Some(true), Some(7))
        );
        assert_eq!(
            parse_launch(args(&["Game", "1"]))
                .unwrap()
                .unwrap()
                .fullscreen,
            Some(false)
        );
        assert_eq!(
            parse_launch(args(&["Game", "0", "8"]))
                .unwrap()
                .unwrap()
                .filter,
            Some(8)
        );
        let p = parse_launch(args(&["Game"])).unwrap().unwrap();
        assert_eq!((p.fullscreen, p.filter), (None, None));
    }
    #[test]
    fn rejects_invalid_arguments() {
        for a in [
            &["Game", "2"][..],
            &["Game", "0", "9"],
            &["Game", "0", "-1"],
            &["Game", "0", "x"],
            &["Game", "0", "0", "extra"],
            &[""],
        ] {
            assert!(parse_launch(args(a)).is_err());
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

#[cfg(test)]
mod startup_tests {
    use super::*;
    #[test]
    fn deck_starts_fullscreen_with_an_old_windowed_preference_but_cli_wins() {
        for saved in [false, true] {
            assert!(startup_fullscreen(true, saved, None));
            assert!(!startup_fullscreen(true, saved, Some(false)));
            assert!(startup_fullscreen(true, saved, Some(true)));
            assert_eq!(startup_fullscreen(false, saved, None), saved);
        }
    }
}
