//! Resource finalization for CAB setup DLLs embedding zlib and explicit .cmp names.
//! Do not execute arbitrary installers or reinterpret .cmp files in ZIP imports.
use crate::CabFile;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const FILE_LIMIT: u64 = 128 * 1024 * 1024;
const IMPORT_LIMIT: u64 = 256 * 1024 * 1024;

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

pub(super) fn materialise(files: &[CabFile], names: &[(PathBuf, PathBuf)]) {
    // IonFx setup DLLs (including GeoRally EX) explicitly name each compressed
    // resource and embed zlib. Require both pieces of evidence; .cmp alone says
    // nothing about a file's format or whether its installer decompresses it.
    let installers: Vec<_> = files
        .iter()
        .filter(|f| f.short_name.to_ascii_lowercase().ends_with(".999"))
        .filter_map(|f| fs::read(&f.extracted_path).ok())
        .filter(|b| {
            b.starts_with(b"MZ") && contains(b, b"Install_Exit") && contains(b, b"inflate 1.")
        })
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if installers.is_empty() {
        return;
    }
    let mut candidates: Vec<_> = names
        .iter()
        .map(|(_, p)| p.clone())
        .chain(files.iter().map(|f| f.extracted_path.clone()))
        .collect();
    candidates.sort();
    candidates.dedup();
    let mut budget = IMPORT_LIMIT;
    for source in candidates {
        if !source
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("cmp"))
        {
            continue;
        }
        let destination = source.with_extension("");
        // Assets only. An EXE/DLL compressed by an installer needs a separate,
        // explicitly supported install path, not executable discovery guesses.
        if !destination.extension().is_some_and(|s| {
            matches!(
                s.to_ascii_lowercase().to_str(),
                Some("pak" | "doc" | "res" | "dat" | "trk" | "txt")
            )
        }) {
            continue;
        }
        let Some(name) = source.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = name.to_ascii_lowercase();
        let wide: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        if !installers
            .iter()
            .any(|b| contains(b, name.as_bytes()) || contains(b, &wide))
        {
            continue;
        }
        if destination.exists() || budget == 0 {
            continue;
        }
        match decode_resource(&source, &destination, budget.min(FILE_LIMIT)) {
            Ok(size) => {
                budget -= size;
                log::info!("installed compressed resource {}", destination.display());
            }
            Err(error) => log::warn!("could not finalize {}: {error}", source.display()),
        }
    }
}

fn decode_resource(source: &Path, destination: &Path, limit: u64) -> io::Result<u64> {
    if fs::symlink_metadata(source)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "resource is a symlink",
        ));
    }
    if fs::metadata(source)?.len() > FILE_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "compressed resource too large",
        ));
    }
    let bytes = fs::read(source)?;
    let mut decoder = flate2::Decompress::new(true);
    let mut output = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &bytes[before_in as usize..],
                &mut chunk,
                flate2::FlushDecompress::None,
            )
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let produced = (decoder.total_out() - before_out) as usize;
        if decoder.total_out() > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "resource limit"));
        }
        output.extend_from_slice(&chunk[..produced]);
        if status == flate2::Status::StreamEnd {
            if decoder.total_in() != bytes.len() as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "trailing resource data",
                ));
            }
            break;
        }
        if decoder.total_in() == before_in && produced == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete zlib resource",
            ));
        }
    }
    // Never replace an installed resource, even if materialization runs twice.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    if let Err(error) = file.write_all(&output).and_then(|_| file.flush()) {
        drop(file);
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(output.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "pocket-cab-finalize-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> CabFile {
            let p = self.0.join(name);
            fs::write(&p, bytes).unwrap();
            CabFile {
                short_name: name.into(),
                extracted_path: p,
                size: bytes.len() as u64,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn compressed(bytes: &[u8]) -> Vec<u8> {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(bytes).unwrap();
        z.finish().unwrap()
    }
    #[test]
    fn cab_resources_require_installer_evidence_and_preserve_existing_files() {
        let f = Fixture::new();
        let resource = f.file("world.pak.cmp", &compressed(&vec![42u8; 200_000]));
        materialise(std::slice::from_ref(&resource), &[]);
        assert!(!f.0.join("world.pak").exists());
        let mut installer = b"MZ Install_Exit inflate 1.2.2 ".to_vec();
        installer.extend(
            "%s\\WORLD.PAK.CMP"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        let setup = f.file("setup.999", &installer);
        let files = [setup, resource];
        materialise(&files, &[]);
        assert_eq!(
            fs::read(f.0.join("world.pak")).unwrap(),
            vec![42u8; 200_000]
        );
        fs::write(f.0.join("world.pak"), b"keep installed changes").unwrap();
        materialise(&files, &[]);
        assert_eq!(
            fs::read(f.0.join("world.pak")).unwrap(),
            b"keep installed changes"
        );
        assert!(f.0.join("world.pak.cmp").exists());
    }
    #[test]
    fn corrupt_truncated_oversized_and_trailing_streams_never_publish() {
        let f = Fixture::new();
        let good = compressed(&[42; 1024]);
        let mut corrupt = good.clone();
        let n = corrupt.len();
        corrupt[n - 1] ^= 1;
        let mut trailing = good.clone();
        trailing.push(99);
        for (i, b) in [corrupt, good[..good.len() - 2].to_vec(), trailing]
            .iter()
            .enumerate()
        {
            let src = f.file(&format!("bad{i}.pak.cmp"), b);
            let dst = src.extracted_path.with_extension("");
            assert!(decode_resource(&src.extracted_path, &dst, 4096).is_err());
            assert!(!dst.exists());
        }
        let src = f.file("large.pak.cmp", &good);
        let dst = src.extracted_path.with_extension("");
        assert!(decode_resource(&src.extracted_path, &dst, 100).is_err());
        assert!(!dst.exists());
    }
}
