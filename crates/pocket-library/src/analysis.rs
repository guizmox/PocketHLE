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
    #[serde(default)]
    pub dependencies: Vec<(String, String)>,
}

/// Best-effort graphics label from the selected executable and its referenced
/// local DLLs. This describes API evidence, not a live renderer.
pub fn detect_graphics_api(path: &std::path::Path) -> Option<&'static str> {
    detect_from_modules(path, read_graphics_module)
}

#[derive(Clone)]
struct ModuleGraphics {
    label: Option<&'static str>,
    dependencies: std::collections::BTreeSet<String>,
}

const GLES_2: &str = "OpenGL ES 2.0 CM";
const GLES_CM: &str = "OpenGL ES 1.1 CM";
const GLES_LEGACY_CM: &str = "OpenGL ES CM";
const GLES_CL: &str = "OpenGL ES 1.0 CL";
const GLES_V1_CL: &str = "OpenGL ES 1.1 CL";
const GLES_GENERIC: &str = "OpenGL ES";

fn gles_driver_label(dll: &str) -> Option<&'static str> {
    match dll {
        "libglesv2.dll" => Some(GLES_2),
        "libgles_cm.dll" => Some(GLES_LEGACY_CM),
        "libglesv1_cm.dll" => Some(GLES_CM),
        "libgles_cl.dll" => Some(GLES_CL),
        "libglesv1_cl.dll" => Some(GLES_V1_CL),
        "gles.dll" | "libgles.dll" => Some(GLES_GENERIC),
        _ => None,
    }
}

const GRAPHICS_PRIORITY: &[&str] = &[
    GLES_2,
    GLES_CM,
    GLES_V1_CL,
    GLES_CL,
    GLES_LEGACY_CM,
    GLES_GENERIC,
    "Direct3D Mobile",
    "DirectDraw",
    "GAPI",
    "GapiDraw",
    "SDL",
    "GDI",
];

fn read_graphics_module(path: &std::path::Path) -> Option<ModuleGraphics> {
    let bytes = std::fs::read(path).ok()?;
    let image = pocket_pe::load_bytes(&bytes).ok()?;
    let mut imports = image.imports;
    // Use the same bundled COREDLL ordinal maps as the runtime, not game names.
    static ARM: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    static MIPS: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let arm = ARM.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../pocket-winceapi/data/coredll-ordinals.json"
        ))
        .expect("bundled ordinals")
    });
    let mips = MIPS.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../pocket-winceapi/data/coredll-mips-ordinals.json"
        ))
        .expect("bundled MIPS ordinals")
    });
    for import in &mut imports {
        if import.dll.eq_ignore_ascii_case("coredll.dll") {
            if let pocket_pe::ImportBinding::Ordinal(ordinal) = import.binding {
                let key = ordinal.to_string();
                let name = if matches!(
                    image.machine,
                    pocket_pe::machine::MIPS_R3000 | pocket_pe::machine::MIPS_R4000
                ) {
                    mips["ordinals"][&key]
                        .as_str()
                        .or_else(|| arm["ordinals"][&key].as_str())
                } else {
                    arm["ordinals"][&key].as_str()
                };
                if let Some(name) = name {
                    import.binding = pocket_pe::ImportBinding::Name(name.into());
                }
            }
        }
    }
    let strings = binary_strings(&bytes);
    let mut dependencies: std::collections::BTreeSet<_> = imports
        .iter()
        .filter_map(|import| dll_basename(&import.dll))
        .collect();
    if imports.iter().any(|import| {
        matches!(
            &import.binding, pocket_pe::ImportBinding::Name(name)
                if matches!(name.to_ascii_lowercase().as_str(),
                    "loadlibraryw" | "loadlibrarya" | "loadlibraryexw" | "loadlibraryexa")
        )
    }) {
        dependencies.extend(strings.iter().filter_map(|name| dll_basename(name)));
    }
    let label = graphics_api_from_evidence(&imports, &strings);
    let label = GRAPHICS_PRIORITY.iter().copied().find(|candidate| {
        Some(*candidate) == label
            || dependencies
                .iter()
                .any(|dll| gles_driver_label(dll) == Some(*candidate))
    });
    Some(ModuleGraphics {
        label,
        dependencies,
    })
}

fn dll_basename(name: &str) -> Option<String> {
    let name = name.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    let stem = name.strip_suffix(".dll")?;
    if stem.is_empty()
        || stem.len() > 128
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b' ' | b'$'))
    {
        return None;
    }
    Some(name)
}

fn detect_from_modules(
    path: &std::path::Path,
    mut inspect: impl FnMut(&std::path::Path) -> Option<ModuleGraphics>,
) -> Option<&'static str> {
    let main = inspect(path)?;
    if main
        .label
        .is_some_and(|label| label != "GDI" && label != GLES_GENERIC)
        || main.dependencies.is_empty()
    {
        return main.label;
    }
    let Some(files) = path.parent().and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return main.label;
    };
    // Cabinets may add $-separated suffixes to installed DLL filenames.
    // Prefer the exact filename and skip ambiguous aliases.
    let files: Vec<_> = files
        .flatten()
        .filter_map(|entry| {
            let name = dll_basename(entry.file_name().to_str()?)?;
            entry
                .file_type()
                .ok()?
                .is_file()
                .then(|| (name, entry.path()))
        })
        .collect();
    let mut labels = std::collections::HashSet::new();
    labels.extend(main.label);
    for dependency in main.dependencies {
        let mut exact = files.iter().filter(|(name, _)| *name == dependency);
        let candidate = if let Some(candidate) = exact.next() {
            exact.next().is_none().then_some(candidate)
        } else {
            let stem = dependency.trim_end_matches(".dll");
            let mut aliases = files.iter().filter(|(name, _)| {
                name.split_once('$')
                    .is_some_and(|(prefix, _)| prefix == stem)
            });
            let first = aliases.next();
            first.filter(|_| aliases.next().is_none())
        };
        if let Some((_, candidate)) = candidate {
            labels.extend(inspect(candidate).and_then(|module| module.label));
        }
    }
    GRAPHICS_PRIORITY
        .iter()
        .copied()
        .find(|label| labels.contains(label))
}

/// Process-local cache for catalogue refreshes. File metadata, not game names,
/// invalidates both known and unknown API results after executable replacement.
#[derive(Default)]
struct GraphicsApiCache<T> {
    entries: std::collections::HashMap<std::path::PathBuf, (u64, std::time::SystemTime, T)>,
}
impl<T: Clone> GraphicsApiCache<T> {
    fn get(&mut self, path: &std::path::Path, detect: impl FnOnce(&std::path::Path) -> T) -> T {
        let stamp = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok().map(|t| (m.len(), t)));
        let Some((len, modified)) = stamp else {
            self.entries.remove(path);
            return detect(path);
        };
        if let Some((old_len, old_time, label)) = self.entries.get(path) {
            if *old_len == len && *old_time == modified {
                return label.clone();
            }
        }
        let label = detect(path);
        self.entries
            .insert(path.to_path_buf(), (len, modified, label.clone()));
        label
    }
}
pub fn detect_graphics_api_cached(path: &std::path::Path) -> Option<&'static str> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<GraphicsApiCache<Option<ModuleGraphics>>>> =
        std::sync::OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    detect_from_modules(path, |module| cache.get(module, read_graphics_module))
}

pub fn game_graphics_api(game: &GameEntry, root: &std::path::Path) -> Option<&'static str> {
    // The original Gizmondo NVIDIA driver reports ES-CM 1.0 via GL_VERSION.
    match detect_graphics_api_cached(&game.launch_path(root))? {
        GLES_LEGACY_CM if crate::is_gizmondo_game(game, root) => Some("OpenGL ES 1.0 CM"),
        label => Some(label),
    }
}

fn graphics_api_from_evidence(
    imports: &[pocket_pe::ImportSymbol],
    strings: &[String],
) -> Option<&'static str> {
    let mut imports = imports.to_vec();
    let dynamic=imports.iter().any(|i| matches!(&i.binding,pocket_pe::ImportBinding::Name(name) if matches!(name.to_ascii_lowercase().as_str(),"getprocaddressw"|"getprocaddressa"|"getprocaddress")));
    if dynamic {
        let strings: Vec<_> = strings.iter().map(|s| s.to_ascii_lowercase()).collect();
        let exact = |name: &str| strings.iter().any(|s| s == name);
        let mut add = |name: &str| {
            imports.push(pocket_pe::ImportSymbol {
                dll: String::new(),
                binding: pocket_pe::ImportBinding::Name(name.into()),
                iat_va: 0,
            })
        };
        if exact("eglinitialize") && exact("eglswapbuffers") {
            add("eglSwapBuffers");
        }
        if strings.iter().any(|s| s.contains("gxbegindraw")) {
            add("GXBeginDraw");
        }
        if exact("directdrawcreate") {
            add("DirectDrawCreate");
        }
        if exact("direct3dmobilecreate") {
            add("Direct3DMobileCreate");
        }
    }
    graphics_api_from_imports(&imports)
}

fn graphics_api_from_imports(imports: &[pocket_pe::ImportSymbol]) -> Option<&'static str> {
    let mut gles = None;
    let mut d3dm = false;
    let mut ddraw = false;
    let mut gapi = false;
    let mut gdi = false;
    let mut sdl = false;
    let mut gapidraw = false;
    for import in imports {
        let dll = import.dll.to_ascii_lowercase();
        if let Some(label) = gles_driver_label(&dll) {
            gles = GRAPHICS_PRIORITY
                .iter()
                .copied()
                .find(|candidate| *candidate == label || Some(*candidate) == gles);
        }
        d3dm |= dll == "d3dm.dll";
        ddraw |= dll == "ddraw.dll";
        if let pocket_pe::ImportBinding::Name(name) = &import.binding {
            let name = name.to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "eglswapbuffers" | "glclear" | "gldrawarrays" | "gldrawelements"
            ) {
                gles.get_or_insert(GLES_GENERIC);
            }
            gapidraw |= name.contains("@cgapidisplay@@")
                && (name.starts_with("?opendisplay@") || name.starts_with("?flip@"));
            sdl |= matches!(
                name.as_str(),
                "sdl_setvideomode" | "sdl_updaterects" | "sdl_createrenderer"
            );
            d3dm |= name == "direct3dmobilecreate";
            ddraw |= name == "directdrawcreate";
            // GXOpenInput/GXGetDefaultKeys alone do not imply GAPI rendering.
            gapi |= ["gxopendisplay", "gxbegindraw", "gxenddraw"]
                .iter()
                .any(|api| name == *api || name.starts_with(&format!("?{api}@@")));
            gdi |= matches!(
                name.as_str(),
                "bitblt"
                    | "stretchblt"
                    | "stretchdibits"
                    | "setdibitstodevice"
                    | "textoutw"
                    | "exttextoutw"
                    | "patblt"
            );
        }
    }
    if gles.is_some() {
        gles
    } else if d3dm {
        Some("Direct3D Mobile")
    } else if ddraw {
        Some("DirectDraw")
    } else if gapi {
        Some("GAPI")
    } else if gapidraw {
        Some("GapiDraw")
    } else if sdl {
        Some("SDL")
    } else if gdi {
        Some("GDI")
    } else {
        None
    }
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

/// Identify the Gizmondo runtime that uses a rotated LCD framebuffer.
pub fn uses_rotated_gizmondo_lcd(image: &pocket_pe::LoadedImage) -> bool {
    use pocket_pe::resources::ResourceKey;
    image
        .resources
        .iter()
        .filter(|entry| entry.ty == ResourceKey::Id(16))
        .any(|entry| {
            let Some(bytes) = image.sections.iter().find_map(|section| {
                let offset = entry.data_rva.checked_sub(section.virtual_address)? as usize;
                section
                    .data
                    .get(offset..offset.checked_add(entry.size as usize)?)
            }) else {
                return false;
            };
            let strings = binary_strings(bytes);
            ["InternalName", "ProductName"].into_iter().all(|key| {
                version_value(&strings, key)
                    .is_some_and(|value| value.eq_ignore_ascii_case("mophun"))
            })
        })
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
            for dll in dlls.iter().filter(|dll| gles_driver_label(dll).is_some()) {
                a.evidence.push(format!("OpenGL ES import: {dll}"));
            }
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
    if let Some(api) = game_graphics_api(game, root).or_else(|| {
        (text.contains("eglcreatewindowsurface") || text.contains("eglinitialize"))
            .then_some("OpenGL ES / EGL")
    }) {
        a.graphics.push(("Renderer".into(), api.into()));
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
    } else {
        a.hardware.clear();
        let sensor = [
            "htcsensorsdk",
            "htcsensoropen",
            "htcsensorgetdataoutput",
            "sen0:",
            "acc1:",
        ];
        let mut detected = false;
        for marker in sensor {
            if text.contains(marker) {
                detected = true;
                a.evidence.push(format!("Accelerometer: {marker}"));
            }
        }
        a.hardware.push((
            "Accelerometer".into(),
            if detected {
                "✓ Detected".into()
            } else {
                "No static evidence (dynamic or ordinal calls may be present)".into()
            },
        ));
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
        let mut game: GameEntry = serde_json::from_value(serde_json::json!({
            "id":"fixture", "display_name":"Fixture", "source_cab":"fixture.zip",
            "executable":"extracted/Autorun.exe", "imported_at":0,
        }))
        .unwrap();
        game.platform = Some(crate::GamePlatform::Gizmondo);
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

        game.platform = Some(crate::GamePlatform::PocketPc);
        let mut sensor = b"HTCSensorSDK.dll\0HTCSensorGetDataOutput\0".to_vec();
        sensor.extend("SEN0:\0ACC1:\0".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&path, sensor).unwrap();
        let a = analyze_game(&game, root.path());
        assert_eq!(a.hardware, vec![("Accelerometer".into(), "✓ Detected".into())]);
        assert!(a.evidence.iter().any(|line| line == "Accelerometer: sen0:"));
        assert!(a.evidence.iter().any(|line| line == "Accelerometer: acc1:"));
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

#[cfg(test)]
mod graphics_api_tests {
    use super::*;
    fn import(dll: &str, name: &str) -> pocket_pe::ImportSymbol {
        pocket_pe::ImportSymbol {
            dll: dll.into(),
            binding: pocket_pe::ImportBinding::Name(name.into()),
            iat_va: 0,
        }
    }
    #[test]
    fn graphics_labels_and_rendering_priority() {
        let gdi = import("COREDLL.dll", "BitBlt");
        let gapi = import("gx.dll", "?GXBeginDraw@@YAPAXXZ");
        assert_eq!(graphics_api_from_imports(&[gdi.clone()]), Some("GDI"));
        assert_eq!(
            graphics_api_from_imports(&[gdi.clone(), gapi.clone()]),
            Some("GAPI")
        );
        assert_eq!(
            graphics_api_from_imports(&[gdi.clone(), import("DDRAW.dll", "DirectDrawCreate")]),
            Some("DirectDraw")
        );
        assert_eq!(
            graphics_api_from_imports(&[import("d3dm.dll", "Direct3DMobileCreate")]),
            Some("Direct3D Mobile")
        );
        let gles = pocket_pe::ImportSymbol {
            dll: "libGLES_CM.dll".into(),
            binding: pocket_pe::ImportBinding::Ordinal(1),
            iat_va: 0,
        };
        assert_eq!(
            graphics_api_from_imports(&[gdi, gapi, gles]),
            Some("OpenGL ES CM")
        );
    }
    #[test]
    fn dynamic_graphics_and_sdl_are_evidence_based() {
        let resolver = import("COREDLL.dll", "GetProcAddressW");
        let symbols = vec!["eglInitialize".into(), "eglSwapBuffers".into()];
        assert_eq!(
            graphics_api_from_evidence(&[resolver.clone()], &symbols),
            Some("OpenGL ES")
        );
        assert_eq!(graphics_api_from_evidence(&[], &symbols), None);
        assert_eq!(
            graphics_api_from_evidence(
                &[resolver],
                &["GAPI: Calling GXBeginDraw() to retrieve framebuffer".into()]
            ),
            Some("GAPI")
        );
        assert_eq!(
            graphics_api_from_imports(&[import("SDL.dll", "SDL_SetVideoMode")]),
            Some("SDL")
        );
        assert_eq!(
            graphics_api_from_imports(&[import("SDL.dll", "SDL_mutexP")]),
            None
        );
        assert_eq!(
            graphics_api_from_imports(&[import(
                "GD101.dll",
                "?OpenDisplay@CGapiDisplay@@QAAJPAUHWND__@@K@Z"
            )]),
            Some("GapiDraw")
        );
        assert_eq!(
            graphics_api_from_imports(&[import("GD101.dll", "?StartTimer@CGapiTimer@@QAAJK@Z")]),
            None
        );
    }

    #[test]
    fn no_renderer_guessed_from_input_or_unknown_ordinals() {
        assert_eq!(
            graphics_api_from_imports(&[import("gx.dll", "?GXOpenInput@@YAHXZ")]),
            None
        );
        assert_eq!(
            graphics_api_from_imports(&[pocket_pe::ImportSymbol {
                dll: "coredll.dll".into(),
                binding: pocket_pe::ImportBinding::Ordinal(1),
                iat_va: 0
            }]),
            None
        );
        assert_eq!(
            detect_graphics_api(std::path::Path::new("/nonexistent/pockethle.exe")),
            None
        );
    }
}

#[cfg(test)]
mod catalogue_cache_tests {
    use super::*;
    #[test]
    fn catalogue_follows_only_the_referenced_local_dll() {
        let temp = tempfile::tempdir().unwrap();
        let main = temp.path().join("game.exe");
        let aliased = temp.path().join("graphics$a$b.dll");
        let unrelated = temp.path().join("other.dll");
        let exact = temp.path().join("graphics.dll");
        for path in [&aliased, &unrelated] {
            std::fs::write(path, b"fixture").unwrap();
        }
        let inspect = |path: &std::path::Path| {
            let label = if path == main {
                None
            } else if path == aliased {
                Some("DirectDraw")
            } else if path == exact {
                Some("GAPI")
            } else {
                panic!("unreferenced DLL was scanned")
            };
            Some(ModuleGraphics {
                label,
                dependencies: (path == main)
                    .then(|| "graphics.dll".into())
                    .into_iter()
                    .collect(),
            })
        };
        assert_eq!(detect_from_modules(&main, inspect), Some("DirectDraw"));
        std::fs::write(&exact, b"fixture").unwrap();
        assert_eq!(detect_from_modules(&main, inspect), Some("GAPI"));
    }

    #[test]
    fn executable_metadata_invalidates_cached_known_and_unknown_results() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("game.exe");
        std::fs::write(&file, b"one").unwrap();
        let mut cache = GraphicsApiCache::default();
        assert_eq!(cache.get(&file, |_| Some("GDI")), Some("GDI"));
        assert_eq!(
            cache.get(&file, |_| panic!("unchanged executable was rescanned")),
            Some("GDI")
        );
        std::fs::write(&file, b"changed").unwrap();
        assert_eq!(cache.get(&file, |_| None), None);
        assert_eq!(
            cache.get(&file, |_| panic!("unknown result must also be cached")),
            None
        );
        std::fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(cache.get(&file, |_| Some("OpenGL ES")), Some("OpenGL ES"));
        std::fs::remove_file(&file).unwrap();
        assert_eq!(cache.get(&file, |_| None), None);
        std::fs::write(&file, b"changed").unwrap();
        assert_eq!(cache.get(&file, |_| Some("GAPI")), Some("GAPI"));
    }
}
