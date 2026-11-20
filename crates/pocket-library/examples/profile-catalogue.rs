//! Profile catalogue opening and the metadata consumed by both library views.
use pocket_library::Library;
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("provide a library root")?);
    for round in 0..3 {
        let start = Instant::now();
        let library = Library::open(&root)?;
        let opened = start.elapsed();
        let mut sum = 0usize;
        for game in library.games() {
            let (gizmondo, title_id) = pocket_library::analysis::game_card_identity(game, &root);
            let ((w, h), api) = pocket_library::analysis::game_card_display(game, &root, gizmondo);
            sum += usize::from(gizmondo) + title_id.map_or(0, |s| s.len());
            sum += (w + h) as usize + api.map_or(0, str::len);
        }
        println!(
            "round={round} games={} open_ms={:.3} cards_ms={:.3} checksum={sum}",
            library.games().len(),
            opened.as_secs_f64() * 1000.0,
            (start.elapsed() - opened).as_secs_f64() * 1000.0
        );
    }
    Ok(())
}
