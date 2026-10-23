use pocket_library::Library;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for source in std::env::args().skip(1) {
        let temp = tempfile::tempdir()?;
        let mut library = Library::open(temp.path())?;
        let pending = library.prepare_import(&source)?;
        if pending.cabinet_choices().is_empty() {
            let entries = library.apply_prepared_import(pending, false)?;
            println!("OK {source}: {:?}", entries.iter().map(|e| &e.display_name).collect::<Vec<_>>());
        } else {
            println!("CHOICE {source}: {:?}", pending.cabinet_choices().iter().map(|c| (&c.name, &c.architecture)).collect::<Vec<_>>());
            assert!(library.games().is_empty());
            let indexes: Vec<_> = pending.cabinet_choices().iter().map(|c| c.index).collect();
            drop(pending);
            for index in indexes {
                let mut pending = library.prepare_import(&source)?;
                pending.choose_cabinet(index)?;
                assert!(pending.cabinet_choices().is_empty());
                let entries = library.apply_prepared_import(pending, true)?;
                println!("  SELECT {index}: {:?}", entries.iter().map(|e| &e.display_name).collect::<Vec<_>>());
            }
        }
    }
    Ok(())
}
