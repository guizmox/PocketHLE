//! Optional direct library launch. No settings are written by argument parsing.
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchArgs {
    pub game: String,
    pub fullscreen: Option<bool>,
    pub filter: Option<usize>,
}
pub const HELP: &str = "Usage: pockethle-gui [--steam-deck] [\"Game name or ID\" [0|1 [filter-index]]]\n       pockethle-gui --ce-exe app.exe [CE options]\n\n0 = fullscreen, 1 = windowed. Omitted display mode opens a normal 1280x800 window (capped to the screen).\nFilters: 0 no filter, 1 reconstruction, 2 SMAA, 3 SMAA Soft,\n4 xBRZ, 5 bicubic, 6 Lanczos, 7 bilinear, 8 nearest.\n--steam-deck selects the Proton/Deck profile, separate settings.\n--minimized opens an inactive, minimized window without taking focus.\n\nCE options:\n  --ce-profile NAME|FILE  Launch a saved CE profile or JSON profile file\n  --ce-root folder       Extracted image containing Windows or fs/Windows\n  --ce-data folder       Writable device data (automatic when omitted)\n  --ce-app-prefix path   Guest folder for the EXE and DLLs (default \\Application\\)\n  --ce-args text         Arguments passed to the guest application\n  --ce-screen WxH        640x480 (default), 320x240, 240x320, 480x320, 480x640\n  --ce-no-frames         Disable classic window decorations\n  --fullscreen          Start the CE session in fullscreen\n  --windowed            Start the CE session in a normal window";

#[derive(Debug, PartialEq)]
pub struct StartupArgs {
    pub profile: pocket_library::LauncherProfile,
    pub help: bool,
    pub minimized: bool,
    pub launch: Option<LaunchArgs>,
    pub ce_launch: Option<crate::ce_launch::CeLaunch>,
    pub ce_profile: Option<String>,
    pub ce_fullscreen: Option<bool>,
}

pub fn parse(mut args: Vec<String>) -> Result<StartupArgs, String> {
    use pocket_library::LauncherProfile;
    let mut profile = LauncherProfile::Standard;
    let mut help = false;
    let mut minimized = false;
    let mut ce = crate::ce_launch::CeLaunch::default();
    let mut ce_options = false;
    let mut ce_profile = None;
    let mut ce_fullscreen = None;
    let mut seen = std::collections::HashSet::new();
    while let Some(arg) = args.first() {
        match arg.as_str() {
            "--steam-deck" if profile == LauncherProfile::Standard => {
                profile = LauncherProfile::SteamDeck
            }
            "--help" | "-h" => help = true,
            "--minimized" if !minimized => minimized = true,
            "--ce-profile" => {
                if ce_profile.is_some() { return Err("Duplicate --ce-profile".into()); }
                ce_profile = Some(args.get(1).cloned().ok_or_else(|| format!("--ce-profile requires a profile name or JSON file\n{HELP}"))?);
                args.remove(0);
            }
            "--ce-exe" | "--ce-root" | "--ce-data" | "--ce-app-prefix" | "--ce-args" | "--ce-screen" => {
                let option = arg.clone();
                if !seen.insert(option.clone()) {
                    return Err(format!("Duplicate option: {option}"));
                }
                let value = args.get(1).cloned().ok_or_else(||format!("{option} requires a value\n{HELP}"))?;
                match option.as_str() {
                    "--ce-exe" => ce.executable = value,
                    "--ce-root" => ce.image_root = value,
                    "--ce-data" => ce.data_dir = value,
                    "--ce-app-prefix" => ce.app_prefix = crate::ce_launch::app_prefix(&value).map_err(|e|e.to_string())?,
                    "--ce-args" => ce.command_line = value,
                    "--ce-screen" => {
                        use pocket_library::ScreenPref;
                        ce.screen = match value.as_str() {
                            "640x480" => ScreenPref::VgaLandscape,
                            "320x240" => ScreenPref::Landscape,
                            "240x320" => ScreenPref::Qvga,
                            "480x320" => ScreenPref::Hpc,
                            "480x640" => ScreenPref::Vga,
                            _ => return Err(format!("Unsupported CE screen: {value}\n{HELP}")),
                        };
                    }
                    _ => unreachable!(),
                }
                ce_options = true;
                args.remove(0);
            }
            "--ce-no-frames" => {
                if !ce.classic_windows { return Err("Duplicate --ce-no-frames".into()); }
                ce.classic_windows = false;
                ce_options = true;
            }
            "--fullscreen" | "--windowed" => {
                if ce_fullscreen.is_some() { return Err("Specify one display mode".into()); }
                ce_fullscreen = Some(arg == "--fullscreen");
            }
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
    if !help && ce_profile.is_some() && (ce_options || !args.is_empty()) {
        return Err("Use --ce-profile with --fullscreen/--windowed; edit other CE options in the profile".into());
    }
    let ce_launch = if !help && ce_options {
        if ce.executable.trim().is_empty() || !args.is_empty() {
            return Err(format!("A CE launch requires --ce-exe and cannot include a library game\n{HELP}"));
        }
        Some(ce)
    } else { None };
    if !help && ce_fullscreen.is_some() && ce_launch.is_none() && ce_profile.is_none() {
        return Err(format!("--fullscreen/--windowed require --ce-exe; library games use 0 or 1\n{HELP}"));
    }
    let launch = if help || ce_launch.is_some() || ce_profile.is_some() { None } else { parse_launch(args)? };
    Ok(StartupArgs {
        profile,
        help,
        minimized,
        launch,
        ce_launch,
        ce_profile,
        ce_fullscreen,
    })
}

/// Startup policy is applied after native-window restoration as well as in its builder.
pub fn startup_fullscreen(explicit: Option<bool>) -> bool {
    explicit.unwrap_or(false)
}

/// Cap the physical outer window at 1280x800, or the monitor when smaller.
/// Native viewport geometry is logical: convert the fixed pixel target once,
/// then subtract measured logical decorations to obtain the client size.
pub fn startup_window_size(monitor: [f32; 2], decorations: [f32; 2], pixels_per_point: f32) -> Option<[f32; 2]> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0
        || monitor.iter().chain(decorations.iter()).any(|v| !v.is_finite())
        || monitor.iter().any(|v| *v <= 0.0) { return None; }
    let size = [
        monitor[0].min(1280.0 / pixels_per_point) - decorations[0].max(0.0),
        monitor[1].min(800.0 / pixels_per_point) - decorations[1].max(0.0),
    ];
    size.iter().all(|v| *v > 0.0).then_some(size)
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
        [] => Err(format!("Game not found in the PocketHLE-Reborn library: {query}")),
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
    fn named_ce_profiles_launch_without_an_executable_option() {
        let p = parse(args(&["--ce-profile", "wince6", "--windowed"])).unwrap();
        assert_eq!(p.ce_profile.as_deref(), Some("wince6"));
        assert_eq!(p.ce_fullscreen, Some(false));
        assert!(p.launch.is_none());
        assert!(p.ce_launch.is_none());
        assert!(parse(args(&["--ce-profile"])).is_err());
        assert!(parse(args(&["--ce-profile", "wince6", "Game"])).is_err());
        assert!(parse(args(&["--ce-profile", "wince6", "--ce-exe", "app.exe"])).is_err());
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
    fn normal_screen_sized_window_is_default_but_explicit_cli_mode_wins() {
        assert!(!startup_fullscreen(None));
        assert!(!startup_fullscreen(Some(false)));
        assert!(startup_fullscreen(Some(true)));
    }
    #[test]
    fn fixed_physical_size_is_capped_to_monitor_and_subtracts_borders() {
        assert_eq!(startup_window_size([1280.,800.],[0.,0.],1.),Some([1280.,800.]));
        assert_eq!(startup_window_size([1920.,1080.],[16.,39.],1.),Some([1264.,761.]));
        assert_eq!(startup_window_size([1024.,600.],[8.,24.],1.),Some([1016.,576.]));
        assert_eq!(startup_window_size([1280.,720.],[8.,24.],2.),Some([632.,376.]));
        assert_eq!(startup_window_size([600.,400.],[8.,24.],2.),Some([592.,376.]));
        assert_eq!(startup_window_size([0.,800.],[0.,0.],1.),None);
        assert_eq!(startup_window_size([f32::NAN,800.],[0.,0.],1.),None);
        assert_eq!(startup_window_size([1280.,800.],[0.,0.],0.),None);
    }
}
