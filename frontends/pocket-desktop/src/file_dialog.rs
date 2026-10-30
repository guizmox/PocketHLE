//! Launcher-owned file dialogs. No native window steals controller focus.
use crate::controller_ui as pad;
use eframe::egui::{self, Key};
use std::{
    path::{Component, Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Import,
    RestoreBackup,
    SaveBackup,
}
impl Purpose {
    fn title(self) -> &'static str {
        match self {
            Self::Import => "Import a game",
            Self::RestoreBackup => "Open PocketHLE backup",
            Self::SaveBackup => "Save PocketHLE backup",
        }
    }
    fn accepts(self, path: &Path) -> bool {
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let extensions: &[&str] = match self {
            Self::Import => &["rar", "cab", "zip", "exe", "dll"],
            _ => &["zip"],
        };
        extensions.iter().any(|e| extension.eq_ignore_ascii_case(e))
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Selected(Purpose, PathBuf),
    Cancelled(Purpose),
}
#[derive(Debug)]
struct Entry {
    path: PathBuf,
    name: String,
    directory: bool,
    hidden: bool,
    size: u64,
}
struct Listing {
    generation: u64,
    result: Result<Vec<Entry>, String>,
}

pub struct FileDialog {
    purpose: Purpose,
    directory: PathBuf,
    path_text: String,
    filename: String,
    search: String,
    all_files: bool,
    show_hidden: bool,
    new_folder: Option<String>,
    overwrite: Option<PathBuf>,
    entries: Vec<Entry>,
    error: Option<String>,
    loading: bool,
    focus_entries: bool,
    generation: u64,
    requests: Sender<(u64, PathBuf)>,
    results: Receiver<Listing>,
    finished: Option<Outcome>,
    selected: Option<PathBuf>,
    zones: [Vec<egui::Id>; 3],
    zone_memory: [Option<egui::Id>; 3],
    zone_jump: Option<ZoneJump>,
}

#[derive(Clone, Copy)]
enum ZoneJump {
    Next,
    Files,
}

fn track(zones: &mut [Vec<egui::Id>; 3], zone: usize, response: egui::Response) -> egui::Response {
    if response.enabled() {
        zones[zone].push(response.id);
    }
    response
}
fn file_kind(entry: &Entry) -> String {
    if entry.directory {
        return "Folder".into();
    }
    match entry
        .path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "zip" => "ZIP archive".into(),
        "cab" => "CAB archive".into(),
        "rar" => "RAR archive".into(),
        "exe" => "Application".into(),
        "dll" => "Library".into(),
        "" => "File".into(),
        other => format!("{} file", other.to_ascii_uppercase()),
    }
}
fn file_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{bytes} B")
    }
}
fn columns(width: f32) -> (f32, f32) {
    let size = if width >= 360.0 { 80.0 } else { 0.0 };
    let kind = if width >= 520.0 { 122.0 } else { 0.0 };
    (width - kind - size, width - size)
}
fn cell(painter: &egui::Painter, pos: egui::Pos2, width: f32, text: &str, color: egui::Color32) {
    if width <= 0.0 {
        return;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.into(),
        egui::FontId::proportional(13.0),
        color,
    );
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    painter.galley(pos, galley, color);
}
fn entry_icon(painter: &egui::Painter, pos: egui::Pos2, folder: bool, weak: egui::Color32) {
    if folder {
        let color = egui::Color32::from_rgb(205, 168, 84);
        painter.rect_filled(
            egui::Rect::from_min_size(pos + egui::vec2(0.0, 1.0), egui::vec2(8.0, 4.0)),
            1.0,
            color,
        );
        painter.rect_filled(
            egui::Rect::from_min_size(pos + egui::vec2(0.0, 4.0), egui::vec2(16.0, 11.0)),
            1.5,
            color,
        );
    } else {
        let rect = egui::Rect::from_min_size(pos + egui::vec2(2.0, 0.0), egui::vec2(12.0, 16.0));
        painter.rect_stroke(rect, 1.0, egui::Stroke::new(1.0_f32, weak));
        for y in [6.0, 9.0, 12.0] {
            painter.line_segment(
                [pos + egui::vec2(5.0, y), pos + egui::vec2(11.0, y)],
                egui::Stroke::new(1.0_f32, weak),
            );
        }
    }
}
struct FileRow<'a> {
    entry: &'a Entry,
    selected: bool,
}
impl egui::Widget for FileRow<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                self.selected,
                &self.entry.name,
            )
        });
        if ui.is_rect_visible(rect) {
            let selected = self.selected || response.has_focus();
            if selected {
                ui.painter().rect_filled(
                    rect,
                    2.0,
                    ui.visuals().selection.bg_fill.gamma_multiply(0.35),
                );
            } else if response.hovered() {
                ui.painter()
                    .rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            let text = ui.visuals().text_color();
            let weak = if selected {
                text
            } else {
                ui.visuals().weak_text_color()
            };
            entry_icon(
                ui.painter(),
                rect.left_top() + egui::vec2(10.0, 8.0),
                self.entry.directory,
                weak,
            );
            let (kind, size) = columns(rect.width());
            cell(
                ui.painter(),
                rect.left_top() + egui::vec2(34.0, 8.0),
                kind - 46.0,
                &self.entry.name,
                text,
            );
            if kind < size {
                cell(
                    ui.painter(),
                    rect.left_top() + egui::vec2(kind + 8.0, 8.0),
                    size - kind - 16.0,
                    &file_kind(self.entry),
                    weak,
                );
            }
            if size < rect.width() && !self.entry.directory {
                ui.painter().text(
                    rect.right_top() + egui::vec2(-10.0, 8.0),
                    egui::Align2::RIGHT_TOP,
                    file_size(self.entry.size),
                    egui::FontId::proportional(13.0),
                    weak,
                );
            }
        }
        response.on_hover_text(self.entry.path.display().to_string())
    }
}

fn home() -> PathBuf {
    directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}
fn places() -> Vec<(String, PathBuf)> {
    let mut result = vec![("Home".into(), home())];
    if let Some(d) = directories::UserDirs::new() {
        for (label, path) in [
            ("Downloads", d.download_dir()),
            ("Documents", d.document_dir()),
            ("Desktop", d.desktop_dir()),
        ] {
            if let Some(path) = path {
                result.push((label.into(), path.to_path_buf()));
            }
        }
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLogicalDrives() -> u32;
        }
        let drives = unsafe { GetLogicalDrives() };
        for index in 0..26 {
            if drives & (1 << index) != 0 {
                let root = format!("{}:\\", (b'A' + index) as char);
                result.push((root.clone(), PathBuf::from(root)));
            }
        }
    }
    #[cfg(not(windows))]
    result.extend([
        ("Filesystem /".into(), PathBuf::from("/")),
        ("Removable media".into(), PathBuf::from("/run/media")),
        ("Media".into(), PathBuf::from("/media")),
        ("Mounts".into(), PathBuf::from("/mnt")),
    ]);
    result
}
fn list_directory(path: &Path) -> Result<Vec<Entry>, String> {
    let reader =
        std::fs::read_dir(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let mut entries = Vec::new();
    for item in reader {
        let item = item.map_err(|e| format!("Cannot finish reading {}: {e}", path.display()))?;
        let name = item.file_name().to_string_lossy().into_owned();
        // metadata follows directory symlinks, including mounted SD-card aliases.
        let Ok(metadata) = std::fs::metadata(item.path()) else {
            continue;
        };
        if !metadata.is_dir() && !metadata.is_file() {
            continue;
        }
        #[allow(unused_mut)]
        let mut hidden = name.starts_with('.');
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            hidden |= metadata.file_attributes() & 2 != 0;
        }
        entries.push(Entry {
            path: item.path(),
            name,
            directory: metadata.is_dir(),
            hidden,
            size: metadata.len(),
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(entries)
}
fn valid_leaf(name: &str) -> bool {
    !name.trim().is_empty()
        && !name.contains(['/', '\\'])
        && matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        )
        && (!cfg!(windows) || !name.contains(['<', '>', ':', '"', '|', '?', '*']))
}
fn backup_filename(name: &str) -> Result<String, String> {
    if !valid_leaf(name) {
        return Err("Enter a filename without a folder path.".into());
    }
    if Path::new(name).extension().is_none() {
        return Ok(format!("{name}.zip"));
    }
    if !Purpose::SaveBackup.accepts(Path::new(name)) {
        return Err("A PocketHLE backup must use the .zip extension.".into());
    }
    Ok(name.to_owned())
}

impl FileDialog {
    pub fn new(purpose: Purpose, initial: Option<PathBuf>, filename: String) -> Self {
        let (requests, queue) = mpsc::channel::<(u64, PathBuf)>();
        let (completed, results) = mpsc::channel();
        // A single reader per dialog; collapse pending navigation requests.
        // Dropping the dialog closes the queue, even if a slow mount is finishing.
        std::thread::spawn(move || {
            while let Ok(mut request) = queue.recv() {
                while let Ok(next) = queue.try_recv() {
                    request = next;
                }
                let result = list_directory(&request.1);
                if completed
                    .send(Listing {
                        generation: request.0,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut dialog = Self {
            purpose,
            directory: PathBuf::new(),
            path_text: String::new(),
            filename,
            search: String::new(),
            all_files: false,
            show_hidden: false,
            new_folder: None,
            overwrite: None,
            entries: Vec::new(),
            error: None,
            loading: false,
            focus_entries: true,
            generation: 0,
            requests,
            results,
            finished: None,
            selected: None,
            zones: Default::default(),
            zone_memory: Default::default(),
            zone_jump: None,
        };
        dialog.navigate(initial.unwrap_or_else(home));
        dialog
    }
    fn navigate(&mut self, path: PathBuf) {
        self.directory = path;
        self.path_text = self.directory.to_string_lossy().into_owned();
        self.search.clear();
        self.selected = None;
        if self.purpose != Purpose::SaveBackup {
            self.filename.clear();
        }
        self.zone_memory = Default::default();
        self.zone_jump = None;
        self.overwrite = None;
        self.new_folder = None;
        self.error = None;
        self.entries.clear();
        self.loading = true;
        self.focus_entries = true;
        self.generation += 1;
        if self
            .requests
            .send((self.generation, self.directory.clone()))
            .is_err()
        {
            self.loading = false;
            self.error = Some("The directory reader stopped. Close and reopen this dialog.".into());
        }
    }
    fn poll(&mut self) {
        while let Ok(listing) = self.results.try_recv() {
            if listing.generation != self.generation {
                continue;
            }
            self.loading = false;
            match listing.result {
                Ok(entries) => {
                    self.entries = entries;
                    self.error = None;
                }
                Err(error) => {
                    self.entries.clear();
                    self.error = Some(error);
                }
            }
        }
    }
    /// SDL's left shoulder is named LeftTrigger in the launcher's existing map.
    pub fn controller(&mut self, control: &str) -> bool {
        match control {
            "RightTrigger" | "RightShoulder" | "LeftThumb" if self.overwrite.is_none() => {
                self.zone_jump = Some(ZoneJump::Next)
            }
            "West" if self.overwrite.is_none() => self.zone_jump = Some(ZoneJump::Files),
            "RightTrigger" | "RightShoulder" | "LeftThumb" | "West" => (),
            "East" => self.cancel(),
            "LeftTrigger" | "LeftShoulder" => self.parent(),
            "North" => self.refresh(),
            _ => return false,
        }
        true
    }
    pub fn cancel(&mut self) {
        if self.overwrite.take().is_some() {
            return;
        }
        if self.new_folder.take().is_some() {
            return;
        }
        self.finished = Some(Outcome::Cancelled(self.purpose));
    }
    pub fn parent(&mut self) {
        if self.overwrite.is_some() {
            return;
        }
        if let Some(parent) = self.directory.parent().map(Path::to_path_buf) {
            if !parent.as_os_str().is_empty() {
                self.navigate(parent);
            }
        }
    }
    pub fn refresh(&mut self) {
        if self.overwrite.is_none() {
            self.navigate(self.directory.clone());
        }
    }
    fn choose(&mut self, path: PathBuf) {
        if self.purpose == Purpose::SaveBackup {
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => {
                    self.error = Some("This name belongs to a folder.".into())
                }
                Ok(_) => self.overwrite = Some(path),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    self.finished = Some(Outcome::Selected(self.purpose, path));
                }
                Err(e) => self.error = Some(format!("Cannot access {}: {e}", path.display())),
            }
        } else if path.is_file() {
            self.finished = Some(Outcome::Selected(self.purpose, path));
        } else {
            self.error =
                Some("The selected file is no longer available. Refresh this folder.".into());
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, keyboard_open: bool) -> Option<Outcome> {
        self.poll();
        if !keyboard_open && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.cancel();
        }
        if self.finished.is_some() {
            return self.finished.take();
        }
        if self.loading {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if keyboard_open || self.new_folder.is_some() {
            self.focus_entries = false;
        }
        let mut focus_entry = None;
        let mut zones: [Vec<egui::Id>; 3] = Default::default();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 7.0);
            ui.spacing_mut().button_padding = egui::vec2(10.0, 5.0);
            ui.spacing_mut().interact_size.y = 29.0;
            if let Some(path) = self.overwrite.clone() {
                pad::page_scope(ui, "file_overwrite", |ui| {
                    ui.heading("Replace existing backup?");
                    ui.label(path.display().to_string());
                    ui.label("The existing file will be replaced when the backup is saved.");
                    if pad::button(ui, "Keep existing file / Back (B)").clicked() { self.overwrite = None; }
                    if pad::button(ui, "Replace file").clicked() { self.finished = Some(Outcome::Selected(self.purpose, path)); }
                });
                return;
            }
            pad::page_scope(ui, ("file_dialog", self.purpose as u8, self.generation), |ui| {
                ui.horizontal(|ui| { ui.heading(self.purpose.title()); ui.weak("PocketHLE"); });
                ui.horizontal(|ui| {
                    if track(&mut zones, 0, pad::add_enabled(ui, self.directory.parent().is_some(), egui::Button::new("Up (LB)"))).clicked() { self.parent(); }
                    if track(&mut zones, 0, pad::button(ui, "Refresh")).clicked() { self.refresh(); }
                    ui.spacing_mut().text_edit_width = (ui.available_width() - 175.0).max(60.0);
                    let response = track(&mut zones, 0, pad::text_edit(ui, &mut self.path_text, "Folder path"));
                    let enter = response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if track(&mut zones, 0, pad::button(ui, "Go")).clicked() || enter {
                        let path = PathBuf::from(&self.path_text);
                        self.navigate(if path.is_absolute() { path } else { self.directory.join(path) });
                    }
                    if track(&mut zones, 0, pad::button(ui, "New folder…")).clicked() { self.new_folder = Some(String::new()); }
                });
                ui.horizontal_wrapped(|ui| {
                    for (label, path) in places() {
                        if track(&mut zones, 0, pad::add_enabled(ui, true, egui::Button::new(label).frame(false))).clicked() { self.navigate(path); }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Search:");
                    ui.spacing_mut().text_edit_width = 210.0_f32.min((ui.available_width() - 200.0).max(100.0));
                    track(&mut zones, 0, pad::text_edit(ui, &mut self.search, "Search this folder"));
                    track(&mut zones, 0, pad::checkbox(ui, &mut self.show_hidden, "Hidden files"));
                    track(&mut zones, 0, pad::checkbox(ui, &mut self.all_files, "All file types"));
                });
                if let Some(error) = &self.error { ui.colored_label(egui::Color32::LIGHT_RED, error); }
                if self.loading { ui.horizontal(|ui| { ui.spinner(); ui.label("Reading folder…"); }); }
                let search = self.search.to_lowercase();
                let mut picked = None;
                let mut count = 0;
                let footer = 87.0 + if self.new_folder.is_some() { 38.0 } else { 0.0 };
                let height = (ui.available_height() - footer).max(80.0);
                egui::Frame::none().fill(ui.visuals().extreme_bg_color)
                    .stroke(ui.visuals().widgets.noninteractive.bg_stroke).rounding(3.0).show(ui, |ui| {
                    let (header, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 27.0), egui::Sense::hover());
                    let weak = ui.visuals().weak_text_color();
                    let content_width = header.width() - ui.spacing().scroll.allocated_width();
                    let (kind, size) = columns(content_width);
                    cell(ui.painter(), header.left_top() + egui::vec2(10.0, 6.0), kind - 20.0, "Name", weak);
                    if kind < size { cell(ui.painter(), header.left_top() + egui::vec2(kind + 8.0, 6.0), size - kind - 16.0, "Type", weak); }
                    if size < content_width { ui.painter().text(header.left_top() + egui::vec2(content_width - 10.0, 6.0), egui::Align2::RIGHT_TOP, "Size", egui::FontId::proportional(13.0), weak); }
                    ui.painter().hline(header.x_range(), header.bottom(), ui.visuals().widgets.noninteractive.bg_stroke);
                    // Always reserve the scrollbar gutter so header and row columns align.
                    egui::ScrollArea::vertical().id_source("file_entries").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .max_height((height - 34.0).max(40.0)).auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for entry in &self.entries {
                            if (!self.show_hidden && entry.hidden) || !entry.name.to_lowercase().contains(&search)
                                || (!entry.directory && !self.all_files && !self.purpose.accepts(&entry.path)) { continue; }
                            count += 1;
                            ui.push_id(&entry.path, |ui| {
                                let response = track(&mut zones, 1, pad::add_enabled(ui, true, FileRow { entry, selected: self.selected.as_ref() == Some(&entry.path) }));
                                if self.focus_entries { focus_entry = Some(response.id); self.focus_entries = false; }
                                if response.has_focus() && self.zone_memory[1] != Some(response.id) {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                }
                                if response.clicked() || response.has_focus() {
                                    self.selected = Some(entry.path.clone());
                                    if !entry.directory { self.filename = entry.name.clone(); }
                                    else if self.purpose != Purpose::SaveBackup { self.filename.clear(); }
                                }
                                let open = response.double_clicked() || (response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)));
                                if open { picked = Some((entry.path.clone(), entry.directory)); }
                            });
                        }
                        if count == 0 && !self.loading { self.focus_entries = false; ui.label("No matching files or folders."); }
                        pad::scroll_focus(ui);
                    });
                });
                if let Some((path, directory)) = picked {
                    if directory { self.navigate(path); }
                    else if self.purpose == Purpose::SaveBackup { self.filename = path.file_name().unwrap().to_string_lossy().into_owned(); }
                    else { self.choose(path); }
                }
                if let Some(mut name) = self.new_folder.take() {
                    let mut keep_form = true;
                    ui.horizontal(|ui| {
                        ui.label("New folder:"); track(&mut zones, 2, pad::text_edit(ui, &mut name, "Folder name"));
                        if track(&mut zones, 2, pad::button(ui, "Create folder")).clicked() {
                            if !valid_leaf(&name) { self.error = Some("Enter a folder name without a path.".into()); }
                            else { let path = self.directory.join(&name); match std::fs::create_dir(&path) {
                                Ok(()) => { self.navigate(path); keep_form = false; }
                                Err(e) => self.error = Some(format!("Cannot create folder: {e}")),
                            }}
                        }
                        if track(&mut zones, 2, pad::button(ui, "Back")).clicked() { keep_form = false; }
                    });
                    if keep_form { self.new_folder = Some(name); }
                }
                ui.horizontal(|ui| {
                    ui.label("File name:");
                    ui.spacing_mut().text_edit_width = (ui.available_width() - 185.0).max(80.0);
                    track(&mut zones, 2, pad::text_edit(ui, &mut self.filename, "Select a file"));
                    if track(&mut zones, 2, pad::button(ui, "Cancel")).clicked() { self.finished = Some(Outcome::Cancelled(self.purpose)); }
                    let label = match self.purpose { Purpose::SaveBackup => "Save backup", Purpose::RestoreBackup => "Open backup", Purpose::Import => "Import" };
                    if track(&mut zones, 2, pad::add_enabled(ui, !self.loading && (!self.filename.is_empty() || self.selected.is_some()), egui::Button::new(label))).clicked() {
                        if self.purpose == Purpose::SaveBackup {
                            match backup_filename(&self.filename) { Ok(name) => self.choose(self.directory.join(name)), Err(error) => self.error = Some(error) }
                        } else if self.selected.as_ref().is_some_and(|path| path.is_dir()) && self.filename.is_empty() {
                            self.navigate(self.selected.clone().unwrap());
                        } else {
                            let path = self.selected.as_ref().filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy() == self.filename))
                                .cloned().unwrap_or_else(|| self.directory.join(&self.filename));
                            self.choose(path);
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.weak(format!("{count} items"));
                    ui.separator();
                    ui.label(egui::RichText::new("A Open / keyboard  ·  B Cancel  ·  LB Up  ·  RB Zones  ·  X Files  ·  Y Refresh").size(11.0).weak());
                });
            });
        });
        if let Some(id) = focus_entry {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if !keyboard_open && self.overwrite.is_none() {
            let focused = ctx.memory(|m| m.focused());
            for (index, ids) in zones.iter().enumerate() {
                if let Some(id) = focused.filter(|id| ids.contains(id)) {
                    self.zone_memory[index] = Some(id);
                }
            }
            if let Some(jump) = self.zone_jump.take() {
                let current = zones
                    .iter()
                    .position(|ids| focused.is_some_and(|id| ids.contains(&id)))
                    .unwrap_or(0);
                let target = match jump {
                    ZoneJump::Files => 1,
                    ZoneJump::Next => (1..=3)
                        .map(|step| (current + step) % 3)
                        .find(|index| !zones[*index].is_empty())
                        .unwrap_or(current),
                };
                if let Some(id) = self.zone_memory[target]
                    .filter(|id| zones[target].contains(id))
                    .or_else(|| zones[target].first().copied())
                {
                    ctx.memory_mut(|m| m.request_focus(id));
                    if let Some(response) = ctx.read_response(id) {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    ctx.request_repaint();
                }
            }
            self.zones = zones;
        }
        self.finished.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn temp_dir() -> std::io::Result<TempDir> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "pockethle-dialog-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(TempDir(path))
    }
    fn loaded(dialog: &mut FileDialog) {
        let end = std::time::Instant::now() + Duration::from_secs(3);
        while dialog.loading && std::time::Instant::now() < end {
            dialog.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!dialog.loading);
    }
    #[test]
    fn listing_sorts_folders_first_and_filters_case_insensitively() {
        let dir = temp_dir().unwrap();
        std::fs::write(dir.path().join("Game.ZIP"), b"zip").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"text").unwrap();
        std::fs::write(dir.path().join(".hidden.cab"), b"cab").unwrap();
        std::fs::create_dir(dir.path().join("Folder")).unwrap();
        let entries = list_directory(dir.path()).unwrap();
        assert_eq!(entries[0].name, "Folder");
        assert!(entries[0].directory);
        assert!(
            entries
                .iter()
                .find(|e| e.name == ".hidden.cab")
                .unwrap()
                .hidden
        );
        assert!(Purpose::Import.accepts(&dir.path().join("Game.ZIP")));
        assert!(!Purpose::RestoreBackup.accepts(Path::new("game.cab")));
        assert!(!Purpose::Import.accepts(Path::new("notes.txt")));
    }
    #[test]
    fn backup_names_cannot_escape_folder_and_zip_suffix_is_not_duplicated() {
        for name in ["", " ", ".", "..", "../backup", "sub/backup", "sub\\backup"] {
            assert!(backup_filename(name).is_err(), "{name:?}");
        }
        assert_eq!(backup_filename("backup").unwrap(), "backup.zip");
        assert_eq!(backup_filename("sauvegarde.ZIP").unwrap(), "sauvegarde.ZIP");
        assert!(backup_filename("backup.txt").is_err());
    }
    #[test]
    fn overwrite_requires_confirmation_and_cancel_never_writes() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("backup.zip");
        std::fs::write(&path, b"original").unwrap();
        let mut dialog = FileDialog::new(
            Purpose::SaveBackup,
            Some(dir.path().into()),
            "backup.zip".into(),
        );
        loaded(&mut dialog);
        dialog.choose(path.clone());
        assert_eq!(dialog.overwrite.as_ref(), Some(&path));
        assert_eq!(dialog.finished, None);
        dialog.cancel();
        assert!(dialog.overwrite.is_none());
        assert_eq!(dialog.finished, None); // B returns to the filename screen.
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        dialog.cancel();
        assert_eq!(
            dialog.finished,
            Some(Outcome::Cancelled(Purpose::SaveBackup))
        );
    }
    #[test]
    fn navigation_discards_stale_results_and_unreadable_folder_remains_recoverable() {
        let dir = temp_dir().unwrap();
        let child = dir.path().join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(child.join("game.cab"), b"cab").unwrap();
        let mut dialog = FileDialog::new(Purpose::Import, Some(dir.path().into()), String::new());
        dialog.navigate(child.clone());
        loaded(&mut dialog);
        assert_eq!(dialog.entries.len(), 1);
        assert_eq!(dialog.entries[0].name, "game.cab");
        dialog.navigate(child.join("missing"));
        loaded(&mut dialog);
        assert!(dialog.error.is_some());
        assert!(dialog.entries.is_empty());
        assert!(dialog.controller("LeftTrigger"));
        loaded(&mut dialog);
        assert_eq!(dialog.directory, child);
        assert_eq!(dialog.entries[0].name, "game.cab");
    }
    fn frame(
        ctx: &egui::Context,
        pad: &mut pad::Navigation,
        dialog: &mut FileDialog,
    ) -> (Option<Outcome>, String) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        pad.prepare_input(ctx, &mut raw, true);
        let mut outcome = None;
        let output = ctx.run(raw, |ctx| {
            pad::begin_frame(ctx);
            outcome = dialog.show(ctx, pad.keyboard_open());
            pad.show(ctx);
        });
        let rect = ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id))
            .map(|r| r.rect);
        fn texts(shape: &egui::epaint::Shape, rect: egui::Rect, out: &mut String) {
            match shape {
                egui::epaint::Shape::Text(text) if rect.contains(text.pos) => {
                    out.push_str(&text.galley.job.text)
                }
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        texts(shape, rect, out);
                    }
                }
                _ => (),
            }
        }
        let mut label = String::new();
        if let Some(rect) = rect {
            for shape in output.shapes {
                texts(&shape.shape, rect, &mut label);
            }
        }
        (outcome, label)
    }
    #[test]
    fn controller_can_focus_and_open_a_file_without_mouse() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("game.CAB");
        std::fs::write(&path, b"cab").unwrap();
        let mut dialog = FileDialog::new(Purpose::Import, Some(dir.path().into()), String::new());
        loaded(&mut dialog);
        let ctx = egui::Context::default();
        let mut pad = pad::Navigation::default();
        let mut found = false;
        for _ in 0..40 {
            let (_, label) = frame(&ctx, &mut pad, &mut dialog);
            if label.contains("game.CAB") {
                found = true;
                break;
            }
            pad.press("DPadDown");
        }
        assert!(found, "file row must be in the controller focus scope");
        pad.press("South");
        assert_eq!(
            frame(&ctx, &mut pad, &mut dialog).0,
            Some(Outcome::Selected(Purpose::Import, path))
        );
    }
    #[test]
    fn controller_opens_filename_keyboard_and_b_keeps_save_dialog_open() {
        let dir = temp_dir().unwrap();
        let mut dialog = FileDialog::new(
            Purpose::SaveBackup,
            Some(dir.path().into()),
            "my-backup.zip".into(),
        );
        loaded(&mut dialog);
        let ctx = egui::Context::default();
        let mut pad = pad::Navigation::default();
        let mut found = false;
        for _ in 0..40 {
            let (_, label) = frame(&ctx, &mut pad, &mut dialog);
            if label.contains("my-backup.zip") {
                found = true;
                break;
            }
            pad.press("DPadDown");
        }
        assert!(found);
        pad.press("South");
        frame(&ctx, &mut pad, &mut dialog);
        assert!(pad.keyboard_open());
        pad.press("East");
        assert_eq!(frame(&ctx, &mut pad, &mut dialog).0, None);
        assert!(!pad.keyboard_open());
        assert_eq!(dialog.filename, "my-backup.zip");
    }
    #[test]
    fn empty_new_folder_editor_survives_frames_and_cancel_stays_in_dialog() {
        let dir = temp_dir().unwrap();
        let mut dialog = FileDialog::new(
            Purpose::SaveBackup,
            Some(dir.path().into()),
            "backup.zip".into(),
        );
        loaded(&mut dialog);
        dialog.new_folder = Some(String::new());
        let ctx = egui::Context::default();
        let mut pad = pad::Navigation::default();
        frame(&ctx, &mut pad, &mut dialog);
        frame(&ctx, &mut pad, &mut dialog);
        assert_eq!(dialog.new_folder, Some(String::new()));
        assert!(dialog.controller("East"));
        assert_eq!(dialog.new_folder, None);
        assert_eq!(dialog.finished, None);
    }
    #[cfg(unix)]
    #[test]
    fn directory_symlinks_can_be_browsed() {
        let dir = temp_dir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let entries = list_directory(dir.path()).unwrap();
        assert!(entries.iter().find(|e| e.path == alias).unwrap().directory);
    }

    #[test]
    fn opening_a_folder_focuses_its_contents_for_the_next_a_press() {
        let dir = temp_dir().unwrap();
        let child = dir.path().join("Games");
        std::fs::create_dir(&child).unwrap();
        let file = child.join("game.zip");
        std::fs::write(&file, b"zip").unwrap();
        let mut dialog = FileDialog::new(Purpose::Import, Some(dir.path().into()), String::new());
        loaded(&mut dialog);
        let ctx = egui::Context::default();
        let mut pad = pad::Navigation::default();
        let (_, label) = frame(&ctx, &mut pad, &mut dialog);
        assert!(label.contains("Games"));
        pad.press("South");
        frame(&ctx, &mut pad, &mut dialog);
        loaded(&mut dialog);
        let (_, label) = frame(&ctx, &mut pad, &mut dialog);
        assert!(label.contains("game.zip"));
        pad.press("South");
        assert_eq!(
            frame(&ctx, &mut pad, &mut dialog).0,
            Some(Outcome::Selected(Purpose::Import, file))
        );
    }
    #[test]
    fn shoulder_cycles_zones_and_x_returns_to_last_file() {
        let dir = temp_dir().unwrap();
        for name in ["a.cab", "b.zip"] {
            std::fs::write(dir.path().join(name), b"game").unwrap();
        }
        let mut dialog = FileDialog::new(Purpose::Import, Some(dir.path().into()), String::new());
        loaded(&mut dialog);
        let ctx = egui::Context::default();
        let mut pad = pad::Navigation::default();
        frame(&ctx, &mut pad, &mut dialog);
        pad.press("DPadDown");
        frame(&ctx, &mut pad, &mut dialog);
        let file = ctx.memory(|m| m.focused()).unwrap();
        assert!(dialog.zones[1].contains(&file));
        assert_eq!(dialog.filename, "b.zip");
        for zone in [2, 0, 1] {
            assert!(dialog.controller("RightTrigger"));
            frame(&ctx, &mut pad, &mut dialog);
            assert!(dialog.zones[zone].contains(&ctx.memory(|m| m.focused()).unwrap()));
        }
        assert_eq!(ctx.memory(|m| m.focused()), Some(file));
        dialog.controller("RightTrigger");
        frame(&ctx, &mut pad, &mut dialog);
        dialog.controller("West");
        frame(&ctx, &mut pad, &mut dialog);
        assert_eq!(ctx.memory(|m| m.focused()), Some(file));
    }
    #[test]
    fn entering_save_folder_preserves_backup_name() {
        let dir = temp_dir().unwrap();
        let child = dir.path().join("Backups");
        std::fs::create_dir(&child).unwrap();
        let mut dialog = FileDialog::new(
            Purpose::SaveBackup,
            Some(dir.path().into()),
            "backup.zip".into(),
        );
        loaded(&mut dialog);
        let ctx = egui::Context::default();
        let mut pad = pad::Navigation::default();
        frame(&ctx, &mut pad, &mut dialog);
        pad.press("South");
        frame(&ctx, &mut pad, &mut dialog);
        loaded(&mut dialog);
        assert_eq!(dialog.directory, child);
        assert_eq!(dialog.filename, "backup.zip");
    }
    #[test]
    fn empty_file_zone_does_not_trap_focus_or_close_overwrite_confirmation() {
        let dir = temp_dir().unwrap();
        let mut dialog = FileDialog::new(
            Purpose::SaveBackup,
            Some(dir.path().into()),
            "backup.zip".into(),
        );
        loaded(&mut dialog);
        let ctx = egui::Context::default();
        let mut pad = pad::Navigation::default();
        frame(&ctx, &mut pad, &mut dialog);
        let before = ctx.memory(|m| m.focused());
        dialog.controller("West");
        frame(&ctx, &mut pad, &mut dialog);
        assert_eq!(ctx.memory(|m| m.focused()), before);
        dialog.controller("RightTrigger");
        frame(&ctx, &mut pad, &mut dialog);
        assert!(dialog.zones[2].contains(&ctx.memory(|m| m.focused()).unwrap()));
        let path = dir.path().join("backup.zip");
        std::fs::write(&path, b"original").unwrap();
        dialog.choose(path.clone());
        dialog.controller("RightTrigger");
        dialog.controller("West");
        assert!(dialog.zone_jump.is_none());
        assert_eq!(dialog.overwrite, Some(path));
    }
    #[test]
    fn file_row_name_starts_at_the_left_icon_and_column_widths_remain_positive() {
        let entry = Entry {
            path: PathBuf::from("long-game-name.cab"),
            name: "long-game-name.cab".into(),
            directory: false,
            hidden: false,
            size: 1_048_576,
        };
        let ctx = egui::Context::default();
        let mut rect = egui::Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 500.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    rect = ui
                        .add(FileRow {
                            entry: &entry,
                            selected: false,
                        })
                        .rect;
                });
            },
        );
        fn find(shape: &egui::epaint::Shape) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(t) if t.galley.job.text == "long-game-name.cab" => {
                    Some(t.pos)
                }
                egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(find),
                _ => None,
            }
        }
        let pos = output.shapes.iter().find_map(|s| find(&s.shape)).unwrap();
        assert!((pos.x - rect.left() - 34.0).abs() < 0.1);
        assert!(pos.x < rect.center().x);
        for width in [200.0, 360.0, 520.0, 940.0] {
            let (kind, size) = columns(width);
            assert!(kind > 46.0 && kind <= size && size <= width);
        }
        assert_eq!(file_size(1_048_576), "1.0 MB");
        assert_eq!(file_kind(&entry), "CAB archive");
    }
}
