//! Asynchronous capture catalogue. Media decoding and shell launch stay off the UI thread.
use eframe::egui::{self, ColorImage, TextureHandle, Vec2};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    modified: u128,
    size: u64,
}
struct Asset {
    key: Key,
    video: bool,
    image: Option<ColorImage>,
    error: Option<String>,
}
struct Entry {
    asset: Asset,
    texture: Option<TextureHandle>,
}
#[derive(Default)]
pub struct Gallery {
    share: crate::native_share::NativeShare,
    entries: Vec<Entry>,
    pending: Option<mpsc::Receiver<Result<Vec<Asset>, String>>>,
    opened: Option<mpsc::Receiver<Result<(), String>>>,
    refreshed: Option<Instant>,
    selected: usize,
    columns: usize,
    scroll: bool,
    error: Option<String>,
    delete: Option<PathBuf>,
    delete_confirm: bool,
}
fn poll_worker<T>(worker: &Option<mpsc::Receiver<Result<T, String>>>) -> Option<Result<T, String>> {
    match worker.as_ref()?.try_recv() {
        Ok(result) => Some(result),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => {
            Some(Err("Media worker stopped unexpectedly".into()))
        }
    }
}
fn media_kind(path: &Path) -> Option<bool> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" => Some(false),
        "avi" | "mp4" => Some(true),
        _ => None,
    }
}
fn scan(root: &Path, known: &HashSet<Key>) -> Result<Vec<Asset>, String> {
    let mut entries = Vec::new();
    for dir in ["screenshots", "recordings"] {
        let paths = match std::fs::read_dir(root.join(dir)) {
            Ok(paths) => paths,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        for path in paths {
            let path = path.map_err(|e| e.to_string())?.path();
            let Some(video) = media_kind(&path) else {
                continue;
            };
            let Ok(meta) = path.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            entries.push(Asset {
                key: Key {
                    path,
                    modified,
                    size: meta.len(),
                },
                video,
                image: None,
                error: None,
            });
        }
    }
    entries.sort_by(|a, b| {
        b.key
            .modified
            .cmp(&a.key.modified)
            .then_with(|| a.key.path.cmp(&b.key.path))
    });
    for entry in &mut entries {
        if known.contains(&entry.key) {
            continue;
        }
        let image = if entry.video {
            pocket_core::winceapi::recording_encoder::video_thumbnail(&entry.key.path, 320, 180)
                .map(|rgb| ColorImage::from_rgb([320, 180], &rgb))
                .map_err(|e| e.to_string())
        } else {
            image::ImageReader::open(&entry.key.path)
                .map_err(|e| e.to_string())
                .and_then(|reader| {
                    reader.decode().map_err(|e| e.to_string()).map(|im| {
                        let im = im.thumbnail(320, 180).into_rgba8();
                        ColorImage::from_rgba_unmultiplied(
                            [im.width() as usize, im.height() as usize],
                            im.as_raw(),
                        )
                    })
                })
        };
        match image {
            Ok(image) => entry.image = Some(image),
            Err(error) => entry.error = Some(error),
        }
    }
    Ok(entries)
}
#[cfg(windows)]
fn open_capture(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            window: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result > 32 {
        Ok(())
    } else {
        Err(format!("Cannot open capture (Windows error {result})"))
    }
}
#[cfg(not(windows))]
fn open_capture(path: &Path) -> Result<(), String> {
    let tool = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let status = std::process::Command::new(tool)
        .arg(path)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Cannot open capture ({status})"))
    }
}
impl Gallery {
    fn refresh(&mut self, root: &Path, ctx: &egui::Context, force: bool) {
        if self.pending.is_some() {
            return;
        }
        let known = if force {
            HashSet::new()
        } else {
            self.entries.iter().map(|e| e.asset.key.clone()).collect()
        };
        let root = root.to_owned();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.refreshed = Some(Instant::now());
        std::thread::spawn(move || {
            let _ = tx.send(scan(&root, &known));
            ctx.request_repaint();
        });
    }
    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        if self.opened.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.opened = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(open_capture(&path));
            ctx.request_repaint();
        });
    }
    fn delete_capture(&mut self) {
        let Some(path) = self.delete.take() else {
            return;
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.entries.retain(|e| e.asset.key.path != path);
                self.selected = self.selected.min(self.entries.len().saturating_sub(1));
                self.pending = None;
                self.refreshed = None;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Could not delete capture: {error}")),
        }
    }
    fn show_delete_confirmation(&mut self, ctx: &egui::Context) {
        let Some(path) = self.delete.clone() else {
            return;
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("Delete capture?")
            .id(egui::Id::new("delete_capture_confirmation"))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                ui.label("Permanently delete this file?");
                ui.horizontal(|ui| {
                    cancel = ui
                        .selectable_label(!self.delete_confirm, "Cancel")
                        .clicked();
                    confirm = ui.selectable_label(self.delete_confirm, "Delete").clicked();
                });
            });
        if confirm {
            self.delete_capture();
        } else if cancel || !open {
            self.delete = None;
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, root: &Path, mut actions: Vec<String>) {
        if self.delete.is_some() {
            for action in std::mem::take(&mut actions) {
                match action.as_str() {
                    "East" => self.delete = None,
                    "DPadLeft" | "DPadRight" | "LeftStickLeft" | "LeftStickRight" => {
                        self.delete_confirm = !self.delete_confirm
                    }
                    "South" => {
                        if self.delete_confirm {
                            self.delete_capture();
                        } else {
                            self.delete = None;
                        }
                    }
                    _ => {}
                }
            }
        }
        self.show_delete_confirmation(ui.ctx());
        if self.delete.is_some() {
            ui.set_enabled(false);
        }
        if let Some(result) = poll_worker(&self.pending) {
            self.pending = None;
            match result {
                Ok(assets) => {
                    let selected = self
                        .entries
                        .get(self.selected)
                        .map(|e| e.asset.key.path.clone());
                    let mut old = std::mem::take(&mut self.entries);
                    self.entries = assets
                        .into_iter()
                        .map(|mut asset| {
                            let previous = old.iter_mut().find(|e| e.asset.key == asset.key);
                            let texture = if let Some(image) = asset.image.take() {
                                Some(ui.ctx().load_texture(
                                    asset.key.path.to_string_lossy(),
                                    image,
                                    egui::TextureOptions::LINEAR,
                                ))
                            } else {
                                previous.and_then(|p| {
                                    asset.error = p.asset.error.clone();
                                    p.texture.take()
                                })
                            };
                            Entry { asset, texture }
                        })
                        .collect();
                    self.selected = selected
                        .and_then(|path| self.entries.iter().position(|e| e.asset.key.path == path))
                        .unwrap_or(self.selected)
                        .min(self.entries.len().saturating_sub(1));
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(result) = poll_worker(&self.opened) {
            self.opened = None;
            if let Err(e) = result {
                self.error = Some(e);
            }
        }
        let mut force = false;
        ui.horizontal(|ui| {
            ui.heading("Recordings");
            ui.label(format!("{} captures", self.entries.len()));
            force = ui.button("Refresh").clicked();
            if self.pending.is_some() {
                ui.spinner();
            }
        });
        if force
            || self
                .refreshed
                .is_none_or(|t| t.elapsed() > Duration::from_secs(3))
        {
            self.refresh(root, ui.ctx(), force);
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        if self.entries.is_empty() {
            ui.add_space(30.);
            ui.label("Your in-game screenshots and videos will appear here.");
            return;
        }
        let mut open = None;
        let mut share = None;
        let mut delete = None;
        let native_sharing = self.share.available();
        for action in actions {
            if action == "South" {
                open = self
                    .entries
                    .get(self.selected)
                    .map(|e| e.asset.key.path.clone());
            } else if action == "RightThumb" {
                delete = self
                    .entries
                    .get(self.selected)
                    .map(|e| e.asset.key.path.clone());
            } else if action == "North" && native_sharing {
                share = self
                    .entries
                    .get(self.selected)
                    .map(|e| e.asset.key.path.clone());
            } else if let Some(next) = crate::deck_navigation::next_index(
                self.selected,
                self.entries.len(),
                self.columns,
                &action,
            ) {
                self.scroll |= next != self.selected;
                self.selected = next;
            }
        }
        let size = Vec2::new(220., 176.);
        let gap = 12.;
        self.columns = ((ui.available_width() + gap) / (size.x + gap))
            .floor()
            .max(1.) as usize;
        egui::ScrollArea::vertical()
            .id_source("recordings_scroll")
            .show(ui, |ui| {
                egui::Grid::new("recordings_grid")
                    .num_columns(self.columns)
                    .spacing(Vec2::splat(gap))
                    .show(ui, |ui| {
                        for (index, entry) in self.entries.iter().enumerate() {
                            let (rect, response) =
                                ui.allocate_exact_size(size, egui::Sense::click());
                            if index == self.selected && self.scroll {
                                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                                self.scroll = false;
                            }
                            ui.painter().rect(
                                rect,
                                8.,
                                egui::Color32::from_rgb(28, 29, 32),
                                egui::Stroke::new(
                                    if index == self.selected { 2. } else { 1. },
                                    if index == self.selected {
                                        egui::Color32::LIGHT_BLUE
                                    } else {
                                        egui::Color32::GRAY
                                    },
                                ),
                            );
                            let view = egui::Rect::from_min_size(
                                rect.min + Vec2::splat(6.),
                                Vec2::new(size.x - 12., 120.),
                            );
                            if let Some(texture) = &entry.texture {
                                let d = texture.size_vec2();
                                let scale = (view.width() / d.x).min(view.height() / d.y);
                                ui.painter().image(
                                    texture.id(),
                                    egui::Rect::from_center_size(view.center(), d * scale),
                                    egui::Rect::from_min_max(
                                        egui::Pos2::ZERO,
                                        egui::Pos2::new(1., 1.),
                                    ),
                                    egui::Color32::WHITE,
                                );
                            } else {
                                ui.painter().text(
                                    view.center(),
                                    egui::Align2::CENTER_CENTER,
                                    if entry.asset.video { "VIDEO" } else { "IMAGE" },
                                    egui::FontId::proportional(20.),
                                    egui::Color32::GRAY,
                                );
                            }
                            let label = entry
                                .asset
                                .key
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            let mut child = ui.child_ui(
                                egui::Rect::from_min_max(
                                    egui::pos2(rect.left() + 8., rect.top() + 132.),
                                    rect.max - Vec2::splat(6.),
                                ),
                                egui::Layout::top_down(egui::Align::Min),
                            );
                            child.add(egui::Label::new(label.as_ref()).truncate(true));
                            child.horizontal(|ui| {
                                ui.small(if entry.asset.video {
                                    "▶ VIDEO"
                                } else {
                                    "SCREENSHOT"
                                });
                                if native_sharing && ui.small_button("Share").clicked() {
                                    share = Some(entry.asset.key.path.clone());
                                }
                                if ui.small_button("Delete").clicked() {
                                    delete = Some(entry.asset.key.path.clone());
                                }
                            });
                            let response = response.on_hover_text(
                                entry
                                    .asset
                                    .error
                                    .as_deref()
                                    .unwrap_or("Open with your default viewer"),
                            );
                            if response.clicked() {
                                self.selected = index;
                                open = Some(entry.asset.key.path.clone());
                            }
                            if (index + 1) % self.columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
        if let Some(path) = delete {
            self.delete = Some(path);
            self.delete_confirm = false;
            ui.ctx().request_repaint();
        } else if let Some(path) = share {
            if let Err(error) = self.share.share(&path) {
                self.error = Some(error);
            }
        } else if let Some(path) = open {
            self.open(path, ui.ctx());
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_media_only() {
        assert_eq!(media_kind(Path::new("game.PNG")), Some(false));
        assert_eq!(media_kind(Path::new("game.MP4")), Some(true));
        assert_eq!(media_kind(Path::new("game.avi")), Some(true));
        assert_eq!(media_kind(Path::new("game.exe")), None);
        assert_eq!(media_kind(Path::new("game.tmp")), None);
    }
    #[test]
    fn catalogue_reads_both_folders_and_reuses_unchanged_thumbnails() {
        let root = std::env::temp_dir().join(format!(
            "pockethle-gallery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("screenshots")).unwrap();
        std::fs::create_dir_all(root.join("recordings")).unwrap();
        image::RgbImage::from_pixel(8, 6, image::Rgb([255, 0, 0]))
            .save(root.join("screenshots/frame.PNG"))
            .unwrap();
        std::fs::write(root.join("recordings/clip.mp4"), b"incomplete capture").unwrap();
        std::fs::write(root.join("recordings/ignore.txt"), b"ignore").unwrap();
        let first = scan(&root, &HashSet::new()).unwrap();
        assert_eq!(first.len(), 2);
        let screenshot = first.iter().find(|a| !a.video).unwrap();
        assert_eq!(screenshot.image.as_ref().unwrap().size, [240, 180]);
        assert_eq!(
            screenshot.image.as_ref().unwrap().pixels[0],
            egui::Color32::RED
        );
        assert!(first.iter().find(|a| a.video).unwrap().error.is_some());
        let known = first.iter().map(|a| a.key.clone()).collect();
        let cached = scan(&root, &known).unwrap();
        assert!(cached
            .iter()
            .all(|a| a.image.is_none() && a.error.is_none()));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn deleting_a_capture_preserves_other_files_and_clears_cached_selection() {
        let root = std::env::temp_dir().join(format!("pockethle-delete-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let capture = root.join("capture.mp4");
        let other = root.join("other.png");
        std::fs::write(&capture, b"capture").unwrap();
        std::fs::write(&other, b"keep").unwrap();
        let mut gallery = Gallery {
            delete: Some(capture.clone()),
            ..Default::default()
        };
        assert!(capture.exists());
        assert!(!gallery.delete_confirm);
        gallery.delete_capture();
        assert!(!capture.exists());
        assert_eq!(std::fs::read(&other).unwrap(), b"keep");
        assert!(gallery.delete.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
