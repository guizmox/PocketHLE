//! Extract a CAB through the same installation/name recovery API as import.
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let output = std::path::Path::new(args.get(2).expect("output directory"));
    let (files, header) =
        pocket_cab::extract_for_install(args.get(1).expect("original CAB filename"), output)?;
    if let Some(header) = header {
        eprintln!("install_dir={:?}", header.install_dir);
        for (_, path) in pocket_cab::materialise_install_header_names(output, &files, &header) {
            eprintln!("{}", path.display());
        }
    }
    for (_, path) in pocket_cab::materialise_setup_names(output, &files) {
        eprintln!("{}", path.display());
    }
    Ok(())
}
