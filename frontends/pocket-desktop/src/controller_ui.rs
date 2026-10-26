//! Controller navigation for launcher widgets. Guest input never uses this path.
use eframe::egui::{self, Event, Id, Key, Modifiers, Response, Ui, WidgetText};
use std::{
    collections::HashMap,
    hash::Hash,
    time::{Duration, Instant},
};

fn state_id() -> Id {
    Id::new("launcher_controller_widgets")
}
#[derive(Clone, Default)]
struct Widgets {
    text: HashMap<Id, String>,
    committed: HashMap<Id, String>,
    popup: Option<Popup>,
    collecting: Option<Vec<(Id, bool)>>,
}
#[derive(Clone)]
struct Popup {
    owner: Id,
    items: Vec<Id>,
}
fn widgets(ctx: &egui::Context) -> Widgets {
    ctx.data_mut(|d| d.get_temp::<Widgets>(state_id()).unwrap_or_default())
}
fn edit_widgets(ctx: &egui::Context, edit: impl FnOnce(&mut Widgets)) {
    ctx.data_mut(|d| {
        let mut state = d.get_temp::<Widgets>(state_id()).unwrap_or_default();
        edit(&mut state);
        d.insert_temp(state_id(), state);
    });
}

#[derive(Default)]
pub struct Navigation {
    pending: Vec<String>,
    held: Option<(String, Instant)>,
    keyboard: Option<Keyboard>,
    used: bool,
}
struct Keyboard {
    field: Id,
    value: String,
    index: usize,
    upper: bool,
    symbols: bool,
}
impl Navigation {
    pub fn press(&mut self, control: &str) {
        self.used = true;
        self.pending.push(control.into());
        if crate::deck_navigation::is_direction(control) {
            self.held = Some((control.into(), Instant::now() + Duration::from_millis(350)));
        }
    }
    pub fn release(&mut self, control: &str) {
        if self.held.as_ref().is_some_and(|(c, _)| c == control) {
            self.held = None;
        }
    }
    pub fn keyboard_open(&self) -> bool {
        self.keyboard.is_some()
    }
    pub fn back(&mut self, ctx: &egui::Context) -> bool {
        self.held = None;
        self.pending.clear();
        if self.keyboard.take().is_some() {
            return true;
        }
        if ctx.memory(|m| m.any_popup_open()) {
            let owner = widgets(ctx).popup.map(|p| p.owner);
            ctx.memory_mut(|m| {
                m.close_popup();
                if let Some(owner) = owner {
                    m.request_focus(owner);
                }
            });
            edit_widgets(ctx, |w| w.popup = None);
            return true;
        }
        false
    }
    pub fn prepare_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput, enabled: bool) {
        if !enabled || !raw.focused {
            self.pending.clear();
            self.held = None;
            if !enabled {
                self.keyboard = None;
            }
            return;
        }
        if let Some((control, next)) = &mut self.held {
            if Instant::now() >= *next {
                self.pending.push(control.clone());
                *next = Instant::now() + Duration::from_millis(120);
            }
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        for control in std::mem::take(&mut self.pending) {
            if self.keyboard.is_some() {
                self.keyboard_control(&control, ctx);
                continue;
            }
            let w = widgets(ctx);
            let focused = ctx.memory(|m| m.focused());
            if let Some(popup) = w.popup.filter(|_| ctx.memory(|m| m.any_popup_open())) {
                if let Some(next) = crate::deck_navigation::menu_index(
                    focused
                        .and_then(|id| popup.items.iter().position(|i| *i == id))
                        .unwrap_or(0),
                    &control,
                    popup.items.len(),
                ) {
                    ctx.memory_mut(|m| m.request_focus(popup.items[next]));
                    continue;
                }
            }
            match control.as_str() {
                "Escape" => key(raw, Key::Escape, false),
                "South" => {
                    if let Some((field, value)) =
                        focused.and_then(|id| w.text.get(&id).cloned().map(|v| (id, v)))
                    {
                        self.keyboard = Some(Keyboard {
                            field,
                            value,
                            index: 0,
                            upper: false,
                            symbols: false,
                        });
                    } else {
                        key(raw, Key::Enter, false);
                    }
                }
                "DPadUp" | "LeftStickUp" => key(raw, Key::Tab, true),
                "DPadDown" | "LeftStickDown" => key(raw, Key::Tab, false),
                "DPadLeft" | "LeftStickLeft" => key(raw, Key::ArrowLeft, false),
                "DPadRight" | "LeftStickRight" => key(raw, Key::ArrowRight, false),
                _ => {}
            }
        }
    }
    fn keyboard_control(&mut self, control: &str, ctx: &egui::Context) {
        let k = self.keyboard.as_mut().unwrap();
        if control == "East" {
            self.keyboard = None;
            return;
        }
        if let Some(index) = crate::deck_navigation::next_index(k.index, 46, 10, control) {
            k.index = index;
        }
        if control == "North" {
            k.symbols = !k.symbols;
        }
        if control == "South" {
            self.keyboard_action(ctx);
        }
    }
    fn keyboard_action(&mut self, ctx: &egui::Context) {
        let k = self.keyboard.as_mut().unwrap();
        match k.index {
            0..=39 => {
                let c = (if k.symbols { SYMBOLS } else { KEYS }).as_bytes()[k.index] as char;
                k.value
                    .push(if k.upper { c.to_ascii_uppercase() } else { c });
            }
            40 => k.upper = !k.upper,
            41 => k.value.push(' '),
            42 => {
                k.value.pop();
            }
            43 => k.value.clear(),
            44 => {
                let k = self.keyboard.take().unwrap();
                edit_widgets(ctx, |w| {
                    w.committed.insert(k.field, k.value);
                });
                ctx.memory_mut(|m| m.request_focus(k.field));
            }
            45 => {
                self.keyboard = None;
            }
            _ => {}
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        if self.used {
            if let Some(response) = ctx
                .memory(|m| m.focused())
                .and_then(|id| ctx.read_response(id))
            {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    Id::new("controller_focus"),
                ));
                painter.rect_stroke(
                    response.rect.expand(2.0),
                    3.0,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(80, 190, 255)),
                );
            }
        }
        if self.keyboard.is_none() {
            return;
        }
        let mut open = true;
        let mut clicked = None;
        egui::Window::new("Keyboard — A: select · B: cancel · Y: symbols")
            .id(Id::new("controller_keyboard"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                let k = self.keyboard.as_ref().unwrap();
                ui.label(&k.value);
                ui.separator();
                egui::Grid::new("controller_keys")
                    .spacing(egui::Vec2::splat(5.0))
                    .show(ui, |ui| {
                        for index in 0..46 {
                            let label = if index < 40 {
                                let c = (if k.symbols { SYMBOLS } else { KEYS }).as_bytes()[index]
                                    as char;
                                if k.upper {
                                    c.to_ascii_uppercase().to_string()
                                } else {
                                    c.to_string()
                                }
                            } else {
                                ["Shift", "Space", "⌫", "Clear", "Apply", "Cancel"][index - 40]
                                    .into()
                            };
                            if ui
                                .add_sized(
                                    [48.0, 30.0],
                                    egui::SelectableLabel::new(k.index == index, label),
                                )
                                .clicked()
                            {
                                clicked = Some(index);
                            }
                            if (index + 1) % 10 == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
        if !open {
            self.keyboard = None;
        } else if let Some(index) = clicked {
            self.keyboard.as_mut().unwrap().index = index;
            self.keyboard_action(ctx);
        }
    }
}
const KEYS: &str = "1234567890qwertyuiopasdfghjkl-zxcvbnm_@.";
const SYMBOLS: &str = r#"!@#$%^&*()[]{}<>/\:;"'=+?,`~|-_. 0123456"#;
fn key(raw: &mut egui::RawInput, key: Key, shift: bool) {
    let modifiers = Modifiers {
        shift,
        ..Default::default()
    };
    raw.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    raw.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    });
}

pub fn text_edit(ui: &mut Ui, value: &mut String, hint: &str) -> Response {
    // Determine the widget ID before drawing so a committed keyboard edit is observed by callers.
    let id = ui.next_auto_id();
    let committed = ui.ctx().data_mut(|d| {
        let mut w = d.get_temp::<Widgets>(state_id()).unwrap_or_default();
        let v = w.committed.remove(&id);
        d.insert_temp(state_id(), w);
        v
    });
    let changed = committed.is_some();
    if let Some(text) = committed {
        *value = text;
    }
    let mut response = ui.add(egui::TextEdit::singleline(value).id(id).hint_text(hint));
    if changed {
        response.mark_changed();
    }
    edit_widgets(ui.ctx(), |w| {
        w.text.insert(response.id, value.clone());
    });
    if response.has_focus() {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    response
}

/// Preserve egui's real combo box, including mouse/keyboard behavior, while recording its focus scope.
pub struct ComboBox(egui::ComboBox);
impl ComboBox {
    pub fn from_id_source(source: impl Hash) -> Self {
        Self(egui::ComboBox::from_id_source(source))
    }
    pub fn selected_text(self, text: impl Into<WidgetText>) -> Self {
        Self(self.0.selected_text(text))
    }
    pub fn show_ui<R>(
        self,
        ui: &mut Ui,
        contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<Option<R>> {
        let ctx = ui.ctx().clone();
        let old = widgets(&ctx).popup;
        edit_widgets(&ctx, |w| w.collecting = Some(Vec::new()));
        let response = self.0.show_ui(ui, contents);
        let entries = widgets(&ctx).collecting.unwrap_or_default();
        edit_widgets(&ctx, |w| w.collecting = None);
        if response.inner.is_some() && ctx.memory(|m| m.any_popup_open()) {
            let items: Vec<_> = entries.iter().map(|(id, _)| *id).collect();
            if old
                .as_ref()
                .is_none_or(|p| p.owner != response.response.id || p.items.is_empty())
            {
                if let Some((id, _)) = entries
                    .iter()
                    .find(|(_, selected)| *selected)
                    .or(entries.first())
                {
                    ctx.memory_mut(|m| m.request_focus(*id));
                }
            }
            edit_widgets(&ctx, |w| {
                w.popup = Some(Popup {
                    owner: response.response.id,
                    items,
                })
            });
        } else if old.is_some_and(|p| p.owner == response.response.id) {
            edit_widgets(&ctx, |w| w.popup = None);
        }
        if response.response.has_focus() {
            response.response.scroll_to_me(Some(egui::Align::Center));
        }
        response
    }
}
pub fn selectable_value<T: PartialEq>(
    ui: &mut Ui,
    current: &mut T,
    value: T,
    text: impl Into<WidgetText>,
) -> Response {
    let selected = *current == value;
    let response = ui.selectable_value(current, value, text);
    let collecting = widgets(ui.ctx()).collecting.is_some();
    if collecting && response.enabled() {
        edit_widgets(ui.ctx(), |w| {
            if let Some(entries) = &mut w.collecting {
                entries.push((response.id, selected));
            }
        });
        if response.has_focus() {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        if response.clicked() {
            let owner = widgets(ui.ctx()).popup.map(|p| p.owner);
            ui.memory_mut(|m| {
                m.close_popup();
                if let Some(owner) = owner {
                    m.request_focus(owner);
                }
            });
        }
    }
    response
}

pub fn begin_frame(ctx: &egui::Context) {
    let open = ctx.memory(|m| m.any_popup_open());
    edit_widgets(ctx, |w| {
        w.text.clear();
        if !open {
            w.popup = None;
        }
    });
}

pub fn scroll_focus(ui: &Ui) {
    if let Some(response) = ui
        .ctx()
        .memory(|m| m.focused())
        .and_then(|id| ui.ctx().read_response(id))
    {
        if response.gained_focus() {
            response.scroll_to_me(Some(egui::Align::Center));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(ctx: &egui::Context, nav: &mut Navigation, ui: impl FnOnce(&mut Ui)) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        nav.prepare_input(ctx, &mut raw, true);
        let _ = ctx.run(raw, |ctx| {
            begin_frame(ctx);
            egui::CentralPanel::default().show(ctx, ui);
        });
    }
    #[test]
    fn controller_a_toggles_real_checkbox_and_down_moves_focus() {
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let mut checked = false;
        let mut checkbox = Id::NULL;
        let mut button = Id::NULL;
        frame(&ctx, &mut nav, |ui| {
            checkbox = ui.checkbox(&mut checked, "Enabled").id;
            button = ui.button("Save").id;
        });
        ctx.memory_mut(|m| m.request_focus(checkbox));
        nav.press("South");
        frame(&ctx, &mut nav, |ui| {
            ui.checkbox(&mut checked, "Enabled");
            let _ = ui.button("Save");
        });
        assert!(checked);
        nav.press("South");
        frame(&ctx, &mut nav, |ui| {
            ui.checkbox(&mut checked, "Enabled");
            let _ = ui.button("Save");
        });
        assert!(!checked);
        nav.press("DPadDown");
        nav.release("DPadDown");
        frame(&ctx, &mut nav, |ui| {
            ui.checkbox(&mut checked, "Enabled");
            let _ = ui.button("Save");
        });
        assert_eq!(ctx.memory(|m| m.focused()), Some(button));
    }
    #[test]
    fn controller_combobox_selects_closes_and_b_restores_owner_without_changing_value() {
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let mut value = 1;
        let mut owner = Id::NULL;
        let draw = |ui: &mut Ui, value: &mut i32| {
            ComboBox::from_id_source("quality")
                .selected_text(value.to_string())
                .show_ui(ui, |ui| {
                    for i in 0..3 {
                        selectable_value(ui, value, i, i.to_string());
                    }
                })
                .response
                .id
        };
        frame(&ctx, &mut nav, |ui| owner = draw(ui, &mut value));
        ctx.memory_mut(|m| m.request_focus(owner));
        nav.press("South");
        frame(&ctx, &mut nav, |ui| {
            draw(ui, &mut value);
        });
        assert!(ctx.memory(|m| m.any_popup_open()));
        // egui sizes a newly created popup in a disabled first pass.
        frame(&ctx, &mut nav, |ui| {
            draw(ui, &mut value);
        });
        let popup = widgets(&ctx).popup.unwrap();
        assert_eq!(ctx.memory(|m| m.focused()), Some(popup.items[1]));
        nav.press("DPadDown");
        nav.release("DPadDown");
        frame(&ctx, &mut nav, |ui| {
            draw(ui, &mut value);
        });
        nav.press("South");
        frame(&ctx, &mut nav, |ui| {
            draw(ui, &mut value);
        });
        assert_eq!(value, 2);
        assert!(!ctx.memory(|m| m.any_popup_open()));
        assert_eq!(ctx.memory(|m| m.focused()), Some(owner));
        nav.press("South");
        frame(&ctx, &mut nav, |ui| {
            draw(ui, &mut value);
        });
        assert!(nav.back(&ctx));
        assert_eq!(value, 2);
        assert_eq!(ctx.memory(|m| m.focused()), Some(owner));
        assert!(!nav.back(&ctx));
    }
    #[test]
    fn b_closes_a_native_toolbar_menu_before_leaving_the_page() {
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let mut owner = Id::NULL;
        frame(&ctx, &mut nav, |ui| {
            owner = menu_button(ui, "Settings", |ui| {
                let _ = ui.button("General");
            })
            .response
            .id
        });
        ctx.memory_mut(|m| m.request_focus(owner));
        nav.press("South");
        frame(&ctx, &mut nav, |ui| {
            menu_button(ui, "Settings", |ui| {
                let _ = ui.button("General");
            });
        });
        assert!(menu_open(&ctx));
        assert!(nav.back(&ctx));
        frame(&ctx, &mut nav, |ui| {
            menu_button(ui, "Settings", |ui| {
                let _ = ui.button("General");
            });
        });
        assert!(!menu_open(&ctx));
        assert!(!nav.back(&ctx));
    }
    #[test]
    fn virtual_keyboard_commits_only_on_apply_and_cancels_with_b() {
        assert_eq!(KEYS.len(), 40);
        assert_eq!(SYMBOLS.len(), 40);
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let mut text = "City".to_owned();
        let mut field = Id::NULL;
        frame(&ctx, &mut nav, |ui| field = text_edit(ui, &mut text, "").id);
        ctx.memory_mut(|m| m.request_focus(field));
        nav.press("South");
        frame(&ctx, &mut nav, |ui| {
            text_edit(ui, &mut text, "");
        });
        assert!(nav.keyboard_open());
        nav.keyboard.as_mut().unwrap().value = "Ville été".into();
        nav.keyboard.as_mut().unwrap().index = 44;
        nav.press("South");
        let mut changed = false;
        frame(&ctx, &mut nav, |ui| {
            changed = text_edit(ui, &mut text, "").changed()
        });
        assert_eq!(text, "Ville été");
        assert!(changed);
        assert!(!nav.keyboard_open());
        nav.press("South");
        frame(&ctx, &mut nav, |ui| {
            text_edit(ui, &mut text, "");
        });
        nav.keyboard.as_mut().unwrap().value = "Discarded".into();
        nav.press("East");
        frame(&ctx, &mut nav, |ui| {
            text_edit(ui, &mut text, "");
        });
        assert_eq!(text, "Ville été");
        assert!(!nav.keyboard_open());
    }
}

pub fn menu_open(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.any_popup_open())
}

/// Top-level launcher menus share the same controller focus scope as combo boxes.
/// Nested menu groups expand in place so focus never escapes into the page behind them.
pub fn menu_button<R>(
    ui: &mut Ui,
    title: impl Into<WidgetText>,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let title = title.into();
    if widgets(ui.ctx()).collecting.is_some() {
        let index = widgets(ui.ctx()).collecting.as_ref().map_or(0, Vec::len);
        let response = egui::CollapsingHeader::new(title).show(ui, contents);
        if response.header_response.enabled() {
            edit_widgets(ui.ctx(), |w| {
                if let Some(entries) = &mut w.collecting {
                    entries.insert(
                        index.min(entries.len()),
                        (response.header_response.id, false),
                    );
                }
            });
        }
        egui::InnerResponse::new(response.body_returned, response.header_response)
    } else {
        ComboBox::from_id_source(("launcher_menu", title.text()))
            .selected_text(title.clone())
            .show_ui(ui, contents)
    }
}
fn register(ui: &Ui, response: &Response, close: bool) {
    if response.enabled() {
        edit_widgets(ui.ctx(), |w| {
            if let Some(entries) = &mut w.collecting {
                entries.push((response.id, false));
            }
        });
    }
    if response.has_focus() {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    if close && response.clicked() && widgets(ui.ctx()).collecting.is_some() {
        let owner = widgets(ui.ctx()).popup.map(|p| p.owner);
        ui.memory_mut(|m| {
            m.close_popup();
            if let Some(owner) = owner {
                m.request_focus(owner);
            }
        });
    }
}
pub fn button(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    let r = ui.button(text);
    register(ui, &r, true);
    r
}
pub fn checkbox(ui: &mut Ui, value: &mut bool, text: impl Into<WidgetText>) -> Response {
    let r = ui.checkbox(value, text);
    register(ui, &r, true);
    r
}
pub fn add_enabled(ui: &mut Ui, enabled: bool, widget: impl egui::Widget) -> Response {
    let r = ui.add_enabled(enabled, widget);
    register(ui, &r, true);
    r
}
pub fn hyperlink_to(ui: &mut Ui, text: impl Into<WidgetText>, url: impl ToString) -> Response {
    let r = ui.hyperlink_to(text, url);
    register(ui, &r, true);
    r
}
