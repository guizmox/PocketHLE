//! egui application: library screen, settings, per-game sheet.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, RichText, ScrollArea, Sense, Vec2};
use egui_extras::image::load_image_bytes;

use pocket_core::kernel::{InputEvent, FB_HEIGHT, FB_WIDTH};
use pocket_library::{
    is_gizmondo_game, CpuBackendPref, GameEntry, GameSettings, GuestButton, LauncherConfig, Library,
    RotationPref, ScreenPref,
};

use crate::runner::{FrameSnapshot, InputCommand, RunOutcome, Runner};

// Virtual button layout for the Run screen — modelled after the
// j2me-loader gamepad: a D-pad on the left and three action buttons
// (A / B / Start) on the right. Pressing a button sends a
// `WM_KEYDOWN`/`WM_KEYUP` pair down to the guest.
//
// Which virtual-key code each button sends lives on
// [`pocket_library::GuestButton`], shared with the Android pad and with
// the keybinding editor, and is pinned against
// `pocket_core::kernel::gapi` — the table `gx.dll`'s `GXGetDefaultKeys`
// hands the guest — by `guest_button_vks_match_the_gapi_table` below.

/// Top-level egui app.
pub struct PocketLauncher {
    icon_cache: std::collections::HashMap<String, egui::TextureHandle>,
    analysis_cache: std::collections::HashMap<String, GameAnalysis>,
    gizmondo_skin: Option<egui::TextureHandle>,
    library: Library,
    selected_game: Option<String>,
    screen: Screen,
    runner: Runner,
    events_rx: Receiver<UiEvent>,
    events_tx: Sender<UiEvent>,
    /// Live framebuffer updates streamed by [`Runner`] while a game
    /// is running. `Some` between [`Self::spawn_run`] and
    /// `UiEvent::RunFinished`; `None` otherwise.
    frame_rx: Option<Receiver<FrameSnapshot>>,
    /// Channel for sending [`InputCommand`]s (taps / D-pad / stop)
    /// to the running emulator. Mirrors `frame_rx`.
    input_tx: Option<Sender<InputCommand>>,
    /// Which guest buttons are held right now, and by which input
    /// source, so we can fire the matching `WM_KEYUP` on release.
    held: HeldButtons,
    /// `Some` while a stylus drag is in progress — carries the last
    /// reported game-space coordinates so we don't spam the guest
    /// with redundant events.
    pointer_down_at: Option<(u16, u16)>,
    /// Game currently being launched, used as a status caption
    /// while the run is in progress.
    running_game: Option<String>,
    status: String,
    config_draft: Option<LauncherConfig>,
    game_settings_draft: Option<(String, GameSettings)>,
    last_frame_texture: Option<egui::TextureHandle>,
    /// Latest unscaled guest framebuffer, retained for pixel-perfect PNG screenshots.
    last_frame_snapshot: Option<FrameSnapshot>,
    last_frame_status: Option<String>,
    frame_stats: FrameStats,
    /// How far the presented frame is turned. Initialised from the
    /// running game's persisted [`RotationPref`] and editable live on
    /// the Run screen, where a change is written straight back to the
    /// game's settings so the next launch starts turned the same way.
    game_rotation: RotationPref,
    /// Id of the game currently on the Run screen, so a rotation
    /// change made there knows which `game.json` to write.
    running_game_id: Option<String>,
    /// True while the current title is a Gizmondo game.
    running_is_gizmondo: bool,
    /// Guest button waiting for the user to press the host key that
    /// should drive it, while the keybinding editor is in "press a
    /// key" mode.
    binding_capture: Option<GuestButton>,
}

/// Texture corners for a rotated presentation, in
/// `[left-top, right-top, left-bottom, right-bottom]` order.
///
/// Turning the *presented* frame rather than the guest's own geometry
/// is what a landscape game that ships as a 240x320 portrait build
/// needs: it keeps rendering into the panel it was written for and the
/// user still sees it the right way up. See
/// [`pocket_library::RotationPref`].
fn rotation_uv(rotation: RotationPref) -> [Pos2; 4] {
    match rotation {
        RotationPref::None => [
            egui::pos2(0.0, 0.0),
            egui::pos2(1.0, 0.0),
            egui::pos2(0.0, 1.0),
            egui::pos2(1.0, 1.0),
        ],
        RotationPref::Cw90 => [
            egui::pos2(0.0, 1.0),
            egui::pos2(0.0, 0.0),
            egui::pos2(1.0, 1.0),
            egui::pos2(1.0, 0.0),
        ],
        RotationPref::Half => [
            egui::pos2(1.0, 1.0),
            egui::pos2(0.0, 1.0),
            egui::pos2(1.0, 0.0),
            egui::pos2(0.0, 0.0),
        ],
        RotationPref::Ccw90 => [
            egui::pos2(1.0, 0.0),
            egui::pos2(1.0, 1.0),
            egui::pos2(0.0, 0.0),
            egui::pos2(0.0, 1.0),
        ],
    }
}


#[derive(Default, Clone)]
struct GameAnalysis {
    platform: String,
    title_id: Option<String>,
    identity: Vec<(String, String)>,
    executable: Vec<(String, String)>,
    graphics: Vec<(String, String)>,
    hardware: Vec<(String, String)>,
    game_data: Vec<(String, String)>,
    languages: Vec<(String, String)>,
    evidence: Vec<String>,
}

fn detail_section(ui: &mut egui::Ui, title: &str, rows: &[(String, String)]) {
    if rows.is_empty() { return; }
    ui.separator();
    ui.label(RichText::new(title).strong().size(12.0).color(Color32::from_gray(150)));
    egui::Grid::new(format!("detail-{title}")).num_columns(2).spacing(Vec2::new(12.0, 4.0)).show(ui, |ui| {
        for (label, value) in rows {
            ui.label(RichText::new(label).color(Color32::from_gray(165)));
            if value == "✓ Detected" || value == "✓ Resource detected" {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(13.0, 13.0), egui::Sense::hover());
                    let stroke = egui::Stroke::new(1.7_f32, Color32::from_rgb(55, 145, 215));
                    let a = Pos2::new(rect.left() + 2.0, rect.center().y);
                    let b = Pos2::new(rect.left() + 5.0, rect.bottom() - 3.0);
                    let c = Pos2::new(rect.right() - 1.5, rect.top() + 2.5);
                    ui.painter().line_segment([a, b], stroke);
                    ui.painter().line_segment([b, c], stroke);
                    ui.label(value.trim_start_matches('✓').trim());
                });
            } else {
                ui.label(value);
            }
            ui.end_row();
        }
    });
    ui.add_space(5.0);
}

fn draw_gizmondo_sd(ui: &egui::Ui, area: Rect, title: &str, title_id: Option<&str>) {
    let p=ui.painter();
    let h=(area.height()-8.0).min(118.0);
    let w=h*0.72;
    let r=Rect::from_center_size(area.center(), Vec2::new(w,h));
    let blue=Color32::from_rgb(24,65,151);
    let edge=Color32::from_rgb(14,42,105);

    // Front-facing SD silhouette. The clipped top-right corner is the only
    // decorative geometry; the game label itself stays deliberately plain.
    let cut=13.0;
    let pts=vec![
        r.left_top(), Pos2::new(r.right()-cut,r.top()), r.right_top()+Vec2::new(0.0,cut),
        r.right_bottom(), r.left_bottom(),
    ];
    p.add(egui::Shape::convex_polygon(pts,blue,egui::Stroke::new(1.5_f32,edge)));

    // Label fills the complete recessed rectangle: white from its very top
    // down to the black Gizmondo band. No invented cover artwork/background.
    let label=Rect::from_min_max(Pos2::new(r.left()+8.0,r.top()+16.0),Pos2::new(r.right()-8.0,r.bottom()-9.0));
    p.rect_filled(label,5.0,Color32::WHITE);
    p.rect_stroke(label,5.0,egui::Stroke::new(1.0_f32,Color32::from_gray(175)));
    let band_h=22.0;
    let band=Rect::from_min_max(Pos2::new(label.left(),label.bottom()-band_h),label.right_bottom());
    p.rect_filled(band,0.0,Color32::from_rgb(18,18,20));
    p.text(band.center(),egui::Align2::CENTER_CENTER,"GIZMONDO",egui::FontId::proportional(12.0),Color32::WHITE);

    let id=title_id.unwrap_or("");
    let content=Rect::from_min_max(label.left_top(),Pos2::new(label.right(),band.top()));
    let shown=if title.trim().is_empty() { id } else { title };
    let title_y=content.top()+content.height()*0.38;
    // Keep the title strictly inside the white label. Multi-word titles use two
    // lines; single long words/IDs are scaled down instead of overflowing.
    let words:Vec<_>=shown.split_whitespace().collect();
    let (l1,l2)=if words.len()>1 {
        let mid=(words.len()+1)/2; (words[..mid].join(" "),words[mid..].join(" "))
    } else {(shown.to_string(),String::new())};
    let longest = l1.chars().count().max(l2.chars().count());
    let title_font = match longest {
        0..=8 => 14.0,
        9..=10 => 11.0,
        11..=13 => 9.5,
        14..=16 => 8.5,
        _ => 7.5,
    };
    p.text(Pos2::new(content.center().x,title_y),egui::Align2::CENTER_CENTER,l1,egui::FontId::proportional(title_font),Color32::BLACK);
    if !l2.is_empty() { p.text(Pos2::new(content.center().x,title_y+16.0),egui::Align2::CENTER_CENTER,l2,egui::FontId::proportional(title_font),Color32::BLACK); }
    if !id.is_empty() { p.text(Pos2::new(content.center().x,content.bottom()-11.0),egui::Align2::CENTER_CENTER,id,egui::FontId::monospace(9.5),Color32::from_gray(45)); }
}

fn detect_gizmondo_title_id(root: &std::path::Path) -> Option<String> {
    fn scan(dir: &std::path::Path, depth: usize) -> Option<String> {
        if depth > 5 { return None; }
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p=e.path();
            if !p.is_dir() { continue; }
            if let Some(n)=p.file_name().and_then(|n| n.to_str()) {
                let b=n.as_bytes();
                if b.len()==10 && b[..2].eq_ignore_ascii_case(b"GZ") && b[2..4].iter().all(|c| c.is_ascii_alphabetic()) && b[4..].iter().all(|c| c.is_ascii_digit()) && p.join(n).is_file() { return Some(n.to_ascii_uppercase()); }
            }
            if let Some(v)=scan(&p, depth+1) { return Some(v); }
        }
        None
    }
    scan(root,0)
}

fn pe_timestamp(bytes: &[u8]) -> Option<u32> {
    if bytes.len()<0x40 || &bytes[..2] != b"MZ" { return None; }
    let o=u32::from_le_bytes(bytes[0x3c..0x40].try_into().ok()?) as usize;
    if o+12>bytes.len() || &bytes[o..o+4] != b"PE\0\0" { return None; }
    Some(u32::from_le_bytes(bytes[o+8..o+12].try_into().ok()?))
}

fn civil_from_days(z: i64) -> (i64,u32,u32) {
    let z=z+719468; let era=if z>=0 {z} else {z-146096}/146097; let doe=z-era*146097;
    let yoe=(doe-doe/1460+doe/36524-doe/146096)/365; let mut y=yoe+era*400;
    let doy=doe-(365*yoe+yoe/4-yoe/100); let mp=(5*doy+2)/153; let d=doy-(153*mp+2)/5+1;
    let m=mp+if mp<10 {3} else {-9}; y += if m<=2 {1} else {0}; (y,m as u32,d as u32)
}
fn format_unix_utc(ts:u32)->String { let s=ts as i64; let days=s/86400; let rem=s%86400; let (y,m,d)=civil_from_days(days); format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",rem/3600,(rem%3600)/60,rem%60) }

fn binary_strings(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for &b in bytes {
        if b.is_ascii_graphic() || b == b' ' || b == b'\\' {
            cur.push(b);
        } else {
            if cur.len() >= 4 { out.push(String::from_utf8_lossy(&cur).into_owned()); }
            cur.clear();
        }
    }
    if cur.len() >= 4 { out.push(String::from_utf8_lossy(&cur).into_owned()); }

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
                if chars.len() >= 4 { out.push(chars.iter().collect()); }
                chars.clear();
            }
            i += 2;
        }
        if chars.len() >= 4 { out.push(chars.iter().collect()); }
    }
    out
}

fn version_value(strings: &[String], key: &str) -> Option<String> {
    for (i, s) in strings.iter().enumerate() {
        if s.eq_ignore_ascii_case(key) {
            for v in strings.iter().skip(i + 1).take(5) {
                let v = v.trim_matches(char::from(0)).trim();
                if !v.is_empty() && !v.eq_ignore_ascii_case(key) && !v.starts_with("VarFileInfo") && !v.starts_with("StringFileInfo") {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn find_title_id_in_strings(strings: &[String]) -> Option<String> {
    for s in strings {
        let b = s.as_bytes();
        for w in b.windows(10) {
            if w[..2].eq_ignore_ascii_case(b"GZ") && w[2..4].iter().all(|c| c.is_ascii_alphabetic()) && w[4..].iter().all(|c| c.is_ascii_digit()) {
                return Some(String::from_utf8_lossy(w).to_ascii_uppercase());
            }
        }
    }
    None
}

fn first_matching_string(strings: &[String], pred: impl Fn(&str) -> bool) -> Option<String> {
    strings.iter().find(|s| pred(&s.to_ascii_lowercase())).map(|s| s.trim().to_string())
}

fn analyze_game(game:&GameEntry, root:&std::path::Path)->GameAnalysis {
    let mut a=GameAnalysis::default();
    let giz=is_gizmondo_game(game,root);
    a.platform=if giz {"Gizmondo".into()} else {"Pocket PC / Windows CE".into()};
    let path=game.executable_path(root);
    let bytes=std::fs::read(&path).unwrap_or_default();
    let strings=binary_strings(&bytes);
    let text=strings.join(" ").to_ascii_lowercase();
    a.title_id=detect_gizmondo_title_id(&game.extracted_dir(root)).or_else(|| find_title_id_in_strings(&strings));

    // PE VERSIONINFO. Keep file/product versions separate and avoid using
    // FileDescription/InternalName as the library title: prototype builds often
    // carry stale SDK/template metadata (Battlestations famously contains Keys).
    for (key,label) in [("CompanyName","Developer / company"),("LegalCopyright","Copyright")] {
        if let Some(v)=version_value(&strings,key) { a.identity.push((label.into(),v.clone())); a.evidence.push(format!("VERSIONINFO {key}: {v}")); }
    }
    for (key,label) in [("FileVersion","File version"),("ProductVersion","Product version")] {
        if let Some(v)=version_value(&strings,key) { a.identity.push((label.into(),v.clone())); a.evidence.push(format!("VERSIONINFO {key}: {v}")); }
    }

    a.executable.push(("File".into(), path.file_name().unwrap_or_default().to_string_lossy().into_owned()));
    if let Ok(img)=pocket_pe::load_file(&path) {
        a.executable.push(("Architecture".into(), img.machine_name().into()));
        a.executable.push(("Format".into(), "PE32 / Windows CE".into()));
        let dlls:std::collections::BTreeSet<_>=img.imports.iter().map(|i| i.dll.to_ascii_lowercase()).collect();
        if dlls.iter().any(|d| d.contains("gles")) {
            a.graphics.push(("Renderer".into(),"OpenGL ES / EGL".into()));
            a.evidence.push("Import: libGLES_CM.dll (OpenGL ES / EGL)".into());
        }
        if dlls.iter().any(|d| d.contains("fmod")) || text.contains("fmodce.dll") {
            a.game_data.push(("Audio middleware".into(),"FMOD CE".into()));
            a.evidence.push("Audio: fmodce.dll / FMOD symbols".into());
        }
        a.evidence.push(format!("PE imports: {} entries analysed", img.imports.len()));
    }
    if let Some(ts)=pe_timestamp(&bytes).filter(|v| *v>315532800 && *v<4102444800) {
        a.executable.push(("PE build timestamp".into(),format_unix_utc(ts)));
    }
    if let Some(p)=game.provider.as_ref().filter(|s|!s.trim().is_empty()) { a.executable.push(("Provider".into(),p.clone())); }
    if giz { a.graphics.push(("Native display".into(),"320 × 240 • Landscape".into())); }
    if text.contains("eglcreatewindowsurface") || text.contains("eglinitialize") {
        if !a.graphics.iter().any(|(k,_)|k=="Renderer") { a.graphics.push(("Renderer".into(),"OpenGL ES / EGL".into())); }
        for api in ["eglInitialize","eglCreateWindowSurface","eglSwapBuffers","glDrawElements","glCompressedTexImage2D"] {
            if text.contains(&api.to_ascii_lowercase()) { a.evidence.push(format!("Graphics symbol: {api}")); }
        }
    }
    if text.contains("vib1:") || text.contains("vibrator") {
        a.hardware.push(("Vibration".into(),"✓ Detected".into()));
        a.evidence.push(if text.contains("vib1:") {"Hardware string: VIB1:".into()} else {"Hardware string: vibrator".into()});
    }
    if text.contains("controlpanel\\backlight") || text.contains("backlightchangeevent") {
        a.hardware.push(("Backlight".into(),"✓ Detected".into()));
        a.evidence.push("Backlight API/resource detected".into());
    }
    if text.contains("lowbatery_msg") || text.contains("criticalbatery_msg") || text.contains("lowbattery") {
        a.hardware.push(("Battery events".into(),"✓ Detected".into()));
        a.evidence.push("Battery notification strings detected".into());
    }
    if giz { a.hardware.insert(0,("Controls".into(),"Gizmondo front panel".into())); }

    if let Some(save)=first_matching_string(&strings, |s| s.contains("\\flash disk\\mygames")) {
        a.game_data.push(("Save support".into(),"✓ Detected".into()));
        a.game_data.push(("Save path".into(),save.clone()));
        a.evidence.push(format!("Save path: {save}"));
    } else if text.contains(".sav") || text.contains("defaultprofile.dat") {
        a.game_data.push(("Save support".into(),"✓ Detected".into()));
    }
    if let Some(cfg)=first_matching_string(&strings, |s| s.ends_with(".cfg") || s.contains(".cfg ")) {
        a.game_data.push(("Configuration".into(),"✓ Detected".into())); a.evidence.push(format!("Config resource: {cfg}"));
    }
    for (needle,label) in [("\\maps\\","Maps"),("\\textures\\","Textures"),("\\sound\\","Audio resources")] {
        if text.contains(needle) { a.game_data.push((label.into(),"✓ Detected".into())); }
    }
    for (needles,label) in [(["choisissez une langue","french.tga"],"French"),(["wählen sie eine sprache","german.tga"],"German"),(["selezioni una lingua","italian.tga"],"Italian"),(["select a language","english.tga"],"English"),(["spanish.tga","español"],"Spanish")] {
        if needles.iter().any(|n| text.contains(n)) { a.languages.push((label.into(),"✓ Resource detected".into())); a.evidence.push(format!("Language resource: {label}")); }
    }
    if let Some(id)=&a.title_id { a.evidence.push(format!("Gizmondo Title ID: {id}")); }
    a
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Library,
    Settings,
    GameSettings,
    Run,
}

/// Where a held guest button came from.
///
/// The on-screen pad and the physical keyboard can hold the same VK at
/// the same time, so [`HeldButtons`] keeps one set per source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputSource {
    Keyboard,
    Pointer,
}

/// Which guest buttons are down, tracked per input source.
///
/// Both sources used to share a single `HashSet<u16>`, and that quietly
/// broke holding a key: [`PocketLauncher::vbutton`] runs every frame and
/// released whatever it found in the set whenever the pointer was not on
/// its own rect, so a keyboard arrow the user was still holding was
/// cancelled with a `WM_KEYUP` in the very next frame. JumpyBall steers
/// its ball for as long as the direction key is down, so on the desktop
/// the ball stopped moving the moment the user had touched the on-screen
/// D-pad once. Keeping the sources apart — and only telling the guest a
/// button came up once *no* source holds it — is what lets a hold on
/// either one last as long as the user holds it.
#[derive(Debug, Default)]
struct HeldButtons {
    keyboard: std::collections::HashSet<u16>,
    pointer: std::collections::HashSet<u16>,
}

impl HeldButtons {
    fn set(&mut self, source: InputSource) -> &mut std::collections::HashSet<u16> {
        match source {
            InputSource::Keyboard => &mut self.keyboard,
            InputSource::Pointer => &mut self.pointer,
        }
    }

    /// Record a press. Returns whether the guest should be told the
    /// button went down — only when no source was holding it yet, so a
    /// key pressed while the same button is held on the pad does not
    /// become a second `WM_KEYDOWN` (which a game like JumpyBall would
    /// act on as another menu step).
    fn press(&mut self, source: InputSource, vk: u16) -> bool {
        let was_held = self.is_held(vk);
        self.set(source).insert(vk);
        !was_held
    }

    /// Record a release. Returns whether the guest should be told the
    /// button came up — only once the other source has let go too.
    fn release(&mut self, source: InputSource, vk: u16) -> bool {
        let was_held = self.set(source).remove(&vk);
        was_held && !self.is_held(vk)
    }

    fn is_held(&self, vk: u16) -> bool {
        self.keyboard.contains(&vk) || self.pointer.contains(&vk)
    }

    fn is_held_by(&self, source: InputSource, vk: u16) -> bool {
        match source {
            InputSource::Keyboard => self.keyboard.contains(&vk),
            InputSource::Pointer => self.pointer.contains(&vk),
        }
    }

    /// Every VK still held, forgetting all of them. Used when the window
    /// is closing so the guest is not left with a stuck key.
    fn drain_all(&mut self) -> Vec<u16> {
        self.keyboard
            .drain()
            .chain(self.pointer.drain())
            // A VK held on both at once must only be released once.
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[derive(Debug)]
pub enum UiEvent {
    ImportFinished(Result<String, String>),
    RunFinished(RunOutcome),
}

#[derive(Debug, Clone)]
struct FrameStats {
    displayed_frames: u64,
    fps: f32,
    window_started_at: Option<Instant>,
    window_frames: u32,
    last_frame_at: Option<Instant>,
    last_frame_ms: Option<f32>,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            displayed_frames: 0,
            fps: 0.0,
            window_started_at: None,
            window_frames: 0,
            last_frame_at: None,
            last_frame_ms: None,
        }
    }
}

impl FrameStats {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn record_frame(&mut self) {
        let now = Instant::now();
        if let Some(previous) = self.last_frame_at {
            self.last_frame_ms = Some(now.duration_since(previous).as_secs_f32() * 1000.0);
        }
        self.last_frame_at = Some(now);
        self.displayed_frames = self.displayed_frames.saturating_add(1);
        self.window_frames = self.window_frames.saturating_add(1);

        let started_at = match self.window_started_at {
            Some(t) => t,
            None => {
                self.window_started_at = Some(now);
                now
            }
        };
        let elapsed = now.duration_since(started_at);
        if elapsed >= Duration::from_secs(1) {
            self.fps = self.window_frames as f32 / elapsed.as_secs_f32();
            self.window_frames = 0;
            self.window_started_at = Some(now);
        }
    }

    fn overlay_text(&self) -> String {
        let last_ms = self.last_frame_ms.unwrap_or(0.0);
        let since_ms = self
            .last_frame_at
            .map(|t| t.elapsed().as_secs_f32() * 1000.0)
            .unwrap_or(0.0);
        format!(
            "FPS {:.1}  Frames {}  Last {:.0}ms  Since {:.0}ms",
            self.fps, self.displayed_frames, last_ms, since_ms
        )
    }
}

impl PocketLauncher {
    pub fn new(_cc: &eframe::CreationContext<'_>, library: Library) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            library,
            icon_cache: std::collections::HashMap::new(),
            analysis_cache: std::collections::HashMap::new(),
            gizmondo_skin: None,
            selected_game: None,
            screen: Screen::Library,
            runner: Runner::new(),
            events_rx: rx,
            events_tx: tx,
            frame_rx: None,
            input_tx: None,
            held: HeldButtons::default(),
            pointer_down_at: None,
            running_game: None,
            status: "Welcome to PocketHLE.".to_string(),
            config_draft: None,
            game_settings_draft: None,
            last_frame_texture: None,
            last_frame_snapshot: None,
            last_frame_status: None,
            frame_stats: FrameStats::default(),
            game_rotation: RotationPref::None,
            running_game_id: None,
            running_is_gizmondo: false,
            binding_capture: None,
        }
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.events_rx.try_recv() {
            match ev {
                UiEvent::ImportFinished(Ok(name)) => {
                    self.status = format!("Imported {name}.");
                    self.reload_library();
                }
                UiEvent::ImportFinished(Err(e)) => {
                    self.status = format!("Import failed: {e}");
                }
                UiEvent::RunFinished(outcome) => {
                    self.last_frame_status = Some(outcome.summary.clone());
                    if let Some(frame) = outcome.framebuffer {
                        self.upload_frame_texture(ctx, &frame);
                    }
                    self.status = outcome.summary;
                    self.frame_rx = None;
                    self.input_tx = None;
                    self.release_all_keys();
                    self.pointer_down_at = None;
                    self.running_game = None;
                }
            }
        }
        // Drain any live preview frames the background runner may
        // have produced since the last UI tick.
        let mut latest: Option<FrameSnapshot> = None;
        if let Some(rx) = self.frame_rx.as_ref() {
            while let Ok(frame) = rx.try_recv() {
                latest = Some(frame);
            }
        }
        if let Some(frame) = latest {
            self.upload_frame_texture(ctx, &frame);
        }
    }

    fn upload_frame_texture(&mut self, ctx: &egui::Context, frame: &FrameSnapshot) {
        // Keep the raw guest pixels as well as the GPU texture. Screenshots must
        // capture the framebuffer itself, not the scaled launcher presentation.
        self.last_frame_snapshot = Some(frame.clone());
        let size = [frame.width as usize, frame.height as usize];
        let img = egui::ColorImage::from_rgba_unmultiplied(size, &frame.rgba);
        if let Some(tex) = self.last_frame_texture.as_mut() {
            tex.set(img, egui::TextureOptions::NEAREST);
        } else {
            let tex = ctx.load_texture("pockethle-fb", img, egui::TextureOptions::NEAREST);
            self.last_frame_texture = Some(tex);
        }
        self.frame_stats.record_frame();
    }

    fn save_framebuffer_screenshot(&self) -> Result<std::path::PathBuf, String> {
        let frame = self
            .last_frame_snapshot
            .as_ref()
            .ok_or_else(|| "no framebuffer available yet".to_string())?;

        let dir = self.library.root().join("screenshots");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;

        let game = self.running_game.as_deref().unwrap_or("PocketHLE");
        let safe_name: String = game
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("system clock error: {e}"))?
            .as_millis();
        let path = dir.join(format!("{safe_name}_{millis}.png"));

        image::save_buffer_with_format(
            &path,
            &frame.rgba,
            frame.width,
            frame.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;

        Ok(path)
    }

    fn reload_library(&mut self) {
        match Library::open(self.library.root()) {
            Ok(lib) => { self.library = lib; self.analysis_cache.clear(); },
            Err(e) => self.status = format!("Could not reload library: {e}"),
        }
    }

    fn ui_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("PocketHLE");
            ui.label(
                RichText::new("Pocket PC / Windows Mobile launcher")
                    .small()
                    .color(Color32::from_gray(160)),
            );
            ui.add_space(16.0);
            if ui
                .selectable_label(self.screen == Screen::Library, "Library")
                .clicked()
            {
                self.screen = Screen::Library;
            }
            if ui
                .selectable_label(self.screen == Screen::Settings, "Settings")
                .clicked()
            {
                self.config_draft = Some(self.library.config().clone());
                self.screen = Screen::Settings;
            }
            let can_screenshot = self.running_game.is_some() && self.last_frame_snapshot.is_some();
            if ui
                .add_enabled(can_screenshot, egui::Button::new("Screenshot"))
                .on_hover_text("Save the native guest framebuffer as a PNG")
                .clicked()
            {
                match self.save_framebuffer_screenshot() {
                    Ok(path) => {
                        self.status = format!("Screenshot saved: {}", path.display());
                    }
                    Err(err) => {
                        self.status = format!("Screenshot failed: {err}");
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Import .CAB / .ZIP / .RAR...").clicked() {
                    self.spawn_import_dialog();
                }
            });
        });
        ui.separator();
    }

    fn ui_library(&mut self, ui: &mut egui::Ui) {
        let games: Vec<GameEntry> = self.library.games().to_vec();
        if games.is_empty() {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No games yet").heading().color(Color32::from_gray(160)));
                ui.add_space(8.0);
                ui.label("Click \"Import .CAB / .ZIP / .RAR...\" to add a Pocket PC game.");
                ui.add_space(20.0);
                if ui.button("Import .CAB / .ZIP / .RAR...").clicked() { self.spawn_import_dialog(); }
            });
            return;
        }

        if self.selected_game.as_ref().is_none_or(|id| !games.iter().any(|g| &g.id == id)) {
            self.selected_game = games.first().map(|g| g.id.clone());
        }
        let selected = self.selected_game.clone();

        ui.horizontal_top(|ui| {
            let details_width = (ui.available_width() * 0.40).clamp(330.0, 470.0);
            let cards_width = (ui.available_width() - details_width - 16.0).max(240.0);
            ui.allocate_ui_with_layout(
                Vec2::new(cards_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.heading("Library");
                    ui.add_space(8.0);
                    ScrollArea::vertical().show(ui, |ui| {
                        let gap = 12.0;
                        let card_size = Vec2::new(190.0, 210.0);
                        let columns = ((ui.available_width() + gap) / (card_size.x + gap)).floor().max(1.0) as usize;
                        egui::Grid::new("library_grid")
                            .num_columns(columns)
                            .min_col_width(card_size.x).max_col_width(card_size.x)
                            .min_row_height(card_size.y).spacing(Vec2::splat(gap))
                            .show(ui, |ui| {
                                for (index, game) in games.iter().enumerate() {
                                    self.ui_game_card(ui, game, card_size, selected.as_deref() == Some(&game.id));
                                    if (index + 1) % columns == 0 { ui.end_row(); }
                                }
                                if !games.len().is_multiple_of(columns) { ui.end_row(); }
                            });
                    });
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                Vec2::new(details_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if let Some(game) = selected.as_ref().and_then(|id| games.iter().find(|g| &g.id == id)) {
                        self.ui_game_details(ui, game);
                    }
                },
            );
        });
    }

    fn ui_game_card(&mut self, ui: &mut egui::Ui, game: &GameEntry, size: Vec2, selected: bool) {
        let (card_rect, card_response) = ui.allocate_exact_size(size, Sense::click());
        let stroke = if selected { egui::Stroke::new(2.0_f32, Color32::from_rgb(95, 170, 225)) }
                     else { egui::Stroke::new(1.0_f32, Color32::from_rgb(82, 86, 96)) };
        ui.painter().rect(card_rect, 14.0, Color32::from_rgb(35, 38, 46), stroke);

        let menu_rect = Rect::from_min_size(Pos2::new(card_rect.right()-36.0, card_rect.top()+3.0), Vec2::splat(30.0));
        let mut menu_ui = ui.child_ui(menu_rect, egui::Layout::right_to_left(egui::Align::Center));
        let menu_response = menu_ui.menu_button(RichText::new("⋮").size(22.0), |ui| {
            if ui.button("Settings").clicked() {
                self.selected_game = Some(game.id.clone());
                self.game_settings_draft = Some((game.id.clone(), game.settings.clone()));
                self.screen = Screen::GameSettings; ui.close_menu();
            }
            if ui.button("Remove").clicked() {
                if let Err(e) = self.library.remove(&game.id) { self.status = format!("Remove failed: {e}"); }
                else { self.status = format!("Removed {}", game.display_name); }
                ui.close_menu();
            }
        });

        let icon_rect = Rect::from_min_size(Pos2::new(card_rect.left(), card_rect.top()+4.0), Vec2::new(size.x, 126.0));
        let giz = is_gizmondo_game(game, self.library.root());
        if giz {
            let title_id = detect_gizmondo_title_id(&game.extracted_dir(self.library.root()));
            draw_gizmondo_sd(ui, icon_rect, &game.display_name, title_id.as_deref());
        } else {
            let mut icon_ui = ui.child_ui(icon_rect, egui::Layout::centered_and_justified(egui::Direction::TopDown));
            let mut drew_icon = false;
            if let Some(path) = game.icon_path(self.library.root()) {
                if let Ok(bytes) = std::fs::read(path) {
                    if let Ok(image) = load_image_bytes(&bytes) {
                        let texture = self.icon_cache.entry(game.id.clone()).or_insert_with(|| icon_ui.ctx().load_texture(format!("icon-{}", game.id), image, egui::TextureOptions::LINEAR));
                        icon_ui.add(egui::Image::from_texture(&*texture).fit_to_exact_size(Vec2::splat(94.0)));
                        drew_icon = true;
                    }
                }
            }
            if !drew_icon { icon_ui.label(RichText::new("📱").size(58.0)); }
        }

        let label_rect = Rect::from_min_size(Pos2::new(card_rect.left(), card_rect.bottom()-78.0), Vec2::new(size.x, 78.0));
        ui.painter().rect_filled(label_rect, 0.0, Color32::from_rgb(28,29,32));
        let mut text_ui = ui.child_ui(label_rect.shrink2(Vec2::new(10.0,7.0)), egui::Layout::top_down(egui::Align::Min));
        text_ui.add(egui::Label::new(RichText::new(&game.display_name).strong().size(16.0)).truncate(true));
        text_ui.label(RichText::new(if giz { "GIZMONDO • 320×240" } else { "POCKET PC / WINCE" }).size(12.0).color(Color32::from_gray(185)));
        if let Some(id) = detect_gizmondo_title_id(&game.extracted_dir(self.library.root())) {
            text_ui.label(RichText::new(id).monospace().size(11.0).color(Color32::from_gray(145)));
        }

        if card_response.clicked() && !menu_response.response.clicked() { self.selected_game = Some(game.id.clone()); }
        if card_response.double_clicked() { self.spawn_run(game); }
    }

    fn ui_game_details(&mut self, ui: &mut egui::Ui, game: &GameEntry) {
        let root = self.library.root().to_path_buf();
        let analysis = self.analysis_cache.entry(game.id.clone()).or_insert_with(|| analyze_game(game, &root)).clone();
        ScrollArea::vertical().id_source("game_details").show(ui, |ui| {
            ui.heading(RichText::new(&game.display_name).size(24.0));
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&analysis.platform).strong());
                if let Some(id) = &analysis.title_id { ui.label(RichText::new(id).monospace()); }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_sized([110.0, 34.0], egui::Button::new(RichText::new("▶  PLAY").strong())).clicked() { self.spawn_run(game); }
                if ui.button("Game settings").clicked() {
                    self.game_settings_draft = Some((game.id.clone(), game.settings.clone()));
                    self.screen = Screen::GameSettings;
                }
            });
            ui.add_space(12.0);
            detail_section(ui, "IDENTITY", &analysis.identity);
            detail_section(ui, "EXECUTABLE", &analysis.executable);
            detail_section(ui, "DISPLAY & GRAPHICS", &analysis.graphics);
            if !analysis.hardware.is_empty() { detail_section(ui, "HARDWARE", &analysis.hardware); }
            if !analysis.game_data.is_empty() { detail_section(ui, "GAME DATA", &analysis.game_data); }
            if !analysis.languages.is_empty() { detail_section(ui, "LANGUAGES", &analysis.languages); }
            if !analysis.evidence.is_empty() {
                egui::CollapsingHeader::new("Technical details / detection evidence").show(ui, |ui| {
                    for line in &analysis.evidence { ui.label(RichText::new(line).monospace().size(11.0)); }
                });
            }
        });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        let Some(mut draft) = self.config_draft.take() else {
            return;
        };
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        ui.heading("Launcher settings");
        ui.add_space(8.0);
        let library_root = self.library.root().display().to_string();
        egui::Grid::new("settings_grid")
            .num_columns(2)
            .spacing(Vec2::new(12.0, 8.0))
            .show(ui, |ui| {
                ui.label("Default CPU backend");
                egui::ComboBox::from_id_source("cpu_backend")
                    .selected_text(draft.default_cpu_backend.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.default_cpu_backend,
                            CpuBackendPref::Stub,
                            CpuBackendPref::Stub.label(),
                        );
                        if cfg!(feature = "unicorn") {
                            ui.selectable_value(
                                &mut draft.default_cpu_backend,
                                CpuBackendPref::Unicorn,
                                CpuBackendPref::Unicorn.label(),
                            );
                        }
                    });
                ui.end_row();

                ui.label("Verbosity (0..3)");
                ui.add(egui::Slider::new(&mut draft.verbosity, 0..=3));
                ui.end_row();

                ui.label("Show FPS overlay");
                ui.checkbox(&mut draft.show_fps, "");
                ui.end_row();

                ui.label("Borderless fullscreen");
                if ui.checkbox(&mut draft.fullscreen, "").changed() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Fullscreen(draft.fullscreen));
                }
                ui.end_row();

                ui.label("Library root");
                ui.label(library_root);
                ui.end_row();
            });

        ui.add_space(12.0);
        self.ui_keybindings(ui, &mut draft);

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save_clicked = true;
            }
            if ui.button("Cancel").clicked() {
                cancel_clicked = true;
            }
        });
        if save_clicked {
            *self.library.config_mut() = draft;
            if let Err(e) = self.library.save() {
                self.status = format!("Could not save settings: {e}");
            } else {
                self.status = "Settings saved.".to_string();
            }
            self.screen = Screen::Library;
        } else if cancel_clicked {
            self.screen = Screen::Library;
        } else {
            self.config_draft = Some(draft);
        }
    }

    /// Keyboard editor: one row per guest button, listing the host keys
    /// bound to it with an ✕ to drop each and an "Add key" that waits
    /// for the next key press.
    ///
    /// Edits land in `draft`, so they are only written to `config.json`
    /// when the user saves the settings screen — and once written they
    /// are what [`Self::handle_physical_keyboard`] reads on every key
    /// event, which is what makes a rebind survive a restart.
    fn ui_keybindings(&mut self, ui: &mut egui::Ui, draft: &mut LauncherConfig) {
        ui.label(RichText::new("Keyboard").strong());
        ui.label(
            RichText::new(
                "Host keys for PocketPC and Gizmondo controls. Existing PocketPC buttons are reused \
                 for their Gizmondo equivalents; only the five Gizmondo piano buttons are extra. \
                 A key only ever drives one button, so rebinding it moves it from the old control.",
            )
            .small()
            .color(Color32::from_gray(160)),
        );
        ui.add_space(6.0);

        // Copied out so the closures below can mutate the capture state
        // without holding a second borrow of `self`.
        let mut capture = self.binding_capture;
        let mut reset_clicked = false;

        // A capture is armed: the next key press the window sees becomes
        // the binding. Read the events directly rather than waiting for
        // a widget to be focused, because the key the user wants may be
        // one egui would otherwise treat as navigation (Tab, arrows).
        if let Some(button) = capture {
            let captured = ui.ctx().input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } => Some(*key),
                    _ => None,
                })
            });
            if let Some(key) = captured {
                draft.keybindings.bind(button, key.name());
                capture = None;
            }
        }

        egui::Grid::new("keybindings_grid")
            .num_columns(3)
            .spacing(Vec2::new(12.0, 6.0))
            .show(ui, |ui| {
                for button in GuestButton::ALL {
                    ui.label(button.label());
                    let keys: Vec<String> = draft.keybindings.keys_for(button).to_vec();
                    ui.horizontal(|ui| {
                        if keys.is_empty() {
                            ui.label(
                                RichText::new("unbound")
                                    .small()
                                    .color(Color32::from_gray(140)),
                            );
                        }
                        for key in &keys {
                            ui.label(RichText::new(key).monospace());
                            if ui.small_button("✕").clicked() {
                                draft.keybindings.unbind(button, key);
                            }
                        }
                    });
                    if capture == Some(button) {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("press a key…")
                                    .small()
                                    .color(Color32::LIGHT_BLUE),
                            );
                            if ui.small_button("Cancel").clicked() {
                                capture = None;
                            }
                        });
                    } else if ui.button("Add key").clicked() {
                        capture = Some(button);
                    }
                    ui.end_row();
                }
            });

        ui.add_space(6.0);
        if ui.button("Reset keys to defaults").clicked() {
            reset_clicked = true;
        }
        if reset_clicked {
            draft.keybindings = pocket_library::KeyBindings::default();
            capture = None;
        }
        self.binding_capture = capture;
    }

    fn ui_game_settings(&mut self, ui: &mut egui::Ui) {
        let Some((id, mut draft)) = self.game_settings_draft.take() else {
            return;
        };
        let display_name = self
            .library
            .get(&id)
            .map(|g| g.display_name.clone())
            .unwrap_or_else(|| id.clone());
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        ui.heading(format!("Settings: {display_name}"));
        ui.add_space(8.0);
        egui::Grid::new("game_settings_grid")
            .num_columns(2)
            .spacing(Vec2::new(12.0, 8.0))
            .show(ui, |ui| {
                ui.label("CPU backend");
                egui::ComboBox::from_id_source("game_cpu_backend")
                    .selected_text(draft.cpu_backend.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.cpu_backend,
                            CpuBackendPref::Stub,
                            CpuBackendPref::Stub.label(),
                        );
                        if cfg!(feature = "unicorn") {
                            ui.selectable_value(
                                &mut draft.cpu_backend,
                                CpuBackendPref::Unicorn,
                                CpuBackendPref::Unicorn.label(),
                            );
                        }
                    });
                ui.end_row();

                ui.label("Max slices");
                ui.add(egui::DragValue::new(&mut draft.max_slices).clamp_range(1..=u64::MAX));
                ui.end_row();

                ui.label("Instructions / slice");
                ui.add(
                    egui::DragValue::new(&mut draft.instructions_per_slice)
                        .clamp_range(1..=u64::MAX),
                );
                ui.end_row();

                ui.label("Screen");
                egui::ComboBox::from_id_source("game_screen")
                    .selected_text(draft.screen.label())
                    .show_ui(ui, |ui| {
                        for pref in [
                            ScreenPref::Portrait,
                            ScreenPref::Landscape,
                            ScreenPref::SmallPortrait,
                            ScreenPref::Wvga,
                            ScreenPref::Hpc,
                        ] {
                            ui.selectable_value(&mut draft.screen, pref, pref.label());
                        }
                    });
                ui.end_row();

                // Presentation-only: the guest keeps rendering into
                // whatever `Screen` says. This is the knob for a
                // landscape game that shipped as a 240x320 portrait
                // build — forcing `Screen` to 320x240 makes some of
                // them lay out against geometry they never shipped
                // against, turning the frame instead always works.
                ui.label("Rotate display");
                egui::ComboBox::from_id_source("game_rotation_pref")
                    .selected_text(draft.rotation.label())
                    .show_ui(ui, |ui| {
                        for choice in RotationPref::ALL {
                            ui.selectable_value(&mut draft.rotation, choice, choice.label());
                        }
                    });
                ui.end_row();

                ui.label("Halt on unimplemented API");
                ui.checkbox(&mut draft.halt_on_unimplemented, "");
                ui.end_row();
            });

        // Satellite libraries are not a setting — they are a fact about
        // what got imported. Showing them here is how a user confirms
        // that picking `solitare.exe` also brought `pegcards.dll` in,
        // without having to go digging in the library directory.
        let companions: Vec<String> = self
            .library
            .get(&id)
            .map(|g| {
                g.companions
                    .iter()
                    .filter_map(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        if !companions.is_empty() {
            ui.add_space(8.0);
            ui.label(format!("Support libraries: {}", companions.join(", ")));
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save_clicked = true;
            }
            if ui.button("Cancel").clicked() {
                cancel_clicked = true;
            }
        });
        if save_clicked {
            if let Err(e) = self.library.update_settings(&id, draft) {
                self.status = format!("Could not save game settings: {e}");
            } else {
                self.status = "Game settings saved.".to_string();
            }
            self.screen = Screen::Library;
        } else if cancel_clicked {
            self.screen = Screen::Library;
        } else {
            self.game_settings_draft = Some((id, draft));
        }
    }

    fn ui_run(&mut self, ui: &mut egui::Ui) {
        ui.heading("Run output");
        ui.add_space(8.0);
        if let Some(name) = self.running_game.as_ref() {
            ui.label(format!("Running {name}…"));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Rotation");
            let mut rotation = self.game_rotation;
            egui::ComboBox::from_id_source("game_rotation")
                .selected_text(rotation.label())
                .show_ui(ui, |ui| {
                    for choice in RotationPref::ALL {
                        ui.selectable_value(&mut rotation, choice, choice.label());
                    }
                });
            if rotation != self.game_rotation {
                self.game_rotation = rotation;
                self.persist_rotation(rotation);
            }
            if ui.button("Fullscreen (F11)").clicked() {
                let fullscreen = ui
                    .ctx()
                    .input(|input| input.viewport().fullscreen.unwrap_or(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Back to library").clicked() {
                    // Best-effort — if the runner thread already
                    // exited the channel will just drop the message.
                    if let Some(tx) = self.input_tx.as_ref() {
                        let _ = tx.send(InputCommand::Stop);
                    }
                    self.release_all_keys();
                    self.screen = Screen::Library;
                }
            });
        });
        ui.add_space(6.0);
        if self.running_is_gizmondo {
            self.ui_gizmondo(ui);
        } else {
            ui.horizontal_top(|ui| {
                self.ui_run_screen(ui);
                ui.add_space(12.0);
                self.ui_virtual_pad(ui);
            });
        }
        ui.add_space(8.0);
        if let Some(s) = self.last_frame_status.as_ref() {
            ui.label(RichText::new(s).small().color(Color32::from_gray(170)));
        }
    }

    /// Gizmondo-specific Run view. The guest framebuffer is drawn into the
    /// console LCD and every physical control is a real held button. Keyboard
    /// shortcuts intentionally describe the *host* keyboard; the VK sent to
    /// Windows CE follows the official Gizmondo Keys SDK sample.
    fn ui_gizmondo(&mut self, ui: &mut egui::Ui) {
        if self.gizmondo_skin.is_none() {
            if let Ok(image) = load_image_bytes(include_bytes!("../assets/gizmondo.png")) {
                self.gizmondo_skin = Some(ui.ctx().load_texture(
                    "gizmondo-skin",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }

        // The supplied skin is 1648x928. Keep that ratio so all hitboxes and
        // the LCD remain locked to the photographed controls while resizing.
        let avail = ui.available_size();
        let aspect = 1648.0 / 928.0;
        let w = avail.x.min(avail.y * aspect).min(1200.0).max(480.0);
        let h = w / aspect;
        let (body, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
        let rr = |x: f32, y: f32, rw: f32, rh: f32| Rect::from_min_size(
            Pos2::new(body.left() + x * w, body.top() + y * h),
            Vec2::new(rw * w, rh * h),
        );

        if let Some(skin) = self.gizmondo_skin.as_ref() {
            ui.painter().image(
                skin.id(), body,
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.painter().rect_filled(body, 24.0, Color32::from_rgb(24, 25, 27));
        }

        // Replace the static picture in the skin with the emulator's live
        // 320x240 framebuffer. These coordinates are measured from the skin.
        let screen = rr(0.3016, 0.2252, 0.3920, 0.5280);
        if let Some(tex) = self.last_frame_texture.clone() {
            let uv = rotation_uv(self.game_rotation);
            let mut mesh = Mesh::with_texture(tex.id());
            let idx = mesh.vertices.len() as u32;
            for (pos, uv) in [(screen.left_top(), uv[0]), (screen.right_top(), uv[1]),
                              (screen.left_bottom(), uv[2]), (screen.right_bottom(), uv[3])] {
                mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
            }
            mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx + 2, idx + 1, idx + 3]);
            ui.painter().add(egui::Shape::mesh(mesh));
            self.handle_pointer(ui.ctx(), &screen, tex.size_vec2());
        } else {
            ui.painter().rect_filled(screen, 0.0, Color32::BLACK);
            ui.painter().text(screen.center(), egui::Align2::CENTER_CENTER, "Starting…",
                              egui::FontId::proportional(18.0), Color32::from_gray(180));
        }

        // Five top "piano" buttons. Their host keys come from Settings;
        // Piano 5 still sends guest VK_F11, as required by the official SDK.
        for (rect, symbol, label, button) in [
            (rr(0.347, 0.020, 0.039, 0.102), "⌂", "Home / Piano 1", GuestButton::GizPiano1),
            (rr(0.410, 0.018, 0.039, 0.104), "♪", "Volume / Piano 2", GuestButton::GizPiano2),
            (rr(0.475, 0.015, 0.039, 0.108), "☀", "Brightness / Piano 3", GuestButton::GizPiano3),
            (rr(0.540, 0.017, 0.039, 0.106), "!", "Alert / Piano 4", GuestButton::GizPiano4),
            (rr(0.606, 0.020, 0.039, 0.102), "⏻", "Power / Piano 5", GuestButton::GizPiano5),
        ] { self.giz_button(ui, rect, symbol, label, button); }

        self.giz_button(ui, rr(0.105, 0.010, 0.105, 0.145), "L", "Left shoulder", GuestButton::Soft1);
        self.giz_button(ui, rr(0.790, 0.010, 0.105, 0.145), "R", "Right shoulder", GuestButton::Soft2);

        self.giz_button(ui, rr(0.113, 0.335, 0.061, 0.105), "▲", "D-pad up", GuestButton::DpadUp);
        self.giz_button(ui, rr(0.075, 0.420, 0.061, 0.105), "◀", "D-pad left", GuestButton::DpadLeft);
        self.giz_button(ui, rr(0.151, 0.420, 0.061, 0.105), "▶", "D-pad right", GuestButton::DpadRight);
        self.giz_button(ui, rr(0.113, 0.505, 0.061, 0.105), "▼", "D-pad down", GuestButton::DpadDown);

        self.giz_button(ui, rr(0.827, 0.285, 0.060, 0.105), "■", "North / Stop", GuestButton::ButtonA);
        self.giz_button(ui, rr(0.775, 0.385, 0.060, 0.105), "◀◀", "West / Rewind", GuestButton::ButtonC);
        self.giz_button(ui, rr(0.875, 0.385, 0.060, 0.105), "▶▶", "East / Forward", GuestButton::ButtonB);
        self.giz_button(ui, rr(0.827, 0.500, 0.060, 0.105), "▶", "South / Play", GuestButton::Action);
    }

    fn giz_button(&mut self, ui: &mut egui::Ui, rect: Rect, symbol: &str, label: &str, button: GuestButton) {
        let vk = button.vk();
        let host = self.library.config().keybindings.keys_for(button).join(", ");
        let was_pressed = self.held.is_held_by(InputSource::Pointer, vk);
        let now_pressed = pointer_held_in(ui.ctx(), &rect);
        if now_pressed && !was_pressed && self.held.press(InputSource::Pointer, vk) {
            self.send_input(InputEvent::KeyDown { vk });
        } else if was_pressed && !now_pressed && self.held.release(InputSource::Pointer, vk) {
            self.send_input(InputEvent::KeyUp { vk });
        }

        // The skin already contains the physical button. On press we only
        // repaint its dark legend in white, which gives the requested
        // illuminated-button feedback without covering the artwork.
        if self.held.is_held(vk) {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, symbol,
                              egui::FontId::proportional((rect.height() * 0.38).max(12.0)),
                              Color32::WHITE);
        }
        let id = ui.make_persistent_id(("giz_button", vk));
        ui.interact(rect, id, Sense::click_and_drag())
            .on_hover_text(if host.is_empty() {
                format!("{label} — keyboard: unbound")
            } else {
                format!("{label} — keyboard: {host}")
            });
    }

    /// Render the live framebuffer (or placeholder) and forward any
    /// pointer presses on it as `WM_LBUTTONDOWN` / `WM_LBUTTONUP`
    /// events, with stylus coordinates in whatever resolution the game
    /// is actually running at.
    fn ui_run_screen(&mut self, ui: &mut egui::Ui) {
        let Some(tex) = self.last_frame_texture.clone() else {
            ui.allocate_ui(
                Vec2::new(FB_WIDTH as f32 * 2.0, FB_HEIGHT as f32 * 2.0),
                |ui| {
                    ui.label("(no framebuffer captured yet — try Run again)");
                },
            );
            return;
        };
        let size = tex.size_vec2();
        // Display at 2x for readability, the same way the CLI's
        // minifb DisplayHook scales — but never larger than the space
        // egui actually gave us. A 480x800 WVGA game at a fixed 2x is
        // 960x1600 and would run off the bottom of a 1080p window.
        let available = ui.available_size();
        let rotated_size = if self.game_rotation.is_quarter_turn() {
            Vec2::new(size.y, size.x)
        } else {
            size
        };
        let scale = 2.0_f32
            .min(available.x / rotated_size.x)
            .min(available.y / rotated_size.y)
            .max(0.1);
        let display_size = rotated_size * scale;
        let (rect, _response) = ui.allocate_exact_size(display_size, Sense::click_and_drag());
        let uv = rotation_uv(self.game_rotation);
        let mut mesh = Mesh::with_texture(tex.id());
        let idx = mesh.vertices.len() as u32;
        for (pos, uv) in [
            (rect.left_top(), uv[0]),
            (rect.right_top(), uv[1]),
            (rect.left_bottom(), uv[2]),
            (rect.right_bottom(), uv[3]),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.indices
            .extend_from_slice(&[idx, idx + 1, idx + 2, idx + 2, idx + 1, idx + 3]);
        ui.painter().add(egui::Shape::mesh(mesh));
        // The j2me-loader-style FPS overlay is opt-in: gated on the
        // launcher's `show_fps` config flag so users who find a
        // permanent debug HUD distracting can switch it off in
        // Settings.
        if self.library.config().show_fps {
            let overlay_rect =
                Rect::from_min_size(rect.min + Vec2::new(6.0, 6.0), Vec2::new(390.0, 24.0));
            ui.painter()
                .rect_filled(overlay_rect, 4.0, Color32::from_black_alpha(190));
            ui.painter().text(
                overlay_rect.min + Vec2::new(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                self.frame_stats.overlay_text(),
                egui::FontId::monospace(13.0),
                Color32::LIGHT_GREEN,
            );
        }
        self.handle_pointer(ui.ctx(), &rect, size);
    }

    /// `size` is the guest framebuffer's own dimensions in pixels — the
    /// coordinate space the game expects to receive stylus events in.
    ///
    /// Press state comes from [`pointer_held_in`] rather than from
    /// egui's click/drag bookkeeping so that a stylus held on the same
    /// spot stays down indefinitely — a tap-and-hold is how a Pocket PC
    /// game is told "keep going", and egui's click timeout would have
    /// let go for the user.
    fn handle_pointer(&mut self, ctx: &egui::Context, rect: &Rect, size: Vec2) {
        let held = pointer_held_in(ctx, rect);
        let pos = ctx.input(|input| input.pointer.latest_pos());
        let coords = pos.map(|pos| {
            // Map UI-space coords to guest framebuffer pixels. The panel
            // scales whatever geometry the game actually runs at, so both
            // factors have to come from the live texture rather than the
            // compile-time constants — otherwise a 480×320 game would see
            // taps land on the wrong pixels.
            rotated_pointer_to_game(pos - rect.min, rect.size(), size, self.game_rotation)
        });
        match (self.pointer_down_at, held) {
            (None, true) => {
                if let Some((x, y)) = coords {
                    self.send_input(InputEvent::PointerDown { x, y });
                    self.pointer_down_at = Some((x, y));
                }
            }
            (Some(last), true) => {
                // Only report movement that actually changed a guest
                // pixel; a still stylus should not flood the pump.
                if let Some((x, y)) = coords {
                    if (x, y) != last {
                        self.send_input(InputEvent::PointerMove { x, y });
                        self.pointer_down_at = Some((x, y));
                    }
                }
            }
            (Some(last), false) => {
                let (x, y) = coords.unwrap_or(last);
                self.send_input(InputEvent::PointerUp { x, y });
                self.pointer_down_at = None;
            }
            (None, false) => {}
        }
    }

    /// j2me-loader-inspired virtual gamepad: a D-pad on the left and
    /// three action buttons (A / B / Start) plus two soft keys on
    /// the right. Each button drives a `WM_KEYDOWN`/`WM_KEYUP` pair
    /// while held.
    fn ui_virtual_pad(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(RichText::new("Controls").strong());
            ui.add_space(4.0);
            // ----- D-pad: 3x3 grid with cardinal arrows -----
            egui::Grid::new("vpad_dpad")
                .spacing(Vec2::new(2.0, 2.0))
                .show(ui, |ui| {
                    ui.label("");
                    self.vbutton(ui, "▲", GuestButton::DpadUp, 44.0);
                    ui.label("");
                    ui.end_row();
                    self.vbutton(ui, "◀", GuestButton::DpadLeft, 44.0);
                    ui.label("");
                    self.vbutton(ui, "▶", GuestButton::DpadRight, 44.0);
                    ui.end_row();
                    ui.label("");
                    self.vbutton(ui, "▼", GuestButton::DpadDown, 44.0);
                    ui.label("");
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                self.vbutton(ui, "Action", GuestButton::Action, 52.0);
                self.vbutton(ui, "A", GuestButton::ButtonA, 52.0);
                self.vbutton(ui, "B", GuestButton::ButtonB, 52.0);
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                self.vbutton(ui, "C", GuestButton::ButtonC, 52.0);
                self.vbutton(ui, "1", GuestButton::Soft1, 52.0);
                self.vbutton(ui, "2", GuestButton::Soft2, 52.0);
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                self.vbutton(ui, "Turbo", GuestButton::Turbo, 60.0);
            });
        });
    }

    /// Render one virtual button. Pressed-while-pointer-is-down
    /// generates `WM_KEYDOWN` once; releasing fires `WM_KEYUP`.
    ///
    /// The held state comes from [`pointer_held_in`] rather than from
    /// the `Response`, because egui stops calling a still-held press a
    /// click after 0.8 s — see that function.
    fn vbutton(&mut self, ui: &mut egui::Ui, label: &str, guest: GuestButton, size: f32) {
        let vk = guest.vk();
        let was_pressed = self.held.is_held_by(InputSource::Pointer, vk);
        let mut button = egui::Button::new(RichText::new(label).size(16.0).strong())
            .min_size(Vec2::new(size, size));
        if self.held.is_held(vk) {
            button = button.fill(Color32::from_rgb(80, 130, 255));
        }
        let keys = self.library.config().keybindings.keys_for(guest).join(", ");
        let response = ui.add_sized(Vec2::new(size, size), button);
        // Naming the keyboard shortcut on the button is how a user
        // discovers what their own bindings ended up as.
        let response = if keys.is_empty() {
            response.on_hover_text(format!("{} (no key bound)", guest.label()))
        } else {
            response.on_hover_text(format!("{} — {keys}", guest.label()))
        };
        let now_pressed = pointer_held_in(ui.ctx(), &response.rect);
        // Only the edges matter, and `HeldButtons` has the last word on
        // whether the guest hears about them: the keyboard may be
        // holding this same button.
        if now_pressed && !was_pressed && self.held.press(InputSource::Pointer, vk) {
            self.send_input(InputEvent::KeyDown { vk });
        } else if was_pressed && !now_pressed && self.held.release(InputSource::Pointer, vk) {
            self.send_input(InputEvent::KeyUp { vk });
        }
    }

    /// Write a rotation picked on the Run screen back to the game's
    /// `game.json`, so the choice survives the run it was made in.
    fn persist_rotation(&mut self, rotation: RotationPref) {
        let Some(id) = self.running_game_id.clone() else {
            return;
        };
        let Some(mut settings) = self.library.get(&id).map(|g| g.settings.clone()) else {
            return;
        };
        settings.rotation = rotation;
        if let Err(e) = self.library.update_settings(&id, settings) {
            self.status = format!("Could not save rotation: {e}");
        }
    }

    fn send_input(&self, ev: InputEvent) {
        if let Some(tx) = self.input_tx.as_ref() {
            let _ = tx.send(InputCommand::Input(ev));
        }
    }

    fn release_all_keys(&mut self) {
        for vk in self.held.drain_all() {
            self.send_input(InputEvent::KeyUp { vk });
        }
    }

    fn handle_physical_keyboard(&mut self, ctx: &egui::Context) {
        if self.screen != Screen::Run {
            return;
        }
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            let egui::Event::Key {
                key,
                pressed,
                repeat,
                ..
            } = event
            else {
                continue;
            };
            if key == egui::Key::F11 && pressed && !repeat {
                let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                continue;
            }
            // Both PocketPC and Gizmondo use the same configurable host-key map.
            // The skin is only presentation; Settings are the single source of truth.
            let vk = self.library.config().keybindings.vk_for_key(key.name());
            let Some(vk) = vk else { continue; };
            if pressed {
                if !repeat && self.held.press(InputSource::Keyboard, vk) {
                    self.send_input(InputEvent::KeyDown { vk });
                }
            } else if self.held.release(InputSource::Keyboard, vk) {
                self.send_input(InputEvent::KeyUp { vk });
            }
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            self.release_all_keys();
        }
    }

    fn spawn_import_dialog(&mut self) {
        let library_root = self.library.root().to_path_buf();
        let last_dir = self.library.config().last_import_dir.clone();
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Import Pocket PC game")
                // A single "Pocket PC game" filter that matches every
                // shape we know how to import keeps the dialog UX
                // simple — the user picks a file and we pick the
                // right loader from the extension below. `.dll` is
                // included so a user who mistakenly picks a support
                // library gets a clear error directing them to import
                // the `.exe` instead, rather than seeing nothing in
                // the picker at all.
                .add_filter(
                    "Pocket PC game (.rar / .cab / .zip / .exe / .dll)",
                    &[
                        "rar", "RAR", "cab", "CAB", "zip", "ZIP", "exe", "EXE", "dll", "DLL",
                    ],
                )
                .add_filter("Cabinet archive", &["cab", "CAB"])
                .add_filter("Zip archive", &["zip", "ZIP"])
                .add_filter("ARM PE executable", &["exe", "EXE"]);
            if let Some(d) = last_dir {
                dialog = dialog.set_directory(d);
            }
            let Some(path) = dialog.pick_file() else {
                let _ = tx.send(UiEvent::ImportFinished(Err("cancelled".into())));
                return;
            };
            let result = (|| -> Result<String, String> {
                let mut lib = Library::open(&library_root).map_err(|e| e.to_string())?;
                let parent = path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or(library_root.clone());
                lib.config_mut().last_import_dir = Some(parent);
                lib.save().map_err(|e| e.to_string())?;
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase);
                let entry = match ext.as_deref() {
                    Some("rar") => lib.import_rar(&path),
                    Some("cab") => lib.import_cab(&path),
                    Some("zip") => lib.import_zip(&path),
                    // Treat anything else (`.exe`, no extension, …)
                    // as a raw ARM PE32. `import_exe` rejects it
                    // with a clear error if the machine type is
                    // wrong.
                    _ => lib.import_exe(&path),
                }
                .map_err(|e| e.to_string())?;
                Ok(entry.display_name.clone())
            })();
            let _ = tx.send(UiEvent::ImportFinished(result));
        });
        self.status = "Importing...".to_string();
    }

    fn spawn_run(&mut self, game: &GameEntry) {
        self.last_frame_texture = None;
        self.last_frame_snapshot = None;
        self.last_frame_status = None;
        self.frame_stats.reset();
        // Start turned the way this game was saved: a portrait-rendered
        // landscape title is unplayable until it is turned, and the user
        // should not have to re-pick that on every launch.
        self.game_rotation = game.settings.rotation;
        self.screen = Screen::Run;
        self.running_game = Some(game.display_name.clone());
        self.running_game_id = Some(game.id.clone());
        self.running_is_gizmondo = is_gizmondo_game(game, self.library.root());
        let (frame_tx, frame_rx) = mpsc::channel();
        let (input_tx, input_rx) = mpsc::channel();
        self.frame_rx = Some(frame_rx);
        self.input_tx = Some(input_tx);
        self.held = HeldButtons::default();
        self.pointer_down_at = None;
        let library_root = self.library.root().to_path_buf();
        let game = game.clone();
        let tx = self.events_tx.clone();
        let runner = self.runner.clone();
        std::thread::spawn(move || {
            let outcome = runner.run_game(library_root, game, Some(frame_tx), Some(input_rx));
            let _ = tx.send(UiEvent::RunFinished(outcome));
        });
        self.status = "Starting emulator...".to_string();
    }
}

/// Is the primary pointer button held down, with the press having
/// started inside `rect`?
///
/// `Response::is_pointer_button_down_on` cannot answer this for a
/// widget that senses clicks: egui gives up on a press being a click
/// after `MAX_CLICK_DURATION` (0.8 s in 0.27) and clears the
/// interaction's `potential_click_id`, at which point the response
/// reports "not down" even though the user never let go. That is the
/// bug where holding a virtual button with the mouse released itself
/// after about a second while the Android pad — which tracks
/// ACTION_DOWN/UP itself — held fine. Reading the pointer state has no
/// such timeout, and keying off `press_origin` keeps the button held
/// even if the pointer drifts off it, which is what a user dragging
/// their thumb across a D-pad expects.
fn pointer_held_in(ctx: &egui::Context, rect: &Rect) -> bool {
    ctx.input(|input| {
        input.pointer.primary_down()
            && input
                .pointer
                .press_origin()
                .is_some_and(|origin| rect.contains(origin))
    })
}

fn rotated_pointer_to_game(
    local: Vec2,
    display_size: Vec2,
    game_size: Vec2,
    rotation: RotationPref,
) -> (u16, u16) {
    let u = (local.x / display_size.x).clamp(0.0, 1.0);
    let v = (local.y / display_size.y).clamp(0.0, 1.0);
    // Un-rotate: the guest only ever knows about its own unturned
    // panel, so a tap has to be mapped back through the same quarter
    // turn the presentation applied.
    let (x, y) = match rotation {
        RotationPref::None => (u, v),
        RotationPref::Cw90 => (v, 1.0 - u),
        RotationPref::Half => (1.0 - u, 1.0 - v),
        RotationPref::Ccw90 => (1.0 - v, u),
    };
    let max_x = game_size.x.max(1.0) as u32 - 1;
    let max_y = game_size.y.max(1.0) as u32 - 1;
    (
        ((x * game_size.x).floor() as u32).min(max_x) as u16,
        ((y * game_size.y).floor() as u32).min(max_y) as u16,
    )
}

impl eframe::App for PocketLauncher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_physical_keyboard(ctx);
        self.drain_events(ctx);
        egui::TopBottomPanel::top("top").show(ctx, |ui| self.ui_top_bar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(Color32::from_gray(140)),
                    );
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.screen {
            Screen::Library => self.ui_library(ui),
            Screen::Settings => self.ui_settings(ui),
            Screen::GameSettings => self.ui_game_settings(ui),
            Screen::Run => self.ui_run(ui),
        });
        // While a game is running we want to drain the live frame
        // channel as fast as the runner produces frames; the original
        // 250 ms cadence capped the launcher at 4 fps, which is most
        // of what users perceived as "lag" in JumpyBall on the
        // desktop. ~16 ms (60 fps) matches the runner's per-slice
        // hook and keeps idle-frame redraws cheap because egui only
        // actually re-uploads textures when something changed.
        let repaint_after = if matches!(self.screen, Screen::Run) || self.frame_rx.is_some() {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(250)
        };
        ctx.request_repaint_after(repaint_after);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_core::kernel::gapi;
    use pocket_library::KeyBindings;

    /// Drawing the on-screen pad must not cancel a key the user is
    /// holding on the keyboard. `vbutton` runs every frame and releases
    /// its button as soon as the pointer is not on it, which — while
    /// both sources shared one set — sent a `WM_KEYUP` for a keyboard
    /// arrow that was still down, so JumpyBall's ball stopped steering
    /// on the desktop after the user had touched the pad once.
    #[test]
    fn the_on_screen_pad_does_not_release_a_key_held_on_the_keyboard() {
        let mut held = HeldButtons::default();
        let vk = GuestButton::DpadRight.vk();

        assert!(held.press(InputSource::Keyboard, vk));
        // The pad's per-frame "pointer is not on me" branch.
        assert!(
            !held.release(InputSource::Pointer, vk),
            "the pad never held this button, so it cannot release it"
        );
        assert!(held.is_held(vk), "the keyboard is still holding it");

        // Only letting go of the key itself reaches the guest.
        assert!(held.release(InputSource::Keyboard, vk));
        assert!(!held.is_held(vk));
    }

    /// The mirror case: while a button is held on the pad *and* on the
    /// keyboard, letting go of one of them must not tell the guest the
    /// button came up — otherwise a user resting a finger on the pad
    /// loses their keyboard hold and vice versa.
    #[test]
    fn a_button_stays_down_until_every_source_releases_it() {
        let mut held = HeldButtons::default();
        let vk = GuestButton::DpadLeft.vk();

        assert!(held.press(InputSource::Pointer, vk));
        assert!(
            !held.press(InputSource::Keyboard, vk),
            "the guest already saw WM_KEYDOWN for this button"
        );
        assert!(!held.release(InputSource::Pointer, vk));
        assert!(held.is_held(vk));
        assert!(held.release(InputSource::Keyboard, vk));
        assert!(!held.is_held(vk));
    }

    /// Closing the window has to release everything, from either source,
    /// or the guest is left with a stuck direction.
    #[test]
    fn draining_reports_every_held_button_once() {
        let mut held = HeldButtons::default();
        let up = GuestButton::DpadUp.vk();
        let action = GuestButton::Action.vk();
        held.press(InputSource::Keyboard, up);
        held.press(InputSource::Pointer, up);
        held.press(InputSource::Pointer, action);

        let mut drained = held.drain_all();
        drained.sort_unstable();
        let mut expected = vec![up, action];
        expected.sort_unstable();
        assert_eq!(drained, expected);
        assert!(!held.is_held(up));
        assert!(!held.is_held(action));
    }

    /// `pocket-library` cannot depend on the emulator (the Android
    /// launcher links it without one), so it repeats the GAPI virtual-key
    /// codes. A `gx.dll` title only reacts to the keys
    /// `GXGetDefaultKeys` named, so a drift between the two tables would
    /// silently stop the D-pad working.
    #[test]
    fn guest_button_vks_match_the_gapi_table() {
        assert_eq!(GuestButton::DpadUp.vk(), gapi::VK_UP);
        assert_eq!(GuestButton::DpadDown.vk(), gapi::VK_DOWN);
        assert_eq!(GuestButton::DpadLeft.vk(), gapi::VK_LEFT);
        assert_eq!(GuestButton::DpadRight.vk(), gapi::VK_RIGHT);
        assert_eq!(GuestButton::Action.vk(), gapi::VK_RETURN);
    }

    /// The default desktop layout mirrors the Gizmondo skin while reusing the
    /// existing PocketPC/GAPI buttons wherever the guest VK is identical.
    #[test]
    fn default_bindings_match_the_gizmondo_layout() {
        let bindings = KeyBindings::default();
        for (key, expected) in [
            (egui::Key::Z, GuestButton::DpadUp),
            (egui::Key::S, GuestButton::DpadDown),
            (egui::Key::Q, GuestButton::DpadLeft),
            (egui::Key::D, GuestButton::DpadRight),
            (egui::Key::ArrowDown, GuestButton::Action),
            (egui::Key::ArrowUp, GuestButton::ButtonA),
            (egui::Key::ArrowRight, GuestButton::ButtonB),
            (egui::Key::ArrowLeft, GuestButton::ButtonC),
            (egui::Key::Num1, GuestButton::Soft1),
            (egui::Key::Num2, GuestButton::Soft2),
            (egui::Key::T, GuestButton::Turbo),
            (egui::Key::F1, GuestButton::GizPiano1),
            (egui::Key::F2, GuestButton::GizPiano2),
            (egui::Key::F3, GuestButton::GizPiano3),
            (egui::Key::F4, GuestButton::GizPiano4),
            (egui::Key::F5, GuestButton::GizPiano5),
        ] {
            assert_eq!(
                bindings.vk_for_key(key.name()),
                Some(expected.vk()),
                "{} should drive {:?}",
                key.name(),
                expected
            );
        }
        assert_eq!(bindings.vk_for_key(egui::Key::F12.name()), None);
    }

    /// A quarter turn has to be undone on the way back in, or a tap
    /// lands somewhere the user did not touch.
    #[test]
    fn a_tap_is_unrotated_before_the_guest_sees_it() {
        let display = Vec2::new(320.0, 240.0);
        let game = Vec2::new(240.0, 320.0);
        // Top-left of a 90°-clockwise presentation is the bottom-left
        // corner of the guest's own portrait panel.
        let (x, y) =
            rotated_pointer_to_game(Vec2::new(0.0, 0.0), display, game, RotationPref::Cw90);
        assert_eq!((x, y), (0, 319));
        let (x, y) =
            rotated_pointer_to_game(Vec2::new(0.0, 0.0), display, game, RotationPref::None);
        assert_eq!((x, y), (0, 0));
    }
}
