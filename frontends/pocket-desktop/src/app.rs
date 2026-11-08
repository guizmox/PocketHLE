//! egui application: library screen, settings, per-game sheet.

#[path = "stylus_controls_ui.rs"]
mod stylus_controls_ui;
#[path = "add_data_ui.rs"]
mod add_data_ui;
#[path = "add_data_prompt.rs"]
mod add_data_prompt;
use add_data_prompt::DataPrompt;
#[path = "title_scroll.rs"]
mod title_scroll;
use pocket_library::stylus_grid::{ControlsMode, GridRouter, HostInput};
use pocket_library::{analyze_game, game_gizmondo_title_id, GameAnalysis};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use stylus_controls_ui::StylusEditor;

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, RichText, ScrollArea, Sense, Vec2};
use egui_extras::image::load_image_bytes;

use pocket_core::kernel::InputEvent;
use pocket_library::{
    is_gizmondo_game, DpadDeadzone, GameEntry, GameSettings, GuestButton, LauncherConfig, Library,
    PreparedImport, RotationPref, ScreenPref,
};

use crate::runner::{FrameSnapshot, InputCommand, RunOutcome, Runner, SystemMediaView};
use pocket_core::winceapi::SystemMediaCommand;

#[derive(Clone)]
struct TitleMarquee {
    title: String,
    width: f32,
    started: f64,
}

/// Clip the full laid-out title rather than truncating or splitting its text.
fn library_title(ui: &mut egui::Ui, game_id: &str, title: &str) {
    let text = egui::WidgetText::from(RichText::new(title).strong().size(16.0));
    let galley = text.into_galley(ui, Some(false), f32::INFINITY, egui::TextStyle::Body);
    let width = ui.available_width().max(0.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, galley.size().y), Sense::hover());
    response.on_hover_text(title);
    if !ui.is_rect_visible(rect) {
        return;
    }
    let overflow = galley.size().x - width;
    let id = egui::Id::new(("library_title_marquee", game_id));
    let mut offset = 0.0;
    if overflow > 0.5 {
        let now = ui.input(|i| i.time);
        let elapsed = ui.ctx().data_mut(|data| {
            let mut state = data
                .get_temp::<TitleMarquee>(id)
                .unwrap_or_else(|| TitleMarquee {
                    title: title.to_owned(),
                    width,
                    started: now,
                });
            if state.title != title || (state.width - width).abs() > 0.5 {
                state = TitleMarquee {
                    title: title.to_owned(),
                    width,
                    started: now,
                };
            }
            let elapsed = now - state.started;
            data.insert_temp(id, state);
            elapsed
        });
        let frame = title_scroll::frame(elapsed, overflow);
        offset = frame.offset;
        if let Some(delay) = frame.repaint_after {
            ui.ctx().request_repaint_after(delay);
        }
    } else {
        ui.ctx().data_mut(|data| data.remove::<TitleMarquee>(id));
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.galley(
        rect.min - Vec2::new(offset, 0.0),
        galley,
        ui.visuals().strong_text_color(),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpscaleFilter {
    None,
    Reconstruction,
    Smaa,
    SmaaSoft,
    Xbrz,
    Bicubic,
    Lanczos,
    Bilinear,
    Nearest,
}
const GAME_MENU_LABELS: [&str; 7] = [
    "Information", "Play", "Rename...", "Settings", "Add/Replace Data", "Remove", "Game ID",
];
const EXECUTABLE_HEAP_HELP: &str = "Some games need this setting to start or run correctly. It can slow down other games. If a game runs slowly, try turning it off in that game’s settings.";

impl UpscaleFilter {
    const ALL: [Self; 9] = [
        Self::None,
        Self::Reconstruction,
        Self::Smaa,
        Self::SmaaSoft,
        Self::Xbrz,
        Self::Bicubic,
        Self::Lanczos,
        Self::Bilinear,
        Self::Nearest,
    ];
    fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Reconstruction => "reconstruction",
            Self::Smaa => "smaa",
            Self::SmaaSoft => "smaa_soft",
            Self::Xbrz => "xbrz",
            Self::Bicubic => "bicubic",
            Self::Lanczos => "lanczos",
            Self::Bilinear => "bilinear",
            Self::Nearest => "nearest",
        }
    }
    fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|f| f.id() == id)
            .unwrap_or(Self::Reconstruction)
    }
    fn label(self) -> &'static str {
        match self {
            Self::None => "No filter",
            Self::Reconstruction => "GPU reconstruction (sharp contours; GPU)",
            Self::Smaa => "SMAA + reconstruction (anti-aliasing; GPU)",
            Self::SmaaSoft => "SMAA + bilinear (very soft; GPU)",
            Self::Xbrz => "xBRZ ×3 (smooth 2D; CPU intensive)",
            Self::Bicubic => "Bicubic (balanced; moderate cost)",
            Self::Lanczos => "Lanczos (sharp; intensive)",
            Self::Bilinear => "Bilinear (soft; fast)",
            Self::Nearest => "Nearest (crisp pixels; fastest)",
        }
    }
    fn toolbar_label(self) -> &'static str {
        match self {
            Self::None => "None", Self::Reconstruction => "Reconstruction", Self::Smaa => "SMAA",
            Self::SmaaSoft => "SMAA soft", Self::Xbrz => "xBRZ ×3", Self::Bicubic => "Bicubic",
            Self::Lanczos => "Lanczos", Self::Bilinear => "Bilinear", Self::Nearest => "Nearest",
        }
    }
    fn stepped(self, forward: bool) -> Self {
        let index = Self::ALL
            .iter()
            .position(|filter| *filter == self)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % Self::ALL.len()
        } else {
            (index + Self::ALL.len() - 1) % Self::ALL.len()
        };
        Self::ALL[next]
    }

    fn gpu_filter(self) -> i32 {
        match self {
            Self::None | Self::Nearest => 0,
            Self::Bilinear => 1,
            Self::Bicubic => 2,
            Self::Reconstruction => 3,
            Self::Lanczos => 4,
            Self::Smaa => 5,
            Self::Xbrz => 6,
            Self::SmaaSoft => 7,
        }
    }

    fn image_filter(self) -> image::imageops::FilterType {
        match self {
            Self::Reconstruction | Self::Smaa | Self::Xbrz | Self::Bicubic => {
                image::imageops::FilterType::CatmullRom
            }
            Self::Lanczos => image::imageops::FilterType::Lanczos3,
            Self::Bilinear | Self::SmaaSoft => image::imageops::FilterType::Triangle,
            Self::None | Self::Nearest => image::imageops::FilterType::Nearest,
        }
    }
}

/// Library window state is independent of temporary game sizing/fullscreen.
#[derive(Clone, Copy, Debug)]
struct LibraryViewport {
    fullscreen: bool,
    maximized: bool,
    size: Option<Vec2>,
    position: Option<Pos2>,
}
impl LibraryViewport {
    fn capture(ctx: &egui::Context) -> Self {
        ctx.input(|input| {
            let viewport = input.viewport();
            Self {
                fullscreen: viewport.fullscreen.unwrap_or(false),
                maximized: viewport.maximized.unwrap_or(false),
                size: viewport.inner_rect.map(|rect| rect.size()),
                position: viewport.outer_rect.map(|rect| rect.min),
            }
        })
    }
    /// Wait for native mode acknowledgements before requesting window geometry.
    /// Resizing a still-fullscreen surface can leave a small backbuffer in a
    /// fullscreen Windows window. Game F12 restore sizes must not compete here.
    fn restore(self, ctx: &egui::Context) -> bool {
        let actual = Self::capture(ctx);
        if actual.fullscreen != self.fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            ctx.request_repaint_after(Duration::from_millis(16));
            return false;
        }
        if self.fullscreen {
            return true;
        }
        if actual.maximized != self.maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maximized));
            ctx.request_repaint_after(Duration::from_millis(16));
            return false;
        }
        if !self.maximized {
            if let Some(position) = self.position {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
            }
            if let Some(size) = self.size {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
        }
        ctx.request_repaint();
        true
    }
}

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

// Prefer retained GPU frames when the UI and guest contexts can share them.
// Unsupported sharing and consumers requiring CPU pixels keep the CPU path.
const SHARED_PRESENTATION_READY: bool = true;

/// Top-level egui app.
pub struct PocketLauncher {
    icon_cache: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    game_card_metadata: std::collections::HashMap<String, (bool, Option<String>)>,
    screen_cache: std::collections::HashMap<String, (ScreenPref, (u32, u32), Option<&'static str>)>,
    analysis_cache: std::collections::HashMap<String, GameAnalysis>,
    upscale_x2: bool,
    upscale_filter: UpscaleFilter,
    control_notice: Option<(String, Instant)>,
    deck_menu_press: Option<(usize, Instant)>,
    deck_view_press: Option<(usize, Instant)>,
    /// PocketPC only: additional Steam / Quick Access FN buttons, when exposed by SDL.
    library_viewport: LibraryViewport,
    library_restore_pending: Option<LibraryViewport>,
    game_viewport_active: bool,
    menu_host_press: std::collections::HashMap<(usize, String), Instant>,
    display_fullscreen: bool,
    startup_window: Option<bool>,
    fullscreen_window_size: Option<Vec2>,
    reconstruction: Option<std::sync::Arc<std::sync::Mutex<crate::reconstruction::Reconstruction>>>,
    screenshot: crate::screenshot::SharedScreenshot,
    recording: crate::recording::SharedRecording,
    library: Library,
    selected_game: Option<String>,
    deck_library_actions: Vec<String>,
    deck_scroll_selection: bool,
    library_columns: usize,
    deck_nav_repeat: Option<(String, Instant)>,
    library_tab: crate::deck_navigation::LibraryTab,
    information_game: Option<String>,
    game_menu: Option<(String, usize)>,
    game_menu_activate: bool,
    game_menu_parent: Option<(String, usize)>,
    controller_ui: crate::controller_ui::Navigation,
    recordings_gallery: crate::recordings_gallery::Gallery,
    rename_draft: Option<(String, String, bool)>,
    remove_game: Option<crate::remove_game::Prompt>,
    add_data: Option<DataPrompt>,
    data_game: Option<String>,
    demo_import: Option<(String, String)>,
    pending_import: Option<PreparedImport>,
    pocketpc_import_options: std::collections::VecDeque<(String, bool)>,
    updating_import: bool,
    library_busy: bool,
    restore_archive: Option<std::path::PathBuf>,
    file_dialog: Option<crate::file_dialog::FileDialog>,
    pending_run: Option<GameEntry>,
    colors_startup_prompt: Option<(
        GameEntry,
        Vec<pocket_library::ColorsLanguage>,
        LauncherConfig,
    )>,
    colors_startup_notice: Option<(String, String)>,
    screen: Screen,
    about_parent: Screen,
    runner: Runner,
    shared_gpu_context: Option<pocket_core::winceapi::gles::ExternalGpuContext>,
    run_thread: Option<std::thread::JoinHandle<()>>,
    events_rx: Receiver<UiEvent>,
    events_tx: Sender<UiEvent>,
    release_update: Option<pocket_library::updates::Update>,
    /// Live framebuffer updates streamed by [`Runner`] while a game
    /// is running. `Some` between [`Self::spawn_run`] and
    /// `UiEvent::RunFinished`; `None` otherwise.
    frame_rx: Option<crate::latest_frame::Receiver<FrameSnapshot>>,
    gps_notice_rx: Option<Receiver<(f64, f64)>>,
    dependency_notice_rx: Option<Receiver<String>>,
    dependency_notice: Option<String>,
    gps_notice: Option<(String, Instant)>,
    /// Channel for sending [`InputCommand`]s (taps / D-pad / stop)
    /// to the running emulator. Mirrors `frame_rx`.
    input_tx: Option<Sender<InputCommand>>,
    system_media: SystemMediaView,
    media_keys_down: [bool; 2],
    /// Which guest buttons are held right now, and by which input
    /// source, so we can fire the matching `WM_KEYUP` on release.
    held: HeldButtons,
    gamepad_rx: crate::gamepad::Monitor,
    gamepad_devices: Vec<(usize, String)>,
    gamepad_selected: Option<usize>,
    gamepad_error: Option<String>,
    gamepad_held: std::collections::HashMap<(usize, String), u16>,
    /// `Some` while a stylus drag is in progress — carries the last
    /// reported game-space coordinates so we don't spam the guest
    /// with redundant events.
    pending_binding_commands: std::cell::RefCell<Vec<u16>>,
    center_tap: std::cell::RefCell<pocket_library::center_tap::CenterTap>,
    pointer_down_at: Option<(u16, u16)>,
    /// Game currently being launched, used as a status caption
    /// while the run is in progress.
    running_game: Option<String>,
    status: String,
    import_error: Option<String>,
    import_error_title: &'static str,
    game_id_popup: Option<String>,
    config_draft: Option<LauncherConfig>,
    game_settings_draft: Option<(String, GameSettings)>,
    last_frame_texture: Option<egui::TextureHandle>,
    uploaded_image_id: Option<u64>,
    /// Latest native guest framebuffer, retained for input and filtered screenshots.
    last_frame_snapshot: Option<FrameSnapshot>,
    last_frame_status: Option<String>,
    frame_stats: FrameStats,
    rate_badge: Option<(Instant, String)>,
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
    input_gestures: pocket_library::input_gestures::GestureTracker,
    input_gesture_clock: Instant,
    input_controller: bool,
    bindings_visual: crate::bindings_visual::Editor,
    stylus_keyboard: std::collections::HashSet<String>,
    stylus_gamepad: std::collections::HashMap<(usize, String), f32>,
    stylus_touch: [f32; 4],
    stylus_last: (i16, i16),
    stylus_pointer: pocket_library::stylus::StylusPointer,
    stylus_clock: Instant,
    stylus_tick: Instant,
    steering_axes: (i16, i16),
    stylus_editor: Option<StylusEditor>,
    stylus_grid_router: GridRouter,
    grid_modifiers: [bool; 4],
    running_controls_mode: ControlsMode,
    running_swap_button_groups: bool,
    running_stylus_grid: pocket_library::stylus_grid::StylusGrid,
}

fn rotation_turns(rotation: RotationPref) -> u8 {
    match rotation {
        RotationPref::None => 0,
        RotationPref::Cw90 => 1,
        RotationPref::Half => 2,
        RotationPref::Ccw90 => 3,
    }
}
fn rotate_direction_button(button: GuestButton, turns: u8) -> GuestButton {
    let directions = [
        GuestButton::DpadUp,
        GuestButton::DpadRight,
        GuestButton::DpadDown,
        GuestButton::DpadLeft,
    ];
    directions
        .iter()
        .position(|b| *b == button)
        .map(|i| directions[(i + turns as usize) % 4])
        .unwrap_or(button)
}
// Desktop uses the Classic control banks. Remap only physical input paths;
// virtual buttons already carry the destination identity after their layout swap.
fn swapped_pocketpc_physical_vk(vk: u16, enabled: bool) -> u16 {
    if !enabled {
        return vk;
    }
    match vk {
        0x11 => 0xc4,
        0xc4 => 0x11, // A / P4
        0x20 => 0xc1,
        0xc1 => 0x20, // B / P1
        0x10 => 0xc2,
        0xc2 => 0x10, // C / P2
        0x0d => 0xc3,
        0xc3 => 0x0d, // Action / P3
        _ => vk,
    }
}

fn rotate_direction_vk(vk: u16, turns: u8) -> u16 {
    let directions = [
        GuestButton::DpadUp,
        GuestButton::DpadRight,
        GuestButton::DpadDown,
        GuestButton::DpadLeft,
    ];
    directions
        .into_iter()
        .find(|b| b.vk() == vk)
        .map(|b| rotate_direction_button(b, turns).vk())
        .unwrap_or(vk)
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

fn detail_section(ui: &mut egui::Ui, title: &str, rows: &[(String, String)]) {
    if rows.is_empty() {
        return;
    }
    ui.separator();
    ui.label(
        RichText::new(title)
            .strong()
            .size(12.0)
            .color(Color32::from_gray(150)),
    );
    let column_width = ((ui.available_width() - 12.0) * 0.5).max(1.0);
    egui::Grid::new(format!("detail-{title}"))
        .min_col_width(0.0)
        .max_col_width(column_width)
        .num_columns(2)
        .spacing(Vec2::new(12.0, 4.0))
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.label(RichText::new(label).color(Color32::from_gray(165)));
                if value == "✓ Detected" || value == "✓ Resource detected" {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(13.0, 13.0), egui::Sense::hover());
                        let stroke = egui::Stroke::new(1.7_f32, Color32::from_rgb(55, 145, 215));
                        let a = Pos2::new(rect.left() + 2.0, rect.center().y);
                        let b = Pos2::new(rect.left() + 5.0, rect.bottom() - 3.0);
                        let c = Pos2::new(rect.right() - 1.5, rect.top() + 2.5);
                        ui.painter().line_segment([a, b], stroke);
                        ui.painter().line_segment([b, c], stroke);
                        ui.label(value.trim_start_matches('✓').trim());
                    });
                } else {
                    ui.add(egui::Label::new(value).wrap(true));
                }
                ui.end_row();
            }
        });
    ui.add_space(5.0);
}

fn draw_gizmondo_sd(
    ui: &egui::Ui,
    area: Rect,
    title: &str,
    title_id: Option<&str>,
    artwork: Option<(egui::TextureId, Vec2)>,
) {
    let p = ui.painter();
    let h = (area.height() - 8.0).min(118.0);
    let w = h * 0.72;
    let r = Rect::from_center_size(area.center(), Vec2::new(w, h));
    let blue = Color32::from_rgb(24, 65, 151);
    let edge = Color32::from_rgb(14, 42, 105);

    // Front-facing SD silhouette. The clipped top-right corner is the only
    // decorative geometry; the game label itself stays deliberately plain.
    let cut = 13.0;
    let pts = vec![
        r.left_top(),
        Pos2::new(r.right() - cut, r.top()),
        r.right_top() + Vec2::new(0.0, cut),
        r.right_bottom(),
        r.left_bottom(),
    ];
    p.add(egui::Shape::convex_polygon(
        pts,
        blue,
        egui::Stroke::new(1.5_f32, edge),
    ));

    // Label fills the complete recessed rectangle: white from its very top
    // down to the black Gizmondo band. No invented cover artwork/background.
    let label = Rect::from_min_max(
        Pos2::new(r.left() + 8.0, r.top() + 16.0),
        Pos2::new(r.right() - 8.0, r.bottom() - 9.0),
    );
    p.rect_filled(label, 5.0, Color32::WHITE);
    p.rect_stroke(
        label,
        5.0,
        egui::Stroke::new(1.0_f32, Color32::from_gray(175)),
    );
    let band_h = 22.0;
    let band = Rect::from_min_max(
        Pos2::new(label.left(), label.bottom() - band_h),
        label.right_bottom(),
    );
    p.rect_filled(band, 0.0, Color32::from_rgb(18, 18, 20));
    p.text(
        band.center(),
        egui::Align2::CENTER_CENTER,
        "GIZMONDO",
        egui::FontId::proportional(12.0),
        Color32::WHITE,
    );

    let id = title_id.unwrap_or("");
    let content = Rect::from_min_max(label.left_top(), Pos2::new(label.right(), band.top()));
    if let Some((texture, image_size)) = artwork {
        // Preserve the BMP's proportions. Wide labels use the full available
        // width; unusually tall images are bounded by the label height too.
        let scale = (content.width() / image_size.x).min(content.height() / image_size.y);
        let image_rect = Rect::from_center_size(content.center(), image_size * scale);
        p.with_clip_rect(content).image(
            texture,
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        return;
    }
    let shown = if title.trim().is_empty() { id } else { title };
    let title_y = content.top() + content.height() * 0.38;
    // Keep the title strictly inside the white label. Multi-word titles use two
    // lines; single long words/IDs are scaled down instead of overflowing.
    let words: Vec<_> = shown.split_whitespace().collect();
    let (l1, l2) = if words.len() > 1 {
        let mid = (words.len() + 1) / 2;
        (words[..mid].join(" "), words[mid..].join(" "))
    } else {
        (shown.to_string(), String::new())
    };
    let longest = l1.chars().count().max(l2.chars().count());
    let title_font = match longest {
        0..=8 => 14.0,
        9..=10 => 11.0,
        11..=13 => 9.5,
        14..=16 => 8.5,
        _ => 7.5,
    };
    p.text(
        Pos2::new(content.center().x, title_y),
        egui::Align2::CENTER_CENTER,
        l1,
        egui::FontId::proportional(title_font),
        Color32::BLACK,
    );
    if !l2.is_empty() {
        p.text(
            Pos2::new(content.center().x, title_y + 16.0),
            egui::Align2::CENTER_CENTER,
            l2,
            egui::FontId::proportional(title_font),
            Color32::BLACK,
        );
    }
    if !id.is_empty() {
        p.text(
            Pos2::new(content.center().x, content.bottom() - 11.0),
            egui::Align2::CENTER_CENTER,
            id,
            egui::FontId::monospace(9.5),
            Color32::from_gray(45),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Library,
    Settings,
    GizmondoOptions,
    PocketPcOptions,
    InputSettings,
    GameSettings,
    About,
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
    Gamepad,
    Gesture,
}

/// Which guest buttons are down, tracked per input source.
///
/// Both sources used to share a single `HashSet<u16>`, and that quietly
/// broke holding a key: the virtual controls runs every frame and
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
    gamepad: std::collections::HashSet<u16>,
    gesture: std::collections::HashSet<u16>,
}

impl HeldButtons {
    fn set(&mut self, source: InputSource) -> &mut std::collections::HashSet<u16> {
        match source {
            InputSource::Keyboard => &mut self.keyboard,
            InputSource::Pointer => &mut self.pointer,
            InputSource::Gamepad => &mut self.gamepad,
            InputSource::Gesture => &mut self.gesture,
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
        self.keyboard.contains(&vk) || self.pointer.contains(&vk) || self.gamepad.contains(&vk) || self.gesture.contains(&vk)
    }

    fn is_held_by(&self, source: InputSource, vk: u16) -> bool {
        match source {
            InputSource::Keyboard => self.keyboard.contains(&vk),
            InputSource::Pointer => self.pointer.contains(&vk),
            InputSource::Gamepad => self.gamepad.contains(&vk),
            InputSource::Gesture => self.gesture.contains(&vk),
        }
    }

    /// Every VK still held, forgetting all of them. Used when the window
    /// is closing so the guest is not left with a stuck key.
    fn drain_all(&mut self) -> Vec<u16> {
        self.keyboard
            .drain()
            .chain(self.pointer.drain())
            .chain(self.gamepad.drain())
            .chain(self.gesture.drain())
            // A VK held on both at once must only be released once.
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[derive(Debug)]
pub enum UiEvent {
    DataPrepared(Result<pocket_library::add_data::PreparedData, String>),
    DataFinished(Result<String, String>),
    BackupFinished(Result<String, String>),
    ColorsUnlocked(Result<String, String>),
    ReleaseAvailable(pocket_library::updates::Update),
    ImportFinished(Result<ImportedGames, String>),
    ImportPrepared(PreparedImport),
    DemoSplitFinished(String, Result<Vec<GameEntry>, String>),
    RunFinished(RunOutcome),
}

#[derive(Debug)]
pub struct ImportedGames {
    entries: Vec<GameEntry>,
    pocketpc_import_options: Vec<(String, bool)>,
}

impl ImportedGames {
    fn analyze(entries: Vec<GameEntry>, library: &Library, existing_ids: &[String]) -> Self {
        let pocketpc_import_options = entries
            .iter()
            .filter(|game| !existing_ids.contains(&game.id) && !library.is_demo_card(&game.id))
            .filter(|game| !is_gizmondo_game(game, library.root()))
            .map(|game| {
                let may_run_fast =
                    crate::frame_pacing::may_run_fast(&game.executable_path(library.root()));
                (game.id.clone(), may_run_fast)
            })
            .collect();
        Self {
            entries,
            pocketpc_import_options,
        }
    }
}

/// Frontend draw completions, never guest submissions or queued uploads.
#[derive(Debug, Clone, Default)]
struct FrameStats {
    state: std::sync::Arc<
        std::sync::Mutex<(
            Option<u64>,
            pocket_core::kernel::framebuffer::PresentationStats,
        )>,
    >,
}
impl FrameStats {
    fn reset(&mut self) {
        *self = Self::default();
    }
    fn record_frame(&self, image_id: u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.0 != Some(image_id) {
            state.0 = Some(image_id);
            state.1.record_present();
        }
    }
    fn current_fps(&self) -> f32 {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .fps()
            .unwrap_or(0.0)
    }
}

/// CPU-owned capture data is detached from any UI share group.
pub(crate) fn filtered_snapshot_image(
    frame: &FrameSnapshot,
    factor: u32,
    filter: image::imageops::FilterType,
) -> Result<image::RgbaImage, String> {
    let frame = frame.cpu_only()?;
    let source = image::RgbaImage::from_raw(frame.texture_width, frame.texture_height, frame.rgba)
        .ok_or_else(|| "invalid RGBA framebuffer size".to_string())?;
    if factor == 1 {
        return Ok(source);
    }
    Ok(image::imageops::resize(
        &source,
        frame.texture_width * factor,
        frame.texture_height * factor,
        filter,
    ))
}
impl PocketLauncher {
    pub fn new(cc: &eframe::CreationContext<'_>, library: Library) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(23, 25, 30);
        visuals.window_fill = Color32::from_rgb(41, 45, 54);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(37, 40, 48);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(41, 45, 54);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(65, 71, 82);
        visuals.selection.bg_fill = Color32::from_rgb(65, 71, 82);
        visuals.override_text_color = Some(Color32::from_rgb(230, 232, 237));
        cc.egui_ctx.set_visuals(visuals);
        let (tx, rx) = mpsc::channel();
        let profile = library.profile();
        let updates_tx = tx.clone();
        let updates_ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            use pocket_library::updates::{self, Platform};
            let platform = if cfg!(windows) {
                Platform::Windows
            } else {
                Platform::Linux
            };
            if let Some(update) = updates::check(env!("CARGO_PKG_VERSION"), platform) {
                let _ = updates_tx.send(UiEvent::ReleaseAvailable(update));
                updates_ctx.request_repaint();
            }
        });
        // CreationContext runs on the UI GL thread. The framework keeps its
        // context alive until on_exit, where we close this capability and join
        // the emulation worker before returning ownership to the framework.
        let shared_gpu_context = if SHARED_PRESENTATION_READY {
            cc.gl.as_ref().and_then(|_| unsafe {
                match pocket_core::winceapi::gles::ExternalGpuContext::capture_current(){
                    Ok(context)=>Some(context),
                    Err(error)=>{log::warn!("UI GPU sharing capability unavailable; using CPU presentation: {error}");None}
                }
            })
        } else {
            None
        };
        let runner = Runner::new(profile);
        runner.set_shared_context_source(shared_gpu_context.as_ref().map(|c| c.source()));
        runner.set_missing_api_logging(library.config().log_unimplemented_apis);
        let upscale_filter = UpscaleFilter::from_id(&library.config().upscale_filter);
        let library_gizmondo = library
            .games()
            .iter()
            .any(|g| is_gizmondo_game(g, library.root()));
        Self {
            library,
            library_tab: if library_gizmondo {
                crate::deck_navigation::LibraryTab::Gizmondo
            } else {
                crate::deck_navigation::LibraryTab::PocketPc
            },
            information_game: None,
            game_menu: None,
            game_menu_activate: false,
            game_menu_parent: None,
            controller_ui: Default::default(),
            recordings_gallery: Default::default(),
            icon_cache: std::collections::HashMap::new(),
            game_card_metadata: std::collections::HashMap::new(),
            screen_cache: std::collections::HashMap::new(),
            analysis_cache: std::collections::HashMap::new(),
            upscale_x2: false,
            upscale_filter,
            control_notice: None,
            deck_menu_press: None,
            deck_view_press: None,
            library_viewport: LibraryViewport::capture(&cc.egui_ctx),
            library_restore_pending: None,
            game_viewport_active: false,
            menu_host_press: Default::default(),
            display_fullscreen: false,
            startup_window: None,
            fullscreen_window_size: None,
            reconstruction: cc.gl.as_ref().and_then(|gl| {
                match crate::reconstruction::Reconstruction::new(
                    gl,
                    shared_gpu_context.as_ref().map(|c| c.consumer()),
                    runner.shared_export_policy(),
                    SHARED_PRESENTATION_READY,
                ) {
                    Ok(renderer) => Some(std::sync::Arc::new(std::sync::Mutex::new(renderer))),
                    Err(error) => {
                        log::warn!("GPU reconstruction unavailable: {error}");
                        None
                    }
                }
            }),
            screenshot: Default::default(),
            recording: Default::default(),
            selected_game: None,
            deck_library_actions: Vec::new(),
            deck_scroll_selection: false,
            library_columns: 1,
            deck_nav_repeat: None,
            rename_draft: None,
            remove_game: None,
            add_data: None,
            data_game: None,
            demo_import: None,
            pending_import: None,
            pocketpc_import_options: Default::default(),
            updating_import: false,
            library_busy: false,
            restore_archive: None,
            file_dialog: None,
            pending_run: None,
            colors_startup_prompt: None,
            screen: Screen::Library,
            about_parent: Screen::Library,
            runner,
            shared_gpu_context,
            run_thread: None,
            events_rx: rx,
            events_tx: tx,
            release_update: None,
            colors_startup_notice: None,
            frame_rx: None,
            gps_notice_rx: None,
            dependency_notice_rx: None,
            dependency_notice: None,
            gps_notice: None,
            input_tx: None,
            system_media: SystemMediaView::default(),
            media_keys_down: [false; 2],
            held: HeldButtons::default(),
            gamepad_rx: crate::gamepad::start(cc.egui_ctx.clone(), profile == pocket_library::LauncherProfile::SteamDeck),
            gamepad_devices: Vec::new(),
            gamepad_selected: None,
            gamepad_error: None,
            gamepad_held: Default::default(),
            pending_binding_commands: Default::default(),
            center_tap: Default::default(),
            pointer_down_at: None,
            running_game: None,
            status: String::new(),
            import_error: None,
            import_error_title: "Import failed",
            game_id_popup: None,
            config_draft: None,
            game_settings_draft: None,
            last_frame_texture: None,
            uploaded_image_id: None,
            last_frame_snapshot: None,
            last_frame_status: None,
            frame_stats: FrameStats::default(),
            rate_badge: None,
            game_rotation: RotationPref::None,
            running_game_id: None,
            running_is_gizmondo: false,
            input_gestures: Default::default(),
            input_gesture_clock: Instant::now(),
            input_controller: false,
            bindings_visual: Default::default(),
            stylus_keyboard: Default::default(),
            stylus_gamepad: Default::default(),
            stylus_touch: [0.0; 4],
            stylus_last: (0, 0),
            stylus_pointer: Default::default(),
            stylus_clock: Instant::now(),
            stylus_tick: Instant::now(),
            steering_axes: (0, 0),
            stylus_editor: None,
            stylus_grid_router: Default::default(),
            grid_modifiers: [false; 4],
            running_controls_mode: ControlsMode::Normal,
            running_swap_button_groups: false,
            running_stylus_grid: Default::default(),
        }
    }

    fn release_prompt_ready(&self) -> bool {
        self.release_update.is_some()
            && self.screen == Screen::Library
            && !self.library_busy
            && self.colors_startup_prompt.is_none()
            && self.demo_import.is_none()
            && self.pending_import.is_none()
            && self.rename_draft.is_none()
            && self.pending_run.is_none()
    }

    fn ui_release_update(&mut self, ctx: &egui::Context) -> bool {
        if !self.release_prompt_ready() {
            return false;
        }
        self.deck_nav_repeat = None;
        self.deck_library_actions.clear();
        let update = self.release_update.as_ref().unwrap();
        let mut download = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let mut dismiss = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.heading("PocketHLE update available");
                ui.label(format!(
                    "Installed: {}   Available: {}",
                    env!("CARGO_PKG_VERSION"),
                    update.version
                ));
                ui.add_space(12.0);
                download |= crate::controller_ui::button(ui, crate::controller_labels::text(ctx,"Download (Enter / {South})")).clicked();
                dismiss |= crate::controller_ui::button(ui, crate::controller_labels::text(ctx,"Later (Escape / {East})")).clicked();
            });
        });
        if download {
            ctx.open_url(egui::OpenUrl::new_tab(&update.url));
        }
        if download || dismiss {
            self.release_update = None;
            self.deck_library_actions.clear();
        }
        true
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.frame_rx.as_ref() {
            let native_wake = ctx.data(|data| {
                data.get_temp::<std::sync::Arc<dyn Fn() + Send + Sync>>(egui::Id::new(
                    "pockethle.native_image_wake",
                ))
            });
            let ctx = ctx.clone();
            rx.set_waker_once(std::sync::Arc::new(move || {
                if let Some(wake) = native_wake.as_ref() {
                    wake();
                } else {
                    ctx.request_repaint();
                }
            }));
        }
        while let Ok(ev) = self.events_rx.try_recv() {
            match ev {
                UiEvent::DataPrepared(result) => {
                    self.controller_ui.stop_repeat();
                    self.deck_library_actions.clear();
                    self.library_busy = false;
                    // Returning to the library during preparation cancels the
                    // staged operation, including its automatic no-conflict commit.
                    if self.data_game.take().is_none() {
                        self.status = "Add/Replace Data cancelled.".into();
                        continue;
                    }
                    match result {
                        Ok(pending) if !pending.executables().is_empty() => {
                            self.add_data = Some(DataPrompt::Executable(pending));
                        }
                        Ok(pending) => self.confirm_data(pending),
                        Err(message) => {
                            self.add_data = Some(DataPrompt::Notice {
                                title: "Add/Replace Data failed",
                                message,
                            });
                        }
                    }
                }
                UiEvent::DataFinished(result) => {
                    self.controller_ui.stop_repeat();
                    self.deck_library_actions.clear();
                    self.library_busy = false;
                    let (title, message) = match result {
                        Ok(message) => {
                            self.analysis_cache.clear();
                            self.game_card_metadata.clear();
                            self.screen_cache.clear();
                            self.icon_cache.clear();
                            self.reload_library();
                            ("Add/Replace Data complete", message)
                        }
                        Err(message) => ("Add/Replace Data failed", message),
                    };
                    self.add_data = Some(DataPrompt::Notice { title, message });
                }
                UiEvent::BackupFinished(result) => {
                    self.library_busy = false;
                    match result {
                        Ok(message) => {
                            self.status = message;
                            self.reload_library();
                            self.icon_cache.clear();
                            self.game_card_metadata.clear();
                            self.screen_cache.clear();
                            self.selected_game = None;
                        }
                        Err(error) => {
                            self.status = format!("Backup/restore failed: {error}");
                            self.import_error_title = "Backup/restore failed";
                            self.import_error = Some(error);
                        }
                    }
                }
                UiEvent::ColorsUnlocked(result) => {
                    self.library_busy = false;
                    self.colors_startup_notice = Some((
                        "Unlock Turf Wars + Tag Editor".into(),
                        result.unwrap_or_else(|error| {
                            format!("Could not unlock Colors menus: {error}")
                        }),
                    ));
                }
                UiEvent::ReleaseAvailable(update) => self.release_update = Some(update),
                UiEvent::ImportPrepared(pending) => {
                    self.pending_import = Some(pending);
                    self.status = "Waiting for import confirmation...".into();
                }
                UiEvent::ImportFinished(Ok(ImportedGames {
                    entries,
                    pocketpc_import_options,
                })) => {
                    self.pocketpc_import_options.extend(pocketpc_import_options);
                    self.library_busy = false;
                    let updated = self.updating_import;
                    self.updating_import = false;
                    self.status = format!(
                        "{} {} game(s).",
                        if updated {
                            "Imported or updated"
                        } else {
                            "Imported"
                        },
                        entries.len()
                    );
                    self.reload_library();
                    self.icon_cache.clear();
                    self.game_card_metadata.clear();
                    if let Some(game) = entries.first() {
                        let (id, name) = (game.id.clone(), game.display_name.clone());
                        self.library_tab = if is_gizmondo_game(game, self.library.root()) {
                            crate::deck_navigation::LibraryTab::Gizmondo
                        } else {
                            crate::deck_navigation::LibraryTab::PocketPc
                        };
                        self.selected_game = Some(id.clone());
                        if !updated && self.library.is_demo_card(&id) {
                            self.demo_import = Some((id, name));
                        } else if !updated
                            && entries.len() == 1
                            && !game.source_cab.starts_with("gzga-collection:")
                        {
                            self.rename_draft = Some((id, name, true));
                        }
                    }
                }
                UiEvent::ImportFinished(Err(e)) => {
                    self.updating_import = false;
                    self.library_busy = false;
                    self.status = format!("Import failed: {e}");
                    if e != "cancelled" {
                        self.import_error_title = "Import failed";
                        self.import_error = Some(e);
                    }
                    self.reload_library();
                }
                UiEvent::DemoSplitFinished(id, result) => {
                    self.library_busy = false;
                    self.reload_library();
                    self.icon_cache.clear();
                    self.game_card_metadata.clear();
                    match result {
                        Ok(entries) => {
                            self.status = format!("Imported {} separate games.", entries.len());
                            self.selected_game = entries.first().map(|e| e.id.clone());
                        }
                        Err(e) => {
                            self.status = format!("Could not split card: {e}");
                            if let Some(entry) = self.library.get(&id) {
                                self.rename_draft = Some((id, entry.display_name.clone(), true));
                            }
                        }
                    }
                }
                UiEvent::RunFinished(outcome) => {
                    if let Some(worker) = self.run_thread.take() {
                        let _ = worker.join();
                    }
                    self.last_frame_status = Some(outcome.summary.clone());
                    // A finished game's final image is not displayed in the
                    // library. Drop it and all UI-held sources before retirement.
                    drop(outcome.framebuffer);
                    self.last_frame_snapshot = None;
                    self.last_frame_texture = None;
                    self.uploaded_image_id = None;
                    if let Some(renderer) = self.reconstruction.as_ref() {
                        renderer
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .end_session();
                    }
                    self.status = outcome.summary;
                    self.frame_rx = None;
                    self.input_tx = None;
                    self.system_media = SystemMediaView::default();
                    self.release_all_keys();
                    self.pointer_down_at = None;
                    self.running_game = None;
                    if self.game_viewport_active && self.pending_run.is_none() {
                        self.return_to_library(ctx);
                    }
                }
            }
        }
        // A second game may be selected while the first worker is still
        // stopping. Wait for its completion event before replacing channels
        // or accepting a final snapshot under the new game's identity.
        if self.running_game.is_none() {
            if let Some(game) = self.pending_run.take() {
                self.spawn_run_selected(&game);
            }
        }
        // Drain any live preview frames the background runner may
        // have produced since the last UI tick.
        let mut latest: Option<FrameSnapshot> = None;
        if let Some(rx) = self.frame_rx.as_ref() {
            // The mailbox already discards superseded images on the producer.
            // Take once: a fast producer must not keep the UI draining forever.
            latest = rx.try_recv().ok();
        }
        if self.dependency_notice.is_none() {
            if let Some(rx) = &self.dependency_notice_rx {
                if let Ok(message) = rx.try_recv() {
                    self.dependency_notice = Some(message);
                }
            }
        }
        if let Some(message) = self.dependency_notice.clone() {
            egui::Window::new("PocketPC dependency")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label(message);
                    if crate::controller_ui::button(ui, "OK").clicked() {
                        self.dependency_notice = None;
                    }
                });
        }
        if let Some(rx) = &self.gps_notice_rx {
            while let Ok((lat, lon)) = rx.try_recv() {
                self.gps_notice = Some((
                    format!(
                        "Fake GPS position refreshed!\nLatitude: {lat:.6}\nLongitude: {lon:.6}"
                    ),
                    Instant::now() + Duration::from_secs(3),
                ));
            }
        }
        if let Some(frame) = latest {
            self.upload_frame_texture(ctx, &frame);
        }
    }

    fn upload_frame_texture(&mut self, ctx: &egui::Context, frame: &FrameSnapshot) {
        if self
            .last_frame_snapshot
            .as_ref()
            .map(|old| (old.width, old.height, old.texture_width, old.texture_height))
            != Some((
                frame.width,
                frame.height,
                frame.texture_width,
                frame.texture_height,
            ))
        {
            if self
                .last_frame_snapshot
                .as_ref()
                .is_some_and(|old| (old.width, old.height) != (frame.width, frame.height))
            {
                self.release_all_keys();
            }
        }
        if frame.text_input
            && !self
                .last_frame_snapshot
                .as_ref()
                .is_some_and(|f| f.text_input)
        {
            self.release_all_keys();
        }
        // Keep native pixels for input coordinates and rebuilding filtered images.
        self.last_frame_snapshot = Some(frame.clone());
        self.refresh_frame_texture(ctx, frame);
    }

    fn filtered_frame_image(
        &self,
        frame: &FrameSnapshot,
        factor: u32,
    ) -> Result<image::RgbaImage, String> {
        filtered_snapshot_image(frame, factor, self.upscale_filter.image_filter())
    }

    fn refresh_frame_texture(&mut self, ctx: &egui::Context, frame: &FrameSnapshot) {
        let smooth_scale = !matches!(
            self.upscale_filter,
            UpscaleFilter::None | UpscaleFilter::Nearest
        );
        if let Some(renderer) = self.reconstruction.as_ref() {
            renderer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .queue(frame, self.upscale_filter.gpu_filter());
        }
        if self.reconstruction.is_some() {
            if self.last_frame_texture.is_none() {
                self.last_frame_texture = Some(ctx.load_texture(
                    "pockethle-frame-ready",
                    egui::ColorImage::new([1, 1], Color32::BLACK),
                    egui::TextureOptions::NEAREST,
                ));
            }
            return;
        }
        let gpu_scale = self.reconstruction.is_some();
        let factor = if smooth_scale && !gpu_scale { 2 } else { 1 };
        let source = match self.filtered_frame_image(frame, factor) {
            Ok(source) => source,
            Err(error) => {
                log::warn!("{error}");
                return;
            }
        };
        let img = egui::ColorImage::from_rgba_unmultiplied(
            [source.width() as usize, source.height() as usize],
            source.as_raw(),
        );
        let options = if smooth_scale && self.upscale_filter != UpscaleFilter::Nearest {
            egui::TextureOptions::LINEAR
        } else {
            egui::TextureOptions::NEAREST
        };
        if let Some(tex) = self.last_frame_texture.as_mut() {
            tex.set(img, options);
        } else {
            self.last_frame_texture = Some(ctx.load_texture("pockethle-fb", img, options));
        }
        self.uploaded_image_id = Some(frame.image_id);
    }

    fn set_upscale_x2(&mut self, ctx: &egui::Context, enabled: bool) {
        if self.upscale_x2 == enabled {
            return;
        }
        self.release_all_keys();
        if let Some((x, y)) = self.pointer_down_at.take() {
            self.send_input(InputEvent::PointerUp { x, y });
        }
        self.upscale_x2 = enabled;
        if let Some(frame) = self.last_frame_snapshot.clone() {
            self.refresh_frame_texture(ctx, &frame);
        }
    }

    fn screenshot_path(&self) -> Result<std::path::PathBuf, String> {
        let dir = self.library.root().join("screenshots");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;

        let game = self.running_game.as_deref().unwrap_or("PocketHLE");
        let safe_name: String = game
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("system clock error: {e}"))?
            .as_millis();
        let path = dir.join(format!("{safe_name}_{millis}.png"));

        Ok(path)
    }

    fn toggle_recording(&mut self) {
        let mut recording = self.recording.lock().unwrap_or_else(|e| e.into_inner());
        if recording.active() {
            recording.stop();
            if let Some(tx) = &self.input_tx {
                let _ = tx.send(InputCommand::RecordAudio(None));
            }
            self.status = "Saving video…".into();
            return;
        }
        if self.screen != Screen::Run || self.last_frame_snapshot.is_none() {
            return;
        }
        recording.set_gizmondo(self.running_is_gizmondo);
        let directory = self.library.root().join("recordings");
        let result = std::fs::create_dir_all(&directory)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                let game: String = self
                    .running_game
                    .as_deref()
                    .unwrap_or("PocketHLE")
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_millis();
                recording.start(
                    directory.join(format!(
                        "{game}_{stamp}.{}",
                        if self.library.config().recording_format == "avi" {
                            "avi"
                        } else {
                            "mp4"
                        }
                    )),
                    &self.library.config().recording_quality,
                    self.library.config().recording_format != "avi",
                )
            });
        match result {
            Ok(audio) => {
                recording.set_gizmondo(self.running_is_gizmondo);
                recording.set_touch_steering_enabled(self.touch_steering_enabled());
                for vk in self
                    .held
                    .keyboard
                    .iter()
                    .chain(self.held.pointer.iter())
                    .chain(self.held.gamepad.iter())
                {
                    if !matches!(*vk, pocket_library::center_tap::COMMAND | pocket_library::keybindings::ROTATION_COMMAND | pocket_library::keybindings::SCREENSHOT_COMMAND | pocket_library::keybindings::STYLUS_CLICK_COMMAND) { recording.observe(InputEvent::KeyDown { vk: *vk }); }
                }
                recording.observe(InputEvent::TouchSteeringPosition {
                    x: self.steering_axes.0,
                    y: self.steering_axes.1,
                });
                if let Some(tx) = &self.input_tx {
                    if tx.send(InputCommand::RecordAudio(Some(audio))).is_err() {
                        recording.stop();
                        self.status = "Video failed: game is no longer running".into();
                        return;
                    }
                }
                self.status = "Recording video (F11 to stop)".into();
            }
            Err(e) => self.status = format!("Video failed: {e}"),
        }
    }

    fn capture_screenshot(&mut self) {
        if self.screen != Screen::Run || self.last_frame_snapshot.is_none() {
            self.status = "Screenshot failed: return to a visible game first.".into();
            return;
        }
        let mut capture = self.screenshot.lock().unwrap_or_else(|e| e.into_inner());
        if capture.busy {
            self.status = "Screenshot is already in progress.".into();
            return;
        }
        match self.screenshot_path() {
            Ok(path) => {
                capture.pending = Some(path);
                capture.busy = true;
                self.status = "Capturing filtered game screen…".into();
            }
            Err(e) => self.status = format!("Screenshot failed: {e}"),
        }
    }

    fn reload_library(&mut self) {
        match Library::open_with_profile(self.library.root(), self.library.profile()) {
            Ok(lib) => {
                self.library = lib;
                self.analysis_cache.clear();
                self.screen_cache.clear();
            }
            Err(e) => self.status = format!("Could not reload library: {e}"),
        }
    }

    fn ui_top_bar(&mut self, ui: &mut egui::Ui) {
        use crate::toolbar_ui::{Action, State};
        let selected = match self.screen {
            Screen::Library => Some(Action::Library),
            Screen::Settings => Some(Action::Emulator),
            Screen::GameSettings => Some(Action::Game),
            Screen::PocketPcOptions => Some(Action::PocketPc),
            Screen::GizmondoOptions => Some(Action::Gizmondo),
            Screen::About => Some(Action::About),
            Screen::InputSettings => Some(if self.input_controller {
                Action::Controller
            } else {
                Action::Keyboard
            }),
            Screen::Run => None,
        };
        let (recording, encoding) = {
            let r = self.recording.lock().unwrap_or_else(|e| e.into_inner());
            (r.active(), r.busy())
        };
        let state = State {
            version: env!("CARGO_PKG_VERSION"),
            selected,
            resume: self.running_game.is_some() && self.screen != Screen::Run,
            import: !self.library_busy && self.file_dialog.is_none(),
            backup: !self.library_busy && self.running_game.is_none() && self.pending_run.is_none(),
            game: self
                .running_game_id
                .as_ref()
                .or(self.selected_game.as_ref())
                .is_some_and(|id| self.library.get(id).is_some()),
            frame: self.last_frame_snapshot.is_some(),
            capture: self.screen == Screen::Run && self.last_frame_snapshot.is_some(),
            recording,
            encoding,
            upscale: self.upscale_x2,
            fullscreen: ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false)),
            filter: self.upscale_filter.toolbar_label(),
            filter_help: self.upscale_filter.label(),
            rotation: self.game_rotation.degrees(),
        };
        let output = crate::toolbar_ui::show(ui, &state);
        if let Some(action) = output.action {
            self.handle_toolbar_action(ui.ctx(), action);
        }
    }

    fn handle_toolbar_action(&mut self, ctx: &egui::Context, action: crate::toolbar_ui::Action) {
        use crate::toolbar_ui::Action;
        match action {
            Action::Library => self.return_to_library(ctx),
            Action::Import => self.spawn_import_dialog(),
            Action::Backup => self.spawn_backup_dialog(false),
            Action::Restore => self.spawn_backup_dialog(true),
            Action::Resume => {
                self.release_all_keys();
                self.screen = Screen::Run;
            }
            Action::Game => {
                let id = self
                    .running_game_id
                    .as_ref()
                    .or(self.selected_game.as_ref());
                let game = id.and_then(|id| self.library.get(id)).cloned();
                if let Some(game) = game {
                    self.release_all_keys();
                    self.game_settings_draft = Some((game.id, game.settings));
                    self.screen = Screen::GameSettings;
                }
            }
            Action::Emulator
            | Action::Keyboard
            | Action::Controller
            | Action::PocketPc
            | Action::Gizmondo => {
                self.release_all_keys();
                self.config_draft = Some(self.settings_draft(ctx));
                self.screen = match action {
                    Action::Emulator => Screen::Settings,
                    Action::PocketPc => Screen::PocketPcOptions,
                    Action::Gizmondo => Screen::GizmondoOptions,
                    _ => {
                        self.input_controller = action == Action::Controller;
                        self.bindings_visual.clear_live();
                        Screen::InputSettings
                    }
                };
            }
            Action::About => {
                if self.screen != Screen::About {
                    self.about_parent = self.screen;
                }
                self.release_all_keys();
                self.screen = Screen::About;
            }
            Action::Upscale => self.set_upscale_x2(ctx, !self.upscale_x2),
            Action::Filter => self.cycle_deck_upscale_filter(ctx, true),
            Action::Rotation => self.cycle_rotation(ctx),
            Action::Fullscreen => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                self.set_fullscreen(ctx, !full);
            }
            Action::Screenshot => self.capture_screenshot(),
            Action::Recording => self.toggle_recording(),
        }
        ctx.request_repaint();
    }

    fn close_about(&mut self) {
        self.screen = if self.about_parent == Screen::Run && self.running_game.is_none() {
            Screen::Library
        } else {
            self.about_parent
        };
    }

    fn ui_about(&mut self, ui: &mut egui::Ui) {
        crate::controller_ui::page_scope(ui, "about_page", |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                ui.heading(format!("PocketHLE v{}", env!("CARGO_PKG_VERSION")));
                crate::controller_ui::hyperlink_to(
                    ui,
                    "PocketHLE fork — guizmox",
                    "https://github.com/guizmox/PocketHLE",
                );
                crate::controller_ui::hyperlink_to(
                    ui,
                    "Original project — j92580498-max",
                    "https://github.com/j92580498-max/PocketHLE",
                );
                crate::controller_ui::hyperlink_to(
                    ui,
                    "GTShell support — Katamaran21",
                    "https://github.com/Katamaran21/PocketHLE-GTShell",
                );
                ui.label("Windows and Linux share this desktop frontend.");
                ui.separator();
                ui.strong("Thanks");
                egui::Grid::new("about_thanks")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for names in [
                            ["Nekomo", "OLego"],
                            ["TimofeyLednev", "Benoît (FX-man)"],
                            ["Jérôme (SoldatGG)", "Tim (Firefly)"],
                        ] {
                            ui.label(names[0]);
                            ui.label(names[1]);
                            ui.end_row();
                        }
                    });
                ui.separator();
                if crate::controller_ui::button(ui, "Back").clicked() {
                    self.close_about();
                }
                crate::controller_ui::scroll_focus(ui);
            });
        });
    }

    fn return_to_library(&mut self, ctx: &egui::Context) {
        self.remove_game = None;
        self.add_data = None;
        self.data_game = None;
        self.deck_library_actions.clear();
        self.pending_run = None;
        self.release_all_keys();
        if let Some(tx) = self.input_tx.as_ref() {
            let _ = tx.send(InputCommand::Stop);
        }
        // Reset image zoom without emitting an InnerSize while native
        // fullscreen may still be active. Restore from the library snapshot.
        self.upscale_x2 = false;
        self.fullscreen_window_size = None;
        self.library_restore_pending = Some(self.library_viewport);
        self.game_viewport_active = false;
        self.screen = Screen::Library;
        ctx.request_repaint();
    }

    fn settings_draft(&mut self, ctx: &egui::Context) -> LauncherConfig {
        let mut draft = self.library.config().clone();
        draft.fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
        for (file, platform) in [
            ("registry-pocketpc.json", &mut draft.device_platform_id),
            ("registry-gizmondo.json", &mut draft.gizmondo_platform_id),
        ] {
            if platform.is_empty() {
                let mut registry = pocket_core::kernel::registry::Registry::default();
                let result = registry
                    .configure_persistence(&self.library.root().join(file))
                    .map_err(|e| e.to_string())
                    .and_then(|_| {
                        pocket_core::kernel::device_identity::DeviceIdentity::load_or_create(
                            &mut registry,
                        )
                    });
                match result {
                    Ok(id) => {
                        *platform =
                            pocket_core::kernel::device_identity::DeviceIdentity::hex(&id.platform)
                    }
                    Err(e) => self.status = format!("Cannot load device identity: {e}"),
                }
            }
        }
        draft
    }

    fn ui_library(&mut self, ui: &mut egui::Ui) {
        let all: Vec<GameEntry> = self.library.games().to_vec();
        let (gizmondo, pocketpc): (Vec<_>, Vec<_>) = all
            .into_iter()
            .partition(|g| self.cached_game_card_metadata(g).0);
        let previous_tab = self.library_tab;
        ui.horizontal(|ui| {
            crate::controller_ui::selectable_value(
                ui,
                &mut self.library_tab,
                crate::deck_navigation::LibraryTab::Gizmondo,
                format!("Gizmondo ({})", gizmondo.len()),
            );
            crate::controller_ui::selectable_value(
                ui,
                &mut self.library_tab,
                crate::deck_navigation::LibraryTab::PocketPc,
                format!("PocketPC ({})", pocketpc.len()),
            );
            crate::controller_ui::selectable_value(
                ui,
                &mut self.library_tab,
                crate::deck_navigation::LibraryTab::Recordings,
                "Recordings",
            );
        });
        if self.library_tab != previous_tab {
            self.game_menu = None;
            self.game_menu_parent = None;
            self.deck_nav_repeat = None;
            ui.memory_mut(|m| m.close_popup());
        }
        ui.separator();
        if self.library_tab == crate::deck_navigation::LibraryTab::Recordings {
            let actions = std::mem::take(&mut self.deck_library_actions);
            self.recordings_gallery
                .show(ui, self.library.root(), actions);
            return;
        }
        let games = if self.library_tab == crate::deck_navigation::LibraryTab::Gizmondo {
            gizmondo
        } else {
            pocketpc
        };
        if self
            .game_menu
            .as_ref()
            .is_some_and(|(id, _)| !games.iter().any(|g| &g.id == id))
        {
            self.game_menu = None;
            ui.memory_mut(|m| m.close_popup());
        }
        if !self.gamepad_devices.is_empty() {
            ui.label(
                crate::controller_labels::text(ui.ctx(), "D-pad / stick: select • {South}: play • Shoulders / triggers: tabs • {North}: game options"),
            );
        }
        if games.is_empty() {
            self.deck_library_actions.clear();
            self.selected_game = None;
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No games yet")
                        .heading()
                        .color(Color32::from_gray(160)),
                );
                ui.add_space(8.0);
                ui.label("Use Import to add a game to this platform.");
                ui.add_space(20.0);
                if crate::controller_ui::button(ui, "Import .CAB / .ZIP / .RAR...").clicked() {
                    self.spawn_import_dialog();
                }
            });
            return;
        }

        if self
            .selected_game
            .as_ref()
            .is_none_or(|id| !games.iter().any(|g| &g.id == id))
        {
            self.selected_game = games.first().map(|g| g.id.clone());
        }
        {
            for control in std::mem::take(&mut self.deck_library_actions) {
                let index = self
                    .selected_game
                    .as_ref()
                    .and_then(|id| games.iter().position(|g| &g.id == id))
                    .unwrap_or(0);
                if let Some((_, selection)) = self.game_menu.as_mut() {
                    match control.as_str() {
                        "South" => self.game_menu_activate = true,
                        "East" | "North" => {
                            self.game_menu = None;
                            ui.memory_mut(|m| m.close_popup());
                            self.deck_nav_repeat = None;
                        }
                        direction => {
                            if let Some(next) =
                                crate::deck_navigation::menu_index(*selection, direction, GAME_MENU_LABELS.len())
                            {
                                *selection = next;
                            }
                        }
                    }
                    continue;
                }
                if control == "South" {
                    let game = games[index].clone();
                    self.deck_nav_repeat = None;
                    self.spawn_run(&game);
                    return;
                }
                if control == "North" {
                    let game = &games[index];
                    self.game_menu = Some((game.id.clone(), 0));
                    self.game_menu_activate = false;
                    ui.memory_mut(|m| m.open_popup(egui::Id::new("library_game_menu")));
                    self.deck_nav_repeat = None;
                    continue;
                }
                if let Some(next) = crate::deck_navigation::next_index(
                    index,
                    games.len(),
                    self.library_columns,
                    &control,
                ) {
                    self.selected_game = Some(games[next].id.clone());
                    self.deck_scroll_selection |= next != index;
                }
            }
        }
        let selected = self.selected_game.clone();

        ui.heading("Library");
        ui.add_space(8.0);
        ScrollArea::vertical().show(ui, |ui| {
            let gap = 12.0;
            let card_size = Vec2::new(190.0, 210.0);
            let columns = ((ui.available_width() + gap) / (card_size.x + gap))
                .floor()
                .max(1.0) as usize;
            self.library_columns = columns;
            egui::Grid::new("library_grid")
                .num_columns(columns)
                .min_col_width(card_size.x)
                .max_col_width(card_size.x)
                .min_row_height(card_size.y)
                .spacing(Vec2::splat(gap))
                .show(ui, |ui| {
                    for (index, game) in games.iter().enumerate() {
                        self.ui_game_card(
                            ui,
                            game,
                            card_size,
                            selected.as_deref() == Some(&game.id),
                        );
                        if (index + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                    if !games.len().is_multiple_of(columns) {
                        ui.end_row();
                    }
                });
        });
    }

    fn ui_game_information(&mut self, ctx: &egui::Context) {
        let Some(game) = self
            .information_game
            .as_ref()
            .and_then(|id| self.library.games().iter().find(|g| &g.id == id))
            .cloned()
        else {
            self.information_game = None;
            return;
        };
        let mut open = true;
        // Keep the title bar/close button inside the host viewport, including
        // small Deck windows and a viewport resized while this dialog is open.
        let bounds = ctx.screen_rect().shrink(8.0);
        let content_size = Vec2::new(
            (bounds.width() - 24.0).clamp(1.0, 560.0),
            (bounds.height() - 60.0).clamp(1.0, 540.0),
        );
        egui::Window::new("Information")
            .id(egui::Id::new("game_information"))
            .open(&mut open)
            .constrain_to(bounds)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(content_size)
            .collapsible(false)
            .show(ctx, |ui| self.ui_game_details(ui, &game));
        if !open || self.screen != Screen::Library {
            self.information_game = None;
        }
    }

    fn cached_game_card_metadata(&mut self, game: &GameEntry) -> (bool, Option<String>) {
        self.game_card_metadata
            .entry(game.id.clone())
            .or_insert_with(|| {
                (
                    is_gizmondo_game(game, self.library.root()),
                    game_gizmondo_title_id(game, self.library.root()),
                )
            })
            .clone()
    }

    fn cached_game_icon(
        &mut self,
        ctx: &egui::Context,
        game: &GameEntry,
    ) -> Option<(egui::TextureId, Vec2)> {
        let texture = self.icon_cache.entry(game.id.clone()).or_insert_with(|| {
            let path = game.icon_path(self.library.root())?;
            let bytes = std::fs::read(path).ok()?;
            let image = load_image_bytes(&bytes).ok()?;
            Some(ctx.load_texture(
                format!("icon-{}", game.id),
                image,
                egui::TextureOptions::LINEAR,
            ))
        });
        texture
            .as_ref()
            .map(|texture| (texture.id(), texture.size_vec2()))
    }

    fn ui_game_card(&mut self, ui: &mut egui::Ui, game: &GameEntry, size: Vec2, selected: bool) {
        let (card_rect, card_response) = ui.allocate_exact_size(size, Sense::click());
        if selected && self.deck_scroll_selection {
            ui.scroll_to_rect(card_rect, Some(egui::Align::Center));
            self.deck_scroll_selection = false;
        }
        if !ui.is_rect_visible(card_rect) {
            return;
        }
        let stroke = if selected {
            egui::Stroke::new(2.0_f32, Color32::from_rgb(95, 170, 225))
        } else {
            egui::Stroke::new(1.0_f32, Color32::from_rgb(82, 86, 96))
        };
        ui.painter()
            .rect(card_rect, 14.0, Color32::from_rgb(35, 38, 46), stroke);

        let menu_rect = Rect::from_min_size(
            Pos2::new(card_rect.right() - 36.0, card_rect.top() + 3.0),
            Vec2::splat(30.0),
        );
        let mut menu_ui = ui.child_ui(menu_rect, egui::Layout::right_to_left(egui::Align::Center));
        let popup_id = egui::Id::new("library_game_menu");
        let menu_response = crate::controller_ui::button(&mut menu_ui, "   ");
        if menu_response.clicked() {
            if self
                .game_menu
                .as_ref()
                .is_some_and(|(id, _)| id == &game.id)
            {
                self.game_menu = None;
                ui.memory_mut(|m| m.close_popup());
            } else {
                self.selected_game = Some(game.id.clone());
                self.game_menu = Some((game.id.clone(), 0));
                self.game_menu_activate = false;
                ui.memory_mut(|m| m.open_popup(popup_id));
            }
        }
        if let Some((_, selection)) = self.game_menu.as_ref().filter(|(id, _)| id == &game.id) {
            let selection = *selection;
            let activate = std::mem::take(&mut self.game_menu_activate);
            let mut chosen = None;
            egui::popup_below_widget(ui, popup_id, &menu_response, |ui| {
                ui.set_min_width(170.0);
                for (index, label) in GAME_MENU_LABELS.into_iter().enumerate()
                {
                    let enabled = index != 4 || (self.running_game.is_none() && self.pending_run.is_none());
                    if (ui.add_enabled(enabled, egui::SelectableLabel::new(index == selection, label)).clicked()
                        || (activate && index == selection)) && enabled
                    {
                        chosen = Some(index);
                    }
                }
            });
            if let Some(index) = chosen {
                self.game_menu = None;
                self.deck_nav_repeat = None;
                ui.memory_mut(|m| m.close_popup());
                self.game_menu_parent =
                    matches!(index, 0 | 2 | 3).then(|| (game.id.clone(), index));
                match index {
                    0 => {
                        self.analysis_cache.remove(&game.id);
                        self.information_game = Some(game.id.clone());
                    }
                    1 => self.spawn_run(game),
                    2 => {
                        self.rename_draft = Some((game.id.clone(), game.display_name.clone(), true))
                    }
                    3 => {
                        self.game_settings_draft = Some((game.id.clone(), game.settings.clone()));
                        self.screen = Screen::GameSettings;
                    }
                    4 => {
                        self.data_game = Some(game.id.clone());
                        self.open_file_dialog(crate::file_dialog::Purpose::AddData, None, String::new());
                    }
                    5 => {
                        self.remove_game=Some(crate::remove_game::Prompt {id:game.id.clone(),name:game.display_name.clone()});
                        self.deck_library_actions.clear();
                        self.deck_nav_repeat=None;
                        self.game_menu=None;
                        self.game_menu_parent=None;
                    }
                    6 => {
                        ui.ctx().output_mut(|o| o.copied_text = game.id.clone());
                        self.game_id_popup = Some(game.id.clone());
                    }
                    _ => unreachable!(),
                }
            } else if !ui.memory(|m| m.is_popup_open(popup_id)) {
                self.game_menu = None;
                self.deck_nav_repeat = None;
            }
        }

        let icon_rect = Rect::from_min_size(
            Pos2::new(card_rect.left(), card_rect.top() + 4.0),
            Vec2::new(size.x, 126.0),
        );
        let (giz, title_id) = self.cached_game_card_metadata(game);
        if giz {
            let artwork = if game.icon.as_deref() == Some(std::path::Path::new("cartridge.png")) {
                self.cached_game_icon(ui.ctx(), game)
            } else {
                None
            };
            draw_gizmondo_sd(
                ui,
                icon_rect,
                &game.display_name,
                title_id.as_deref(),
                artwork,
            );
        } else {
            let device = Rect::from_center_size(icon_rect.center(), Vec2::new(82.0, 116.0));
            let painter = ui.painter();
            painter.rect(
                device,
                10.0,
                Color32::from_rgb(98, 112, 130),
                egui::Stroke::new(1.5, Color32::from_rgb(158, 174, 193)),
            );
            let screen = Rect::from_min_max(
                device.min + Vec2::new(8.0, 11.0),
                device.max - Vec2::new(8.0, 24.0),
            );
            painter.rect_filled(screen, 3.0, Color32::from_rgb(38, 61, 80));
            let artwork = self.cached_game_icon(ui.ctx(), game);
            if let Some((texture, size)) = artwork {
                let available = screen.shrink(5.0).size();
                let scale = (available.x / size.x).min(available.y / size.y);
                painter.image(
                    texture,
                    Rect::from_center_size(screen.center(), size * scale),
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                painter.text(
                    screen.center(),
                    egui::Align2::CENTER_CENTER,
                    "Pocket PC",
                    egui::FontId::proportional(12.0),
                    Color32::from_rgb(198, 220, 234),
                );
            }
            painter.circle_filled(
                Pos2::new(device.center().x, device.bottom() - 12.0),
                5.0,
                Color32::from_rgb(204, 214, 225),
            );
        }

        let dots = menu_response.rect.center();
        for dx in [-6.0, 0.0, 6.0] {
            ui.painter().circle_filled(
                dots + Vec2::new(dx, 0.0),
                1.8,
                Color32::from_rgb(210, 218, 228),
            );
        }
        menu_response.clone().on_hover_text("Game actions");

        let label_rect = Rect::from_min_size(
            Pos2::new(card_rect.left(), card_rect.bottom() - 78.0),
            Vec2::new(size.x, 78.0),
        );
        ui.painter()
            .rect_filled(label_rect, 0.0, Color32::from_rgb(28, 29, 32));
        let mut text_ui = ui.child_ui(
            label_rect.shrink2(Vec2::new(10.0, 7.0)),
            egui::Layout::top_down(egui::Align::Min),
        );
        text_ui.spacing_mut().item_spacing.y = 1.0;
        library_title(&mut text_ui, &game.id, &game.display_name);
        let ((width, height), graphics_api) = match self.screen_cache.get(&game.id) {
            Some((pref, size, api)) if *pref == game.settings.screen => (*size, *api),
            _ => {
                let size = game.resolved_screen_size(self.library.root());
                let api = pocket_library::analysis::detect_graphics_api(
                    &game.executable_path(self.library.root()),
                );
                self.screen_cache
                    .insert(game.id.clone(), (game.settings.screen, size, api));
                (size, api)
            }
        };
        let caption = match graphics_api {
            Some(api) => format!("{width}×{height} • {api}"),
            None => format!("{width}×{height}"),
        };
        text_ui.label(
            RichText::new(caption)
                .size(12.0)
                .color(Color32::from_gray(185)),
        );
        if game.settings.has_custom_settings(self.library.config()) {
            text_ui.label(
                RichText::new("Custom settings")
                    .size(11.0)
                    .color(Color32::from_rgb(215, 181, 116)),
            );
        }
        if let Some(id) = title_id {
            text_ui.label(
                RichText::new(id)
                    .monospace()
                    .size(11.0)
                    .color(Color32::from_gray(145)),
            );
        }

        if card_response.clicked() && !menu_response.clicked() {
            self.selected_game = Some(game.id.clone());
        }
        if card_response.double_clicked() {
            self.spawn_run(game);
        }
    }

    fn ui_import_update(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_import.as_ref() else {
            return;
        };
        if !pending.cabinet_choices().is_empty() {
            let choices = pending.cabinet_choices().to_vec();
            let mut selected = None;
            let mut cancel = false;
            egui::Window::new("Choose installer cabinet")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("This installer contains several CABs. Which one should be imported?");
                    for choice in &choices {
                        if crate::controller_ui::button(
                            ui,
                            format!("{} — {}", choice.name, choice.architecture),
                        )
                        .clicked()
                        {
                            selected = Some(choice.index);
                        }
                    }
                    cancel = crate::controller_ui::button(ui, "Cancel import").clicked();
                });
            if cancel {
                self.pending_import = None;
                self.library_busy = false;
                self.status = "Import cancelled.".into();
            } else if let Some(index) = selected {
                let mut pending = self.pending_import.take().unwrap();
                let tx = self.events_tx.clone();
                self.status = "Preparing selected cabinet...".into();
                std::thread::spawn(move || match pending.choose_cabinet(index) {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::ImportPrepared(pending));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::ImportFinished(Err(error.to_string())));
                    }
                });
            }
            return;
        }
        if let Some((id, name)) = pending.platform_choices().first().cloned() {
            let mut choice = None;
            let mut cancel = false;
            egui::Window::new("Choose game platform").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(format!("No Gizmondo marker was found for \"{name}\". Which platform is this game for?"));
                ui.horizontal(|ui| {
                    if crate::controller_ui::button(ui, "Gizmondo").clicked() { choice=Some(pocket_library::GamePlatform::Gizmondo); }
                    if crate::controller_ui::button(ui, "Pocket PC").clicked() { choice=Some(pocket_library::GamePlatform::PocketPc); }
                    cancel=crate::controller_ui::button(ui, "Cancel import").clicked();
                });
            });
            if cancel {
                self.pending_import = None;
                self.library_busy = false;
                self.status = "Import cancelled.".into();
            } else if let Some(platform) = choice {
                self.pending_import
                    .as_mut()
                    .unwrap()
                    .choose_platform(&id, platform);
            }
            return;
        }
        if pending.updates().is_empty() {
            let pending = self.pending_import.take().unwrap();
            let profile = self.library.profile();
            let root = self.library.root().to_path_buf();
            let tx = self.events_tx.clone();
            self.status = "Importing...".into();
            std::thread::spawn(move || {
                let result = Library::open_with_profile(root, profile)
                    .and_then(|mut lib| {
                        let entries = lib.apply_prepared_import(pending, false)?;
                        Ok(ImportedGames::analyze(entries, &lib, &[]))
                    })
                    .map_err(|e| e.to_string());
                let _ = tx.send(UiEvent::ImportFinished(result));
            });
            return;
        }
        let names: Vec<_> = pending
            .updates()
            .iter()
            .map(|game| game.display_name.clone())
            .collect();
        let count = pending.entries().len();
        let mut update = false;
        let mut cancel = false;
        egui::Window::new("Update existing games?")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("These games are already installed:");
                ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for name in &names {
                        ui.label(name);
                    }
                });
                ui.label("Replace their game files and keep saves, names and settings?");
                if count > names.len() {
                    ui.label(format!(
                        "{} new game(s) will also be imported.",
                        count - names.len()
                    ));
                }
                ui.horizontal(|ui| {
                    cancel = crate::controller_ui::button(ui, "Cancel import").clicked();
                    update = crate::controller_ui::button(ui, "Update games").clicked();
                });
            });
        if cancel {
            self.pending_import = None;
            self.library_busy = false;
            self.status = "Import cancelled.".into();
        } else if update {
            let pending = self.pending_import.take().unwrap();
            let profile = self.library.profile();
            let root = self.library.root().to_path_buf();
            let tx = self.events_tx.clone();
            self.updating_import = true;
            self.status = "Updating game files...".into();
            std::thread::spawn(move || {
                let result = Library::open_with_profile(root, profile)
                    .and_then(|mut lib| {
                        let existing_ids: Vec<_> =
                            lib.games().iter().map(|game| game.id.clone()).collect();
                        let entries = lib.apply_prepared_import(pending, true)?;
                        Ok(ImportedGames::analyze(entries, &lib, &existing_ids))
                    })
                    .map_err(|e| e.to_string());
                let _ = tx.send(UiEvent::ImportFinished(result));
            });
        }
    }

    fn ui_pocketpc_import_options(&mut self, ctx: &egui::Context) -> bool {
        if self.screen != Screen::Library || self.rename_draft.is_some() {
            return false;
        }
        let Some((id, may_run_fast)) = self.pocketpc_import_options.front() else {
            return false;
        };
        let Some(game) = self.library.games().iter().find(|game| &game.id == id) else {
            self.pocketpc_import_options.pop_front();
            ctx.request_repaint();
            return false;
        };
        self.deck_nav_repeat = None;
        let mut open_settings = self.deck_library_actions.iter().any(|c| c == "East")
            || ctx.input(|i| i.key_pressed(egui::Key::B));
        let mut dismiss = self.deck_library_actions.iter().any(|c| c == "South")
            || ctx.input(|i| i.key_pressed(egui::Key::A));
        self.deck_library_actions.clear();
        self.controller_ui.stop_repeat();
        crate::controller_ui::claim_modal_input(ctx);
        egui::CentralPanel::default().show(ctx, |_| {});
        egui::Window::new("PocketPC display settings")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.strong(&game.display_name);
                ui.label("PocketPC games were designed for different screen sizes and orientations. If the game appears sideways, stretched, or cut off, you can adjust Screen and Rotate display in its launch settings.");
                if *may_run_fast {
                    ui.label("This game may run too fast at higher frame rates. If that happens, try setting an FPS limit in the same settings.");
                }
                ui.label("Would you like to open the launch settings now?");
                ui.horizontal(|ui| {
                    dismiss |= crate::controller_ui::button(ui, crate::controller_labels::text(ctx, "Not Now ({South})")).clicked();
                    open_settings |= crate::controller_ui::button(ui, crate::controller_labels::text(ctx, "Settings ({East})")).clicked();
                });
                ui.weak("Keyboard: A = Not Now · B = Settings");
            });
        if open_settings {
            self.selected_game = Some(game.id.clone());
            self.game_settings_draft = Some((game.id.clone(), game.settings.clone()));
            self.screen = Screen::GameSettings;
        }
        if open_settings || dismiss {
            self.pocketpc_import_options.pop_front();
            ctx.request_repaint();
        }
        self.controller_ui.show(ctx);
        true
    }

    fn ui_demo_import(&mut self, ctx: &egui::Context) {
        let Some((id, name)) = self.demo_import.clone() else {
            return;
        };
        let mut split = false;
        let mut keep = false;
        egui::Window::new("Import demo card").collapsible(false).resizable(false).show(ctx,|ui| {
            ui.label("Keep the original launcher or create a separate library entry for each game?");
            ui.label("Separate games each have their own directory. Shared card resources are copied.");
            keep=crate::controller_ui::button(ui, "Keep launcher").clicked();
            split=crate::controller_ui::button(ui, "Separate games").clicked();
        });
        if keep {
            self.demo_import = None;
            self.rename_draft = Some((id, name, true));
        } else if split {
            self.demo_import = None;
            self.library_busy = true;
            self.status = "Separating demo card games…".into();
            let profile = self.library.profile();
            let root = self.library.root().to_path_buf();
            let tx = self.events_tx.clone();
            std::thread::spawn(move || {
                let result = Library::open_with_profile(root, profile)
                    .and_then(|mut library| library.split_demo_card(&id))
                    .map_err(|e| e.to_string());
                let _ = tx.send(UiEvent::DemoSplitFinished(id, result));
            });
        }
    }

    fn ui_rename_game(&mut self, ctx: &egui::Context) {
        let Some((id, mut name, first_frame)) = self.rename_draft.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let mut open = true;
        egui::Window::new("Game name")
            .id(egui::Id::new("rename_game"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Game name");
                let edit = crate::controller_ui::text_edit(ui, &mut name, "");
                if first_frame {
                    edit.request_focus();
                }
                let valid = !name.trim().is_empty();
                ui.horizontal(|ui| {
                    save = crate::controller_ui::add_enabled(ui, valid, egui::Button::new("Save"))
                        .clicked();
                    cancel = crate::controller_ui::button(ui, "Cancel").clicked();
                });
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) && valid {
                    save = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        if cancel || !open {
            return;
        }
        if save {
            match self.library.rename_game(&id, &name) {
                Ok(()) => {
                    self.status = format!("Renamed game to {}", name.trim());
                    return;
                }
                Err(error) => {
                    self.status = format!("Rename failed: {error}");
                }
            }
        }
        self.rename_draft = Some((id, name, false));
    }

    fn ui_game_details(&mut self, ui: &mut egui::Ui, game: &GameEntry) {
        let root = self.library.root().to_path_buf();
        let analysis = self
            .analysis_cache
            .entry(game.id.clone())
            .or_insert_with(|| {
                let mut analysis = analyze_game(game, &root);
                if !is_gizmondo_game(game, &root) {
                    analysis.dependencies =
                        pocket_core::dependencies::information(&game.executable_path(&root), &root);
                }
                analysis
            })
            .clone();
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_source("game_details")
            .show(ui, |ui| {
                ui.heading(RichText::new(&game.display_name).size(24.0));
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&analysis.platform).strong());
                    if let Some(id) = &analysis.title_id {
                        ui.label(RichText::new(id).monospace());
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(
                            [110.0, 34.0],
                            egui::Button::new(RichText::new("▶  PLAY").strong()),
                        )
                        .clicked()
                    {
                        self.spawn_run(game);
                    }
                    if crate::controller_ui::button(ui, "Game settings").clicked() {
                        self.game_settings_draft = Some((game.id.clone(), game.settings.clone()));
                        self.screen = Screen::GameSettings;
                    }
                });
                ui.add_space(12.0);
                detail_section(ui, "IDENTITY", &analysis.identity);
                detail_section(ui, "EXECUTABLE", &analysis.executable);
                detail_section(ui, "DEPENDENCIES", &analysis.dependencies);
                detail_section(ui, "DISPLAY & GRAPHICS", &analysis.graphics);
                if is_gizmondo_game(game, &root) && !analysis.hardware.is_empty() {
                    detail_section(ui, "HARDWARE", &analysis.hardware);
                }
                if !analysis.game_data.is_empty() {
                    detail_section(ui, "GAME DATA", &analysis.game_data);
                }
                if !analysis.languages.is_empty() {
                    detail_section(ui, "LANGUAGES", &analysis.languages);
                }
                if !analysis.evidence.is_empty() {
                    egui::CollapsingHeader::new("Technical details / detection evidence").show(
                        ui,
                        |ui| {
                            for line in &analysis.evidence {
                                ui.label(RichText::new(line).monospace().size(11.0));
                            }
                        },
                    );
                }
            });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        let Some(mut draft) = self.config_draft.take() else {
            return;
        };
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        let gizmondo = self.screen == Screen::GizmondoOptions;
        let pocketpc = self.screen == Screen::PocketPcOptions;
        let input = self.screen == Screen::InputSettings;
        let scroll_id = if gizmondo {
            "gizmondo_options_scroll"
        } else if pocketpc {
            "pocketpc_options_scroll"
        } else {
            "emulator_options_scroll"
        };
        ScrollArea::vertical().id_source(scroll_id).auto_shrink([false,false]).show(ui,|ui| {
        ui.heading(if input { if self.input_controller { "External controller bindings" } else { "Keyboard bindings" } } else if gizmondo { "Gizmondo options" } else if pocketpc { "PocketPC options" } else { "Emulator Settings" });
        if input { self.ui_keybindings(ui, &mut draft);save_clicked |= self.bindings_visual.take_quick_save(); } else {
        if gizmondo {
        crate::controller_ui::checkbox(ui, &mut draft.gps_enabled, "GPS / host location (GPS1)")
            .on_hover_text("Allow games to read the host location service where available.");
        crate::controller_ui::checkbox(ui, &mut draft.gprs_enabled, "GPRS/data")
            .on_hover_text("Use the PC Internet connection for the Gizmondo data connection. Takes effect on the next launch.");
        crate::controller_ui::checkbox(ui, &mut draft.bluetooth_enabled, "Bluetooth hardware (Classic / RFCOMM)");
        crate::controller_ui::checkbox(ui, &mut draft.camera_enabled, "Camera hardware (CAM1)")
            .on_hover_text("Allow games to use the first Windows webcam. Takes effect on the next launch.");
        } else if pocketpc {
        egui::Grid::new("pocketpc_options_grid").num_columns(2).show(ui, |ui| {
                ui.label("PocketPC");
                ui.label("Global device profile (all PocketPC games)");
                ui.end_row();
                ui.label("Processor profile");
                crate::controller_ui::ComboBox::from_id_source("pocketpc_processor")
                    .selected_text(match draft.pocketpc_processor.as_str() {
                        "pxa255"=>"Intel PXA255", "sa1110"=>"StrongARM SA-1110", "arm"=>"Generic ARM", _=>"Intel PXA270 (Dell Axim X51v)",
                    }).show_ui(ui, |ui| {
                        for (id,label) in [("pxa270","Intel PXA270 (Dell Axim X51v)"),("pxa255","Intel PXA255"),("sa1110","StrongARM SA-1110"),("arm","Generic ARM")] {
                            crate::controller_ui::selectable_value(ui,&mut draft.pocketpc_processor,id.to_owned(),label);
                        }
                    });
                ui.end_row();
                ui.label("");
                ui.add(egui::Label::new("Identity reported to ARM PocketPC games; applies next launch. Does not change CPU speed or backend. MIPS games retain MIPS identity.").wrap(true));
                ui.end_row();
        });
        ui.separator();
        ui.heading("Graphics compatibility");
        crate::controller_ui::checkbox(ui, &mut draft.pocketpc_intel2700g_depth_bias,
            "Intel 2700G depth-bias compatibility");
        ui.add(egui::Label::new("Use if projected lights or ground decals show through foreground objects. Reproduces the original WinCE driver's integer constant bias without its slope term, with 24-bit depth. Applies to PocketPC games on the next launch, in software and GPU modes.").wrap(true));
        } else {
        let library_root = self.library.root().display().to_string();
        ui.add_space(10.0);
        ui.separator();
        ui.heading("Graphics");
        egui::Grid::new("emulator_graphics").num_columns(2).spacing(Vec2::new(12.0, 8.0)).show(ui, |ui| {
                ui.label("GPU rendering");
                crate::controller_ui::checkbox(ui, &mut draft.gpu_renderer, "").on_hover_text("Host OpenGL; takes effect on the next launch. Can be overridden per game.");
                ui.end_row();
                #[cfg(windows)]
                if !crate::gamepad_host::is_wine() {
                    ui.label("Graphics adapter");
                    ui.vertical(|ui| {
                        for name in crate::host_gpu::adapters() { ui.label(name); }
                        if crate::controller_ui::button(ui, "Choose GPU in Windows…").clicked() {
                            crate::host_gpu::open_settings();
                        }
                        ui.add(egui::Label::new("Select this PocketHLE executable in Windows Graphics settings, choose a GPU, then restart PocketHLE. By default, PocketHLE lets the system choose the GPU.").wrap(true));
                    });
                    ui.end_row();
                }
                ui.label("OpenGLES internal resolution (GPU only)");
                ui.add_enabled_ui(draft.gpu_renderer, |ui| {
                    crate::controller_ui::ComboBox::from_id_source("gles_internal_resolution")
                        .selected_text(match draft.gles_internal_resolution { 5 => "4x - only for 320x240 games", 4 => "3x - only for 320x240 games", 3 => "2x - only for 320x240 games", 2 => "2x - also for 640x480 games", _ => "Native (1×)" })
                        .show_ui(ui, |ui| {
                            crate::controller_ui::selectable_value(ui, &mut draft.gles_internal_resolution, 1, "Native (1×)");
                            crate::controller_ui::selectable_value(ui, &mut draft.gles_internal_resolution, 3, "2x - only for 320x240 games");
                            crate::controller_ui::selectable_value(ui, &mut draft.gles_internal_resolution, 4, "3x - only for 320x240 games");
                            crate::controller_ui::selectable_value(ui, &mut draft.gles_internal_resolution, 5, "4x - only for 320x240 games");
                            crate::controller_ui::selectable_value(ui, &mut draft.gles_internal_resolution, 2, "2x - also for 640x480 games");
                        });
                }).response.on_hover_text("Includes portrait equivalents (240x320 and 480x640). QVGA-only 2x/3x/4x modes leave larger surfaces native; the other 2x mode doubles them. Display zoom uses the rendered image; touch coordinates stay logical. More GPU/readback work; no extra detail for 2D assets. Applies next launch.");
                ui.end_row();
                ui.label("");
                ui.add(egui::Label::new("2x/3x/4x increases the rendered resolution; display zoom then scales that image. Applies next launch.").wrap(true));
                ui.end_row();
                ui.label("VSync (PC / Steam Deck)");
                crate::controller_ui::checkbox(ui, &mut draft.desktop_vsync, "Synchronize with the display");
                ui.end_row();
                ui.label("");
                ui.add(egui::Label::new("Limits window presentation to the display refresh rate and prevents tearing. Restart PocketHLE after changing this setting.").wrap(true));
                ui.end_row();
                ui.label("Frame rate limit");
                crate::controller_ui::ComboBox::from_id_source("global_frame_rate_limit")
                    .selected_text(draft.frame_rate_limit.label())
                    .show_ui(ui, |ui| {
                        for value in pocket_library::FrameRateLimit::ALL {
                            crate::controller_ui::selectable_value(ui, &mut draft.frame_rate_limit, value, value.label());
                        }
                    });
                ui.end_row();
                ui.label("");
                ui.add(egui::Label::new("Default maximum game FPS. Games can inherit this limit or choose another value, including Unlimited. Applies next launch.").wrap(true));
                ui.end_row();





        });
        ui.add_space(10.0);
        ui.separator();
        ui.heading("Video Recording");
        egui::Grid::new("emulator_recording").num_columns(2).spacing(Vec2::new(12.0, 8.0)).show(ui, |ui| {
                ui.label("Recording format");
                crate::controller_ui::ComboBox::from_id_source("recording_format").selected_text(if draft.recording_format=="avi"{"AVI (MJPEG + PCM)"}else{"MP4 (H.264 + AAC)"}).show_ui(ui,|ui|{
                    crate::controller_ui::selectable_value(ui, &mut draft.recording_format,"avi".into(),"AVI (MJPEG + PCM)");
                    ui.add_enabled_ui(pocket_core::winceapi::recording_encoder::available(),|ui|{crate::controller_ui::selectable_value(ui, &mut draft.recording_format,"mp4".into(),"MP4 (H.264 + AAC)");});
                });
                if !pocket_core::winceapi::recording_encoder::available(){ui.label(format!("MP4 unavailable: {}", pocket_core::winceapi::recording_encoder::unavailable_reason()));}
                ui.end_row();
                ui.label("Recording quality / frame rate");
                let selected = crate::recording::PRESETS.iter()
                    .find(|(value, _)| *value == draft.recording_quality.as_str())
                    .map(|(_, label)| *label).unwrap_or("Medium — 30 FPS");
                crate::controller_ui::ComboBox::from_id_source("recording_quality").selected_text(selected).show_ui(ui, |ui| {
                    for (value, label) in crate::recording::PRESETS {
                        crate::controller_ui::selectable_value(ui, &mut draft.recording_quality, value.into(), label);
                    }
                });
                ui.end_row();

        });
        ui.add_space(10.0);
        ui.separator();
        ui.heading("Diagnostics");
        egui::Grid::new("emulator_diagnostics").num_columns(2).spacing(Vec2::new(12.0, 8.0)).show(ui, |ui| {
                ui.label("Verbosity (0..3)");
                let slider = ui.add(egui::Slider::new(&mut draft.verbosity, 0..=3));
                crate::controller_ui::track_focus(ui, &slider);
                ui.end_row();

                ui.label("Log unimplemented APIs");
                crate::controller_ui::checkbox(ui, &mut draft.log_unimplemented_apis,"")
                    .on_hover_text("Write game, process and missing API details to pockethle-unimplemented-apis.log next to the main log. Changes apply after Save, including a running game.");
                ui.end_row();



        });
        ui.add_space(10.0);
        ui.separator();
        ui.heading("Misc");
        egui::Grid::new("emulator_misc").num_columns(2).spacing(Vec2::new(12.0, 8.0)).show(ui, |ui| {
                ui.label("Borderless fullscreen");
                if crate::controller_ui::checkbox(ui, &mut draft.fullscreen, "").changed() {
                    self.set_fullscreen(ui.ctx(), draft.fullscreen);
                }
                ui.end_row();
                ui.label("Show FPS on game screen");
                crate::controller_ui::checkbox(ui, &mut draft.show_fps, "");
                ui.end_row();
                ui.label("Show IPS on game screen")
                    .on_hover_text("Applies on next launch. Uses CPU pixel readback for OpenGL ES.");
                crate::controller_ui::checkbox(ui, &mut draft.show_ips, "");
                ui.end_row();
                ui.label("Executable heap");
                crate::controller_ui::ComboBox::from_id_source("emulator_executable_heap")
                    .selected_text(if draft.executable_heap { "Enabled" } else { "Disabled" })
                    .show_ui(ui, |ui| {
                        crate::controller_ui::selectable_value(ui, &mut draft.executable_heap, true, "Enabled");
                        crate::controller_ui::selectable_value(ui, &mut draft.executable_heap, false, "Disabled");
                    });
                ui.end_row();
                ui.label("");
                ui.add(egui::Label::new(EXECUTABLE_HEAP_HELP).wrap(true));
                ui.end_row();
                ui.label("Library root");
                ui.label(library_root);
                ui.end_row();
        });

        ui.add_space(12.0);
        }

        if gizmondo || pocketpc {
        ui.separator();
        ui.label("Device identity");
        ui.label(if gizmondo { "Used instead of the WinCE identity for Gizmondo titles." } else { "Used by WinCE applications outside the Gizmondo profile." });
        ui.label(if gizmondo { "Device name (Preset ID)" } else { "Device name (Preset ID — ASCII)" });
        match draft.preset_text(gizmondo) {
            Ok(mut text) => {
                if crate::controller_ui::text_edit(ui, &mut text, "").changed() {
                    if let Err(e) = draft.set_preset_text(gizmondo, &text) { self.status = e; }
                }
            }
            Err(e) => { ui.colored_label(Color32::RED, e); }
        }
        let platform = if gizmondo { &mut draft.gizmondo_platform_id } else { &mut draft.device_platform_id };
        ui.label("Console identifier (Platform ID — hexadecimal)");
        crate::controller_ui::text_edit(ui, platform, "");
        ui.label("Device names are encoded automatically. Changes apply at the next launch.");
        }

        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if crate::controller_ui::button(ui, "Save").clicked() {
                save_clicked = true;
            }
            if crate::controller_ui::button(ui, "Cancel").clicked() {
                cancel_clicked = true;
            }
        });
        crate::controller_ui::scroll_focus(ui);
        });
        if save_clicked && (gizmondo || pocketpc) {
            if draft
                .preset_text(gizmondo)
                .map(|s| s.is_empty())
                .unwrap_or(false)
            {
                self.status = "Enter a device name".into();
                self.config_draft = Some(draft);
                return;
            }

            let (preset, platform) = if gizmondo {
                (&draft.gizmondo_preset_id, &draft.gizmondo_platform_id)
            } else {
                (&draft.device_preset_id, &draft.device_platform_id)
            };
            if let Err(e) =
                pocket_core::kernel::device_identity::DeviceIdentity::from_hex(preset, platform)
            {
                self.status = e;
                self.config_draft = Some(draft);
                return;
            }
        }
        if save_clicked {
            self.release_all_keys();
            self.runner
                .set_missing_api_logging(draft.log_unimplemented_apis);
            *self.library.config_mut() = draft;
            if let Err(e) = self.library.save() {
                self.status = format!("Could not save settings: {e}");
            } else {
                self.status = "Settings saved.".to_string();
            }
            self.screen = if self.running_game.is_some() {
                Screen::Run
            } else {
                Screen::Library
            };
        } else if cancel_clicked {
            self.stylus_editor = None;
            self.screen = if self.running_game.is_some() {
                Screen::Run
            } else {
                Screen::Library
            };
        } else {
            self.config_draft = Some(draft);
        }
    }

    /// One input-first assistant; the device picture and summary only inspect
    /// bindings. Changes are persisted by Save or the assistant's Save and finish.
    fn ui_keybindings(&mut self, ui: &mut egui::Ui, draft: &mut LauncherConfig) {
        self.bindings_visual.profile(ui);
        self.bindings_visual
            .quick_assign(ui, draft, self.input_controller);
        if self.bindings_visual.quick_active() {
            ui.set_enabled(false);
        }
        let controller = self.input_controller;
        if controller {
            ui.separator();
            ui.heading("Controller");
            ui.label("Connect your controller by USB or pair it in the operating system's Bluetooth settings.");
            if let Some(error) = &self.gamepad_error {
                ui.colored_label(
                    Color32::LIGHT_RED,
                    format!("Controller input unavailable: {error}"),
                );
            }
            if self.gamepad_devices.is_empty() {
                ui.label("No controller detected.");
            } else {
                let old = self.gamepad_selected;
                crate::controller_ui::ComboBox::from_id_source("selected_controller")
                    .selected_text(
                        self.gamepad_devices
                            .iter()
                            .find(|(id, _)| Some(*id) == old)
                            .map(|(_, name)| name.as_str())
                            .unwrap_or("Controller"),
                    )
                    .show_ui(ui, |ui| {
                        for (id, name) in &self.gamepad_devices {
                            crate::controller_ui::selectable_value(
                                ui,
                                &mut self.gamepad_selected,
                                Some(*id),
                                name,
                            );
                        }
                    });
                if old != self.gamepad_selected {
                    if let Some(device)=self.gamepad_selected {crate::controller_labels::activate(ui.ctx(),device);}
                    self.bindings_visual.clear_live();
                    self.deck_nav_repeat = None;
                    self.deck_menu_press = None;
                    self.deck_view_press = None;
                    let keys: Vec<_> = self.gamepad_held.keys().cloned().collect();
                    for key in keys {
                        self.release_gamepad_control(&key);
                    }
                }
            }
            if !ui.ctx().input(|i| i.viewport().focused.unwrap_or(true)) {
                ui.colored_label(
                    Color32::YELLOW,
                    "Window inactive: controller assignment is paused.",
                );
            }
        }
        self.bindings_visual.show(ui, draft, controller);
        if crate::controller_ui::button(
            ui,
            if controller {
                "Reset controller defaults"
            } else {
                "Reset keys to defaults"
            },
        )
        .clicked()
        {
            let marker = if controller {
                ":controller:"
            } else {
                ":keyboard:"
            };
            draft
                .short_press_bindings
                .retain(|key, _| !key.contains(marker));
            draft
                .long_press_bindings
                .retain(|key, _| !key.contains(marker));
            if controller {
                draft.gamepad_bindings = pocket_library::keybindings::steam_deck_gamepad_bindings();
                draft.gamepad_fullscreen = Some("Select".into());
            } else {
                draft.keybindings = pocket_library::KeyBindings::default();
            }
        }
        if controller {
            egui::CollapsingHeader::new("Controller shortcuts").show(ui, |ui| {
                ui.colored_label(
                    crate::bindings_visual::PURPLE,
                    "View tap: fullscreen · Menu tap: next filter",
                );
                ui.colored_label(
                    crate::bindings_visual::LONG_PRESS,
                    "View hold: return to library · Menu hold: recording",
                );
                ui.label("Library shoulders / triggers: previous / next catalogue tab");
            });
        }
        if !self.bindings_visual.is_gizmondo() {
            ui.separator();
            ui.heading("Stylus");
            if controller {
                ui.label("Double-click R3 to switch stylus mode. Stick: automatic drags, single click unused. Pointer: move the cursor, single click to tap. Right stick and R3 are reserved.");
            } else {
                ui.label("Stylus click / mode: P by default. Double-press to switch mode; single press taps in Pointer mode. Reassign this key and stylus directions with Assign an input. Current stylus keys cannot be used in per-game grids.");
            }
            let slider = ui.add(
                egui::Slider::new(&mut draft.stylus_stick.speed, 30..=1200)
                    .text("Stylus speed (pixels)"),
            );
            crate::controller_ui::track_focus(ui, &slider);
        }
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
        let is_gizmondo = self
            .library
            .get(&id)
            .is_some_and(|game| is_gizmondo_game(game, self.library.root()));
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        ui.heading(format!("Settings: {display_name}"));
        ui.add_space(8.0);
        ui.separator();
        ui.heading("Controls");
        egui::Grid::new("game_settings_controls")
            .num_columns(2)
            .spacing(Vec2::new(12.0, 8.0))
            .show(ui, |ui| {
                ui.label("D-pad dead zone");
                crate::controller_ui::ComboBox::from_id_source("game_dpad_deadzone")
                    .selected_text(draft.dpad_deadzone.label())
                    .show_ui(ui, |ui| {
                        for value in DpadDeadzone::ALL {
                            crate::controller_ui::selectable_value(ui, &mut draft.dpad_deadzone, value, value.label());
                        }
                    });
                ui.end_row();
                if !is_gizmondo {
                    ui.label("Swap action / P buttons");
                    crate::controller_ui::checkbox(ui, &mut draft.pocketpc_swap_button_groups, "Enabled")
                        .on_hover_text("Exchange A/B/C/Action with P1–P4 on the on-screen controls. Also swaps configured keyboard and gamepad inputs. Applies on the next launch.");
                    ui.end_row();
                    ui.label("Control mode");
                    let mut mode=draft.controls_mode();
                    crate::controller_ui::ComboBox::from_id_source("ppc_controls_mode")
                        .selected_text(mode.label()).show_ui(ui,|ui|{
                            for value in ControlsMode::ALL {crate::controller_ui::selectable_value(ui,&mut mode,value,value.label());}
                        });
                    draft.set_controls_mode(mode);
                    ui.end_row();
                    ui.label("");
                    ui.vertical(|ui|{
                        ui.set_max_width(380.0);
                        ui.add(egui::Label::new(mode.description()).wrap(true));
                        ui.label(RichText::new("Follows screen rotation. Applies at the next game launch.").small().color(Color32::GRAY));
                        if mode != ControlsMode::Normal && crate::controller_ui::button(ui,if mode==ControlsMode::Zones {"Test controls…"}else{"Configure grid…"}).clicked(){
                            let (w,h)=draft.screen.size();
                            let portrait=if rotation_turns(draft.rotation)%2==0 {h>w}else{w>h};
                            self.controller_ui.stop_repeat();
                            self.stylus_editor=Some(StylusEditor::new(mode==ControlsMode::Zones,portrait));
                        }
                    });
                    ui.end_row();
                }
            });
        ui.add_space(10.0);
        ui.separator();
        ui.heading("Graphics");
        egui::Grid::new("game_settings_graphics")
            .num_columns(2)
            .spacing(Vec2::new(12.0, 8.0))
            .show(ui, |ui| {
                ui.label("Frame rate limit");
                let inherited_limit_label = format!("Emulator default ({})", self.library.config().frame_rate_limit.label());
                crate::controller_ui::ComboBox::from_id_source("game_frame_rate_limit")
                    .selected_text(draft.frame_rate_limit.map(|v| v.label().to_owned()).unwrap_or_else(|| inherited_limit_label.clone()))
                    .show_ui(ui, |ui| {
                        crate::controller_ui::selectable_value(ui, &mut draft.frame_rate_limit, None, inherited_limit_label.as_str());
                        for value in pocket_library::FrameRateLimit::ALL {
                            crate::controller_ui::selectable_value(ui, &mut draft.frame_rate_limit, Some(value), value.label());
                        }
                    });
                ui.end_row();
                ui.label("");
                ui.vertical(|ui| {
                    ui.set_max_width(380.0);
                    ui.add(egui::Label::new("Maximum game FPS, or Unlimited. Emulator default inherits the global limit; an explicit value overrides it. Restart the game to apply.").wrap(true))
                        .on_hover_text("Caps GAPI, DirectDraw and OpenGL ES presentations, not video recording FPS. GDI-only drawing is not capped. A cap does not speed up a slow game.");
                });
                ui.end_row();
                if is_gizmondo {
                    ui.label("Screen");
                    let portrait = draft.screen == ScreenPref::Qvga;
                    crate::controller_ui::ComboBox::from_id_source("game_screen")
                        .selected_text(if portrait { "QVGA portrait (240 × 320)" } else { "Gizmondo default (320 × 240)" })
                        .show_ui(ui, |ui| {
                            let mut choice = portrait;
                            crate::controller_ui::selectable_value(ui, &mut choice, false, "Gizmondo default (320 × 240)");
                            crate::controller_ui::selectable_value(ui, &mut choice, true, "QVGA portrait (240 × 320)");
                            if choice != portrait {
                                draft.screen = if choice { ScreenPref::Qvga } else { ScreenPref::Portrait };
                            }
                        });
                    ui.end_row();
                } else {
                    ui.label("Screen");
                    crate::controller_ui::ComboBox::from_id_source("game_screen")
                        .selected_text(draft.screen.label())
                        .show_ui(ui, |ui| {
                            for pref in [
                                ScreenPref::Portrait,
                                ScreenPref::Qvga,
                                ScreenPref::Landscape,
                                ScreenPref::SmallPortrait,
                                ScreenPref::Vga,
                                ScreenPref::VgaLandscape,
                                ScreenPref::Wvga,
                                ScreenPref::Hpc,
                            ] {
                                crate::controller_ui::selectable_value(ui, &mut draft.screen, pref, pref.label());
                            }
                        });
                    ui.end_row();
                }

                // Presentation-only: the guest keeps rendering into
                // whatever `Screen` says. This is the knob for a
                // landscape game that shipped as a 240x320 portrait
                // build — forcing `Screen` to 320x240 makes some of
                // them lay out against geometry they never shipped
                // against, turning the frame instead always works.
                ui.label("Rotate display");
                crate::controller_ui::ComboBox::from_id_source("game_rotation_pref")
                    .selected_text(draft.rotation.label())
                    .show_ui(ui, |ui| {
                        for choice in RotationPref::ALL {
                            crate::controller_ui::selectable_value(ui, &mut draft.rotation, choice, choice.label());
                        }
                    });
                ui.end_row();

                ui.label("GPU rendering");
                crate::controller_ui::ComboBox::from_id_source("game_gpu_renderer").selected_text(match draft.gpu_renderer { None => "Emulator default", Some(false) => "Software", Some(true) => "GPU" }).show_ui(ui, |ui| {
                    crate::controller_ui::selectable_value(ui, &mut draft.gpu_renderer, None, "Emulator default");
                    crate::controller_ui::selectable_value(ui, &mut draft.gpu_renderer, Some(false), "Software");
                    crate::controller_ui::selectable_value(ui, &mut draft.gpu_renderer, Some(true), "GPU");
                });
                ui.end_row();



                if !is_gizmondo {
                    ui.label("GAPI display compatibility");
                    crate::controller_ui::ComboBox::from_id_source("pocketpc_gapi_layout")
                        .selected_text(if draft.pocketpc_gapi_columns { "Rotated LCD columns" } else { "Linear scanlines (default)" })
                        .show_ui(ui, |ui| {
                            crate::controller_ui::selectable_value(ui, &mut draft.pocketpc_gapi_columns, false, "Linear scanlines (default)");
                            crate::controller_ui::selectable_value(ui, &mut draft.pocketpc_gapi_columns, true, "Rotated LCD columns");
                        }).response.on_hover_text("Changes the physical layout of direct framebuffer memory. Use columns for software designed for a rotated LCD. Applies on next launch.");
                    ui.end_row();
                }
            });
        ui.add_space(10.0);
        ui.separator();
        ui.heading("Sound");
        ui.label("No per-game sound options yet.");
        if !is_gizmondo {
            ui.add_space(10.0);
            ui.separator();
            ui.heading("Misc");
            egui::Grid::new("game_settings_misc")
                .num_columns(2)
                .spacing(Vec2::new(12.0, 8.0))
                .show(ui, |ui| {
                    ui.label("Executable heap");
                    crate::controller_ui::ComboBox::from_id_source("game_executable_heap")
                        .selected_text(match draft.executable_heap {
                            None => "Use emulator setting",
                            Some(true) => "Enabled",
                            Some(false) => "Disabled",
                        })
                        .show_ui(ui, |ui| {
                            crate::controller_ui::selectable_value(ui, &mut draft.executable_heap, None, "Use emulator setting");
                            crate::controller_ui::selectable_value(ui, &mut draft.executable_heap, Some(true), "Enabled");
                            crate::controller_ui::selectable_value(ui, &mut draft.executable_heap, Some(false), "Disabled");
                        });
                    ui.end_row();
                    ui.label("");
                    ui.add(egui::Label::new(EXECUTABLE_HEAP_HELP).wrap(true));
                    ui.end_row();
                });
            ui.add_space(10.0);
            ui.separator();
            ui.heading("Device Identity");
            egui::Grid::new("game_settings_identity")
                .num_columns(2)
                .spacing(Vec2::new(12.0, 8.0))
                .show(ui, |ui| {
                    ui.label("OEM model (readable text)");
                    crate::controller_ui::text_edit(ui, &mut draft.pocketpc_oem_info, "PocketHLE");
                    ui.end_row();
                    ui.label("Platform type (readable text)");
                    crate::controller_ui::text_edit(ui, &mut draft.pocketpc_platform_type, "PocketPC");
                    ui.end_row();
                    ui.label("Random seed override (compatibility)");
                    crate::controller_ui::text_edit(ui, &mut draft.pocketpc_random_seed, "Empty = normal randomness");
                    ui.end_row();
                    ui.label("");
                    ui.add(egui::Label::new("Use to reproduce a legacy registration request. Decimal value; overrides every srand seed for this game. Not a hardware identity field. Leave empty for normal randomness.").wrap(true));
                    ui.end_row();
                    ui.label("Owner name (PocketPC)");
                    crate::controller_ui::text_edit(ui, &mut draft.pocketpc_owner_name, "Device default");
                    ui.end_row();
                    ui.label("Device name (Preset ID — ASCII)");
                    crate::controller_ui::text_edit(ui, &mut draft.pocketpc_device_name, "Device default");
                    ui.end_row();
                    ui.label("Preset ID override (hexadecimal)");
                    crate::controller_ui::text_edit(ui, &mut draft.pocketpc_preset_id, "Empty = use device name/default");
                    ui.end_row();
                    ui.label("PocketPC ID (Platform ID — hexadecimal)");
                    crate::controller_ui::text_edit(ui, &mut draft.pocketpc_device_id, "Device default");
                    ui.end_row();
                });
        }

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
            if crate::controller_ui::button(ui, "Save").clicked() {
                save_clicked = true;
            }
            if crate::controller_ui::button(ui, "Cancel").clicked() {
                cancel_clicked = true;
            }
        });
        if save_clicked {
            self.stylus_editor = None;
            self.release_all_keys();
            if let Err(e) = self.library.update_settings(&id, draft) {
                self.status = format!("Could not save game settings: {e}");
            } else {
                self.status = "Game settings saved.".to_string();
            }
            self.screen = if self.running_game.is_some() {
                Screen::Run
            } else {
                Screen::Library
            };
        } else if cancel_clicked {
            self.stylus_editor = None;
            self.screen = if self.running_game.is_some() {
                Screen::Run
            } else {
                Screen::Library
            };
        } else {
            self.game_settings_draft = Some((id, draft));
        }
    }

    fn set_fullscreen(&mut self, ctx: &egui::Context, enabled: bool) {
        self.release_all_keys();
        if let Some((x, y)) = self.pointer_down_at.take() {
            self.send_input(InputEvent::PointerUp { x, y });
        }
        if enabled && !ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
            self.fullscreen_window_size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(enabled));
    }

    fn paint_stylus_pointer(&self, ui: &egui::Ui, rect: Rect) {
        if self.running_is_gizmondo || !self.stylus_pointer.pointer {
            return;
        }
        let (Some(frame), Some((x, y))) = (
            self.last_frame_snapshot.as_ref(),
            self.stylus_pointer.position(),
        ) else {
            return;
        };
        let (x, y) = (f32::from(x), f32::from(y));
        let (w, h) = (frame.width as f32, frame.height as f32);
        let (u, v) = match rotation_turns(self.game_rotation) {
            1 => ((h - 1.0 - y) / h, x / w),
            2 => ((w - 1.0 - x) / w, (h - 1.0 - y) / h),
            3 => (y / h, (w - 1.0 - x) / w),
            _ => (x / w, y / h),
        };
        let point = rect.min + Vec2::new(u * rect.width(), v * rect.height());
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        for (width, color) in [(4.0, Color32::BLACK), (2.0, Color32::WHITE)] {
            painter.line_segment(
                [point - Vec2::new(7.0, 0.0), point + Vec2::new(7.0, 0.0)],
                egui::Stroke::new(width, color),
            );
            painter.line_segment(
                [point - Vec2::new(0.0, 7.0), point + Vec2::new(0.0, 7.0)],
                egui::Stroke::new(width, color),
            );
        }
    }

    fn paint_guest_frame(&mut self, ui: &egui::Ui, rect: Rect) {
        if !rect.intersect(ui.clip_rect()).is_positive() {
            let mut capture = self.screenshot.lock().unwrap_or_else(|e| e.into_inner());
            if capture.pending.take().is_some() {
                capture.busy = false;
                capture.completed = Some(Err("game screen is not visible".into()));
            }
            return;
        }
        let uv = rotation_uv(self.game_rotation);
        if let Some(renderer) = self.reconstruction.as_ref() {
            let renderer = std::sync::Arc::clone(renderer);
            let frame_stats = self.frame_stats.clone();
            let filter = self.upscale_filter.gpu_filter();
            let capture = std::sync::Arc::clone(&self.screenshot);
            let recording = self.recording.clone();
            let video_size = self
                .last_frame_snapshot
                .as_ref()
                .map(|f| (f.width, f.height))
                .unwrap_or((1, 1));
            let video_turns = rotation_turns(self.game_rotation);
            let ctx = ui.ctx().clone();
            let transform = [
                uv[0].x,
                uv[0].y,
                uv[1].x - uv[0].x,
                uv[1].y - uv[0].y,
                uv[2].x - uv[0].x,
                uv[2].y - uv[0].y,
            ];
            ui.painter().add(egui::PaintCallback {
                rect,
                callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(
                    move |info, painter| {
                        let mut renderer = renderer.lock().unwrap_or_else(|e| e.into_inner());
                        renderer.paint(painter.gl(), transform, filter);
                        if let Some(image_id) = renderer.drawn_image_id() {
                            frame_stats.record_frame(image_id);
                        }
                        drop(renderer);
                        crate::screenshot::capture(
                            painter.gl(),
                            egui::PaintCallbackInfo {
                                viewport: info.viewport,
                                clip_rect: info.clip_rect,
                                pixels_per_point: info.pixels_per_point,
                                screen_size_px: info.screen_size_px,
                            },
                            &capture,
                            &ctx,
                        );
                        crate::recording::capture(
                            painter.gl(),
                            info,
                            &recording,
                            video_size,
                            video_turns,
                        );
                    },
                )),
            });
            self.paint_stylus_pointer(ui, rect);
            self.paint_recording_indicator(ui, rect);
            self.paint_rate_indicators(ui, rect);
            self.ui_system_media(ui.ctx(), rect);
            return;
        }
        if let Some(tex) = self.last_frame_texture.as_ref() {
            let mut mesh = Mesh::with_texture(tex.id());
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
            mesh.indices.extend_from_slice(&[0, 1, 2, 2, 1, 3]);
            ui.painter().add(egui::Shape::mesh(mesh));
            {
                let frame_stats = self.frame_stats.clone();
                let image_id = self.uploaded_image_id;
                let capture = std::sync::Arc::clone(&self.screenshot);
                let recording = self.recording.clone();
                let video_size = self
                    .last_frame_snapshot
                    .as_ref()
                    .map(|f| (f.width, f.height))
                    .unwrap_or((1, 1));
                let video_turns = rotation_turns(self.game_rotation);
                let ctx = ui.ctx().clone();
                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(
                        move |info, painter| {
                            if let Some(image_id) = image_id {
                                frame_stats.record_frame(image_id);
                            }
                            crate::screenshot::capture(
                                painter.gl(),
                                egui::PaintCallbackInfo {
                                    viewport: info.viewport,
                                    clip_rect: info.clip_rect,
                                    pixels_per_point: info.pixels_per_point,
                                    screen_size_px: info.screen_size_px,
                                },
                                &capture,
                                &ctx,
                            );
                            crate::recording::capture(
                                painter.gl(),
                                info,
                                &recording,
                                video_size,
                                video_turns,
                            );
                        },
                    )),
                });
            }
        }
        self.paint_stylus_pointer(ui, rect);
        self.paint_recording_indicator(ui, rect);
        self.paint_rate_indicators(ui, rect);
        self.ui_system_media(ui.ctx(), rect);
    }

    /// Host overlay: use the actual displayed image bounds, including rotation
    /// and internal GLES resolution. Captures above keep the original image.
    fn paint_rate_indicators(&mut self, ui: &egui::Ui, rect: Rect) {
        let config = self.library.config();
        if config.show_ips {
            let Some(frame) = &self.last_frame_snapshot else {
                return;
            };
            let now = Instant::now();
            let interval = Duration::from_secs(1);
            if self
                .rate_badge
                .as_ref()
                .is_none_or(|(sampled_at, _)| now.duration_since(*sampled_at) >= interval)
            {
                let ips = frame.presentation_stats.ips().unwrap_or(0.0);
                let fps = config.show_fps.then(|| self.frame_stats.current_fps());
                let text = match fps {
                    Some(fps) => format!("{fps:.0} FPS / {ips:.0} IPS"),
                    None => format!("{ips:.0} IPS"),
                };
                self.rate_badge = Some((now, text));
            }
            if let Some((sampled_at, text)) = &self.rate_badge {
                Self::paint_rate_badge(ui, rect, text.clone());
                ui.ctx()
                    .request_repaint_after(interval - now.duration_since(*sampled_at));
            }
        } else if config.show_fps {
            Self::paint_rate_badge(
                ui,
                rect,
                format!("{:.1} FPS", self.frame_stats.current_fps()),
            );
        }
    }

    fn paint_rate_badge(ui: &egui::Ui, rect: Rect, text: String) {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let galley = painter.layout_no_wrap(
            text,
            egui::FontId::proportional(13.0),
            Color32::from_white_alpha(220),
        );
        let size = galley.size() + Vec2::new(10.0, 6.0);
        let origin = rect.right_top() + Vec2::new(-size.x - 6.0, 6.0);
        let badge = Rect::from_min_size(origin, size);
        painter.rect_filled(badge, 3.0, Color32::from_black_alpha(115));
        painter.galley(badge.min + Vec2::new(5.0, 3.0), galley, Color32::WHITE);
    }

    // Painted after the framebuffer capture callbacks: visible locally only.
    fn paint_recording_indicator(&self, ui: &egui::Ui, rect: Rect) {
        if !self
            .recording
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active()
        {
            return;
        }
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let origin = rect.left_top() + Vec2::new(14.0, 14.0);
        if ui.ctx().input(|i| i.time).rem_euclid(1.0) < 0.5 {
            painter.circle_filled(origin, 5.0, Color32::from_rgb(255, 45, 45));
        }
        let text = origin + Vec2::new(12.0, 0.0);
        let font = egui::FontId::proportional(15.0);
        painter.text(
            text + Vec2::new(1.0, 1.0),
            egui::Align2::LEFT_CENTER,
            "REC",
            font.clone(),
            Color32::BLACK,
        );
        painter.text(
            text,
            egui::Align2::LEFT_CENTER,
            "REC",
            font,
            Color32::from_rgb(255, 70, 70),
        );
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }

    fn ui_fullscreen_game(&mut self, ui: &mut egui::Ui) {
        let Some(frame) = self.last_frame_snapshot.as_ref() else {
            return;
        };
        let guest_size = Vec2::new(frame.width as f32, frame.height as f32);
        let native = if !self.running_is_gizmondo && self.game_rotation.is_quarter_turn() {
            [frame.texture_height, frame.texture_width]
        } else {
            [frame.texture_width, frame.texture_height]
        };
        let available = ui.available_rect_before_wrap();
        self.stylus_touch = [0.0; 4];
        let dpi = ui.ctx().pixels_per_point();
        let physical = [
            (available.width() * dpi).floor() as u32,
            (available.height() * dpi).floor() as u32,
        ];
        let fit = crate::fullscreen_layout::qvga_fullscreen_fit(physical, native);
        let [left, top, width, height] = if fit.width > physical[0] || fit.height > physical[1] {
            crate::fullscreen_layout::aspect_fit(physical, native)
        } else {
            [fit.left, fit.top, fit.width, fit.height]
        };
        let rect = Rect::from_min_size(
            available.min + Vec2::new(left as f32, top as f32) / dpi,
            Vec2::new(width as f32, height as f32) / dpi,
        );
        self.paint_guest_frame(ui, rect);
        self.handle_pointer(ui.ctx(), &rect, guest_size);
    }

    fn ui_run(&mut self, ui: &mut egui::Ui) {
        if self.running_is_gizmondo {
            self.ui_gizmondo(ui);
            return;
        }
        self.ui_pocketpc(ui);
    }

    /// Host presentation only: fixed controls, native pixels and the existing inverse input transform.
    fn ui_pocketpc(&mut self, ui: &mut egui::Ui) {
        let bounds = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(bounds, 0.0, Color32::from_rgb(8, 13, 21));
        let gap = crate::fullscreen_layout::POCKET_PC_GAP;
        let side = (bounds.width() * 0.19).clamp(72.0, 140.0);
        let pad = side.min((bounds.height() - 60.0).max(30.0) * 0.45);
        let pad_top = bounds.bottom() - pad - gap;
        let top = bounds.top() + crate::fullscreen_layout::POCKET_PC_TOOLBAR_HEIGHT;
        let cell_w = (side - gap) / 2.0;
        let cell_h = ((pad_top - 18.0 - top - gap) / 2.0).max(20.0);
        let banks = [
            (
                bounds.left() + gap,
                [
                    ("A", GuestButton::ButtonA),
                    ("P2", GuestButton::PocketPcApp2),
                    ("Action", GuestButton::Action),
                    ("P1", GuestButton::PocketPcApp1),
                ],
            ),
            (
                bounds.right() - side - gap,
                [
                    ("P4", GuestButton::PocketPcApp4),
                    ("C", GuestButton::ButtonC),
                    ("P3", GuestButton::PocketPcApp3),
                    ("B", GuestButton::ButtonB),
                ],
            ),
        ];
        for (x, buttons) in banks {
            for (i, (label, guest)) in buttons.into_iter().enumerate() {
                let (label, guest) = if self.running_swap_button_groups {
                    match guest {
                        GuestButton::ButtonA => ("P4", GuestButton::PocketPcApp4),
                        GuestButton::ButtonB => ("P1", GuestButton::PocketPcApp1),
                        GuestButton::ButtonC => ("P2", GuestButton::PocketPcApp2),
                        GuestButton::Action => ("P3", GuestButton::PocketPcApp3),
                        GuestButton::PocketPcApp1 => ("B", GuestButton::ButtonB),
                        GuestButton::PocketPcApp2 => ("C", GuestButton::ButtonC),
                        GuestButton::PocketPcApp3 => ("Action", GuestButton::Action),
                        GuestButton::PocketPcApp4 => ("A", GuestButton::ButtonA),
                        _ => (label, guest),
                    }
                } else {
                    (label, guest)
                };
                let rect = Rect::from_min_size(
                    Pos2::new(
                        x + (i % 2) as f32 * (cell_w + gap),
                        top + (i / 2) as f32 * (cell_h + gap),
                    ),
                    Vec2::new(cell_w, cell_h),
                );
                ui.allocate_ui_at_rect(rect, |ui| self.vbutton_size(ui, label, guest, rect.size()));
            }
        }
        // Auxiliary keys are deliberately short; host actions share the orange palette.
        let toolbar_w = 298.0;
        let toolbar = Rect::from_min_size(
            Pos2::new(bounds.center().x - toolbar_w / 2.0, bounds.top() + 4.0),
            Vec2::new(toolbar_w, 26.0),
        );
        ui.allocate_ui_at_rect(toolbar, |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.horizontal(|ui| {
                for (label, guest) in [
                    ("TAB", GuestButton::Soft1),
                    ("ESC", GuestButton::Soft2),
                    ("TURBO", GuestButton::Turbo),
                ] {
                    self.vbutton_size(ui, label, guest, Vec2::new(52.0, 24.0));
                }
                ui.add_space(10.0);
                let host = |label: &str| {
                    egui::Button::new(label)
                        .fill(Color32::from_rgb(169, 89, 45))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(207, 139, 88)))
                        .rounding(8.0)
                };
                if ui.add_sized(Vec2::new(48.0, 24.0), host("EXIT")).clicked() {
                    self.return_to_library(ui.ctx());
                }
                if ui
                    .add_sized(Vec2::new(28.0, 24.0), host("↻"))
                    .on_hover_text("Rotate the game screen clockwise")
                    .clicked()
                {
                    self.cycle_rotation(ui.ctx());
                }
            });
        });
        let dpad_rect =
            Rect::from_min_size(Pos2::new(bounds.left() + gap, pad_top), Vec2::splat(pad));
        let cell = pad / 3.0;
        ui.painter()
            .circle_filled(dpad_rect.center(), pad / 2.0, Color32::from_rgb(16, 24, 36));
        for (label, guest, col, row) in [
            ("▲", GuestButton::DpadUp, 1, 0),
            ("▼", GuestButton::DpadDown, 1, 2),
            ("◀", GuestButton::DpadLeft, 0, 1),
            ("▶", GuestButton::DpadRight, 2, 1),
        ] {
            let rect = Rect::from_min_size(
                dpad_rect.min + Vec2::new(col as f32 * cell, row as f32 * cell),
                Vec2::splat(cell),
            );
            ui.allocate_ui_at_rect(rect, |ui| self.vbutton_size(ui, label, guest, rect.size()));
        }
        ui.painter().circle_filled(
            dpad_rect.center(),
            cell * 0.5,
            Color32::from_rgb(23, 34, 48),
        );
        let stick = Rect::from_min_size(
            Pos2::new(bounds.right() - gap - pad, pad_top),
            Vec2::splat(pad),
        );
        self.ui_stylus_pad_at(ui, stick);
        let room = Rect::from_min_max(
            Pos2::new(bounds.left() + side + 2.0 * gap, top),
            Pos2::new(bounds.right() - side - 2.0 * gap, bounds.bottom() - gap),
        );
        if let Some(frame) = self.last_frame_snapshot.as_ref() {
            let guest = Vec2::new(frame.width as f32, frame.height as f32);
            let native = if self.game_rotation.is_quarter_turn() {
                [frame.texture_height, frame.texture_width]
            } else {
                [frame.texture_width, frame.texture_height]
            };
            let dpi = ui.ctx().pixels_per_point();
            let [left, top, width, height] = crate::fullscreen_layout::presentation_fit_capped(
                [
                    (room.width().max(1.0) * dpi).floor() as u32,
                    (room.height().max(1.0) * dpi).floor() as u32,
                ],
                native,
                4,
            );
            // Physical-pixel origin as well as size: fractional host DPI must not soften x1.
            let origin = Pos2::new(
                (room.left() * dpi).round() / dpi,
                (room.top() * dpi).round() / dpi,
            );
            let rect = Rect::from_min_size(
                origin + Vec2::new(left as f32, top as f32) / dpi,
                Vec2::new(width as f32, height as f32) / dpi,
            );
            ui.allocate_ui_at_rect(room, |ui| {
                ui.set_clip_rect(room.intersect(ui.clip_rect()));
                self.paint_guest_frame(ui, rect);
                self.handle_pointer_clipped(ui.ctx(), &rect, guest, room);
            });
        } else {
            ui.allocate_ui_at_rect(room, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("Starting Pocket PC…");
                });
            });
        }
        ui.allocate_rect(bounds, Sense::hover());
    }

    fn ui_stylus_pad_at(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let response = ui.interact(rect, ui.id().with("ppc_aim"), Sense::drag());
        let mut offset = Vec2::ZERO;
        if pointer_held_in(ui.ctx(), &rect.intersect(ui.clip_rect())) {
            if let Some(pos) = response.interact_pointer_pos() {
                offset = (pos - rect.center()) / (rect.width() * 0.4).max(1.0);
                if offset.length() > 1.0 {
                    offset = offset.normalized();
                }
            }
        }
        self.stylus_touch = [
            (-offset.y).max(0.0),
            offset.y.max(0.0),
            (-offset.x).max(0.0),
            offset.x.max(0.0),
        ];
        ui.painter().circle_filled(
            rect.center(),
            rect.width() * 0.46,
            Color32::from_rgb(16, 24, 36),
        );
        ui.painter().circle_stroke(
            rect.center(),
            rect.width() * 0.46,
            egui::Stroke::new(1.0, Color32::from_rgb(80, 96, 120)),
        );
        ui.painter().circle_filled(
            rect.center() + offset * rect.width() * 0.28,
            rect.width() * 0.16,
            Color32::from_rgb(48, 58, 71),
        );
    }

    /// Render one virtual button. Pressed-while-pointer-is-down
    /// generates `WM_KEYDOWN` once; releasing fires `WM_KEYUP`.
    ///
    /// The held state comes from [`pointer_held_in`] rather than from
    /// the `Response`, because egui stops calling a still-held press a
    /// click after 0.8 s — see that function.
    fn vbutton_size(&mut self, ui: &mut egui::Ui, label: &str, guest: GuestButton, size: Vec2) {
        let vk = rotate_direction_vk(guest.vk(), (4 - rotation_turns(self.game_rotation)) % 4);
        let was_pressed = self.held.is_held_by(InputSource::Pointer, vk);
        let mut button = egui::Button::new(
            RichText::new(label)
                .size(if size.y < 30.0 { 12.0 } else { 16.0 })
                .strong(),
        )
        .min_size(size)
        .fill(Color32::from_rgb(48, 58, 71))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(80, 96, 120)))
        .rounding(10.0);
        if self.held.is_held(vk) {
            button = button.fill(Color32::from_rgb(80, 130, 255));
        }
        let keys = self.library.config().keybindings.keys_for(guest).join(", ");
        let response = ui.add_sized(size, button);
        // Naming the keyboard shortcut on the button is how a user
        // discovers what their own bindings ended up as.
        let response = if keys.is_empty() {
            response.on_hover_text(format!("{} (no key bound)", guest.label()))
        } else {
            response.on_hover_text(format!("{} — {keys}", guest.label()))
        };
        let now_pressed = pointer_held_in(ui.ctx(), &response.rect.intersect(ui.clip_rect()));
        // Only the edges matter, and `HeldButtons` has the last word on
        // whether the guest hears about them: the keyboard may be
        // holding this same button.
        if now_pressed && !was_pressed && self.held.press(InputSource::Pointer, vk) {
            self.send_input(InputEvent::KeyDown { vk });
        } else if was_pressed && !now_pressed && self.held.release(InputSource::Pointer, vk) {
            self.send_input(InputEvent::KeyUp { vk });
        }
    }

    /// Host-only Gizmondo controls; guest key codes and user bindings are unchanged.
    fn ui_gizmondo(&mut self, ui: &mut egui::Ui) {
        let fullscreen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
        let native = self
            .last_frame_snapshot
            .as_ref()
            .map_or([320, 240], |f| [f.texture_width, f.texture_height]);
        let guest = self
            .last_frame_snapshot
            .as_ref()
            .map_or(Vec2::new(320., 240.), |f| {
                Vec2::new(f.width as f32, f.height as f32)
            });
        let bounds = ui
            .allocate_exact_size(ui.available_size(), Sense::hover())
            .0;
        let layout = crate::gizmondo_layout::calculate(
            [bounds.width(), bounds.height()],
            native,
            ui.ctx().pixels_per_point(),
            if fullscreen {
                None
            } else {
                Some(if self.upscale_x2 { 2 } else { 1 })
            },
        );
        let rect = |r: crate::gizmondo_layout::BoxRect| {
            Rect::from_min_size(bounds.min + Vec2::new(r.x, r.y), Vec2::new(r.w, r.h))
        };
        ui.painter()
            .rect_filled(bounds, 0., Color32::from_rgb(8, 13, 21));
        ui.painter().text(
            bounds.min + Vec2::new(12., 12.),
            egui::Align2::LEFT_CENTER,
            "PocketHLE · Gizmondo",
            egui::FontId::proportional(13.),
            Color32::from_gray(220),
        );
        let screen = rect(layout.screen);
        ui.painter().rect_filled(screen, 0., Color32::BLACK);
        if self.last_frame_texture.is_some() {
            self.paint_guest_frame(ui, screen);
            self.handle_pointer(ui.ctx(), &screen, guest);
        } else {
            ui.painter().text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Starting…",
                egui::FontId::proportional(16.),
                Color32::from_gray(180),
            );
        }
        for (i, (caption, button)) in [
            ("HOME", GuestButton::GizPiano1),
            ("VOLUME", GuestButton::GizPiano2),
            ("BRIGHTNESS", GuestButton::GizPiano3),
            ("GEOFENCE", GuestButton::GizPiano4),
            ("POWER", GuestButton::GizPiano5),
        ]
        .into_iter()
        .enumerate()
        {
            self.giz_button(
                ui,
                rect(layout.functions[i]),
                GizIcon::Function(i),
                caption,
                button,
                false,
            );
        }
        if ui
            .put(
                rect(layout.functions[5]),
                egui::Button::new("EXIT")
                    .fill(Color32::from_rgb(169, 89, 45))
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(207, 139, 88)))
                    .rounding(8.0),
            )
            .on_hover_text("Return to library")
            .clicked()
        {
            self.return_to_library(ui.ctx());
        }
        for (i, (label, button)) in [("L", GuestButton::Soft1), ("R", GuestButton::Soft2)]
            .into_iter()
            .enumerate()
        {
            self.giz_button(
                ui,
                rect(layout.shoulders[i]),
                GizIcon::Text(label),
                label,
                button,
                false,
            );
        }
        for (i, (symbol, button)) in [
            ("▲", GuestButton::DpadUp),
            ("◀", GuestButton::DpadLeft),
            ("▶", GuestButton::DpadRight),
            ("▼", GuestButton::DpadDown),
        ]
        .into_iter()
        .enumerate()
        {
            self.giz_button(
                ui,
                rect(layout.directions[i]),
                GizIcon::Text(symbol),
                "D-pad",
                button,
                false,
            );
        }
        let pad = rect(layout.dpad);
        ui.painter().circle_filled(
            pad.center(),
            pad.width() / 5.0,
            Color32::from_rgb(20, 28, 40),
        );
        ui.painter().circle_stroke(
            pad.center(),
            pad.width() / 5.0,
            egui::Stroke::new(1.5, Color32::from_rgb(80, 96, 120)),
        );
        for (i, (caption, button)) in [
            ("STOP", GuestButton::ButtonA),
            ("REW", GuestButton::ButtonC),
            ("FWD", GuestButton::ButtonB),
            ("PLAY", GuestButton::Action),
        ]
        .into_iter()
        .enumerate()
        {
            self.giz_button(
                ui,
                rect(layout.actions[i]),
                GizIcon::Media(i),
                caption,
                button,
                true,
            );
        }
    }

    fn giz_button(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        icon: GizIcon,
        caption: &str,
        button: GuestButton,
        round: bool,
    ) {
        let vk = button.vk();
        let hit = rect.intersect(ui.clip_rect());
        let mut pressed = pointer_held_in(ui.ctx(), &hit);
        if round {
            let shape = crate::gizmondo_layout::BoxRect {
                x: rect.left(),
                y: rect.top(),
                w: rect.width(),
                h: rect.height(),
            };
            pressed &= ui.ctx().input(|i| {
                i.pointer
                    .press_origin()
                    .is_some_and(|p| shape.contains([p.x, p.y], true))
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| shape.contains([p.x, p.y], true))
            });
        }
        let was_pressed = self.held.is_held_by(InputSource::Pointer, vk);
        if pressed && !was_pressed && self.held.press(InputSource::Pointer, vk) {
            self.send_input(InputEvent::KeyDown { vk });
        } else if was_pressed && !pressed && self.held.release(InputSource::Pointer, vk) {
            self.send_input(InputEvent::KeyUp { vk });
        }
        let held = self.held.is_held(vk);
        let fill = if held {
            Color32::from_rgb(50, 136, 191)
        } else {
            Color32::from_rgb(48, 58, 71)
        };
        let outline = egui::Stroke::new(
            1.5,
            if held {
                Color32::from_rgb(145, 214, 255)
            } else {
                Color32::from_rgb(80, 96, 120)
            },
        );
        if round {
            ui.painter()
                .circle(rect.center(), rect.width() / 2., fill, outline);
        } else {
            ui.painter().rect(rect, 10., fill, outline);
        }
        match icon {
            GizIcon::Text(symbol) => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    symbol,
                    egui::FontId::proportional((rect.height() * 0.36).clamp(14., 34.)),
                    Color32::WHITE,
                );
            }
            _ => {
                let mut label_size = (rect.width() * 0.16)
                    .clamp(8., 12.)
                    .min(((rect.width() - 8.) / (caption.len().max(1) as f32 * 0.65)).max(6.));
                if matches!(icon, GizIcon::Function(_)) {
                    label_size = (label_size * 2.0 / 3.0).max(6.0);
                }
                let center = rect.center() - Vec2::new(0., label_size * 0.60);
                paint_giz_icon(
                    ui.painter(),
                    center,
                    rect.width().min(rect.height()) * 0.22,
                    icon,
                );
                ui.painter().text(
                    Pos2::new(rect.center().x, rect.bottom() - 5.),
                    egui::Align2::CENTER_BOTTOM,
                    caption,
                    egui::FontId::proportional(label_size),
                    Color32::WHITE,
                );
            }
        }
        let keys = self
            .library
            .config()
            .keybindings
            .keys_for(button)
            .join(", ");
        ui.interact(
            rect,
            ui.make_persistent_id(("giz_button", vk)),
            Sense::click_and_drag(),
        )
        .on_hover_text(if keys.is_empty() {
            format!("{caption} — keyboard: unbound")
        } else {
            format!("{caption} — keyboard: {keys}")
        });
    }

    fn handle_pointer(&mut self, ctx: &egui::Context, rect: &Rect, size: Vec2) {
        self.handle_pointer_clipped(ctx, rect, size, *rect);
    }

    fn handle_pointer_clipped(&mut self, ctx: &egui::Context, rect: &Rect, size: Vec2, clip: Rect) {
        let events = ctx.input(|input| input.events.clone());
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(true));
        let rotation = self.game_rotation;
        let strokes = crate::desktop_pointer::gestures(
            &events,
            &mut self.pointer_down_at,
            rect.intersect(clip),
            focused,
            |pos| rotated_pointer_to_game(pos - rect.min, rect.size(), size, rotation),
        );
        for stroke in strokes {
            let event = match stroke {
                crate::desktop_pointer::Stroke::Down(x, y) => {
                    // The guest cancels its emulated stroke on physical Down.
                    // Rearm a held stick after even a same-frame touch tap.
                    self.cancel_grid();
                    self.stylus_pointer.cancel_click();
                    self.stylus_last = (0, 0);
                    InputEvent::PointerDown { x, y }
                }
                crate::desktop_pointer::Stroke::Move(x, y) => InputEvent::PointerMove { x, y },
                crate::desktop_pointer::Stroke::Up(x, y) => InputEvent::PointerUp { x, y },
            };
            self.send_input(event);
        }
    }

    fn persist_rotation(&mut self, rotation: RotationPref) {
        self.cancel_grid();
        if !self.running_is_gizmondo {
            self.send_input(InputEvent::KeyboardRotation {
                turns: rotation_turns(rotation),
            });
        }

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

    fn stylus_button(&mut self, down: bool) {
        if self.screen != Screen::Run
            || self.running_is_gizmondo
            || self
                .last_frame_snapshot
                .as_ref()
                .is_some_and(|f| f.text_input)
        {
            return;
        }
        if self
            .stylus_pointer
            .button(down, self.stylus_clock.elapsed().as_millis() as u64)
        {
            self.send_input(InputEvent::StylusAim { x: 0, y: 0 });
            self.stylus_last = (0, 0);
            let mode = if self.stylus_pointer.pointer {
                "Pointer"
            } else {
                "Stick"
            };
            self.control_notice = Some((
                format!("Stylus mode: {mode}"),
                Instant::now() + Duration::from_secs(3),
            ));
        }
    }

    fn update_stylus_input(&mut self, ctx: &egui::Context) {
        let mut strengths = self.stylus_touch;
        let config = &self.library.config().stylus_stick;
        for i in 0..4 {
            if config.keyboard[i].iter().any(|name| {
                self.stylus_keyboard
                    .iter()
                    .any(|k| pocket_library::keybindings::keys_match(k, name))
            }) {
                strengths[i] = 1.0;
            }
            for ((_, name), value) in &self.stylus_gamepad {
                if pocket_library::stylus::controller_direction(name) == Some(i) {
                    strengths[i] = strengths[i].max(*value);
                }
            }
        }

        let active = self.screen == Screen::Run
            && !self.running_is_gizmondo
            && self.running_game.is_some()
            && ctx.input(|i| i.viewport().focused.unwrap_or(true))
            && !self
                .last_frame_snapshot
                .as_ref()
                .is_some_and(|f| f.text_input);
        if !active {
            self.stylus_keyboard.clear();
            self.stylus_gamepad.clear();
            self.stylus_touch = [0.0; 4];
        }
        // Touch temporarily owns the stylus, but does not release held
        // controller axes. SDL may not send another event for an unchanged
        // stick position when the finger lifts.
        let vector = if active
            && self.pointer_down_at.is_none()
            && !self.stylus_grid_router.active()
            && !self.center_tap.borrow().active()
        {
            self.last_frame_snapshot.as_ref().map_or((0, 0), |frame| {
                config.velocity_for_size(
                    strengths,
                    rotation_turns(self.game_rotation),
                    frame.width,
                    frame.height,
                )
            })
        } else {
            (0, 0)
        };
        let now = Instant::now();
        let dt = now.duration_since(self.stylus_tick).as_secs_f32();
        self.stylus_tick = now;
        if let Some(frame) = self.last_frame_snapshot.as_ref() {
            self.stylus_pointer
                .advance(vector, dt, (frame.width, frame.height));
        }
        if !active {
            self.stylus_pointer.cancel_click();
        }
        if let Some((x, y)) = self
            .stylus_pointer
            .take_tap(self.stylus_clock.elapsed().as_millis() as u64)
        {
            if active
                && self.pointer_down_at.is_none()
                && !self.stylus_grid_router.active()
                && !self.center_tap.borrow().active()
            {
                if self
                    .center_tap
                    .borrow_mut()
                    .press_at(self.input_gesture_clock.elapsed().as_millis() as u64, x, y)
                    .is_some()
                {
                    self.dispatch_input(InputEvent::StylusAim { x: 0, y: 0 });
                    self.dispatch_input(InputEvent::PointerDown { x, y });
                }
            }
        }
        let animate = self.stylus_pointer.needs_tick(vector);
        let vector = if self.stylus_pointer.pointer {
            (0, 0)
        } else {
            vector
        };
        if active && animate {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        self.recording
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .aim(vector.0 as i32, vector.1 as i32);
        if vector != self.stylus_last {
            self.send_input(InputEvent::StylusAim {
                x: vector.0,
                y: vector.1,
            });
            self.stylus_last = vector;
        }
    }

    fn media_command(&self, command: SystemMediaCommand) {
        if let Some(tx) = &self.input_tx {
            let _ = tx.send(InputCommand::SystemMedia(command));
        }
    }

    fn media_active(&self) -> bool {
        self.system_media.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    fn ui_system_media(&self, ctx: &egui::Context, display: Rect) {
        let media = self.system_media.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(media) = media else { return };
        let has_progress = media.duration.is_finite() && media.duration > 0.0;
        let height = if has_progress { 34.0 } else { 25.0 };
        ctx.request_repaint_after(Duration::from_secs(1));
        egui::Area::new(egui::Id::new("system_media_controls"))
            .order(egui::Order::Foreground)
            .fixed_pos(display.left_bottom() - Vec2::new(0.0, height))
            .constrain(false)
            .show(ctx, |ui| {
                ui.set_width(display.width());
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let panel = Rect::from_min_size(ui.cursor().min, Vec2::new(display.width(), height));
                ui.painter().rect_filled(panel, 0.0, Color32::from_black_alpha(225));
                if has_progress {
                    ui.add(egui::ProgressBar::new((media.position() / media.duration) as f32)
                        .desired_width(display.width()).desired_height(9.0));
                }
                ui.horizontal(|ui| {
                    for (label, command, enabled) in [
                        ("−10 s", SystemMediaCommand::Rewind, has_progress),
                        (if media.paused { "Play" } else { "Pause" }, SystemMediaCommand::TogglePause, true),
                        ("Stop", SystemMediaCommand::Stop, true),
                        ("+10 s", SystemMediaCommand::Forward, has_progress),
                    ] {
                        if ui.add_enabled(enabled, egui::Button::new(label)
                            .min_size(Vec2::new(display.width() / 4.0, 25.0))).clicked() {
                            self.media_command(command);
                        }
                    }
                });
            });
    }

    fn cancel_center_tap(&self) {
        let point = self.center_tap.borrow_mut().cancel();
        if let Some((x, y)) = point {
            self.dispatch_input(InputEvent::PointerUp { x, y });
        }
    }

    fn send_input(&self, ev: InputEvent) {
        match ev {
            InputEvent::KeyDown {
                vk: pocket_library::keybindings::STYLUS_CLICK_COMMAND,
            } => {
                if !self.running_is_gizmondo && self.screen == Screen::Run {
                    self.pending_binding_commands
                        .borrow_mut()
                        .push(pocket_library::keybindings::STYLUS_CLICK_COMMAND);
                }
                return;
            }
            InputEvent::KeyUp {
                vk: pocket_library::keybindings::STYLUS_CLICK_COMMAND,
            } => {
                if !self.running_is_gizmondo && self.screen == Screen::Run {
                    self.pending_binding_commands
                        .borrow_mut()
                        .push(pocket_library::keybindings::STYLUS_RELEASE_COMMAND);
                }
                return;
            }
            InputEvent::KeyDown {
                vk:
                    vk @ (pocket_library::keybindings::ROTATION_COMMAND
                    | pocket_library::keybindings::SCREENSHOT_COMMAND),
            } => {
                if self.screen == Screen::Run {
                    self.pending_binding_commands.borrow_mut().push(vk);
                }
                return;
            }
            InputEvent::KeyUp {
                vk:
                    pocket_library::keybindings::ROTATION_COMMAND
                    | pocket_library::keybindings::SCREENSHOT_COMMAND,
            } => return,
            InputEvent::KeyDown {
                vk: pocket_library::center_tap::COMMAND,
            } => {
                if self.running_is_gizmondo
                    || self.screen != Screen::Run
                    || self.pointer_down_at.is_some()
                    || self.stylus_grid_router.active()
                {
                    return;
                }
                if let Some(frame) = self.last_frame_snapshot.as_ref() {
                    let point = self.center_tap.borrow_mut().press(
                        self.input_gesture_clock.elapsed().as_millis() as u64,
                        frame.width,
                        frame.height,
                    );
                    if let Some((x, y)) = point {
                        self.dispatch_input(InputEvent::StylusAim { x: 0, y: 0 });
                        self.dispatch_input(InputEvent::PointerDown { x, y });
                    }
                }
                return;
            }
            InputEvent::KeyUp {
                vk: pocket_library::center_tap::COMMAND,
            } => return,
            InputEvent::PointerDown { .. }
            | InputEvent::PointerMove { .. }
            | InputEvent::PointerUp { .. } => self.cancel_center_tap(),
            _ => {}
        }
        self.dispatch_input(ev);
    }

    fn dispatch_input(&self, ev: InputEvent) {
        {
            let mut recording = self.recording.lock().unwrap_or_else(|e| e.into_inner());
            recording.set_gizmondo(self.running_is_gizmondo);
            recording.set_touch_steering_enabled(self.touch_steering_enabled());
            recording.observe(ev);
        }
        if let Some(tx) = self.input_tx.as_ref() {
            let _ = tx.send(InputCommand::Input(ev));
        }
    }

    fn send_gesture_edges(&mut self, edges: Vec<(u16, bool)>) {
        for (vk, down) in edges {
            let vk = swapped_pocketpc_physical_vk(vk, self.running_swap_button_groups);
            let vk = if self.running_is_gizmondo {
                vk
            } else {
                rotate_direction_vk(vk, (4 - rotation_turns(self.game_rotation)) % 4)
            };
            if down {
                if self.held.press(InputSource::Gesture, vk) {
                    self.send_input(InputEvent::KeyDown { vk });
                }
            } else if self.held.release(InputSource::Gesture, vk) {
                self.send_input(InputEvent::KeyUp { vk });
            }
        }
    }
    fn route_long_press(
        &mut self,
        controller: bool,
        device: usize,
        input: &str,
        down: bool,
    ) -> bool {
        if self.screen != Screen::Run {
            return false;
        }
        let config = self.library.config();
        let reserved = if controller {
            matches!(input, "Select" | "Start")
                || config.gamepad_toggles_fullscreen(input)
        } else {
            pocket_library::keybindings::reserved_host_key(input)
        };
        if reserved {
            return false;
        }
        let key = pocket_library::input_gestures::binding_key(
            self.running_is_gizmondo,
            controller,
            input,
        );
        let long = config.long_press_bindings.get(&key).copied();
        let custom = config.short_press_bindings.get(&key);
        if long.is_none() && custom.is_none() {
            return false;
        }
        let short = if let Some(custom) = custom {
            custom.map(|b| b.vk())
        } else if controller {
            pocket_library::keybindings::steam_deck_guest_button(input, self.running_is_gizmondo)
                .or_else(|| config.gamepad_bindings.get(input).copied())
                .map(|b| b.vk())
        } else {
            pocket_library::keybindings::keyboard_button_for_profile(&config.keybindings, input, self.running_is_gizmondo).map(|b| b.vk())
        };
        let token = format!(
            "{}:{}:{}",
            if controller { "pad" } else { "key" },
            device,
            input
        );
        let edges = if let Some(long) = long {
            self.input_gestures.edge(
                token,
                down,
                self.input_gesture_clock.elapsed().as_millis() as u64,
                short,
                long.vk(),
            )
        } else {
            self.input_gestures.immediate_edge(token, down, short)
        };
        self.send_gesture_edges(edges);
        true
    }
    fn release_all_keys(&mut self) {
        self.cancel_center_tap();
        self.input_gestures.clear();
        self.cancel_grid();
        self.grid_modifiers = [false; 4];
        if self.steering_axes != (0, 0) {
            self.send_input(InputEvent::TouchSteeringPosition { x: 0, y: 0 });
            self.steering_axes = (0, 0);
        }
        self.stylus_pointer.cancel_click();
        self.pending_binding_commands.borrow_mut().retain(|v|!matches!(*v,pocket_library::keybindings::STYLUS_CLICK_COMMAND|pocket_library::keybindings::STYLUS_RELEASE_COMMAND));
        self.stylus_tick=Instant::now();
        self.stylus_keyboard.clear();
        self.stylus_gamepad.clear();
        self.stylus_touch = [0.0; 4];
        if self.stylus_last != (0, 0) {
            self.send_input(InputEvent::StylusAim { x: 0, y: 0 });
        }
        self.stylus_last = (0, 0);
        self.deck_view_press = None;
        self.deck_menu_press = None;
        self.gamepad_held.clear();
        for vk in self.held.drain_all() {
            self.send_input(InputEvent::KeyUp { vk });
        }
    }

    fn update_gamepad_input(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if self.screen!=Screen::InputSettings || !focused {self.bindings_visual.clear_live();}
        if !focused { self.menu_host_press.clear(); }
        if self.screen != Screen::Run
            || !focused
            || self.file_dialog.is_some()
            || self.restore_archive.is_some()
        {
            // UI frames release guest inputs, but a focused View press must
            // survive until release so the UI uses the same tap/hold gesture.
            let view_press = if focused { self.deck_view_press } else { None };
            self.release_all_keys();
            self.deck_view_press = view_press;
        }
        self.gamepad_rx.set_output(
            self.gamepad_selected,
            self.screen == Screen::Run
                && self.file_dialog.is_none()
                && self.restore_archive.is_none()
                && self.running_game.is_some()
                && ctx.input(|i| i.viewport().focused.unwrap_or(true)),
        );
        self.handle_gamepads(ctx);
    }

    fn touch_steering_enabled(&self) -> bool {
        self.screen == Screen::Run
            && !self.running_is_gizmondo
            && self.running_controls_mode == ControlsMode::Zones
    }

    /// View/Menu host gestures must run before modal capture or UI navigation.
    fn handle_menu_host_control(
        &mut self,
        ctx: &egui::Context,
        device: usize,
        control: &str,
        down: bool,
    ) {
        if self.screen == Screen::InputSettings {
            self.bindings_visual.live_input(device, control, down);
        }
        let key = (device, control.to_owned());
        if down {
            self.menu_host_press.entry(key).or_insert_with(Instant::now);
        } else if let Some(at) = self.menu_host_press.remove(&key) {
            if control == "Select" {
                if at.elapsed() < Duration::from_millis(650) {
                    let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    self.set_fullscreen(ctx, !full);
                } else {
                    self.remove_game = None;
                    self.add_data = None;
                    self.data_game = None;
                    self.deck_library_actions.clear();
                    self.config_draft = None;
                    self.bindings_visual = Default::default();
                    self.screen = Screen::Library;
                }
            } else if at.elapsed() < Duration::from_millis(650) {
                self.cycle_deck_upscale_filter(ctx, true);
            } else if self.screen != Screen::InputSettings {
                self.toggle_recording();
            }
        }
        ctx.request_repaint();
    }

    fn handle_gamepads(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.gamepad_rx.rx.try_recv() {
            match event {
                crate::gamepad::Event::SteeringAxis {
                    device,
                    horizontal,
                    value,
                } => {
                    if self.file_dialog.is_some() || self.restore_archive.is_some() || self.add_data.is_some() {
                        continue;
                    }
                    if Some(device) == self.gamepad_selected
                        && ctx.input(|i| i.viewport().focused.unwrap_or(true))
                    {
                        if self.capture_grid_axis(ctx, false, horizontal, value) {
                            continue;
                        }
                        if let Some(editor) = self.stylus_editor.as_mut() {
                            editor.axis(horizontal, value);
                            continue;
                        }
                    }
                    if Some(device) == self.gamepad_selected
                        && ctx.input(|i| i.viewport().focused.unwrap_or(true))
                    {
                        self.route_grid_axis(false, horizontal, value);
                    }
                    if self.touch_steering_enabled()
                        && Some(device) == self.gamepad_selected
                        && ctx.input(|i| i.viewport().focused.unwrap_or(true))
                    {
                        let value = (value.clamp(-1.0, 1.0) * 1000.0) as i16;
                        if horizontal {
                            self.steering_axes.0 = value;
                        } else {
                            self.steering_axes.1 = value;
                        }
                        self.send_input(InputEvent::TouchSteeringPosition {
                            x: self.steering_axes.0,
                            y: self.steering_axes.1,
                        });
                    }
                }
                crate::gamepad::Event::AimAxis {
                    device,
                    horizontal,
                    value,
                } => {
                    if self.file_dialog.is_some() || self.restore_archive.is_some() || self.add_data.is_some() {
                        continue;
                    }
                    if Some(device) == self.gamepad_selected
                        && ctx.input(|i| i.viewport().focused.unwrap_or(true))
                    {
                        if self.capture_grid_axis(ctx, true, horizontal, value) { continue; }
                        if self.running_is_gizmondo { self.route_grid_axis(true, horizontal, value); }
                    }
                    if self.screen == Screen::Run
                        && !self.running_is_gizmondo
                        && Some(device) == self.gamepad_selected
                    {
                        let (negative, positive) = if horizontal {
                            ("RightStickLeft", "RightStickRight")
                        } else {
                            ("RightStickUp", "RightStickDown")
                        };
                        for (name, strength) in [(negative, -value), (positive, value)] {
                            let key = (device, name.to_string());
                            let strength = ((strength - 0.18) / 0.82).clamp(0.0, 1.0);
                            if strength > 0.0
                            {
                                self.stylus_gamepad.insert(key, strength);
                            } else {
                                self.stylus_gamepad.remove(&key);
                            }
                        }
                    }
                }
                crate::gamepad::Event::Error(error) => {self.gamepad_error=Some(error);self.bindings_visual.clear_live();},
                crate::gamepad::Event::Devices(devices) => {
                    if !devices.iter().any(|(id,_)|Some(*id)==self.gamepad_selected) { self.stylus_pointer.cancel_click(); }
                    if self
                        .gamepad_selected
                        .is_some_and(|id| !devices.iter().any(|(d, _)| *d == id))
                    {
                        self.cancel_grid();
                    }
                    if self
                        .gamepad_selected
                        .is_some_and(|id| !devices.iter().any(|(device, _)| *device == id))
                    {
                        self.send_input(InputEvent::TouchSteeringPosition { x: 0, y: 0 });
                        self.steering_axes = (0, 0);
                    }
                    self.deck_nav_repeat = None;
                    self.deck_menu_press = None;
                    self.deck_view_press = None;
                    let disconnected:Vec<_>=self.gamepad_devices.iter().filter(|(id,_)|!devices.iter().any(|(new,_)|new==id)).map(|(id,_)|*id).collect();
                    for id in disconnected {let edges=self.input_gestures.cancel_prefix(&format!("pad:{id}:"));self.send_gesture_edges(edges);}
                    self.menu_host_press.retain(|(id, _), _| devices.iter().any(|(device, _)| device == id));
                    self.gamepad_devices = devices;
                    self.bindings_visual.retain_live_devices(&self.gamepad_devices);
                    self.stylus_gamepad.retain(|(id, _), _| {
                        self.gamepad_devices.iter().any(|(device, _)| device == id)
                    });
                    if !self
                        .gamepad_devices
                        .iter()
                        .any(|(id, _)| Some(*id) == self.gamepad_selected)
                    {
                        self.gamepad_selected = self.gamepad_devices.first().map(|(id, _)| *id);
                    }
                    let gone: Vec<_> = self
                        .gamepad_held
                        .keys()
                        .filter(|(id, _)| {
                            !self
                                .gamepad_devices
                                .iter()
                                .any(|(connected, _)| connected == id)
                        })
                        .cloned()
                        .collect();
                    for key in gone {
                        self.release_gamepad_control(&key);
                    }
                }
                crate::gamepad::Event::Input {
                    device,
                    control,
                    down,
                } => {
                    if down && Some(device) == self.gamepad_selected {
                        crate::controller_labels::activate(ctx, device);
                    }
                    // Host gestures precede binding capture and generic menu navigation.
                    if self.screen != Screen::Run
                        && Some(device) == self.gamepad_selected
                        && ctx.input(|i| i.viewport().focused.unwrap_or(true))
                        && matches!(control.as_str(), "Select" | "Start")
                    {
                        if let Some(editor)=self.stylus_editor.as_mut().filter(|e|e.waiting.is_some()) {
                            if down { editor.reserved_input_attempt(); }
                            continue;
                        }
                        if down && self.screen==Screen::InputSettings && self.bindings_visual.reserved_input_attempt() {
                            self.control_notice=Some(("This input is reserved by PocketHLE.".into(),Instant::now()+Duration::from_secs(3)));
                        }
                        self.handle_menu_host_control(ctx, device, &control, down);
                        continue;
                    }
                    if self.screen==Screen::InputSettings {
                        let focused=ctx.input(|i|i.viewport().focused.unwrap_or(true));
                        if Some(device)==self.gamepad_selected && focused {
                            self.bindings_visual.live_input(device,&control,down);
                            if self.input_controller {
                                self.bindings_visual.controller_shortcut(device);
                            }
                            ctx.request_repaint();
                            if !down {self.controller_ui.release(&control);continue;}
                            continue;
                        }
                    }
                    if self.screen == Screen::Run && !self.running_is_gizmondo && Some(device) == self.gamepad_selected
                        && pocket_library::stylus::controller_reserved(&control, false) {
                        if control == "RightThumb" && ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
                            self.stylus_button(down);
                        }
                        continue;
                    }
                    if self.remove_game.is_some() && Some(device)==self.gamepad_selected
                        && ctx.input(|i|i.viewport().focused.unwrap_or(true)) {
                        if down && matches!(control.as_str(),"South"|"East") {self.deck_library_actions.push(control.clone());ctx.request_repaint();}
                        continue;
                    }
                    if self.screen==Screen::Library && !self.library_busy && !self.pocketpc_import_options.is_empty() && self.rename_draft.is_none()
                        && self.file_dialog.is_none() && Some(device)==self.gamepad_selected && ctx.input(|i|i.viewport().focused.unwrap_or(true)) {
                        if down && matches!(control.as_str(),"South"|"East") {self.deck_library_actions.push(control.clone());ctx.request_repaint();}
                        continue;
                    }
                    if Some(device)==self.gamepad_selected && ctx.input(|i|i.viewport().focused.unwrap_or(true)) && self.route_long_press(true,device,&control,down) {continue;}
                    if (self.file_dialog.is_some() || self.restore_archive.is_some() || self.add_data.is_some())
                        && Some(device) == self.gamepad_selected
                        && ctx.input(|i| i.viewport().focused.unwrap_or(true))
                    {
                        if !down {
                            self.controller_ui.release(&control);
                        } else if self.controller_ui.keyboard_open() {
                            self.controller_ui.press(&control);
                        } else if control == "East" && self.controller_ui.back(ctx) {
                            // Close a popup before closing its owner.
                        } else if let Some(dialog) = self.file_dialog.as_mut() {
                            if !dialog.controller(&control) {
                                self.controller_ui.press(&control);
                            }
                        } else {
                            self.controller_ui.press(if control == "East" {
                                "Escape"
                            } else {
                                &control
                            });
                        }
                        // Controller events arrive after raw_input_hook. Schedule
                        // the next frame to consume the queued widget input.
                        ctx.request_repaint();
                        continue;
                    }
                    if self.touch_steering_enabled()
                        && control.starts_with("LeftStick")
                        && Some(device) == self.gamepad_selected
                    {
                        continue;
                    }
                    if Some(device) == self.gamepad_selected
                        && ctx.input(|i| i.viewport().focused.unwrap_or(true))
                    {
                        if (control.starts_with("LeftStick") || control.starts_with("RightStick"))
                            && (self
                                .stylus_editor
                                .as_ref()
                                .is_some_and(|e| e.owns_stick_input(&control))
                                || self.grid_assigned(&HostInput::Controller(control.clone())))
                        {
                            continue;
                        }
                        if self.stylus_editor_pad(ctx, &control, down) {
                            continue;
                        }
                        if ctx.input(|i| i.viewport().focused.unwrap_or(true))
                            && self.route_grid(HostInput::Controller(control.clone()), down)
                        {
                            continue;
                        }
                    }
                    let key = (device, control.clone());
                    if !down {
                        self.controller_ui.release(&control);
                        if control == "Select"
                            && self
                                .deck_view_press
                                .as_ref()
                                .is_some_and(|(id, _)| *id == device)
                        {
                            if let Some((_, pressed_at)) = self.deck_view_press.take() {
                                if ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
                                    let elapsed = pressed_at.elapsed();
                                    if self.screen == Screen::Run
                                        && crate::deck_navigation::menu_action(elapsed, true)
                                            == Some(
                                                crate::deck_navigation::MenuAction::ReturnToLibrary,
                                            )
                                    {
                                        // Both device profiles: View/FN hold returns to the catalogue.
                                        self.return_to_library(ctx);
                                    } else {
                                        // A short View tap toggles fullscreen.
                                        self.handle_deck_view(ctx, device, elapsed, true);
                                    }
                                }
                            }
                        }
                        if control == "Start"
                            && self
                                .deck_menu_press
                                .as_ref()
                                .is_some_and(|(id, _)| *id == device)
                        {
                            if let Some((_, pressed_at)) = self.deck_menu_press.take() {
                                if self.screen == Screen::Run
                                    && ctx.input(|i| i.viewport().focused.unwrap_or(true))
                                {
                                    match crate::deck_navigation::right_fn_action(
                                        pressed_at.elapsed(),
                                        true,
                                    ) {
                                        Some(crate::deck_navigation::RightFnAction::NextFilter) => {
                                            self.cycle_deck_upscale_filter(ctx, true)
                                        }
                                        Some(
                                            crate::deck_navigation::RightFnAction::ToggleRecording,
                                        ) => self.toggle_recording(),
                                        None => {}
                                    }
                                }
                            }
                        }
                        if Some(device) == self.gamepad_selected
                            && self
                                .deck_nav_repeat
                                .as_ref()
                                .is_some_and(|(held, _)| held == &control)
                        {
                            self.deck_nav_repeat = None;
                        }
                        self.release_gamepad_control(&key);
                        continue;
                    }
                    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
                    let selected = Some(device) == self.gamepad_selected;
                    if !selected || !focused {
                        continue;
                    }
                    if self.release_prompt_ready() {
                        if control == "South" {
                            if let Some(update) = self.release_update.take() {
                                ctx.open_url(egui::OpenUrl::new_tab(&update.url));
                            }
                        } else if control == "East" {
                            self.release_update = None;
                        }
                        self.deck_library_actions.clear();
                        self.deck_nav_repeat = None;
                        continue;
                    }
                    if self.colors_startup_prompt.is_some() {
                        self.deck_library_actions.push(control);
                        continue;
                    }
                    if self.screen == Screen::Library
                        && self.library_tab == crate::deck_navigation::LibraryTab::Recordings
                        && matches!(control.as_str(), "East" | "RightThumb")
                    {
                        self.deck_library_actions.push(control.clone());
                        continue;
                    }
                    if self.screen != Screen::Run {
                        if control == "LeftThumb" {
                            self.deck_library_actions.clear();
                            self.deck_nav_repeat = None;
                            self.controller_ui.press(&control);
                            ctx.request_repaint();
                            continue;
                        }
                        if self.game_id_popup.is_some() {
                            if control == "East" {
                                self.game_id_popup = None;
                            } else {
                                self.controller_ui.press(&control);
                            }
                            continue;
                        }
                        if self.controller_ui.keyboard_open() {
                            self.controller_ui.press(&control);
                            continue;
                        }
                        let catalogue = self.screen == Screen::Library
                            && self.information_game.is_none()
                            && self.rename_draft.is_none()
                            && self.pending_import.is_none()
                            && self.demo_import.is_none()
                            && self.colors_startup_prompt.is_none()
                            && !crate::controller_ui::toolbar_focused(ctx)
                            && (!crate::controller_ui::menu_open(ctx) || self.game_menu.is_some());
                        if self.game_menu.is_none()
                            && control == "East"
                            && self.controller_ui.back(ctx)
                        {
                            continue;
                        }
                        if !catalogue && control != "East" {
                            self.controller_ui.press(&control);
                            continue;
                        }
                    }
                    let deck_reserved = (self.screen == Screen::Library
                        && crate::deck_navigation::tab_direction(&control).is_some())
                        || matches!(control.as_str(), "Select" | "LeftTrigger")
                        || (self.screen == Screen::Run
                            && (matches!(control.as_str(), "Start" | "Mode")
                                || pocket_library::keybindings::steam_deck_guest_button(
                                    &control,
                                    self.running_is_gizmondo,
                                )
                                .is_some()));
                    if self.screen == Screen::Run
                        && !deck_reserved
                        && self.library.config().gamepad_toggles_fullscreen(&control)
                    {
                        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                        self.set_fullscreen(ctx, !fullscreen);
                        continue;
                    }
                    {
                        if self.colors_startup_prompt.is_some() {
                            self.deck_library_actions.push(control);
                            continue;
                        }
                        if self.screen == Screen::Run && control == "Start" {
                            self.deck_menu_press.get_or_insert((device, Instant::now()));
                            continue;
                        }
                        if control == "Select" {
                            self.deck_view_press = Some((device, Instant::now()));
                            continue;
                        }
                        if control == "East" && self.screen != Screen::Run {
                            if let Some(parent) = self.game_menu_parent.take() {
                                self.config_draft = None;
                                self.game_settings_draft = None;
                                self.rename_draft = None;
                                self.information_game = None;
                                self.screen = Screen::Library;
                                self.game_menu = Some(parent);
                                self.game_menu_activate = false;
                                self.deck_library_actions.clear();
                                self.deck_nav_repeat = None;
                                ctx.memory_mut(|m| {
                                    m.open_popup(egui::Id::new("library_game_menu"))
                                });
                                continue;
                            }
                        }
                        if self.screen == Screen::Library && self.game_menu.is_some() {
                            self.deck_library_actions.push(control.clone());
                            if crate::deck_navigation::is_direction(&control) {
                                self.deck_nav_repeat = Some((
                                    control.clone(),
                                    Instant::now() + Duration::from_millis(350),
                                ));
                            }
                            continue;
                        }
                        if self.screen == Screen::Library && self.information_game.is_some() {
                            if control == "East" {
                                self.information_game = None;
                            }
                            continue;
                        }
                        if self.screen == Screen::About && control == "East" {self.close_about();continue;}
                        if self.screen != Screen::Run && control == "East" {
                            self.config_draft = None;
                            self.game_settings_draft = None;
                            self.rename_draft = None;
                            self.information_game = None;
                            self.screen = if self.running_game.is_some() {
                                Screen::Run
                            } else {
                                Screen::Library
                            };
                            self.deck_library_actions.clear();
                            self.deck_nav_repeat = None;
                            continue;
                        }
                        if self.screen == Screen::Library
                            && !self.library_busy
                            && self.rename_draft.is_none()
                            && self.pending_import.is_none()
                            && self.demo_import.is_none()
                        {
                            match control.as_str() {
                                control
                                    if crate::deck_navigation::tab_direction(control).is_some() =>
                                {
                                    self.library_tab = self.library_tab.step(
                                        crate::deck_navigation::tab_direction(control).unwrap(),
                                    );
                                    self.selected_game = None;
                                    self.deck_scroll_selection = true;
                                    self.deck_library_actions.clear();
                                    self.deck_nav_repeat = None;
                                }
                                "South" | "North" => {
                                    self.deck_library_actions.push(control.clone())
                                }
                                direction if crate::deck_navigation::is_direction(direction) => {
                                    self.deck_library_actions.push(control.clone());
                                    self.deck_nav_repeat = Some((
                                        control.clone(),
                                        Instant::now() + Duration::from_millis(350),
                                    ));
                                }
                                _ => {}
                            }
                            continue;
                        }
                    }
                    if self.screen != Screen::Run {
                        continue;
                    }
                    let deck_button = pocket_library::keybindings::steam_deck_guest_button(
                        &control,
                        self.running_is_gizmondo,
                    );
                    let Some(button) = deck_button.or_else(|| {
                        self.library
                            .config()
                            .gamepad_bindings
                            .get(&control)
                            .copied()
                    }) else {
                        continue;
                    };
                    let vk =
                        swapped_pocketpc_physical_vk(button.vk(), self.running_swap_button_groups);
                    let vk = if self.running_is_gizmondo {
                        vk
                    } else {
                        rotate_direction_vk(vk, (4 - rotation_turns(self.game_rotation)) % 4)
                    };
                    self.gamepad_held.insert(key, vk);
                    if self.held.press(InputSource::Gamepad, vk) {
                        self.send_input(InputEvent::KeyDown { vk });
                    }
                }
            }
        }
    }

    fn release_gamepad_control(&mut self, key: &(usize, String)) {
        self.stylus_gamepad.remove(key);
        if let Some(vk) = self.gamepad_held.remove(key) {
            if !self.gamepad_held.values().any(|other| *other == vk)
                && self.held.release(InputSource::Gamepad, vk)
            {
                self.send_input(InputEvent::KeyUp { vk });
            }
        }
    }

    fn handle_deck_view(
        &mut self,
        ctx: &egui::Context,
        _device: usize,
        elapsed: Duration,
        released: bool,
    ) {
        match crate::deck_navigation::view_action(elapsed, released) {
            Some(crate::deck_navigation::ViewAction::ToggleFullscreen) => {
                let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                self.set_fullscreen(ctx, !fullscreen);
            }
            Some(crate::deck_navigation::ViewAction::ReturnToLibrary) => self.return_to_library(ctx),
            None => {}
        }
    }

    fn cycle_deck_upscale_filter(&mut self, ctx: &egui::Context, forward: bool) {
        self.upscale_filter = self.upscale_filter.stepped(forward);
        self.library.config_mut().upscale_filter = self.upscale_filter.id().into();
        if let Some(draft)=self.config_draft.as_mut() {draft.upscale_filter=self.upscale_filter.id().into();}
        self.status = match self.library.save() {
            Ok(()) => format!("Upscaling filter: {}", self.upscale_filter.label()),
            Err(error) => format!("Could not save filter: {error}"),
        };
        self.control_notice = Some((
            format!("Filter: {}", self.upscale_filter.label()),
            Instant::now() + Duration::from_secs(3),
        ));
        if let Some(frame) = self.last_frame_snapshot.clone() {
            self.refresh_frame_texture(ctx, &frame);
        }
    }

    fn cycle_rotation(&mut self, ctx: &egui::Context) {
        if self.last_frame_snapshot.is_none() {
            return;
        }
        self.release_all_keys();
        if let Some((x, y)) = self.pointer_down_at.take() {
            self.send_input(InputEvent::PointerUp { x, y });
        }
        let next = RotationPref::ALL[(usize::from(rotation_turns(self.game_rotation)) + 1) % 4];
        self.game_rotation = next;
        self.persist_rotation(next);
        ctx.request_repaint();
    }

    /// Run reserved host keys before any editor, grid or guest key consumer.
    fn handle_host_keys(&mut self, ctx: &egui::Context) {
        let keys = crate::host_shortcuts::take(ctx);
        let mut fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        for key in keys {
            if let Some(editor) = self.stylus_editor.as_mut().filter(|e| e.waiting.is_some()) {
                editor.reserved_input_attempt();
                continue;
            }
            if self.screen == Screen::InputSettings && self.bindings_visual.reserved_input_attempt()
            {
                self.control_notice = Some((
                    "This input is reserved by PocketHLE.".into(),
                    Instant::now() + Duration::from_secs(3),
                ));
            }
            match key {
                egui::Key::F7 => self.cycle_rotation(ctx),
                egui::Key::F8 => {
                    self.remove_game = None;
                    self.add_data = None;
                    self.data_game = None;
                    self.deck_library_actions.clear();
                    self.bindings_visual.clear_live();
                    self.config_draft = None;
                    self.game_settings_draft = None;
                    self.file_dialog = None;
                    self.stylus_editor = None;
                    self.information_game = None;
                    self.rename_draft = None;
                    self.game_menu = None;
                    if self.running_game.is_some() || self.game_viewport_active {
                        self.return_to_library(ctx);
                    } else {
                        self.pending_run = None;
                        self.release_all_keys();
                        self.screen = Screen::Library;
                        ctx.request_repaint();
                    }
                }
                egui::Key::F9 => {
                    self.cycle_deck_upscale_filter(ctx, true);
                }
                egui::Key::F10
                    if self.screen != Screen::InputSettings
                        && self.last_frame_snapshot.is_some() =>
                {
                    self.capture_screenshot()
                }
                egui::Key::F11 if self.screen != Screen::InputSettings => self.toggle_recording(),
                egui::Key::F12 => {
                    fullscreen = !fullscreen;
                    self.set_fullscreen(ctx, fullscreen);
                }
                _ => {}
            }
        }
    }
    fn keyboard_profile_vk(&self, key: &str) -> Option<u16> {
        pocket_library::keybindings::keyboard_button_for_profile(
            &self.library.config().keybindings,
            key,
            self.running_is_gizmondo,
        )
        .map(|b| b.vk())
    }

    fn handle_physical_keyboard(&mut self, ctx: &egui::Context) {
        if self.screen==Screen::InputSettings {return;}

        if self.stylus_editor_keyboard(ctx) {
            return;
        }
        if !ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
            return;
        }
        let text_input = self.screen == Screen::Run
            && !self.running_is_gizmondo
            && self
                .last_frame_snapshot
                .as_ref()
                .is_some_and(|f| f.text_input);
        if !text_input {
            for (input, down) in self.grid_modifier_edges(ctx) {
                self.route_grid(input, down);
            }
        }
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            if text_input {
                if let egui::Event::Text(text) | egui::Event::Paste(text) = &event {
                    for character in text.chars().filter(|c| !c.is_control()) {
                        self.send_input(InputEvent::Text { character });
                    }
                    continue;
                }
                if let egui::Event::Key { key, pressed, .. } = &event {
                    if matches!(key, egui::Key::Backspace) {
                        if *pressed {
                            self.send_input(InputEvent::Text { character: '\u{8}' });
                        }
                        continue;
                    }
                    // Printable keys arrive as committed Text, never as game actions.
                    if !matches!(
                        key,
                        egui::Key::Tab
                            | egui::Key::Enter
                            | egui::Key::Escape
                            | egui::Key::ArrowLeft
                            | egui::Key::ArrowRight
                            | egui::Key::ArrowUp
                            | egui::Key::ArrowDown
                            | egui::Key::F10
                            | egui::Key::F12
                            | egui::Key::F11
                            | egui::Key::F6
                    ) {
                        continue;
                    }
                }
            }

            let egui::Event::Key {
                key,
                pressed,
                repeat,
                ..
            } = event
            else {
                continue;
            };
            if key == egui::Key::F6
                && self.screen == Screen::Run
                && self.library.config().gps_fixed_enabled
                && self.library.config().gps_fixed_random
                && self
                    .running_game_id
                    .as_deref()
                    .and_then(|id| self.library.get(id))
                    .is_some_and(|game| pocket_library::is_colors_game(game, self.library.root()))
            {
                if pressed && !repeat {
                    if let Some(tx) = self.input_tx.as_ref() {
                        let _ = tx.send(InputCommand::RefreshColorsGps);
                    }
                }
                continue;
            }
            if key == egui::Key::Escape
                && self.screen == Screen::Run
                && ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
            {
                if pressed && !repeat {
                    self.set_fullscreen(ctx, false);
                }
                continue;
            }
            if self.screen == Screen::About && key == egui::Key::Escape && pressed && !repeat {self.close_about();continue;}
            if self.screen != Screen::Run {
                continue;
            }
            if !text_input && self.route_long_press(false,0,key.name(),pressed) {continue;}
            if !text_input && self.route_grid(HostInput::Keyboard(key.name().into()), pressed) {
                continue;
            }
            let steering_direction = self.touch_steering_enabled()
                && self.keyboard_profile_vk(key.name())
                    .is_some_and(|vk| matches!(vk, 0x25..=0x28));
            if !self.running_is_gizmondo
                && !text_input
                && !steering_direction

                && self
                    .library
                    .config()
                    .stylus_stick
                    .keyboard
                    .iter()
                    .any(|names| {
                        names
                            .iter()
                            .any(|n| pocket_library::keybindings::keys_match(n, key.name()))
                    })
            {
                if pressed {
                    self.stylus_keyboard.insert(key.name().into());
                } else {
                    self.stylus_keyboard.remove(key.name());
                }
                continue;
            }
            // Both PocketPC and Gizmondo use the same configurable host-key map.
            // PocketPC directions follow the displayed image; action bindings follow the per-game swap.
            let vk = self.keyboard_profile_vk(key.name());
            let Some(vk) = vk else {
                continue;
            };
            let vk = swapped_pocketpc_physical_vk(vk, self.running_swap_button_groups);
            let vk = if self.running_is_gizmondo {
                vk
            } else {
                rotate_direction_vk(vk, (4 - rotation_turns(self.game_rotation)) % 4)
            };
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

    fn spawn_backup_dialog(&mut self, restore: bool) {
        if self.library_busy
            || self.running_game.is_some()
            || self.pending_run.is_some()
            || self.config_draft.is_some()
            || self.game_settings_draft.is_some()
            || self.rename_draft.is_some()
            || self.pending_import.is_some()
        {
            return;
        }
        self.library_busy = true;
        self.status = if restore {
            "Choose a PocketHLE backup…"
        } else {
            "Saving library backup…"
        }
        .into();
        let purpose = if restore {
            crate::file_dialog::Purpose::RestoreBackup
        } else {
            crate::file_dialog::Purpose::SaveBackup
        };
        self.open_file_dialog(purpose, None, pocket_library::backup::suggested_filename());
    }

    fn open_file_dialog(
        &mut self,
        purpose: crate::file_dialog::Purpose,
        directory: Option<std::path::PathBuf>,
        filename: String,
    ) {
        self.controller_ui.stop_repeat();
        self.deck_nav_repeat = None;
        self.deck_library_actions.clear();
        self.file_dialog = Some(crate::file_dialog::FileDialog::new(
            purpose, directory, filename,
        ));
    }

    fn ui_file_dialog(&mut self, ctx: &egui::Context) {
        let keyboard_open = self.controller_ui.keyboard_open();
        if keyboard_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.controller_ui.back(ctx);
        }
        let outcome = self
            .file_dialog
            .as_mut()
            .and_then(|dialog| dialog.show(ctx, keyboard_open));
        self.controller_ui.show(ctx);
        let Some(outcome) = outcome else {
            return;
        };
        ctx.request_repaint();
        self.file_dialog = None;
        if keyboard_open {
            self.controller_ui.back(ctx);
        }
        self.controller_ui.stop_repeat();
        use crate::file_dialog::{Outcome, Purpose};
        match outcome {
            Outcome::Cancelled(_) => {
                self.library_busy = false;
                self.data_game = None;
                self.status = "File selection cancelled.".into();
            }
            Outcome::Selected(Purpose::AddData, path) => {
                if let Some(id) = self.data_game.take() {
                    if pocket_library::add_data::is_zip(&path.to_string_lossy()) {
                        self.add_data = Some(DataPrompt::Zip { id, path });
                    } else {
                        self.prepare_data(id, path, false);
                    }
                }
            }
            Outcome::Selected(Purpose::Import, path) => self.import_selected_file(path),
            Outcome::Selected(Purpose::RestoreBackup, path) => {
                self.restore_archive = Some(path);
                self.status = "Choose a restore mode.".into();
            }
            Outcome::Selected(Purpose::SaveBackup, path) => {
                self.status = "Saving library backup…".into();
                let root = self.library.root().to_path_buf();
                let tx = self.events_tx.clone();
                std::thread::spawn(move || {
                    let result = pocket_library::backup::save(&root, &path)
                        .map(|()| format!("Backup saved: {}", path.display()))
                        .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(UiEvent::BackupFinished(result));
                });
            }
        }
    }

    fn ui_restore_backup(&mut self, ctx: &egui::Context) {
        use pocket_library::backup::RestoreMode;
        let mut chosen = None;
        let mut cancel = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        egui::Window::new("Restore library backup")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                crate::controller_ui::page_scope(ui, "restore_backup", |ui| {
                    ui.label("Choose how to restore files already present in the library.");
                    ui.label("Replace removes existing files absent from the backup.");
                    for (label, mode) in [
                        ("Merge (no overwrite)", RestoreMode::MergeNoOverwrite),
                        ("Merge (overwrite)", RestoreMode::MergeOverwrite),
                        ("Replace", RestoreMode::Replace),
                    ] {
                        if crate::controller_ui::button(ui, label).clicked() {
                            chosen = Some(mode);
                        }
                    }
                    if crate::controller_ui::button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.restore_archive = None;
            self.library_busy = false;
            self.status = "Restore cancelled.".into();
        } else if let Some(mode) = chosen {
            let archive = self.restore_archive.take().unwrap();
            let root = self.library.root().to_path_buf();
            let tx = self.events_tx.clone();
            self.status = "Validating and restoring backup…".into();
            std::thread::spawn(move || {
                let result = crate::with_suspended_library_log(&root, || {
                    pocket_library::backup::restore(&root, &archive, mode)
                })
                .map(|()| "Library restored.".into())
                .map_err(|e| format!("{e:#}"));
                let _ = tx.send(UiEvent::BackupFinished(result));
            });
        }
    }

    fn spawn_import_dialog(&mut self) {
        if self.library_busy || self.demo_import.is_some() || self.colors_startup_prompt.is_some() {
            return;
        }
        self.library_busy = true;
        let directory = self.library.config().last_import_dir.clone();
        self.open_file_dialog(
            crate::file_dialog::Purpose::Import,
            directory,
            String::new(),
        );
        self.status = "Choose a game to import…".into();
    }

    fn import_selected_file(&mut self, path: std::path::PathBuf) {
        let profile = self.library.profile();
        let library_root = self.library.root().to_path_buf();
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Option<ImportedGames>, String> {
                let mut lib = Library::open_with_profile(&library_root, profile).map_err(|e| e.to_string())?;
                let parent = path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or(library_root.clone());
                lib.config_mut().last_import_dir = Some(parent);
                lib.save().map_err(|e| e.to_string())?;
                let pending = lib.prepare_import(&path).map_err(|e| e.to_string())?;
                if !pending.updates().is_empty()
                    || !pending.platform_choices().is_empty()
                    || !pending.cabinet_choices().is_empty()
                {
                    tx.send(UiEvent::ImportPrepared(pending))
                        .map_err(|e| e.to_string())?;
                    return Ok(None);
                }
                let entries = lib
                    .apply_prepared_import(pending, false)
                    .map_err(|e| e.to_string())?;
                Ok(Some(ImportedGames::analyze(entries, &lib, &[])))
            })();
            if let Some(result) = result.transpose() {
                let _ = tx.send(UiEvent::ImportFinished(result));
            }
        });
        self.status = "Importing...".to_string();
    }

    pub fn request_startup_window(&mut self, fullscreen: bool) {
        self.startup_window = Some(fullscreen);
    }

    pub fn launch_from_command_line(&mut self, game: &GameEntry, filter: Option<usize>) {
        if let Some(filter) = filter.and_then(|i| UpscaleFilter::ALL.get(i)).copied() {
            self.upscale_filter = filter;
        }
        self.selected_game = Some(game.id.clone());
        // Preserve the normal launch path, including required startup prompts.
        self.spawn_run(game);
    }

    fn spawn_run(&mut self, game: &GameEntry) {
        let languages = pocket_library::colors_languages(game, self.library.root());
        if pocket_library::is_colors_game(game, self.library.root()) {
            let mut draft = self.library.config().clone();
            draft.gps_fixed_enabled = false;
            draft.colors_language = pocket_library::colors_launch_language(
                game,
                self.library.root(),
                &draft.colors_language,
            )
            .to_owned();
            self.colors_startup_prompt = Some((game.clone(), languages, draft));
        } else {
            self.spawn_run_selected(game);
        }
    }

    fn ui_colors_startup(&mut self, ctx: &egui::Context) {
        if let Some((title, message)) = self.colors_startup_notice.clone() {
            let mut close = ctx.input(|input| {
                input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Escape)
            });
            close |= self
                .deck_library_actions
                .iter()
                .any(|control| matches!(control.as_str(), "South" | "East"));
            self.deck_library_actions.clear();
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(message);
                    close |= crate::controller_ui::button(ui, "OK").clicked();
                });
            if close {
                self.colors_startup_notice = None;
            }
            return;
        }
        let Some((game, languages, previous)) = self.colors_startup_prompt.as_ref() else {
            return;
        };
        let game = game.clone();
        let languages = languages.clone();
        let mut draft = previous.clone();
        let mut selected = None;
        let mut cancelled = false;
        let mut unlock_requested = false;
        if self.library_busy {
            self.deck_library_actions.clear();
        }
        if !self.library_busy {
            for control in std::mem::take(&mut self.deck_library_actions) {
                match control.as_str() {
                    "South" => {
                        selected = Some(draft.clone());
                        break;
                    }
                    "East" => {
                        cancelled = true;
                        break;
                    }
                    "West" if self.running_game.is_none() => {
                        unlock_requested = true;
                        break;
                    }
                    direction
                        if crate::deck_navigation::is_direction(direction)
                            && languages.len() > 1 =>
                    {
                        let index = languages
                            .iter()
                            .position(|language| language.id == draft.colors_language)
                            .unwrap_or(0);
                        if let Some(next) =
                            crate::deck_navigation::next_index(index, languages.len(), 1, direction)
                        {
                            draft.colors_language = languages[next].id.to_owned();
                        }
                    }
                    _ => {}
                }
            }
        }
        egui::Window::new("Colors startup")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                if !self.gamepad_devices.is_empty() {
                    ui.label(crate::controller_labels::text(ctx, "{South}: Start • {East}: Back • {West}: Unlock Turf Wars + Tag Editor • D-pad / stick: language"));
                }
                if languages.len() > 1 {
                ui.label("Language");
                for language in &languages {
                    ui.radio_value(&mut draft.colors_language, language.id.to_owned(), language.label);
                }
                } else if let Some(language) = languages.first() { ui.label(format!("Language: {}", language.label)); }
        {
            crate::controller_ui::checkbox(ui, &mut draft.gps_fixed_enabled, "Fake GPS")
                .on_hover_text("Use a simulated position instead of host location.");
            ui.add_enabled_ui(draft.gps_fixed_enabled, |ui| {
                crate::controller_ui::ComboBox::from_id_source("simulated_gps_mode")
                    .selected_text(if draft.gps_fixed_random { "Near Eiffel Tower (within 800 m)" } else { "Manual coordinates" })
                    .show_ui(ui, |ui| {
                        crate::controller_ui::selectable_value(ui, &mut draft.gps_fixed_random, true, "Near Eiffel Tower (within 800 m)");
                        crate::controller_ui::selectable_value(ui, &mut draft.gps_fixed_random, false, "Manual coordinates");
                    });
                if draft.gps_fixed_enabled && draft.gps_fixed_random {
                    ui.label("F6 / Geofence: choose a new random position while playing.");
                }
                if !draft.gps_fixed_random {
                ui.horizontal(|ui| {
                    ui.label("Latitude");
                    ui.add(egui::DragValue::new(&mut draft.gps_fixed_latitude).speed(0.0001).clamp_range(-90.0..=90.0).max_decimals(7));
                });
                ui.horizontal(|ui| {
                    ui.label("Longitude");
                    ui.add(egui::DragValue::new(&mut draft.gps_fixed_longitude).speed(0.0001).clamp_range(-180.0..=180.0).max_decimals(7));
                });
                ui.label("Decimal coordinates: north/east positive, south/west negative.");
                }
                ui.label("The game will use this simulated position instead of host location.");
            });
        }
                ui.separator();
                ui.label("Colors multiplayer server");
                crate::controller_ui::text_edit(ui, &mut draft.colors_server_url, "")
                    .on_hover_text("Host, IP or HTTP(S) origin. Leave empty to use the public Colors server.");
                ui.separator();
                if crate::controller_ui::add_enabled(ui, !self.library_busy && self.running_game.is_none(), egui::Button::new("Unlock Turf Wars + Tag Editor")).clicked() {
                    unlock_requested = true;
                }
                if self.library_busy { ui.horizontal(|ui| { ui.spinner(); ui.label("Unlocking Turf Wars + Tag Editor…"); }); }
                if crate::controller_ui::add_enabled(ui, !self.library_busy, egui::Button::new("Start")).clicked() {
                    selected = Some(draft.clone());
                }
                if crate::controller_ui::add_enabled(ui, !self.library_busy, egui::Button::new("Cancel")).clicked() {
                    cancelled = true;
                }
            });
        if let Some((_, _, value)) = self.colors_startup_prompt.as_mut() {
            *value = draft;
        }
        if unlock_requested && !self.library_busy && self.running_game.is_none() {
            self.library_busy = true;
            self.status = "Unlocking Turf Wars + Tag Editor…".into();
            let root = self.library.root().to_path_buf();
            let tx = self.events_tx.clone();
            std::thread::spawn(move || {
                let result = pocket_library::colors_save::unlock_turf_wars(&game, &root)
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiEvent::ColorsUnlocked(result));
            });
            return;
        }
        if let Some(draft) = selected {
            let input = draft.colors_server_url.trim().trim_end_matches('/');
            let server = if input.is_empty() || input.contains("://") {
                input.to_owned()
            } else {
                format!("http://{input}")
            };
            if pocket_core::kernel::internet::State::default()
                .set_colors_endpoint(&server)
                .is_err()
            {
                self.colors_startup_notice = Some((
                    "Colors startup".into(),
                    "Enter a host or HTTP(S) origin without a path.".into(),
                ));
                return;
            }
            let config = self.library.config_mut();
            config.colors_language = draft.colors_language;
            config.gps_fixed_enabled = draft.gps_fixed_enabled;
            config.gps_fixed_random = draft.gps_fixed_random;
            config.gps_fixed_latitude = draft.gps_fixed_latitude;
            config.gps_fixed_longitude = draft.gps_fixed_longitude;
            config.colors_server_url = server;
            match self.library.save() {
                Ok(()) => {
                    self.colors_startup_prompt = None;
                    self.spawn_run_selected(&game);
                }
                Err(error) => self.status = format!("Could not save startup options: {error}"),
            }
        } else if cancelled {
            self.colors_startup_prompt = None;
        }
    }

    fn spawn_run_selected(&mut self, game: &GameEntry) {
        #[cfg(windows)]
        if self.library.config().gps_enabled
            && !(self.library.config().gps_fixed_enabled
                && pocket_library::is_colors_game(game, self.library.root()))
        {
            pocket_core::kernel::gps::prepare_host_access();
        }
        if self.running_game.is_some() {
            self.pending_run = Some(game.clone());
            self.release_all_keys();
            if let Some(tx) = self.input_tx.as_ref() {
                let _ = tx.send(InputCommand::Stop);
            }
            self.status = format!(
                "Stopping current game before starting {}...",
                game.display_name
            );
            return;
        }
        self.last_frame_texture = None;
        self.uploaded_image_id = None;
        self.last_frame_snapshot = None;
        self.last_frame_status = None;
        self.frame_stats.reset();
        self.rate_badge = None;
        if let Some(renderer) = self.reconstruction.as_ref() {
            renderer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .begin_session(self.upscale_filter.gpu_filter());
        }
        // Start turned the way this game was saved: a portrait-rendered
        // landscape title is unplayable until it is turned, and the user
        // should not have to re-pick that on every launch.
        self.game_rotation = game.settings.rotation;
        self.gamepad_rx.set_deadzone(game.settings.dpad_deadzone);
        self.screen = Screen::Run;
        self.game_viewport_active = true;
        self.running_game = Some(game.display_name.clone());
        self.running_game_id = Some(game.id.clone());
        self.running_is_gizmondo = is_gizmondo_game(game, self.library.root());
        self.running_swap_button_groups =
            !self.running_is_gizmondo && game.settings.pocketpc_swap_button_groups;
        self.running_controls_mode = if self.running_is_gizmondo {
            ControlsMode::Normal
        } else {
            game.settings.controls_mode()
        };
        self.stylus_pointer=Default::default();
        self.stylus_tick=Instant::now();
        self.running_stylus_grid = game.settings.pocketpc_stylus_grid.clone();
        let (frame_tx, frame_rx) = crate::latest_frame::channel();
        let (input_tx, input_rx) = mpsc::channel();
        let (dependency_notice_tx, dependency_notice_rx) = mpsc::channel();
        self.dependency_notice_rx = Some(dependency_notice_rx);
        self.dependency_notice = None;
        let (gps_notice_tx, gps_notice_rx) = mpsc::channel();
        self.gps_notice_rx = Some(gps_notice_rx);
        self.gps_notice = None;
        self.frame_rx = Some(frame_rx);
        self.input_tx = Some(input_tx);
        self.system_media = SystemMediaView::default();
        self.media_keys_down = [false; 2];
        let system_media = self.system_media.clone();
        if !self.running_is_gizmondo {
            self.send_input(InputEvent::KeyboardRotation {
                turns: rotation_turns(self.game_rotation),
            });
        }
        self.held = HeldButtons::default();
        self.pointer_down_at = None;
        let library_root = self.library.root().to_path_buf();
        let game = game.clone();
        let tx = self.events_tx.clone();
        let runner = self.runner.clone();
        self.run_thread = Some(std::thread::spawn(move || {
            let outcome = runner.run_game(
                library_root,
                game,
                Some(frame_tx),
                Some(input_rx),
                Some(gps_notice_tx),
                Some(dependency_notice_tx),
                system_media,
            );
            let _ = tx.send(UiEvent::RunFinished(outcome));
        }));
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
    crate::desktop_pointer::held_in(ctx, rect)
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

impl PocketLauncher {
    fn finish_shared_gpu_worker(&mut self) {
        self.runner.stop_shared_worker(
            &mut self.shared_gpu_context,
            &mut self.input_tx,
            &mut self.frame_rx,
            &mut self.run_thread,
        );
    }
}
impl Drop for PocketLauncher {
    fn drop(&mut self) {
        self.finish_shared_gpu_worker();
    }
}

impl eframe::App for PocketLauncher {
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.cancel_center_tap();
        self.finish_shared_gpu_worker();
        self.recording
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .finish_and_wait();
        if let (Some(gl), Some(renderer)) = (gl, self.reconstruction.take()) {
            renderer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .destroy(gl);
        }
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.screen == Screen::InputSettings || (self.screen == Screen::Run && !ctx.wants_keyboard_input()) {
            crate::host_shortcuts::normalize_number_row(raw);
        }
        if self.screen == Screen::Run {
            let active = self.media_active();
            raw.events.retain(|event| {
                let egui::Event::Key { key, pressed, .. } = event else {
                    return true;
                };
                let (index, command) = match key {
                    egui::Key::Enter => (0, SystemMediaCommand::TogglePause),
                    egui::Key::Escape => (1, SystemMediaCommand::Stop),
                    _ => return true,
                };
                let captured = self.media_keys_down[index];
                if *pressed && active {
                    self.media_keys_down[index] = true;
                    return false;
                }
                if !*pressed && captured {
                    self.media_keys_down[index] = false;
                    if active {
                        self.media_command(command);
                    }
                    return false;
                }
                true
            });
        }
        crate::desktop_pointer::finish_missing_releases(ctx, raw);
        self.controller_ui.prepare_input(
            ctx,
            raw,
            self.screen != Screen::Run
                || self.file_dialog.is_some()
                || self.restore_archive.is_some(),
        );
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let released = self.center_tap.borrow_mut().release_due(self.input_gesture_clock.elapsed().as_millis() as u64);
        if let Some((x,y)) = released { self.dispatch_input(InputEvent::PointerUp {x,y}); }
        let commands=std::mem::take(&mut *self.pending_binding_commands.borrow_mut());
        for command in commands {
            if matches!(command,pocket_library::keybindings::STYLUS_CLICK_COMMAND|pocket_library::keybindings::STYLUS_RELEASE_COMMAND)
                && !ctx.input(|i|i.viewport().focused.unwrap_or(true)) {self.stylus_pointer.cancel_click();continue;}
            match command {
                pocket_library::keybindings::STYLUS_CLICK_COMMAND => self.stylus_button(true),
                pocket_library::keybindings::STYLUS_RELEASE_COMMAND => self.stylus_button(false),
                pocket_library::keybindings::ROTATION_COMMAND => self.cycle_rotation(ctx),
                pocket_library::keybindings::SCREENSHOT_COMMAND if self.last_frame_snapshot.is_some() => self.capture_screenshot(),
                _ => {}
            }
        }
        self.handle_host_keys(ctx);
        if self.screen == Screen::InputSettings {
            self.bindings_visual.keyboard_shortcut(ctx, self.input_controller);
        }
        // Input-editor keystrokes are visual tests; the assistant and open popups retain events.
        if self.screen==Screen::InputSettings && !self.bindings_visual.quick_active()
            && !ctx.memory(|memory|memory.any_popup_open()) {
            ctx.input_mut(|input|input.events.retain(|event|!matches!(event,egui::Event::Key{..}|egui::Event::Text(_))));
        }

        if self.screen==Screen::Run && ctx.input(|i|i.viewport().focused.unwrap_or(true)) {
            let edges=self.input_gestures.tick(self.input_gesture_clock.elapsed().as_millis() as u64);self.send_gesture_edges(edges);
            if self.input_gestures.active() {ctx.request_repaint_after(Duration::from_millis(10));}
        }
        // Early-return dialogs retain ordinary host repaint behavior. Only a
        // complete Run update enables image-driven redraws.
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("pockethle.frame_driven_ui"), false);
        });
        // Observe the launcher before input/events can start a game. Freeze this
        // state throughout the session, including a queued game-to-game launch.
        let observed = LibraryViewport::capture(ctx);
        if self.screen == Screen::Library
            && self.running_game.is_none()
            && self.library_restore_pending.is_none()
        {
            self.library_viewport = observed;
        } else if self.library_viewport.size.is_none() {
            self.library_viewport.size = observed.size;
            self.library_viewport.position = observed.position;
        }
        // Apply startup mode after native restoration, then size a normal window
        // using a fixed physical target capped to the actual monitor, never a game.
        if let Some(fullscreen) = self.startup_window {
            let target = LibraryViewport { fullscreen, maximized: false, size: None, position: None };
            if target.restore(ctx) {
                if fullscreen {
                    self.startup_window = None;
                } else {
                    let geometry = ctx.input(|input| {
                        let viewport = input.viewport();
                        let monitor = viewport.monitor_size?;
                        let borders = viewport.inner_rect.zip(viewport.outer_rect)
                            .map_or(Vec2::ZERO, |(inner, outer)| outer.size() - inner.size());
                        crate::launch_args::startup_window_size([monitor.x, monitor.y], [borders.x, borders.y], input.pixels_per_point)
                    });
                    if let Some(size) = geometry {
                        let size = Vec2::from(size);
                        let minimum = Vec2::new(640.0, 420.0) / ctx.pixels_per_point();
                        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(minimum.min(size)));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                        self.fullscreen_window_size = Some(size);
                        self.library_viewport = LibraryViewport { size: Some(size), ..target };
                        self.startup_window = None;
                    } else {
                        ctx.request_repaint_after(Duration::from_millis(16));
                    }
                }
            }
        }

        crate::controller_ui::begin_frame(ctx);
        if self.screen != Screen::Library
            || self.library_tab != crate::deck_navigation::LibraryTab::Recordings
        {
            self.recordings_gallery.close_player();
        }
        self.drain_events(ctx);
        if let Some(renderer) = self.reconstruction.as_ref() {
            if renderer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .needs_session_cleanup()
            {
                let renderer = renderer.clone();
                ctx.layer_painter(egui::LayerId::background())
                    .add(egui::PaintCallback {
                        rect: ctx.screen_rect(),
                        callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(
                            move |_, _| {
                                renderer
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .collect_session_resources();
                            },
                        )),
                    });
            }
        }
        if self.file_dialog.is_some() {
            self.update_gamepad_input(ctx);
            self.ui_file_dialog(ctx);
            return;
        }
        if self.add_data.is_some() {
            self.update_gamepad_input(ctx);
            self.ui_add_data(ctx);
            self.controller_ui.show(ctx);
            return;
        }
        if self.restore_archive.is_some() {
            self.update_gamepad_input(ctx);
            egui::CentralPanel::default().show(ctx, |_| {});
            self.ui_restore_backup(ctx);
            self.controller_ui.show(ctx);
            return;
        }
        if let Some(id) = self.game_id_popup.clone() {
            self.handle_gamepads(ctx);
            egui::CentralPanel::default().show(ctx, |_| {});
            let mut open = true;
            let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            egui::Window::new("Game ID")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .max_width((ctx.screen_rect().width() - 32.0).max(100.0))
                .show(ctx, |ui| {
                    crate::controller_ui::page_scope(ui, "game_id_popup", |ui| {
                        ui.label(egui::RichText::new(&id).monospace());
                        ui.label("Copied to clipboard.");
                        if crate::controller_ui::button(ui, "Copy again").clicked() {
                            ctx.output_mut(|o| o.copied_text = id.clone());
                        }
                        if crate::controller_ui::button(ui, "Close").clicked() {
                            close = true;
                        }
                    });
                });
            if close || !open {
                self.game_id_popup = None;
            }
            return;
        }

        if let Some(error) = self.import_error.clone() {
            egui::CentralPanel::default().show(ctx, |_| {});
            let mut open = true;
            let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            egui::Window::new(self.import_error_title)
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(440.0)
                .max_width((ctx.screen_rect().width() - 32.0).max(100.0))
                .max_height((ctx.screen_rect().height() - 32.0).max(100.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(&error);
                    });
                    if crate::controller_ui::button(ui, "Close").clicked() {
                        close = true;
                    }
                });
            if close || !open {
                self.import_error = None;
            }
            return;
        }
        if self.screen != Screen::Run || self.last_frame_snapshot.is_none() {
            let mut r = self.recording.lock().unwrap_or_else(|e| e.into_inner());
            if r.active() {
                r.stop();
                if let Some(tx) = &self.input_tx {
                    let _ = tx.send(InputCommand::RecordAudio(None));
                }
            }
        }
        let video_result = {
            self.recording
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .poll()
        };
        if let Some(result) = video_result {
            self.status = match result {
                Ok(paths) => format!(
                    "Video saved: {} ({} segment(s))",
                    paths[0].display(),
                    paths.len()
                ),
                Err(e) => format!("Video recording failed: {e}"),
            };
        }
        self.update_gamepad_input(ctx);
        if self.screen == Screen::Run {
            if let Some((_, pressed_at)) = self.deck_view_press {
                if crate::deck_navigation::menu_action(pressed_at.elapsed(), false)
                    == Some(crate::deck_navigation::MenuAction::ReturnToLibrary)
                {
                    self.deck_view_press = None;
                    self.return_to_library(ctx);
                }
            }
            let mut toggle_right_recording = false;
            if let Some((_, pressed_at)) = self.deck_menu_press {
                if crate::deck_navigation::right_fn_action(pressed_at.elapsed(), false)
                    == Some(crate::deck_navigation::RightFnAction::ToggleRecording)
                {
                    self.deck_menu_press = None;
                    toggle_right_recording = true;
                }
            }
            if toggle_right_recording {
                self.toggle_recording();
            }
        }
        if self.screen != Screen::Library
            || !ctx.input(|i| i.viewport().focused.unwrap_or(true))
            || self.colors_startup_prompt.is_some()
            || self.library_busy
            || self.rename_draft.is_some()
            || self.remove_game.is_some()
            || self.demo_import.is_some()
            || self.pending_import.is_some()
        {
            self.deck_nav_repeat = None;
        } else if let Some((control, next)) = self.deck_nav_repeat.as_mut() {
            if Instant::now() >= *next {
                self.deck_library_actions.push(control.clone());
                *next = Instant::now() + Duration::from_millis(120);
            }
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        if self.colors_startup_prompt.is_some() {
            egui::CentralPanel::default().show(ctx, |_| {});
            self.ui_colors_startup(ctx);
            self.controller_ui.show(ctx);
            return;
        }
        if self.library_busy || self.demo_import.is_some() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    if self.library_busy {
                        ui.spinner();
                        ui.label(&self.status);
                    }
                });
            });
            self.ui_import_update(ctx);
            if !self.library_busy {
                self.ui_demo_import(ctx);
            }
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if let Some(prompt)=self.remove_game.as_ref() {
            self.deck_nav_repeat=None;
            self.controller_ui.stop_repeat();
            let actions=std::mem::take(&mut self.deck_library_actions);
            if let Some(confirm)=prompt.show(ctx,&actions) {
                let prompt=self.remove_game.take().unwrap();
                if confirm && self.running_game.is_some() {
                    self.status="Stop the running game before removing a library entry.".into();
                } else if confirm {
                    match self.library.remove(&prompt.id) {
                        Ok(())=> {
                            self.analysis_cache.remove(&prompt.id);
                            if self.selected_game.as_deref()==Some(&prompt.id) {self.selected_game=None;}
                            self.status=format!("Removed {}",prompt.name);
                        }
                        Err(error)=>self.status=format!("Remove failed: {error}"),
                    }
                }
            }
            return;
        }
        if self.ui_pocketpc_import_options(ctx) {
            return;
        }
        if self.ui_release_update(ctx) {
            self.controller_ui.show(ctx);
            return;
        }
        self.handle_physical_keyboard(ctx);
        self.update_stylus_input(ctx);
        if !self.pending_binding_commands.borrow().is_empty() {ctx.request_repaint();}
        self.drain_events(ctx);
        if let Some(result) = self
            .screenshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .completed
            .take()
        {
            self.status = match result {
                Ok(path) => format!("Screenshot saved: {}", path.display()),
                Err(e) => format!("Screenshot failed: {e}"),
            };
        }
        // A game can finish or leave the Run screen before the paint callback.
        if self.screen != Screen::Run || self.last_frame_snapshot.is_none() {
            let mut capture = self.screenshot.lock().unwrap_or_else(|e| e.into_inner());
            if capture.pending.take().is_some() {
                capture.busy = false;
                self.status = "Screenshot cancelled: game screen is unavailable.".into();
            }
        }
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        if fullscreen != self.display_fullscreen {
            self.display_fullscreen = fullscreen;
            if !fullscreen && self.library_restore_pending.is_none() && self.startup_window.is_none() {
                let size = self.fullscreen_window_size.take()
                    .or(self.library_viewport.size)
                    .unwrap_or(Vec2::new(1280.0, 800.0));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
            if let Some(frame) = self.last_frame_snapshot.clone() {
                self.refresh_frame_texture(ctx, &frame);
            }
        }
        if let Some(saved) = self.library_restore_pending {
            if saved.restore(ctx) {
                self.library_restore_pending = None;
            }
        }
        let console_only = fullscreen && self.screen == Screen::Run;
        if !console_only {
            egui::TopBottomPanel::top("top").show(ctx, |ui| {
                ui.add_enabled_ui(
                    self.information_game.is_none()
                        && self.rename_draft.is_none()
                        && !self.controller_ui.keyboard_open(),
                    |ui| crate::controller_ui::toolbar_scope(ui, |ui| self.ui_top_bar(ui)),
                );
            });
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let reserve = 45.0;
                    ui.add_sized(
                        Vec2::new(
                            (ui.available_width() - reserve).max(0.0),
                            ui.text_style_height(&egui::TextStyle::Small),
                        ),
                        egui::Label::new(
                            RichText::new(self.status.lines().next().unwrap_or("")).small(),
                        )
                        .truncate(true),
                    )
                    .on_hover_text(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .small()
                                .color(Color32::from_gray(140)),
                        );
                    });
                });
                ui.add(egui::Label::new(RichText::new(crate::controller_labels::text(ctx,crate::toolbar_ui::NAV_HINT)).size(12.0).color(Color32::from_gray(153))).wrap(true));
            });
        }
        let central = egui::CentralPanel::default();
        let central = if console_only {
            central.frame(egui::Frame::none().fill(Color32::BLACK))
        } else if self.screen == Screen::Run {
            central.frame(egui::Frame::none())
        } else {
            central
        };
        central.show(ctx, |ui| {
            if self.controller_ui.keyboard_open() {
                ui.set_enabled(false);
            }
            if self.library_busy && self.screen != Screen::About {
                ui.spinner();
                ui.label(&self.status);
                return;
            }
            if console_only {
                self.ui_fullscreen_game(ui);
                return;
            }
            match self.screen {
                Screen::About => self.ui_about(ui),
                Screen::Library => {
                    ui.add_enabled_ui(
                        self.information_game.is_none() && self.rename_draft.is_none(),
                        |ui| {
                            crate::controller_ui::page_scope(ui, "library", |ui| {
                                self.ui_library(ui)
                            })
                        },
                    );
                }
                Screen::Settings
                | Screen::GizmondoOptions
                | Screen::PocketPcOptions
                | Screen::InputSettings => {
                    let page = match self.screen {
                        Screen::GizmondoOptions => "gizmondo_options",
                        Screen::PocketPcOptions => "pocketpc_options",
                        Screen::InputSettings => "input_settings",
                        _ => "emulator_settings",
                    };
                    crate::controller_ui::page_scope(ui, page, |ui| self.ui_settings(ui));
                }
                Screen::GameSettings if self.stylus_editor.is_some() => {
                    if let Some((id, mut draft)) = self.game_settings_draft.take() {
                        ui.heading("PocketPC controls");
                        self.ui_stylus_editor(ui.ctx(), &mut draft);
                        self.game_settings_draft = Some((id, draft));
                    }
                }
                Screen::GameSettings => {
                    crate::controller_ui::page_scope(ui, "game_settings_page", |ui| {
                        ScrollArea::vertical().show(ui, |ui| {
                            self.ui_game_settings(ui);
                            crate::controller_ui::scroll_focus(ui);
                        });
                    });
                }
                Screen::Run => self.ui_run(ui),
            }
        });
        if self.screen == Screen::Library {
            self.ui_game_information(ctx);
        }
        if let Some((label, expires)) = self.control_notice.clone() {
            if Instant::now() < expires {
                egui::Area::new(egui::Id::new("control_notice"))
                    .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(label);
                        });
                    });
            } else {
                self.control_notice = None;
            }
        }
        if let Some((message, expires)) = self.gps_notice.clone() {
            if Instant::now() < expires && self.screen == Screen::Run {
                egui::Area::new(egui::Id::new("gps_refresh_notice"))
                    .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 16.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(message);
                        });
                    });
            } else {
                self.gps_notice = None;
            }
        }
        if !console_only && !self.library_busy {
            self.ui_demo_import(ctx);
            self.ui_rename_game(ctx);
        }
        if self.game_menu_parent.is_some()
            && self.screen != Screen::GameSettings
            && self.information_game.is_none()
            && self.rename_draft.is_none()
        {
            self.game_menu_parent = None;
        }
        if self.screen != Screen::Run {
            self.controller_ui.show(ctx);
        }
        // Image arrival wakes the native event loop. Keep a bounded fallback
        // for hover/controller processing when a game stops publishing images.
        let frame_driven = self.screen == Screen::Run && self.frame_rx.is_some();
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("pockethle.frame_driven_ui"), frame_driven);
        });
        let repaint_after = if frame_driven {
            // Images notify immediately. Keep input/hover/controller processing
            // alive even if the guest stops producing images; no FPS cap here.
            Duration::from_millis(100)
        } else if matches!(self.screen, Screen::Run) || self.frame_rx.is_some() {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(250)
        };
        ctx.request_repaint_after(repaint_after);
        if self.center_tap.borrow().active() { ctx.request_repaint_after(Duration::from_millis(pocket_library::center_tap::DURATION_MS)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library_restore_pass(
        saved: LibraryViewport,
        fullscreen: bool,
        maximized: bool,
    ) -> (bool, Vec<egui::ViewportCommand>) {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        let viewport = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        viewport.fullscreen = Some(fullscreen);
        viewport.maximized = Some(maximized);
        viewport.inner_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0)));
        let mut done = false;
        let output = ctx.run(input, |ctx| {
            done = saved.restore(ctx);
        });
        (
            done,
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .clone(),
        )
    }

    #[test]
    fn library_windowed_restore_waits_for_fullscreen_exit_before_geometry() {
        let saved = LibraryViewport {
            fullscreen: false,
            maximized: false,
            size: Some(Vec2::new(960.0, 600.0)),
            position: Some(Pos2::new(70.0, 40.0)),
        };
        let (done, commands) = library_restore_pass(saved, true, false);
        assert!(!done);
        assert_eq!(commands, vec![egui::ViewportCommand::Fullscreen(false)]);
        // Only the next native acknowledgement allows restoring size/position.
        let (done, commands) = library_restore_pass(saved, false, false);
        assert!(done);
        assert_eq!(
            commands,
            vec![
                egui::ViewportCommand::OuterPosition(Pos2::new(70.0, 40.0)),
                egui::ViewportCommand::InnerSize(Vec2::new(960.0, 600.0))
            ]
        );
    }

    #[test]
    fn library_that_started_fullscreen_stays_fullscreen_without_resizing() {
        let saved = LibraryViewport {
            fullscreen: true,
            maximized: false,
            size: Some(Vec2::new(1920.0, 1080.0)),
            position: Some(Pos2::ZERO),
        };
        let (done, commands) = library_restore_pass(saved, false, false);
        assert!(!done);
        assert_eq!(commands, vec![egui::ViewportCommand::Fullscreen(true)]);
        let (done, commands) = library_restore_pass(saved, true, false);
        assert!(done);
        assert!(commands.is_empty());
    }

    #[test]
    fn library_maximized_restore_does_not_apply_game_or_monitor_size() {
        let saved = LibraryViewport {
            fullscreen: false,
            maximized: true,
            size: Some(Vec2::new(1920.0, 1040.0)),
            position: Some(Pos2::ZERO),
        };
        let (done, commands) = library_restore_pass(saved, true, false);
        assert!(!done);
        assert_eq!(commands, vec![egui::ViewportCommand::Fullscreen(false)]);
        let (done, commands) = library_restore_pass(saved, false, false);
        assert!(!done);
        assert_eq!(commands, vec![egui::ViewportCommand::Maximized(true)]);
        let (done, commands) = library_restore_pass(saved, false, true);
        assert!(done);
        assert!(commands.is_empty());
    }

    #[test]
    fn library_unmaximized_restore_waits_before_applying_saved_geometry() {
        let saved = LibraryViewport {
            fullscreen: false,
            maximized: false,
            size: Some(Vec2::new(960.0, 600.0)),
            position: None,
        };
        let (done, commands) = library_restore_pass(saved, false, true);
        assert!(!done);
        assert_eq!(commands, vec![egui::ViewportCommand::Maximized(false)]);
        let (done, commands) = library_restore_pass(saved, false, false);
        assert!(done);
        assert_eq!(
            commands,
            vec![egui::ViewportCommand::InnerSize(Vec2::new(960.0, 600.0))]
        );
    }

    #[test]
    fn displayed_fps_excludes_dropped_frames_repaints_and_capture_redraws() {
        let stats = FrameStats::default();
        // A producer can submit 400 images; only every seventh reaches paint.
        for image_id in 1..=400 {
            if image_id % 7 == 0 {
                for _ in 0..10 {
                    stats.record_frame(image_id);
                }
            }
        }
        assert_eq!(stats.state.lock().unwrap().1.completed(), 57);
        let capture_clone = stats.clone();
        capture_clone.record_frame(399);
        assert_eq!(stats.state.lock().unwrap().1.completed(), 57);
        let mut fresh_session = stats.clone();
        fresh_session.reset();
        assert_eq!(fresh_session.current_fps(), 0.0);
        assert_eq!(fresh_session.state.lock().unwrap().1.completed(), 0);
        stats.record_frame(401); // A previous session callback cannot affect the reset meter.
        assert_eq!(fresh_session.state.lock().unwrap().1.completed(), 0);
    }

    #[test]
    fn physical_swap_matches_classic_slots_and_leaves_other_keys_unchanged() {
        for (source, destination) in [
            (0x11, 0xc4),
            (0xc2, 0x10),
            (0x0d, 0xc3),
            (0xc1, 0x20),
            (0xc4, 0x11),
            (0x10, 0xc2),
            (0xc3, 0x0d),
            (0x20, 0xc1),
        ] {
            assert_eq!(swapped_pocketpc_physical_vk(source, true), destination);
            assert_eq!(swapped_pocketpc_physical_vk(source, false), source);
            assert_eq!(swapped_pocketpc_physical_vk(destination, true), source);
        }
        for vk in [0x25, 0x26, 0x27, 0x28, 0x09, 0x1b, 0x72, 0x70] {
            assert_eq!(swapped_pocketpc_physical_vk(vk, true), vk);
        }
    }

    #[test]
    fn deck_filter_steps_wrap_and_reverse_without_skipping_a_filter() {
        for filter in UpscaleFilter::ALL {
            assert_eq!(filter.stepped(true).stepped(false), filter);
            let mut current = filter;
            for _ in 0..UpscaleFilter::ALL.len() {
                current = current.stepped(true);
            }
            assert_eq!(current, filter);
        }
        assert_eq!(UpscaleFilter::Nearest.stepped(true), UpscaleFilter::None);
        assert_eq!(UpscaleFilter::None.stepped(false), UpscaleFilter::Nearest);
    }

    #[test]
    fn saved_filter_ids_restore_every_mode_and_unknown_ids_fall_back() {
        for filter in UpscaleFilter::ALL {
            assert_eq!(UpscaleFilter::from_id(filter.id()), filter);
        }
        assert_eq!(
            UpscaleFilter::from_id("future_filter"),
            UpscaleFilter::Reconstruction
        );
    }
    #[test]
    fn ppc_side_pad_maps_display_directions_back_to_guest() {
        let up = GuestButton::DpadUp.vk();
        for (rotation, expected) in [
            (RotationPref::None, GuestButton::DpadUp),
            (RotationPref::Cw90, GuestButton::DpadLeft),
            (RotationPref::Half, GuestButton::DpadDown),
            (RotationPref::Ccw90, GuestButton::DpadRight),
        ] {
            let inverse = (4 - rotation_turns(rotation)) % 4;
            assert_eq!(rotate_direction_vk(up, inverse), expected.vk());
            for button in [
                GuestButton::Action,
                GuestButton::ButtonA,
                GuestButton::ButtonB,
                GuestButton::ButtonC,
                GuestButton::Soft1,
                GuestButton::Soft2,
                GuestButton::Turbo,
            ] {
                assert_eq!(rotate_direction_vk(button.vk(), inverse), button.vk());
            }
        }
    }

    use pocket_core::kernel::gapi;
    use pocket_library::KeyBindings;

    /// Drawing the on-screen pad must not cancel a key the user is
    /// holding on the keyboard. the virtual controls run every frame and releases
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
    fn controller_keyboard_pointer_overlap_and_disconnect_release() {
        let mut held = HeldButtons::default();
        let vk = GuestButton::Action.vk();
        assert!(held.press(InputSource::Gamepad, vk));
        assert!(!held.press(InputSource::Keyboard, vk));
        assert!(!held.release(InputSource::Gamepad, vk));
        assert!(held.is_held(vk));
        assert!(!held.press(InputSource::Pointer, vk));
        assert!(!held.release(InputSource::Keyboard, vk));
        assert!(held.release(InputSource::Pointer, vk));
        assert!(held.press(InputSource::Gamepad, vk));
        assert_eq!(held.drain_all(), vec![vk]);
        assert!(!held.release(InputSource::Gamepad, vk));
    }

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
        assert_eq!(bindings.vk_for_key(egui::Key::F11.name()), None);
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

#[derive(Clone, Copy)]
enum GizIcon {
    Text(&'static str),
    Function(usize),
    Media(usize),
}
fn paint_giz_icon(p: &egui::Painter, c: Pos2, r: f32, icon: GizIcon) {
    let color = Color32::WHITE;
    let stroke = egui::Stroke::new((r * 0.12).max(1.2), color);
    let line = |a: Vec2, b: Vec2| {
        p.line_segment([c + a * r, c + b * r], stroke);
    };
    let polygon = |points: &[Vec2]| {
        p.add(egui::Shape::convex_polygon(
            points.iter().map(|v| c + *v * r).collect(),
            color,
            egui::Stroke::new(0., color),
        ));
    };
    let arc = |origin: Vec2, radius: f32, start: f32, end: f32| {
        let points = (0..=20)
            .map(|i| {
                let a = start + (end - start) * i as f32 / 20.;
                c + (origin + Vec2::new(a.cos(), a.sin()) * radius) * r
            })
            .collect();
        p.add(egui::Shape::line(points, stroke));
    };
    let pi = std::f32::consts::PI;
    match icon {
        GizIcon::Function(0) => {
            line(Vec2::new(-1., 0.), Vec2::new(0., -0.9));
            line(Vec2::new(0., -0.9), Vec2::new(1., 0.));
            line(Vec2::new(-0.65, -0.2), Vec2::new(-0.65, 0.8));
            line(Vec2::new(0.65, -0.2), Vec2::new(0.65, 0.8));
            line(Vec2::new(-0.65, 0.8), Vec2::new(0.65, 0.8));
        }
        GizIcon::Function(1) => {
            polygon(&[
                Vec2::new(-0.9, -0.3),
                Vec2::new(-0.5, -0.3),
                Vec2::new(0., -0.8),
                Vec2::new(0., 0.8),
                Vec2::new(-0.5, 0.3),
                Vec2::new(-0.9, 0.3),
            ]);
            arc(Vec2::new(-0.1, 0.), 0.65, -0.75, 0.75);
            arc(Vec2::new(-0.1, 0.), 1.05, -0.75, 0.75);
        }
        GizIcon::Function(2) => {
            p.circle_stroke(c, r * 0.45, stroke);
            for i in 0..8 {
                let a = i as f32 * pi / 4.;
                let v = Vec2::new(a.cos(), a.sin());
                line(v * 0.7, v);
            }
        }
        GizIcon::Function(3) => {
            line(Vec2::new(0., -0.25), Vec2::new(0., 0.85));
            line(Vec2::new(-0.4, 0.85), Vec2::new(0.4, 0.85));
            p.circle_filled(c + Vec2::new(0., -0.35) * r, r * 0.12, color);
            for radius in [0.5, 0.9] {
                arc(Vec2::new(0., -0.35), radius, -0.7, 0.7);
                arc(Vec2::new(0., -0.35), radius, pi - 0.7, pi + 0.7);
            }
        }
        GizIcon::Function(_) => {
            arc(Vec2::ZERO, 0.85, -pi / 2. + 0.5, 3. * pi / 2. - 0.5);
            line(Vec2::new(0., -1.), Vec2::new(0., 0.1));
        }
        GizIcon::Media(0) => {
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(r * 1.25)), 1., color);
        }
        GizIcon::Media(n) => {
            let count = if n == 3 { 1 } else { 2 };
            for i in 0..count {
                let x = if count == 1 {
                    -0.25
                } else {
                    -0.55 + i as f32 * 0.85
                };
                let sign = if n == 1 { -1. } else { 1. };
                polygon(&[
                    Vec2::new(sign * (x - 0.35), -0.65),
                    Vec2::new(sign * (x + 0.55), 0.),
                    Vec2::new(sign * (x - 0.35), 0.65),
                ]);
            }
        }
        GizIcon::Text(_) => {}
    }
}
