//! Interactive, unbranded views of the saved input bindings.
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2};
use pocket_library::{GuestButton, LauncherConfig};

const LIVE: Color32 = Color32::from_rgb(115, 235, 250);
const BLUE: Color32 = Color32::from_rgb(93, 179, 235);
const GREEN: Color32 = Color32::from_rgb(93, 194, 162);
pub(crate) const LONG_PRESS: Color32 = Color32::from_rgb(231, 181, 96);
pub(crate) const PURPLE: Color32 = Color32::from_rgb(176, 135, 231);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assignment {
    Unbound,
    Guest(GuestButton),
    Stylus(usize),
}

impl Assignment {
    fn short(self) -> &'static str {
        use GuestButton::*;
        match self {
            Self::Unbound => "—",
            Self::Stylus(0) => "Pen Up",
            Self::Stylus(1) => "Pen Down",
            Self::Stylus(2) => "Pen Left",
            Self::Stylus(_) => "Pen Right",
            Self::Guest(button) => match button {
                DpadUp => "Up",
                DpadDown => "Down",
                DpadLeft => "Left",
                DpadRight => "Right",
                Action => "Action",
                ButtonA => "A",
                ButtonB => "B",
                ButtonC => "C",
                Soft1 => "Soft 1",
                Soft2 => "Soft 2",
                Turbo => "Turbo",
                GizPiano1 => "Home",
                GizPiano2 => "Vol",
                GizPiano3 => "Light",
                GizPiano4 => "Geo",
                GizPiano5 => "Power",
                PocketPcApp1 => "App 1",
                PocketPcApp2 => "App 2",
                PocketPcApp3 => "App 3",
                PocketPcApp4 => "App 4",
            },
        }
    }

    fn label(self) -> String {
        match self {
            Self::Unbound => "Unassigned".into(),
            Self::Guest(button) => readable(button.label()),
            Self::Stylus(direction) => {
                format!("Stylus {}", pocket_library::stylus::DIRECTIONS[direction])
            }
        }
    }

    fn label_for(self, gizmondo: bool) -> String {
        if let Self::Guest(button) = self {
            let label = readable(button.label());
            if let Some((ppc, giz)) = label.split_once(" / ") {
                return if gizmondo {
                    giz.trim_start_matches("Gizmondo ").into()
                } else {
                    ppc.into()
                };
            }
        }
        self.label()
    }

    fn color(self) -> Color32 {
        match self {
            Self::Unbound => Color32::from_gray(90),
            Self::Guest(_) => BLUE,
            Self::Stylus(_) => GREEN,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Capture {
    Keyboard(GuestButton),
    Controller(GuestButton),
    StylusKeyboard(usize),
    StylusController(usize),
}

#[derive(Clone, serde::Deserialize)]
struct SkinKey {
    #[serde(default)]
    live_key: Option<String>,
    input: Option<String>,
    label: String,
    rect: [f32; 4],
}

fn readable(text: &str) -> String {
    text.replace('↑', "Up")
        .replace('↓', "Down")
        .replace('←', "Left")
        .replace('→', "Right")
}

fn load_skin(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let image = image::load_from_memory(bytes)
        .expect("bundled input skin")
        .to_rgba8();
    let pixels = egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    ctx.load_texture(name, pixels, egui::TextureOptions::LINEAR)
}

pub struct Editor {
    gizmondo: bool,
    live_controls: std::collections::HashSet<(usize, String)>,
    controller_skin: Option<egui::TextureHandle>,
    keyboard_skin: Option<egui::TextureHandle>,
    keyboard_layout: Vec<SkinKey>,
    key: Option<String>,
    control: Option<String>,
    keyboard_target: Option<Assignment>,
    controller_target: Option<Assignment>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            gizmondo: false,
            live_controls: Default::default(),
            controller_skin: None,
            keyboard_skin: None,
            keyboard_layout: {
                let mut keys: Vec<SkinKey> =
                    serde_json::from_str(include_str!("../assets/input/keyboard-layout.json"))
                        .expect("bundled keyboard layout");
                for key in &mut keys {
                    if let Some(name) = key.input.as_mut() {
                        *name = egui::Key::from_name(name)
                            .expect("supported skin key")
                            .name()
                            .into();
                    }
                }
                keys
            },
            key: Some("Z".into()),
            control: Some("DPadUp".into()),
            keyboard_target: None,
            controller_target: None,
        }
    }
}

fn same_key(a: &str, b: &str) -> bool {
    pocket_library::keybindings::keys_match(a, b)
}

fn visible_buttons(gizmondo: bool) -> impl Iterator<Item = GuestButton> {
    GuestButton::ALL.into_iter().filter(move |button| {
        let index = GuestButton::ALL.iter().position(|b| b == button).unwrap();
        if gizmondo {
            index < 16 && *button != GuestButton::Turbo
        } else {
            index < 11 || index >= 16
        }
    })
}

fn guest_assignment(button: GuestButton, gizmondo: bool) -> Assignment {
    // The legacy Turbo binding sends VK_F3, which is Gizmondo's Brightness key.
    Assignment::Guest(if gizmondo && button == GuestButton::Turbo {
        GuestButton::GizPiano3
    } else {
        button
    })
}

fn controller_tap(control: &str) -> Option<&'static str> {
    match control {
        "Select" => Some("Fullscreen"),
        "Start" | "Misc1" => Some("Next filter"),
        _ => None,
    }
}
fn controller_hold(control: &str, gizmondo: bool) -> Option<&'static str> {
    match control {
        "Select" if gizmondo => Some("Brightness"),
        "Select" => Some("Return to library"),
        "Start" | "Misc1" => Some("Start / stop recording"),
        "Mode" if !gizmondo => Some("Return to library"),
        _ => None,
    }
}
fn short_hold(control: &str, gizmondo: bool) -> Option<&'static str> {
    controller_hold(control, gizmondo).map(|role| match role {
        "Brightness" => "Brightness",
        "Return to library" => "Exit",
        _ => "Record",
    })
}

fn keyboard_assignment(config: &LauncherConfig, key: &str, gizmondo: bool) -> Assignment {
    if !gizmondo && config.stylus_stick.enabled {
        if let Some(index) = config
            .stylus_stick
            .keyboard
            .iter()
            .position(|keys| keys.iter().any(|bound| same_key(bound, key)))
        {
            return Assignment::Stylus(index);
        }
    }
    config
        .keybindings
        .button_for_key(key)
        .map_or(Assignment::Unbound, |button| {
            guest_assignment(button, gizmondo)
        })
}

fn keyboard_shortcut(key: &str) -> Option<&'static str> {
    match key {
        "F10" => Some("Screenshot"),
        "F11" => Some("Start / stop recording"),
        "F12" => Some("Toggle fullscreen"),
        _ => None,
    }
}

fn keyboard_context_note(key: &str) -> Option<&'static str> {
    match key {
        "Escape" => Some("In fullscreen, Escape exits fullscreen before reaching the game."),
        "F6" => Some(
            "F6 also navigates the launcher toolbar. In Colors, randomized GPS can consume it.",
        ),
        _ => None,
    }
}

fn controller_shortcut(control: &str, gizmondo: bool) -> Option<&'static str> {
    match control {
        "Select" if gizmondo => Some("Tap: fullscreen · hold: Brightness"),
        "Select" => Some("Tap: fullscreen · hold: return to library"),
        "Start" | "Misc1" => Some("Tap: next filter · hold: start / stop recording"),
        "Mode" if !gizmondo => Some("Hold: return to library (if delivered by the OS)"),
        _ => None,
    }
}

fn controller_assignment(config: &LauncherConfig, control: &str, gizmondo: bool) -> Assignment {
    if let Some(button) = pocket_library::keybindings::steam_deck_guest_button(control, gizmondo) {
        return Assignment::Guest(button);
    }
    if controller_shortcut(control, gizmondo).is_some()
        || config.gamepad_toggles_fullscreen(control)
    {
        return Assignment::Unbound;
    }
    if !gizmondo && config.stylus_stick.enabled {
        if let Some(index) = config
            .stylus_stick
            .controller
            .iter()
            .position(|keys| keys.iter().any(|key| key == control))
        {
            return Assignment::Stylus(index);
        }
    }
    config
        .gamepad_bindings
        .get(control)
        .copied()
        .map_or(Assignment::Unbound, |button| {
            guest_assignment(button, gizmondo)
        })
}

fn controller_fixed(control: &str, gizmondo: bool) -> bool {
    controller_shortcut(control, gizmondo).is_some()
        || pocket_library::keybindings::steam_deck_guest_button(control, gizmondo).is_some()
}

fn set_assignment(config: &mut LauncherConfig, input: &str, controller: bool, target: Assignment) {
    if controller {
        config.gamepad_bindings.remove(input);
        if config.gamepad_fullscreen.as_deref() == Some(input) {
            config.gamepad_fullscreen = None;
        }
        for keys in &mut config.stylus_stick.controller {
            keys.retain(|key| key != input);
        }
    } else {
        for button in GuestButton::ALL {
            config.keybindings.unbind(button, input);
        }
        for keys in &mut config.stylus_stick.keyboard {
            keys.retain(|key| !same_key(key, input));
        }
    }
    if matches!(target, Assignment::Stylus(_)) {
        config.stylus_stick.enabled = true;
    }
    match target {
        Assignment::Guest(button) if controller => {
            config.gamepad_bindings.insert(input.into(), button);
        }
        Assignment::Guest(button) => config.keybindings.bind(button, input),
        Assignment::Stylus(direction) if controller => {
            config.stylus_stick.controller[direction].push(input.into());
        }
        Assignment::Stylus(direction) => config.stylus_stick.keyboard[direction].push(input.into()),
        Assignment::Unbound => {}
    }
}

impl Editor {
    pub fn live_input(&mut self, device: usize, control: &str, down: bool) {
        let key = (device, control.to_owned());
        if down {
            self.live_controls.insert(key);
        } else {
            self.live_controls.remove(&key);
        }
    }
    pub fn clear_live(&mut self) {
        self.live_controls.clear();
    }
    pub fn retain_live_devices(&mut self, devices: &[(usize, String)]) {
        self.live_controls
            .retain(|(id, _)| devices.iter().any(|(device, _)| device == id));
    }
    fn live_control(&self, name: &str) -> bool {
        self.live_controls
            .iter()
            .any(|(_, control)| control == name)
    }
    fn live_key(ui: &Ui, name: &str) -> bool {
        ui.input(|input| {
            if !input.viewport().focused.unwrap_or(true) {
                return false;
            }
            match name {
                "Shift" => input.modifiers.shift,
                "Ctrl" => input.modifiers.ctrl,
                "Alt" => input.modifiers.alt,
                "Super" => input.modifiers.mac_cmd,
                _ => egui::Key::from_name(name).is_some_and(|key| input.key_down(key)),
            }
        })
    }
    pub fn is_gizmondo(&self) -> bool {
        self.gizmondo
    }
    /// Reuse the visual editor's conflict handling for physical input capture.
    pub fn capture(
        &mut self,
        config: &mut LauncherConfig,
        input: &str,
        controller: bool,
        target: Assignment,
    ) -> bool {
        if (controller && controller_fixed(input, self.gizmondo))
            || (!controller && keyboard_shortcut(input).is_some())
        {
            return false;
        }
        set_assignment(config, input, controller, target);
        if controller {
            self.control = Some(input.into());
            self.controller_target = None;
        } else {
            self.key = Some(input.into());
            self.keyboard_target = None;
        }
        true
    }

    pub fn profile(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Preview device");
            for (value, label) in [(false, "Pocket PC"), (true, "Gizmondo")] {
                if crate::controller_ui::selectable_value(ui, &mut self.gizmondo, value, label)
                    .changed()
                {
                    self.keyboard_target = None;
                    self.controller_target = None;
                }
            }
        });
        ui.label("Select a key or control to inspect its saved assignment. The preview device does not change the game profile. Game-specific swaps and screen rotation apply when playing.");
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        config: &mut LauncherConfig,
        controller: bool,
    ) -> Option<Capture> {
        // Controller heading belongs before the launcher's device diagnostics.
        if !controller {
            ui.separator();
            ui.heading("Keyboard");
        }
        let wide = ui.available_width() >= 1100.0;
        let diagram_width = if wide {
            ui.available_width() * 0.72
        } else {
            ui.available_width()
        };
        let inspector_width = (ui.available_width() - diagram_width - 24.0).max(200.0);
        let mut request = None;
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(diagram_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(diagram_width);
                        self.diagram(ui, config, controller);
                    },
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_width(inspector_width);
                    request = self.inspector_card(ui, config, controller);
                });
            });
        } else {
            self.diagram(ui, config, controller);
            request = self.inspector_card(ui, config, controller);
        }
        self.unassigned(ui, config, controller);
        request
    }

    fn inspector_card(
        &mut self,
        ui: &mut Ui,
        config: &mut LauncherConfig,
        controller: bool,
    ) -> Option<Capture> {
        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(31, 35, 41))
            .stroke(Stroke::new(1.0, Color32::from_gray(65)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Assignment").strong());
                ui.add_space(8.0);
                self.inspector(ui, config, controller)
            })
            .inner
    }

    fn inspector(
        &mut self,
        ui: &mut Ui,
        config: &mut LauncherConfig,
        controller: bool,
    ) -> Option<Capture> {
        // Also expose saved inputs that are absent from the compact schematic.
        let mut inputs: Vec<String> = if controller {
            config
                .gamepad_bindings
                .keys()
                .cloned()
                .chain(config.stylus_stick.controller.iter().flatten().cloned())
                .collect()
        } else {
            GuestButton::ALL
                .iter()
                .flat_map(|button| config.keybindings.keys_for(*button).iter().cloned())
                .chain(config.stylus_stick.keyboard.iter().flatten().cloned())
                .collect()
        };
        inputs.sort();
        inputs.dedup();
        let selected = if controller {
            &mut self.control
        } else {
            &mut self.key
        };
        ui.label(if controller {
            "Controller input"
        } else {
            "Keyboard key"
        });
        crate::controller_ui::ComboBox::from_id_source(("saved_input", controller))
            .selected_text(selected.as_deref().unwrap_or("Saved inputs"))
            .show_ui(ui, |ui| {
                for input in inputs {
                    if crate::controller_ui::selectable_value(
                        ui,
                        selected,
                        Some(input.clone()),
                        input,
                    )
                    .changed()
                    {
                        if controller {
                            self.controller_target = None;
                        } else {
                            self.keyboard_target = None;
                        }
                    }
                }
            });
        let Some(input) = selected.clone() else {
            ui.label("Select a key or button on the diagram.");
            return None;
        };
        let physical = if controller {
            controller_inputs()
                .into_iter()
                .find(|(name, _, _, _)| *name == input)
                .map(|(_, label, _, _)| label)
                .unwrap_or(&input)
        } else {
            &input
        };
        ui.heading(physical);
        ui.separator();
        if !controller {
            if let Some(note) = keyboard_context_note(&input) {
                ui.weak(note);
            }
        }
        let shortcut = if controller {
            controller_shortcut(&input, self.gizmondo)
        } else {
            keyboard_shortcut(&input)
        };
        if let Some(description) = shortcut {
            if controller {
                if let Some(action) = controller_tap(&input) {
                    ui.colored_label(PURPLE, format!("Tap: {action}"));
                }
                if let Some(action) = controller_hold(&input, self.gizmondo) {
                    ui.colored_label(LONG_PRESS, format!("Hold: {action}"));
                }
            } else {
                ui.colored_label(PURPLE, description);
            }
            ui.label("Built-in PocketHLE shortcut.");
            return None;
        }
        let current = if controller {
            controller_assignment(config, &input, self.gizmondo)
        } else {
            keyboard_assignment(config, &input, self.gizmondo)
        };
        if controller && controller_fixed(&input, self.gizmondo) {
            ui.colored_label(current.color(), current.label_for(self.gizmondo));
            ui.label(
                "Built-in device mapping. This takes priority over the saved controller table.",
            );
            return None;
        }
        if controller && config.gamepad_toggles_fullscreen(&input) {
            ui.colored_label(PURPLE, "Fullscreen shortcut (configured below)");
        } else {
            ui.label("Mapped to");
            ui.colored_label(current.color(), current.label_for(self.gizmondo));
        }
        let draft = if controller {
            &mut self.controller_target
        } else {
            &mut self.keyboard_target
        };
        let mut target = draft.unwrap_or(current);
        ui.add_space(8.0);
        ui.label("Change assignment");
        crate::controller_ui::ComboBox::from_id_source(("visual_assignment", controller))
            .selected_text(target.short())
            .show_ui(ui, |ui| {
                crate::controller_ui::selectable_value(
                    ui,
                    &mut target,
                    Assignment::Unbound,
                    "Unassigned",
                );
                for button in visible_buttons(self.gizmondo) {
                    crate::controller_ui::selectable_value(
                        ui,
                        &mut target,
                        Assignment::Guest(button),
                        Assignment::Guest(button).label_for(self.gizmondo),
                    );
                }
                if !self.gizmondo {
                    for (direction, label) in pocket_library::stylus::DIRECTIONS.iter().enumerate()
                    {
                        crate::controller_ui::selectable_value(
                            ui,
                            &mut target,
                            Assignment::Stylus(direction),
                            format!("Stylus {label}"),
                        );
                    }
                }
            });
        *draft = Some(target);
        ui.horizontal_wrapped(|ui| {
            if crate::controller_ui::button(ui, "Apply assignment").clicked() {
                set_assignment(config, &input, controller, target);
            }
            if crate::controller_ui::button(ui, "Clear").clicked() {
                set_assignment(config, &input, controller, Assignment::Unbound);
                *draft = Some(Assignment::Unbound);
            }
        });
        ui.add_space(10.0);
        ui.separator();
        ui.label("Add another input for this command");
        let mut capture = None;
        if target != Assignment::Unbound
            && crate::controller_ui::button(ui, "Press an input…").clicked()
        {
            capture = match (controller, target) {
                (false, Assignment::Guest(button)) => Some(Capture::Keyboard(button)),
                (true, Assignment::Guest(button)) => Some(Capture::Controller(button)),
                (false, Assignment::Stylus(direction)) => Some(Capture::StylusKeyboard(direction)),
                (true, Assignment::Stylus(direction)) => Some(Capture::StylusController(direction)),
                _ => None,
            };
        }
        capture
    }

    fn unassigned(&mut self, ui: &mut Ui, config: &LauncherConfig, controller: bool) {
        let missing: Vec<_> = visible_buttons(self.gizmondo)
            .filter(|button| {
                if controller {
                    !controller_inputs()
                        .iter()
                        .map(|(name, _, _, _)| *name)
                        .chain(config.gamepad_bindings.keys().map(String::as_str))
                        .any(|name| {
                            controller_shortcut(name, self.gizmondo).is_none()
                                && controller_assignment(config, name, self.gizmondo)
                                    == Assignment::Guest(*button)
                        })
                        && !(*button == GuestButton::GizPiano3 && self.gizmondo)
                } else {
                    !GuestButton::ALL
                        .iter()
                        .flat_map(|mapped| config.keybindings.keys_for(*mapped))
                        .any(|key| {
                            keyboard_shortcut(key).is_none()
                                && keyboard_assignment(config, key, self.gizmondo)
                                    == Assignment::Guest(*button)
                        })
                }
            })
            .collect();
        ui.add_space(8.0);
        if !missing.is_empty() {
            egui::CollapsingHeader::new(format!("Unassigned commands ({})", missing.len()))
                .id_source(("unassigned_controls", controller))
                .show(ui, |ui| {
                    for button in missing {
                        ui.label(Assignment::Guest(button).label_for(self.gizmondo));
                    }
                });
        }
    }

    fn diagram(&mut self, ui: &mut Ui, config: &LauncherConfig, controller: bool) {
        let width = ui.available_width().max(200.0);
        if controller {
            self.controller_diagram(ui, config, width);
        } else {
            let texture = self
                .keyboard_skin
                .get_or_insert_with(|| {
                    load_skin(
                        ui.ctx(),
                        "input-keyboard-flat",
                        include_bytes!("../assets/input/keyboard-flat.png"),
                    )
                })
                .id();
            let (bounds, _) =
                ui.allocate_exact_size(Vec2::new(width, width * 0.34), Sense::hover());
            ui.painter().image(
                texture,
                bounds,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let scale = width / 1000.0;
            for key in self.keyboard_layout.clone() {
                let [x, y, w, h] = key.rect;
                let rect = Rect::from_min_size(
                    bounds.min + Vec2::new(x, y) * scale,
                    Vec2::new(w, h) * scale,
                );
                if let Some(input) = key.input {
                    self.control(ui, config, false, &input, &key.label, rect);
                } else {
                    if key
                        .live_key
                        .as_deref()
                        .is_some_and(|name| Self::live_key(ui, name))
                    {
                        ui.painter().rect(
                            rect,
                            3.0,
                            Color32::from_rgb(30, 62, 72),
                            Stroke::new(2.0, LIVE),
                        );
                    }
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        key.label,
                        FontId::proportional((13.0 * scale).clamp(8.0, 13.0)),
                        Color32::from_gray(175),
                    );
                }
            }
            ui.label("AZERTY keyboard. Press a key to identify it; click to edit its assignment.");
        }
        ui.horizontal_wrapped(|ui| {
            for (color, label) in [
                (BLUE, "Game action"),
                (GREEN, "Stylus"),
                (PURPLE, "Shortcut"),
                (LONG_PRESS, "Long press"),
                (Color32::GRAY, "Unassigned"),
                (LIVE, "Pressed input"),
            ] {
                ui.colored_label(color, label);
            }
        });
    }

    fn controller_diagram(&mut self, ui: &mut Ui, config: &LauncherConfig, width: f32) {
        let texture = self
            .controller_skin
            .get_or_insert_with(|| {
                load_skin(
                    ui.ctx(),
                    "input-controller-flat",
                    include_bytes!("../assets/input/controller-flat.png"),
                )
            })
            .id();
        let compact = width < 620.0;
        let art_width = if compact { width - 24.0 } else { width * 0.62 };
        let art_height = art_width * 500.0 / 760.0;
        let height = if compact {
            art_height + 28.0 + 13.0 * 26.0
        } else {
            (art_height + 44.0).max(13.0 * 26.0 + 20.0)
        };
        let (bounds, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let art = Rect::from_min_size(
            bounds.min + Vec2::new((width - art_width) * 0.5, 12.0),
            Vec2::new(art_width, art_height),
        );
        ui.painter().image(
            texture,
            art,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        let rail_width = if compact {
            (width - 12.0) * 0.5
        } else {
            width * 0.18
        };
        let inputs = controller_inputs();
        // The shared layout lists the left rail followed by the right rail.
        for (side, controls) in inputs.chunks(13).enumerate() {
            for (row, (name, label, x, y)) in controls.iter().enumerate() {
                let rail = Rect::from_min_size(
                    bounds.min
                        + Vec2::new(
                            if side == 0 { 0.0 } else { width - rail_width },
                            if compact { art_height + 28.0 } else { 10.0 } + row as f32 * 26.0,
                        ),
                    Vec2::new(rail_width, 23.0),
                );
                let point = art.min + Vec2::new(*x, *y) * (art_width / 760.0);
                let active = self.control(ui, config, true, name, label, rail);
                {
                    if self.live_control(name) {
                        ui.painter().circle(
                            point,
                            (art_width * 0.028).max(9.0),
                            Color32::from_rgb(30, 62, 72),
                            Stroke::new(2.5, LIVE),
                        );
                    }
                    if *name == "Misc1" && self.live_control(name) {
                        for dx in [-4.0, 0.0, 4.0] {
                            ui.painter().circle_filled(
                                point + Vec2::new(dx, 0.0),
                                1.2,
                                Color32::LIGHT_GRAY,
                            );
                        }
                    }
                    let assignment = controller_assignment(config, name, self.gizmondo);
                    let shortcut = controller_shortcut(name, self.gizmondo);
                    let configured_fullscreen = !controller_fixed(name, self.gizmondo)
                        && config.gamepad_toggles_fullscreen(name);
                    let label = if configured_fullscreen {
                        "Full"
                    } else if shortcut.is_some() {
                        match *name {
                            "Select" => "Full",
                            "Start" => "Filter",
                            "Mode" => "Exit",
                            _ => "Host",
                        }
                    } else {
                        match assignment {
                            Assignment::Stylus(0) => "PenU",
                            Assignment::Stylus(1) => "PenD",
                            Assignment::Stylus(2) => "PenL",
                            Assignment::Stylus(_) => "PenR",
                            Assignment::Guest(GuestButton::Action) => "Act",
                            Assignment::Guest(GuestButton::Soft1) => "S1",
                            Assignment::Guest(GuestButton::Soft2) => "S2",
                            Assignment::Guest(GuestButton::PocketPcApp1) => "P1",
                            Assignment::Guest(GuestButton::PocketPcApp2) => "P2",
                            Assignment::Guest(GuestButton::PocketPcApp3) => "P3",
                            Assignment::Guest(GuestButton::PocketPcApp4) => "P4",
                            _ => assignment.short(),
                        }
                    };
                    let mut label_point = point;
                    if *name == "Misc1" {
                        label_point.y += 17.0;
                    }
                    if name.starts_with("LeftStick") || name.starts_with("RightStick") {
                        let distance = 14.0 * art_width / 760.0;
                        if name.ends_with("Left") {
                            label_point.x -= distance;
                        }
                        if name.ends_with("Right") {
                            label_point.x += distance;
                        }
                        if name.ends_with("Up") {
                            label_point.y -= distance;
                        }
                        if name.ends_with("Down") {
                            label_point.y += distance;
                        }
                    }
                    ui.painter().text(
                        label_point,
                        Align2::CENTER_CENTER,
                        label,
                        FontId::proportional((art_width / 760.0 * 14.0).clamp(8.0, 12.0)),
                        if shortcut.is_some() && controller_tap(name).is_none() {
                            LONG_PRESS
                        } else if shortcut.is_some() || configured_fullscreen {
                            PURPLE
                        } else {
                            assignment.color()
                        },
                    );
                    if let Some(hold) = short_hold(name, self.gizmondo) {
                        ui.painter().text(
                            point + Vec2::new(0.0, if *name == "Misc1" { 27.0 } else { 14.0 }),
                            Align2::CENTER_CENTER,
                            format!("Hold: {hold}"),
                            FontId::proportional(8.5),
                            LONG_PRESS,
                        );
                    }
                    let hit =
                        Rect::from_center_size(point, Vec2::splat((art_width * 0.045).max(14.0)));
                    let response =
                        ui.interact(hit, ui.id().with(("skin-hotspot", name)), Sense::click());
                    if response.clicked() {
                        self.control = Some((*name).into());
                        self.controller_target = None;
                    }
                    if active || response.hovered() {
                        ui.painter().circle_stroke(
                            point,
                            (art_width * 0.023).max(7.0),
                            Stroke::new(1.5, BLUE),
                        );
                        if !compact {
                            let from = if side == 0 {
                                rail.right_center()
                            } else {
                                rail.left_center()
                            };
                            ui.painter()
                                .line_segment([from, point], Stroke::new(1.0, BLUE));
                        }
                    }
                }
            }
        }
    }

    fn control(
        &mut self,
        ui: &mut Ui,
        config: &LauncherConfig,
        controller: bool,
        name: &str,
        label: &str,
        rect: Rect,
    ) -> bool {
        let assignment = if controller {
            controller_assignment(config, name, self.gizmondo)
        } else {
            keyboard_assignment(config, name, self.gizmondo)
        };
        let shortcut = if controller {
            controller_shortcut(name, self.gizmondo)
        } else {
            keyboard_shortcut(name)
        };
        let fullscreen = controller
            && !controller_fixed(name, self.gizmondo)
            && config.gamepad_toggles_fullscreen(name);
        let color = if shortcut.is_some() || fullscreen {
            PURPLE
        } else {
            assignment.color()
        };
        let selected = if controller { &self.control } else { &self.key };
        let is_selected = selected.as_deref() == Some(name);
        let pressed = if controller {
            self.live_control(name)
        } else {
            Self::live_key(ui, name)
        };
        // A child UI supplies a real navigable button without moving the diagram's cursor.
        let mut hit_ui = ui.child_ui(rect, egui::Layout::top_down(egui::Align::Min));
        let widgets = &mut hit_ui.style_mut().visuals.widgets;
        for visual in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            visual.bg_fill = Color32::TRANSPARENT;
            visual.weak_bg_fill = Color32::TRANSPARENT;
            visual.bg_stroke = Stroke::NONE;
        }
        let response = hit_ui
            .push_id(("binding_diagram", controller, name), |ui| {
                crate::controller_ui::sized_button(ui, rect.size(), "")
            })
            .inner;
        if controller {
            ui.painter().rect(
                rect,
                4.0,
                if pressed {
                    Color32::from_rgb(30, 62, 72)
                } else if is_selected {
                    Color32::from_rgb(36, 55, 72)
                } else {
                    Color32::from_rgb(34, 38, 44)
                },
                Stroke::new(
                    1.0,
                    if pressed {
                        LIVE
                    } else if is_selected {
                        BLUE
                    } else {
                        Color32::from_gray(68)
                    },
                ),
            );
        } else {
            ui.painter().rect(
                rect.shrink(0.5),
                3.0,
                if pressed {
                    Color32::from_rgb(30, 62, 72)
                } else if is_selected {
                    Color32::from_rgba_unmultiplied(65, 130, 180, 65)
                } else {
                    Color32::TRANSPARENT
                },
                Stroke::new(
                    if pressed || is_selected { 2.0 } else { 1.0 },
                    if pressed { LIVE } else { color },
                ),
            );
        }
        let command = if fullscreen {
            "Fullscreen"
        } else if shortcut.is_some() {
            match name {
                "F10" => "Photo",
                "F11" => "Video",
                "F12" => "Full",
                "Select" => "Fullscreen",
                "Mode" => "",
                "Start" | "Misc1" => "Filter",
                _ => "Host",
            }
        } else {
            assignment.short()
        };
        if controller {
            ui.painter().text(
                rect.left_center() + Vec2::new(6.0, 0.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                Color32::from_gray(210),
            );
            if let Some(hold) = short_hold(name, self.gizmondo) {
                if controller_tap(name).is_some() {
                    ui.painter().text(
                        rect.right_top() + Vec2::new(-6.0, 6.0),
                        Align2::RIGHT_CENTER,
                        command,
                        FontId::proportional(9.0),
                        PURPLE,
                    );
                }
                ui.painter().text(
                    rect.right_bottom() + Vec2::new(-6.0, -6.0),
                    Align2::RIGHT_CENTER,
                    format!("Hold: {hold}"),
                    FontId::proportional(9.0),
                    LONG_PRESS,
                );
            } else {
                ui.painter().text(
                    rect.right_center() - Vec2::new(6.0, 0.0),
                    Align2::RIGHT_CENTER,
                    command,
                    FontId::proportional(10.0),
                    color,
                );
            }
        } else {
            ui.painter().text(
                rect.center_top() + Vec2::new(0.0, rect.height() * 0.28),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional((rect.height() * 0.30).clamp(8.0, 14.0)),
                Color32::WHITE,
            );
            ui.painter().text(
                rect.center_top() + Vec2::new(0.0, rect.height() * 0.74),
                Align2::CENTER_CENTER,
                command,
                FontId::proportional((rect.height() * 0.22).clamp(7.0, 11.0)),
                color,
            );
        }
        response.clone().on_hover_text(format!(
            "{name}: {}",
            shortcut
                .map(str::to_owned)
                .unwrap_or_else(|| assignment.label_for(self.gizmondo))
        ));
        if response.clicked() {
            if controller {
                self.control = Some(name.into());
                self.controller_target = None;
            } else {
                self.key = Some(name.into());
                self.keyboard_target = None;
            }
        }
        is_selected || response.hovered()
    }
}

fn controller_inputs() -> Vec<(&'static str, &'static str, f32, f32)> {
    #[derive(serde::Deserialize)]
    struct Control {
        input: String,
        label: String,
        x: f32,
        y: f32,
    }
    static LAYOUT: std::sync::OnceLock<Vec<Control>> = std::sync::OnceLock::new();
    LAYOUT
        .get_or_init(|| {
            serde_json::from_str(include_str!("../assets/input/controller-layout.json"))
                .expect("bundled controller layout")
        })
        .iter()
        .map(|control| {
            (
                control.input.as_str(),
                control.label.as_str(),
                control.x,
                control.y,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_keyboard_state_survives_editor_event_consumption_and_releases() {
        let ctx = egui::Context::default();
        for down in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Z,
                        physical_key: Some(egui::Key::Z),
                        pressed: down,
                        repeat: false,
                        modifiers: egui::Modifiers::default(),
                    }],
                    ..Default::default()
                },
                |ctx| {
                    ctx.input_mut(|input| input.events.clear());
                    egui::CentralPanel::default()
                        .show(ctx, |ui| assert_eq!(Editor::live_key(ui, "Z"), down));
                },
            );
        }
    }

    #[test]
    fn live_controls_release_independently_and_clear_on_disconnect() {
        let mut editor = Editor::default();
        editor.live_input(1, "South", true);
        editor.live_input(2, "South", true);
        editor.live_input(1, "South", false);
        assert!(editor.live_control("South"));
        editor.retain_live_devices(&[(1, "pad".into())]);
        assert!(!editor.live_control("South"));
        editor.live_input(1, "Misc1", true);
        editor.clear_live();
        assert!(!editor.live_control("Misc1"));
        assert!(controller_inputs()
            .iter()
            .find(|(name, _, _, _)| *name == "Misc1")
            .is_some_and(|(_, _, x, y)| *x > 0.0 && *y > 0.0));
    }

    #[test]
    fn keyboard_skin_hitboxes_are_distinct_and_inside_the_asset() {
        let editor = Editor::default();
        for (index, key) in editor.keyboard_layout.iter().enumerate() {
            let [x, y, w, h] = key.rect;
            let rect = Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, h));
            assert!(x >= 0.0 && y >= 0.0 && rect.right() <= 1000.0 && rect.bottom() <= 340.0);
            for other in &editor.keyboard_layout[index + 1..] {
                let [ox, oy, ow, oh] = other.rect;
                assert!(
                    !rect.intersects(Rect::from_min_size(egui::pos2(ox, oy), Vec2::new(ow, oh)))
                );
            }
        }
    }

    #[test]
    fn gizmondo_has_brightness_not_turbo_and_view_hold_is_explicit() {
        assert!(!visible_buttons(true).any(|b| b == GuestButton::Turbo));
        assert!(visible_buttons(false).any(|b| b == GuestButton::Turbo));
        let config = LauncherConfig::default();
        assert_eq!(
            keyboard_assignment(&config, "T", true),
            Assignment::Guest(GuestButton::GizPiano3)
        );
        assert_eq!(controller_hold("Select", true), Some("Brightness"));
        assert_eq!(controller_tap("Select"), Some("Fullscreen"));
    }

    #[test]
    fn preview_matches_runtime_priority() {
        let mut config = LauncherConfig::default();
        config
            .gamepad_bindings
            .insert("South".into(), GuestButton::Turbo);
        assert_eq!(
            controller_assignment(&config, "South", false),
            Assignment::Guest(GuestButton::ButtonA)
        );
        assert_eq!(
            controller_assignment(&config, "South", true),
            Assignment::Guest(GuestButton::GizPiano3)
        );
        assert_eq!(
            keyboard_assignment(&config, "O", false),
            Assignment::Stylus(0)
        );
        assert_eq!(keyboard_assignment(&config, "O", true), Assignment::Unbound);
    }

    #[test]
    fn capture_moves_conflicting_bindings_and_selects_input() {
        let mut config = LauncherConfig::default();
        let mut editor = Editor::default();
        assert!(editor.capture(
            &mut config,
            "O",
            false,
            Assignment::Guest(GuestButton::Action)
        ));
        assert_eq!(
            config.keybindings.button_for_key("O"),
            Some(GuestButton::Action)
        );
        assert!(config
            .stylus_stick
            .keyboard
            .iter()
            .all(|keys| !keys.iter().any(|key| key == "O")));
        assert_eq!(editor.key.as_deref(), Some("O"));
        config.gamepad_fullscreen = Some("RightStickUp".into());
        assert!(editor.capture(
            &mut config,
            "RightStickUp",
            true,
            Assignment::Guest(GuestButton::Turbo)
        ));
        assert_eq!(config.gamepad_fullscreen, None);
        assert!(config.stylus_stick.controller[0].is_empty());
        assert!(editor.capture(&mut config, "RightStickUp", true, Assignment::Stylus(1)));
        assert!(!config.gamepad_bindings.contains_key("RightStickUp"));
        assert_eq!(
            controller_assignment(&config, "RightStickUp", false),
            Assignment::Stylus(1)
        );
    }

    #[test]
    fn capture_keeps_reserved_inputs_unchanged() {
        let mut config = LauncherConfig::default();
        let before = serde_json::to_value(&config).unwrap();
        let mut editor = Editor::default();
        for input in ["Start", "Select", "South", "LeftTrigger"] {
            assert!(!editor.capture(
                &mut config,
                input,
                true,
                Assignment::Guest(GuestButton::Turbo)
            ));
        }
        assert!(!editor.capture(&mut config, "F12", false, Assignment::Stylus(0)));
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
    }

    #[test]
    fn both_profiles_render_at_narrow_and_wide_widths() {
        for width in [400.0, 820.0, 1200.0] {
            for gizmondo in [false, true] {
                let ctx = egui::Context::default();
                let mut editor = Editor {
                    gizmondo,
                    ..Default::default()
                };
                let mut config = LauncherConfig::default();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 1800.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            editor.profile(ui);
                            editor.show(ui, &mut config, false);
                            editor.show(ui, &mut config, true);
                            assert!(
                                ui.min_rect().right() <= width + 1.0,
                                "UI exceeds width {width}: {:?}",
                                ui.min_rect()
                            );
                        });
                    },
                );
                assert!(!output.shapes.is_empty());
            }
        }
    }
}
