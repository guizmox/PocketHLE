//! Read-only SD-card import. VHD/FAT parsing is independent of executable format.
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn le16(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(b[p..p + 2].try_into().unwrap())
}
fn le32(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn be32(b: &[u8], p: usize) -> u32 {
    u32::from_be_bytes(b[p..p + 4].try_into().unwrap())
}
fn be64(b: &[u8], p: usize) -> u64 {
    u64::from_be_bytes(b[p..p + 8].try_into().unwrap())
}
fn checksum(b: &[u8], at: usize) -> bool {
    !b.iter()
        .enumerate()
        .filter(|(i, _)| !(at..at + 4).contains(i))
        .fold(0u32, |n, (_, x)| n.wrapping_add(u32::from(*x)))
        == be32(b, at)
}

struct Disk {
    file: File,
    size: u64,
}
impl Disk {
    fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < 1024 {
            return Err(bad("Truncated VHD"));
        }
        file.seek(SeekFrom::End(-512))?;
        let mut footer = [0; 512];
        file.read_exact(&mut footer)?;
        if &footer[..8] != b"conectix" || !checksum(&footer, 64) {
            return Err(bad("Invalid VHD footer or checksum"));
        }
        if be32(&footer, 12) != 0x10000 || be32(&footer, 60) != 2 {
            return Err(bad("Only fixed VHD SD-card images are supported (not dynamic/differencing VHD or VHDX)"));
        }
        let size = be64(&footer, 48);
        if size == 0 || size > len - 512 || size % 512 != 0 {
            return Err(bad("VHD virtual disk extends beyond its payload"));
        }
        Ok(Self { file, size })
    }
    fn read(&mut self, pos: u64, bytes: &mut [u8]) -> io::Result<()> {
        if pos
            .checked_add(bytes.len() as u64)
            .is_none_or(|n| n > self.size)
        {
            return Err(bad("SD-card read outside virtual disk"));
        }
        self.file.seek(SeekFrom::Start(pos))?;
        self.file.read_exact(bytes)
    }
}

struct Fat {
    disk: Disk,
    start: u64,
    end: u64,
    cluster_size: usize,
    clusters: u32,
    bits: u8,
    table: Vec<u8>,
    data: u64,
    root: u64,
    root_size: usize,
    root_cluster: u32,
    files: usize,
    bytes: u64,
    directories: HashSet<u32>,
}
impl Fat {
    fn open(mut disk: Disk) -> io::Result<Self> {
        let mut boot = [0; 512];
        disk.read(0, &mut boot)?;
        let valid_bpb = |b: &[u8]| {
            matches!(le16(b, 11), 512 | 1024 | 2048 | 4096)
                && b[13].is_power_of_two()
                && b[13] <= 128
                && le16(b, 14) != 0
                && matches!(b[16], 1 | 2)
                && b[510..] == [0x55, 0xaa]
        };
        let (start, length) = if valid_bpb(&boot) {
            (0, disk.size)
        } else {
            if boot[510..] != [0x55, 0xaa] {
                return Err(bad("Invalid SD-card partition table"));
            }
            let entries: Vec<_> = boot[446..510]
                .chunks_exact(16)
                .filter(|e| matches!(e[4], 1 | 4 | 6 | 0x0b | 0x0c | 0x0e))
                .collect();
            if entries.len() != 1 {
                return Err(bad("Expected one FAT partition on the SD card"));
            }
            (
                u64::from(le32(entries[0], 8)) * 512,
                u64::from(le32(entries[0], 12)) * 512,
            )
        };
        let end = start
            .checked_add(length)
            .filter(|n| *n <= disk.size)
            .ok_or_else(|| bad("Invalid FAT partition extent"))?;
        disk.read(start, &mut boot)?;
        if !valid_bpb(&boot) {
            return Err(bad("Invalid FAT boot sector"));
        }
        let sector = u64::from(le16(&boot, 11));
        let total = if le16(&boot, 19) != 0 {
            u64::from(le16(&boot, 19))
        } else {
            u64::from(le32(&boot, 32))
        };
        let reserved = u64::from(le16(&boot, 14));
        let fat_sectors = if le16(&boot, 22) != 0 {
            u64::from(le16(&boot, 22))
        } else {
            u64::from(le32(&boot, 36))
        };
        let root_size = usize::from(le16(&boot, 17)) * 32;
        let root_sectors = (root_size as u64).div_ceil(sector);
        let overhead = reserved + u64::from(boot[16]) * fat_sectors + root_sectors;
        if total <= overhead
            || total.checked_mul(sector).is_none_or(|n| n > length)
            || fat_sectors == 0
        {
            return Err(bad("Invalid FAT geometry"));
        }
        let clusters = (total - overhead) / u64::from(boot[13]);
        let bits = if clusters < 4085 {
            12
        } else if clusters < 65525 {
            16
        } else {
            32
        };
        if clusters > 0x0ffffff5 || (bits == 32) != (root_size == 0 && le16(&boot, 22) == 0) {
            return Err(bad("Inconsistent FAT type"));
        }
        let table_size = fat_sectors * sector;
        if table_size > 64 * 1024 * 1024 || table_size * 8 / u64::from(bits) < clusters + 2 {
            return Err(bad("Invalid or excessive FAT table"));
        }
        let active = if bits == 32 && le16(&boot, 40) & 0x80 != 0 {
            u64::from(le16(&boot, 40) & 15)
        } else {
            0
        };
        if active >= u64::from(boot[16]) {
            return Err(bad("Invalid active FAT index"));
        }
        let mut table = vec![0; table_size as usize];
        disk.read(
            start + (reserved + active * fat_sectors) * sector,
            &mut table,
        )?;
        Ok(Self {
            disk,
            start,
            end: end.min(start + total * sector),
            cluster_size: (sector * u64::from(boot[13])) as usize,
            clusters: clusters as u32,
            bits,
            table,
            data: start + overhead * sector,
            root: start + (reserved + u64::from(boot[16]) * fat_sectors) * sector,
            root_size,
            root_cluster: if bits == 32 {
                le32(&boot, 44) & 0xfffffff
            } else {
                0
            },
            files: 0,
            bytes: 0,
            directories: HashSet::new(),
        })
    }
    fn chain(&self, first: u32, max_bytes: usize) -> io::Result<Vec<u32>> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut c = first;
        loop {
            if result.len() >= max_bytes / self.cluster_size {
                return Err(bad("Excessive FAT cluster chain"));
            }
            if c < 2 || c >= self.clusters + 2 || !seen.insert(c) {
                return Err(bad("Invalid or cyclic FAT cluster chain"));
            }
            result.push(c);
            let p = match self.bits {
                12 => c as usize + c as usize / 2,
                16 => c as usize * 2,
                _ => c as usize * 4,
            };
            c = match self.bits {
                12 => {
                    let n = le16(&self.table, p);
                    u32::from(if c & 1 != 0 { n >> 4 } else { n & 0xfff })
                }
                16 => u32::from(le16(&self.table, p)),
                _ => le32(&self.table, p) & 0xfffffff,
            };
            let eof = match self.bits {
                12 => 0xff8,
                16 => 0xfff8,
                _ => 0xffffff8,
            };
            if c >= eof {
                return Ok(result);
            }
        }
    }
    fn cluster(&mut self, n: u32, bytes: &mut [u8]) -> io::Result<()> {
        let pos = self.data + u64::from(n - 2) * self.cluster_size as u64;
        if pos < self.start || pos + bytes.len() as u64 > self.end {
            return Err(bad("FAT cluster outside partition"));
        }
        self.disk.read(pos, bytes)
    }
    fn directory(
        &mut self,
        cluster: u32,
        dest: &Path,
        depth: usize,
        written: &mut Vec<PathBuf>,
    ) -> io::Result<()> {
        if depth > 32 {
            return Err(bad("Excessive SD-card directory depth"));
        }
        let mut bytes;
        if cluster == 0 {
            bytes = vec![0; self.root_size];
            self.disk.read(self.root, &mut bytes)?;
        } else {
            let chain = self.chain(cluster, 16 * 1024 * 1024)?;
            if chain.len() * self.cluster_size > 16 * 1024 * 1024 {
                return Err(bad("Excessive FAT directory"));
            }
            bytes = vec![0; chain.len() * self.cluster_size];
            for (c, data) in chain.into_iter().zip(bytes.chunks_mut(self.cluster_size)) {
                if !self.directories.insert(c) {
                    return Err(bad("Cross-linked FAT directory"));
                }
                self.cluster(c, data)?;
            }
        }
        let mut names = HashSet::new();
        let mut long: Option<LongName> = None;
        for e in bytes.chunks_exact(32) {
            if e[0] == 0 {
                break;
            }
            if e[0] == 0xe5 {
                long = None;
                continue;
            }
            if e[11] == 0x0f {
                LongName::push(&mut long, e);
                continue;
            }
            let lfn = long.take().and_then(|n| n.finish(&e[..11]));
            if e[11] & 8 != 0 {
                continue;
            }
            let short = short_name(e);
            if short == "." || short == ".." {
                continue;
            }
            let name = lfn.unwrap_or(short);
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.ends_with(['.', ' '])
                || name
                    .chars()
                    .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
                || !names.insert(name.to_lowercase())
            {
                return Err(bad("Unsafe or duplicate SD-card filename"));
            }
            self.files += 1;
            if self.files > 65536 {
                return Err(bad("Too many SD-card entries"));
            }
            let path = dest.join(name);
            let first = u32::from(le16(e, 26))
                | if self.bits == 32 {
                    u32::from(le16(e, 20)) << 16
                } else {
                    0
                };
            if e[11] & 16 != 0 {
                if first < 2 {
                    return Err(bad("Invalid FAT subdirectory"));
                }
                fs::create_dir(&path)?;
                self.directory(first, &path, depth + 1, written)?;
            } else {
                let size = u64::from(le32(e, 28));
                self.bytes += size;
                if size > 1024 * 1024 * 1024 || self.bytes > 4 * 1024 * 1024 * 1024 {
                    return Err(bad("Excessive extracted SD-card data"));
                }
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                if size != 0 {
                    let chain = self.chain(first, 1024 * 1024 * 1024)?;
                    if chain.len() as u64 * (self.cluster_size as u64) < size {
                        return Err(bad("Truncated FAT file"));
                    }
                    let mut remaining = size as usize;
                    let mut data = vec![0; self.cluster_size];
                    for c in chain {
                        if remaining == 0 {
                            break;
                        }
                        let n = remaining.min(data.len());
                        self.cluster(c, &mut data[..n])?;
                        output.write_all(&data[..n])?;
                        remaining -= n;
                    }
                }
                written.push(path);
            }
        }
        Ok(())
    }
}
fn short_name(e: &[u8]) -> String {
    // DOS/OEM bytes outside ASCII are preserved as Latin-1 when no LFN exists.
    let part = |b: &[u8]| {
        b.iter()
            .copied()
            .map(char::from)
            .collect::<String>()
            .trim_end()
            .to_owned()
    };
    let mut name = part(&e[..8]);
    let ext = part(&e[8..11]);
    if e[12] & 8 != 0 {
        name.make_ascii_lowercase();
    }
    let ext = if e[12] & 16 != 0 {
        ext.to_ascii_lowercase()
    } else {
        ext
    };
    if !ext.is_empty() {
        name.push('.');
        name.push_str(&ext);
    }
    name
}
struct LongName {
    words: Vec<u16>,
    next: u8,
    checksum: u8,
}
impl LongName {
    fn push(state: &mut Option<Self>, e: &[u8]) {
        let order = e[0] & 0x1f;
        if order == 0 || order > 20 || e[0] & 0xa0 != 0 || e[12] != 0 || le16(e, 26) != 0 {
            *state = None;
            return;
        }
        if e[0] & 0x40 != 0 {
            *state = Some(Self {
                words: vec![0xffff; order as usize * 13],
                next: order,
                checksum: e[13],
            });
        }
        let Some(n) = state.as_mut() else { return };
        if n.next != order || n.checksum != e[13] {
            *state = None;
            return;
        }
        for (i, p) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
            .into_iter()
            .enumerate()
        {
            n.words[(order as usize - 1) * 13 + i] = le16(e, p);
        }
        n.next -= 1;
    }
    fn finish(self, short: &[u8]) -> Option<String> {
        if self.next != 0
            || short
                .iter()
                .fold(0u8, |c, b| c.rotate_right(1).wrapping_add(*b))
                != self.checksum
        {
            return None;
        }
        let end = self
            .words
            .iter()
            .position(|v| *v == 0)
            .unwrap_or(self.words.len());
        if end > 255
            || self.words[..end].contains(&0xffff)
            || self.words[end..].iter().any(|v| *v != 0 && *v != 0xffff)
        {
            return None;
        }
        String::from_utf16(&self.words[..end]).ok()
    }
}

pub(super) fn extract(path: &Path, dest: &Path) -> io::Result<Vec<PathBuf>> {
    let mut fat = Fat::open(Disk::open(path)?)?;
    let mut written = Vec::new();
    fat.directory(fat.root_cluster, dest, 0, &mut written)?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0u8; 100 * 512 + 512];
        b[11..13].copy_from_slice(&512u16.to_le_bytes());
        b[13] = 1;
        b[14] = 1;
        b[16] = 1;
        b[17] = 16;
        b[19] = 100;
        b[21] = 0xf8;
        b[22] = 1;
        b[510..512].copy_from_slice(&[0x55, 0xaa]);
        b[512..517].copy_from_slice(&[0xf8, 0xff, 0xff, 0xff, 0x0f]);
        b[1024..1035].copy_from_slice(b"AUTORUN EXE");
        b[1035] = 32;
        b[1050] = 2;
        b[1052] = 64;
        b[1536..1544].copy_from_slice(&[0x39, 0x23, 0xae, 0xb8, 0x4a, 0x0b, 0x53, 0x35]);
        let f = &mut b[100 * 512..];
        f[..8].copy_from_slice(b"conectix");
        f[12..16].copy_from_slice(&0x10000u32.to_be_bytes());
        f[48..56].copy_from_slice(&(100u64 * 512).to_be_bytes());
        f[60..64].copy_from_slice(&2u32.to_be_bytes());
        let sum = !f.iter().fold(0u32, |a, b| a + u32::from(*b));
        f[64..68].copy_from_slice(&sum.to_be_bytes());
        b
    }
    #[test]
    fn fixed_fat_import_preserves_bytes_name_and_gizmondo_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("My Demo Card - Original.VHD");
        let image = fixture();
        fs::write(&path, &image).unwrap();
        let mut library = crate::Library::open(root.path().join("library")).unwrap();
        let entry = library.import_vhd(&path).unwrap().clone();
        assert_eq!(entry.display_name, "My Demo Card - Original");
        assert!(crate::is_gizmondo_game(&entry, library.root()));
        let exe = fs::read(entry.executable_path(library.root())).unwrap();
        assert_eq!(exe, image[1536..1600]);
        assert_eq!(fs::read(path).unwrap(), image);
    }
    #[test]
    fn autorun_has_priority_over_larger_exe_and_absence_uses_fallback() {
        for autorun in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let mut image = fixture();
            // Add a second, larger encrypted EXE at cluster 3. Recognition
            // during import deliberately does not attempt to decode payloads.
            image[516] = 0xff;
            image[517] = 0xff;
            image[1056..1067].copy_from_slice(b"GAME    EXE");
            image[1067] = 32;
            image[1082] = 3;
            image[1084] = 128;
            let header = image[1536..1544].to_vec();
            image[2048..2056].copy_from_slice(&header);
            if !autorun {
                image[1024] = 0xe5;
            }
            let source = root.path().join("Card.vhd");
            fs::write(&source, image).unwrap();
            let mut library = crate::Library::open(root.path().join("library")).unwrap();
            let entry = library.import_vhd(source).unwrap();
            assert_eq!(entry.platform, Some(crate::GamePlatform::Gizmondo));
            assert_eq!(entry.display_name, "Card");
            assert_eq!(
                entry.executable.file_name().unwrap(),
                if autorun { "AUTORUN.EXE" } else { "GAME.EXE" }
            );
        }
    }

    #[test]
    fn malformed_footers_chains_and_names_are_rejected() {
        for fault in 0..4 {
            let root = tempfile::tempdir().unwrap();
            let mut image = fixture();
            match fault {
                0 => image[100 * 512 + 65] ^= 1, // footer checksum
                1 => image[515] = 2,             // cluster 2 points to itself
                2 => image[1024] = b'/',         // unsafe 8.3 name
                _ => {
                    image[1052..1056].copy_from_slice(&1000u32.to_le_bytes());
                }
            }
            let source = root.path().join("bad.vhd");
            fs::write(&source, image).unwrap();
            let dest = root.path().join("out");
            fs::create_dir(&dest).unwrap();
            assert!(extract(&source, &dest).is_err(), "fault {fault}");
        }
    }
    #[test]
    fn long_names_require_checksum_and_complete_order() {
        let short = b"LONGNA~1EXE";
        let checksum = short
            .iter()
            .fold(0u8, |a, b| a.rotate_right(1).wrapping_add(*b));
        let mut e = [0xff; 32];
        e[0] = 0x41;
        e[11] = 15;
        e[12] = 0;
        e[13] = checksum;
        e[26] = 0;
        e[27] = 0;
        let words: Vec<_> = "longname.exe".encode_utf16().chain(Some(0)).collect();
        for (word, offset) in words
            .iter()
            .zip([1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30])
        {
            e[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
        }
        let mut name = None;
        LongName::push(&mut name, &e);
        assert_eq!(
            name.take().unwrap().finish(short).as_deref(),
            Some("longname.exe")
        );
        e[13] ^= 1;
        LongName::push(&mut name, &e);
        assert!(name.take().unwrap().finish(short).is_none());
    }
    #[test]
    fn supplied_sd_card_import_and_load() {
        let Some(source) = std::env::var_os("POCKET_GIZMONDO_VHD") else {
            return;
        };
        let source = PathBuf::from(source);
        let root = tempfile::tempdir().unwrap();
        let mut library = crate::Library::open(root.path()).unwrap();
        let entry = library.import_vhd(&source).unwrap().clone();
        assert_eq!(
            entry.display_name,
            source.file_stem().unwrap().to_string_lossy()
        );
        assert_eq!(
            entry
                .executable
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_lowercase(),
            "autorun.exe"
        );
        let path = entry.executable_path(root.path());
        let before = fs::read(&path).unwrap();
        assert!(pocket_pe::gizmondo::is_encrypted(&before));
        let image = pocket_pe::load_file(&path).unwrap();
        assert!(matches!(
            image.machine,
            pocket_pe::machine::ARM | pocket_pe::machine::THUMB | pocket_pe::machine::ARMNT
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(entry
            .extracted_dir(root.path())
            .join("GZGA200000/ChicaneTeaser.wmv")
            .is_file());
        // The same encrypted file must work without any VHD-specific runtime
        // flag, both as a directly imported EXE and as an ordinary ZIP member.
        let raw = library.import_exe(&path).unwrap().clone();
        assert!(crate::is_gizmondo_game(&raw, root.path()));
        pocket_pe::load_file(raw.executable_path(root.path())).unwrap();
        let zip_path = root.path().join("Encrypted-card.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        zip.start_file("autorun.exe", zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(&before).unwrap();
        zip.finish().unwrap();
        let zipped = library.import_zip(&zip_path).unwrap().clone();
        assert!(crate::is_gizmondo_game(&zipped, root.path()));
        let zip_exe = zipped.executable_path(root.path());
        pocket_pe::load_file(&zip_exe).unwrap();
        assert_eq!(fs::read(zip_exe).unwrap(), before);
        println!(
            "VHD import: {} bytes encrypted; loaded ARM entry 0x{:08x}",
            before.len(),
            image.entry_point
        );
    }
}
