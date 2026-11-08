//! Interactive, unbranded views of the saved input bindings.
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2};
use pocket_library::{GuestButton, LauncherConfig};

const RESERVED: Color32 = Color32::from_rgb(225, 95, 95);
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
                GizPiano3 => "Brightness",
                GizPiano4 => "Geo",
                GizPiano5 => "Power",
                PocketPcApp1 => "App 1",
                PocketPcApp2 => "App 2",
                PocketPcApp3 => "App 3",
                PocketPcApp4 => "App 4",
                CenterTap => "Tap",
                RotateScreen => "Rotate",
                Screenshot => "Screenshot",
                StylusClick => "Click ×2",
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
            if gizmondo {
                let name = match button {
                    GuestButton::Action => Some("Play"),
                    GuestButton::ButtonA => Some("Stop"),
                    GuestButton::ButtonB => Some("Forward"),
                    GuestButton::ButtonC => Some("Rewind"),
                    GuestButton::Soft1 => Some("L1"),
                    GuestButton::Soft2 => Some("R1"),
                    _ => None,
                };
                if let Some(name) = name {
                    return name.into();
                }
            }
            if !gizmondo {
                let name = match button {
                    GuestButton::Action => Some("ACTION"),
                    GuestButton::Soft1 => Some("TAB"),
                    GuestButton::Soft2 => Some("ESC"),
                    GuestButton::Turbo => Some("TURBO"),
                    GuestButton::ButtonA => Some("A"),
                    GuestButton::ButtonB => Some("B"),
                    GuestButton::ButtonC => Some("C"),
                    GuestButton::PocketPcApp1 => Some("P1"),
                    GuestButton::PocketPcApp2 => Some("P2"),
                    GuestButton::PocketPcApp3 => Some("P3"),
                    GuestButton::PocketPcApp4 => Some("P4"),
                    _ => None,
                };
                if let Some(name) = name {
                    return name.into();
                }
            }
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

    /// Compact captions for diagrams; command lists keep the full profile names.
    fn diagram_label(self, gizmondo: bool) -> String {
        if gizmondo
            && matches!(
                self,
                Self::Guest(
                    GuestButton::Action
                        | GuestButton::ButtonA
                        | GuestButton::ButtonB
                        | GuestButton::ButtonC
                        | GuestButton::Soft1
                        | GuestButton::Soft2
                )
            )
        {
            return self.label_for(true);
        }
        match self {
            Self::Guest(GuestButton::Action) => "Act".into(),
            Self::Guest(GuestButton::Soft1) => if gizmondo { "S1" } else { "Tab" }.into(),
            Self::Guest(GuestButton::Soft2) => if gizmondo { "S2" } else { "Esc" }.into(),
            Self::Guest(GuestButton::PocketPcApp1) => "P1".into(),
            Self::Guest(GuestButton::PocketPcApp2) => "P2".into(),
            Self::Guest(GuestButton::PocketPcApp3) => "P3".into(),
            Self::Guest(GuestButton::PocketPcApp4) => "P4".into(),
            _ => self.short().into(),
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Unbound => Color32::from_gray(90),
            Self::Guest(GuestButton::CenterTap | GuestButton::StylusClick) => GREEN,
            Self::Guest(GuestButton::RotateScreen | GuestButton::Screenshot) => PURPLE,
            Self::Guest(_) => BLUE,
            Self::Stylus(_) => GREEN,
        }
    }
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

#[derive(Default)]
struct QuickAssign {
    controller: bool,
    wait_launch_release: bool,
    launch_device: Option<usize>,
    done: bool,
    pressed: Option<(String, std::time::Instant)>,
    choice: Option<(String, bool)>,
    index: usize,
    reveal_choice: bool,
    message: String,
}

pub struct Editor {
    quick: Option<QuickAssign>,
    quick_save: bool,
    quick_nav: Vec<String>,
    gizmondo: bool,
    live_controls: std::collections::HashSet<(usize, String)>,
    thumb_chord_latched: std::collections::HashSet<usize>,
    controller_skin: Option<egui::TextureHandle>,
    keyboard_skin: Option<egui::TextureHandle>,
    keyboard_layout: Vec<SkinKey>,
    key: Option<String>,
    control: Option<String>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            quick: None,
            quick_save: false,
            quick_nav: Vec::new(),
            gizmondo: false,
            live_controls: Default::default(),
            thumb_chord_latched: Default::default(),
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
        }
    }
}

fn configured_inputs(config: &LauncherConfig, gizmondo: bool, controller: bool) -> Vec<String> {
    let prefix = pocket_library::input_gestures::binding_key(gizmondo, controller, "");
    config
        .short_press_bindings
        .keys()
        .chain(config.long_press_bindings.keys())
        .filter_map(|key| key.strip_prefix(&prefix))
        .map(|name| {
            if !controller && name.len() == 1 {
                name.to_ascii_uppercase()
            } else {
                name.to_owned()
            }
        })
        .collect()
}

fn same_key(a: &str, b: &str) -> bool {
    pocket_library::keybindings::keys_match(a, b)
}

fn visible_buttons(gizmondo: bool) -> impl Iterator<Item = GuestButton> {
    GuestButton::ALL.into_iter().filter(move |button| {
        let index = GuestButton::ALL.iter().position(|b| b == button).unwrap();
        if gizmondo {
            (index < 16 && *button != GuestButton::Turbo)
                || matches!(*button, GuestButton::RotateScreen | GuestButton::Screenshot)
        } else {
            index < 11 || index >= 16
        }
    })
}

fn guest_assignment(button: GuestButton, gizmondo: bool) -> Assignment {
    if gizmondo && matches!(button, GuestButton::CenterTap | GuestButton::StylusClick) {
        return Assignment::Unbound;
    }
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
        "Start" => Some("Next filter"),
        _ => None,
    }
}
fn controller_hold(control: &str, _gizmondo: bool) -> Option<&'static str> {
    match control {
        "Select" => Some("Return to library"),
        "Start" => Some("Start / stop recording"),
        _ => None,
    }
}
fn short_hold(control: &str, gizmondo: bool) -> Option<&'static str> {
    controller_hold(control, gizmondo).map(|role| match role {
        "Return to library" => "Exit",
        _ => "Record",
    })
}

fn keyboard_assignment(config: &LauncherConfig, key: &str, gizmondo: bool) -> Assignment {
    if pocket_library::keybindings::reserved_host_key(key) {
        return Assignment::Unbound;
    }
    if let Some(button) =
        config
            .short_press_bindings
            .get(&pocket_library::input_gestures::binding_key(
                gizmondo, false, key,
            ))
    {
        return button.map_or(Assignment::Unbound, |b| guest_assignment(b, gizmondo));
    }
    if !gizmondo
        && !config
            .long_press_bindings
            .contains_key(&pocket_library::input_gestures::binding_key(
                gizmondo, false, key,
            ))
    {
        if let Some(index) = config
            .stylus_stick
            .keyboard
            .iter()
            .position(|keys| keys.iter().any(|bound| same_key(bound, key)))
        {
            return Assignment::Stylus(index);
        }
    }
    pocket_library::keybindings::keyboard_button_for_profile(&config.keybindings, key, gizmondo)
        .map_or(Assignment::Unbound, |button| {
            guest_assignment(button, gizmondo)
        })
}

fn keyboard_shortcut(key: &str) -> Option<&'static str> {
    match key.to_ascii_uppercase().as_str() {
        "F7" => Some("Next rotation"),
        "F8" => Some("Return to library"),
        "F9" => Some("Next filter"),
        "F10" => Some("Screenshot"),
        "F11" => Some("Start / stop recording"),
        "F12" => Some("Toggle fullscreen"),
        _ => None,
    }
}

fn keyboard_context_note(key: &str) -> Option<&'static str> {
    match key {
        "Escape" => Some("In fullscreen, Escape exits fullscreen before reaching the game."),
        "F6" => Some("In Colors, randomized GPS can consume F6."),
        _ => None,
    }
}

fn controller_shortcut(control: &str, _gizmondo: bool) -> Option<&'static str> {
    match control {
        "Select" => Some("Tap: fullscreen · hold: return to library"),
        "Start" => Some("Tap: next filter · hold: start / stop recording"),
        _ => None,
    }
}

fn controller_assignment(config: &LauncherConfig, control: &str, gizmondo: bool) -> Assignment {
    if let Some(direction) =
        pocket_library::stylus::controller_direction(control).filter(|_| !gizmondo)
    {
        return Assignment::Stylus(direction);
    }
    if controller_fixed(control, gizmondo) {
        return Assignment::Unbound;
    }
    if let Some(button) =
        config
            .short_press_bindings
            .get(&pocket_library::input_gestures::binding_key(
                gizmondo, true, control,
            ))
    {
        return button.map_or(Assignment::Unbound, |b| guest_assignment(b, gizmondo));
    }
    if let Some(button) = pocket_library::keybindings::steam_deck_guest_button(control, gizmondo) {
        return Assignment::Guest(button);
    }
    if controller_shortcut(control, gizmondo).is_some()
        || config.gamepad_toggles_fullscreen(control)
    {
        return Assignment::Unbound;
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
    matches!(control, "Select" | "Start")
        || pocket_library::stylus::controller_reserved(control, gizmondo)
}

fn set_assignment(
    config: &mut LauncherConfig,
    input: &str,
    controller: bool,
    gizmondo: bool,
    target: Assignment,
) {
    if controller && (controller_fixed(input, gizmondo) || matches!(target, Assignment::Stylus(_)))
    {
        return;
    }
    let key = pocket_library::input_gestures::binding_key(gizmondo, controller, input);
    if controller && config.gamepad_fullscreen.as_deref() == Some(input) {
        config.gamepad_fullscreen = None;
    }
    let direction = match target {
        Assignment::Guest(button) => {
            if button == GuestButton::StylusClick {
                config.long_press_bindings.remove(&key);
            }
            config.short_press_bindings.insert(key, Some(button));
            return;
        }
        Assignment::Unbound => {
            config.short_press_bindings.insert(key, None);
            return;
        }
        Assignment::Stylus(direction) => {
            config.short_press_bindings.remove(&key);
            config.long_press_bindings.remove(&key);
            direction
        }
    };
    if !controller {
        for button in GuestButton::ALL {
            config.keybindings.unbind(button, input);
        }
        for keys in &mut config.stylus_stick.keyboard {
            keys.retain(|key| !same_key(key, input));
        }
        config.stylus_stick.keyboard[direction].push(input.into());
    }
}

fn assignment_targets(
    gizmondo: bool,
    controller: bool,
    _input: Option<&str>,
    long: bool,
) -> Vec<Assignment> {
    let stylus = !controller && !gizmondo && !long;
    std::iter::once(Assignment::Unbound)
        .chain(
            visible_buttons(gizmondo)
                .filter(|b| *b != GuestButton::StylusClick || (!long && !controller))
                .map(Assignment::Guest),
        )
        .chain((0..if stylus { 4 } else { 0 }).map(Assignment::Stylus))
        .collect()
}

fn assignment_choices(
    ui: &mut Ui,
    targets: &[Assignment],
    selected: usize,
    gizmondo: bool,
    reveal: bool,
) -> egui::scroll_area::ScrollAreaOutput<Option<usize>> {
    egui::ScrollArea::vertical()
        .id_source("input_assignment_choices")
        .max_height(320.0)
        .show(ui, |ui| {
            let mut clicked = None;
            for (index, target) in targets.iter().enumerate() {
                let row = ui.selectable_label(index == selected, target.label_for(gizmondo));
                if reveal && index == selected {
                    row.scroll_to_me(Some(egui::Align::Center));
                }
                if row.clicked() {
                    clicked = Some(index);
                }
            }
            clicked
        })
}

impl Editor {
    /// Called before the editor's ordinary key-event suppression.
    pub fn keyboard_shortcut(&mut self, ctx: &egui::Context, controller: bool) {
        if controller
            || self.quick_active()
            || !ctx.input(|i| i.viewport().focused.unwrap_or(true))
            || ctx.memory(|m| m.any_popup_open())
        {
            return;
        }
        let activate=ctx.input(|i|i.events.iter().any(|e|matches!(e,egui::Event::Key{key:egui::Key::Enter,pressed:true,repeat:false,modifiers,..} if modifiers.ctrl && !modifiers.alt && !modifiers.shift && !modifiers.mac_cmd)));
        if activate {
            self.quick = Some(QuickAssign {
                wait_launch_release: true,
                ..Default::default()
            });
            self.quick_nav.clear();
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(
                        e,
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            ..
                        } | egui::Event::Text(_)
                    )
                })
            });
            ctx.request_repaint();
        }
    }
    /// Both click buttons must be held on the same selected controller.
    pub fn controller_shortcut(&mut self, device: usize) {
        let left = self.live_controls.contains(&(device, "LeftThumb".into()));
        let right = self.live_controls.contains(&(device, "RightThumb".into()));
        if !left && !right {
            self.thumb_chord_latched.remove(&device);
        }
        if left && right && self.thumb_chord_latched.insert(device) && !self.quick_active() {
            self.quick = Some(QuickAssign {
                controller: true,
                wait_launch_release: true,
                launch_device: Some(device),
                ..Default::default()
            });
            self.quick_nav.clear();
        }
    }
    pub fn quick_active(&self) -> bool {
        self.quick.is_some()
    }
    pub fn take_quick_save(&mut self) -> bool {
        std::mem::take(&mut self.quick_save)
    }
    pub fn quick_assign(&mut self, ui: &mut Ui, config: &mut LauncherConfig, controller: bool) {
        if ui
            .add_enabled_ui(!self.quick_active(), |ui| {
                crate::controller_ui::sized_button(
                    ui,
                    [ui.available_width(), 48.0].into(),
                    if controller {
                        "Assign an input… (L3 + R3)"
                    } else {
                        "Assign an input… (Ctrl+Enter)"
                    },
                )
            })
            .inner
            .clicked()
        {
            self.quick = Some(QuickAssign {
                controller,
                ..Default::default()
            });
            self.quick_nav.clear();
        }
        ui.label("Press and release an input, then choose a device command. Hold for 0.65 s to assign a long press.");
        let Some(mut q) = self.quick.take() else {
            return;
        };
        crate::controller_ui::claim_modal_input(ui.ctx());
        let mut cancel = false;
        let mut accept = false;
        let events = ui.input(|i| i.events.clone());
        let waiting = q.wait_launch_release;
        if waiting
            && !q.controller
            && !ui.input(|i| i.modifiers.ctrl || i.key_down(egui::Key::Enter))
        {
            q.wait_launch_release = false;
        }
        if !q.controller && !waiting {
            for event in &events {
                if let egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    ..
                } = event
                {
                    if q.choice.is_some() || q.done {
                        if *pressed {
                            self.quick_nav.push(
                                match key {
                                    egui::Key::ArrowUp => "DPadUp",
                                    egui::Key::ArrowDown => "DPadDown",
                                    egui::Key::Enter => "South",
                                    egui::Key::Escape => "East",
                                    _ => "",
                                }
                                .into(),
                            );
                        }
                    } else if *pressed {
                        if q.pressed.is_none() {
                            q.pressed = Some((key.name().into(), std::time::Instant::now()));
                        }
                    } else if q
                        .pressed
                        .as_ref()
                        .is_some_and(|(name, _)| name == key.name())
                    {
                        let (name, at) = q.pressed.take().unwrap();
                        q.choice = Some((name, at.elapsed().as_millis() >= 650));
                        q.reveal_choice = true;
                    }
                }
            }
        }
        ui.ctx().input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)))
        });
        let targets = assignment_targets(
            self.gizmondo,
            controller,
            q.choice.as_ref().map(|(input, _)| input.as_str()),
            q.choice.as_ref().is_some_and(|(_, long)| *long),
        );
        let count = if q.done { 3 } else { targets.len() };
        for nav in self.quick_nav.drain(..) {
            match nav.as_str() {
                "DPadUp" | "LeftStickUp" => {
                    q.index = (q.index + count - 1) % count;
                    q.reveal_choice = true;
                }
                "DPadDown" | "LeftStickDown" => {
                    q.index = (q.index + 1) % count;
                    q.reveal_choice = true;
                }
                "South" => accept = true,
                "East" => cancel = true,
                _ => {}
            }
        }
        // Reserved inputs return to capture without requiring a pointer action.
        if let Some((input, long)) = &q.choice {
            let fixed = if controller {
                controller_fixed(input, self.gizmondo)
            } else {
                keyboard_shortcut(input).is_some() || (*long && input == "Escape")
            };
            if fixed {
                q.message = "This input is reserved by PocketHLE. Choose another input.".into();
                q.choice = None;
                q.pressed = None;
                accept = false;
            }
        }
        egui::Window::new("Assign an input")
            .id(egui::Id::new("input_assignment_assistant"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .movable(false)
            .default_width((ui.ctx().screen_rect().width() - 32.0).clamp(180.0, 460.0))
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if !q.message.is_empty() {
                    ui.colored_label(LIVE, &q.message);
                }
                if q.done {
                    ui.label(crate::controller_labels::text(ui.ctx(),"Up / Down to choose · Enter / {South} to continue"));
                    for (index, label) in [
                        "Assign another input",
                        "Save and finish",
                        "Back to overview",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if ui.selectable_label(index == q.index, *label).clicked() {
                            q.index = index;
                            accept = true;
                        }
                    }
                } else if let Some((input, long)) = &q.choice {
                    ui.heading(format!(
                        "{} · {} press",
                        if q.controller {crate::controller_labels::button(ui.ctx(),input,input)} else {input.clone()},
                        if *long { "Long" } else { "Short" }
                    ));
                    ui.label(crate::controller_labels::text(ui.ctx(),"Up / Down to choose · Enter / {South} to apply · Escape / {East} to cancel"));
                    if *long {
                        ui.label(
                            "Adding a long action delays this input's short action until release.",
                        );
                    }
                    if let Some(index) =
                        assignment_choices(ui, &targets, q.index, self.gizmondo, q.reveal_choice)
                            .inner
                    {
                        q.index = index;
                        accept = true;
                    }
                    if crate::controller_ui::button(ui, "Apply assignment").clicked() {
                        accept = true;
                    }
                } else {
                    ui.heading(if q.wait_launch_release {
                        "Release the shortcut buttons first…"
                    } else {
                        "Press and release a key or controller button…"
                    });
                    if let Some((name, at)) = &q.pressed {
                        ui.colored_label(
                            if at.elapsed().as_millis() >= 650 {
                                LONG_PRESS
                            } else {
                                LIVE
                            },
                            format!(
                                "{} · {} press — release to continue",
                                name,
                                if at.elapsed().as_millis() >= 650 {
                                    "Long"
                                } else {
                                    "Short"
                                }
                            ),
                        );
                    }
                }
                if crate::controller_ui::button(ui, "Cancel").clicked() {
                    cancel = true;
                }
            });
        q.reveal_choice = false;
        if accept && !cancel {
            if q.done {
                match q.index {
                    0 => {
                        q.done = false;
                        q.choice = None;
                        q.pressed = None;
                        q.index = 0;
                    }
                    1 => {
                        self.quick_save = true;
                        cancel = true;
                    }
                    _ => cancel = true,
                }
            } else if let Some((input, long)) = q.choice.clone() {
                let target = targets[q.index];
                let applied = if long {
                    let key = pocket_library::input_gestures::binding_key(
                        self.gizmondo,
                        controller,
                        &input,
                    );
                    if let Assignment::Guest(button) = target {
                        if controller && config.gamepad_fullscreen.as_deref() == Some(&input) {
                            config.gamepad_fullscreen = None;
                        }
                        config.long_press_bindings.insert(key, button);
                    } else {
                        config.long_press_bindings.remove(&key);
                    }
                    if controller {
                        self.control = Some(input.clone());
                    } else {
                        self.key = Some(input.clone());
                    }
                    true
                } else {
                    self.assign_short(config, &input, controller, target)
                };
                if applied {
                    q.message = format!(
                        "{} press: {} → {}",
                        if long { "Long" } else { "Short" },
                        if controller {crate::controller_labels::button(ui.ctx(),&input,&input)} else {input.clone()},
                        target.label_for(self.gizmondo)
                    );
                    q.done = true;
                    q.choice = None;
                    q.index = 0;
                }
            }
        }
        if !cancel {
            self.quick = Some(q);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    pub fn live_input(&mut self, device: usize, control: &str, down: bool) {
        let key = (device, control.to_owned());
        if down {
            self.live_controls.insert(key);
        } else {
            self.live_controls.remove(&key);
        }
        let launch_released = self
            .quick
            .as_ref()
            .and_then(|q| q.launch_device)
            .is_some_and(|id| {
                !self.live_controls.contains(&(id, "LeftThumb".into()))
                    && !self.live_controls.contains(&(id, "RightThumb".into()))
            });
        if let Some(q) = self.quick.as_mut() {
            if q.controller {
                if q.wait_launch_release {
                    if launch_released {
                        q.wait_launch_release = false;
                    }
                    return;
                }
                if q.choice.is_some() || q.done {
                    if down {
                        self.quick_nav.push(control.into());
                    }
                } else if down {
                    if q.pressed.is_none() {
                        q.pressed = Some((control.into(), std::time::Instant::now()));
                    }
                } else if q.pressed.as_ref().is_some_and(|(name, _)| name == control) {
                    let (name, at) = q.pressed.take().unwrap();
                    q.choice = Some((name, at.elapsed().as_millis() >= 650));
                    q.reveal_choice = true;
                }
            }
        }
    }
    pub fn clear_live(&mut self) {
        self.live_controls.clear();
        self.thumb_chord_latched.clear();
        self.quick = None;
        self.quick_nav.clear();
    }
    pub fn retain_live_devices(&mut self, devices: &[(usize, String)]) {
        self.thumb_chord_latched
            .retain(|id| devices.iter().any(|(device, _)| device == id));
        if self
            .live_controls
            .iter()
            .any(|(id, _)| !devices.iter().any(|(device, _)| device == id))
        {
            self.quick = None;
            self.quick_nav.clear();
        }
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
    pub fn reserved_input_attempt(&mut self) -> bool {
        if let Some(quick) = self.quick.as_mut().filter(|q| !q.wait_launch_release) {
            quick.message = "This input is reserved by PocketHLE. Choose another input.".into();
            return true;
        }
        false
    }
    pub fn is_gizmondo(&self) -> bool {
        self.gizmondo
    }
    /// Apply the assistant's short assignment and select the affected input.
    fn assign_short(
        &mut self,
        config: &mut LauncherConfig,
        input: &str,
        controller: bool,
        target: Assignment,
    ) -> bool {
        if self.gizmondo
            && matches!(
                target,
                Assignment::Guest(GuestButton::CenterTap | GuestButton::StylusClick)
            )
        {
            return false;
        }
        if (controller
            && (matches!(target, Assignment::Guest(GuestButton::StylusClick))
                || controller_fixed(input, self.gizmondo)
                || matches!(target, Assignment::Stylus(_))))
            || (!controller && keyboard_shortcut(input).is_some())
        {
            return false;
        }
        set_assignment(config, input, controller, self.gizmondo, target);
        if controller {
            self.control = Some(input.into());
        } else {
            self.key = Some(input.into());
        }
        true
    }

    pub fn profile(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Device profile");
            for (value, label) in [(false, "Pocket PC"), (true, "Gizmondo")] {
                if crate::controller_ui::selectable_value(ui, &mut self.gizmondo, value, label)
                    .changed()
                {
                    self.quick = None;
                }
            }
        });
        ui.label("Select an input to inspect its assignments. Use Assign an input to change them. Assignments apply to the selected device profile.");
    }

    pub fn show(&mut self, ui: &mut Ui, config: &LauncherConfig, controller: bool) {
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
        let summary_width = (ui.available_width() - diagram_width - 24.0).max(200.0);
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
                    ui.set_width(summary_width);
                    self.summary_card(ui, config, controller);
                });
            });
        } else {
            self.diagram(ui, config, controller);
            self.summary_card(ui, config, controller);
        }
        self.unassigned(ui, config, controller);
    }

    fn summary_card(&mut self, ui: &mut Ui, config: &LauncherConfig, controller: bool) {
        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(31, 35, 41))
            .stroke(Stroke::new(1.0, Color32::from_gray(65)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Selected input").strong());
                ui.add_space(8.0);
                self.summary(ui, config, controller)
            })
            .inner
    }

    fn summary(&self, ui: &mut Ui, config: &LauncherConfig, controller: bool) {
        let input = if controller {
            self.control.as_deref()
        } else {
            self.key.as_deref()
        };
        let Some(input) = input else {
            ui.label("Select an input on the diagram.");
            return;
        };
        let physical = if controller {
            controller_inputs()
                .into_iter()
                .find(|(name, _, _, _)| *name == input)
                .map(|(_, label, _, _)| label)
                .unwrap_or(input)
        } else {
            input
        };
        ui.heading(if controller {crate::controller_labels::button(ui.ctx(),input,physical)} else {physical.to_owned()});
        if controller && !self.gizmondo && pocket_library::stylus::controller_reserved(input, false)
        {
            ui.colored_label(GREEN, "Reserved stylus control");
            ui.label("Double-click R3 to switch stylus mode.");
            ui.label("Stick: automatic drags; single click unused.");
            ui.label("Pointer: move cursor; single click to tap.");
            return;
        }

        let shortcut = if controller {
            controller_shortcut(input, self.gizmondo)
        } else {
            keyboard_shortcut(input)
        };
        if let Some(shortcut) = shortcut {
            if controller {
                if let Some(action) = controller_tap(input) {
                    ui.colored_label(PURPLE, format!("Short press: {action}"));
                }
                if let Some(button) = config
                    .long_press_bindings
                    .get(&pocket_library::input_gestures::binding_key(
                        self.gizmondo,
                        true,
                        input,
                    ))
                    .filter(|_| !controller_fixed(input, self.gizmondo))
                {
                    ui.colored_label(
                        LONG_PRESS,
                        format!(
                            "Long press: {}",
                            Assignment::Guest(*button).label_for(self.gizmondo)
                        ),
                    );
                } else if let Some(action) = controller_hold(input, self.gizmondo) {
                    ui.colored_label(LONG_PRESS, format!("Long press: {action}"));
                }
            } else {
                ui.colored_label(PURPLE, format!("Short press: {shortcut}"));
            }
        } else if controller && config.gamepad_toggles_fullscreen(input) {
            ui.colored_label(PURPLE, "Short press: Fullscreen");
            ui.weak("Long press: Unassigned");
        } else {
            let current = if controller {
                controller_assignment(config, input, self.gizmondo)
            } else {
                keyboard_assignment(config, input, self.gizmondo)
            };
            ui.colored_label(
                current.color(),
                format!("Short press: {}", current.label_for(self.gizmondo)),
            );
            let key = pocket_library::input_gestures::binding_key(self.gizmondo, controller, input);
            if let Some(button) = config.long_press_bindings.get(&key) {
                ui.colored_label(
                    LONG_PRESS,
                    format!(
                        "Long press: {}",
                        Assignment::Guest(*button).label_for(self.gizmondo)
                    ),
                );
            } else {
                ui.weak("Long press: Unassigned");
            }
        }
        if !controller {
            if let Some(note) = keyboard_context_note(input) {
                ui.weak(note);
            }
        }
    }

    fn unassigned(&mut self, ui: &mut Ui, config: &LauncherConfig, controller: bool) {
        let mut inputs = configured_inputs(config, self.gizmondo, controller);
        if controller {
            inputs.extend(
                controller_inputs()
                    .iter()
                    .map(|(name, _, _, _)| name.to_string()),
            );
            inputs.extend(config.gamepad_bindings.keys().cloned());
        } else {
            inputs.extend(
                GuestButton::ALL
                    .iter()
                    .flat_map(|b| config.keybindings.keys_for(*b).iter().cloned()),
            );
            if !self.gizmondo {
                inputs.extend((1..=4).map(|n| n.to_string()));
                inputs.push("Tab".into());
            }
        }
        let prefix = pocket_library::input_gestures::binding_key(self.gizmondo, controller, "");
        let missing: Vec<_> = visible_buttons(self.gizmondo)
            .filter(|b| !controller || *b != GuestButton::StylusClick)
            .filter(|button| {
                if config.long_press_bindings.iter().any(|(key, b)| {
                    key.starts_with(&prefix)
                        && b == button
                        && if controller {
                            !controller_fixed(
                                key.strip_prefix(&prefix).unwrap_or(""),
                                self.gizmondo,
                            )
                        } else {
                            !pocket_library::keybindings::reserved_host_key(
                                key.strip_prefix(&prefix).unwrap_or(""),
                            )
                        }
                }) {
                    return false;
                }
                !inputs.iter().any(|name| {
                    if controller {
                        controller_shortcut(name, self.gizmondo).is_none()
                            && controller_assignment(config, name, self.gizmondo)
                                == Assignment::Guest(*button)
                    } else {
                        keyboard_shortcut(name).is_none()
                            && keyboard_assignment(config, name, self.gizmondo)
                                == Assignment::Guest(*button)
                    }
                })
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
                (RESERVED, "Reserved input"),
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
                    if self.live_control(name) && controller_fixed(name, self.gizmondo) {
                        let outline = Rect::from_center_size(
                            point,
                            Vec2::new(46.0, 24.0) * (art_width / 760.0),
                        );
                        ui.painter().rect(
                            outline,
                            10.0 * (art_width / 760.0),
                            Color32::from_rgb(30, 62, 72),
                            Stroke::new(1.5, RESERVED),
                        );
                    } else if self.live_control(name) {
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
                    let label = if *name == "RightThumb" && !self.gizmondo {
                        "Click / ×2"
                    } else if configured_fullscreen {
                        "Full"
                    } else if shortcut.is_some() {
                        match *name {
                            "Select" => "Full",
                            "Start" => "Filter",
                            _ => "Host",
                        }
                    } else {
                        assignment.short()
                    };
                    let label = if shortcut.is_none() && matches!(assignment, Assignment::Guest(_))
                    {
                        assignment.diagram_label(self.gizmondo)
                    } else {
                        label.to_owned()
                    };
                    let mut label_point = point;
                    if *name == "Misc1" {
                        label_point.y += 17.0;
                    } else if *name == "Mode" && shortcut.is_some() {
                        label_point.y += 26.0 * (art_width / 760.0);
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
                    if self.gizmondo || !name.starts_with("RightStick") {
                        ui.painter().text(
                            label_point,
                            Align2::CENTER_CENTER,
                            label,
                            FontId::proportional((art_width / 760.0 * 14.0).clamp(8.0, 12.0)),
                            if *name == "RightThumb" && !self.gizmondo {
                                GREEN
                            } else if shortcut.is_some() && controller_tap(name).is_none() {
                                LONG_PRESS
                            } else if shortcut.is_some() || configured_fullscreen {
                                PURPLE
                            } else {
                                assignment.color()
                            },
                        );
                    }
                    let custom_hold = (!controller_fixed(name, self.gizmondo))
                        .then(|| {
                            config
                                .long_press_bindings
                                .get(&pocket_library::input_gestures::binding_key(
                                    self.gizmondo,
                                    true,
                                    name,
                                ))
                                .map(|b| Assignment::Guest(*b).diagram_label(self.gizmondo))
                        })
                        .flatten();
                    if let Some(hold) = custom_hold
                        .as_deref()
                        .or_else(|| short_hold(name, self.gizmondo))
                    {
                        ui.painter().text(
                            point
                                + Vec2::new(
                                    0.0,
                                    if *name == "Misc1" {
                                        27.0
                                    } else {
                                        30.0 * art_width / 760.0
                                    },
                                ),
                            Align2::CENTER_CENTER,
                            format!("Hold: {hold}"),
                            FontId::proportional((8.5 * art_width / 760.0).clamp(7.0, 9.0)),
                            LONG_PRESS,
                        );
                    }
                    if *name == "RightThumb" && !self.gizmondo {
                        ui.painter().circle_stroke(
                            point,
                            (art_width * 0.058).max(14.0),
                            Stroke::new(1.5, RESERVED),
                        );
                    }
                    if matches!(*name, "Select" | "Start") {
                        let outline = Rect::from_center_size(
                            point,
                            Vec2::new(46.0, 24.0) * (art_width / 760.0),
                        );
                        ui.painter().rect_stroke(
                            outline,
                            10.0 * (art_width / 760.0),
                            Stroke::new(1.5, RESERVED),
                        );
                    }
                    let hit =
                        Rect::from_center_size(point, Vec2::splat((art_width * 0.045).max(14.0)));
                    let response =
                        ui.interact(hit, ui.id().with(("skin-hotspot", name)), Sense::click());
                    if response.clicked() {
                        self.control = Some((*name).into());
                    }
                    if active || response.hovered() {
                        if !controller_fixed(name, self.gizmondo) {
                            ui.painter().circle_stroke(
                                point,
                                (art_width * 0.023).max(7.0),
                                Stroke::new(1.5, BLUE),
                            );
                        }
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
        let display_label=if controller {crate::controller_labels::button(ui.ctx(),name,label)} else {label.to_owned()};
        let label=display_label.as_str();
        let reserved = !controller && pocket_library::keybindings::reserved_host_key(name);
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
        let color = if controller && name == "RightThumb" && !self.gizmondo {
            GREEN
        } else if shortcut.is_some() || fullscreen {
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
                    if controller_fixed(name, self.gizmondo) {
                        RESERVED
                    } else if pressed {
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
                    if reserved || pressed || is_selected {
                        2.0
                    } else {
                        1.0
                    },
                    if reserved {
                        RESERVED
                    } else if pressed {
                        LIVE
                    } else {
                        color
                    },
                ),
            );
        }
        let command = if fullscreen {
            "Fullscreen"
        } else if shortcut.is_some() {
            match name {
                "F7" => "Rotate",
                "F8" => "Library",
                "F9" => "Filter",
                "F10" => "Screenshot",
                "F11" => "Video",
                "F12" => "Full",
                "Select" => "Fullscreen",
                "Start" => "Filter",
                _ => "Host",
            }
        } else {
            assignment.short()
        };
        let command_label = if matches!(assignment, Assignment::Guest(_)) {
            assignment.diagram_label(self.gizmondo)
        } else {
            command.to_owned()
        };
        let command = if controller && name == "RightThumb" && !self.gizmondo {
            "Mode ×2"
        } else {
            command_label.as_str()
        };
        if controller {
            ui.painter().text(
                rect.left_center() + Vec2::new(6.0, 0.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                Color32::from_gray(210),
            );
            let custom_hold = (!controller_fixed(name, self.gizmondo))
                .then(|| {
                    config
                        .long_press_bindings
                        .get(&pocket_library::input_gestures::binding_key(
                            self.gizmondo,
                            true,
                            name,
                        ))
                        .map(|b| Assignment::Guest(*b).diagram_label(self.gizmondo))
                })
                .flatten();
            if let Some(hold) = custom_hold
                .as_deref()
                .or_else(|| short_hold(name, self.gizmondo))
            {
                if custom_hold.is_some() || controller_tap(name).is_some() {
                    ui.painter().text(
                        rect.right_top() + Vec2::new(-6.0, 6.0),
                        Align2::RIGHT_CENTER,
                        command,
                        FontId::proportional(9.0),
                        if custom_hold.is_some() { color } else { PURPLE },
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
        if !controller
            && config.long_press_bindings.contains_key(
                &pocket_library::input_gestures::binding_key(self.gizmondo, false, name),
            )
        {
            ui.painter()
                .circle_filled(rect.right_top() + Vec2::new(-5.0, 5.0), 2.5, LONG_PRESS);
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
            } else {
                self.key = Some(name.into());
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

    fn quick_frame(editor: &mut Editor, config: &mut LauncherConfig) {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| editor.quick_assign(ui, config, true));
        });
    }
    #[test]
    fn keyboard_factory_bindings_follow_the_selected_device_profile() {
        let mut config = LauncherConfig::default();
        for (key, button) in [
            ("1", GuestButton::PocketPcApp1),
            ("2", GuestButton::PocketPcApp2),
            ("3", GuestButton::PocketPcApp3),
            ("4", GuestButton::PocketPcApp4),
        ] {
            assert!(
                matches!(keyboard_assignment(&config,key,false),Assignment::Guest(b) if b==button)
            );
        }
        for key in ["F1", "F2", "F3", "F4", "F5"] {
            assert!(matches!(
                keyboard_assignment(&config, key, false),
                Assignment::Unbound
            ));
        }
        assert!(matches!(
            keyboard_assignment(&config, "T", true),
            Assignment::Unbound
        ));
        assert!(matches!(
            keyboard_assignment(&config, "F3", true),
            Assignment::Guest(GuestButton::GizPiano3)
        ));
        let key = pocket_library::input_gestures::binding_key(false, false, "1");
        config
            .short_press_bindings
            .insert(key, Some(GuestButton::Action));
        assert!(matches!(
            keyboard_assignment(&config, "1", false),
            Assignment::Guest(GuestButton::Action)
        ));
    }

    #[test]
    fn gizmondo_has_its_own_labels_and_no_extra_or_system_brightness() {
        for (button, label) in [
            (GuestButton::Action, "Play"),
            (GuestButton::ButtonA, "Stop"),
            (GuestButton::ButtonB, "Forward"),
            (GuestButton::ButtonC, "Rewind"),
            (GuestButton::Soft1, "L1"),
            (GuestButton::Soft2, "R1"),
        ] {
            assert_eq!(Assignment::Guest(button).label_for(true), label);
            assert_eq!(Assignment::Guest(button).diagram_label(true), label);
        }
        let config = LauncherConfig::default();
        assert!(matches!(
            controller_assignment(&config, "Misc1", true),
            Assignment::Guest(GuestButton::RotateScreen)
        ));
        assert!(matches!(
            controller_assignment(&config, "Misc1", false),
            Assignment::Guest(GuestButton::RotateScreen)
        ));
        assert_eq!(controller_hold("Select", true), Some("Return to library"));
        assert!(!controller_shortcut("Select", true)
            .unwrap()
            .contains("Brightness"));
    }

    #[test]
    fn diagram_captions_are_compact_while_command_names_stay_explicit() {
        for (button, caption) in [
            (GuestButton::Action, "Act"),
            (GuestButton::Soft1, "Tab"),
            (GuestButton::Soft2, "Esc"),
            (GuestButton::DpadLeft, "Left"),
            (GuestButton::DpadRight, "Right"),
        ] {
            assert_eq!(Assignment::Guest(button).diagram_label(false), caption);
        }
        assert_eq!(
            Assignment::Guest(GuestButton::Action).label_for(false),
            "ACTION"
        );
    }

    #[test]
    fn guide_tab_and_extra_rotation_are_customizable() {
        let mut config = LauncherConfig::default();
        let mut editor = Editor::default();
        assert!(matches!(
            controller_assignment(&config, "Mode", false),
            Assignment::Guest(GuestButton::Soft1)
        ));
        assert!(matches!(
            controller_assignment(&config, "Misc1", false),
            Assignment::Guest(GuestButton::RotateScreen)
        ));
        for input in ["Mode", "Misc1"] {
            assert!(editor.assign_short(&mut config, input, true, Assignment::Unbound));
        }
        assert!(matches!(
            controller_assignment(&config, "Mode", false),
            Assignment::Unbound
        ));
        assert!(visible_buttons(false).any(|b| b == GuestButton::Screenshot));
        assert!(visible_buttons(true).any(|b| b == GuestButton::Screenshot));
    }

    #[test]
    fn pocketpc_command_labels_and_unassigned_extra_match_the_guest_codes() {
        use GuestButton::*;
        for (button, label) in [
            (Action, "ACTION"),
            (Soft1, "TAB"),
            (Soft2, "ESC"),
            (Turbo, "TURBO"),
            (ButtonA, "A"),
            (ButtonB, "B"),
            (ButtonC, "C"),
            (PocketPcApp1, "P1"),
            (PocketPcApp2, "P2"),
            (PocketPcApp3, "P3"),
            (PocketPcApp4, "P4"),
        ] {
            assert_eq!(Assignment::Guest(button).label_for(false), label);
        }
        assert_eq!(Soft1.vk(), 0x09);
        assert_eq!(Soft2.vk(), 0x1b);
        let config = LauncherConfig::default();
        assert!(matches!(
            controller_assignment(&config, "Misc1", false),
            Assignment::Guest(GuestButton::RotateScreen)
        ));
        assert!(!controller_fixed("Misc1", false));
        assert_eq!(controller_tap("Mode"), None);
        assert_eq!(controller_hold("Mode", false), None);
        assert_eq!(controller_hold("Mode", true), None);
    }

    #[test]
    fn function_buttons_ignore_legacy_short_and_long_guest_assignments() {
        for gizmondo in [false, true] {
            let mut config = LauncherConfig::default();
            let mut editor = Editor::default();
            editor.gizmondo = gizmondo;
            for control in ["Select", "Start"] {
                let key = pocket_library::input_gestures::binding_key(gizmondo, true, control);
                config
                    .short_press_bindings
                    .insert(key.clone(), Some(GuestButton::ButtonA));
                config.long_press_bindings.insert(key, GuestButton::ButtonB);
                assert!(controller_fixed(control, gizmondo));
                assert!(matches!(
                    controller_assignment(&config, control, gizmondo),
                    Assignment::Unbound
                ));
                assert!(!editor.assign_short(
                    &mut config,
                    control,
                    true,
                    Assignment::Guest(GuestButton::Action)
                ));
            }
        }
    }

    #[test]
    fn assistant_is_centered_when_opened_and_after_viewport_resize() {
        let ctx = egui::Context::default();
        let mut e = Editor::default();
        let mut c = LauncherConfig::default();
        e.quick = Some(QuickAssign::default());
        for size in [Vec2::new(800.0, 600.0), Vec2::new(1200.0, 800.0)] {
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |ctx| {
                        crate::controller_ui::begin_frame(ctx);
                        egui::CentralPanel::default()
                            .show(ctx, |ui| e.quick_assign(ui, &mut c, false));
                    },
                );
            }
            let rect = ctx
                .memory(|m| m.area_rect(egui::Id::new("input_assignment_assistant")))
                .unwrap();
            assert!(
                (rect.center() - egui::pos2(size.x / 2.0, size.y / 2.0)).length() < 2.0,
                "{rect:?} in {size:?}"
            );
        }
    }
    #[test]
    fn mouse_wheel_does_not_snap_command_list_back_to_selected_top_row() {
        let ctx = egui::Context::default();
        let targets: Vec<_> = GuestButton::ALL
            .into_iter()
            .chain(GuestButton::ALL)
            .map(Assignment::Guest)
            .collect();
        let mut offset = 0.0;
        for frame in 0..15 {
            let events = if frame == 2 {
                vec![
                    egui::Event::PointerMoved(egui::pos2(100.0, 100.0)),
                    egui::Event::Scroll(Vec2::new(0.0, -180.0)),
                ]
            } else {
                vec![]
            };
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    time: Some(frame as f64 / 60.0),
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        offset = assignment_choices(ui, &targets, 0, false, frame == 0)
                            .state
                            .offset
                            .y
                    });
                },
            );
        }
        assert!(offset > 100.0, "wheel position snapped to {offset}");
    }
    #[test]
    fn reserved_function_keys_reject_all_assignments_and_ignore_stale_overrides() {
        let mut e = Editor::default();
        let mut c = LauncherConfig::default();
        for key in ["F7", "F8", "F9", "F10", "F11", "F12"] {
            let id = pocket_library::input_gestures::binding_key(false, false, key);
            c.short_press_bindings
                .insert(id.clone(), Some(GuestButton::Action));
            c.long_press_bindings.insert(id, GuestButton::ButtonA);
            assert!(!e.assign_short(&mut c, key, false, Assignment::Guest(GuestButton::Action)));
            assert_eq!(keyboard_assignment(&c, key, false), Assignment::Unbound);
            assert!(keyboard_shortcut(key).is_some());
        }
    }
    #[test]
    fn controller_launch_chord_waits_for_both_releases_without_binding_them() {
        let mut e = Editor::default();
        e.live_input(1, "LeftThumb", true);
        e.controller_shortcut(1);
        assert!(!e.quick_active());
        e.live_input(1, "RightThumb", true);
        e.controller_shortcut(1);
        assert!(e.quick.as_ref().unwrap().wait_launch_release);
        e.live_input(1, "LeftThumb", false);
        e.controller_shortcut(1);
        assert!(e.quick.as_ref().unwrap().wait_launch_release);
        e.live_input(1, "South", true);
        assert!(e.quick.as_ref().unwrap().pressed.is_none());
        e.live_input(1, "South", false);
        e.live_input(1, "RightThumb", false);
        e.controller_shortcut(1);
        assert!(!e.quick.as_ref().unwrap().wait_launch_release);
        assert!(e.quick.as_ref().unwrap().choice.is_none());
        e.live_input(1, "DPadDown", true);
        e.live_input(1, "DPadDown", false);
        assert_eq!(
            e.quick.as_ref().unwrap().choice,
            Some(("DPadDown".into(), false))
        );
    }
    #[test]
    fn controller_launch_requires_one_device_and_rearms_only_after_full_release() {
        let mut e = Editor::default();
        e.live_input(1, "LeftThumb", true);
        e.controller_shortcut(1);
        e.live_input(2, "RightThumb", true);
        e.controller_shortcut(2);
        assert!(!e.quick_active());
        e.live_input(1, "RightThumb", true);
        e.controller_shortcut(1);
        assert!(e.quick_active());
        e.quick = None;
        e.controller_shortcut(1);
        assert!(!e.quick_active());
        e.live_input(1, "LeftThumb", false);
        e.controller_shortcut(1);
        e.live_input(1, "LeftThumb", true);
        e.controller_shortcut(1);
        assert!(!e.quick_active());
        e.live_input(1, "LeftThumb", false);
        e.controller_shortcut(1);
        e.live_input(1, "RightThumb", false);
        e.controller_shortcut(1);
        e.live_input(1, "LeftThumb", true);
        e.controller_shortcut(1);
        e.live_input(1, "RightThumb", true);
        e.controller_shortcut(1);
        assert!(e.quick_active());
        e.clear_live();
        assert!(!e.quick_active());
        assert!(e.thumb_chord_latched.is_empty());
    }
    #[test]
    fn ctrl_enter_starts_keyboard_assistant_and_is_not_captured() {
        let ctx = egui::Context::default();
        let mut e = Editor::default();
        let mut c = LauncherConfig::default();
        let ctrl = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed,
            repeat: false,
            modifiers: ctrl,
        };
        let frame = |e: &mut Editor, c: &mut LauncherConfig, mods, events, controller| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    modifiers: mods,
                    ..Default::default()
                },
                |ctx| {
                    e.keyboard_shortcut(ctx, controller);
                    egui::CentralPanel::default().show(ctx, |ui| e.quick_assign(ui, c, controller));
                },
            );
        };
        frame(&mut e, &mut c, ctrl, vec![key(true)], true);
        assert!(!e.quick_active());
        frame(&mut e, &mut c, ctrl, vec![key(false)], false);
        frame(&mut e, &mut c, ctrl, vec![key(true)], false);
        assert!(e.quick.as_ref().unwrap().wait_launch_release);
        assert!(e.quick.as_ref().unwrap().pressed.is_none());
        frame(&mut e, &mut c, ctrl, vec![key(false)], false);
        assert!(e.quick.as_ref().unwrap().wait_launch_release);
        frame(&mut e, &mut c, Default::default(), vec![], false);
        assert!(!e.quick.as_ref().unwrap().wait_launch_release);
        assert!(e.quick.as_ref().unwrap().choice.is_none());
        for pressed in [true, false] {
            frame(
                &mut e,
                &mut c,
                Default::default(),
                vec![egui::Event::Key {
                    key: egui::Key::X,
                    physical_key: Some(egui::Key::X),
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                false,
            );
        }
        assert_eq!(e.quick.as_ref().unwrap().choice, Some(("X".into(), false)));
    }
    #[test]
    fn assistant_keeps_supported_stylus_assignments_without_a_second_editor() {
        assert!(assignment_targets(false, false, Some("O"), false).contains(&Assignment::Stylus(0)));
        assert!(
            !assignment_targets(false, true, Some("RightStickUp"), false)
                .contains(&Assignment::Stylus(0))
        );
        for (giz, pad, name, long) in [
            (true, false, "O", false),
            (false, false, "O", true),
            (false, true, "South", false),
        ] {
            assert!(!assignment_targets(giz, pad, Some(name), long)
                .iter()
                .any(|target| matches!(target, Assignment::Stylus(_))));
        }
    }
    #[test]
    fn keyboard_wizard_starts_from_button_then_uses_only_keys() {
        let ctx = egui::Context::default();
        let mut e = Editor::default();
        let mut c = LauncherConfig::default();
        let frame = |e: &mut Editor, c: &mut LauncherConfig, events: Vec<egui::Event>| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| e.quick_assign(ui, c, false));
                },
            );
        };
        frame(&mut e, &mut c, vec![]);
        for down in [true, false] {
            frame(
                &mut e,
                &mut c,
                vec![
                    egui::Event::PointerMoved(egui::pos2(100.0, 24.0)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(100.0, 24.0),
                        button: egui::PointerButton::Primary,
                        pressed: down,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert!(e.quick_active());
        for key in [
            egui::Key::X,
            egui::Key::ArrowDown,
            egui::Key::Enter,
            egui::Key::ArrowDown,
            egui::Key::Enter,
        ] {
            for down in [true, false] {
                frame(
                    &mut e,
                    &mut c,
                    vec![egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: down,
                        repeat: false,
                        modifiers: Default::default(),
                    }],
                );
            }
        }
        assert!(e.take_quick_save());
        assert!(!e.quick_active());
        assert_eq!(
            keyboard_assignment(&c, "X", false),
            Assignment::Guest(GuestButton::DpadUp)
        );
    }
    #[test]
    fn wizard_can_assign_and_save_without_pointer_after_start() {
        let mut editor = Editor::default();
        let mut config = LauncherConfig::default();
        editor.quick = Some(QuickAssign {
            controller: true,
            ..Default::default()
        });
        editor.live_input(1, "DPadUp", true);
        editor.live_input(1, "DPadUp", false);
        assert_eq!(
            editor.quick.as_ref().unwrap().choice,
            Some(("DPadUp".into(), false))
        );
        editor.live_input(1, "DPadDown", true);
        editor.live_input(1, "DPadDown", false);
        quick_frame(&mut editor, &mut config);
        editor.live_input(1, "South", true);
        editor.live_input(1, "South", false);
        quick_frame(&mut editor, &mut config);
        assert_eq!(
            controller_assignment(&config, "DPadUp", false),
            Assignment::Guest(GuestButton::DpadUp)
        );
        assert!(editor.quick.as_ref().unwrap().done);
        editor.live_input(1, "DPadDown", true);
        editor.live_input(1, "DPadDown", false);
        quick_frame(&mut editor, &mut config);
        editor.live_input(1, "South", true);
        editor.live_input(1, "South", false);
        quick_frame(&mut editor, &mut config);
        assert!(editor.take_quick_save());
        assert!(!editor.quick_active());
    }
    #[test]
    fn wizard_long_is_profile_specific_and_reserved_recaptures() {
        let mut editor = Editor::default();
        let mut config = LauncherConfig::default();
        editor.quick = Some(QuickAssign {
            controller: true,
            pressed: Some((
                "DPadUp".into(),
                std::time::Instant::now() - std::time::Duration::from_millis(700),
            )),
            ..Default::default()
        });
        editor.live_input(1, "DPadUp", false);
        editor.quick.as_mut().unwrap().index = 1;
        editor.live_input(1, "South", true);
        editor.live_input(1, "South", false);
        quick_frame(&mut editor, &mut config);
        assert_eq!(
            config.long_press_bindings.get("pocketpc:controller:DPadUp"),
            Some(&GuestButton::DpadUp)
        );
        assert!(!config
            .long_press_bindings
            .contains_key("gizmondo:controller:DPadUp"));
        editor.quick = Some(QuickAssign {
            controller: true,
            choice: Some(("Start".into(), true)),
            ..Default::default()
        });
        quick_frame(&mut editor, &mut config);
        assert!(editor.quick.as_ref().unwrap().choice.is_none());
        assert!(!config
            .long_press_bindings
            .contains_key("pocketpc:controller:Start"));
        editor.clear_live();
        assert!(!editor.quick_active());
    }
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
        assert_eq!(keyboard_assignment(&config, "T", true), Assignment::Unbound);
        assert_eq!(controller_hold("Select", true), Some("Return to library"));
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
            Assignment::Guest(GuestButton::Action)
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
        assert!(editor.assign_short(
            &mut config,
            "O",
            false,
            Assignment::Guest(GuestButton::Action)
        ));
        assert_eq!(
            keyboard_assignment(&config, "O", false),
            Assignment::Guest(GuestButton::Action)
        );
        assert_eq!(keyboard_assignment(&config, "O", true), Assignment::Unbound);
        assert_eq!(editor.key.as_deref(), Some("O"));
        config.gamepad_fullscreen = Some("RightStickUp".into());
        let before = serde_json::to_value(&config).unwrap();
        for input in [
            "RightThumb",
            "RightStickUp",
            "RightStickDown",
            "RightStickLeft",
            "RightStickRight",
        ] {
            assert!(!editor.assign_short(
                &mut config,
                input,
                true,
                Assignment::Guest(GuestButton::Turbo)
            ));
            assert!(!editor.assign_short(&mut config, input, true, Assignment::Stylus(1)));
        }
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
        assert_eq!(
            controller_assignment(&config, "RightStickUp", false),
            Assignment::Stylus(0)
        );
    }

    #[test]
    fn capture_keeps_reserved_inputs_unchanged() {
        let mut config = LauncherConfig::default();
        let before = serde_json::to_value(&config).unwrap();
        let mut editor = Editor::default();
        for input in ["Start", "Select"] {
            assert!(!editor.assign_short(
                &mut config,
                input,
                true,
                Assignment::Guest(GuestButton::Turbo)
            ));
        }
        assert!(!editor.assign_short(&mut config, "F12", false, Assignment::Stylus(0)));
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
    }

    #[test]
    fn controller_hold_captions_do_not_overlap_function_button_labels() {
        for width in [400.0, 820.0, 1280.0] {
            let ctx = egui::Context::default();
            let mut e = Editor::default();
            let c = LauncherConfig::default();
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 1000.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| e.show(ui, &c, true));
                },
            );
            let texts: Vec<_> = out
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some((t.galley.text(), Rect::from_min_size(t.pos, t.galley.size())))
                    } else {
                        None
                    }
                })
                .collect();
            for (caption, hold) in [("Full", "Hold: Exit"), ("Filter", "Hold: Record")] {
                let function = texts.iter().find(|(text, _)| *text == caption).unwrap().1;
                let label = texts
                    .iter()
                    .filter(|(text, _)| *text == hold)
                    .min_by(|a, b| {
                        (a.1.center().x - function.center().x)
                            .abs()
                            .total_cmp(&(b.1.center().x - function.center().x).abs())
                    })
                    .unwrap()
                    .1;
                assert!(
                    !function.intersects(label),
                    "{caption} overlaps {hold} at width {width}"
                );
                assert!(label.top() > function.bottom());
            }
        }
    }
    #[test]
    fn keyboard_click_mode_and_stylus_directions_can_be_reassigned() {
        let mut c = LauncherConfig::default();
        let mut e = Editor::default();
        assert_eq!(
            keyboard_assignment(&c, "P", false),
            Assignment::Guest(GuestButton::StylusClick)
        );
        assert!(e.assign_short(&mut c, "P", false, Assignment::Unbound));
        assert!(e.assign_short(
            &mut c,
            "J",
            false,
            Assignment::Guest(GuestButton::StylusClick)
        ));
        assert!(e.assign_short(&mut c, "I", false, Assignment::Stylus(0)));
        assert!(!c.stylus_keyboard_reserved("P"));
        assert!(c.stylus_keyboard_reserved("J"));
        assert!(c.stylus_keyboard_reserved("I"));
        assert!(e.assign_short(&mut c, "O", false, Assignment::Unbound));
        assert!(!c.stylus_keyboard_reserved("O"));
        assert!(!assignment_targets(false, true, Some("South"), false)
            .contains(&Assignment::Guest(GuestButton::StylusClick)));
        assert!(!assignment_targets(false, false, Some("J"), true)
            .contains(&Assignment::Guest(GuestButton::StylusClick)));
    }
    #[test]
    fn reserved_stylus_input_card_explains_mode_switch_and_ignores_stale_overrides() {
        let ctx = egui::Context::default();
        let mut editor = Editor::default();
        editor.control = Some("RightThumb".into());
        let mut config = LauncherConfig::default();
        config.short_press_bindings.insert(
            pocket_library::input_gestures::binding_key(false, true, "RightThumb"),
            Some(GuestButton::Soft1),
        );
        assert_eq!(
            controller_assignment(&config, "RightThumb", false),
            Assignment::Unbound
        );
        assert!(!controller_fixed("RightThumb", true));
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1280., 720.),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.show(ui, &config, true));
            },
        );
        fn has(shapes: &[egui::epaint::ClippedShape], text: &str) -> bool {
            shapes
                .iter()
                .any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains(text)))
        }
        assert!(has(&out.shapes, "Double-click R3 to switch stylus mode."));
        assert!(has(&out.shapes, "Mode ×2"));
        assert!(has(&out.shapes, "Tab"));
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
