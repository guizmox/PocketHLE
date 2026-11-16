//! Host-side storage for one experimental snapshot per game.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAGIC: &[u8; 8] = b"PHSNAP01";

/// Keep host snapshot data outside of the guest's mounted saves directory.
pub fn path_for(library_root: &Path, game: &crate::GameEntry) -> PathBuf {
    library_root
        .join(game.relative_dir())
        .join("snapshots")
        .join("snapshot-01.bin")
}

/// Writes into the same directory and replaces the slot only after capture
/// succeeds. Returns the UTC timestamp in milliseconds since the Unix epoch.
pub fn save(
    path: &Path,
    capture: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> io::Result<u64> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory)?;
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let timestamp = u64::try_from(elapsed.as_millis()).map_err(io::Error::other)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        writer.write_all(MAGIC)?;
        writer.write_all(&timestamp.to_le_bytes())?;
        capture(&mut writer)?;
        writer.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary
        .into_temp_path()
        .persist(path)
        .map_err(|error| error.error)?;
    Ok(timestamp)
}

/// Returns a reader positioned at the start of the payload. A missing
/// snapshot is a no-op for the caller.
pub fn open(path: &Path) -> io::Result<Option<(u64, BufReader<File>)>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut reader = BufReader::new(file);
    let mut magic = [0; MAGIC.len()];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "unsupported snapshot header",
        ));
    }
    let mut timestamp = [0; 8];
    reader.read_exact(&mut timestamp)?;
    Ok(Some((u64::from_le_bytes(timestamp), reader)))
}

pub fn timestamp(path: &Path) -> io::Result<Option<u64>> {
    Ok(open(path)?.map(|(timestamp, _)| timestamp))
}

pub fn format_timestamp(timestamp_ms: u64) -> String {
    crate::analysis::format_unix_utc(timestamp_ms / 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_slot_is_optional_and_replaced_only_on_success() {
        let directory = tempfile::tempdir().unwrap();
        let game: crate::GameEntry = serde_json::from_value(serde_json::json!({
            "id": "fixture",
            "display_name": "Fixture",
            "executable": "extracted/game.exe",
            "source_cab": "fixture.cab"
        }))
        .unwrap();
        let path = path_for(directory.path(), &game);
        assert_eq!(
            path,
            directory
                .path()
                .join("games/fixture/snapshots/snapshot-01.bin")
        );
        let snapshots = path.parent().unwrap();
        assert!(!snapshots.exists());
        assert!(open(&path).unwrap().is_none());
        assert_eq!(timestamp(&path).unwrap(), None);
        let first_timestamp = save(&path, |writer| writer.write_all(b"first")).unwrap();
        assert_eq!(timestamp(&path).unwrap(), Some(first_timestamp));

        assert!(save(&path, |writer| {
            writer.write_all(b"incomplete")?;
            Err(io::Error::other("capture failed"))
        })
        .is_err());
        let (saved_at, mut reader) = open(&path).unwrap().unwrap();
        let mut payload = Vec::new();
        reader.read_to_end(&mut payload).unwrap();
        drop(reader);
        assert_eq!(saved_at, first_timestamp);
        assert_eq!(payload.as_slice(), b"first");

        let second_timestamp = save(&path, |writer| writer.write_all(b"second")).unwrap();
        let (saved_at, mut reader) = open(&path).unwrap().unwrap();
        payload.clear();
        reader.read_to_end(&mut payload).unwrap();
        assert_eq!(saved_at, second_timestamp);
        assert_eq!(payload.as_slice(), b"second");
        assert_eq!(fs::read_dir(snapshots).unwrap().count(), 1);
    }
}
