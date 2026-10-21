//! Game library and persistent configuration shared by all GUI
//! frontends (the egui desktop launcher and the Android launcher).
//!
//! The library lives on disk under a single root directory:
//!
//! ```text
//! <library_root>/
//!     library.json     # registry of installed games (this crate)
//!     config.json      # global launcher settings (this crate)
//!     games/
//!         <id>/
//!             game.json    # per-game manifest (this crate)
//!             extracted/   # files extracted from the imported .CAB
//! ```
//!
//! `<library_root>` is platform-specific:
//!
//! * Linux/Windows: a path supplied by the desktop frontend, typically
//!   the user's `Documents/PocketHLE` folder.
//! * Android: `Context.getExternalFilesDir(null)` or any other path the
//!   Java side hands across JNI.
//!
//! The crate is designed to be `Send + Sync` so the Android side can
//! call into it from any thread without locking.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub mod colors_save;

use rars::ArchiveReader;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod analysis;
pub use analysis::{analyze_game, detect_gizmondo_title_id, game_gizmondo_title_id, GameAnalysis};

pub mod gizmondo_titles;
pub mod keybindings;
pub mod save_data;
pub mod stylus;
pub mod updates;

pub use keybindings::{GuestButton, KeyBinding, KeyBindings};

/// Errors returned by [`Library`] operations.
#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("cab error: {0}")]
    Cab(#[from] pocket_cab::CabError),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("rar error: {0}")]
    Rar(String),
    #[error("pe error: {0}")]
    Pe(String),
    #[error("game with id `{0}` not found")]
    NotFound(String),
    #[error("the cabinet does not contain any ARM PE32 executable")]
    NoExecutable,
    #[error("game name must not be empty")]
    InvalidName,
    #[error("invalid game id `{0}`")]
    InvalidId(String),
    #[error("file `{0}` is not an ARM PE32 executable")]
    NotArmPe(String),
    #[error("Windows installer `{0}` contains no Pocket PC CAB")]
    NoPocketPcCab(String),
    #[error(
        "`{0}` is a support library, not a game — import the .exe instead \
         and any DLLs next to it are picked up automatically"
    )]
    IsLibrary(String),
}

fn default_schema_version() -> u32 {
    1
}

/// User classification for a ZIP whose platform cannot be identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamePlatform {
    Gizmondo,
    PocketPc,
}

/// One installed game entry. Stored both inline in `library.json`
/// (for fast listing) and as a separate `game.json` inside the game
/// directory (for per-game settings and so the directory is
/// self-describing if a user copies it around).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<GamePlatform>,
    /// Stable identifier, derived from the source cab name. Used as
    /// the directory name. Always a-z 0-9 _ - .
    pub id: String,
    /// Human-readable name shown in the launcher.
    pub display_name: String,
    /// Optional one-line subtitle (provider / publisher).
    #[serde(default)]
    pub provider: Option<String>,
    /// Path to the main ARM `.exe` inside the extracted directory.
    /// Stored relative to `<library_root>/games/<id>/`.
    pub executable: PathBuf,
    /// Source cab basename, kept for display purposes.
    pub source_cab: String,
    /// WinCE installation directory recorded by the CAB, if available.
    #[serde(default)]
    pub install_dir: Option<String>,
    /// Every guest directory the CAB installs files into.
    #[serde(default)]
    pub install_dirs: Vec<String>,
    /// WinCE directory used by the game for save data, if the CAB records one.
    #[serde(default)]
    pub save_prefix: Option<String>,
    /// Registry values installed by the CAB before the first launch.
    #[serde(default)]
    pub registry: Vec<pocket_cab::SetupRegistryValue>,
    /// Best-effort UNIX timestamp of when the game was imported.
    #[serde(default)]
    pub imported_at: i64,
    /// Per-game runtime settings.
    #[serde(default)]
    pub settings: GameSettings,
    /// Launcher icon extracted from the executable's `RT_GROUP_ICON`
    /// resource, stored as a PNG relative to
    /// `<library_root>/games/<id>/`. `None` when the binary ships no
    /// icon, in which case frontends fall back to a placeholder.
    #[serde(default)]
    pub icon: Option<PathBuf>,
    /// Support libraries that were copied in alongside the executable,
    /// relative to `<library_root>/games/<id>/`.
    ///
    /// Purely informational — the emulator finds these by name at
    /// `LoadLibraryW` time because they sit in the same directory as
    /// the executable. Recorded so the launcher can tell the user that
    /// importing one `.exe` also brought in its satellite DLLs, and so
    /// a game directory stays self-describing.
    #[serde(default)]
    pub companions: Vec<PathBuf>,
}

/// Whether this entry is a Gizmondo card image.
///
/// ZIP repacks do not have a CAB install manifest, so the launcher detects
/// the card by an exact `GZxx######` directory, including empty serial
/// directories in repacks. The legacy Alien Hominid repack is retained as a narrow fallback.
pub fn is_gizmondo_game(entry: &GameEntry, library_root: &Path) -> bool {
    if let Some(platform) = entry.platform {
        return platform == GamePlatform::Gizmondo;
    }
    let extracted = entry.extracted_dir(library_root);
    entry.source_cab.starts_with("demo-card:")
        || entry.source_cab.starts_with("gzga-collection:")
        || is_gizmondo_title_id(&entry.id)
        || has_gizmondo_marker(&extracted)
        || (entry
            .source_cab
            .to_ascii_lowercase()
            .contains("alien-hominid")
            && entry.executable.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case("Alien Hominid.exe")
            }))
}

fn has_gizmondo_marker(root: &Path) -> bool {
    fn scan(dir: &Path, depth: usize) -> bool {
        // Some Gizmondo CAB/ZIP repacks add one or more wrapper directories
        // below `extracted`. Keep the marker test strict, but search a few
        // levels down so layouts such as
        // extracted\Battlestations_12092005\GZGA200038\GZGA200038
        // are recognised as Gizmondo titles.
        if depth > 4 {
            return false;
        }

        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() && !file_type.is_file() {
                continue;
            }

            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            if is_gizmondo_title_id(name) {
                return true;
            }

            if file_type.is_dir() && scan(&path, depth + 1) {
                return true;
            }
        }

        false
    }

    scan(root, 0)
}

fn is_gizmondo_title_id(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 10
        && bytes[..2].eq_ignore_ascii_case(b"GZ")
        && bytes[2..4].iter().all(|b| b.is_ascii_alphabetic())
        && bytes[4..].iter().all(|b| b.is_ascii_digit())
}

impl GameEntry {
    /// Device geometry used at launch and in library captions.
    pub fn resolved_screen_size(&self, root: &Path) -> (u32, u32) {
        if is_gizmondo_game(self, root) {
            return (320, 240);
        }
        let image = (self.settings.screen == ScreenPref::Portrait)
            .then(|| pocket_pe::load_file(self.launch_path(root)).ok())
            .flatten();
        self.settings.screen.resolve_image(image.as_ref())
    }

    /// Path to this game's directory, relative to the library root.
    pub fn relative_dir(&self) -> PathBuf {
        PathBuf::from("games").join(&self.id)
    }

    /// Absolute path of the extracted launcher icon, if the executable
    /// carried one.
    pub fn icon_path(&self, library_root: &Path) -> Option<PathBuf> {
        let icon = self.icon.as_ref()?;
        Some(library_root.join(self.relative_dir()).join(icon))
    }

    /// Absolute path to the directory holding the extracted cab.
    pub fn extracted_dir(&self, library_root: &Path) -> PathBuf {
        library_root.join(self.relative_dir()).join("extracted")
    }

    /// Serial aliases serve flattened games, but must not hide a real card
    /// directory (Supernaturals keeps its marker in GZGA200099/GZGA200099).
    pub fn install_mount_dir(&self, library_root: &Path, prefix: &str) -> PathBuf {
        let extracted = self.extracted_dir(library_root);
        let parts: Vec<_> = prefix
            .split(['\\', '/'])
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() == 2
            && (parts[0].eq_ignore_ascii_case("SD Card")
                || parts[0].eq_ignore_ascii_case("Storage Card"))
            && is_gizmondo_title_id(parts[1])
        {
            if let Ok(entries) = fs::read_dir(&extracted) {
                if let Some(directory) = entries.flatten().find(|entry| {
                    entry.file_type().is_ok_and(|kind| kind.is_dir())
                        && entry
                            .file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case(parts[1])
                }) {
                    return directory.path();
                }
            }
        }
        extracted
    }

    /// Include the original SDK card paths as well as catalogue variant IDs.
    /// Colors E3 is filed under GZGA800015 but still opens GZGA200015 paths.
    pub fn install_mounts(&self, library_root: &Path) -> Vec<(String, PathBuf)> {
        let mut mounts: Vec<_> = self
            .install_dirs
            .iter()
            .map(|prefix| (prefix.clone(), self.install_mount_dir(library_root, prefix)))
            .collect();
        let extracted = self.extracted_dir(library_root);
        let executable = self.executable_path(library_root);
        let mut roots = vec![extracted.clone()];
        if let Some(parent) = executable.parent().filter(|parent| *parent != extracted) {
            roots.push(parent.to_path_buf());
        }
        for root in roots {
            if let Ok(entries) = fs::read_dir(&root) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
                    if !is_gizmondo_title_id(&name) {
                        continue;
                    }
                    let Ok(kind) = entry.file_type() else {
                        continue;
                    };
                    if !kind.is_file() && !kind.is_dir() {
                        continue;
                    }
                    let target = if kind.is_dir() {
                        entry.path()
                    } else {
                        root.clone()
                    };
                    for card in ["SD Card", "Storage Card"] {
                        let prefix = format!("\\{card}\\{name}\\");
                        if let Some((_, directory)) = mounts
                            .iter_mut()
                            .find(|(existing, _)| existing.eq_ignore_ascii_case(&prefix))
                        {
                            // A declared flattened-root fallback must not hide
                            // an actual marker beside a nested executable.
                            if *directory == extracted && target != extracted {
                                *directory = target.clone();
                            }
                        } else {
                            mounts.push((prefix, target.clone()));
                        }
                    }
                }
            }
        }
        // A repack can rename the marker directory to the ALT ID while its
        // executable still hardcodes the original card path. Hit & Myth's
        // GZGA900049/GZGA200049 marker is one such layout: marker discovery at
        // the extraction root alone cannot supply GZGA200049/data.
        let mut ids = vec![self.id.as_str()];
        for prefix in &self.install_dirs {
            let parts: Vec<_> = prefix
                .split(['\\', '/'])
                .filter(|part| !part.is_empty())
                .collect();
            if parts.len() == 2
                && (parts[0].eq_ignore_ascii_case("SD Card")
                    || parts[0].eq_ignore_ascii_case("Storage Card"))
            {
                ids.push(parts[1]);
            }
        }
        for id in ids {
            let Some(original) = gizmondo_titles::original_card_id(id) else {
                continue;
            };
            for card in ["SD Card", "Storage Card"] {
                let prefix = format!("\\{card}\\{original}\\");
                if !mounts
                    .iter()
                    .any(|(existing, _)| existing.eq_ignore_ascii_case(&prefix))
                {
                    let target = self.install_mount_dir(library_root, &prefix);
                    mounts.push((prefix, target));
                }
            }
        }
        mounts
    }

    /// A wrapped MultiBoot ZIP keeps its launcher and GZGA directories inside
    /// one wrapper. Mount that directory as the card, not the extraction root:
    /// rooted multiboot.hog reads must not rely on cross-directory fallback.
    pub fn gizmondo_card_root(&self, library_root: &Path) -> PathBuf {
        let extracted = self.extracted_dir(library_root);
        if self.source_cab.starts_with("demo-card:") {
            return extracted;
        }
        let executable = self.executable_path(library_root);
        let Some(parent) = executable.parent() else {
            return extracted;
        };
        if parent.starts_with(&extracted)
            && executable
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("autorun.exe"))
            && fs::read_dir(parent).is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case("multiboot.hog")
                        && entry.file_type().is_ok_and(|kind| kind.is_file())
                })
            })
        {
            parent.to_path_buf()
        } else {
            extracted
        }
    }

    /// Preserve the executable's actual directory on a Gizmondo card.
    /// A flattened import can still use its declared install alias.
    pub fn gizmondo_guest_exe_path(&self, library_root: &Path) -> Option<String> {
        let executable = self.launch_path(library_root);
        let card = self.gizmondo_card_root(library_root);
        let relative = executable.strip_prefix(&card).ok()?;
        let parts: Vec<_> = relative
            .components()
            .map(|c| c.as_os_str().to_str())
            .collect::<Option<_>>()?;
        if parts.is_empty() || parts.iter().any(|p| *p == "..") {
            return None;
        }
        if parts.len() == 1 {
            if let Some(path) = self.guest_exe_path() {
                return Some(path);
            }
        }
        Some(format!("\\SD Card\\{}", parts.join("\\")))
    }

    pub fn save_dir(&self, library_root: &Path) -> PathBuf {
        save_data::save_dir_for(library_root, &self.id)
    }

    /// Alternative builds retain the original SDK's Flash Disk paths, but
    /// must not load another version's config/save format (Colors E3 crashes
    /// parsing the standard build's config.cfg before its main menu).
    pub fn gizmondo_flash_dir(&self, library_root: &Path) -> PathBuf {
        let alternative = gizmondo_titles::is_alternative(&self.id)
            || self.install_dirs.iter().any(|prefix| {
                let parts: Vec<_> = prefix
                    .split(['\\', '/'])
                    .filter(|part| !part.is_empty())
                    .collect();
                parts.len() == 2
                    && (parts[0].eq_ignore_ascii_case("SD Card")
                        || parts[0].eq_ignore_ascii_case("Storage Card"))
                    && gizmondo_titles::is_alternative(parts[1])
            });
        if alternative {
            self.save_dir(library_root).join("flash")
        } else {
            library_root.join("flash")
        }
    }

    /// Absolute path to the main executable.
    pub fn executable_path(&self, library_root: &Path) -> PathBuf {
        library_root
            .join(self.relative_dir())
            .join(&self.executable)
    }

    /// Absolute path of the build to actually launch.
    ///
    /// [`Self::executable_path`] is the name the installer's shortcut
    /// pointed at; a cabinet that ships one build per 3D chip expects
    /// its setup DLL to have replaced that file at install time. See
    /// [`accelerated_renderer_build`].
    pub fn launch_path(&self, library_root: &Path) -> PathBuf {
        let exe = self.executable_path(library_root);
        accelerated_renderer_build(&exe).unwrap_or(exe)
    }

    /// Guest directory the installer would have written this game to,
    /// normalised to exactly one trailing backslash (for example
    /// `"\\Program Files\\EA\\Spore v1.0.4\\"`).
    ///
    /// Games that keep their assets in one archive open it by absolute
    /// path -- Spore Origins asks for `<install dir>\data.vfs` -- so the
    /// extracted directory has to be mounted there as well as under the
    /// generic `\Program Files\` prefix. Without it the open fails and
    /// the game calls through the handle it never managed to store.
    pub fn guest_install_prefix(&self) -> Option<String> {
        let raw = self.install_dir.as_deref()?.trim();
        let trimmed = raw.trim_end_matches('\\');
        if trimmed.is_empty() {
            return None;
        }
        Some(format!("{trimmed}\\"))
    }

    /// Path `GetModuleFileNameW` should report for this game, derived
    /// from [`Self::guest_install_prefix`] and the executable's file
    /// name. Relative guest paths resolve against its directory.
    pub fn guest_exe_path(&self) -> Option<String> {
        let prefix = self.guest_install_prefix()?;
        let name = self.executable.file_name()?.to_str()?;
        Some(format!("{prefix}{name}"))
    }

    pub fn guest_save_prefix(&self) -> Option<String> {
        self.save_prefix
            .clone()
            .or_else(|| self.guest_install_prefix())
    }
}

/// Runtime settings stored per game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSettings {
    /// Experimental OpenGL host rasterization; None inherits the global choice.
    #[serde(default)]
    pub gpu_renderer: Option<bool>,
    /// None inherits the global GPU-only anisotropic filter (0, 2, 4, 8).
    #[serde(default)]
    pub anisotropic_filtering: Option<u8>,
    /// Which CPU backend the user prefers for this game.
    #[serde(default)]
    pub cpu_backend: CpuBackendPref,
    /// Maximum number of host-resumed slices per run; zero means unlimited.
    #[serde(default = "default_max_slices")]
    pub max_slices: u64,
    /// Instructions per slice budget passed to the CPU.
    #[serde(default = "default_instructions_per_slice")]
    pub instructions_per_slice: u64,
    /// If true, the run loop halts as soon as an unimplemented API
    /// is encountered (great for debugging).
    #[serde(default)]
    pub halt_on_unimplemented: bool,
    /// Emulated display geometry. Smartphone builds are shipped for a
    /// specific screen and query it once at start-up, so a landscape
    /// title rendered into a portrait framebuffer comes out clipped.
    #[serde(default)]
    pub screen: ScreenPref,
    /// How far to turn the *presented* frame before it reaches the
    /// user. Presentation only — the guest still renders into the
    /// geometry [`Self::screen`] asked for, and stylus coordinates are
    /// un-rotated on the way back in.
    ///
    /// This is the setting for a landscape game that ships as a
    /// portrait build. Plenty of Pocket PC titles laid a landscape
    /// scene out inside the stock 240x320 panel and expected the user
    /// to turn the device; forcing `screen` to 320x240 instead makes
    /// them lay out against geometry they never shipped against, and
    /// some break outright. Leaving the guest at 240x320 and turning
    /// the presented frame a quarter turn is what actually works.
    #[serde(default)]
    pub rotation: RotationPref,
    /// Touch D-pad and directional analog-stick sensitivity, per game.
    #[serde(default)]
    pub dpad_deadzone: DpadDeadzone,
}

/// Digital D-pad contacts stay direct; these thresholds apply to analog sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DpadDeadzone {
    Small,
    Medium,
    #[default]
    Large,
}
impl DpadDeadzone {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small (sensitive)",
            Self::Medium => "Medium",
            Self::Large => "Large (precise)",
        }
    }
    pub fn touch_radius(self) -> f32 {
        match self {
            Self::Small => 0.16,
            Self::Medium => 0.305,
            Self::Large => 0.45,
        }
    }
    pub fn stick_threshold(self, held: bool) -> f32 {
        let (press, release) = match self {
            Self::Small => (0.35, 0.20),
            Self::Medium => (0.60, 0.35),
            Self::Large => (0.75, 0.50),
        };
        if held {
            release
        } else {
            press
        }
    }
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            gpu_renderer: None,
            anisotropic_filtering: None,
            cpu_backend: CpuBackendPref::default(),
            max_slices: default_max_slices(),
            instructions_per_slice: default_instructions_per_slice(),
            halt_on_unimplemented: false,
            screen: ScreenPref::default(),
            rotation: RotationPref::default(),
            dpad_deadzone: DpadDeadzone::default(),
        }
    }
}

/// Quarter-turn applied to the presented frame, shared by every
/// frontend so a game rotated on the desktop is rotated on Android
/// too.
///
/// The angles are clockwise, named the way a user thinks about them
/// ("turn the picture 90° right"), and the frontends are responsible
/// for applying the *inverse* rotation to pointer input — see
/// `pocket-desktop`'s `rotated_pointer_to_game` and the Android
/// `GameActivity.mapTouchToGame`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RotationPref {
    #[default]
    None,
    /// 90° clockwise — the one a portrait-rendered landscape game needs.
    Cw90,
    Half,
    /// 90° counter-clockwise.
    Ccw90,
}

impl RotationPref {
    pub fn label(self) -> &'static str {
        match self {
            RotationPref::None => "0\u{b0}",
            RotationPref::Cw90 => "90\u{b0} clockwise",
            RotationPref::Half => "180\u{b0}",
            RotationPref::Ccw90 => "90\u{b0} counter-clockwise",
        }
    }

    /// Degrees clockwise, which is what the Android renderer wants.
    pub fn degrees(self) -> u32 {
        match self {
            RotationPref::None => 0,
            RotationPref::Cw90 => 90,
            RotationPref::Half => 180,
            RotationPref::Ccw90 => 270,
        }
    }

    /// True when the turn swaps width and height.
    pub fn is_quarter_turn(self) -> bool {
        matches!(self, RotationPref::Cw90 | RotationPref::Ccw90)
    }

    pub const ALL: [RotationPref; 4] = [
        RotationPref::None,
        RotationPref::Cw90,
        RotationPref::Half,
        RotationPref::Ccw90,
    ];
}

/// Emulated display geometry for one game.
///
/// A GAPI game asks the OS for the screen size once (`GetSystemMetrics`
/// / `GXGetDisplayProperties`) and lays its whole HUD out around the
/// answer, so this has to be right before the first frame. The default
/// is the classic 240x320 Pocket PC portrait panel; Smartphone ports
/// built for a landscape device — Asphalt 2 3D's Motorola Q9 build, for
/// one — need 320x240 or their menus and speedometer are cut off at the
/// right edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScreenPref {
    /// 240x320 portrait (Pocket PC / most Smartphone builds).
    #[default]
    Portrait,
    /// Explicit QVGA override; Portrait remains the legacy automatic default.
    Qvga,
    /// 320x240 landscape (Motorola Q / other landscape Smartphones).
    Landscape,
    /// 176x220 — the small Smartphone panel.
    SmallPortrait,
    /// 480x640 VGA Pocket PC display.
    Vga,
    /// 640x480 landscape VGA Pocket PC display.
    VgaLandscape,
    /// 480x800 WVGA Windows Phone display.
    Wvga,
    /// 480x320 — the Handheld PC / H/PC landscape panel.
    ///
    /// Windows CE's bundled applets (Solitaire among them) are H/PC
    /// class and lay their UI out against this geometry. Launching
    /// them at the 240x320 Pocket PC default leaves the window
    /// clipped, so they need their own entry rather than being
    /// squeezed into `Landscape`.
    Hpc,
}

impl ScreenPref {
    /// CEUX/HI_RES_AWARE is the Windows Mobile declaration that a program
    /// accepts native high-resolution coordinates. Use a VGA device for that
    /// declaration; keep legacy QVGA for unmarked software. Explicit profiles
    /// always win, and Gizmondo callers continue to force 320x240.
    pub fn resolve_image(self, image: Option<&pocket_pe::LoadedImage>) -> (u32, u32) {
        if self != Self::Portrait {
            return self.size();
        }
        let high_resolution = image.is_some_and(|image| {
            image.resources.iter().any(|resource| {
                use pocket_pe::resources::ResourceKey;
                if !matches!(&resource.ty,ResourceKey::Name(s) if s=="CEUX")
                    || !matches!(&resource.name,ResourceKey::Name(s) if s=="HI_RES_AWARE")
                    || resource.size < 2
                {
                    return false;
                }
                let va = resource.data_rva;
                image.sections.iter().any(|section| {
                    let Some(offset) = va.checked_sub(section.virtual_address).map(|v| v as usize)
                    else {
                        return false;
                    };
                    section.data.get(offset..offset.saturating_add(2)) == Some(&[1, 0][..])
                })
            })
        });
        if high_resolution {
            Self::Vga.size()
        } else {
            Self::Portrait.size()
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ScreenPref::Portrait => "Automatic (Pocket PC)",
            ScreenPref::Qvga => "240x320 (QVGA portrait)",
            ScreenPref::Landscape => "320x240 (landscape)",
            ScreenPref::SmallPortrait => "176x220 (small)",
            ScreenPref::Vga => "480x640 (VGA portrait)",
            ScreenPref::VgaLandscape => "640x480 (VGA landscape)",
            ScreenPref::Wvga => "480x800 (WVGA)",
            ScreenPref::Hpc => "480x320 (Handheld PC)",
        }
    }

    /// `(width, height)` in pixels.
    pub fn size(self) -> (u32, u32) {
        match self {
            ScreenPref::Portrait | ScreenPref::Qvga => (240, 320),
            ScreenPref::Landscape => (320, 240),
            ScreenPref::SmallPortrait => (176, 220),
            ScreenPref::Vga => (480, 640),
            ScreenPref::VgaLandscape => (640, 480),
            ScreenPref::Wvga => (480, 800),
            ScreenPref::Hpc => (480, 320),
        }
    }
}

fn default_max_slices() -> u64 {
    // Interactive play ends on Stop or guest exit, not an accumulated
    // dispatch count. Jump polls its frame clock in a busy loop and can
    // consume the old 50-million budget in under a minute of normal play.
    // Explicit saved limits remain available for bounded diagnostic runs.
    0
}

fn default_instructions_per_slice() -> u64 {
    1_000_000
}

/// User preference for the CPU backend. `Unicorn` is the only backend
/// that actually executes ARM code — `Stub` is trace-only and cannot
/// run a real game. Both frontends default to `Unicorn` and only fall
/// back to `Stub` when the user explicitly picks it (e.g. for API
/// tracing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CpuBackendPref {
    Stub,
    #[default]
    Unicorn,
}

impl CpuBackendPref {
    pub fn label(self) -> &'static str {
        match self {
            CpuBackendPref::Stub => "Stub (trace-only)",
            CpuBackendPref::Unicorn => "Unicorn (ARM)",
        }
    }
}

fn default_gpu_renderer() -> bool {
    true
}

/// Hexadecimal opaque identifiers. A blank platform uses the persistent device registry.
fn default_device_preset_id() -> String {
    "506F636B6574484C452D57696E4345".into()
}
fn default_gizmondo_preset_id() -> String {
    // Confirmed in the Gizmondo OAL: UTF-16LE "Gizmondo", including NUL.
    "470069007A006D006F006E0064006F000000".into()
}

fn default_gizmondo_platform_id() -> String {
    "AD0D71F3E65E5B9A47E4773478FA926B".into()
}
fn identity_revision() -> u8 {
    1
}

/// Persistent global configuration. Lives at
/// `<library_root>/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    #[serde(default = "default_device_preset_id")]
    pub device_preset_id: String,
    #[serde(default = "String::new")]
    pub device_platform_id: String,
    #[serde(default = "default_gizmondo_preset_id")]
    pub gizmondo_preset_id: String,
    #[serde(default = "default_gizmondo_platform_id")]
    pub gizmondo_platform_id: String,
    #[serde(default)]
    pub gizmondo_identity_revision: u8,
    /// Host GPU rendering is enabled by default; games may override it.
    #[serde(default = "default_gpu_renderer")]
    pub gpu_renderer: bool,
    /// GPU-only texture anisotropy. Zero disables it.
    #[serde(default)]
    pub anisotropic_filtering: u8,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Default CPU backend used when importing a new game.
    #[serde(default)]
    pub default_cpu_backend: CpuBackendPref,
    /// Default verbosity level (0..=3).
    #[serde(default)]
    pub verbosity: u8,
    /// Append missing API reports beside the main launcher log.
    #[serde(default = "default_log_unimplemented_apis")]
    pub log_unimplemented_apis: bool,
    #[serde(default)]
    pub bluetooth_enabled: bool,
    /// Allow CAM1 to acquire the host camera, only after the guest starts it.
    #[serde(default)]
    pub camera_enabled: bool,
    /// Allow GPS1 to subscribe to the host location service.
    #[serde(default)]
    pub gps_enabled: bool,
    /// Explicit simulated GPS on Windows and Android, independent of host permission.
    #[serde(default)]
    pub gps_fixed_enabled: bool,
    /// Pick a simulated fix within 800 m of the Eiffel Tower at launch or explicit refresh.
    #[serde(default)]
    pub gps_fixed_random: bool,
    #[serde(default)]
    pub gps_fixed_latitude: f64,
    #[serde(default)]
    pub gps_fixed_longitude: f64,
    /// Bridge Gizmondo GPRS shell requests to the configured host transport.
    #[serde(default = "default_gprs_enabled")]
    pub gprs_enabled: bool,
    /// Replacement origin for Colors Turf War; missing/blank values use the public server.
    #[serde(
        default = "default_colors_server_url",
        deserialize_with = "deserialize_colors_server_url"
    )]
    pub colors_server_url: String,
    /// Optional explicit device ID for separate multiplayer test installations.
    #[serde(default)]
    pub colors_terminal_id: String,
    /// Global Colors text language; unavailable translations fall back to English.
    #[serde(default = "default_colors_language")]
    pub colors_language: String,
    /// Desktop smoothing filter, remembered across sessions.
    #[serde(default = "default_upscale_filter")]
    pub upscale_filter: String,
    /// Android presentation scale: 0 = fit, 1..4 = native multiple (clamped to fit).
    #[serde(default)]
    pub android_display_scale: u32,
    /// Last folder the user picked a `.cab` from. Used to remember
    /// the file dialog start directory.
    #[serde(default)]
    pub last_import_dir: Option<PathBuf>,
    /// Render a j2me-loader-style FPS counter on top of the
    /// in-game framebuffer. Defaults to `true` so a freshly
    /// installed launcher shows the counter out of the box; users
    /// who find it distracting can switch it off in launcher
    /// settings.
    #[serde(default = "default_show_fps")]
    pub show_fps: bool,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default = "default_fullscreen_mode")]
    pub fullscreen_mode: String,
    #[serde(default = "default_orientation")]
    pub orientation: String,
    /// Host key → guest button map, shared by both frontends.
    ///
    /// Lives in the global config rather than per game: a user rebinds
    /// the D-pad to match their keyboard once, not once per title.
    #[serde(default)]
    pub keybindings: KeyBindings,
    #[serde(default)]
    pub stylus_stick: stylus::StylusBindings,
    /// Physical controller inputs, independent of keyboard bindings.
    #[serde(default = "keybindings::default_gamepad_bindings")]
    pub gamepad_bindings: std::collections::BTreeMap<String, GuestButton>,
    /// Desktop host action, independent of guest Piano 5 / VK_F11.
    #[serde(default = "keybindings::default_gamepad_fullscreen")]
    pub gamepad_fullscreen: Option<String>,
    /// Show the `Backend: … / Running…` status line under the game.
    ///
    /// It is a debug read-out, and on a phone in landscape it eats a
    /// row of pixels the game could have used, so it can be switched
    /// off. Defaults to `true` to keep the existing look.
    #[serde(default = "default_show_backend_log")]
    pub show_backend_log: bool,
    /// Opacity of the Android on-screen control pad, `0.1..=1.0`.
    ///
    /// A game that draws its own HUD near the bottom of the panel
    /// (JumpyBall's score line, for one) is hidden behind an opaque
    /// pad once the pad overlays the surface in landscape, so the user
    /// can fade it.
    #[serde(default = "default_controls_opacity")]
    pub controls_opacity: f32,
}

fn default_log_unimplemented_apis() -> bool {
    true
}
fn default_colors_language() -> String {
    "english".into()
}

/// Colors is selected by card identity, never by a user-editable display name.
pub fn is_colors_game(game: &GameEntry, root: &Path) -> bool {
    game_gizmondo_title_id(game, root).is_some_and(|id| {
        id.eq_ignore_ascii_case("GZGA200015") || id.eq_ignore_ascii_case("GZGA270015")
    })
}

/// Languages physically available beside this Colors installation's strings.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ColorsLanguage {
    pub id: &'static str,
    pub label: &'static str,
}

pub fn colors_languages(game: &GameEntry, root: &Path) -> Vec<ColorsLanguage> {
    if !is_colors_game(game, root) {
        return Vec::new();
    }
    let exe = game.executable_path(root);
    let Some(directory) = exe.parent() else {
        return Vec::new();
    };
    let card_root = game.gizmondo_card_root(root);
    let mut directories = vec![directory.to_path_buf()];
    // Walk up only inside this card, never into another installed variant.
    {
        let card = card_root;
        let mut parent = directory;
        while parent != card && parent.starts_with(&card) {
            let Some(next) = parent.parent() else {
                break;
            };
            directories.push(next.to_path_buf());
            parent = next;
        }
    }
    for directory in directories {
        let names: Vec<_> = fs::read_dir(directory)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .map(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase())
            .collect();
        let mut languages = Vec::new();
        for (id, label, extensions) in [
            ("english", "English", &["eng"][..]),
            ("french", "French", &["fre", "fra"][..]),
            ("german", "German", &["ger", "deu"][..]),
            ("italian", "Italian", &["ita"][..]),
            ("spanish", "Spanish", &["spa", "esp"][..]),
        ] {
            if extensions.iter().any(|extension| {
                names
                    .iter()
                    .filter(|name| **name == format!("bstrings.{extension}"))
                    .count()
                    == 1
            }) {
                languages.push(ColorsLanguage { id, label });
            }
        }
        if !languages.is_empty() {
            return languages;
        }
    }
    Vec::new()
}

/// One-language cards start directly; unavailable preferences fall back to English.
pub fn colors_launch_language(game: &GameEntry, root: &Path, preferred: &str) -> &'static str {
    let languages = colors_languages(game, root);
    if languages.len() == 1 {
        return languages[0].id;
    }
    languages
        .iter()
        .find(|language| language.id == preferred)
        .or_else(|| languages.iter().find(|language| language.id == "english"))
        .or_else(|| languages.first())
        .map_or("english", |language| language.id)
}

fn default_colors_server_url() -> String {
    "https://vps-50cdd0fc.vps.ovh.ca".to_owned()
}

fn deserialize_colors_server_url<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    Ok(if value.trim().is_empty() {
        default_colors_server_url()
    } else {
        value
    })
}

fn default_gprs_enabled() -> bool {
    true
}
fn default_upscale_filter() -> String {
    "reconstruction".into()
}

fn default_show_fps() -> bool {
    true
}

fn default_show_backend_log() -> bool {
    true
}

fn default_controls_opacity() -> f32 {
    1.0
}

fn default_orientation() -> String {
    "auto".to_string()
}

fn default_fullscreen_mode() -> String {
    "with_controls".to_string()
}

impl LauncherConfig {
    pub fn gamepad_toggles_fullscreen(&self, control: &str) -> bool {
        // Preserve an existing custom guest binding when upgrading old configs.
        self.gamepad_fullscreen.as_deref() == Some(control)
            && !self.gamepad_bindings.contains_key(control)
    }
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            device_preset_id: default_device_preset_id(),
            device_platform_id: String::new(),
            gizmondo_preset_id: default_gizmondo_preset_id(),
            gizmondo_platform_id: default_gizmondo_platform_id(),
            gizmondo_identity_revision: identity_revision(),
            gpu_renderer: true,
            anisotropic_filtering: 0,
            schema_version: 1,
            default_cpu_backend: CpuBackendPref::default(),
            verbosity: 1,
            log_unimplemented_apis: default_log_unimplemented_apis(),
            bluetooth_enabled: true,
            camera_enabled: true,
            gps_enabled: true,
            gps_fixed_enabled: false,
            gps_fixed_random: false,
            gps_fixed_latitude: 0.,
            gps_fixed_longitude: 0.,
            gprs_enabled: default_gprs_enabled(),
            colors_server_url: default_colors_server_url(),
            colors_terminal_id: String::new(),
            colors_language: default_colors_language(),
            upscale_filter: default_upscale_filter(),
            android_display_scale: 0,
            last_import_dir: None,
            show_fps: default_show_fps(),
            fullscreen: false,
            fullscreen_mode: default_fullscreen_mode(),
            orientation: default_orientation(),
            keybindings: KeyBindings::default(),
            stylus_stick: Default::default(),
            gamepad_bindings: keybindings::default_gamepad_bindings(),
            gamepad_fullscreen: keybindings::default_gamepad_fullscreen(),
            show_backend_log: default_show_backend_log(),
            controls_opacity: default_controls_opacity(),
        }
    }
}

/// Top-level handle to an on-disk PocketHLE library.
///
/// Cheap to clone: the only state is the root path and the in-memory
/// registry; mutations are written through to disk immediately.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
    library: LibraryFile,
    config: LauncherConfig,
}

/// An import extracted off the UI thread, without changing installed games.
/// Dropping it cancels the operation and deletes its private staging directory.
#[derive(Debug)]
pub struct PreparedImport {
    staging: tempfile::TempDir,
    target_root: PathBuf,
    zip_source: bool,
    entries: Vec<GameEntry>,
    updates: Vec<GameEntry>,
}

impl PreparedImport {
    /// Ask only for unresolved ZIP games, not CABs or already classified updates.
    pub fn platform_choices(&self) -> Vec<(String, String)> {
        if !self.zip_source {
            return Vec::new();
        }
        self.entries
            .iter()
            .filter(|entry| {
                entry.platform.is_none()
                    && !entry.source_cab.to_ascii_lowercase().ends_with(".cab")
                    && !is_gizmondo_game(entry, self.staging.path())
                    && !self
                        .updates
                        .iter()
                        .any(|old| old.id == entry.id && old.platform.is_some())
            })
            .map(|entry| (entry.id.clone(), entry.display_name.clone()))
            .collect()
    }
    pub fn choose_platform(&mut self, id: &str, platform: GamePlatform) -> bool {
        if !self.platform_choices().iter().any(|(key, _)| key == id) {
            return false;
        }
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.platform = Some(platform);
            return true;
        }
        false
    }

    pub fn entries(&self) -> &[GameEntry] {
        &self.entries
    }

    pub fn updates(&self) -> &[GameEntry] {
        &self.updates
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LibraryFile {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    games: Vec<GameEntry>,
}

/// Deck settings are isolated while the imported catalogue remains shared.
pub const CONFIG_FILE_NAME: &str = if cfg!(feature = "steam-deck") {
    "config-steamdeck.json"
} else {
    "config.json"
};

/// Resolve the default library root, in the order the desktop
/// launcher has always used:
///
/// 1. the `POCKETHLE_LIBRARY` environment variable,
/// 2. `<documents>/PocketHLE` (e.g. `~/Documents/PocketHLE`),
/// 3. `<data_dir>/PocketHLE/library` (XDG / `%APPDATA%`),
/// 4. `./pockethle-library` as a last resort.
///
/// Lives here rather than in a frontend so the CLI's `import` command
/// and the GUI always agree on which library they are touching.
pub fn default_library_root() -> PathBuf {
    if let Some(p) = std::env::var_os("POCKETHLE_LIBRARY") {
        return PathBuf::from(p);
    }
    if let Some(dirs) = directories::UserDirs::new() {
        if let Some(docs) = dirs.document_dir() {
            return docs.join("PocketHLE");
        }
    }
    if let Some(dirs) = directories::ProjectDirs::from("ai", "PocketHLE", "PocketHLE") {
        return dirs.data_dir().join("library");
    }
    PathBuf::from("./pockethle-library")
}

impl Library {
    /// Prepare any supported source once. The ordinary importers run in an
    /// isolated library, so their ZIP classification and executable selection
    /// are exactly the same as a normal import, including collections.
    pub fn prepare_import(&self, path: impl AsRef<Path>) -> Result<PreparedImport, LibraryError> {
        use std::io::Read;
        let path = path.as_ref();
        let staging = tempfile::Builder::new()
            .prefix(".pending-import-")
            .tempdir_in(self.root.join("games"))?;
        let mut provisional = Library::open(staging.path())?;
        provisional.config = self.config.clone();
        let mut signature = [0; 4];
        let count = fs::File::open(path)?.read(&mut signature)?;
        let ext = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let zip_source = count == 4 && signature[..2] == *b"PK" || ext == "zip";
        let _imported = if zip_source {
            provisional.import_zip_entries(path)?
        } else {
            let entry = if signature == *b"MSCF" || ext == "cab" {
                provisional.import_cab(path)?
            } else if signature == *b"Rar!" || ext == "rar" {
                provisional.import_rar(path)?
            } else {
                provisional.import_exe(path)?
            };
            vec![entry.clone()]
        };
        // Nested installers/RAR wrappers may import more than the first entry
        // returned by their compatibility API. Publish every prepared game.
        let entries = provisional.games().to_vec();
        let mut updates = Vec::new();
        for entry in &entries {
            let source = entry
                .source_cab
                .strip_prefix("gzga-collection:")
                .unwrap_or(&entry.source_cab);
            if let Some(previous) = self.previous_import(&entry.id, source)? {
                updates.push(previous);
            }
        }
        Ok(PreparedImport {
            staging,
            target_root: self.root.clone(),
            zip_source,
            entries,
            updates,
        })
    }

    /// Publish only after the frontend's update confirmation. Re-read current
    /// preferences and saves; do not overwrite changes made during the dialog.
    pub fn apply_prepared_import(
        &mut self,
        pending: PreparedImport,
        allow_updates: bool,
    ) -> Result<Vec<GameEntry>, LibraryError> {
        if pending.target_root != self.root {
            return Err(LibraryError::InvalidId(
                "import belongs to another library".into(),
            ));
        }
        let mut prepared = Vec::new();
        for mut entry in pending.entries {
            let source = entry
                .source_cab
                .strip_prefix("gzga-collection:")
                .unwrap_or(&entry.source_cab);
            let previous = self.previous_import(&entry.id, source)?;
            if let Some(previous) = &previous {
                if !allow_updates || !pending.updates.iter().any(|game| game.id == previous.id) {
                    return Err(LibraryError::InvalidId(format!(
                        "update confirmation required for {}",
                        previous.display_name
                    )));
                }
            }
            let directory = pending.staging.path().join("games").join(&entry.id);
            if previous.is_some() {
                let incoming_saves = directory.join(save_data::SAVE_DIR_NAME);
                if incoming_saves.exists() {
                    fs::remove_dir_all(incoming_saves)?;
                }
            }
            self.preserve_import_preferences(&mut entry, previous.as_ref(), &directory)?;
            write_json(&directory.join("game.json"), &entry)?;
            prepared.push((entry, directory, previous));
        }
        self.publish_staged_imports(pending.staging, prepared, None)
    }

    /// Open the library rooted at `root`, creating the directory and
    /// default `library.json` / `config.json` if they don't exist.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, LibraryError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        fs::create_dir_all(root.join("games"))?;

        let library = read_or_default::<LibraryFile>(&root.join("library.json"))?;
        let mut config = read_or_default::<LauncherConfig>(&root.join(CONFIG_FILE_NAME))?;
        if config.gizmondo_identity_revision == 0 {
            config.gizmondo_platform_id = default_gizmondo_platform_id();
            config.gizmondo_identity_revision = identity_revision();
            write_json(&root.join(CONFIG_FILE_NAME), &config)?;
        }
        keybindings::migrate_steam_deck_gamepad_bindings(&mut config.gamepad_bindings);
        let mut this = Self {
            root,
            library,
            config,
        };
        this.migrate_legacy_entries();
        Ok(this)
    }

    /// Upgrade game entries and global config persisted by older
    /// versions of PocketHLE so they are usable under the current
    /// defaults.
    ///
    /// * `cpu_backend = Stub` is bumped to `Unicorn`. Stub is
    ///   trace-only — it never executes ARM instructions, so any
    ///   pre-#8 game crashes with "guest jumped to unmapped address
    ///   0x00000000" out of the box, which is exactly the failure
    ///   mode users hit before this migration existed.
    /// * `max_slices <= 1_048_576` is bumped to the current default
    ///   (50M). The legacy 1024 default was effectively a smoke
    ///   test and timed out long before the title screen.
    /// * The global `default_cpu_backend` is also flipped from
    ///   `Stub` to `Unicorn` so freshly imported games pick a
    ///   backend that actually runs.
    ///
    /// Both the in-memory state and the on-disk `game.json` /
    /// `library.json` / `config.json` are updated so subsequent
    /// launches see the migrated values. Migration is silent when
    /// no changes are needed.
    fn migrate_legacy_entries(&mut self) {
        let mut library_changed = false;
        for game in self.library.games.iter_mut() {
            let mut migrated = false;
            // Upgrade ID placeholders and corrected catalogue names.
            // Other user-edited names are preserved.
            if let Some(id) = game_gizmondo_title_id(game, &self.root) {
                let corrected_date = match id.as_str() {
                    "GZAP200001" => ["Copilot Navigation 2005", "Copilot UK"]
                        .iter()
                        .any(|old| game.display_name.eq_ignore_ascii_case(old)),
                    "GZGA900069" => game
                        .display_name
                        .eq_ignore_ascii_case("Catapult (may 2005)"),
                    "GZGA800056" => ["Agaju (oct 2005)", "Agaju (october 2005)"]
                        .iter()
                        .any(|old| game.display_name.eq_ignore_ascii_case(old)),
                    _ => false,
                };
                if game.display_name.eq_ignore_ascii_case(&id) || corrected_date {
                    if let Some(title) = gizmondo_titles::lookup(&id) {
                        game.display_name = title.to_owned();
                        migrated = true;
                    }
                }
            }
            if game.settings.cpu_backend == CpuBackendPref::Stub {
                game.settings.cpu_backend = CpuBackendPref::Unicorn;
                migrated = true;
            }
            if game.settings.max_slices <= 1_048_576 {
                game.settings.max_slices = default_max_slices();
                migrated = true;
            }
            if migrated {
                library_changed = true;
                let manifest = self.root.join("games").join(&game.id).join("game.json");
                if let Err(e) = write_json(&manifest, game) {
                    log::warn!("could not migrate {}: {e}", manifest.display());
                }
                log::info!(
                    "migrated legacy game entry {}: backend=Unicorn, max_slices={}",
                    game.id,
                    game.settings.max_slices,
                );
            }
        }
        let mut config_changed = false;
        if self.config.default_cpu_backend == CpuBackendPref::Stub {
            self.config.default_cpu_backend = CpuBackendPref::Unicorn;
            config_changed = true;
        }
        // A `config.json` written before a button existed lists every
        // other button, so `#[serde(default)]` cannot help: the map
        // deserialises fine and is simply missing an entry. Fill it in
        // rather than leaving the new button dead.
        let buttons_before = self.config.keybindings.bindings.len();
        self.config.keybindings.fill_missing_buttons();
        if self.config.keybindings.bindings.len() != buttons_before {
            config_changed = true;
        }
        let clamped = self.config.controls_opacity.clamp(0.1, 1.0);
        if clamped != self.config.controls_opacity {
            self.config.controls_opacity = clamped;
            config_changed = true;
        }
        if library_changed {
            self.library
                .games
                .sort_by(|a, b| a.display_name.cmp(&b.display_name));
            if let Err(e) = write_json(&self.root.join("library.json"), &self.library) {
                log::warn!("could not save migrated library.json: {e}");
            }
        }
        if config_changed {
            if let Err(e) = write_json(&self.root.join(CONFIG_FILE_NAME), &self.config) {
                log::warn!("could not save migrated config.json: {e}");
            }
        }
    }

    /// Library root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All known games, sorted by display name.
    pub fn games(&self) -> &[GameEntry] {
        &self.library.games
    }

    /// Look up one game by id.
    pub fn get(&self, id: &str) -> Option<&GameEntry> {
        self.library.games.iter().find(|g| g.id == id)
    }

    /// Mutable access to one game by id.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut GameEntry> {
        self.library.games.iter_mut().find(|g| g.id == id)
    }

    /// Persist the current `library.json` and `config.json` to disk.
    pub fn save(&self) -> Result<(), LibraryError> {
        write_json(&self.root.join("library.json"), &self.library)?;
        write_json(&self.root.join(CONFIG_FILE_NAME), &self.config)?;
        Ok(())
    }

    /// Read-only view of the global launcher config.
    pub fn config(&self) -> &LauncherConfig {
        &self.config
    }

    /// Mutable view of the global launcher config. The caller is
    /// responsible for calling [`Library::save`] when finished.
    pub fn config_mut(&mut self) -> &mut LauncherConfig {
        &mut self.config
    }

    /// Import a Pocket PC `.CAB` into the library.
    ///
    /// Returns the freshly created [`GameEntry`]. The cab is extracted
    /// into `<library_root>/games/<id>/extracted/`, where `<id>` is
    /// derived from the source cab's filename. Existing entries with
    /// the same id are replaced (the directory is wiped first).
    pub fn import_cab(&mut self, cab_path: impl AsRef<Path>) -> Result<&GameEntry, LibraryError> {
        let cab_path = cab_path.as_ref();
        let source_cab = cab_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.cab".to_string());
        let id = sanitize_id(cab_path.file_stem().map(|s| s.to_string_lossy()).as_deref());
        if id.is_empty() {
            return Err(LibraryError::InvalidId(source_cab));
        }
        let game_dir = self.root.join("games").join(&id);
        if game_dir.exists() {
            fs::remove_dir_all(&game_dir)?;
        }
        let extracted_dir = game_dir.join("extracted");
        fs::create_dir_all(&extracted_dir)?;

        let (files, header) = pocket_cab::extract_for_install(cab_path, &extracted_dir)?;
        for (index, companion) in find_resource_companion_cabs(cab_path)
            .into_iter()
            .enumerate()
        {
            import_resource_companion_cab(&game_dir, &extracted_dir, index, &companion);
        }
        // A cabinet stores its payload under generated 8.3 names and keeps
        // the real destination names in `_setup.xml`. The game only ever
        // opens the long ones -- Asphalt 2 3D wants `light.bar` next to
        // `Asphalt2_SPV_C600.exe` -- so recreate them before picking an
        // entry point. Without this a title that runs fine through
        // `pockethle run` fails to load its data from the library.
        let long_names = pocket_cab::materialise_setup_names(&extracted_dir, &files);
        // Cabs predating `_setup.xml` keep the same mapping in their
        // binary `.000` header instead. Rayman Ultimate records all 198
        // of its payload names there and nowhere else, so a library
        // import that skips this step ends up with a directory of
        // `00000RAY.004`-style names the game cannot open.
        let structured_header = header.as_ref().filter(|h| h.structured);
        if let Some(h) = structured_header {
            pocket_cab::materialise_install_header_names(&extracted_dir, &files, h);
        } else {
            materialise_legacy_assets(
                &extracted_dir,
                &files,
                header.as_ref().and_then(|h| h.app_name.as_deref()),
            );
            materialise_legacy_install_files(&extracted_dir, &files, header.as_ref());
        }

        let setup = files
            .iter()
            .find(|f| f.short_name.eq_ignore_ascii_case("_setup.xml"))
            .and_then(|f| fs::read(&f.extracted_path).ok())
            .map(|bytes| pocket_cab::WinCeSetupScript::parse_bytes(&bytes));
        let materialised_exe = setup
            .as_ref()
            .and_then(|script| {
                let install_root = script.install_root();
                let by_long_path = |long: &str| {
                    let relative = script.relative_destination(long, install_root.as_deref())?;
                    let candidate = relative
                        .split('\\')
                        .filter(|s| !s.is_empty())
                        .fold(extracted_dir.to_path_buf(), |acc, seg| acc.join(seg));
                    (is_guest_exe(&candidate)).then_some(candidate)
                };
                if let Some(target) = &script.shortcut_target {
                    if target.to_ascii_lowercase().ends_with(".exe") {
                        if let Some(path) = by_long_path(target) {
                            return Some(path);
                        }
                    }
                }
                script
                    .renames
                    .iter()
                    .filter(|(_, long)| long.to_ascii_lowercase().ends_with(".exe"))
                    .filter_map(|(_, long)| by_long_path(long))
                    .max_by_key(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
            })
            .or_else(|| {
                // `.000`-only cabs: the LINKS section names the binary
                // the device's shell would launch, which beats picking
                // the largest PE when a cab ships helper executables.
                let header = structured_header?;
                let by_dest = |dest: &str| {
                    header
                        .host_path(&extracted_dir, dest)
                        .filter(|p| is_guest_exe(p))
                };
                if let Some(target) = header
                    .shortcut_target
                    .as_deref()
                    .filter(|t| t.to_ascii_lowercase().ends_with(".exe"))
                {
                    if let Some(path) = by_dest(target) {
                        return Some(path);
                    }
                }
                header
                    .files
                    .iter()
                    .filter(|e| e.destination.to_ascii_lowercase().ends_with(".exe"))
                    .filter_map(|e| by_dest(&e.destination))
                    .max_by_key(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
            });
        let (mut exe_abs, _) = if let Some(path) = materialised_exe {
            (
                path.clone(),
                fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            )
        } else {
            // Legacy cabinets without setup.xml do not provide long-name
            // mappings, so retain the size-based PE fallback.
            let mut best: Option<(PathBuf, u64)> = None;
            for f in &files {
                let lower = f.short_name.to_ascii_lowercase();
                if lower.ends_with(".000") || lower.ends_with(".dll") {
                    continue;
                }
                if !is_pe_file(&f.extracted_path) {
                    continue;
                }
                // A cabinet renames its payload to 8.3 names, so the
                // extension check above misses a satellite library
                // stored as `PEGCAR~1.002`. The COFF DLL bit does not
                // lie. A cabinet that ships *only* libraries still
                // falls through to `NoExecutable` below.
                if is_guest_dll(&f.extracted_path) {
                    continue;
                }
                if best.as_ref().map(|(_, sz)| *sz).unwrap_or(0) < f.size {
                    best = Some((f.extracted_path.clone(), f.size));
                }
            }
            best.ok_or(LibraryError::NoExecutable)?
        };
        // Prefer the long name `_setup.xml` asked for: that is what the
        // installer would have written on the device, and it is the name
        // `GetModuleFileNameW` reports to the game.
        if let Some((_, long)) = long_names.iter().find(|(short, _)| *short == exe_abs) {
            exe_abs = long.clone();
        }
        let executable = exe_abs
            .strip_prefix(&game_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or(exe_abs.clone());

        let setup_app_name = setup.as_ref().and_then(|script| script.app_name.clone());
        let display_name = setup_app_name
            .or_else(|| header.as_ref().and_then(|h| h.app_name.clone()))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| pretty_id(&id));
        let provider = header.as_ref().and_then(|h| h.provider.clone());

        let registry = {
            let from_setup = setup_registry(&files);
            if from_setup.is_empty() {
                header
                    .as_ref()
                    .map(|h| h.registry.clone())
                    .unwrap_or_default()
            } else {
                from_setup
            }
        };
        let entry = GameEntry {
            platform: None,
            id: id.clone(),
            display_name,
            provider,
            executable,
            source_cab,
            install_dir: setup_install_dir(&files)
                .or_else(|| header.as_ref().and_then(|h| h.install_dir.clone()))
                .or_else(|| infer_install_dir(&files)),
            install_dirs: setup_install_dirs(&files),
            save_prefix: setup_save_dir(&files).or_else(|| {
                header.as_ref().and_then(|h| {
                    h.registry
                        .iter()
                        .find(|value| value.name.eq_ignore_ascii_case("SaveDir"))
                        .and_then(|value| value.string.clone())
                })
            }),
            registry,
            imported_at: now_unix_seconds(),
            settings: GameSettings {
                cpu_backend: self.config.default_cpu_backend,
                screen: guess_screen(
                    &exe_abs,
                    &files,
                    header.as_ref().and_then(|h| h.app_name.as_deref()),
                ),
                ..GameSettings::default()
            },
            icon: extract_icon_png(&game_dir, &exe_abs),
            companions: record_extracted_libraries(&extracted_dir, &game_dir),
        };

        self.commit_entry(&id, entry)
    }

    /// Import a standalone ARM PE32 `.exe` (or any extensionless PE)
    /// into the library. The file is copied verbatim into
    /// `<library_root>/games/<id>/extracted/<exe-basename>` so the
    /// rest of the launcher pipeline can treat it identically to a
    /// CAB-extracted game.
    ///
    /// Any guest DLLs sitting next to the executable are copied in as
    /// well. A fair number of titles split their artwork or engine into
    /// a satellite library and pull it in with `LoadLibraryW` at
    /// runtime — the CE shell's Solitaire keeps every card face in
    /// `pegcards.dll` — and the emulator resolves those by name in the
    /// executable's own directory, exactly as a real device would. Copy
    /// only the executable and the game loads but cannot draw.
    ///
    /// The user-supplied .exe is checked for an ARM machine type
    /// before being accepted; a mistakenly-imported x86 desktop
    /// build returns [`LibraryError::NotArmPe`] rather than silently
    /// crashing the emulator with "guest jumped to unmapped address"
    /// later on. A DLL handed in as the entry point is rejected with
    /// [`LibraryError::IsLibrary`]: it is a real file the game needs,
    /// but it has no entry point to start.
    pub fn import_exe(&mut self, exe_path: impl AsRef<Path>) -> Result<&GameEntry, LibraryError> {
        let exe_path = exe_path.as_ref();
        if is_nsis_installer(exe_path) {
            return self.import_nsis_installer(exe_path);
        }
        let source_name = exe_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.exe".to_string());
        let id = sanitize_id(exe_path.file_stem().map(|s| s.to_string_lossy()).as_deref());
        if id.is_empty() {
            return Err(LibraryError::InvalidId(source_name));
        }
        let Some(pe) = sniff_pe(exe_path).ok().flatten() else {
            return Err(LibraryError::NotArmPe(source_name));
        };
        if !pe.is_supported_guest() {
            return Err(LibraryError::NotArmPe(source_name));
        }
        if pe.is_dll() {
            return Err(LibraryError::IsLibrary(source_name));
        }

        let game_dir = self.root.join("games").join(&id);
        if game_dir.exists() {
            fs::remove_dir_all(&game_dir)?;
        }
        let extracted_dir = game_dir.join("extracted");
        fs::create_dir_all(&extracted_dir)?;
        let dest_exe = extracted_dir.join(&source_name);
        fs::copy(exe_path, &dest_exe)?;
        let companions = copy_sibling_libraries(exe_path, &extracted_dir, &game_dir);

        let executable = dest_exe
            .strip_prefix(&game_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| dest_exe.clone());

        let entry = GameEntry {
            platform: None,
            id: id.clone(),
            display_name: pretty_id(&id),
            provider: None,
            executable,
            source_cab: source_name,
            install_dir: None,
            install_dirs: Vec::new(),
            save_prefix: None,
            registry: Vec::new(),
            imported_at: now_unix_seconds(),
            settings: GameSettings {
                cpu_backend: self.config.default_cpu_backend,
                ..GameSettings::default()
            },
            icon: extract_icon_png(&game_dir, &dest_exe),
            companions,
        };

        self.commit_entry(&id, entry)
    }

    fn import_nsis_installer(&mut self, installer_path: &Path) -> Result<&GameEntry, LibraryError> {
        let source_name = installer_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "installer.exe".to_string());
        let data = fs::read(installer_path)?;
        let archive = newtua_nsis::NsisArchive::open(std::io::Cursor::new(data))
            .map_err(|error| LibraryError::NoPocketPcCab(format!("{source_name}: {error}")))?;
        let temp_root = self.root.join(".nsis-import");
        if temp_root.exists() {
            fs::remove_dir_all(&temp_root)?;
        }
        fs::create_dir_all(&temp_root)?;
        let mut cab_paths = Vec::new();
        for (index, entry) in archive.entries().iter().enumerate() {
            if entry.is_dir() {
                continue;
            }
            let path = entry.name();
            let name = path
                .rsplit(|byte| *byte == b'/' || *byte == b'\\')
                .next()
                .unwrap_or(path);
            let name = String::from_utf8_lossy(name).to_string();
            let Some(extension) = Path::new(&name).extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !extension.eq_ignore_ascii_case("cab") {
                continue;
            }
            let destination = temp_root.join(&name);
            let mut output = fs::File::create(&destination)?;
            archive
                .read_entry(index, &mut output)
                .map_err(|error| LibraryError::NoPocketPcCab(format!("{source_name}: {error}")))?;
            cab_paths.push(destination);
        }
        let Some(main_cab) = cab_paths.first().cloned() else {
            let _ = fs::remove_dir_all(&temp_root);
            return Err(LibraryError::NoPocketPcCab(source_name));
        };
        let game_id = match self.import_cab(&main_cab) {
            Ok(entry) => entry.id.clone(),
            Err(error) => {
                let _ = fs::remove_dir_all(&temp_root);
                return Err(error);
            }
        };
        let game_dir = self.root.join("games").join(&game_id);
        let extracted_dir = game_dir.join("extracted");
        let source_family = main_cab
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        for (index, companion) in cab_paths.iter().skip(1).enumerate() {
            if companion
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_ascii_lowercase)
                .is_some_and(|stem| stem.starts_with(&source_family))
            {
                import_resource_companion_cab(&game_dir, &extracted_dir, index, companion);
            }
        }
        let _ = fs::remove_dir_all(&temp_root);
        self.library
            .games
            .iter()
            .find(|entry| entry.id == game_id)
            .ok_or(LibraryError::NotFound(game_id))
    }

    /// Import a `.zip` archive (typically a community repack of a
    /// PocketPC game) into the library.
    ///
    /// Every entry is extracted into
    /// `<library_root>/games/<id>/extracted/`. If the zip turns out
    /// to contain a nested `.cab`, we transparently recurse via
    /// [`Library::import_cab`] on the extracted CAB so the user gets
    /// the proper `app_name` / `provider` from the cabinet header
    /// rather than a stem-derived placeholder. A demo card with sibling
    /// `autorun.exe` and `multiboot.hog` keeps its launcher and full tree;
    /// otherwise the largest guest executable is picked as the entry point.
    /// Compatibility entry point: a GZGA collection imports every game and
    /// returns its first entry. Frontends use import_zip_entries for the count.
    pub fn import_zip(&mut self, zip_path: impl AsRef<Path>) -> Result<&GameEntry, LibraryError> {
        let entries = self.import_zip_entries(zip_path)?;
        let id = &entries.first().ok_or(LibraryError::NoExecutable)?.id;
        self.get(id).ok_or(LibraryError::NoExecutable)
    }

    /// Root launcher/HOG pairs are demo cards; otherwise import all GZGA games.
    /// Real demo cards retain the existing keep-launcher/split choice.
    pub fn import_zip_entries(
        &mut self,
        zip_path: impl AsRef<Path>,
    ) -> Result<Vec<GameEntry>, LibraryError> {
        self.import_zip_single(zip_path)
    }

    fn import_zip_single(
        &mut self,
        zip_path: impl AsRef<Path>,
    ) -> Result<Vec<GameEntry>, LibraryError> {
        let zip_path = zip_path.as_ref();
        let source_name = zip_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.zip".to_string());
        let id = sanitize_id(zip_path.file_stem().map(|s| s.to_string_lossy()).as_deref());
        if id.is_empty() {
            return Err(LibraryError::InvalidId(source_name));
        }

        // Classify before publishing any library entry. Own game trees move
        // into their final extracted directory; no archive-named placeholder.
        let staging = tempfile::Builder::new()
            .prefix(".gzga-import-")
            .tempdir_in(self.root.join("games"))?;
        let game_dir = staging.path().join("single");
        let extracted_dir = game_dir.join("extracted");
        fs::create_dir_all(&extracted_dir)?;

        // Extract every entry next to the would-be executable.
        let f = fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(f)?;
        let mut written: Vec<PathBuf> = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let Some(rel) = entry.enclosed_name().map(Path::to_path_buf) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let dest = extracted_dir.join(&rel);
            if entry.is_dir() {
                fs::create_dir_all(&dest)?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = fs::File::create(&dest)?;
            std::io::copy(&mut entry, &mut out)?;
            written.push(dest);
        }
        if written.is_empty() {
            return Err(LibraryError::NoExecutable);
        }
        // Strip only sole enclosing folders; retain a GZGA boundary so its
        // identity and the demo-card/GZGA priority are not lost.
        let mut content_root = extracted_dir.clone();
        loop {
            let children: Vec<_> = fs::read_dir(&content_root)?.collect::<Result<_, _>>()?;
            if children.len() != 1 || !children[0].file_type()?.is_dir() {
                break;
            }
            let child = children[0].path();
            let name = children[0].file_name().to_string_lossy().to_string();
            if is_gizmondo_title_id(&name) {
                break;
            }
            content_root = child;
        }
        if content_root != extracted_dir {
            let unwrapped = staging.path().join("unwrapped");
            fs::rename(&content_root, &unwrapped)?;
            fs::remove_dir_all(&extracted_dir)?;
            fs::rename(unwrapped, &extracted_dir)?;
            written = written
                .into_iter()
                .map(|path| extracted_dir.join(path.strip_prefix(&content_root).unwrap()))
                .collect();
        }

        let demo_launcher = demo_card_launcher(&written, &extracted_dir);

        // ZIPs that are really just installer wrappers around a CAB —
        // recurse so we get the proper PocketPC display metadata
        // instead of a stem-derived placeholder.
        let nested_cabs: Vec<PathBuf> = written
            .iter()
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("cab"))
            })
            .cloned()
            .collect();
        if demo_launcher.is_none()
            && gzga_collection_root(&written, &extracted_dir).is_none()
            && !nested_cabs.is_empty()
        {
            let main_cab = nested_cabs
                .iter()
                .find(|path| {
                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    !stem.contains("res") && !stem.contains("resource")
                })
                .cloned()
                .unwrap_or_else(|| nested_cabs[0].clone());
            log::info!(
                "zip {} contains nested cabinets; importing {} with its companion cabinets",
                zip_path.display(),
                main_cab.display(),
            );
            let nested_dir = self.root.join("games").join(format!(".{id}-nested-cabs"));
            fs::create_dir_all(&nested_dir)?;
            let staged_main =
                nested_dir.join(main_cab.file_name().ok_or(LibraryError::NoExecutable)?);
            for cab in &nested_cabs {
                let name = cab.file_name().ok_or(LibraryError::NoExecutable)?;
                fs::copy(cab, nested_dir.join(name))?;
            }
            let result = self
                .import_cab(&staged_main)
                .map(|entry| vec![entry.clone()]);
            let _ = fs::remove_dir_all(&nested_dir);
            return result;
        }

        // Find the largest ARM PE inside the extraction. Satellite
        // libraries are skipped: a resource-only DLL can easily be the
        // largest PE in the archive — it is where the artwork lives —
        // and it has no entry point to start.
        let mut best: Option<(PathBuf, u64)> = None;
        for path in &written {
            let Ok(meta) = fs::metadata(path) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            if !is_guest_exe(path) {
                continue;
            }
            if best.as_ref().map(|(_, sz)| *sz).unwrap_or(0) < meta.len() {
                best = Some((path.clone(), meta.len()));
            }
        }
        let is_demo_card = demo_launcher.is_some();
        let exe_abs = if let Some(launcher) = demo_launcher {
            log::info!("demo card launcher selected: {}", launcher.display());
            launcher
        } else {
            best.ok_or(LibraryError::NoExecutable)?.0
        };
        let executable = exe_abs
            .strip_prefix(&game_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| exe_abs.clone());

        let display_name = if is_demo_card {
            "Demo Card".to_string()
        } else {
            pretty_id(&id)
        };
        let entry = GameEntry {
            platform: None,
            id: id.clone(),
            display_name,
            provider: None,
            executable,
            source_cab: source_name.clone(),
            install_dir: None,
            install_dirs: Vec::new(),
            save_prefix: None,
            registry: Vec::new(),
            imported_at: now_unix_seconds(),
            settings: GameSettings {
                cpu_backend: self.config.default_cpu_backend,
                ..GameSettings::default()
            },
            icon: extract_icon_png(&game_dir, &exe_abs),
            companions: record_extracted_libraries(&extracted_dir, &game_dir),
        };

        if !is_demo_card && gzga_collection_root(&written, &extracted_dir).is_some() {
            return self.import_game_trees(staging, entry, &extracted_dir, &written, None);
        }
        let mut entry = entry;
        if !is_demo_card {
            if let Some(code) = detect_gizmondo_title_id(&extracted_dir).or_else(|| {
                // A flat card ZIP may carry its only explicit title ID in the
                // archive name (Android normalizes that name to lowercase).
                is_gizmondo_title_id(&id).then(|| id.to_ascii_uppercase())
            }) {
                entry.id = code;
                entry.display_name = gizmondo_titles::lookup(&entry.id)
                    .unwrap_or(&entry.id)
                    .to_owned();
            }
        }
        let previous = self.previous_import(&entry.id, &source_name)?;
        self.preserve_import_preferences(&mut entry, previous.as_ref(), &game_dir)?;
        write_json(&game_dir.join("game.json"), &entry)?;
        self.publish_staged_imports(staging, vec![(entry, game_dir, previous)], None)
    }

    fn previous_import(
        &self,
        id: &str,
        source_name: &str,
    ) -> Result<Option<GameEntry>, LibraryError> {
        let matches: Vec<_> = self
            .library
            .games
            .iter()
            .filter(|game| {
                game.id.eq_ignore_ascii_case(id)
                    || game
                        .source_cab
                        .strip_prefix("gzga-collection:")
                        .is_some_and(|source| source.eq_ignore_ascii_case(source_name))
                        && game_gizmondo_title_id(game, &self.root)
                            .is_some_and(|code| code.eq_ignore_ascii_case(id))
                    || game.source_cab.eq_ignore_ascii_case(source_name)
                        && !self.is_demo_card_entry(game)
                        && game_gizmondo_title_id(game, &self.root)
                            .is_some_and(|code| code.eq_ignore_ascii_case(id))
                        && !game
                            .executable_path(&self.root)
                            .parent()
                            .is_some_and(|parent| {
                                fs::read_dir(parent).is_ok_and(|files| {
                                    files.flatten().any(|file| {
                                        file.file_name()
                                            .to_string_lossy()
                                            .eq_ignore_ascii_case("multiboot.hog")
                                    })
                                })
                            })
            })
            .cloned()
            .collect();
        if matches.len() > 1 {
            return Err(LibraryError::InvalidId(format!(
                "multiple existing imports for {id}"
            )));
        }
        Ok(matches.into_iter().next())
    }

    fn preserve_import_preferences(
        &self,
        entry: &mut GameEntry,
        previous: Option<&GameEntry>,
        staged: &Path,
    ) -> Result<(), LibraryError> {
        if let Some(previous) = previous {
            entry.settings = previous.settings.clone();
            if previous.platform.is_some() {
                entry.platform = previous.platform;
            }
            entry.display_name = previous.display_name.clone();
            let saves = previous.save_dir(&self.root);
            if saves.is_dir() {
                for file in import_tree_files(&saves)? {
                    let dest = staged
                        .join(save_data::SAVE_DIR_NAME)
                        .join(file.strip_prefix(&saves).unwrap());
                    fs::create_dir_all(dest.parent().unwrap())?;
                    fs::copy(file, dest)?;
                }
            }
        }
        let gizmondo = entry.source_cab.starts_with("gzga-collection:")
            || is_gizmondo_title_id(&entry.id)
            || has_gizmondo_marker(&staged.join("extracted"));
        if gizmondo {
            if let Some(icon) = cartridge_artwork(staged, &staged.join(&entry.executable)) {
                entry.icon = Some(icon);
            }
        } else if previous.is_none()
            && entry.settings.rotation == RotationPref::None
            && entry.executable.file_stem().is_some_and(|name| {
                name.to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("cod2")
            })
        {
            entry.settings.rotation = RotationPref::Cw90;
        }
        Ok(())
    }

    /// Move each GZGA's contents into its ordinary extracted directory. The
    /// ZIP's outer folders never become part of the installed game's path.
    fn import_game_trees(
        &mut self,
        staging: tempfile::TempDir,
        source: GameEntry,
        extracted: &Path,
        files: &[PathBuf],
        remove_card: Option<GameEntry>,
    ) -> Result<Vec<GameEntry>, LibraryError> {
        let mut games = std::collections::BTreeMap::new();
        for exe in files.iter().filter(|p| is_guest_exe(p)) {
            let directory = if remove_card.is_some() {
                if exe.parent() == Some(extracted) {
                    continue;
                }
                let Some(first) = exe
                    .strip_prefix(extracted)
                    .ok()
                    .and_then(|relative| relative.components().next())
                else {
                    continue;
                };
                extracted.join(first.as_os_str())
            } else {
                let Some(directory) = gzga_game_directory(exe, extracted) else {
                    continue;
                };
                directory
            };
            let preferred = exe
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("autorun.exe"));
            if preferred || !games.contains_key(&directory) {
                games.insert(directory, exe.clone());
            }
        }
        if games.is_empty() {
            return Err(LibraryError::NoExecutable);
        }
        let shared: Vec<_> = files
            .iter()
            .filter(|file| {
                // A collection is a set of independent card directories, not
                // a shared installation. Only demo-card splits copy shared files.
                remove_card.is_some()
                    && gzga_game_directory(file, extracted).is_none()
                    && !is_guest_exe(file)
                    && !games.keys().any(|directory| file.starts_with(directory))
            })
            .filter(|file| {
                !file
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("multiboot.hog"))
            })
            .cloned()
            .collect();
        let mut ids = std::collections::BTreeSet::new();
        for directory in games.keys() {
            let folder = directory.file_name().unwrap().to_string_lossy();
            let id = if is_gizmondo_title_id(&folder) {
                folder.to_ascii_uppercase()
            } else {
                sanitize_id(Some(&folder))
            };
            if !ids.insert(id.clone()) {
                return Err(LibraryError::InvalidId(format!(
                    "duplicate GZGA directory {id}"
                )));
            }
        }
        let mut prepared = Vec::new();
        for (directory, exe) in games {
            let folder = directory.file_name().unwrap().to_string_lossy();
            let id = if is_gizmondo_title_id(&folder) {
                folder.to_ascii_uppercase()
            } else {
                sanitize_id(Some(&folder))
            };
            let previous = self.previous_import(&id, &source.source_cab)?;
            let destination = staging.path().join("entries").join(&id);
            fs::create_dir_all(&destination)?;
            let relative_exe = exe.strip_prefix(&directory).unwrap().to_path_buf();
            fs::rename(&directory, destination.join("extracted"))?;
            for file in &shared {
                let Ok(relative) = file.strip_prefix(directory.parent().unwrap()) else {
                    continue;
                };
                let dest = destination.join("extracted").join(relative);
                if !dest.exists() {
                    fs::create_dir_all(dest.parent().unwrap())?;
                    fs::copy(file, dest)?;
                }
            }
            let mut entry = source.clone();
            entry.id = id.clone();
            entry.display_name = gizmondo_titles::lookup(&id).unwrap_or(&id).to_owned();
            entry.source_cab = format!(
                "{}:{}",
                if is_gizmondo_title_id(&id) {
                    "gzga-collection"
                } else {
                    "demo-card"
                },
                source.source_cab
            );
            entry.executable = PathBuf::from("extracted").join(relative_exe);
            // Keep SDK absolute serial paths working after removing the host
            // GZGA wrapper. These are existing frontend mount aliases.
            entry.install_dirs = vec![
                format!("\\SD Card\\{folder}\\"),
                format!("\\Storage Card\\{folder}\\"),
            ];
            entry.icon = extract_icon_png(&destination, &destination.join(&entry.executable));
            entry.companions =
                record_extracted_libraries(&destination.join("extracted"), &destination);
            self.preserve_import_preferences(&mut entry, previous.as_ref(), &destination)?;
            write_json(&destination.join("game.json"), &entry)?;
            prepared.push((entry, destination, previous));
        }
        self.publish_staged_imports(staging, prepared, remove_card)
    }

    /// Publish one catalog update only after all game files are ready. Existing
    /// installations stay recoverable until that update succeeds.
    fn publish_staged_imports(
        &mut self,
        staging: tempfile::TempDir,
        prepared: Vec<(GameEntry, PathBuf, Option<GameEntry>)>,
        remove_card: Option<GameEntry>,
    ) -> Result<Vec<GameEntry>, LibraryError> {
        for (entry, _, previous) in &prepared {
            let destination = self.root.join(entry.relative_dir());
            if destination.exists()
                && !previous
                    .as_ref()
                    .is_some_and(|old| old.id.eq_ignore_ascii_case(&entry.id))
            {
                return Err(LibraryError::InvalidId(format!(
                    "destination already exists: {}",
                    destination.display()
                )));
            }
        }
        let mut next = self.library.clone();
        let mut backups = Vec::new();
        let mut installed = Vec::new();
        let result = (|| -> Result<(), LibraryError> {
            for (index, (entry, directory, previous)) in prepared.iter().enumerate() {
                if let Some(previous) = previous {
                    let original = self.root.join(previous.relative_dir());
                    if original.exists() {
                        let backup = staging.path().join(format!("backup-{index}"));
                        fs::rename(&original, &backup)?;
                        backups.push((original, backup));
                    }
                    next.games.retain(|game| game.id != previous.id);
                }
                let destination = self.root.join(entry.relative_dir());
                fs::rename(directory, &destination)?;
                installed.push(destination);
                next.games.retain(|game| game.id != entry.id);
                next.games.push(entry.clone());
            }
            if let Some(card) = &remove_card {
                let original = self.root.join(card.relative_dir());
                let backup = staging.path().join("backup-card");
                fs::rename(&original, &backup)?;
                backups.push((original, backup));
                next.games.retain(|game| game.id != card.id);
            }
            next.games
                .sort_by(|a, b| a.display_name.cmp(&b.display_name));
            write_json(&self.root.join("library.json"), &next)?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut restored = true;
            for directory in installed.iter().rev() {
                restored &= fs::remove_dir_all(directory).is_ok();
            }
            for (original, backup) in backups.iter().rev() {
                restored &= fs::rename(backup, original).is_ok();
            }
            if !restored {
                let recovery = staging.keep();
                log::error!(
                    "import rollback incomplete; retained recovery files at {}",
                    recovery.display()
                );
            }
            return Err(error);
        }
        self.library = next;
        Ok(prepared.into_iter().map(|(entry, _, _)| entry).collect())
    }

    /// Import a RAR archive containing a Pocket PC game.
    ///
    /// RAR extraction is kept in the shared library so Android, desktop,
    /// and CLI imports use the same behavior. The archive is unpacked into
    /// the normal per-game directory, then the largest ARM PE executable is
    /// selected just like for ZIP imports.
    pub fn import_rar(&mut self, rar_path: impl AsRef<Path>) -> Result<&GameEntry, LibraryError> {
        let rar_path = rar_path.as_ref();
        let source_name = rar_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.rar".to_string());
        let id = sanitize_id(rar_path.file_stem().map(|s| s.to_string_lossy()).as_deref());
        if id.is_empty() {
            return Err(LibraryError::InvalidId(source_name));
        }

        let game_dir = self.root.join("games").join(&id);
        if game_dir.exists() {
            fs::remove_dir_all(&game_dir)?;
        }
        let extracted_dir = game_dir.join("extracted");
        fs::create_dir_all(&extracted_dir)?;

        let archive = ArchiveReader::read_path(rar_path)
            .map_err(|error| LibraryError::Rar(error.to_string()))?;
        let mut written = Vec::new();
        archive
            .extract_to(None, |meta| {
                let Some(relative) = safe_archive_path(meta.name_bytes()) else {
                    return Err(rars::Error::UnsupportedSignature);
                };
                if relative.as_os_str().is_empty() {
                    return Ok(Box::new(std::io::sink()));
                }
                let destination = extracted_dir.join(relative);
                if meta.is_directory {
                    fs::create_dir_all(&destination)?;
                    return Ok(Box::new(std::io::sink()));
                }
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)?;
                }
                written.push(destination.clone());
                Ok(Box::new(fs::File::create(destination)?))
            })
            .map_err(|error| LibraryError::Rar(error.to_string()))?;

        if written.is_empty() {
            return Err(LibraryError::NoExecutable);
        }
        let (exe_abs, _) = written
            .iter()
            .filter(|path| is_guest_exe(path))
            .filter_map(|path| {
                fs::metadata(path)
                    .ok()
                    .map(|meta| (path.clone(), meta.len()))
            })
            .max_by_key(|(_, size)| *size)
            .ok_or(LibraryError::NoExecutable)?;
        let executable = exe_abs
            .strip_prefix(&game_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| exe_abs.clone());
        let entry = GameEntry {
            platform: None,
            id: id.clone(),
            display_name: pretty_id(&id),
            provider: None,
            executable,
            source_cab: source_name,
            install_dir: None,
            install_dirs: Vec::new(),
            save_prefix: None,
            registry: Vec::new(),
            imported_at: now_unix_seconds(),
            settings: GameSettings {
                cpu_backend: self.config.default_cpu_backend,
                ..GameSettings::default()
            },
            icon: extract_icon_png(&game_dir, &exe_abs),
            companions: record_extracted_libraries(&extracted_dir, &game_dir),
        };

        self.commit_entry(&id, entry)
    }

    /// Import whatever `path` points at, picking the right importer
    /// from its extension: `.cab`, `.zip`, or otherwise a raw ARM
    /// PE32 executable. This is the entry point the launcher UI and
    /// the CLI's `import` subcommand both use, so a game imported from
    /// a script behaves exactly like one added through the GUI.
    pub fn import_any(&mut self, path: impl AsRef<Path>) -> Result<&GameEntry, LibraryError> {
        let path = path.as_ref();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("cab") => self.import_cab(path),
            Some("zip") => self.import_zip(path),
            Some("rar") => self.import_rar(path),
            _ => self.import_exe(path),
        }
    }

    /// Identify an already imported card before asking how it should appear.
    pub fn is_demo_card(&self, id: &str) -> bool {
        self.get(id)
            .is_some_and(|entry| self.is_demo_card_entry(entry))
    }

    /// Separate titles into independent directories. Keep shared root resources
    /// and original SD-card paths, but exclude the other games' directory trees.
    pub fn split_demo_card(&mut self, id: &str) -> Result<Vec<GameEntry>, LibraryError> {
        let source = self
            .get(id)
            .cloned()
            .ok_or_else(|| LibraryError::NotFound(id.into()))?;
        if !self.is_demo_card_entry(&source) {
            return Err(LibraryError::NoExecutable);
        }
        let staging = tempfile::Builder::new()
            .prefix(".gzga-import-")
            .tempdir_in(self.root.join("games"))?;
        let extracted = staging.path().join("card");
        fs::create_dir_all(&extracted)?;
        let card_root = source.extracted_dir(&self.root);
        let mut files = Vec::new();
        for file in import_tree_files(&card_root)? {
            let dest = extracted.join(file.strip_prefix(&card_root).unwrap());
            fs::create_dir_all(dest.parent().unwrap())?;
            fs::copy(file, &dest)?;
            files.push(dest);
        }
        self.import_game_trees(staging, source.clone(), &extracted, &files, Some(source))
    }

    fn is_demo_card_entry(&self, entry: &GameEntry) -> bool {
        if entry.source_cab.starts_with("gzga-collection:")
            || entry.source_cab.starts_with("demo-card:")
        {
            return false;
        }
        let exe = entry.executable_path(&self.root);
        if !exe
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("autorun.exe"))
            || exe.parent() != Some(entry.extracted_dir(&self.root).as_path())
        {
            return false;
        }
        exe.parent()
            .and_then(|p| fs::read_dir(p).ok())
            .is_some_and(|files| {
                files.flatten().any(|f| {
                    f.file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case("multiboot.hog")
                        && f.file_type().is_ok_and(|kind| kind.is_file())
                })
            })
    }

    /// Persist `entry` and return a stable reference. Replaces any
    /// existing game with the same id.
    fn commit_entry(&mut self, id: &str, mut entry: GameEntry) -> Result<&GameEntry, LibraryError> {
        let game_dir = self.root.join("games").join(id);
        if !is_gizmondo_game(&entry, &self.root)
            && entry.executable.file_stem().is_some_and(|name| {
                name.to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("cod2")
            })
            && entry.settings.rotation == RotationPref::None
        {
            entry.settings.rotation = RotationPref::Cw90;
        }
        if is_gizmondo_game(&entry, &self.root) && !self.is_demo_card_entry(&entry) {
            if let Some(icon) = cartridge_artwork(&game_dir, &entry.executable_path(&self.root)) {
                entry.icon = Some(icon);
            }
        }
        // A known static title is the suggestion; BMP artwork remains intact.
        if is_gizmondo_game(&entry, &self.root) && !self.is_demo_card_entry(&entry) {
            if let Some(title) = game_gizmondo_title_id(&entry, &self.root)
                .and_then(|id| gizmondo_titles::lookup(&id))
            {
                entry.display_name = title.to_owned();
            }
        }
        // Persist a per-game manifest so the game directory is
        // self-describing.
        write_json(&game_dir.join("game.json"), &entry)?;

        // Replace any existing entry with the same id.
        self.library.games.retain(|g| g.id != id);
        self.library.games.push(entry);
        self.library
            .games
            .sort_by(|a, b| a.display_name.cmp(&b.display_name));
        self.save()?;

        // Return a stable reference.
        Ok(self
            .library
            .games
            .iter()
            .find(|g| g.id == id)
            .expect("just inserted"))
    }

    /// Remove a game and its on-disk files.
    pub fn remove(&mut self, id: &str) -> Result<(), LibraryError> {
        let game_dir = self.root.join("games").join(id);
        if game_dir.exists() {
            fs::remove_dir_all(&game_dir)?;
        }
        self.library.games.retain(|g| g.id != id);
        self.save()
    }

    /// Change only the display name, preserving the stable id and all paths.
    /// Commit both manifests before publishing the new in-memory entry.
    pub fn rename_game(&mut self, id: &str, name: &str) -> Result<(), LibraryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::InvalidName);
        }
        let original = self
            .get(id)
            .ok_or_else(|| LibraryError::NotFound(id.into()))?
            .clone();
        let mut next = self.library.clone();
        let game = next.games.iter_mut().find(|g| g.id == id).unwrap();
        game.display_name = name.into();
        let renamed = game.clone();
        next.games
            .sort_by(|a, b| a.display_name.cmp(&b.display_name));
        let manifest = self.root.join("games").join(id).join("game.json");
        write_json(&manifest, &renamed)?;
        if let Err(error) = write_json(&self.root.join("library.json"), &next) {
            let _ = write_json(&manifest, &original);
            return Err(error);
        }
        self.library = next;
        Ok(())
    }

    /// Update the per-game settings and save the library.
    pub fn update_settings(
        &mut self,
        id: &str,
        settings: GameSettings,
    ) -> Result<(), LibraryError> {
        let game = self
            .get_mut(id)
            .ok_or_else(|| LibraryError::NotFound(id.to_string()))?;
        game.settings = settings;
        let cloned = game.clone();
        let game_dir = self.root.join("games").join(id);
        write_json(&game_dir.join("game.json"), &cloned)?;
        self.save()
    }
}

/// Number of satellite libraries we are willing to pull in next to a
/// hand-picked `.exe`. A game ships a handful; a user who points the
/// importer at a system directory should not have it copied wholesale.
const MAX_SIBLING_LIBRARIES: usize = 16;

/// Copy the guest support libraries sitting next to `exe_path` into
/// `dest_dir`, so `LoadLibraryW` finds them at run time.
///
/// Returns the copied files as paths relative to `game_dir`, for
/// [`GameEntry::companions`].
///
/// The filter is deliberately narrow: only ARM/MIPS PE images with
/// `IMAGE_FILE_DLL` set, only in the executable's own directory, never
/// recursing. That is enough for the satellite-DLL layout games
/// actually ship (`solitare.exe` + `pegcards.dll` in one folder) while
/// keeping an import from a busy Downloads folder from dragging in
/// unrelated binaries. Host x86 DLLs are skipped because they fail the
/// machine check.
fn copy_sibling_libraries(exe_path: &Path, dest_dir: &Path, game_dir: &Path) -> Vec<PathBuf> {
    let Some(src_dir) = exe_path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(src_dir) else {
        return Vec::new();
    };
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p != exe_path)
        .filter(|p| p.is_file())
        .filter(|p| is_guest_dll(p))
        .collect();
    // Stable order so a re-import produces the same manifest.
    candidates.sort();

    let mut copied = Vec::new();
    for src in candidates.iter().take(MAX_SIBLING_LIBRARIES) {
        let Some(name) = src.file_name() else {
            continue;
        };
        let dest = dest_dir.join(name);
        if let Err(e) = fs::copy(src, &dest) {
            log::warn!("could not copy support library {}: {e}", src.display());
            continue;
        }
        log::info!("imported support library {}", src.display());
        copied.push(
            dest.strip_prefix(game_dir)
                .map(Path::to_path_buf)
                .unwrap_or(dest),
        );
    }
    if candidates.len() > MAX_SIBLING_LIBRARIES {
        log::warn!(
            "{} has {} sibling DLLs; imported the first {MAX_SIBLING_LIBRARIES}",
            src_dir.display(),
            candidates.len(),
        );
    }
    copied
}

/// List the guest DLLs already sitting in a CAB/ZIP extraction, as
/// paths relative to `game_dir`.
///
/// Unlike [`copy_sibling_libraries`] this copies nothing: an archive
/// import has already written every payload file into `extracted_dir`,
/// so the libraries are where the emulator will look for them. We only
/// record them for [`GameEntry::companions`].
fn record_extracted_libraries(extracted_dir: &Path, game_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(extracted_dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_guest_dll(p))
        .filter_map(|p| p.strip_prefix(game_dir).map(Path::to_path_buf).ok())
        .collect();
    found.sort();
    found
}

fn import_resource_companion_cab(
    game_dir: &Path,
    extracted_dir: &Path,
    index: usize,
    companion: &Path,
) {
    let staging = game_dir.join(format!(".resource-cab-{index}"));
    if fs::create_dir_all(&staging).is_err() {
        return;
    }
    match pocket_cab::extract_for_install(companion, &staging) {
        Ok((companion_files, companion_header)) => {
            if let Some(header) = companion_header.as_ref().filter(|h| h.structured) {
                pocket_cab::materialise_install_header_names(
                    extracted_dir,
                    &companion_files,
                    header,
                );
            } else {
                materialise_legacy_assets(
                    extracted_dir,
                    &companion_files,
                    companion_header
                        .as_ref()
                        .and_then(|h| h.app_name.as_deref()),
                );
            }
            log::info!(
                "imported companion resource cabinet {}",
                companion.display()
            );
        }
        Err(error) => {
            log::warn!(
                "could not import companion resource cabinet {}: {error}",
                companion.display()
            );
        }
    }
    let _ = fs::remove_dir_all(staging);
}

fn find_resource_companion_cabs(source: &Path) -> Vec<PathBuf> {
    let Some(parent) = source.parent() else {
        return Vec::new();
    };
    let source_stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let family = source_stem.split(['-', '_']).next().unwrap_or(&source_stem);
    let mut matches: Vec<PathBuf> = fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path != source)
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("cab"))
        })
        .filter(|path| {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            stem.split(['-', '_'])
                .next()
                .is_some_and(|candidate| candidate == family)
                && (stem.contains("res") || stem.contains("resource"))
        })
        .collect();
    matches.sort();
    matches
}

fn materialise_legacy_assets(root: &Path, files: &[pocket_cab::CabFile], app_name: Option<&str>) {
    let by_short: std::collections::HashMap<String, &Path> = files
        .iter()
        .map(|f| {
            (
                f.short_name.to_ascii_uppercase(),
                f.extracted_path.as_path(),
            )
        })
        .collect();

    let known_names = [
        ("ATOMIC~3.001", "AtomicDreams.exe"),
        ("ATOMIC~1.002", "AtomicDreams.pak"),
    ];
    let mut copied_known = false;
    for (short, long) in known_names {
        if let Some(src) = by_short.get(short) {
            let _ = fs::copy(src, root.join(long));
            copied_known = true;
        }
    }
    if copied_known {
        return;
    }

    let arm = files
        .iter()
        .filter(|f| is_arm_pe(&f.extracted_path).unwrap_or(false))
        .max_by_key(|f| f.size);
    let data = files
        .iter()
        .filter(|f| !is_arm_pe(&f.extracted_path).unwrap_or(false))
        .filter(|f| !f.short_name.to_ascii_lowercase().ends_with(".000"))
        .max_by_key(|f| f.size);
    let Some(arm) = arm else { return };
    let stem: String = app_name
        .unwrap_or("Game")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let stem = if stem.is_empty() { "Game" } else { &stem };
    let _ = fs::copy(&arm.extracted_path, root.join(format!("{stem}.exe")));
    if let Some(data) = data {
        let _ = fs::copy(&data.extracted_path, root.join(format!("{stem}.pak")));
    }
}

fn materialise_legacy_install_files(
    root: &Path,
    files: &[pocket_cab::CabFile],
    header: Option<&pocket_cab::WinCeInstallHeader>,
) {
    let Some(header) = header else { return };
    let install_dir = header.install_dir.as_deref().unwrap_or("");
    for entry in &header.files {
        let Some(source_id) = entry.source.rsplit('.').next() else {
            continue;
        };
        let Some(src) = files.iter().find(|file| {
            file.short_name
                .rsplit('.')
                .next()
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(source_id))
        }) else {
            continue;
        };
        let destination_lower = entry.destination.to_ascii_lowercase();
        let install_lower = install_dir.to_ascii_lowercase();
        let relative = if destination_lower.starts_with(&install_lower) {
            &entry.destination[install_dir.len()..]
        } else {
            &entry.destination
        };
        let relative = relative.trim_start_matches(['\\', '/']);
        if relative.is_empty() || relative.split(['\\', '/']).any(|part| part == "..") {
            continue;
        }
        let dest = relative
            .split(['\\', '/'])
            .filter(|part| !part.is_empty())
            .fold(root.to_path_buf(), |acc, part| acc.join(part));
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if dest != src.extracted_path {
            let _ = fs::copy(&src.extracted_path, &dest);
        }
    }
}

/// Install directory the cabinet's `_setup.xml` declares.
///
/// `_setup.xml` is the authoritative source: it is the script a real
/// installer follows, so it names the directory the game's own code was
/// compiled to read from. The `.000` install header is only a fallback
/// for legacy cabinets that ship no script, and guessing from the
/// executable's file name is a last resort.
///
/// `install_dirs` (every directory the `FileOperation` block writes
/// into) is preferred over the declared `InstallDir` because the two
/// frequently disagree: Gameloft's Sonic Unleashed declares
/// `%CE1%\SONIC` but installs into `%CE1%\Gameloft\SONIC`, which is
/// the path the game hard-codes. This mirrors how `pockethle run <cab>`
/// resolves the same cabinet, so a title behaves identically whether it
/// is launched from a file or from the library.
fn setup_save_dir(files: &[pocket_cab::CabFile]) -> Option<String> {
    let setup = files
        .iter()
        .find(|f| f.short_name.eq_ignore_ascii_case("_setup.xml"))?;
    let data = fs::read(&setup.extracted_path).ok()?;
    pocket_cab::WinCeSetupScript::parse_bytes(&data)
        .registry
        .into_iter()
        .find(|value| value.name.eq_ignore_ascii_case("SaveDir"))
        .and_then(|value| value.string)
        .map(|value| {
            let mut path = value.replace('/', "\\");
            if !path.starts_with('\\') {
                path.insert(0, '\\');
            }
            if !path.ends_with('\\') {
                path.push('\\');
            }
            path
        })
}

fn setup_registry(files: &[pocket_cab::CabFile]) -> Vec<pocket_cab::SetupRegistryValue> {
    let Some(setup) = files
        .iter()
        .find(|f| f.short_name.eq_ignore_ascii_case("_setup.xml"))
    else {
        return Vec::new();
    };
    fs::read(&setup.extracted_path)
        .ok()
        .map(|data| pocket_cab::WinCeSetupScript::parse_bytes(&data).registry)
        .unwrap_or_default()
}

fn setup_install_dirs(files: &[pocket_cab::CabFile]) -> Vec<String> {
    let Some(setup) = files
        .iter()
        .find(|f| f.short_name.eq_ignore_ascii_case("_setup.xml"))
    else {
        return Vec::new();
    };
    let Ok(data) = fs::read(&setup.extracted_path) else {
        return Vec::new();
    };
    pocket_cab::WinCeSetupScript::parse_bytes(&data).install_dirs
}

fn setup_install_dir(files: &[pocket_cab::CabFile]) -> Option<String> {
    let setup = files
        .iter()
        .find(|f| f.short_name.eq_ignore_ascii_case("_setup.xml"))?;
    let data = fs::read(&setup.extracted_path).ok()?;
    // `install_root` strips the trailing separator; the manifest stores
    // the directory form every other guest path is built against.
    let root = pocket_cab::WinCeSetupScript::parse_bytes(&data).install_root()?;
    Some(format!("{root}\\"))
}

fn infer_install_dir(files: &[pocket_cab::CabFile]) -> Option<String> {
    let has_asphalt_exe = files.iter().any(|file| {
        file.short_name.eq_ignore_ascii_case("ASPHAL~1.001")
            && is_arm_pe(&file.extracted_path).unwrap_or(false)
    });
    has_asphalt_exe.then(|| "\\Program Files\\Asphalt 2 3D\\".to_string())
}

fn read_or_default<T>(path: &Path) -> Result<T, LibraryError>
where
    T: Default + serde::de::DeserializeOwned,
{
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<T>(&bytes) {
            Ok(v) => Ok(v),
            Err(e) => {
                log::warn!(
                    "could not parse {}: {e}; falling back to default",
                    path.display()
                );
                Ok(T::default())
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(LibraryError::Io(e)),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), LibraryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    let temp = path.with_file_name(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("manifest.json")
    ));
    fs::write(&temp, bytes)?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

fn safe_archive_path(raw: &[u8]) -> Option<PathBuf> {
    let name = String::from_utf8_lossy(raw);
    let mut path = PathBuf::new();
    for component in name.replace('\\', "/").split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." || component.contains(':') {
            return None;
        }
        path.push(component);
    }
    Some(path)
}

fn sanitize_id(stem: Option<&str>) -> String {
    let raw = stem.unwrap_or("game");
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('.').trim_matches('_').trim_matches('-');
    if trimmed.is_empty() {
        "game".to_string()
    } else {
        trimmed.to_string()
    }
}

fn pretty_id(id: &str) -> String {
    let cleaned = id.replace(['_', '-', '.'], " ");
    let mut out = String::with_capacity(cleaned.len());
    let mut new_word = true;
    for ch in cleaned.chars() {
        if ch == ' ' {
            new_word = true;
            out.push(' ');
        } else if new_word {
            out.extend(ch.to_uppercase());
            new_word = false;
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn is_pe_file(path: &Path) -> bool {
    let mut head = [0u8; 2];
    match fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)) {
        Ok(()) => &head == b"MZ",
        Err(_) => false,
    }
}

/// `IMAGE_FILE_MACHINE_*` constants from the PE/COFF spec. Mirrors
/// the values in `pocket-cli`'s archive helper — we deliberately read
/// the raw bytes here so the hot import-time scan doesn't have to
/// fully parse every PE.
const IMAGE_FILE_MACHINE_ARM: u16 = 0x01c0;
const IMAGE_FILE_MACHINE_THUMB: u16 = 0x01c2;
const IMAGE_FILE_MACHINE_ARMNT: u16 = 0x01c4;
const IMAGE_FILE_MACHINE_MIPS_R3000: u16 = 0x0162;
const IMAGE_FILE_MACHINE_MIPS_R4000: u16 = 0x0166;

fn is_nsis_installer(path: &Path) -> bool {
    let Ok(data) = fs::read(path) else {
        return false;
    };
    newtua_nsis::NsisArchive::recognize(&data)
}

fn is_supported_guest_machine(machine: u16) -> bool {
    matches!(
        machine,
        IMAGE_FILE_MACHINE_ARM
            | IMAGE_FILE_MACHINE_THUMB
            | IMAGE_FILE_MACHINE_ARMNT
            | IMAGE_FILE_MACHINE_MIPS_R3000
            | IMAGE_FILE_MACHINE_MIPS_R4000
    )
}

/// `IMAGE_FILE_DLL` in the COFF characteristics word. The only
/// reliable way to tell a library from an executable: a WinCE cabinet
/// stores its payload under generated 8.3 names, so the `.dll`
/// extension is frequently not there to read.
const IMAGE_FILE_DLL: u16 = 0x2000;

/// The COFF header fields we care about at import time.
#[derive(Debug, Clone, Copy)]
struct PeSniff {
    machine: u16,
    characteristics: u16,
}

impl PeSniff {
    fn is_supported_guest(self) -> bool {
        is_supported_guest_machine(self.machine)
    }

    fn is_dll(self) -> bool {
        (self.characteristics & IMAGE_FILE_DLL) != 0
    }
}

/// Resolve an executable's nearest GZGA directory inside this extraction.
fn gzga_game_directory(exe: &Path, extracted: &Path) -> Option<PathBuf> {
    exe.parent()?
        .ancestors()
        .take_while(|p| p.starts_with(extracted))
        .find(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|name| {
                name.to_ascii_uppercase().starts_with("GZGA") && is_gizmondo_title_id(name)
            })
        })
        .map(Path::to_path_buf)
}

fn gzga_collection_root(files: &[PathBuf], extracted: &Path) -> Option<PathBuf> {
    files
        .iter()
        .any(|p| is_guest_exe(p) && gzga_game_directory(p, extracted).is_some())
        .then(|| extracted.to_path_buf())
}

/// Only the supported autorun.exe/multiboot.hog pair at the ZIP root is
/// a demo card. A pair inside a GZGA directory belongs to that game.
fn demo_card_launcher(files: &[PathBuf], extracted: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<&PathBuf> = files
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("autorun.exe"))
                && is_guest_exe(path)
                && path.parent() == Some(extracted)
                && files.iter().any(|hog| {
                    hog.parent() == path.parent()
                        && hog
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.eq_ignore_ascii_case("multiboot.hog"))
                        && hog.is_file()
                })
        })
        .collect();
    candidates.sort_by_key(|path| path.components().count());
    let first = candidates.first()?;
    if candidates
        .get(1)
        .is_some_and(|second| second.components().count() == first.components().count())
    {
        return None;
    }
    Some((*first).clone())
}

/// Cheap PE header sniff: read the COFF machine and characteristics
/// without parsing the whole image.
///
/// Returns `Ok(None)` for non-PE / short files so the caller can scan
/// a whole directory without having to discriminate between "isn't a
/// PE" and "actually failed I/O".
fn sniff_pe(path: &Path) -> std::io::Result<Option<PeSniff>> {
    let mut f = fs::File::open(path)?;
    let mut head = [0u8; 0x40];
    let n = f.read(&mut head)?;
    if n < 0x40 {
        return Ok(None);
    }
    if &head[0..2] != b"MZ" {
        return Ok(None);
    }
    let lfanew = u32::from_le_bytes(head[0x3c..0x40].try_into().unwrap()) as u64;
    f.seek(SeekFrom::Start(lfanew))?;
    // PE signature (4) + machine (2) + sections (2) + timestamp (4)
    // + symbol table (8) + optional header size (2) + characteristics (2).
    let mut coff = [0u8; 24];
    if f.read(&mut coff)? < 24 {
        return Ok(None);
    }
    if &coff[0..4] != b"PE\0\0" {
        return Ok(None);
    }
    Ok(Some(PeSniff {
        machine: u16::from_le_bytes([coff[4], coff[5]]),
        characteristics: u16::from_le_bytes([coff[22], coff[23]]),
    }))
}

/// Cheap PE header sniff: is `path` an ARM or little-endian MIPS PE32.
///
/// Returns `Ok(false)` for non-PE / short / non-ARM files so the
/// caller can scan a whole directory without having to discriminate
/// between "isn't an ARM PE" and "actually failed I/O".
fn is_arm_pe(path: &Path) -> std::io::Result<bool> {
    Ok(sniff_pe(path)?.is_some_and(PeSniff::is_supported_guest))
}

/// Is `path` a guest PE that can serve as a process entry point —
/// i.e. an ARM/MIPS image that is *not* a DLL?
///
/// A resource-only satellite library (Solitaire's `pegcards.dll` is
/// 132 KiB of card artwork with zero exports) looks exactly like a
/// game to a size-ranked scan, so every entry-point search has to
/// filter on this rather than on the file extension.
fn is_guest_exe(path: &Path) -> bool {
    sniff_pe(path)
        .ok()
        .flatten()
        .is_some_and(|pe| pe.is_supported_guest() && !pe.is_dll())
}

/// Is `path` a guest DLL — an ARM/MIPS PE with `IMAGE_FILE_DLL` set?
fn is_guest_dll(path: &Path) -> bool {
    sniff_pe(path)
        .ok()
        .flatten()
        .is_some_and(|pe| pe.is_supported_guest() && pe.is_dll())
}

/// The GL ES driver libraries PocketHLE answers itself — the same two
/// names `pocket_winceapi::gles` claims at the import boundary. The
/// launcher only has to *recognise* a build that draws through a
/// driver, not service its calls, so it keeps the names here rather
/// than depending on the emulator core.
const EMULATED_GLES_DRIVERS: [&str; 2] = ["libgles_cm.dll", "libgles_cl.dll"];

/// The accelerated sibling a cabinet's setup DLL would have installed
/// over `shortcut_target`, if there is one.
///
/// A game that shipped one binary per 3D chip installs all of them,
/// points its Start-menu shortcut at one fixed name, and leaves the
/// choice to the setup DLL the cabinet declares. Call of Duty 2 is the
/// worked example: `SETUPDLL.999` imports `RegOpenKeyExW`,
/// `DeleteAndRenameFile` and `DeleteFileW`, and carries the strings
/// `Software\NVIDIA Corporation\GFSDK`, `\Windows\wmv9decoder2700g.dll`
/// and `%s\cod2.exe` / `%s\cod2_gles.exe` / `%s\cod2_goforce.exe`. On a
/// GoForce handheld it renames `cod2_goforce.exe` (which imports the
/// bundled `libGLES_CM.dll`) over `cod2.exe`; on an Intel 2700G device,
/// whose ROM carries `wmv9decoder2700g.dll`, it renames `cod2_gles.exe`
/// (`libGLES_CL.dll`) instead; a device with neither keeps the
/// software-rendered `cod2.exe`.
///
/// PocketHLE does not run install-time DLLs, so a plain import lands on
/// the software build — the one case that never touches `pocket-gles`.
/// The game then rasterises every pixel in emulated ARM code instead of
/// calling a driver the emulator implements in native Rust, which on
/// this host costs Call of Duty 2 the difference between 13.7 and 39.1
/// fps, and far more on a phone. The emulator always provides the
/// driver, so the accelerated build is the faithful answer.
///
/// Returns `None` when the shortcut target already draws through a
/// driver, when the cabinet ships a single build, or when the siblings
/// want a driver PocketHLE does not implement.
pub fn accelerated_renderer_build(shortcut_target: &Path) -> Option<PathBuf> {
    let dir = shortcut_target.parent()?;
    let stem = shortcut_target.file_stem()?.to_str()?.to_ascii_lowercase();
    if imported_gles_driver(shortcut_target).is_some() {
        return None;
    }
    // WinCE file names are case-insensitive and a cabinet writes
    // whatever case it likes, so match on lower-cased names throughout.
    let entries: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_ascii_lowercase();
            Some((name, path))
        })
        .collect();
    let mut best: Option<(bool, String, PathBuf)> = None;
    for (name, path) in &entries {
        let candidate = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_ascii_lowercase(),
            None => continue,
        };
        // One family, one game: `cod2` -> `cod2_gles`, `cod2_goforce`.
        // Without this a shortcut to a launcher could be answered with
        // some unrelated tool the cabinet happens to install.
        if candidate == stem || !candidate.starts_with(&stem) {
            continue;
        }
        if !is_guest_exe(path) {
            continue;
        }
        let Some(driver) = imported_gles_driver(path) else {
            continue;
        };
        // Two accelerated builds, one shipped driver: prefer the build
        // whose driver the cabinet carries. COD2 bundles
        // `libGLES_CM.dll` for the GoForce build; the 2700G build binds
        // to the `libGLES_CL.dll` that device's ROM already has.
        let shipped = entries.iter().any(|(other, _)| *other == driver);
        let better = match &best {
            None => true,
            Some((best_shipped, best_name, _)) => match (shipped, *best_shipped) {
                (true, false) => true,
                (false, true) => false,
                _ => name < best_name,
            },
        };
        if better {
            best = Some((shipped, name.clone(), path.clone()));
        }
    }
    let (_, _, chosen) = best?;
    log::info!(
        "{} is this cabinet's software-renderer build; launching {} instead, the way its \
         setup DLL would have on a device with a 3D chip",
        shortcut_target.display(),
        chosen.display(),
    );
    Some(chosen)
}

/// The GL ES driver `path` imports, lower-cased, if it is one PocketHLE
/// implements.
fn imported_gles_driver(path: &Path) -> Option<String> {
    let image = pocket_pe::load_file(path).ok()?;
    image
        .imports
        .iter()
        .map(|import| import.dll.to_ascii_lowercase())
        .find(|dll| EMULATED_GLES_DRIVERS.contains(&dll.as_str()))
}

fn now_unix_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Extract the executable's Windows icon and store it next to the game
/// as `icon.png`, returning the path relative to the game directory.
///
/// Every frontend can then show the game's real icon the way
/// j2me-loader shows a MIDlet icon, instead of a generic placeholder.
/// A missing or unreadable icon is not an import error - we return
/// `None` and let the UI fall back.
fn extract_icon_png(game_dir: &Path, exe_abs: &Path) -> Option<PathBuf> {
    let bytes = fs::read(exe_abs).ok()?;
    let icon = pocket_pe::icon_from_pe_bytes(&bytes).ok().flatten()?;
    let path = game_dir.join("icon.png");
    let file = fs::File::create(&path).ok()?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), icon.width, icon.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    if writer.write_image_data(&icon.rgba).is_err() {
        let _ = fs::remove_file(&path);
        return None;
    }
    drop(writer);
    log::info!(
        "extracted {}x{} icon from {}",
        icon.width,
        icon.height,
        exe_abs.display()
    );
    Some(PathBuf::from("icon.png"))
}

/// Guess the display geometry a cabinet was built for.
///
/// Windows Mobile games ship one build per handset and hard-code that
/// handset's screen: the Smartphone edition of Asphalt 2 3D draws a
/// 240x320 portrait screen, while the Motorola Q9 edition draws 320x240
/// landscape and gets clipped on a portrait framebuffer. Nothing in the
/// cabinet states the resolution, but the payload is named after the
/// target device (`Asphalt2_MOTO_Q9.exe`), so match on the handful of
/// device tags that mean "landscape QWERTY Smartphone". Anything we do
/// not recognise stays portrait, and the per-game Settings sheet can
/// override the guess either way.
/// Guess the emulated panel geometry from the file names a cabinet
/// installs.
///
/// Gameloft shipped one binary per handset and put the device in the
/// name — `Asphalt2_SPV_C600.exe` is the 240x320 portrait build,
/// `Asphalt2_MOTO_Q9.exe` the 320x240 landscape one — and a landscape
/// build lays its HUD out past the right edge of a portrait
/// framebuffer, so getting this wrong visibly clips the game. The
/// device tag is only a hint; the per-game Settings sheet is the
/// override.
///
/// `entry_point` is checked first because it is the long name restored
/// from `_setup.xml`; the raw cabinet entries only carry generated 8.3
/// names like `ASPHAL~1.001`, which say nothing about the device.
fn guess_screen(
    entry_point: &Path,
    files: &[pocket_cab::CabFile],
    app_name: Option<&str>,
) -> ScreenPref {
    const LANDSCAPE_TAGS: [&str; 5] = ["moto_q", "motoq", "_q9", "_q8", "_q11"];
    const WVGA_TAGS: [&str; 7] = [
        "wvga", "touch_hd", "hd2", "hd7", "hd_game", "480x800", "asphalt4",
    ];
    let names = std::iter::once(entry_point.to_path_buf())
        .chain(app_name.map(PathBuf::from))
        .chain(files.iter().map(|f| f.extracted_path.clone()))
        .filter_map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
        });
    for name in names {
        if LANDSCAPE_TAGS.iter().any(|tag| name.contains(tag)) {
            return ScreenPref::Landscape;
        }
        if WVGA_TAGS.iter().any(|tag| name.contains(tag)) {
            return ScreenPref::Wvga;
        }
    }
    ScreenPref::default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn deadzone_defaults_and_per_game_json_roundtrip() {
        use super::*;
        let default = LauncherConfig::default();
        assert!(default.gpu_renderer);
        assert_eq!(default.anisotropic_filtering, 0);
        let mut legacy = serde_json::to_value(&default).unwrap();
        legacy.as_object_mut().unwrap().remove("gpu_renderer");
        assert!(
            serde_json::from_value::<LauncherConfig>(legacy.clone())
                .unwrap()
                .gpu_renderer
        );
        legacy["gpu_renderer"] = serde_json::json!(false);
        assert!(
            !serde_json::from_value::<LauncherConfig>(legacy)
                .unwrap()
                .gpu_renderer
        );
        let settings: GameSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.dpad_deadzone, DpadDeadzone::Large);
        assert_eq!(settings.gpu_renderer, None);
        assert_eq!(settings.anisotropic_filtering, None);
        let gpu = GameSettings {
            gpu_renderer: Some(true),
            ..GameSettings::default()
        };
        let decoded: GameSettings =
            serde_json::from_str(&serde_json::to_string(&gpu).unwrap()).unwrap();
        assert_eq!(decoded.gpu_renderer, Some(true));
        for zone in DpadDeadzone::ALL {
            let settings = GameSettings {
                dpad_deadzone: zone,
                ..GameSettings::default()
            };
            let restored: GameSettings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored.dpad_deadzone, zone);
            assert!(zone.stick_threshold(true) < zone.stick_threshold(false));
        }
        assert_eq!(DpadDeadzone::Small.touch_radius(), 0.16);
        assert_eq!(DpadDeadzone::Large.touch_radius(), 0.45);
    }

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn edition_settings_save_without_replacing_the_other_edition() {
        let root = tempfile::tempdir().unwrap();
        let other = if cfg!(feature = "steam-deck") {
            "config.json"
        } else {
            "config-steamdeck.json"
        };
        let original = br#"{"schema_version":1,"colors_language":"fr","fullscreen":true}"#;
        fs::write(root.path().join(other), original).unwrap();
        let mut library = Library::open(root.path()).unwrap();
        library.config_mut().fullscreen = false;
        library.save().unwrap();
        assert_eq!(fs::read(root.path().join(other)).unwrap(), original);
        assert!(root.path().join(CONFIG_FILE_NAME).is_file());
        assert!(!Library::open(root.path()).unwrap().config().fullscreen);
    }

    #[test]
    fn colors_language_eligibility_uses_title_id_not_display_name() {
        let root = tempfile::tempdir().unwrap();
        let mut game: GameEntry = serde_json::from_value(serde_json::json!({
            "id": "test", "display_name": "Renamed game", "source_cab": "game.zip",
            "executable": "extracted/GZGA200015/autorun.exe", "imported_at": 0
        }))
        .unwrap();
        assert!(is_colors_game(&game, root.path()));
        game.executable = "extracted/gzga270015/autorun.exe".into();
        assert!(is_colors_game(&game, root.path()));
        game.display_name = "Colors".into();
        game.executable = "extracted/GZGA200016/autorun.exe".into();
        assert!(!is_colors_game(&game, root.path()));
    }

    #[test]
    fn colors_launch_choices_use_local_files_and_remember_only_available_languages() {
        let root = tempfile::tempdir().unwrap();
        let mut game: GameEntry = serde_json::from_value(serde_json::json!({
            "id": "GZGA200015", "display_name": "Custom name", "source_cab": "game.zip",
            "executable": "extracted/Colors.exe", "imported_at": 0
        }))
        .unwrap();
        let card = game.extracted_dir(root.path());
        fs::create_dir_all(&card).unwrap();
        assert!(is_colors_game(&game, root.path()));
        assert!(colors_languages(&game, root.path()).is_empty());
        fs::write(card.join("bstrings.ENG"), b"english").unwrap();
        assert_eq!(colors_languages(&game, root.path()).len(), 1);
        assert_eq!(
            colors_launch_language(&game, root.path(), "french"),
            "english"
        );
        fs::write(card.join("BSTRINGS.FRA"), b"french").unwrap();
        fs::write(card.join("bstrings.fre"), b"same language alias").unwrap();
        fs::write(card.join("bstrings.xyz"), b"unknown").unwrap();
        assert_eq!(
            colors_languages(&game, root.path())
                .iter()
                .map(|l| l.id)
                .collect::<Vec<_>>(),
            vec!["english", "french"]
        );
        assert_eq!(
            colors_launch_language(&game, root.path(), "french"),
            "french"
        );
        assert_eq!(
            colors_launch_language(&game, root.path(), "german"),
            "english"
        );
        game.id = "GZGA270015".into();
        let other = game.extracted_dir(root.path());
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("bstrings.ita"), b"italian").unwrap();
        assert_eq!(
            colors_launch_language(&game, root.path(), "french"),
            "italian"
        );
        assert_eq!(colors_languages(&game, root.path()).len(), 1);
        fs::write(other.join("bstrings.spa"), b"spanish").unwrap();
        assert_eq!(
            colors_launch_language(&game, root.path(), "french"),
            "italian"
        );
        for id in ["GZGA800015", "GZGA900015", "GZGA200016"] {
            game.id = id.into();
            assert!(!is_colors_game(&game, root.path()));
            assert!(colors_languages(&game, root.path()).is_empty());
        }
    }

    #[test]
    fn colors_configuration_migrates_defaults_and_roundtrips_player_settings() {
        let old: LauncherConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(old.colors_server_url, default_colors_server_url());
        assert_eq!(
            LauncherConfig::default().colors_server_url,
            default_colors_server_url()
        );
        for value in ["", "   "] {
            let config: LauncherConfig =
                serde_json::from_value(serde_json::json!({"colors_server_url": value})).unwrap();
            assert_eq!(config.colors_server_url, default_colors_server_url());
        }
        assert_eq!(old.colors_language, "english");
        assert!(old.colors_terminal_id.is_empty());
        assert!(old.gprs_enabled);
        assert!(!old.gps_fixed_enabled);
        let mut updated = old;
        updated.colors_server_url = "https://colors.example.org".into();
        updated.colors_terminal_id = "Player-A".into();
        updated.colors_language = "french".into();
        updated.gprs_enabled = false;
        updated.gps_fixed_enabled = true;
        updated.gps_fixed_latitude = -33.1234567;
        updated.gps_fixed_longitude = 151.7654321;
        let decoded: LauncherConfig =
            serde_json::from_str(&serde_json::to_string(&updated).unwrap()).unwrap();
        assert_eq!(decoded.colors_server_url, updated.colors_server_url);
        assert_eq!(decoded.colors_terminal_id, updated.colors_terminal_id);
        assert_eq!(decoded.colors_language, "french");
        assert!(!decoded.gprs_enabled);
        assert!(decoded.gps_fixed_enabled);
        assert_eq!(decoded.gps_fixed_latitude, -33.1234567);
        assert_eq!(decoded.gps_fixed_longitude, 151.7654321);
    }

    fn tmpdir(name: &str) -> PathBuf {
        // Tests run in parallel and `now_unix_seconds` has one-second
        // granularity, so the timestamp alone is not unique: two tests
        // starting in the same second used to share a directory and
        // read each other's fixture files. A process-wide counter makes
        // every path distinct.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "pockethle-library-test-{}-{}-{}",
            name,
            now_unix_seconds(),
            unique
        ));
        let _ = fs::remove_dir_all(&p);
        p
    }

    fn entry_with_install_dir(install_dir: Option<&str>) -> GameEntry {
        GameEntry {
            platform: None,
            id: "spore".to_string(),
            display_name: "Spore v1.0.4".to_string(),
            provider: None,
            executable: PathBuf::from("extracted/spore.exe"),
            source_cab: "spore.cab".to_string(),
            install_dir: install_dir.map(str::to_string),
            install_dirs: Vec::new(),
            save_prefix: None,
            registry: Vec::new(),
            imported_at: 0,
            settings: GameSettings::default(),
            icon: None,
            companions: Vec::new(),
        }
    }

    #[test]
    fn setup_xml_install_dir_wins_over_the_binary_header() {
        // A cabinet whose `_setup.xml` installs into a nested vendor
        // directory. The library must follow the script, not the
        // executable's own file name.
        let root = tmpdir("setup_install_dir_vendor_subdir");
        std::fs::create_dir_all(&root).unwrap();
        let xml = root.join("_setup.xml");
        std::fs::write(
            &xml,
            concat!(
                "<wap-provisioningdoc>",
                "<characteristic type=\"Install\">",
                "<parm name=\"InstallDir\" value=\"%CE1%\\SONIC\" />",
                "</characteristic>",
                "<characteristic type=\"FileOperation\">",
                "<characteristic type=\"%CE1%\\Gameloft\\SONIC\" translation=\"install\" />",
                "</characteristic>",
                "</wap-provisioningdoc>",
            ),
        )
        .unwrap();
        let files = vec![pocket_cab::CabFile {
            short_name: "_setup.xml".to_string(),
            extracted_path: xml,
            size: 0,
        }];
        assert_eq!(
            setup_install_dir(&files).as_deref(),
            Some("\\Program Files\\Gameloft\\SONIC\\")
        );
    }

    #[test]
    fn setup_install_dir_is_absent_without_a_script() {
        assert!(setup_install_dir(&[]).is_none());
    }

    #[test]
    fn setup_install_dir_skips_the_start_menu() {
        // Asphalt 2 3D's script makes `%CE2%\\Start Menu` before it
        // makes `%InstallDir%`, so declaration order alone would pick
        // the shell folder the shortcut lives in.
        let root = tmpdir("setup_install_dir_start_menu");
        fs::create_dir_all(&root).unwrap();
        let xml = root.join("_setup.xml");
        fs::write(
            &xml,
            concat!(
                "<wap-provisioningdoc>",
                "<characteristic type=\"Install\">",
                "<parm name=\"InstallDir\" value=\"%CE1%\\Asphalt 2 3D\" />",
                "</characteristic>",
                "<characteristic type=\"FileOperation\">",
                "<characteristic type=\"%CE2%\\Start Menu\">",
                "<characteristic type=\"MakeDir\" />",
                "</characteristic>",
                "<characteristic type=\"%InstallDir%\">",
                "<characteristic type=\"MakeDir\" />",
                "</characteristic>",
                "</characteristic>",
                "</wap-provisioningdoc>",
            ),
        )
        .unwrap();
        let files = vec![pocket_cab::CabFile {
            short_name: "_setup.xml".to_string(),
            extracted_path: xml,
            size: 0,
        }];
        assert_eq!(
            setup_install_dir(&files).as_deref(),
            Some("\\Program Files\\Asphalt 2 3D\\")
        );
    }

    #[test]
    fn gizmondo_module_path_keeps_nested_card_directory_and_mount_target() {
        let root = tmpdir("gizmondo-module-nested");
        let mut entry = entry_with_install_dir(Some("\\SD Card\\"));
        entry.id = "GZAP250006".into();
        entry.executable = "extracted/GZAP250006/CoPilot.exe".into();
        let data = entry.extracted_dir(&root).join("GZAP250006");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("CoPilot.exe"), b"fixture").unwrap();
        fs::write(data.join("pid.cfg"), b"pid").unwrap();
        assert_eq!(
            entry.gizmondo_guest_exe_path(&root).as_deref(),
            Some("\\SD Card\\GZAP250006\\CoPilot.exe")
        );
        assert_eq!(
            entry.install_mount_dir(&root, "\\SD Card\\GZAP250006\\"),
            data
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gizmondo_flattened_module_preserves_declared_install_alias() {
        let root = tmpdir("gizmondo-module-flat");
        let mut entry = entry_with_install_dir(Some("\\SD Card\\GZAP250006\\"));
        entry.executable = "extracted/CoPilot.exe".into();
        assert_eq!(
            entry.gizmondo_guest_exe_path(&root).as_deref(),
            Some("\\SD Card\\GZAP250006\\CoPilot.exe")
        );
        entry.install_dir = None;
        assert_eq!(
            entry.gizmondo_guest_exe_path(&root).as_deref(),
            Some("\\SD Card\\CoPilot.exe")
        );
    }

    #[test]
    fn guest_paths_follow_the_cabinet_install_dir() {
        let entry = entry_with_install_dir(Some("\\Program Files\\EA\\Spore v1.0.4\\"));
        assert_eq!(
            entry.guest_install_prefix().as_deref(),
            Some("\\Program Files\\EA\\Spore v1.0.4\\")
        );
        assert_eq!(
            entry.guest_exe_path().as_deref(),
            Some("\\Program Files\\EA\\Spore v1.0.4\\spore.exe")
        );
    }

    #[test]
    fn guest_prefix_normalises_a_missing_separator() {
        let entry = entry_with_install_dir(Some("\\Program Files\\Asphalt 2 3D"));
        assert_eq!(
            entry.guest_exe_path().as_deref(),
            Some("\\Program Files\\Asphalt 2 3D\\spore.exe")
        );
    }

    #[test]
    fn guest_paths_are_absent_without_an_install_dir() {
        assert!(entry_with_install_dir(None)
            .guest_install_prefix()
            .is_none());
        assert!(entry_with_install_dir(Some("  "))
            .guest_exe_path()
            .is_none());
    }

    #[test]
    fn open_creates_layout() {
        let root = tmpdir("layout");
        let _lib = Library::open(&root).unwrap();
        assert!(root.join("games").is_dir());
        assert!(!root.join("library.json").exists() || root.join("library.json").is_file());
    }

    #[test]
    fn save_and_reload_round_trips() {
        let root = tmpdir("roundtrip");
        let mut lib = Library::open(&root).unwrap();
        lib.config_mut().verbosity = 2;
        lib.config_mut().show_fps = false;
        lib.config_mut().upscale_filter = "smaa_soft".into();
        lib.save().unwrap();

        let lib2 = Library::open(&root).unwrap();
        assert_eq!(lib2.config().verbosity, 2);
        assert!(!lib2.config().show_fps);
        assert_eq!(lib2.config().upscale_filter, "smaa_soft");

        lib.config_mut().orientation = "landscape".to_string();
        lib.save().unwrap();
        let lib3 = Library::open(&root).unwrap();
        assert_eq!(lib3.config().orientation, "landscape");
    }

    #[test]
    fn missing_api_logging_defaults_on_for_old_configs_and_persists_off() {
        let root = tmpdir("missing_api_log");
        let mut lib = Library::open(&root).unwrap();
        assert!(lib.config().log_unimplemented_apis);
        fs::write(root.join(CONFIG_FILE_NAME), br#"{"schema_version":1}"#).unwrap();
        lib = Library::open(&root).unwrap();
        assert!(lib.config().log_unimplemented_apis);
        assert_eq!(lib.config().upscale_filter, "reconstruction");
        lib.config_mut().log_unimplemented_apis = false;
        lib.save().unwrap();
        assert!(
            !Library::open(&root)
                .unwrap()
                .config()
                .log_unimplemented_apis
        );
    }

    #[test]
    fn show_fps_defaults_to_true() {
        // A brand-new library that has never been saved should
        // come back with the FPS overlay enabled.
        let root = tmpdir("showfps_default");
        let lib = Library::open(&root).unwrap();
        assert!(lib.config().show_fps);

        // A pre-existing config.json that pre-dates the show_fps
        // field must also default to enabled when re-opened.
        let root2 = tmpdir("showfps_legacy");
        fs::create_dir_all(&root2).unwrap();
        let legacy_config = serde_json::json!({
            "schema_version": 1,
            "default_cpu_backend": "unicorn",
            "verbosity": 1,
            "last_import_dir": null,
        });
        fs::write(
            root2.join(CONFIG_FILE_NAME),
            serde_json::to_vec_pretty(&legacy_config).unwrap(),
        )
        .unwrap();
        let lib2 = Library::open(&root2).unwrap();
        assert!(lib2.config().show_fps);
    }

    #[test]
    fn gpu_settings_survive_native_write_and_library_reopen() {
        let root = tmpdir("gpu_persistence");
        let mut lib = Library::open(&root).unwrap();
        let entry = entry_with_install_dir(Some("\\Program Files\\Spore"));
        let id = entry.id.clone();
        lib.library.games.push(entry);
        lib.save().unwrap();
        for global in [true, false] {
            for game in [Some(true), Some(false), None] {
                for level in [None, Some(0), Some(2), Some(4), Some(8)] {
                    let mut lib = Library::open(&root).unwrap();
                    // Same JSON boundaries as the Android JNI write methods.
                    let mut config = serde_json::to_value(lib.config()).unwrap();
                    config["gpu_renderer"] = serde_json::json!(global);
                    config["anisotropic_filtering"] = serde_json::json!(8);
                    *lib.config_mut() = serde_json::from_value(config).unwrap();
                    lib.save().unwrap();
                    let mut settings =
                        serde_json::to_value(&lib.get(&id).unwrap().settings).unwrap();
                    settings["gpu_renderer"] = serde_json::json!(game);
                    settings["anisotropic_filtering"] = serde_json::json!(level);
                    lib.update_settings(&id, serde_json::from_value(settings).unwrap())
                        .unwrap();
                    let reopened = Library::open(&root).unwrap();
                    assert_eq!(reopened.config().gpu_renderer, global);
                    assert_eq!(reopened.config().anisotropic_filtering, 8);
                    assert_eq!(
                        reopened.get(&id).unwrap().settings.anisotropic_filtering,
                        level
                    );
                    assert_eq!(reopened.get(&id).unwrap().settings.gpu_renderer, game);
                }
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_game_persists_both_manifests_without_changing_identity() {
        let root = tmpdir("rename");
        let mut lib = Library::open(&root).unwrap();
        let original = entry_with_install_dir(Some("\\Program Files\\Spore"));
        lib.library.games.push(original.clone());
        lib.save().unwrap();
        lib.rename_game(&original.id, "  01 — Spore  ").unwrap();
        let reopened = Library::open(&root).unwrap();
        let game = reopened.get(&original.id).unwrap();
        assert_eq!(game.display_name, "01 — Spore");
        assert_eq!(game.id, original.id);
        assert_eq!(game.executable, original.executable);
        assert_eq!(game.install_dir, original.install_dir);
        let manifest: GameEntry =
            serde_json::from_slice(&fs::read(root.join("games/spore/game.json")).unwrap()).unwrap();
        assert_eq!(manifest.display_name, game.display_name);
        let before = fs::read(root.join("library.json")).unwrap();
        assert!(matches!(
            lib.rename_game(&original.id, "   "),
            Err(LibraryError::InvalidName)
        ));
        assert!(matches!(
            lib.rename_game("missing", "Name"),
            Err(LibraryError::NotFound(_))
        ));
        assert_eq!(fs::read(root.join("library.json")).unwrap(), before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sanitize_id_strips_garbage() {
        assert_eq!(sanitize_id(Some("JumpyBall PPC")), "jumpyball_ppc");
        assert_eq!(sanitize_id(Some("../../etc/passwd")), "etcpasswd");
        assert_eq!(sanitize_id(Some("")), "game");
        assert_eq!(sanitize_id(None), "game");
    }

    #[test]
    fn pretty_id_titlecases_words() {
        assert_eq!(pretty_id("jumpy_ball"), "Jumpy Ball");
        assert_eq!(pretty_id("foo-bar.baz"), "Foo Bar Baz");
    }

    #[test]
    fn legacy_stub_entries_are_migrated_to_unicorn() {
        let root = tmpdir("migrate");
        fs::create_dir_all(root.join("games").join("legacy")).unwrap();
        // Pre-#8 game.json shape: backend = stub, max_slices = 1024.
        let legacy_json = serde_json::json!({
            "id": "legacy",
            "display_name": "Legacy Game",
            "provider": null,
            "executable": "extracted/LEGACY.exe",
            "source_cab": "legacy.cab",
            "imported_at": 0,
            "settings": {
                "cpu_backend": "stub",
                "max_slices": 1024,
                "instructions_per_slice": 1_000_000,
                "halt_on_unimplemented": false,
            },
        });
        fs::write(
            root.join("games").join("legacy").join("game.json"),
            serde_json::to_vec_pretty(&legacy_json).unwrap(),
        )
        .unwrap();
        let library_file = serde_json::json!({
            "schema_version": 1,
            "games": [legacy_json],
        });
        fs::write(
            root.join("library.json"),
            serde_json::to_vec_pretty(&library_file).unwrap(),
        )
        .unwrap();
        // Legacy global config that defaults new games to Stub.
        let legacy_config = serde_json::json!({
            "schema_version": 1,
            "default_cpu_backend": "stub",
            "verbosity": 1,
            "last_import_dir": null,
        });
        fs::write(
            root.join(CONFIG_FILE_NAME),
            serde_json::to_vec_pretty(&legacy_config).unwrap(),
        )
        .unwrap();

        let lib = Library::open(&root).unwrap();
        let game = lib.get("legacy").unwrap();
        assert_eq!(game.settings.cpu_backend, CpuBackendPref::Unicorn);
        assert_eq!(game.settings.max_slices, default_max_slices());
        assert_eq!(lib.config().default_cpu_backend, CpuBackendPref::Unicorn);

        // The migration must also be persisted to disk so the next
        // open() doesn't have to redo the work.
        let lib2 = Library::open(&root).unwrap();
        let game2 = lib2.get("legacy").unwrap();
        assert_eq!(game2.settings.cpu_backend, CpuBackendPref::Unicorn);
        assert_eq!(game2.settings.max_slices, default_max_slices());
    }

    /// Write a PE that is just enough header for [`sniff_pe`]: an `MZ`
    /// stub whose `e_lfanew` points at a COFF header carrying `machine`
    /// and `characteristics`. No sections, no optional header — the
    /// importer's accept/reject decision is made entirely from these
    /// two fields.
    fn write_stub_pe(path: &Path, machine: u16, characteristics: u16) {
        let mut buf = vec![0u8; 0x40];
        buf[0..2].copy_from_slice(b"MZ");
        buf[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        let mut coff = vec![0u8; 24];
        coff[0..4].copy_from_slice(b"PE\0\0");
        coff[4..6].copy_from_slice(&machine.to_le_bytes());
        coff[22..24].copy_from_slice(&characteristics.to_le_bytes());
        buf.extend_from_slice(&coff);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, &buf).unwrap();
    }

    #[test]
    fn wrapped_launcher_card_root_preserves_shared_card_layout_and_split_titles() {
        let root = tempfile::tempdir().unwrap();
        let mut game: GameEntry = serde_json::from_value(serde_json::json!({
            "id":"card", "display_name":"Demo Card", "source_cab":"card.zip",
            "executable":"extracted/14 - Complete Set of Games/autorun.exe", "imported_at":0
        }))
        .unwrap();
        let card = game
            .executable_path(root.path())
            .parent()
            .unwrap()
            .to_path_buf();
        fs::create_dir_all(card.join("GZGA900016")).unwrap();
        fs::write(card.join("MULTIBOOT.HOG"), b"WART3.00").unwrap();
        assert_eq!(game.gizmondo_card_root(root.path()), card);
        assert!(game
            .gizmondo_card_root(root.path())
            .join("GZGA900016")
            .is_dir());
        fs::remove_file(card.join("MULTIBOOT.HOG")).unwrap();
        assert_eq!(
            game.gizmondo_card_root(root.path()),
            game.extracted_dir(root.path())
        );
        fs::write(card.join("multiboot.hog"), b"WART3.00").unwrap();
        game.source_cab = "demo-card:card.zip".into();
        assert_eq!(
            game.gizmondo_card_root(root.path()),
            game.extracted_dir(root.path())
        );
        game.source_cab = "flat.zip".into();
        game.executable = "extracted/autorun.exe".into();
        fs::write(
            game.extracted_dir(root.path()).join("multiboot.hog"),
            b"WART3.00",
        )
        .unwrap();
        assert_eq!(
            game.gizmondo_card_root(root.path()),
            game.extracted_dir(root.path())
        );
    }

    #[test]
    fn demo_card_zip_keeps_launcher_over_larger_games_and_nested_cabs() {
        use std::io::Write;
        for wrapper in [""] {
            let root = tmpdir("demo_card_zip");
            fs::create_dir_all(&root).unwrap();
            let launcher = root.join("fixture.exe");
            write_pe_importing(&launcher, &[]);
            let bytes = fs::read(&launcher).unwrap();
            let mut larger = bytes.clone();
            larger.resize(bytes.len() + 4096, 0);
            let archive_path = root.join("Demo.zip");
            let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
            for (name, data) in [
                ("GZGA200024/autorun.exe", larger.as_slice()),
                ("GZGA200024/helper.cab", b"not a cabinet".as_slice()),
                ("MuLtIbOoT.HoG", b"WART3.00".as_slice()),
                ("AuToRuN.ExE", bytes.as_slice()),
            ] {
                archive
                    .start_file(
                        format!("{wrapper}{name}"),
                        zip::write::FileOptions::default(),
                    )
                    .unwrap();
                archive.write_all(data).unwrap();
            }
            archive.finish().unwrap();
            let library_root = root.join("library");
            let mut library = Library::open(&library_root).unwrap();
            let entry = library.import_zip(&archive_path).unwrap().clone();
            assert_eq!(
                entry.executable,
                PathBuf::from(format!("extracted/{wrapper}AuToRuN.ExE"))
            );
            assert!(entry.executable_path(&library_root).is_file());
            assert_eq!(entry.display_name, "Demo Card");
            let reopened = Library::open(&library_root).unwrap();
            assert_eq!(reopened.library.games[0].executable, entry.executable);
            assert_eq!(reopened.library.games[0].display_name, "Demo Card");
            let manifest: GameEntry = serde_json::from_slice(
                &fs::read(library_root.join("games").join(&entry.id).join("game.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest.executable, entry.executable);
            assert_eq!(manifest.display_name, "Demo Card");
            assert!(library_root
                .join("games")
                .join(&entry.id)
                .join(format!("extracted/{wrapper}GZGA200024/helper.cab"))
                .is_file());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn gzap_artwork_handles_bootstrap_and_nested_executable_without_borrowing() {
        let root = tmpdir("gzap_artwork");
        let extracted = root.join("extracted");
        let card = extracted.join("GZAP200001");
        fs::create_dir_all(card.join("bin")).unwrap();
        let bitmap = card.join("CoPilot.bmp");
        image::RgbImage::from_pixel(4, 2, image::Rgb([10, 20, 30]))
            .save_with_format(&bitmap, image::ImageFormat::Bmp)
            .unwrap();
        for exe in [extracted.join("CoPilot.exe"), card.join("bin/CoPilot.exe")] {
            assert_eq!(
                cartridge_artwork(&root, &exe),
                Some(PathBuf::from("cartridge.png"))
            );
            let image = image::open(root.join("cartridge.png")).unwrap();
            assert_eq!((image.width(), image.height()), (4, 2));
        }
        // Multiple application cards are ambiguous for a root bootstrap.
        fs::create_dir_all(extracted.join("GZAP200006")).unwrap();
        assert_eq!(
            cartridge_artwork(&root, &extracted.join("CoPilot.exe")),
            None
        );
        assert_eq!(
            gizmondo_titles::lookup("GZAP200001"),
            Some("Gizmondo Navigator (UK)")
        );
        assert_eq!(
            gizmondo_titles::lookup("GZAP200006"),
            Some("Gizmondo Navigator USA West")
        );
        assert!(is_gizmondo_title_id("GZAP200001"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn real_cartridge_bmp_supplies_artwork_without_changing_name() {
        let root = tmpdir("cartridge_bmp");
        let card = root.join("games/test/extracted/GZGA200001");
        fs::create_dir_all(&card).unwrap();
        fs::write(card.join("GZGA200001"), b"marker").unwrap();
        let exe = card.join("autorun.exe");
        write_pe_importing(&exe, &[]);
        image::RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30]))
            .save_with_format(card.join("My Game.bmp"), image::ImageFormat::Bmp)
            .unwrap();
        assert_eq!(
            cartridge_artwork(&root, &exe),
            Some(PathBuf::from("cartridge.png"))
        );
        assert!(root.join("cartridge.png").is_file());
        let mut library = Library::open(&root).unwrap();
        let game: GameEntry = serde_json::from_value(serde_json::json!({
            "id": "test", "display_name": "Import fallback", "source_cab": "game.zip",
            "executable": "extracted/GZGA200001/autorun.exe", "imported_at": 0
        }))
        .unwrap();
        let entry = library.commit_entry("test", game).unwrap();
        assert_eq!(entry.display_name, "Import fallback");
        assert_eq!(entry.icon, Some(PathBuf::from("cartridge.png")));
        // ZIP fallback must not borrow a title from any resource filename either.
        use std::io::Write;
        let archive_path = root.join("Archive_Name.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        for (name, data) in [
            ("GZGA200001/autorun.exe", fs::read(&exe).unwrap()),
            (
                "GZGA200001/My Game.bmp",
                fs::read(card.join("My Game.bmp")).unwrap(),
            ),
            ("GZGA200001/GZGA200001", b"marker".to_vec()),
        ] {
            archive
                .start_file(name, zip::write::FileOptions::default())
                .unwrap();
            archive.write_all(&data).unwrap();
        }
        archive.finish().unwrap();
        let entry = library.import_zip(&archive_path).unwrap();
        assert_eq!(entry.display_name, "GZGA200001");
        assert_eq!(entry.icon, Some(PathBuf::from("cartridge.png")));
        fs::write(card.join("My Game.bmp"), b"bad bitmap").unwrap();
        assert!(cartridge_artwork(&root, &exe).is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn gzga_zip_collection_imports_all_titles_without_launcher_or_markers() {
        use std::io::Write;
        for wrapper in ["", "Wrapper/Nested/"] {
            let root = tmpdir("gzga_collection");
            fs::create_dir_all(&root).unwrap();
            let fixture = root.join("fixture.exe");
            write_pe_importing(&fixture, &[]);
            let pe = fs::read(fixture).unwrap();
            let archive_path = root.join("Collection.zip");
            let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
            for (name, data) in [
                ("common/data.bin", b"shared".as_slice()),
                ("Movies/trailer.wmv", b"external movie".as_slice()),
                ("GZGA900013/Movies/own.wmv", b"game movie".as_slice()),
                ("GZGA900013/autorun.exe", pe.as_slice()),
                ("GZGA900013/helper.cab", b"not an installer".as_slice()),
                ("GZGA900013/multiboot.hog", b"WART3.00".as_slice()),
                ("gzga900049/bin/game.exe", pe.as_slice()),
                ("gzga900049/data.bin", b"game2".as_slice()),
            ] {
                archive
                    .start_file(
                        format!("{wrapper}{name}"),
                        zip::write::FileOptions::default(),
                    )
                    .unwrap();
                archive.write_all(data).unwrap();
            }
            archive.finish().unwrap();
            let mut lib = Library::open(root.join("library")).unwrap();
            let entries = lib.import_zip_entries(&archive_path).unwrap();
            assert_eq!(entries.len(), 2);
            assert_eq!(lib.games().len(), 2);
            assert!(lib.get("collection").is_none());
            let names: Vec<_> = entries.iter().map(|e| e.display_name.as_str()).collect();
            assert!(names.contains(&"Richard Burns Rally (april 2005)"));
            assert!(names.contains(&"Hit & Myth (may 2005)"));
            for entry in &entries {
                assert!(is_gizmondo_game(entry, lib.root()));
                assert!(!lib.is_demo_card(&entry.id));
                assert!(entry.executable_path(lib.root()).is_file());
                let extracted = entry.extracted_dir(lib.root());
                assert!(!extracted.join("common").exists());
                assert!(!extracted.join("Movies/trailer.wmv").exists());
                if entry.id == "GZGA900013" {
                    assert_eq!(
                        fs::read(extracted.join("Movies/own.wmv")).unwrap(),
                        b"game movie"
                    );
                }
                let own = game_gizmondo_title_id(entry, lib.root()).unwrap();
                let other = if own == "GZGA900013" {
                    "gzga900049"
                } else {
                    "GZGA900013"
                };
                assert!(!extracted.join(other).exists());
                assert!(!extracted.join("Wrapper").exists());
                assert!(!extracted.join(&own).exists());
                assert_eq!(entry.id, own);
                assert_eq!(
                    extracted,
                    lib.root().join("games").join(&own).join("extracted")
                );
            }
            lib.remove(&entries[0].id).unwrap();
            assert!(entries[1].executable_path(lib.root()).is_file());
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn gzga_collection_imports_games_with_unpaired_root_controls_or_foreign_exes() {
        use std::io::Write;
        for extra in ["autorun.exe", "MULTIBOOT.HOG", "Other/game.exe"] {
            let root = tmpdir("gzga_not_collection");
            fs::create_dir_all(&root).unwrap();
            let fixture = root.join("fixture.exe");
            write_pe_importing(&fixture, &[]);
            let pe = fs::read(fixture).unwrap();
            let archive_path = root.join("Mixed.zip");
            let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
            for name in ["GZGA900013/autorun.exe", "GZGA900049/autorun.exe", extra] {
                archive
                    .start_file(name, zip::write::FileOptions::default())
                    .unwrap();
                archive.write_all(&pe).unwrap();
            }
            archive.finish().unwrap();
            let mut lib = Library::open(root.join("library")).unwrap();
            assert_eq!(lib.import_zip_entries(&archive_path).unwrap().len(), 2);
            assert_eq!(lib.games().len(), 2);
            let _ = fs::remove_dir_all(root);
        }
    }

    fn update_fixture(root: &Path, payload: &[u8]) -> PathBuf {
        update_fixture_with_id(root, payload, "GZGA200016")
    }

    fn update_fixture_with_id(root: &Path, payload: &[u8], id: &str) -> PathBuf {
        use std::io::Write;
        let exe = root.join("fixture.exe");
        write_pe_importing(&exe, &[]);
        let pe = fs::read(exe).unwrap();
        let zip = root.join("update.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        for (name, bytes) in [
            (format!("{id}/autorun.exe"), pe.as_slice()),
            (format!("{id}/data.bin"), payload),
        ] {
            archive
                .start_file(name, zip::write::FileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        zip
    }

    #[test]
    fn alternative_flash_survives_confirmed_reimport() {
        let root = tmpdir("alternative_flash_update");
        fs::create_dir_all(&root).unwrap();
        let mut lib = Library::open(root.join("library")).unwrap();
        let archive = update_fixture_with_id(&root, b"old", "GZGA800015");
        let game = lib.import_zip(&archive).unwrap().clone();
        let config = game
            .gizmondo_flash_dir(lib.root())
            .join("mygames/gzga200015 colors/config.cfg");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, b"compatible E3 config").unwrap();
        let archive = update_fixture_with_id(&root, b"new", "GZGA800015");
        let pending = lib.prepare_import(&archive).unwrap();
        let updated = lib.apply_prepared_import(pending, true).unwrap();
        assert_eq!(
            updated[0].gizmondo_flash_dir(lib.root()),
            game.gizmondo_flash_dir(lib.root())
        );
        assert_eq!(fs::read(config).unwrap(), b"compatible E3 config");
        assert_eq!(
            fs::read(game.extracted_dir(lib.root()).join("data.bin")).unwrap(),
            b"new"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepared_update_requires_confirmation_and_preserves_latest_names_settings_and_saves() {
        let root = tmpdir("prepared_update");
        fs::create_dir_all(&root).unwrap();
        let mut lib = Library::open(root.join("library")).unwrap();
        let old = update_fixture(&root, b"old");
        let game = lib.import_zip(&old).unwrap().clone();
        let save = game.save_dir(lib.root()).join("nested/progress.dat");
        fs::create_dir_all(save.parent().unwrap()).unwrap();
        fs::write(&save, b"progress").unwrap();
        let flash = lib.root().join("flash/other-game.sav");
        fs::create_dir_all(flash.parent().unwrap()).unwrap();
        fs::write(&flash, b"other save").unwrap();
        let new = update_fixture(&root, b"new");
        let pending = lib.prepare_import(&new).unwrap();
        assert_eq!(pending.updates().len(), 1);
        assert_eq!(
            fs::read(game.extracted_dir(lib.root()).join("data.bin")).unwrap(),
            b"old"
        );
        drop(pending);
        assert_eq!(fs::read(&save).unwrap(), b"progress");
        let pending = lib.prepare_import(&new).unwrap();
        assert!(lib.apply_prepared_import(pending, false).is_err());
        assert_eq!(
            fs::read(game.extracted_dir(lib.root()).join("data.bin")).unwrap(),
            b"old"
        );
        let pending = lib.prepare_import(&new).unwrap();
        lib.rename_game(&game.id, "My Chicane").unwrap();
        lib.get_mut(&game.id).unwrap().settings.rotation = RotationPref::Cw90;
        lib.save().unwrap();
        let updated = lib.apply_prepared_import(pending, true).unwrap();
        assert_eq!(updated[0].display_name, "My Chicane");
        assert_eq!(updated[0].settings.rotation, RotationPref::Cw90);
        assert_eq!(
            fs::read(game.extracted_dir(lib.root()).join("data.bin")).unwrap(),
            b"new"
        );
        assert_eq!(fs::read(save).unwrap(), b"progress");
        assert_eq!(fs::read(flash).unwrap(), b"other save");
        assert_eq!(lib.games().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepared_new_import_refuses_an_unconfirmed_intervening_install() {
        let root = tmpdir("prepared_race");
        fs::create_dir_all(&root).unwrap();
        let mut lib = Library::open(root.join("library")).unwrap();
        let archive = update_fixture(&root, b"data");
        let pending = lib.prepare_import(&archive).unwrap();
        assert!(pending.updates().is_empty());
        let game = lib.import_zip(&archive).unwrap().clone();
        fs::write(
            game.extracted_dir(lib.root()).join("data.bin"),
            b"changed by another import",
        )
        .unwrap();
        assert!(lib.apply_prepared_import(pending, true).is_err());
        assert_eq!(
            fs::read(game.extracted_dir(lib.root()).join("data.bin")).unwrap(),
            b"changed by another import"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepared_collection_updates_only_existing_game_and_imports_new_game() {
        use std::io::Write;
        let root = tmpdir("prepared_collection");
        fs::create_dir_all(&root).unwrap();
        let mut lib = Library::open(root.join("library")).unwrap();
        let old = update_fixture(&root, b"old");
        let old_game = lib.import_zip(&old).unwrap().clone();
        let save = old_game.save_dir(lib.root()).join("save.dat");
        fs::create_dir_all(save.parent().unwrap()).unwrap();
        fs::write(&save, b"saved progress").unwrap();
        let pe = fs::read(root.join("fixture.exe")).unwrap();
        let archive_path = root.join("collection.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        for (name, bytes) in [
            ("GZGA200016/autorun.exe", pe.as_slice()),
            ("GZGA200016/data.bin", b"new".as_slice()),
            ("GZGA200015/autorun.exe", pe.as_slice()),
            ("GZGA200015/data.bin", b"colors".as_slice()),
            ("Movies/trailer.wmv", b"external".as_slice()),
        ] {
            archive
                .start_file(name, zip::write::FileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        let pending = lib.prepare_import(&archive_path).unwrap();
        assert_eq!(pending.entries().len(), 2);
        assert_eq!(pending.updates().len(), 1);
        assert_eq!(pending.updates()[0].id, old_game.id);
        lib.apply_prepared_import(pending, true).unwrap();
        assert_eq!(lib.games().len(), 2);
        assert_eq!(fs::read(&save).unwrap(), b"saved progress");
        assert_eq!(
            fs::read(old_game.extracted_dir(lib.root()).join("data.bin")).unwrap(),
            b"new"
        );
        for game in lib.games() {
            assert!(!game.extracted_dir(lib.root()).join("Movies").exists());
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepared_update_rolls_back_when_catalog_publish_fails() {
        let root = tmpdir("prepared_update_rollback");
        fs::create_dir_all(&root).unwrap();
        let mut lib = Library::open(root.join("library")).unwrap();
        let old = update_fixture(&root, b"old");
        let game = lib.import_zip(&old).unwrap().clone();
        let save = game.save_dir(lib.root()).join("progress.dat");
        fs::create_dir_all(save.parent().unwrap()).unwrap();
        fs::write(&save, b"progress").unwrap();
        let new = update_fixture(&root, b"new");
        let pending = lib.prepare_import(&new).unwrap();
        fs::remove_file(lib.root().join("library.json")).unwrap();
        fs::create_dir(lib.root().join("library.json")).unwrap();
        assert!(lib.apply_prepared_import(pending, true).is_err());
        assert_eq!(
            fs::read(game.extracted_dir(lib.root()).join("data.bin")).unwrap(),
            b"old"
        );
        assert_eq!(fs::read(save).unwrap(), b"progress");
        assert_eq!(lib.games().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn single_directory_unwrapping_preserves_sdk_card_boundaries() {
        use std::io::Write;
        let root = tmpdir("sdk_boundary");
        fs::create_dir_all(&root).unwrap();
        let fixture = root.join("fixture.exe");
        write_pe_importing(&fixture, &[]);
        let pe = fs::read(fixture).unwrap();
        let path = root.join("diagnostic.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, bytes) in [
            ("Wrapper/GZVT999998/AUTORUN.EXE", pe.as_slice()),
            ("Wrapper/GZVT999998/GZVT999998", b"mark".as_slice()),
        ] {
            archive
                .start_file(name, zip::write::FileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        let mut lib = Library::open(root.join("library")).unwrap();
        let entry = lib.import_zip(&path).unwrap().clone();
        assert_eq!(
            entry.executable,
            PathBuf::from("extracted/GZVT999998/AUTORUN.EXE")
        );
        assert!(entry
            .extracted_dir(lib.root())
            .join("GZVT999998/GZVT999998")
            .is_file());
        assert!(!entry.extracted_dir(lib.root()).join("Wrapper").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn serial_mount_alias_preserves_real_directories_and_flattened_files() {
        let root = tmpdir("serial_mount");
        let mut game = entry_with_install_dir(None);
        game.id = "GZGA200099".into();
        let extracted = game.extracted_dir(&root);
        fs::create_dir_all(extracted.join("gzga200099")).unwrap();
        fs::write(extracted.join("gzga200099/GZGA200099"), [1, 2, 3, 4]).unwrap();
        for prefix in ["\\SD Card\\GZGA200099\\", "\\Storage Card\\gzga200099\\"] {
            assert_eq!(
                game.install_mount_dir(&root, prefix),
                extracted.join("gzga200099")
            );
        }
        // A marker file itself must not become a directory mount (Toy Golf).
        fs::remove_dir_all(extracted.join("gzga200099")).unwrap();
        fs::write(extracted.join("GZGA200099"), [1, 2, 3, 4]).unwrap();
        assert_eq!(
            game.install_mount_dir(&root, "\\SD Card\\GZGA200099\\"),
            extracted
        );
        assert_eq!(
            game.install_mount_dir(&root, "\\Program Files\\GZGA200099\\"),
            extracted
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn serial_file_next_to_executable_identifies_gizmondo() {
        let root = tmpdir("serial_file");
        let mut game = entry_with_install_dir(None);
        game.id = "unknown".into();
        game.source_cab = "unknown.zip".into();
        game.executable = "extracted/Autorun.exe".into();
        let exe = game.executable_path(&root);
        write_pe_importing(&exe, &[]);
        fs::write(exe.parent().unwrap().join("gzga800069"), b"serial").unwrap();
        assert!(is_gizmondo_game(&game, &root));
        assert_eq!(
            game_gizmondo_title_id(&game, &root).as_deref(),
            Some("GZGA800069")
        );
        fs::remove_file(exe.parent().unwrap().join("gzga800069")).unwrap();
        fs::write(exe.parent().unwrap().join("GZGA800069.txt"), b"text").unwrap();
        assert!(!is_gizmondo_game(&game, &root));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_serial_directory_identifies_root_game_and_wrappers_are_removed() {
        use std::io::Write;
        let root = tmpdir("empty_serial");
        fs::create_dir_all(&root).unwrap();
        let fixture = root.join("fixture.exe");
        write_pe_importing(&fixture, &[]);
        let pe = fs::read(fixture).unwrap();
        for serial in [Some("GZGA800069"), None] {
            let archive_path = root.join("mon_jeu.zip");
            let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
            archive
                .start_file(
                    "Wrapper/Nested/Autorun.exe",
                    zip::write::FileOptions::default(),
                )
                .unwrap();
            archive.write_all(&pe).unwrap();
            if let Some(serial) = serial {
                archive
                    .add_directory(
                        format!("Wrapper/Nested/{serial}/"),
                        zip::write::FileOptions::default(),
                    )
                    .unwrap();
            }
            archive.finish().unwrap();
            let mut lib =
                Library::open(root.join(if serial.is_some() { "giz" } else { "normal" })).unwrap();
            let entry = lib.import_zip(&archive_path).unwrap().clone();
            assert_eq!(entry.id, serial.unwrap_or("mon_jeu"));
            assert_eq!(entry.executable, PathBuf::from("extracted/Autorun.exe"));
            assert_eq!(is_gizmondo_game(&entry, lib.root()), serial.is_some());
            assert!(!entry.extracted_dir(lib.root()).join("Wrapper").exists());
            if serial.is_some() {
                assert_eq!(
                    game_gizmondo_title_id(&entry, lib.root()).as_deref(),
                    serial
                );
            }
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn demo_split_creates_independent_game_trees_and_preserves_shared_resources() {
        use std::io::Write;
        let root = tmpdir("demo_split");
        fs::create_dir_all(&root).unwrap();
        let fixture = root.join("fixture.exe");
        write_pe_importing(&fixture, &[]);
        let pe = fs::read(fixture).unwrap();
        let archive_path = root.join("Demo.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        for (name, data) in [
            ("autorun.exe", pe.as_slice()),
            ("multiboot.hog", b"WART3.00".as_slice()),
            ("common/data.bin", b"shared".as_slice()),
            ("GZGA200001/autorun.exe", pe.as_slice()),
            ("GZGA200001/GZGA200001", b"marker".as_slice()),
            ("Agaju/autorun.exe", pe.as_slice()),
            ("Agaju/data.bin", b"game2".as_slice()),
        ] {
            archive
                .start_file(name, zip::write::FileOptions::default())
                .unwrap();
            archive.write_all(data).unwrap();
        }
        archive.finish().unwrap();
        let mut lib = Library::open(root.join("library")).unwrap();
        let id = lib.import_zip(&archive_path).unwrap().id.clone();
        assert!(lib.is_demo_card(&id));
        let entries = lib.split_demo_card(&id).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(lib.get(&id).is_none());
        for e in &entries {
            assert!(e.executable_path(lib.root()).is_file());
            assert!(e
                .extracted_dir(lib.root())
                .join("common/data.bin")
                .is_file());
            assert_eq!(e.executable, PathBuf::from("extracted/autorun.exe"));
            let other = if e.id == "agaju" {
                "GZGA200001"
            } else {
                "Agaju"
            };
            assert!(!e.extracted_dir(lib.root()).join(other).exists());
        }
        assert_ne!(entries[0].id, entries[1].id);
        lib.remove(&entries[0].id).unwrap();
        assert!(entries[1].executable_path(lib.root()).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catalogue_upgrades_id_placeholders_but_preserves_custom_names_and_save_paths() {
        let root = tmpdir("title_catalogue_migration");
        let mut lib = Library::open(&root).unwrap();
        let mut game = entry_with_install_dir(None);
        game.id = "demo_chicane".into();
        game.source_cab = "demo-card:Demo.zip".into();
        game.executable = "extracted/GZGA200016/Autorun.exe".into();
        game.display_name = "Gzga200016".into();
        let exe = game.executable_path(&root);
        write_pe_importing(&exe, &[]);
        lib.library.games.push(game.clone());
        lib.save().unwrap();
        let mut loaded = Library::open(&root).unwrap();
        let e = loaded.get("demo_chicane").unwrap();
        assert_eq!(e.display_name, "Chicane");
        assert_eq!(e.executable, game.executable);
        assert_eq!(e.save_prefix, game.save_prefix);
        loaded
            .rename_game("demo_chicane", "My custom Chicane")
            .unwrap();
        let reloaded = Library::open(&root).unwrap();
        assert_eq!(
            reloaded.get("demo_chicane").unwrap().display_name,
            "My custom Chicane"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn navigator_and_skystriker_imports_use_catalogue_names_and_keep_custom_renames() {
        use std::io::Write;
        let root = tmpdir("navigator_skystriker_names");
        fs::create_dir_all(&root).unwrap();
        let fixture = root.join("fixture.exe");
        write_pe_importing(&fixture, &[]);
        let pe = fs::read(fixture).unwrap();
        for (id, title) in [
            ("GZAP200001", "Gizmondo Navigator (UK)"),
            ("GZAP250006", "Gizmondo Navigator (USA West Coast)"),
            ("GZGA200096", "Skystriker"),
        ] {
            let path = root.join(format!("{id}.zip"));
            let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            archive
                .start_file(
                    format!("{id}/Autorun.exe"),
                    zip::write::FileOptions::default(),
                )
                .unwrap();
            archive.write_all(&pe).unwrap();
            archive
                .start_file(format!("{id}/{id}"), zip::write::FileOptions::default())
                .unwrap();
            archive.write_all(b"serial").unwrap();
            archive.finish().unwrap();
            let library_root = root.join(id);
            let mut lib = Library::open(&library_root).unwrap();
            let entry = lib.import_zip(&path).unwrap().clone();
            assert_eq!(entry.id, id);
            assert_eq!(entry.display_name, title);
            assert!(is_gizmondo_game(&entry, &library_root));
            let manifest: GameEntry = serde_json::from_slice(
                &fs::read(library_root.join("games").join(id).join("game.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest.display_name, title);
            let mut reopened = Library::open(&library_root).unwrap();
            assert_eq!(reopened.get(id).unwrap().display_name, title);
            reopened.rename_game(id, "Mon titre personnel").unwrap();
            assert_eq!(
                reopened.import_zip(&path).unwrap().display_name,
                "Mon titre personnel"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepared_flat_card_imports_use_archive_title_id() {
        use std::io::Write;
        let root = tmpdir("prepared_flat_card_names");
        fs::create_dir_all(&root).unwrap();
        let fixture = root.join("fixture.exe");
        write_pe_importing(&fixture, &[]);
        let pe = fs::read(fixture).unwrap();
        for (id, title) in [
            ("GZAP200001", "Gizmondo Navigator (UK)"),
            ("GZAP250006", "Gizmondo Navigator (USA West Coast)"),
            ("GZGA200096", "Skystriker"),
        ] {
            let path = root.join(format!("{}.zip", id.to_ascii_lowercase()));
            let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            archive
                .start_file("Autorun.exe", zip::write::FileOptions::default())
                .unwrap();
            archive.write_all(&pe).unwrap();
            archive.finish().unwrap();
            let mut lib = Library::open(root.join(id)).unwrap();
            let pending = lib.prepare_import(&path).unwrap();
            assert_eq!(pending.entries[0].id, id);
            assert_eq!(pending.entries[0].display_name, title);
            assert!(lib.games().is_empty());
            let imported = lib.apply_prepared_import(pending, false).unwrap();
            assert_eq!(imported[0].display_name, title);
            lib.rename_game(id, "Mon titre").unwrap();
            let pending = lib.prepare_import(&path).unwrap();
            let imported = lib.apply_prepared_import(pending, true).unwrap();
            assert_eq!(imported[0].display_name, "Mon titre");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn navigator_old_default_is_updated_without_replacing_a_personal_name() {
        let root = tmpdir("navigator_name_migration");
        let mut lib = Library::open(&root).unwrap();
        let mut entry = entry_with_install_dir(None);
        entry.id = "GZAP200001".into();
        entry.display_name = "Copilot UK".into();
        entry.executable = "extracted/CoPilot.exe".into();
        write_pe_importing(&entry.executable_path(&root), &[]);
        lib.library.games.push(entry);
        lib.save().unwrap();
        let mut loaded = Library::open(&root).unwrap();
        assert_eq!(
            loaded.get("GZAP200001").unwrap().display_name,
            "Gizmondo Navigator (UK)"
        );
        loaded.rename_game("GZAP200001", "Mon Navigator").unwrap();
        assert_eq!(
            Library::open(&root)
                .unwrap()
                .get("GZAP200001")
                .unwrap()
                .display_name,
            "Mon Navigator"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalogue_corrects_old_variant_dates_without_renaming_custom_titles() {
        let root = tmpdir("variant_date_migration");
        let mut lib = Library::open(&root).unwrap();
        for (id, old_name) in [
            ("GZGA900069", "Catapult (may 2005)"),
            ("GZGA800056", "Agaju (october 2005)"),
        ] {
            let mut entry = entry_with_install_dir(None);
            entry.id = id.into();
            entry.display_name = old_name.into();
            entry.executable = "extracted/autorun.exe".into();
            write_pe_importing(&entry.executable_path(&root), &[]);
            lib.library.games.push(entry);
        }
        lib.save().unwrap();
        let mut loaded = Library::open(&root).unwrap();
        assert_eq!(
            loaded.get("GZGA900069").unwrap().display_name,
            "Catapult (august 2005)"
        );
        assert_eq!(
            loaded.get("GZGA800056").unwrap().display_name,
            "Agaju (august 2005)"
        );
        loaded.rename_game("GZGA800056", "My Agaju demo").unwrap();
        assert_eq!(
            Library::open(&root)
                .unwrap()
                .get("GZGA800056")
                .unwrap()
                .display_name,
            "My Agaju demo"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generic_alt_suffix_mounts_assets_and_isolates_flash_without_a_catalogue_entry() {
        let root = tmpdir("generic_alt_suffix");
        let table: std::collections::BTreeMap<String, String> =
            serde_json::from_str(include_str!("../data/gizmondo-titles.json")).unwrap();
        let mut ids: Vec<&str> = table
            .keys()
            .filter(|id| {
                id.starts_with("GZGA7") || id.starts_with("GZGA8") || id.starts_with("GZGA9")
            })
            .map(String::as_str)
            .collect();
        assert_eq!(ids.len(), 11);
        ids.extend(["GZGA900123", "GZGA600123"]);
        for id in ids {
            let original = format!("GZGA2{}", &id[5..]);
            let mut game = entry_with_install_dir(None);
            game.id = id.into();
            game.display_name = "Custom title".into();
            game.executable = "extracted/autorun.exe".into();
            game.install_dirs = vec![format!("\\SD Card\\{id}\\")];
            let extracted = game.extracted_dir(&root);
            fs::create_dir_all(extracted.join(id)).unwrap();
            fs::create_dir_all(extracted.join("data")).unwrap();
            fs::write(extracted.join(id).join(&original), [1, 2, 3, 4]).unwrap();
            fs::write(extracted.join("data/asset.bin"), b"assets").unwrap();
            let mounts = game.install_mounts(&root);
            for card in ["SD Card", "Storage Card"] {
                let prefix = format!("\\{card}\\{original}\\");
                let (_, target) = mounts.iter().find(|(p, _)| p == &prefix).unwrap();
                assert_eq!(fs::read(target.join("data/asset.bin")).unwrap(), b"assets");
                assert_eq!(mounts.iter().filter(|(p, _)| p == &prefix).count(), 1);
            }
            assert_eq!(
                game.gizmondo_flash_dir(&root),
                game.save_dir(&root).join("flash")
            );
            game.id = format!("legacy_{id}");
            assert!(game
                .install_mounts(&root)
                .iter()
                .any(|(p, _)| p == &format!("\\SD Card\\{original}\\")));
            assert_eq!(
                game.gizmondo_flash_dir(&root),
                game.save_dir(&root).join("flash")
            );
        }
        for id in [
            "GZGA200049",
            "GZGA270049",
            "GZGA280045",
            "GZGA90012",
            "GZGA90012x",
            "GZAP900049",
            "GZGA999999",
        ] {
            assert_eq!(gizmondo_titles::original_card_id(id), None);
        }
        assert_eq!(
            gizmondo_titles::original_card_id("gzga900049").as_deref(),
            Some("GZGA200049")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn alternative_flash_disks_do_not_share_standard_configs_or_each_other() {
        let root = tmpdir("alternative_flash_disks");
        let mut game = entry_with_install_dir(None);
        game.id = "GZGA200015".into();
        let standard = game.gizmondo_flash_dir(&root);
        assert_eq!(standard, root.join("flash"));
        let config = Path::new("mygames/gzga200015 colors/config.cfg");
        fs::create_dir_all(standard.join(config).parent().unwrap()).unwrap();
        fs::write(standard.join(config), b"standard configuration").unwrap();
        for id in ["GZGA800015", "GZGA900015", "GZGA800016", "GZGA900056"] {
            game.id = id.into();
            let isolated = game.gizmondo_flash_dir(&root);
            assert_eq!(isolated, game.save_dir(&root).join("flash"));
            assert!(!isolated.join(config).exists());
            fs::create_dir_all(isolated.join(config).parent().unwrap()).unwrap();
            fs::write(isolated.join(config), id.as_bytes()).unwrap();
        }
        game.id = "GZGA800015".into();
        game.display_name = "My renamed demo".into();
        assert_eq!(
            fs::read(game.gizmondo_flash_dir(&root).join(config)).unwrap(),
            b"GZGA800015"
        );
        game.id = "old_archive_id".into();
        game.install_dirs = vec!["\\SD Card\\gzga800015\\".into()];
        assert_eq!(
            game.gizmondo_flash_dir(&root),
            game.save_dir(&root).join("flash")
        );
        game.install_dirs = vec!["\\SD Card\\GZGA200015\\".into()];
        assert_eq!(game.gizmondo_flash_dir(&root), standard);
        game.id = "GZGA999999".into();
        assert_eq!(game.gizmondo_flash_dir(&root), standard);
        assert_eq!(
            fs::read(standard.join(config)).unwrap(),
            b"standard configuration"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn original_sdk_marker_aliases_preserve_alternative_identity_and_real_directories() {
        let root = tmpdir("alt_original_marker_alias");
        let mut game = entry_with_install_dir(None);
        game.id = "GZGA800015".into();
        game.executable = "extracted/Autorun.exe".into();
        game.install_dirs = vec!["\\SD Card\\GZGA800015\\".into()];
        let extracted = game.extracted_dir(&root);
        fs::create_dir_all(&extracted).unwrap();
        fs::write(extracted.join("GZGA200015"), [1, 2, 3, 4]).unwrap();
        fs::create_dir_all(extracted.join("GZGA200099")).unwrap();
        fs::write(extracted.join("GZGA200099/GZGA200099"), [1, 2, 3, 4]).unwrap();
        fs::write(extracted.join("GZGA900015.txt"), b"not a marker").unwrap();
        let mounts = game.install_mounts(&root);
        assert!(mounts.contains(&("\\SD Card\\GZGA800015\\".into(), extracted.clone())));
        for card in ["SD Card", "Storage Card"] {
            assert!(mounts.contains(&(format!("\\{card}\\GZGA200015\\"), extracted.clone())));
            assert!(mounts.contains(&(
                format!("\\{card}\\GZGA200099\\"),
                extracted.join("GZGA200099")
            )));
        }
        assert_eq!(mounts.len(), 5);
        assert_eq!(game.id, "GZGA800015");
        game.executable = "extracted/bin/Autorun.exe".into();
        game.install_dirs.push("\\SD Card\\GZGA200015\\".into());
        fs::create_dir_all(extracted.join("bin")).unwrap();
        fs::write(extracted.join("bin/GZGA200015"), [1, 2, 3, 4]).unwrap();
        let nested = game.install_mounts(&root);
        assert!(nested.contains(&("\\SD Card\\GZGA200015\\".into(), extracted.join("bin"))));
        assert!(nested.contains(&("\\Storage Card\\GZGA200015\\".into(), extracted.join("bin"))));
        assert!(nested.contains(&(
            "\\SD Card\\GZGA200099\\".into(),
            extracted.join("GZGA200099")
        )));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn demo_card_detection_requires_supported_sibling_pair() {
        let root = tmpdir("demo_card_pair");
        let launcher = root.join("autorun.exe");
        write_stub_pe(&launcher, MACHINE_ARM, 0x0102);
        let hog = root.join("multiboot.hog");
        assert!(demo_card_launcher(&[launcher.clone()], &root).is_none());
        fs::write(&hog, b"WART3.00").unwrap();
        let other = root.join("Other/multiboot.hog");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, b"WART3.00").unwrap();
        assert!(demo_card_launcher(&[launcher.clone(), other], &root).is_none());
        assert_eq!(
            demo_card_launcher(&[launcher.clone(), hog.clone()], &root),
            Some(launcher.clone())
        );
        write_stub_pe(&launcher, MACHINE_X86, 0x0102);
        assert!(demo_card_launcher(&[launcher.clone(), hog.clone()], &root).is_none());
        write_stub_pe(&launcher, MACHINE_ARM, IMAGE_FILE_DLL | 0x0102);
        assert!(demo_card_launcher(&[launcher, hog], &root).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    const MACHINE_ARM: u16 = 0x01c0;
    const MACHINE_X86: u16 = 0x014c;

    /// Write an ARM PE32 whose import directory names `dlls`, each with
    /// one ordinal import — enough for `pocket_pe` to report the DLL,
    /// which is what tells a hardware-renderer build from a software
    /// one.
    #[test]
    fn compatibility_update_zip_platform_choice_persists_and_known_cards_skip_the_question() {
        use std::io::Write;
        for platform in [GamePlatform::Gizmondo, GamePlatform::PocketPc] {
            let root = tempfile::tempdir().unwrap();
            let pe = root.path().join("fixture.exe");
            write_pe_importing(&pe, &[]);
            let archive = root.path().join("unmarked.zip");
            let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
            zip.start_file("Autorun.exe", zip::write::FileOptions::default())
                .unwrap();
            zip.write_all(&fs::read(pe).unwrap()).unwrap();
            zip.finish().unwrap();
            let library_path = root.path().join("library");
            let mut lib = Library::open(&library_path).unwrap();
            let mut pending = lib.prepare_import(&archive).unwrap();
            let choices = pending.platform_choices();
            assert_eq!(choices.len(), 1);
            assert!(lib.games().is_empty());
            assert!(pending.choose_platform(&choices[0].0, platform));
            assert!(pending.platform_choices().is_empty());
            let entries = lib.apply_prepared_import(pending, false).unwrap();
            assert_eq!(entries[0].platform, Some(platform));
            assert_eq!(
                is_gizmondo_game(&entries[0], &library_path),
                platform == GamePlatform::Gizmondo
            );
            let reopened = Library::open(&library_path).unwrap();
            assert_eq!(reopened.games()[0].platform, Some(platform));
            let pending = reopened.prepare_import(&archive).unwrap();
            assert!(pending.platform_choices().is_empty()); // explicit prior choice is retained
            let entries = lib.apply_prepared_import(pending, true).unwrap();
            assert_eq!(entries[0].platform, Some(platform));
            let marked = update_fixture(root.path(), b"data");
            let pending = lib.prepare_import(marked).unwrap();
            assert!(pending.platform_choices().is_empty());
            // The same preparation mechanism for CAB/EXE sources does not ask.
            let pe = root.path().join("plain.exe");
            write_pe_importing(&pe, &[]);
            assert!(lib
                .prepare_import(pe)
                .unwrap()
                .platform_choices()
                .is_empty());
        }
    }

    fn write_pe_importing(path: &Path, dlls: &[&str]) {
        const LFANEW: u32 = 0x80;
        const OPTIONAL_HEADER_SIZE: u16 = 0xe0;
        const SECTION_RVA: u32 = 0x1000;
        const RAW_OFFSET: u32 = 0x200;

        // `.idata`: a null-terminated descriptor array, then per DLL a
        // one-entry lookup table and the name string.
        let descriptors_len = 20 * (dlls.len() as u32 + 1);
        let mut idata = vec![0u8; descriptors_len as usize];
        let rva = |offset: usize| SECTION_RVA + offset as u32;
        for (index, dll) in dlls.iter().enumerate() {
            let thunk_offset = idata.len();
            // `IMAGE_ORDINAL_FLAG | 1`, then the terminator.
            idata.extend_from_slice(&0x8000_0001u32.to_le_bytes());
            idata.extend_from_slice(&0u32.to_le_bytes());
            let name_offset = idata.len();
            idata.extend_from_slice(dll.as_bytes());
            idata.push(0);
            let descriptor = &mut idata[index * 20..index * 20 + 20];
            descriptor[0..4].copy_from_slice(&rva(thunk_offset).to_le_bytes());
            descriptor[12..16].copy_from_slice(&rva(name_offset).to_le_bytes());
            descriptor[16..20].copy_from_slice(&rva(thunk_offset).to_le_bytes());
        }

        let mut buf = vec![0u8; LFANEW as usize];
        buf[0..2].copy_from_slice(b"MZ");
        buf[0x3c..0x40].copy_from_slice(&LFANEW.to_le_bytes());
        buf.extend_from_slice(b"PE\0\0");
        let mut coff = vec![0u8; 20];
        coff[0..2].copy_from_slice(&MACHINE_ARM.to_le_bytes());
        coff[2..4].copy_from_slice(&1u16.to_le_bytes()); // one section
        coff[16..18].copy_from_slice(&OPTIONAL_HEADER_SIZE.to_le_bytes());
        // `IMAGE_FILE_EXECUTABLE_IMAGE | IMAGE_FILE_32BIT_MACHINE`.
        coff[18..20].copy_from_slice(&0x0102u16.to_le_bytes());
        buf.extend_from_slice(&coff);
        let mut optional = vec![0u8; OPTIONAL_HEADER_SIZE as usize];
        optional[0..2].copy_from_slice(&0x010bu16.to_le_bytes()); // PE32
        optional[16..20].copy_from_slice(&SECTION_RVA.to_le_bytes()); // entry point
        optional[28..32].copy_from_slice(&0x0001_0000u32.to_le_bytes()); // image base
        optional[32..36].copy_from_slice(&0x1000u32.to_le_bytes()); // section align
        optional[36..40].copy_from_slice(&0x0200u32.to_le_bytes()); // file align
        optional[56..60].copy_from_slice(&0x2000u32.to_le_bytes()); // size of image
        optional[60..64].copy_from_slice(&RAW_OFFSET.to_le_bytes()); // size of headers
        optional[68..70].copy_from_slice(&9u16.to_le_bytes()); // WinCE GUI subsystem
        optional[92..96].copy_from_slice(&16u32.to_le_bytes()); // data directories
        optional[104..108].copy_from_slice(&SECTION_RVA.to_le_bytes()); // data dir 1: imports
        optional[108..112].copy_from_slice(&descriptors_len.to_le_bytes()); // its size
        buf.extend_from_slice(&optional);
        let mut section = vec![0u8; 40];
        section[0..7].copy_from_slice(b".idata\0");
        section[8..12].copy_from_slice(&(idata.len() as u32).to_le_bytes());
        section[12..16].copy_from_slice(&SECTION_RVA.to_le_bytes());
        section[16..20].copy_from_slice(&(idata.len() as u32).to_le_bytes());
        section[20..24].copy_from_slice(&RAW_OFFSET.to_le_bytes());
        section[36..40].copy_from_slice(&0xc000_0040u32.to_le_bytes()); // init data R/W
        buf.extend_from_slice(&section);
        buf.resize(RAW_OFFSET as usize, 0);
        buf.extend_from_slice(&idata);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, &buf).unwrap();
    }

    /// The three builds Call of Duty 2's cabinet installs, plus the
    /// driver it carries for the GoForce one.
    fn write_cod2_install(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        write_pe_importing(&dir.join("cod2.exe"), &["coredll.dll"]);
        write_pe_importing(
            &dir.join("cod2_gles.exe"),
            &["coredll.dll", "libGLES_CL.dll"],
        );
        write_pe_importing(
            &dir.join("cod2_goforce.exe"),
            &["coredll.dll", "libGLES_CM.dll"],
        );
        fs::write(dir.join("libGLES_CM.dll"), b"MZ").unwrap();
    }

    /// The synthetic PEs above have to be readable by the same parser
    /// production code uses, or the tests below prove nothing.
    #[test]
    fn the_test_pe_builder_writes_an_import_directory_pocket_pe_can_read() {
        let dir = tmpdir("pe_builder");
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("game.exe");
        write_pe_importing(&exe, &["coredll.dll", "libGLES_CM.dll"]);
        let image = pocket_pe::load_file(&exe).expect("synthetic PE should parse");
        let dlls: Vec<String> = image
            .imports
            .iter()
            .map(|import| import.dll.to_ascii_lowercase())
            .collect();
        assert_eq!(dlls, ["coredll.dll", "libgles_cm.dll"]);
        assert!(is_guest_exe(&exe));
    }

    /// A cabinet that ships one build per 3D chip leaves the pick to its
    /// setup DLL, which PocketHLE never runs — so the shortcut target is
    /// the software build, and running it means rasterising in emulated
    /// ARM code instead of through the emulator's own GL ES.
    #[test]
    fn a_cabinet_that_ships_a_hardware_renderer_build_launches_it() {
        let dir = tmpdir("renderer_build_pick");
        write_cod2_install(&dir);
        assert_eq!(
            accelerated_renderer_build(&dir.join("cod2.exe")),
            Some(dir.join("cod2_goforce.exe")),
        );
    }

    /// Two accelerated builds, one bundled driver: the cabinet carries
    /// `libGLES_CM.dll` for the GoForce build, while the Intel 2700G
    /// build binds to a `libGLES_CL.dll` that only that device's ROM
    /// has. Both work under PocketHLE, so the bundled pairing decides
    /// and the choice stays deterministic.
    #[test]
    fn the_driver_the_cabinet_ships_decides_between_two_hardware_builds() {
        let dir = tmpdir("renderer_build_shipped_driver");
        write_cod2_install(&dir);
        fs::remove_file(dir.join("libGLES_CM.dll")).unwrap();
        fs::write(dir.join("libGLES_CL.dll"), b"MZ").unwrap();
        assert_eq!(
            accelerated_renderer_build(&dir.join("cod2.exe")),
            Some(dir.join("cod2_gles.exe")),
        );
    }

    /// The launchers all go through [`GameEntry::launch_path`], so a
    /// game imported before this fix — its recorded executable is still
    /// the shortcut target — starts accelerated without a re-import,
    /// while `GetModuleFileNameW` keeps reporting the installed name.
    #[test]
    fn a_library_entry_launches_the_hardware_renderer_build() {
        let root = tmpdir("renderer_build_entry");
        let mut entry = entry_with_install_dir(Some("\\Program Files\\COD2"));
        entry.id = "cod2soinstaller".to_string();
        entry.executable = PathBuf::from("extracted/cod2.exe");
        let extracted = root.join(entry.relative_dir()).join("extracted");
        write_cod2_install(&extracted);
        assert_eq!(entry.launch_path(&root), extracted.join("cod2_goforce.exe"));
        assert_eq!(
            entry.guest_exe_path().as_deref(),
            Some("\\Program Files\\COD2\\cod2.exe"),
        );
    }

    /// Everything else keeps the shortcut target: a build that already
    /// draws through a driver, a cabinet with a single executable, a
    /// sibling that wants a driver we do not implement, and an unrelated
    /// tool installed next to the game.
    #[test]
    fn a_shortcut_target_without_a_renderer_sibling_is_left_alone() {
        let dir = tmpdir("renderer_build_left_alone");
        write_cod2_install(&dir);
        assert_eq!(
            accelerated_renderer_build(&dir.join("cod2_goforce.exe")),
            None,
        );

        let single = tmpdir("renderer_build_single");
        fs::create_dir_all(&single).unwrap();
        write_pe_importing(&single.join("game.exe"), &["coredll.dll"]);
        assert_eq!(accelerated_renderer_build(&single.join("game.exe")), None);

        let foreign = tmpdir("renderer_build_foreign_driver");
        fs::create_dir_all(&foreign).unwrap();
        write_pe_importing(&foreign.join("game.exe"), &["coredll.dll"]);
        write_pe_importing(&foreign.join("game_d3dm.exe"), &["d3dm.dll"]);
        write_pe_importing(&foreign.join("tools3d.exe"), &["libGLES_CM.dll"]);
        assert_eq!(accelerated_renderer_build(&foreign.join("game.exe")), None);
    }

    #[test]
    fn a_dll_handed_in_as_a_game_is_rejected_as_a_library() {
        // The user picks `pegcards.dll` in the import dialog. It is a
        // real ARM PE the game needs, so the machine check passes and
        // the old importer accepted it as a game that then could not
        // start. The DLL characteristic bit has to be what decides.
        let root = tmpdir("import_dll_rejected");
        let src = tmpdir("import_dll_rejected_src");
        fs::create_dir_all(&src).unwrap();
        let dll = src.join("pegcards.dll");
        write_stub_pe(&dll, MACHINE_ARM, IMAGE_FILE_DLL);

        let mut lib = Library::open(&root).unwrap();
        match lib.import_exe(&dll) {
            Err(LibraryError::IsLibrary(name)) => assert_eq!(name, "pegcards.dll"),
            other => panic!("expected IsLibrary, got {other:?}"),
        }
        // Nothing may be left behind for a rejected import.
        assert!(!root.join("games").join("pegcards").exists());
    }

    #[test]
    fn importing_an_exe_brings_its_satellite_libraries_along() {
        // Solitaire's layout: the executable plus the resource-only DLL
        // holding every card face. The emulator resolves the DLL by
        // name in the executable's own directory, so the import has to
        // put them back in one directory or the game draws nothing.
        let root = tmpdir("import_exe_companions");
        let src = tmpdir("import_exe_companions_src");
        fs::create_dir_all(&src).unwrap();
        let exe = src.join("solitare.exe");
        write_stub_pe(&exe, MACHINE_ARM, 0);
        write_stub_pe(&src.join("pegcards.dll"), MACHINE_ARM, IMAGE_FILE_DLL);
        // An unrelated host DLL in the same folder must not come along.
        write_stub_pe(&src.join("msvcrt.dll"), MACHINE_X86, IMAGE_FILE_DLL);

        let mut lib = Library::open(&root).unwrap();
        let entry = lib.import_exe(&exe).unwrap().clone();
        assert_eq!(entry.id, "solitare");
        assert_eq!(
            entry.companions,
            vec![PathBuf::from("extracted/pegcards.dll")]
        );

        let extracted = root.join("games").join("solitare").join("extracted");
        assert!(extracted.join("solitare.exe").is_file());
        assert!(extracted.join("pegcards.dll").is_file());
        assert!(!extracted.join("msvcrt.dll").exists());
    }

    #[test]
    fn an_x86_exe_is_still_rejected_as_the_wrong_architecture() {
        let root = tmpdir("import_x86_rejected");
        let src = tmpdir("import_x86_rejected_src");
        fs::create_dir_all(&src).unwrap();
        let exe = src.join("setup.exe");
        write_stub_pe(&exe, MACHINE_X86, 0);

        let mut lib = Library::open(&root).unwrap();
        assert!(matches!(
            lib.import_exe(&exe),
            Err(LibraryError::NotArmPe(_))
        ));
    }

    #[test]
    fn recognizes_nsis_installer_as_an_importable_container() {
        let installer =
            Path::new("/home/.z/chat-uploads/My_Little_Tank_for_Pocket_PC_v1.1-5971193d8ec0.exe");
        if installer.is_file() {
            assert!(is_nsis_installer(installer));
        }
    }

    #[test]
    fn guest_exe_and_dll_sniffing_disagree_only_on_the_dll_bit() {
        let dir = tmpdir("sniff_dll_bit");
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("game.exe");
        let dll = dir.join("cards.dll");
        write_stub_pe(&exe, MACHINE_ARM, 0);
        write_stub_pe(&dll, MACHINE_ARM, IMAGE_FILE_DLL);

        assert!(is_guest_exe(&exe));
        assert!(!is_guest_dll(&exe));
        assert!(is_guest_dll(&dll));
        // The important half: a satellite DLL must never be picked as
        // an entry point by the archive scans.
        assert!(!is_guest_exe(&dll));

        // A truncated file is neither.
        let stub = dir.join("tiny.exe");
        fs::write(&stub, b"MZ").unwrap();
        assert!(!is_guest_exe(&stub));
        assert!(!is_guest_dll(&stub));
    }

    #[test]
    fn resource_companion_cabs_are_selected_by_game_family() {
        let root = tmpdir("resource_companion_selection");
        fs::create_dir_all(&root).unwrap();
        for name in ["wwp.CAB", "wwp-res.CAB", "wwp-2003.CAB", "other-res.CAB"] {
            fs::write(root.join(name), []).unwrap();
        }
        let found = find_resource_companion_cabs(&root.join("wwp.CAB"));
        assert_eq!(found, vec![root.join("wwp-res.CAB")]);
    }

    #[test]
    fn native_resolution_uses_the_resource_value_and_preserves_explicit_overrides() {
        use pocket_pe::resources::{ResourceEntry, ResourceKey};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("any-name.exe");
        write_pe_importing(&path, &[]);
        let mut image = pocket_pe::load_file(&path).unwrap();
        assert_eq!(ScreenPref::Portrait.resolve_image(Some(&image)), (240, 320));
        let resource = ResourceEntry {
            ty: ResourceKey::Name("CEUX".into()),
            name: ResourceKey::Name("HI_RES_AWARE".into()),
            data_rva: image.sections[0].virtual_address,
            size: 2,
            codepage: 0,
        };
        image.resources.push(resource);
        image.sections[0].data[..2].copy_from_slice(&[1, 0]);
        assert_eq!(ScreenPref::Portrait.resolve_image(Some(&image)), (480, 640));
        for pref in [
            ScreenPref::Qvga,
            ScreenPref::Landscape,
            ScreenPref::VgaLandscape,
            ScreenPref::Hpc,
            ScreenPref::Wvga,
        ] {
            assert_eq!(pref.resolve_image(Some(&image)), pref.size());
        }
        image.sections[0].data[..2].copy_from_slice(&[0, 0]);
        assert_eq!(ScreenPref::Portrait.resolve_image(Some(&image)), (240, 320));
        assert_eq!(ScreenPref::Portrait.resolve_image(None), (240, 320));
    }

    #[test]
    fn the_handheld_pc_screen_preference_is_480x320() {
        assert_eq!(ScreenPref::Hpc.size(), (480, 320));
        // Round-trips through the manifest, so an existing library
        // keeps the setting across a restart.
        let json = serde_json::to_string(&ScreenPref::Hpc).unwrap();
        assert_eq!(
            serde_json::from_str::<ScreenPref>(&json).unwrap(),
            ScreenPref::Hpc
        );
    }
}

impl LauncherConfig {
    /// One fix per call: used at launch and for explicit Eiffel-mode refresh only.
    pub fn simulated_gps_position(&self) -> Option<(f64, f64)> {
        if !self.gps_fixed_enabled {
            return None;
        }
        if !self.gps_fixed_random {
            return Some((self.gps_fixed_latitude, self.gps_fixed_longitude));
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Some(eiffel_position(seed))
    }
}

fn eiffel_position(mut seed: u64) -> (f64, f64) {
    fn unit(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = *seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / ((1u64 << 53) as f64)
    }
    let radius = 800.0 * unit(&mut seed).sqrt();
    let bearing = std::f64::consts::TAU * unit(&mut seed);
    let angular = radius / 6_371_000.0;
    let lat = 48.8584_f64.to_radians();
    let lon = 2.2945_f64.to_radians();
    let target_lat = (lat.sin() * angular.cos() + lat.cos() * angular.sin() * bearing.cos()).asin();
    let target_lon = lon
        + (bearing.sin() * angular.sin() * lat.cos())
            .atan2(angular.cos() - lat.sin() * target_lat.sin());
    (target_lat.to_degrees(), target_lon.to_degrees())
}

#[cfg(test)]
mod simulated_gps_tests {
    use super::*;
    #[test]
    fn eiffel_fixes_stay_inside_800_metres_and_manual_values_survive() {
        for seed in 0..1000 {
            let (lat, lon) = eiffel_position(seed);
            let dlat = (lat - 48.8584).to_radians();
            let dlon = (lon - 2.2945).to_radians();
            let a = (dlat / 2.).sin().powi(2)
                + lat.to_radians().cos()
                    * 48.8584_f64.to_radians().cos()
                    * (dlon / 2.).sin().powi(2);
            assert!(2. * 6_371_000. * a.sqrt().asin() <= 800.0001);
        }
        let mut c = LauncherConfig::default();
        assert!(c.simulated_gps_position().is_none());
        c.gps_fixed_enabled = true;
        c.gps_fixed_latitude = 12.;
        c.gps_fixed_longitude = 34.;
        assert_eq!(c.simulated_gps_position(), Some((12., 34.)));
    }
}

/// Use a real label BMP beside the game binary. Reject oversized inputs before decoding.
fn cartridge_artwork(game_dir: &Path, exe: &Path) -> Option<PathBuf> {
    // CoPilot can select a bootstrap EXE above its GZAP application folder,
    // or an EXE below the card BMP. Search only that install's ancestors and
    // a unique immediate GZAP folder; never borrow art from another app card.
    let extracted = game_dir.join("extracted");
    let mut directories = vec![exe.parent()?.to_path_buf()];
    if exe.starts_with(&extracted) {
        let mut parent = exe.parent()?;
        while parent != extracted {
            parent = parent.parent()?;
            directories.push(parent.to_path_buf());
        }
    }
    let mut candidates = Vec::new();
    for directory in directories {
        let mut card_dirs = Vec::new();
        let mut bitmaps = Vec::new();
        for entry in fs::read_dir(&directory)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
            if path.is_dir() && name.starts_with("GZAP") && is_gizmondo_title_id(&name) {
                card_dirs.push(path);
            } else if path.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("bmp"))
            {
                bitmaps.push(path);
            }
        }
        bitmaps.sort();
        candidates.extend(bitmaps);
        if card_dirs.len() == 1 {
            let mut card_bitmaps: Vec<_> = fs::read_dir(&card_dirs[0])
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && path
                            .extension()
                            .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("bmp"))
                })
                .collect();
            card_bitmaps.sort();
            candidates.extend(card_bitmaps);
        }
    }
    for path in candidates {
        if fs::metadata(&path).ok()?.len() > 16 * 1024 * 1024 {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if bytes.len() < 26 || &bytes[..2] != b"BM" {
            continue;
        }
        let width = i32::from_le_bytes(bytes[18..22].try_into().ok()?).unsigned_abs();
        let height = i32::from_le_bytes(bytes[22..26].try_into().ok()?).unsigned_abs();
        if width == 0 || height == 0 || width > 2048 || height > 2048 {
            continue;
        }
        let Ok(image) = image::load_from_memory_with_format(&bytes, image::ImageFormat::Bmp) else {
            continue;
        };
        if image
            .save_with_format(game_dir.join("cartridge.png"), image::ImageFormat::Png)
            .is_err()
        {
            continue;
        }
        return Some(PathBuf::from("cartridge.png"));
    }
    None
}

fn import_tree_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    fn visit(root: &Path, depth: usize, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        if depth > 32 {
            return Ok(());
        }
        for item in fs::read_dir(root)? {
            let item = item?;
            let kind = item.file_type()?;
            if kind.is_dir() {
                visit(&item.path(), depth + 1, files)?;
            } else if kind.is_file() {
                files.push(item.path());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, 0, &mut files)?;
    files.sort();
    Ok(files)
}

impl LauncherConfig {
    pub fn preset_text(&self, gizmondo: bool) -> Result<String, String> {
        let hex = self.device_identity_hex(gizmondo).0;
        if hex.len() % 2 != 0 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid stored Preset ID".into());
        }
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        if gizmondo {
            if bytes.len() % 2 != 0 {
                return Err("Preset ID is not UTF-16LE text".into());
            }
            let mut words: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            if words.last() == Some(&0) {
                words.pop();
            }
            if words.contains(&0) {
                return Err("Preset ID contains an embedded NUL".into());
            }
            String::from_utf16(&words).map_err(|_| "Preset ID is not UTF-16LE text".into())
        } else {
            if !bytes.is_ascii() || bytes.contains(&0) {
                return Err("Preset ID is not ASCII text".into());
            }
            String::from_utf8(bytes).map_err(|e| e.to_string())
        }
    }

    pub fn set_preset_text(&mut self, gizmondo: bool, text: &str) -> Result<(), String> {
        if text.contains('\0') {
            return Err("Enter a device name without NUL characters".into());
        }
        let bytes: Vec<u8> = if gizmondo {
            text.encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            if !text.is_ascii() {
                return Err("The WinCE device name must contain ASCII characters".into());
            }
            text.as_bytes().to_vec()
        };
        if bytes.len() > 4096 {
            return Err("Device name is too long".into());
        }
        let hex = bytes.iter().map(|b| format!("{b:02X}")).collect();
        if gizmondo {
            self.gizmondo_preset_id = hex;
        } else {
            self.device_preset_id = hex;
        }
        Ok(())
    }

    /// Gizmondo IDs replace, rather than merge with, the general WinCE profile.
    pub fn device_identity_hex(&self, gizmondo: bool) -> (&str, &str) {
        if gizmondo {
            (&self.gizmondo_preset_id, &self.gizmondo_platform_id)
        } else {
            (&self.device_preset_id, &self.device_platform_id)
        }
    }
}

#[cfg(test)]
mod device_identity_config_tests {
    use super::*;
    #[test]
    fn readable_presets_roundtrip_without_changing_guest_bytes() {
        let mut c = LauncherConfig::default();
        assert_eq!(c.preset_text(true).unwrap(), "Gizmondo");
        assert_eq!(c.preset_text(false).unwrap(), "PocketHLE-WinCE");
        c.set_preset_text(false, "").unwrap();
        assert_eq!(c.preset_text(false).unwrap(), "");
        c.set_preset_text(false, "PocketHLE-WinCE").unwrap();
        let original = c.gizmondo_preset_id.clone();
        c.set_preset_text(true, "Gizmondo").unwrap();
        assert_eq!(c.gizmondo_preset_id, original);
        c.set_preset_text(true, "Console é 🚗").unwrap();
        assert_eq!(c.preset_text(true).unwrap(), "Console é 🚗");
        assert!(c.set_preset_text(false, "é").is_err());
        assert!(c.set_preset_text(true, "bad\0name").is_err());
        c.gizmondo_preset_id = "00D8".into();
        assert!(c.preset_text(true).is_err());
    }

    #[test]
    fn gizmondo_dump_default_migrates_once_without_changing_wince_or_future_edits() {
        let dir = tempfile::tempdir().unwrap();
        let config = serde_json::json!({"device_platform_id":"00112233445566778899AABBCCDDEEFF", "gizmondo_platform_id":"112233445566778899AABBCCDDEEFF00"});
        std::fs::write(dir.path().join(CONFIG_FILE_NAME), config.to_string()).unwrap();
        let mut lib = Library::open(dir.path()).unwrap();
        assert_eq!(
            lib.config().gizmondo_platform_id,
            "AD0D71F3E65E5B9A47E4773478FA926B"
        );
        assert_eq!(
            lib.config().device_platform_id,
            "00112233445566778899AABBCCDDEEFF"
        );
        lib.config_mut().gizmondo_platform_id = "ABCDEF".into();
        lib.save().unwrap();
        assert_eq!(
            Library::open(dir.path())
                .unwrap()
                .config()
                .gizmondo_platform_id,
            "ABCDEF"
        );
    }

    #[test]
    fn old_config_defaults_and_profile_overrides_survive_roundtrip() {
        let mut c: LauncherConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(c.gizmondo_preset_id, "470069007A006D006F006E0064006F000000");
        assert!(c.device_platform_id.is_empty());
        c.device_platform_id = "01020304".into();
        c.gizmondo_platform_id = "AABBCCDD".into();
        let c: LauncherConfig = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(
            c.device_identity_hex(false),
            (c.device_preset_id.as_str(), "01020304")
        );
        assert_eq!(
            c.device_identity_hex(true),
            (c.gizmondo_preset_id.as_str(), "AABBCCDD")
        );
    }
}
