use super::*;
use std::time::{Duration, Instant};

/// Supply the user's original CAB locally; the game is not a repository fixture.
#[test]
#[ignore = "requires POCKETHLE_DIAMOND_BEER_CAB"]
fn starts_diamond_beer_from_cab_and_presents_game_frames() {
    let cab = PathBuf::from(std::env::var_os("POCKETHLE_DIAMOND_BEER_CAB").unwrap());
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let prepared = library.prepare_import(&cab).unwrap();
    assert!(prepared.cabinet_choices().is_empty());
    let games = library.apply_prepared_import(prepared, false).unwrap();
    assert_eq!(games.len(), 1);
    assert!(games[0].executable.ends_with("DiamondBeer.exe"));
    let image = pocket_core::pe::load_file(games[0].launch_path(root.path())).unwrap();
    let size = games[0].settings.screen.resolve_image(Some(&image));
    let session = start_with_presentation(root.path().to_path_buf(), games[0].id.clone(), None)
        .expect("create normal frontend session");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut frames = 0;
    let mut distinct = false;
    let mut previous = Vec::new();
    let mut moved = false;
    while frames < 12 && Instant::now() < deadline && session.is_running() {
        if let Some(frame) = session.poll_frame() {
            assert_eq!((frame.width, frame.height), size);
            assert!(frame.rgba.iter().any(|byte| *byte != 0));
            distinct |= !previous.is_empty() && previous != frame.rgba;
            previous = frame.rgba;
            frames += 1;
            if frames == 3 {
                session.send_input(InputCommand::Input(InputEvent::PointerDown { x: 3, y: 3 }));
                session.send_input(InputCommand::Input(InputEvent::PointerUp { x: 3, y: 3 }));
                moved = true;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let summary = session.finish();
    assert_eq!(frames, 12, "{summary}");
    assert!(moved);
    assert!(distinct, "the managed scene did not animate: {summary}");
    assert!(
        summary.contains("Compact Framework session ended."),
        "{summary}"
    );
}
