//! CoPilot Gizmondo compatibility for `dll32.dll`.
//!
//! The original `CEMgr_GetDataCDExistCode` implementation (ordinal 340,
//! RVA 0x6944) writes a DWORD status through its second parameter and
//! optional narrow/ANSI strings through parameters three and four.
//! CoPilot.exe reads the DWORD after the call at 0x0005fd64.
//!
//! Original status values used by CoPilot.exe:
//! 0 = data accepted, 1 = version incompatible, 2 = data unavailable.
//!
//! Rather than manufacture a successful result, use the mounted WinCE
//! filesystem to check CoPilot's actual data.cfg and DataCD contents.

use pocket_kernel::{vfs::Vfs, DispatchOutcome, KernelError};

use crate::{CallCtx, WinCeDispatcher};

const STATUS_OK: u32 = 0;
const STATUS_BAD_VERSION: u32 = 1;
const STATUS_NO_DATA: u32 = 2;

const CONFIG_FILES: &[&str] = &[
    "\\SD Card\\GZAP200001\\data.cfg",
    "\\SD Card\\data.cfg",
    "\\Storage Card\\GZAP200001\\data.cfg",
    "\\Storage Card\\data.cfg",
];

const DEFAULT_DATA_DIRS: &[&str] = &[
    "\\SD Card\\GZAP200001\\DataCD",
    "\\Storage Card\\GZAP200001\\DataCD",
    "\\SD Card\\DataCD",
    "\\Storage Card\\DataCD",
    "\\My Documents\\DataCD",
];

/// The three pieces of data which CoPilot 5.0.1.7's `data.cfg` requests
/// on startup, plus the real map container. The files are checked as a
/// group to avoid treating an incomplete SD-card import as installed.
const REQUIRED_DATA_FILES: &[&str] =
    &["version.dat", "00000000.hsh", "00000000.qd", "00000000.cgd"];

pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler(
        "dll32.dll",
        "CEMgr_GetDataCDExistCode",
        get_data_cd_exist_code,
    );
    d.register_handler("dll32.dll", "ord:340", get_data_cd_exist_code);
    d.register_handler("dll32.dll", "#340", get_data_cd_exist_code);

    // CoPilot.exe calls this exported function ten times while checking the
    // paths loaded from pid.cfg. Returning the generic unimplemented value
    // (zero) makes it believe that *every* resource is missing.
    d.register_handler("dALKUtil.dll", "FileExists", dalk_file_exists);
    d.register_handler("dALKUtil.dll", "ord:352", dalk_file_exists);
    d.register_handler("dALKUtil.dll", "#352", dalk_file_exists);
    // Original ordinal 46. UK English uses the source phrase as the
    // untranslated fallback; the return remains FALSE in that case.
    for name in ["LANG_GetPhrase", "ord:46", "#46"] {
        d.register_handler("language.dll", name, language_get_phrase);
    }
    for (name, handler) in [
        (
            "Config_GetIntVal",
            config_get_int as fn(&mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError>,
        ),
        (
            "Config_GetStrVal",
            config_get_string as fn(&mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError>,
        ),
    ] {
        d.register_handler("config.dll", name, handler);
    }
    d.register_handler("config.dll", "ord:56", config_get_int);
    d.register_handler("config.dll", "ord:59", config_get_string);
    d.register_handler(
        "dll32.dll",
        "CEMgr_CheckWhereIsSpeech",
        check_speech_location,
    );
    d.register_handler("dll32.dll", "ord:326", check_speech_location);
    d.register_handler("log.dll", "Log_Write", log_write);
    d.register_handler("log.dll", "ord:47", log_write);
}

/// Native config.dll ordinals 56/59 use ANSI "file\\section", key,
/// output pointer (and capacity for strings). Missing entries preserve the
/// caller's default, while a found value is written and returns TRUE.
fn config_value(ctx: &CallCtx<'_>, category: &str, key: &str) -> Option<String> {
    let (file, section) = category.split_once('\\')?;
    let file = format!("{file}.cfg");
    let host = ctx.kernel.find_module_file(&file)?;
    let bytes = std::fs::read(host).ok()?;
    let content = String::from_utf8_lossy(&bytes);
    cfg_value(&content, section, key).map(str::to_owned)
}

fn config_args(ctx: &mut CallCtx<'_>) -> Result<Option<String>, KernelError> {
    let category = ctx.arg_u32(0)?;
    let key = ctx.arg_u32(1)?;
    let category = guest_ansi_path(ctx, category)?;
    let key = guest_ansi_path(ctx, key)?;
    Ok(match (category, key) {
        (Some(category), Some(key)) => config_value(ctx, &category, &key),
        _ => None,
    })
}

fn config_get_int(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(2)?;
    let value = config_args(ctx)?.and_then(|v| v.parse::<i32>().ok());
    if let Some(value) = value.filter(|_| output != 0) {
        ctx.cpu.write_mem(output, &value.to_le_bytes())?;
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn config_get_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(2)?;
    let capacity = ctx.arg_u32(3)?;
    if let Some(value) = config_args(ctx)?.filter(|_| output != 0 && capacity != 0) {
        let mut bytes: Vec<u8> = value.bytes().take(capacity as usize - 1).collect();
        bytes.push(0);
        ctx.cpu.write_mem(output, &bytes)?;
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn check_speech_location(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Ordinal 326 actually has NO arguments (tail branch to RVA 0x6e84).
    // R0 at CoPilot's call site is stale, not an output buffer. This subset
    // locates SD-card speech; 0 is the original card location, 3 unknown.
    let configured = config_value(ctx, "Data\\Directories", "WaveDir");
    let exists = configured
        .and_then(|v| guest_dir(&v))
        .is_some_and(|p| ctx.kernel.vfs.resolve(&p).is_some_and(|host| host.is_dir()));
    Ok(DispatchOutcome::ReturnedR0(if exists { 0 } else { 3 }))
}

fn log_write(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Native ordinal 47 is void Log_Write(const char*, int level), not a
    // printf-style variadic function. Route the actual text to the host log.
    let text = ctx.arg_u32(0)?;
    let level = ctx.arg_u32(1)?;
    if let Some(text) = guest_ansi_path(ctx, text)? {
        log::info!("CoPilot log[{level}]: {text}");
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// Preserve LANG_GetPhrase's bounded English fallback, established from
/// language.dll ordinal 46 (RVA 0x4264) and its copy path at RVA 0x1670.
/// A FALSE translation status still requires writing the source phrase.
/// This does not claim support for selecting/translating other languages.
fn language_get_phrase(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let source = ctx.arg_u32(0)?;
    let destination = ctx.arg_u32(1)?;
    let capacity = ctx.arg_u32(2)? & 0xffff;
    if destination == 0 || capacity == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let mut bytes = Vec::new();
    if source != 0 {
        for offset in 0..capacity - 1 {
            let byte = ctx.cpu.read_mem(source.wrapping_add(offset), 1)?[0];
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }
    }
    bytes.push(0);
    ctx.cpu.write_mem(destination, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// Read a NUL-terminated ANSI path from guest memory, as passed by the
/// original `dALKUtil.dll!FileExists` (ordinal 352, RVA 0x532c).
/// CoPilot.exe builds this string on its stack and passes its address in R0
/// at the call ending at 0x000601f8.
fn guest_ansi_path(ctx: &mut CallCtx<'_>, ptr: u32) -> Result<Option<String>, KernelError> {
    if ptr == 0 {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    // WinCE's MAX_PATH is 260. Allow slightly more for vendor-added prefixes
    // while always bounding guest memory reads.
    for offset in 0..512u32 {
        let byte = ctx.cpu.read_mem(ptr.wrapping_add(offset), 1)?[0];
        if byte == 0 {
            if bytes.is_empty() {
                return Ok(None);
            }
            return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()));
        }
        bytes.push(byte);
    }
    // Do not interpret a non-terminated buffer as a file path.
    Ok(None)
}

fn guest_file_exists(vfs: &Vfs, guest_path: &str) -> bool {
    vfs.resolve(guest_path).is_some_and(|path| path.is_file())
}

/// Original return convention: nonzero for an existing file, zero otherwise.
/// The result is tested with `TST R0, #255` at CoPilot.exe:0x000601f8.
fn dalk_file_exists(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let address = ctx.arg_u32(0)?;
    let path = guest_ansi_path(ctx, address)?;
    let exists = path
        .as_deref()
        .is_some_and(|guest_path| guest_file_exists(&ctx.kernel.vfs, guest_path));
    if std::env::var_os("POCKETHLE_COPILOT_TRACE").is_some() {
        log::info!("CoPilot dALKUtil FileExists: path={path:?} exists={exists}");
    }
    Ok(DispatchOutcome::ReturnedR0(u32::from(exists)))
}

/// Read a legacy CoPilot INI value without interpreting the host's locale.
/// The original data.cfg uses quoted keys, quoted values and CR-CR-LF lines.
fn cfg_value<'a>(content: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut active = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            active = line[1..line.len() - 1].eq_ignore_ascii_case(section);
            continue;
        }
        if !active {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim().trim_matches('"').eq_ignore_ascii_case(key) {
            return Some(value.trim().trim_matches('"'));
        }
    }
    None
}

/// CoPilot's data.cfg uses `SD Card/GZAP200001/datacd` without a leading
/// backslash. This must be resolved as a WinCE guest path, not as a host path.
fn guest_dir(raw: &str) -> Option<String> {
    let dir = raw.trim().replace('/', "\\");
    if dir.is_empty() || dir.split('\\').any(|part| part == "..") {
        return None;
    }
    Some(format!(
        "\\{}",
        dir.trim_start_matches('\\').trim_end_matches('\\')
    ))
}

fn read_guest_file(vfs: &Vfs, guest_path: &str) -> Option<String> {
    let host = vfs.resolve(guest_path)?;
    std::fs::read_to_string(host).ok()
}

fn data_version(contents: &str) -> Option<&str> {
    contents.lines().find_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("Data Version")
            .then_some(value.trim())
    })
}

fn has_data_files(vfs: &Vfs, dir: &str) -> bool {
    REQUIRED_DATA_FILES.iter().all(|file| {
        vfs.resolve(&format!("{dir}\\{file}"))
            .is_some_and(|path| path.is_file())
    })
}

#[derive(Debug)]
struct DataCheck {
    status: u32,
    dir: Option<String>,
    version: Option<String>,
}

#[cfg(test)]
fn find_data(vfs: &Vfs) -> DataCheck {
    find_data_from_config(vfs, None)
}

fn find_data_from_config(vfs: &Vfs, selected_config: Option<&str>) -> DataCheck {
    let mut dirs = Vec::<String>::new();
    let mut expected_version = None::<String>;

    // Navigator USA West uses GZAP250006 rather than the UK directory.
    // Read the selected executable's own config before compatibility aliases.
    let configs = selected_config.map(str::to_owned).into_iter().chain(
        CONFIG_FILES
            .iter()
            .filter_map(|path| read_guest_file(vfs, path)),
    );
    for cfg in configs {
        if let Some(version) = cfg_value(&cfg, "Version", "Major") {
            expected_version = Some(version.to_string());
        }
        if let Some(dir) = cfg_value(&cfg, "Directories", "LRGeoDir").and_then(guest_dir) {
            dirs.push(dir);
        }
        break;
    }
    dirs.extend(DEFAULT_DATA_DIRS.iter().map(|path| (*path).to_string()));

    let mut incompatible = None;
    for dir in dirs {
        // Other possible mounts may name the same data directory.
        let Some(version_txt) = read_guest_file(vfs, &format!("{dir}\\version.dat")) else {
            continue;
        };
        if !has_data_files(vfs, &dir) {
            log::warn!("CoPilot: partial data directory at {dir}; maps are missing");
            continue;
        }
        let Some(version) = data_version(&version_txt) else {
            log::warn!("CoPilot: version.dat has no Data Version at {dir}");
            continue;
        };
        let check = DataCheck {
            status: if expected_version
                .as_deref()
                .is_some_and(|expected| expected != version)
            {
                STATUS_BAD_VERSION
            } else {
                STATUS_OK
            },
            dir: Some(dir),
            version: Some(version.to_string()),
        };
        if check.status == STATUS_OK {
            return check;
        }
        incompatible = Some(check);
    }

    incompatible.unwrap_or(DataCheck {
        status: STATUS_NO_DATA,
        dir: None,
        version: None,
    })
}

/// Write an ANSI string to one of the original DLL's 260-byte buffers.
/// Both buffers are optional; never write beyond the declared capacity.
fn write_ansi(ctx: &mut CallCtx<'_>, address: u32, value: &str) -> Result<(), KernelError> {
    if address == 0 {
        return Ok(());
    }
    let mut bytes: Vec<u8> = value.bytes().take(259).collect();
    bytes.push(0);
    ctx.cpu.write_mem(address, &bytes)?;
    Ok(())
}

fn get_data_cd_exist_code(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let status_out = ctx.arg_u32(1)?;
    let version_out = ctx.arg_u32(2)?;
    let path_out = ctx.arg_u32(3)?;

    let config = ctx
        .kernel
        .find_module_file("data.cfg")
        .and_then(|p| std::fs::read_to_string(p).ok());
    let check = find_data_from_config(&ctx.kernel.vfs, config.as_deref());
    if status_out != 0 {
        ctx.cpu.write_mem(status_out, &check.status.to_le_bytes())?;
    }
    write_ansi(ctx, version_out, check.version.as_deref().unwrap_or(""))?;
    write_ansi(ctx, path_out, check.dir.as_deref().unwrap_or(""))?;

    // INFO is intentional: the ordinary GUI log should record the reason
    // for failure without requiring RUST_LOG=debug.
    log::info!(
        "CoPilot CEMgr_GetDataCDExistCode: status={} data_dir={:?} data_version={:?}",
        check.status,
        check.dir,
        check.version
    );
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_location_uses_card_files_and_never_dereferences_stale_r0() {
        use crate::bluetooth::tests::{call, setup};
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        kernel.module_search_dirs.push(root.path().to_path_buf());
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "dll32.dll",
                "CEMgr_CheckWhereIsSpeech",
                &[0xdead_beef]
            ),
            DispatchOutcome::ReturnedR0(3)
        );
        std::fs::create_dir_all(root.path().join("speech/UK English")).unwrap();
        std::fs::write(
            root.path().join("data.cfg"),
            "[Directories]\n\"WaveDir\"=\"SD Card/speech/UK English\"\n",
        )
        .unwrap();
        kernel.vfs.mount_read_only("\\SD Card\\", root.path());
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "dll32.dll",
                "CEMgr_CheckWhereIsSpeech",
                &[0xdead_beef]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }

    #[test]
    fn config_reads_real_cfg_and_preserves_defaults_for_missing_keys() {
        use crate::bluetooth::tests::{call, setup};
        use pocket_cpu::Cpu;
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("preferences.cfg"), "[InfoBar]\r\r\n\"Line2Index\"=7\r\r\n[autodial]\r\n\"name\"=\"Connection Manager\"\r\n").unwrap();
        kernel.module_search_dirs.push(root.path().to_path_buf());
        cpu.write_mem(0x1100, b"Preferences\\InfoBar\0").unwrap();
        cpu.write_mem(0x1140, b"Line2Index\0").unwrap();
        cpu.write_mem(0x1200, &99u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "config.dll",
                "Config_GetIntVal",
                &[0x1100, 0x1140, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 7);
        cpu.write_mem(0x1140, b"Missing\0").unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "config.dll",
                "Config_GetIntVal",
                &[0x1100, 0x1140, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 7);
        cpu.write_mem(0x1100, b"Preferences\\autodial\0").unwrap();
        cpu.write_mem(0x1140, b"name\0").unwrap();
        cpu.write_mem(0x1200, &[0xaa; 8]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "config.dll",
                "Config_GetStrVal",
                &[0x1100, 0x1140, 0x1200, 4]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_mem(0x1200, 8).unwrap(), b"Con\0\xaa\xaa\xaa\xaa");
    }

    #[test]
    fn exports_are_registered() {
        let d = WinCeDispatcher::new();
        for name in ["CEMgr_GetDataCDExistCode", "ord:340", "#340"] {
            assert!(d
                .registered_iter()
                .any(|(dll, symbol)| dll == "dll32.dll" && symbol == name));
        }
        for name in ["FileExists", "ord:352", "#352"] {
            assert!(d
                .registered_iter()
                .any(|(dll, symbol)| dll == "dalkutil.dll" && symbol == name));
        }
    }

    #[test]
    fn dalk_file_exists_checks_actual_guest_files_and_rejects_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("GZAP200001")).unwrap();
        std::fs::write(root.join("GZAP200001").join("test.bin"), b"data").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_read_only("\\SD Card\\", root);
        assert!(guest_file_exists(&vfs, "\\SD Card\\GZAP200001\\test.bin"));
        assert!(!guest_file_exists(
            &vfs,
            "\\SD Card\\GZAP200001\\missing.bin"
        ));
        assert!(!guest_file_exists(&vfs, "\\SD Card\\GZAP200001"));
    }

    #[test]
    fn parses_original_copilot_5017_config() {
        let original = "[Directories]\r\r\n\"LRGeoDir\"=\"SD Card/GZAP200001/datacd\"\r\r\n[Version]\r\r\n\"Major\"=\"5.0.1.7\"\r\r\n";
        assert_eq!(
            cfg_value(original, "Directories", "LRGeoDir"),
            Some("SD Card/GZAP200001/datacd")
        );
        assert_eq!(cfg_value(original, "Version", "Major"), Some("5.0.1.7"));
        assert_eq!(
            guest_dir("SD Card/GZAP200001/datacd").as_deref(),
            Some("\\SD Card\\GZAP200001\\datacd")
        );
    }

    #[test]
    fn reads_original_datacd_version() {
        assert_eq!(
            data_version("Product Name=CoPilot Pocket PC\r\nData Version=5.0.1.7\r\nData Type=AQ"),
            Some("5.0.1.7")
        );
    }

    #[test]
    fn rejects_missing_maps_and_checks_data_version() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("DataCD")).unwrap();
        std::fs::write(
            root.join("data.cfg"),
            "[Directories]\n\"LRGeoDir\"=\"SD Card/GZAP200001/DataCD\"\n[Version]\n\"Major\"=\"5.0.1.7\"\n",
        )
        .unwrap();
        let data_dir = root.join("DataCD");
        std::fs::write(data_dir.join("version.dat"), "Data Version=5.0.1.7\n").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_read_only("\\SD Card\\GZAP200001\\", root);
        assert_eq!(find_data(&vfs).status, STATUS_NO_DATA);
        for name in &REQUIRED_DATA_FILES[1..] {
            std::fs::write(data_dir.join(name), b"sample").unwrap();
        }
        let ok = find_data(&vfs);
        assert_eq!(ok.status, STATUS_OK);
        assert_eq!(ok.version.as_deref(), Some("5.0.1.7"));
        std::fs::write(data_dir.join("version.dat"), "Data Version=4.0.0.0\n").unwrap();
        assert_eq!(find_data(&vfs).status, STATUS_BAD_VERSION);
    }
}
