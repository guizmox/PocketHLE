//! Controller-navigable choices for staged game data; rendering never commits files.
use eframe::egui::{self, Vec2};
use pocket_library::add_data::PreparedData;
use std::path::PathBuf;

pub(super) enum DataPrompt {
    Zip {
        id: String,
        path: PathBuf,
    },
    Executable(PreparedData),
    Overwrite(PreparedData),
    Notice {
        title: &'static str,
        message: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DataChoice {
    Cancel,
    Zip(bool),
    Executable(Option<String>),
    Overwrite,
}

impl DataPrompt {
    pub(super) fn show(&self, ctx: &egui::Context) -> Option<DataChoice> {
        let mut choice = None;
        let mut executable_choice: Option<Option<String>> = None;
        let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let title = match self {
            DataPrompt::Zip { .. } => "Add game data — ZIP archive",
            DataPrompt::Executable(_) => "Choose game executable",
            DataPrompt::Overwrite(_) => "Replace existing data?",
            DataPrompt::Notice { title, .. } => title,
        };
        egui::CentralPanel::default().show(ctx, |_| {});
        egui::Window::new(title)
            .collapsible(false).resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(460.0)
            .max_width((ctx.screen_rect().width() - 32.0).max(100.0))
            .show(ctx, |ui| {
                let mut contents = |ui: &mut egui::Ui| {
                    match self {
                        DataPrompt::Zip { .. } => {
                            ui.label("Extract the ZIP contents into the game folder, or copy the ZIP file unchanged?");
                            if crate::controller_ui::button(ui, "Extract contents").clicked() { choice = Some(true); }
                            if crate::controller_ui::button(ui, "Copy ZIP unchanged").clicked() { choice = Some(false); }
                        }
                        DataPrompt::Executable(pending) => {
                            ui.label("Use an added EXE instead of the current game executable?");
                            if crate::controller_ui::button(ui, "Keep current executable").clicked() { executable_choice = Some(None); }
                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                for path in pending.executables() {
                                    if crate::controller_ui::button(ui, path).clicked() { executable_choice = Some(Some(path.clone())); }
                                }
                            });
                        }
                        DataPrompt::Overwrite(pending) => {
                            ui.label("Existing files or folders will be replaced. No files have been copied into the game folder yet.");
                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                for path in pending.conflicts() { ui.label(path); }
                            });
                            if crate::controller_ui::button(ui, "Yes").clicked() { choice = Some(true); }
                        }
                        DataPrompt::Notice { message, .. } => {
                            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| { ui.label(message); });
                        }
                    }
                    let label = if matches!(self, DataPrompt::Notice { .. }) { "Close" } else { "Cancel" };
                    if crate::controller_ui::button(ui, label).clicked() { cancel = true; }
                };
                // The first Window pass only measures disabled contents. Do not
                // consume initial page focus until the buttons are interactive.
                if ui.is_enabled() {
                    crate::controller_ui::page_scope(ui, title, contents);
                } else {
                    contents(ui);
                }
            });
        if cancel {
            Some(DataChoice::Cancel)
        } else if let Some(path) = executable_choice {
            Some(DataChoice::Executable(path))
        } else {
            choice.map(|choice| {
                if matches!(self, Self::Overwrite(_)) {
                    DataChoice::Overwrite
                } else {
                    DataChoice::Zip(choice)
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller_ui::{self, Navigation};

    fn frame(ctx: &egui::Context, nav: &mut Navigation, prompt: &DataPrompt) -> Option<DataChoice> {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        nav.prepare_input(ctx, &mut raw, true);
        let mut choice = None;
        let _ = ctx.run(raw, |ctx| {
            controller_ui::begin_frame(ctx);
            choice = prompt.show(ctx);
            nav.show(ctx);
        });
        choice
    }

    #[test]
    fn controller_can_choose_copy_zip_and_cancel_without_touch_input() {
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let prompt = DataPrompt::Zip {
            id: "game".into(),
            path: "data.zip".into(),
        };
        assert_eq!(frame(&ctx, &mut nav, &prompt), None);
        // egui measures a new Window with disabled contents on its first pass.
        assert_eq!(frame(&ctx, &mut nav, &prompt), None);
        nav.press("DPadDown");
        assert_eq!(frame(&ctx, &mut nav, &prompt), None);
        nav.press("South");
        assert_eq!(frame(&ctx, &mut nav, &prompt), Some(DataChoice::Zip(false)));
        nav.press("Escape"); // App maps the controller's back button to Escape.
        assert_eq!(frame(&ctx, &mut nav, &prompt), Some(DataChoice::Cancel));
    }

    #[test]
    fn controller_can_close_destination_notice() {
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let prompt = DataPrompt::Notice {
            title: "Add game data complete",
            message: "Files were placed in: /games/test/extracted".into(),
        };
        assert_eq!(frame(&ctx, &mut nav, &prompt), None);
        // egui measures a new Window with disabled contents on its first pass.
        assert_eq!(frame(&ctx, &mut nav, &prompt), None);
        nav.press("South");
        assert_eq!(frame(&ctx, &mut nav, &prompt), Some(DataChoice::Cancel));
    }
}
