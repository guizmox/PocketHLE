//! Interactive, unbranded views of the saved input bindings.
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2};
use pocket_library::{GuestButton, LauncherConfig};

const BLUE: Color32 = Color32::from_rgb(93, 179, 235);
const GREEN: Color32 = Color32::from_rgb(93, 194, 162);
const PURPLE: Color32 = Color32::from_rgb(176, 135, 231);

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
            Self::Stylus(0) => "Pen ↑",
            Self::Stylus(1) => "Pen ↓",
            Self::Stylus(2) => "Pen ←",
            Self::Stylus(_) => "Pen →",
            Self::Guest(button) => match button {
                DpadUp => "↑",
                DpadDown => "↓",
                DpadLeft => "←",
                DpadRight => "→",
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
            Self::Guest(button) => button.label().into(),
            Self::Stylus(direction) => {
                format!("Stylus {}", pocket_library::stylus::DIRECTIONS[direction])
            }
        }
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

pub struct Editor {
    gizmondo: bool,
    key: Option<String>,
    control: Option<String>,
    keyboard_target: Option<Assignment>,
    controller_target: Option<Assignment>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            gizmondo: false,
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
            index < 16
        } else {
            index < 11 || index >= 16
        }
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
        .map_or(Assignment::Unbound, Assignment::Guest)
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
        .map_or(Assignment::Unbound, Assignment::Guest)
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
        ui.separator();
        ui.heading(if controller { "Controller" } else { "Keyboard" });
        let wide = ui.available_width() >= 800.0;
        let diagram_width = if wide {
            ui.available_width() * 0.64
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
                    request = self.inspector(ui, config, controller);
                });
            });
        } else {
            self.diagram(ui, config, controller);
            request = self.inspector(ui, config, controller);
        }
        self.unassigned(ui, config, controller);
        request
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
        ui.label(if controller {
            "Selected control"
        } else {
            "Selected key"
        });
        ui.heading(&input);
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
            ui.colored_label(PURPLE, description);
            ui.label("Built-in PocketHLE shortcut.");
            return None;
        }
        let current = if controller {
            controller_assignment(config, &input, self.gizmondo)
        } else {
            keyboard_assignment(config, &input, self.gizmondo)
        };
        if controller && controller_fixed(&input, self.gizmondo) {
            ui.label(current.label());
            ui.label(
                "Built-in device mapping. This takes priority over the saved controller table.",
            );
            return None;
        }
        if controller && config.gamepad_toggles_fullscreen(&input) {
            ui.colored_label(PURPLE, "Fullscreen shortcut (configured below)");
        } else {
            ui.label(format!("Current: {}", current.label()));
        }
        let draft = if controller {
            &mut self.controller_target
        } else {
            &mut self.keyboard_target
        };
        let mut target = draft.unwrap_or(current);
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
                        button.label(),
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
        ui.label("Assign this command to another input");
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
                    !config.keybindings.keys_for(*button).iter().any(|key| {
                        keyboard_shortcut(key).is_none()
                            && keyboard_assignment(config, key, self.gizmondo)
                                == Assignment::Guest(*button)
                    })
                }
            })
            .collect();
        if missing.is_empty() {
            ui.label("All device commands have an assignment.");
        } else {
            ui.colored_label(
                Color32::from_rgb(238, 193, 114),
                format!(
                    "Unassigned: {}",
                    missing
                        .iter()
                        .map(|b| b.label())
                        .collect::<Vec<_>>()
                        .join(" · ")
                ),
            );
        }
    }

    fn diagram(&mut self, ui: &mut Ui, config: &LauncherConfig, controller: bool) {
        let width = ui.available_width().max(200.0);
        let height = width * if controller { 0.53 } else { 0.34 };
        let (bounds, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        if controller {
            let scale = width / 760.0;
            let center = bounds.min + Vec2::new(380.0, 210.0) * scale;
            ui.painter()
                .rect(bounds, 12.0, Color32::from_gray(28), Stroke::NONE);
            ui.painter().rect(
                Rect::from_center_size(center, Vec2::new(505.0, 230.0) * scale),
                65.0 * scale,
                Color32::from_rgb(59, 67, 80),
                Stroke::new(1.0, Color32::GRAY),
            );
            for x in [190.0, 570.0] {
                ui.painter().circle_filled(
                    bounds.min + Vec2::new(x, 285.0) * scale,
                    80.0 * scale,
                    Color32::from_rgb(59, 67, 80),
                );
            }
            for (name, text, x, y) in controller_inputs() {
                let rect = Rect::from_center_size(
                    bounds.min + Vec2::new(x, y) * scale,
                    Vec2::new(64.0, 38.0) * scale,
                );
                self.control(ui, config, true, name, text, rect);
            }
        } else {
            let rows: &[&[&str]] = &[
                &[
                    "Escape", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11",
                    "F12",
                ],
                &[
                    "1",
                    "2",
                    "3",
                    "4",
                    "5",
                    "6",
                    "7",
                    "8",
                    "9",
                    "0",
                    "Backspace",
                ],
                &[
                    "Tab", "A", "Z", "E", "R", "T", "Y", "U", "I", "O", "P", "Enter",
                ],
                &["Q", "S", "D", "F", "G", "H", "J", "K", "L", "M", "Up"],
                &[
                    "W", "X", "C", "V", "B", "N", "Space", "Left", "Down", "Right",
                ],
            ];
            let cell = width / 13.0;
            for (row, keys) in rows.iter().enumerate() {
                for (column, key) in keys.iter().enumerate() {
                    let min =
                        bounds.min + Vec2::new(column as f32 * cell, row as f32 * height / 5.0);
                    let rect = Rect::from_min_size(min, Vec2::new(cell - 3.0, height / 5.0 - 4.0));
                    self.control(ui, config, false, key, key, rect);
                }
            }
        }
        ui.label("Blue: game command · green: stylus · purple: host shortcut · gray: unassigned");
        if !controller {
            ui.label("AZERTY letter layout. Keys are stored by name. Other keys can be assigned using ‘Press an input…’.");
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
    ) {
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
        // A child UI supplies a real navigable button without moving the diagram's cursor.
        let mut hit_ui = ui.child_ui(rect, egui::Layout::top_down(egui::Align::Min));
        let response = hit_ui
            .push_id(("binding_diagram", controller, name), |ui| {
                crate::controller_ui::sized_button(ui, rect.size(), "")
            })
            .inner;
        ui.painter().rect(
            rect,
            5.0,
            if is_selected {
                Color32::from_rgb(43, 83, 116)
            } else {
                Color32::from_rgb(35, 41, 51)
            },
            Stroke::new(if is_selected { 2.0 } else { 1.0 }, color),
        );
        let text = match label {
            "Escape" => "Esc",
            "Backspace" => "Back",
            "Enter" => "Enter",
            other => other,
        };
        ui.painter().text(
            rect.center_top() + Vec2::new(0.0, rect.height() * 0.30),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional((rect.height() * 0.34).clamp(9.0, 15.0)),
            Color32::WHITE,
        );
        let command = if fullscreen {
            "Fullscr"
        } else if shortcut.is_some() {
            match name {
                "F10" => "Photo",
                "F11" => "Video",
                "F12" => "Fullscr",
                "Escape" => "Window",
                "Select" => "View",
                "Start" | "Misc1" => "Filter",
                _ => "Host",
            }
        } else {
            assignment.short()
        };
        ui.painter().text(
            rect.center_top() + Vec2::new(0.0, rect.height() * 0.73),
            Align2::CENTER_CENTER,
            command,
            FontId::proportional((rect.height() * 0.24).clamp(8.0, 12.0)),
            color,
        );
        response.clone().on_hover_text(format!(
            "{name}: {}",
            shortcut
                .map(str::to_owned)
                .unwrap_or_else(|| assignment.label())
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
    }
}

fn controller_inputs() -> Vec<(&'static str, &'static str, f32, f32)> {
    vec![
        ("LeftTrigger2", "LT", 220.0, 36.0),
        ("RightTrigger2", "RT", 540.0, 36.0),
        ("LeftTrigger", "LB", 210.0, 88.0),
        ("RightTrigger", "RB", 550.0, 88.0),
        ("LeftStickUp", "LS ↑", 210.0, 145.0),
        ("LeftStickLeft", "LS ←", 144.0, 186.0),
        ("LeftThumb", "L3", 210.0, 186.0),
        ("LeftStickRight", "LS →", 276.0, 186.0),
        ("LeftStickDown", "LS ↓", 210.0, 227.0),
        ("DPadUp", "↑", 240.0, 270.0),
        ("DPadLeft", "←", 174.0, 311.0),
        ("DPadRight", "→", 306.0, 311.0),
        ("DPadDown", "↓", 240.0, 352.0),
        ("RightStickUp", "RS ↑", 500.0, 270.0),
        ("RightStickLeft", "RS ←", 434.0, 311.0),
        ("RightThumb", "R3", 500.0, 311.0),
        ("RightStickRight", "RS →", 566.0, 311.0),
        ("RightStickDown", "RS ↓", 500.0, 352.0),
        ("North", "North", 570.0, 145.0),
        ("West", "West", 504.0, 186.0),
        ("East", "East", 636.0, 186.0),
        ("South", "South", 570.0, 227.0),
        ("Select", "View", 335.0, 180.0),
        ("Start", "Menu", 410.0, 180.0),
        ("Mode", "Guide", 375.0, 125.0),
        ("Misc1", "Extra", 690.0, 311.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Assignment::Guest(GuestButton::Turbo)
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
