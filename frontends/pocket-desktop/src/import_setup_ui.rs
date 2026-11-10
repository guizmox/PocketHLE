use super::*;
use pocket_library::import_setup::{self, Choices, Platform};

pub(super) struct ImportSetupDraft {
    step: u8,
    supersampling: bool,
    initial_supersampling: bool,
    camera: bool,
    bluetooth: bool,
    location: bool,
    data: bool,
    pub skip: bool,
    error: Option<String>,
}
impl ImportSetupDraft {
    fn new(config: &LauncherConfig) -> Self {
        let supersampling = config.gpu_renderer && config.gles_internal_resolution > 1;
        Self {
            step: 0,
            supersampling,
            initial_supersampling: supersampling,
            camera: config.camera_enabled,
            bluetooth: config.bluetooth_enabled,
            location: config.gps_enabled,
            data: config.gprs_enabled,
            skip: false,
            error: None,
        }
    }
}
impl PocketLauncher {
    pub(super) fn ui_first_import_setup(&mut self, ctx: &egui::Context) -> bool {
        if self.screen != Screen::Library
            || self.library_busy
            || self.rename_draft.is_some()
            || self.demo_import.is_some()
            || self.pending_import.is_some()
            || self.add_data.is_some()
        {
            return false;
        }
        if self.first_import_setup.is_none() {
            while let Some(kind) = self.first_import_setups.pop_front() {
                if kind == Platform::Gizmondo
                    && !self.library.config().first_import_setup.contains(kind)
                {
                    self.first_import_setup = Some(ImportSetupDraft::new(self.library.config()));
                    break;
                }
            }
        }
        let Some(mut draft) = self.first_import_setup.take() else {
            return false;
        };
        self.deck_nav_repeat = None;
        self.deck_library_actions.clear();
        draft.skip |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut save = false;
        egui::CentralPanel::default().show(ctx, |_| {});
        egui::Window::new("Welcome to Gizmondo")
            .id(egui::Id::new("first_gizmondo_import"))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false).resizable(false).default_width(470.0)
            .show(ctx, |ui| {
                crate::controller_ui::page_scope(ui, ("first_import_setup", draft.step), |ui| {
                    egui::ScrollArea::vertical().max_height((ctx.available_rect().height()-140.0).max(100.0)).show(ui, |ui| {
                        ui.heading(if draft.step == 0 { "Sharper graphics" } else { "Hardware connections" });
                        ui.label("Choose the defaults for your Gizmondo games. You can change them later in Settings.");
                        if draft.step == 0 {
                            crate::controller_ui::checkbox(ui, &mut draft.supersampling, "Enable supersampling (2x to start)");
                            ui.label("Renders 3D games at a higher internal resolution for a cleaner image. Requires GPU rendering and more GPU power; it adds no detail to 2D artwork.");
                            ui.label("This is the global OpenGL ES setting and also affects PocketPC GPU games. An existing higher scale is kept. Per-game renderer overrides still apply.");
                        } else {
                            ui.label("Each bridge lets a game use the corresponding host service. Host permissions still apply.");
                            crate::controller_ui::checkbox(ui, &mut draft.camera, "Camera");
                            crate::controller_ui::checkbox(ui, &mut draft.bluetooth, "Bluetooth");
                            crate::controller_ui::checkbox(ui, &mut draft.location, "Location / GPS");
                            crate::controller_ui::checkbox(ui, &mut draft.data, "Internet / data (host connection)");
                        }
                        if let Some(error) = &draft.error { ui.colored_label(Color32::LIGHT_RED, error); }
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            if draft.step > 0 && crate::controller_ui::button(ui, "Back").clicked() { draft.step -= 1; }
                            if crate::controller_ui::button(ui, "Keep current settings").clicked() { draft.skip = true; }
                            if draft.step == 0 {
                                if crate::controller_ui::button(ui, "Next").clicked() { draft.step = 1; }
                            } else { save = crate::controller_ui::button(ui, "Save choices").clicked(); }
                        });
                        ui.weak(crate::controller_labels::text(ctx,"D-pad / stick: navigate · {South}: select · {East}: keep current settings"));
                    });
                });
            });
        if save || draft.skip {
            let choices = Choices::Gizmondo {
                supersampling: (!draft.skip && draft.supersampling != draft.initial_supersampling)
                    .then_some(draft.supersampling),
                camera: (!draft.skip).then_some(draft.camera),
                bluetooth: (!draft.skip).then_some(draft.bluetooth),
                location: (!draft.skip).then_some(draft.location),
                data: (!draft.skip).then_some(draft.data),
                controls_size: None,
                overlap: None,
            };
            match import_setup::save(self.library.root(), self.library.profile(), &choices) {
                Ok(config) => {
                    *self.library.config_mut() = config;
                    self.controller_ui.stop_repeat();
                    ctx.request_repaint();
                }
                Err(error) => {
                    draft.error = Some(format!("Could not save choices: {error}"));
                    draft.skip = false;
                    self.first_import_setup = Some(draft);
                }
            }
        } else {
            self.first_import_setup = Some(draft);
        }
        self.controller_ui.show(ctx);
        true
    }
}
