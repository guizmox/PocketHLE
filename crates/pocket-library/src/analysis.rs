//! Shared executable metadata for desktop and Android information screens.

use crate::{is_gizmondo_game, GameEntry};

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GameAnalysis {
    pub platform: String,
    pub title_id: Option<String>,
    pub identity: Vec<(String, String)>,
    pub executable: Vec<(String, String)>,
    pub graphics: Vec<(String, String)>,
    pub hardware: Vec<(String, String)>,
    pub game_data: Vec<(String, String)>,
    pub languages: Vec<(String, String)>,
    pub evidence: Vec<String>,
}

pub fn detect_gizmondo_title_id(root: &std::path::Path) -> Option<String> {
    fn scan(dir: &std::path::Path, depth: usize) -> Option<String> {
        if depth > 5 {
            return None;
        }
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            let kind = e.file_type().ok()?;
            if !kind.is_dir() && !kind.is_file() {
                continue;
            }
            if let Some(n) = p.file_name().and_then(|n| n.to_str()) {
                let b = n.as_bytes();
                if b.len() == 10
                    && b[..2].eq_ignore_ascii_case(b"GZ")
                    && b[2..4].iter().all(|c| c.is_ascii_alphabetic())
                    && b[4..].iter().all(|c| c.is_ascii_digit())
                {
                    return Some(n.to_ascii_uppercase());
                }
            }
            if kind.is_dir() {
                if let Some(v) = scan(&p, depth + 1) {
                    return Some(v);
                }
            }
        }
        None
    }
    scan(root, 0)
}

/// Prefer the selected executable's card directory over shared demo-card assets.
/// This is cheap enough for library tiles: never rescan a whole binary per frame.
pub fn game_gizmondo_title_id(game: &GameEntry, root: &std::path::Path) -> Option<String> {
    let extracted = game.extracted_dir(root);
    let executable = game.executable_path(root);
    let mut directory = executable.parent()?;
    while directory.starts_with(&extracted) {
        if let Some(name) = directory.file_name().and_then(|n| n.to_str()) {
            if crate::is_gizmondo_title_id(name) {
                return Some(name.to_ascii_uppercase());
            }
        }
        if directory == extracted {
            break;
        }
        directory = directory.parent()?;
    }
    // Flattened imports store their exact serial as the stable library ID.
    if crate::is_gizmondo_title_id(&game.id) {
        return Some(game.id.to_ascii_uppercase());
    }
    // A split title without its own serial must not inherit the launcher's
    // serial (or another shared card's marker). A local marker is still valid.
    if game.source_cab.starts_with("demo-card:") {
        detect_gizmondo_title_id(executable.parent()?)
    } else {
        detect_gizmondo_title_id(&extracted)
    }
}

fn pe_timestamp(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return None;
    }
    let o = u32::from_le_bytes(bytes[0x3c..0x40].try_into().ok()?) as usize;
    let header = bytes.get(o..o.checked_add(12)?)?;
    if &header[..4] != b"PE\0\0" {
        return None;
    }
    Some(u32::from_le_bytes(header[8..12].try_into().ok()?))
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += if m <= 2 { 1 } else { 0 };
    (y, m as u32, d as u32)
}
fn format_unix_utc(ts: u32) -> String {
    let s = ts as i64;
    let days = s / 86400;
    let rem = s % 86400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn binary_strings(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for &b in bytes {
        if b.is_ascii_graphic() || b == b' ' || b == b'\\' {
            cur.push(b);
        } else {
            if cur.len() >= 4 {
                out.push(String::from_utf8_lossy(&cur).into_owned());
            }
            cur.clear();
        }
    }
    if cur.len() >= 4 {
        out.push(String::from_utf8_lossy(&cur).into_owned());
    }

    // VERSIONINFO and many WinCE resources are UTF-16LE. Scan both alignments;
    // keeping only printable runs makes this useful for ordinary resource text too.
    for alignment in 0..=1 {
        let mut chars = Vec::new();
        let mut i = alignment;
        while i + 1 < bytes.len() {
            let u = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
            if (0x20..=0x7e).contains(&u) || (0xa0..=0x024f).contains(&u) {
                chars.push(char::from_u32(u as u32).unwrap_or('?'));
            } else {
                if chars.len() >= 4 {
                    out.push(chars.iter().collect());
                }
                chars.clear();
            }
            i += 2;
        }
        if chars.len() >= 4 {
            out.push(chars.iter().collect());
        }
    }
    out
}

/// Serial APIs and COM8 alone may belong to a modem or Bluetooth device.
/// Require GPS-specific vocabulary as well, including dynamically built ports.
fn serial_gps_evidence(text: &str) -> Option<&'static str> {
    let gps = [
        "nmea",
        "gpssettings",
        "gps engine",
        "ttgpsserportmap",
        "ttnnmeareceiver",
    ]
    .iter()
    .any(|s| text.contains(s));
    let transport = [
        "com8:",
        "com%d:",
        "setcommstate",
        "getcommstate",
        "setcommtimeouts",
        "waitcommevent",
        "clearcommerror",
    ]
    .iter()
    .any(|s| text.contains(s));
    (gps && transport).then_some("Serial/NMEA GPS (port string or serial API with GPS evidence)")
}

fn version_value(strings: &[String], key: &str) -> Option<String> {
    for (i, s) in strings.iter().enumerate() {
        if s.eq_ignore_ascii_case(key) {
            for v in strings.iter().skip(i + 1).take(5) {
                let v = v.trim_matches(char::from(0)).trim();
                if !v.is_empty()
                    && !v.eq_ignore_ascii_case(key)
                    && !v.starts_with("VarFileInfo")
                    && !v.starts_with("StringFileInfo")
                {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn first_matching_string(strings: &[String], pred: impl Fn(&str) -> bool) -> Option<String> {
    strings
        .iter()
        .find(|s| pred(&s.to_ascii_lowercase()))
        .map(|s| s.trim().to_string())
}

pub fn analyze_game(game: &GameEntry, root: &std::path::Path) -> GameAnalysis {
    let mut a = GameAnalysis::default();
    let giz = is_gizmondo_game(game, root);
    a.platform = if giz {
        "Gizmondo".into()
    } else {
        "Pocket PC / Windows CE".into()
    };
    let path = game.executable_path(root);
    let bytes = std::fs::read(&path).unwrap_or_default();
    let strings = binary_strings(&bytes);
    let mut text = strings.join(" ").to_ascii_lowercase();
    // Resolve the shared CE4.2 ordinal map for Gizmondo; Pocket PC versions
    // can use different tables, so do not infer their features from these ordinals.
    if giz {
        if let Ok(img) = pocket_pe::load_file(&path) {
            let table: serde_json::Value = serde_json::from_str(include_str!(
                "../../pocket-winceapi/data/coredll-ordinals.json"
            ))
            .unwrap_or_default();
            for import in &img.imports {
                if import.dll.eq_ignore_ascii_case("coredll.dll") {
                    if let pocket_pe::ImportBinding::Ordinal(ordinal) = import.binding {
                        if let Some(name) = table["ordinals"][ordinal.to_string()].as_str() {
                            text.push(' ');
                            text.push_str(&name.to_ascii_lowercase());
                            a.evidence
                                .push(format!("Import: COREDLL #{ordinal} = {name} (CE4.2 map)"));
                        }
                    }
                }
            }
        }
    }
    a.title_id = game_gizmondo_title_id(game, root);

    // PE VERSIONINFO. Keep file/product versions separate and avoid using
    // FileDescription/InternalName as the library title: prototype builds often
    // carry stale SDK/template metadata (Battlestations famously contains Keys).
    for (key, label) in [
        ("CompanyName", "Developer / company"),
        ("LegalCopyright", "Copyright"),
    ] {
        if let Some(v) = version_value(&strings, key) {
            a.identity.push((label.into(), v.clone()));
            a.evidence.push(format!("VERSIONINFO {key}: {v}"));
        }
    }
    for (key, label) in [
        ("FileVersion", "File version"),
        ("ProductVersion", "Product version"),
    ] {
        if let Some(v) = version_value(&strings, key) {
            a.identity.push((label.into(), v.clone()));
            a.evidence.push(format!("VERSIONINFO {key}: {v}"));
        }
    }

    a.executable.push((
        "File".into(),
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    ));
    if let Ok(img) = pocket_pe::load_file(&path) {
        a.executable
            .push(("Architecture".into(), img.machine_name().into()));
        a.executable
            .push(("Format".into(), "PE32 / Windows CE".into()));
        let dlls: std::collections::BTreeSet<_> = img
            .imports
            .iter()
            .map(|i| i.dll.to_ascii_lowercase())
            .collect();
        if dlls.iter().any(|d| d.contains("gles")) {
            a.graphics
                .push(("Renderer".into(), "OpenGL ES / EGL".into()));
            a.evidence
                .push("Import: libGLES_CM.dll (OpenGL ES / EGL)".into());
        }
        if dlls.iter().any(|d| d.contains("fmod")) || text.contains("fmodce.dll") {
            a.game_data
                .push(("Audio middleware".into(), "FMOD CE".into()));
            a.evidence.push("Audio: fmodce.dll / FMOD symbols".into());
        }
        a.evidence.push(format!(
            "PE imports: {} entries analysed",
            img.imports.len()
        ));
    }
    if let Some(ts) = pe_timestamp(&bytes).filter(|v| *v > 315532800 && *v < 4102444800) {
        a.executable
            .push(("PE build timestamp".into(), format_unix_utc(ts)));
    }
    if let Some(p) = game.provider.as_ref().filter(|s| !s.trim().is_empty()) {
        a.executable.push(("Provider".into(), p.clone()));
    }
    if giz {
        a.graphics
            .push(("Native display".into(), "320 × 240 • Landscape".into()));
    }
    if text.contains("eglcreatewindowsurface") || text.contains("eglinitialize") {
        if !a.graphics.iter().any(|(k, _)| k == "Renderer") {
            a.graphics
                .push(("Renderer".into(), "OpenGL ES / EGL".into()));
        }
        for api in [
            "eglInitialize",
            "eglCreateWindowSurface",
            "eglSwapBuffers",
            "glDrawElements",
            "glCompressedTexImage2D",
        ] {
            if text.contains(&api.to_ascii_lowercase()) {
                a.evidence.push(format!("Graphics symbol: {api}"));
            }
        }
    }
    // Device names and transport APIs are evidence; scene-camera names are not.
    // Imports referenced by ordinal have no readable name, so absence is not proof.
    for (label, needles) in [
        (
            "Bluetooth",
            &[
                "bth1:",
                "bth.dll",
                "bthutil.dll",
                "bluetoothapis.dll",
                "bthgetsdp",
                "bthenum",
                "rfcomm",
            ][..],
        ),
        (
            "Data / Internet",
            &[
                "wininet.dll",
                "ws2.dll",
                "ws2_32.dll",
                "internetconnect",
                "httpsendrequest",
            ][..],
        ),
        ("GPS", &["gps1:", "gpsopen", "gpsgetposition"][..]),
        (
            "Camera",
            &["cam1:", "cameracapture.dll", "cameracapturestart"][..],
        ),
        (
            "Power management",
            &[
                "getsystempowerstatus",
                "requestpowernotifications",
                "setpowersupply",
                "powermanager/",
            ][..],
        ),
        (
            "SIM / cellular",
            &[
                "siminitialize",
                "simgetdevcaps",
                "simgetrecord",
                "sim.dll",
                "ril.dll",
                "sim1:",
            ][..],
        ),
    ] {
        let evidence: Vec<_> = needles
            .iter()
            .filter(|needle| text.contains(**needle))
            .collect();
        let serial_gps = label == "GPS" && serial_gps_evidence(&text).is_some();
        a.hardware.push((
            label.into(),
            if evidence.is_empty() && !serial_gps {
                "No static evidence (dynamic or ordinal calls may be present)".into()
            } else {
                "✓ Detected".into()
            },
        ));
        for needle in evidence {
            a.evidence.push(format!("{label}: {needle}"));
        }
        if serial_gps {
            a.evidence
                .push(format!("GPS: {}", serial_gps_evidence(&text).unwrap()));
        }
    }
    if text.contains("vib1:") || text.contains("vibrator") || text.contains("nledsetdevice") {
        a.hardware.push(("Vibration".into(), "✓ Detected".into()));
        a.evidence.push(if text.contains("vib1:") {
            "Hardware string: VIB1:".into()
        } else {
            "Hardware string: vibrator".into()
        });
    }
    if text.contains("controlpanel\\backlight") || text.contains("backlightchangeevent") {
        a.hardware.push(("Backlight".into(), "✓ Detected".into()));
        a.evidence.push("Backlight API/resource detected".into());
    }
    if text.contains("lowbatery_msg")
        || text.contains("criticalbatery_msg")
        || text.contains("lowbattery")
        || text.contains("getsystempowerstatus")
        || text.contains("requestpowernotifications")
    {
        a.hardware
            .push(("Battery events".into(), "✓ Detected".into()));
        a.evidence
            .push("Battery notification strings detected".into());
    }
    for label in ["Vibration", "Backlight", "Battery events"] {
        if !a.hardware.iter().any(|(name, _)| name == label) {
            a.hardware.push((
                label.into(),
                "No static evidence (dynamic or ordinal calls may be present)".into(),
            ));
        }
    }
    if giz {
        a.hardware
            .insert(0, ("Controls".into(), "Gizmondo front panel".into()));
    }

    if let Some(save) = first_matching_string(&strings, |s| s.contains("\\flash disk\\mygames")) {
        a.game_data
            .push(("Save support".into(), "✓ Detected".into()));
        a.game_data.push(("Save path".into(), save.clone()));
        a.evidence.push(format!("Save path: {save}"));
    } else if text.contains(".sav") || text.contains("defaultprofile.dat") {
        a.game_data
            .push(("Save support".into(), "✓ Detected".into()));
    }
    if let Some(cfg) =
        first_matching_string(&strings, |s| s.ends_with(".cfg") || s.contains(".cfg "))
    {
        a.game_data
            .push(("Configuration".into(), "✓ Detected".into()));
        a.evidence.push(format!("Config resource: {cfg}"));
    }
    for (needle, label) in [
        ("\\maps\\", "Maps"),
        ("\\textures\\", "Textures"),
        ("\\sound\\", "Audio resources"),
    ] {
        if text.contains(needle) {
            a.game_data.push((label.into(), "✓ Detected".into()));
        }
    }
    for (needles, label) in [
        (["choisissez une langue", "french.tga"], "French"),
        (["wählen sie eine sprache", "german.tga"], "German"),
        (["selezioni una lingua", "italian.tga"], "Italian"),
        (["select a language", "english.tga"], "English"),
        (["spanish.tga", "español"], "Spanish"),
    ] {
        if needles.iter().any(|n| text.contains(n)) {
            a.languages
                .push((label.into(), "✓ Resource detected".into()));
            a.evidence.push(format!("Language resource: {label}"));
        }
    }
    if let Some(id) = &a.title_id {
        a.evidence.push(format!("Gizmondo Title ID: {id}"));
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn information_json_preserves_binary_metadata_and_detection_sections() {
        let root = tempfile::tempdir().unwrap();
        let game: GameEntry = serde_json::from_value(serde_json::json!({
            "id": "metadata", "display_name": "Library title",
            "source_cab": "game.zip", "executable": "extracted/GZGA200016/AUTORUN.EXE"
        }))
        .unwrap();
        let card = game.extracted_dir(root.path()).join("GZGA200016");
        std::fs::create_dir_all(&card).unwrap();
        std::fs::write(card.join("GZGA200016"), []).unwrap();
        std::fs::write(game.executable_path(root.path()),
            b"CompanyName\0Example Studio\0FileVersion\01.2.3\0VIB1:\0eglInitialize\0english.tga\0\\Flash Disk\\MyGames\\save.sav\0").unwrap();
        let analysis = analyze_game(&game, root.path());
        let json = serde_json::to_value(&analysis).unwrap();
        assert_eq!(json["platform"], "Gizmondo");
        assert_eq!(json["title_id"], "GZGA200016");
        assert!(analysis
            .identity
            .contains(&("Developer / company".into(), "Example Studio".into())));
        assert!(analysis
            .identity
            .contains(&("File version".into(), "1.2.3".into())));
        assert!(analysis
            .hardware
            .contains(&("Vibration".into(), "✓ Detected".into())));
        assert!(analysis
            .graphics
            .contains(&("Renderer".into(), "OpenGL ES / EGL".into())));
        assert!(analysis
            .languages
            .contains(&("English".into(), "✓ Resource detected".into())));
        assert_eq!(
            json["executable"][0],
            serde_json::json!(["File", "AUTORUN.EXE"])
        );
        assert!(!analysis.evidence.is_empty());
    }

    #[test]
    fn version_resources_support_both_utf16_alignments() {
        for alignment in [0, 1] {
            let mut bytes = vec![0; alignment];
            for ch in "CompanyName\0Example Studio\0".encode_utf16() {
                bytes.extend_from_slice(&ch.to_le_bytes());
            }
            assert_eq!(
                version_value(&binary_strings(&bytes), "CompanyName").as_deref(),
                Some("Example Studio")
            );
        }
    }

    #[test]
    fn malformed_pe_offsets_are_not_metadata_or_panics() {
        let mut bytes = vec![0; 64];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(pe_timestamp(&bytes), None);
        assert_eq!(pe_timestamp(b"MZ"), None);
    }
}

#[cfg(test)]
mod hardware_detection_tests {
    #[test]
    fn device_strings_detect_features_without_confusing_scene_cameras() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let game: GameEntry = serde_json::from_value(serde_json::json!({
            "id":"fixture", "display_name":"Fixture", "source_cab":"fixture.zip",
            "executable":"extracted/Autorun.exe", "imported_at":0,
        }))
        .unwrap();
        let path = game.executable_path(root.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            b"GPS1:\0CAM1:\0nBTH1:\0WININET.dll\0VIB1:\0PowerManager/ActivityTimer\0",
        )
        .unwrap();
        let a = analyze_game(&game, root.path());
        for label in [
            "GPS",
            "Camera",
            "Bluetooth",
            "Data / Internet",
            "Vibration",
            "Power management",
        ] {
            assert!(a.hardware.contains(&(label.into(), "✓ Detected".into())));
        }
        std::fs::write(&path, b"FREECAMERA\0CHASECAMERA\0").unwrap();
        let a = analyze_game(&game, root.path());
        assert!(!a.hardware.contains(&("Camera".into(), "✓ Detected".into())));
    }
}

#[cfg(test)]
mod selected_card_id_tests {
    #[test]
    fn executable_card_wins_over_shared_demo_marker_and_unknown_ids_stay_unknown() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let mut game: GameEntry = serde_json::from_value(serde_json::json!({
            "id":"split", "display_name":"Game", "source_cab":"demo-card:Demo.zip",
            "executable":"extracted/Wrapper/GZGA200016/AUTORUN.EXE", "imported_at":0,
        }))
        .unwrap();
        let shared = game.extracted_dir(root.path()).join("Wrapper/GZGA200007");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("GZGA200007"), []).unwrap();
        let own = game.executable_path(root.path());
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        std::fs::write(&own, []).unwrap();
        assert_eq!(
            game_gizmondo_title_id(&game, root.path()).as_deref(),
            Some("GZGA200016")
        );
        assert_eq!(
            analyze_game(&game, root.path()).title_id.as_deref(),
            Some("GZGA200016")
        );
        game.executable = "extracted/Wrapper/Agaju/AUTORUN.EXE".into();
        let own = game.executable_path(root.path());
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        std::fs::write(own, b"Misleading catalogue reference: GZGA200015").unwrap();
        assert!(game_gizmondo_title_id(&game, root.path()).is_none());
        assert!(analyze_game(&game, root.path()).title_id.is_none());
    }
}

#[cfg(test)]
mod serial_gps_detection_tests {
    use super::*;
    #[test]
    fn compatibility_update_nmea_gps_requires_transport_and_gps_evidence() {
        for text in [
            "com8: nmea",
            "com%d: ttgpsserportmap",
            "setcommstate software\\tomtom\\gpssettings",
        ] {
            assert!(serial_gps_evidence(text).is_some());
        }
        for text in [
            "com8: modem",
            "setcommstate printer",
            "nmea manual only",
            "bluetooth serial port com8:",
        ] {
            assert!(serial_gps_evidence(text).is_none());
        }
        let bytes: Vec<_> = "COM%d:\0TTNNmeaReceiver\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(
            serial_gps_evidence(&binary_strings(&bytes).join(" ").to_ascii_lowercase()).is_some()
        );
    }
}
