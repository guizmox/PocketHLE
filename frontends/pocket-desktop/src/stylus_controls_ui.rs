use super::*;
use pocket_library::stylus_grid::{ControlsMode, HostInput, TouchAction};

pub(super) struct StylusEditor {
    pub waiting: Option<usize>,
    selected: Option<usize>,
    message: String,
    testing: bool,
    preview: pocket_core::kernel::touch_steering::TouchSteering,
    axes: (i16, i16),
    point: (f32, f32),
    portrait: bool,
    capture_held: std::collections::HashSet<String>,
    captured_until_up: std::collections::HashSet<String>,
    modifiers: [bool; 4],
}
impl StylusEditor {
    pub fn new(testing: bool, portrait: bool) -> Self {
        Self {
            waiting: None,
            selected: None,
            message: String::new(),
            testing,
            preview: Default::default(),
            axes: (0, 0),
            point: (0.5, 0.5),
            portrait,
            capture_held: Default::default(),
            captured_until_up: Default::default(),
            modifiers: [false; 4],
        }
    }
    fn preview_event(&mut self, e: Option<InputEvent>) {
        if let Some(InputEvent::PointerDown { x, y } | InputEvent::PointerMove { x, y }) = e {
            self.point = (f32::from(x) / 400.0, f32::from(y) / 400.0);
        }
    }
    pub fn owns_stick_input(&self, control: &str) -> bool {
        self.testing || self.waiting.is_some() || self.captured_until_up.contains(control)
    }
    pub fn axis(&mut self, horizontal: bool, value: f32) {
        let v = (value.clamp(-1.0, 1.0) * 1000.0) as i16;
        if horizontal {
            self.axes.0 = v;
        } else {
            self.axes.1 = v;
        }
        let e = self.preview.position(self.axes.0, self.axes.1, 0, 400, 400);
        self.preview_event(e);
    }
    fn key(&mut self, vk: u16, down: bool) {
        let e = if down {
            InputEvent::KeyDown { vk }
        } else {
            InputEvent::KeyUp { vk }
        };
        let e = self.preview.input(e, 0, 400, 400);
        self.preview_event(e);
    }
}
impl PocketLauncher {
    fn grid_settings(&self) -> Option<&pocket_library::stylus_grid::StylusGrid> {
        (self.screen == Screen::Run
            && !self.running_is_gizmondo
            && self.running_controls_mode == ControlsMode::Grid)
            .then_some(&self.running_stylus_grid)
    }
    pub(super) fn grid_assigned(&self, input: &HostInput) -> bool {
        self.grid_settings().is_some_and(|s| {
            s.cell_for(input, self.library.config().gamepad_fullscreen.as_deref())
                .is_some()
        })
    }
    fn send_grid_action(&self, action: TouchAction) {
        let Some(frame) = self.last_frame_snapshot.as_ref() else {
            return;
        };
        let (x, y) = match action {
            TouchAction::Down(x, y) | TouchAction::Move(x, y) | TouchAction::Up(x, y) => (x, y),
        };
        let (x, y) = pocket_library::stylus_grid::guest_point(
            x,
            y,
            rotation_turns(self.game_rotation),
            frame.width,
            frame.height,
        );
        self.send_input(match action {
            TouchAction::Down(..) => InputEvent::PointerDown { x, y },
            TouchAction::Move(..) => InputEvent::PointerMove { x, y },
            TouchAction::Up(..) => InputEvent::PointerUp { x, y },
        });
    }
    pub(super) fn cancel_grid(&mut self) {
        if let Some(action) = self.stylus_grid_router.cancel() {
            self.send_grid_action(action);
        }
    }
    pub(super) fn route_grid(&mut self, input: HostInput, down: bool) -> bool {
        let Some(grid) = self.grid_settings().cloned() else {
            return false;
        };
        // A physical touch owns the stylus until it is released. Assigned buttons
        // remain muted; they need a fresh press afterwards to begin a new stroke.
        if self.pointer_down_at.is_some() {
            return grid
                .cell_for(&input, self.library.config().gamepad_fullscreen.as_deref())
                .is_some();
        }
        let (consumed, action) = self.stylus_grid_router.input(
            &grid,
            input,
            down,
            self.library.config().gamepad_fullscreen.as_deref(),
        );
        if let Some(action) = action {
            self.send_grid_action(action);
        }
        consumed
    }
    pub(super) fn grid_modifier_edges(&mut self, ctx: &egui::Context) -> Vec<(HostInput, bool)> {
        let mods = ctx.input(|i| i.modifiers);
        let now = [mods.shift, mods.ctrl, mods.alt, mods.mac_cmd];
        let previous = if let Some(editor) = self.stylus_editor.as_mut() {
            &mut editor.modifiers
        } else {
            &mut self.grid_modifiers
        };
        let mut edges = Vec::new();
        for (i, name) in ["Shift", "Ctrl", "Alt", "Command"].iter().enumerate() {
            if now[i] != previous[i] {
                edges.push((HostInput::Keyboard((*name).into()), now[i]));
            }
        }
        *previous = now;
        edges
    }
    pub(super) fn route_grid_axis(&mut self, right: bool, horizontal: bool, value: f32) {
        let prefix = if right { "RightStick" } else { "LeftStick" };
        let (neg, pos) = if horizontal {
            ("Left", "Right")
        } else {
            ("Up", "Down")
        };
        // Release before pressing so a sign change never briefly selects both sides.
        let mut edges = vec![
            (format!("{prefix}{neg}"), -value),
            (format!("{prefix}{pos}"), value),
        ];
        edges.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (name, strength) in edges {
            let input = HostInput::Controller(name);
            let down = strength
                >= if self.stylus_grid_router.held(&input) {
                    0.14
                } else {
                    0.18
                };
            self.route_grid(input, down);
        }
    }
    pub(super) fn capture_grid_axis(
        &mut self,
        ctx: &egui::Context,
        right: bool,
        horizontal: bool,
        value: f32,
    ) -> bool {
        let Some(editor) = self.stylus_editor.as_mut() else {
            return false;
        };
        let prefix = if right { "RightStick" } else { "LeftStick" };
        let (negative, positive) = if horizontal {
            ("Left", "Right")
        } else {
            ("Up", "Down")
        };
        let mut pressed = None;
        for (direction, strength) in [(negative, -value), (positive, value)] {
            let name = format!("{prefix}{direction}");
            if strength >= 0.18 {
                if editor.capture_held.insert(name.clone()) {
                    pressed = Some(name);
                }
            } else if strength < 0.14 {
                editor.capture_held.remove(&name);
                editor.captured_until_up.remove(&name);
            }
        }
        let consume = !editor.testing || right;
        if editor.waiting.is_some() {
            if let Some(name) = pressed {
                if let Some(editor) = self.stylus_editor.as_mut() {
                    editor.captured_until_up.insert(name.clone());
                }
                self.stylus_editor_input(HostInput::Controller(name), true);
            }
        }
        ctx.request_repaint();
        consume
    }
    fn stylus_editor_input(&mut self, input: HostInput, down: bool) -> bool {
        let Some(mut editor) = self.stylus_editor.take() else {
            return false;
        };
        if editor.testing {
            let vk = match &input {
                HostInput::Keyboard(n) => self.library.config().keybindings.vk_for_key(n),
                HostInput::Controller(n) => self
                    .library
                    .config()
                    .gamepad_bindings
                    .get(n)
                    .map(|b| b.vk()),
            };
            if let Some(vk) = vk {
                editor.key(vk, down);
            }
        } else if down {
            if let Some(cell) = editor.waiting {
                if let Some((_, draft)) = self.game_settings_draft.as_mut() {
                    match draft.pocketpc_stylus_grid.bind(
                        cell,
                        input,
                        self.library.config().gamepad_fullscreen.as_deref(),
                    ) {
                        Ok(()) => {
                            editor.waiting = None;
                            editor.message = "Assigned. Changes are applied with Save.".into();
                        }
                        Err(error) => editor.message = error.into(),
                    }
                }
            }
        }
        self.stylus_editor = Some(editor);
        true
    }
    pub(super) fn stylus_editor_keyboard(&mut self, ctx: &egui::Context) -> bool {
        if self.screen != Screen::GameSettings {
            self.stylus_editor = None;
            return false;
        }
        if self.stylus_editor.is_none() {
            return false;
        }
        if !ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
            if let Some(e) = self.stylus_editor.as_mut() {
                e.preview.cancel();
                e.axes = (0, 0);
                e.modifiers = [false; 4];
                e.point = (0.5, 0.5);
            }
            return true;
        }
        let swallow = self
            .stylus_editor
            .as_ref()
            .is_some_and(|e| e.waiting.is_some() || e.testing);
        for (input, down) in self.grid_modifier_edges(ctx) {
            self.stylus_editor_input(input, down);
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Key {
                key,
                pressed,
                repeat,
                ..
            } = event
            {
                if pressed && repeat {
                    continue;
                }
                if key == egui::Key::Escape {
                    if pressed {
                        if self
                            .stylus_editor
                            .as_ref()
                            .is_some_and(|e| e.waiting.is_some())
                        {
                            if let Some(e) = self.stylus_editor.as_mut() {
                                e.waiting = None;
                                e.message = "Assignment cancelled.".into();
                            }
                        } else {
                            self.stylus_editor = None;
                        }
                    }
                } else {
                    self.stylus_editor_input(HostInput::Keyboard(key.name().into()), pressed);
                }
            }
        }
        if swallow {
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(
                        e,
                        egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_)
                    )
                })
            });
        }
        true
    }
    pub(super) fn stylus_editor_pad(
        &mut self,
        ctx: &egui::Context,
        control: &str,
        down: bool,
    ) -> bool {
        let Some(editor) = self.stylus_editor.as_ref() else {
            return false;
        };
        if !down {
            self.controller_ui.release(control);
        }
        if editor.waiting.is_some() || editor.testing {
            if down && control == "East" && editor.testing {
                self.stylus_editor = None;
            } else {
                self.stylus_editor_input(HostInput::Controller(control.into()), down);
            }
        } else if down {
            if control == "East" {
                self.stylus_editor = None;
            } else if matches!(control, "Start" | "Select" | "Mode" | "Misc1") {
                if let Some(e) = self.stylus_editor.as_mut() {
                    e.message = "Reserved PocketHLE input.".into();
                }
            } else {
                self.controller_ui.press(control);
            }
        } else {
            self.controller_ui.release(control);
        }
        ctx.request_repaint();
        true
    }
    pub(super) fn ui_stylus_editor(&mut self, ctx: &egui::Context, draft: &mut GameSettings) {
        let Some(mut editor) = self.stylus_editor.take() else {
            return;
        };
        if !draft.pocketpc_stylus_grid.valid_size() {
            draft.pocketpc_stylus_grid = Default::default();
        }
        let mut open = true;
        let mut done = false;
        let title = if editor.testing {
            "Test touch steering"
        } else {
            "Touch zone mapping"
        };
        egui::Window::new(title).id(egui::Id::new("stylus_controls_editor"))
            .open(&mut open).collapsible(false).resizable(false)
            .max_width((ctx.available_rect().width()-32.0).clamp(160.0,600.0))
            .show(ctx,|ui| {
                crate::controller_ui::page_scope(ui,"stylus_grid_modal",|ui| {
                egui::ScrollArea::vertical().max_height((ctx.available_rect().height()-110.0).max(100.0)).show(ui,|ui|{
                    if !editor.testing {
                        crate::controller_ui::ComboBox::from_id_source("stylus_grid_size")
                            .selected_text(format!("{} × {}",draft.pocketpc_stylus_grid.columns,draft.pocketpc_stylus_grid.rows))
                            .show_ui(ui,|ui|{
                                for (c,r) in pocket_library::stylus_grid::StylusGrid::SIZES {
                                    if crate::controller_ui::button(ui,format!("{c} × {r}")).clicked(){
                                        draft.pocketpc_stylus_grid.resize(c,r);editor.waiting=None;editor.selected=None;
                                        editor.message="Grid size changed; assignments cleared.".into();
                                    }
                                }
                            });
                        ui.label("Select a zone, then press a keyboard key, controller button or stick direction.");
                        ui.label("Only assigned inputs become stylus presses. Reserved emulator shortcuts are refused.");
                    }else{
                        ui.label("Try your arrows / D-pad and left stick. Release to return to the centre.");
                    }
                    let ratio=if editor.portrait {3.0/4.0}else{4.0/3.0};
                    let max_h=(ctx.available_rect().height()-260.0).clamp(130.0,360.0);
                    let width=ui.available_width().min(480.0).min(max_h*ratio).max(100.0);
                    if editor.testing {
                        let (rect,_)=ui.allocate_exact_size(Vec2::new(width,width/ratio),Sense::hover());
                        let p=ui.painter();p.rect_filled(rect,4.0,Color32::from_rgb(18,32,48));
                        p.line_segment([egui::pos2(rect.center().x,rect.top()),egui::pos2(rect.center().x,rect.bottom())],egui::Stroke::new(1.0,Color32::GRAY));
                        p.line_segment([egui::pos2(rect.left(),rect.center().y),egui::pos2(rect.right(),rect.center().y)],egui::Stroke::new(1.0,Color32::GRAY));
                        p.circle_filled(rect.min+Vec2::new(editor.point.0*rect.width(),editor.point.1*rect.height()),7.0,Color32::from_rgb(255,100,65));
                    }else{
                        let cols=draft.pocketpc_stylus_grid.columns.max(1) as usize;
                        let rows=draft.pocketpc_stylus_grid.rows.max(1) as usize;
                        egui::Grid::new("stylus_binding_cells").spacing(Vec2::splat(2.0)).show(ui,|ui|{
                            for row in 0..rows {
                                for col in 0..cols {
                                    let cell=row*cols+col;
                                    let label=if editor.waiting==Some(cell){"Press input…".into()}else{
                                        draft.pocketpc_stylus_grid.bindings.iter().find(|b|b.cell==cell).map(|b|b.input.label()).unwrap_or_else(||format!("{}",cell+1))
                                    };
                                    let text=RichText::new(label).size(11.0).color(if editor.waiting==Some(cell){Color32::from_rgb(255,175,70)}else if draft.pocketpc_stylus_grid.bindings.iter().any(|b|b.cell==cell){Color32::from_rgb(100,220,180)}else{Color32::GRAY});
                                    let response=crate::controller_ui::sized_button(ui,Vec2::new(width/cols as f32-2.0,width/ratio/rows as f32-2.0),text);
                                    if response.clicked(){self.controller_ui.stop_repeat();editor.selected=Some(cell);editor.waiting=Some(cell);editor.message="Waiting for input. Escape cancels.".into();}
                                }
                                ui.end_row();
                            }
                        });
                        ui.horizontal(|ui|{
                            if crate::controller_ui::button(ui,"Clear selected").clicked(){
                                if let Some(cell)=editor.selected {draft.pocketpc_stylus_grid.bindings.retain(|b|b.cell!=cell);}
                                editor.waiting=None;
                            }
                            if crate::controller_ui::button(ui,"Clear all").clicked(){draft.pocketpc_stylus_grid.bindings.clear();editor.waiting=None;}
                            if editor.waiting.is_some() && crate::controller_ui::button(ui,"Cancel assignment").clicked(){editor.waiting=None;}
                        });
                        ui.label(&editor.message);
                    }
                    if crate::controller_ui::button(ui,"Done").clicked(){done=true;}
                });
                });
            });
        if open && !done {
            self.stylus_editor = Some(editor);
        } else {
            self.controller_ui.stop_repeat();
        }
    }
}
