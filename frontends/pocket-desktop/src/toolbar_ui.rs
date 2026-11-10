//! Direct launcher actions, grouped into a compact two-row toolbar.
use eframe::egui::{self, Color32, RichText, Stroke, Ui, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Library,
    Import,
    Backup,
    Restore,
    Emulator,
    Game,
    Keyboard,
    Controller,
    PocketPc,
    Gizmondo,
    Upscale,
    Filter,
    Rotation,
    Fullscreen,
    Screenshot,
    Recording,
    About,
    Resume,
}

pub struct State<'a> {
    pub version: &'a str,
    pub selected: Option<Action>,
    pub resume: bool,
    pub import: bool,
    pub backup: bool,
    pub game: bool,
    pub frame: bool,
    pub capture: bool,
    pub recording: bool,
    pub encoding: bool,
    pub upscale: bool,
    pub fullscreen: bool,
    pub filter: &'a str,
    pub filter_help: &'a str,
    pub rotation: u32,
}

fn button(ui: &mut Ui, action: Action, state: &State<'_>, width: f32) -> bool {
    use Action::*;
    let label = match action {
        Library => "Library (F8)".into(),
        Import => "Import".into(),
        Backup => "Backup".into(),
        Restore => "Restore".into(),
        Emulator => "Emulator".into(),
        Game => "Game options".into(),
        Keyboard => "Keyboard".into(),
        Controller => "Controller".into(),
        PocketPc => "PocketPC".into(),
        Gizmondo => "Gizmondo".into(),
        Upscale => "Upscale ×2".into(),
        Filter => format!("Filter: {} (F9)", state.filter),
        Rotation => format!("Rotation: {}° (F7)", state.rotation),
        Fullscreen => "Fullscreen (F12)".into(),
        Screenshot => "Screenshot (F10)".into(),
        Recording if state.recording => "Stop video (F11)".into(),
        Recording if state.encoding => "Saving video…".into(),
        Recording => "Record video (F11)".into(),
        About => "About".into(),
        Resume => "Return to game".into(),
    };
    let enabled = match action {
        Import => state.import,
        Backup | Restore => state.backup,
        Game => state.game,
        Upscale | Filter | Rotation => state.frame,
        Screenshot => state.capture,
        Recording => state.recording || (state.capture && !state.encoding),
        Resume => state.resume,
        _ => true,
    };
    let active = state.selected == Some(action)
        || (action == Upscale && state.upscale)
        || (action == Fullscreen && state.fullscreen);
    let (fill, border) = if action == Recording && state.recording {
        (
            Color32::from_rgb(78, 32, 38),
            Color32::from_rgb(206, 78, 87),
        )
    } else if active {
        (
            Color32::from_rgb(40, 56, 70),
            Color32::from_rgb(96, 161, 201),
        )
    } else {
        (Color32::from_rgb(40, 44, 51), Color32::from_rgb(65, 70, 79))
    };
    ui.push_id(action, |ui| {
        let response = crate::controller_ui::add_sized_enabled(
            ui,
            enabled,
            Vec2::new(width, 32.0),
            egui::Button::new(label)
                .wrap(true)
                .rounding(4.0)
                .fill(fill)
                .stroke(Stroke::new(1.0, border)),
        );
        let response = match action {
            Library => response.on_hover_text("Return to the game library (F8)"),
            Import => response.on_hover_text("Import .CAB / .ZIP / .RAR"),
            Backup => {
                response.on_hover_text("Save a ZIP backup of the library; stop the game first")
            }
            Restore => response.on_hover_text("Restore a ZIP library backup; stop the game first"),
            Filter => response.on_hover_text(format!(
                "{}\nActivate to select the next filter",
                state.filter_help
            )),
            Rotation => response.on_hover_text("Next rotation: 0° → 90° → 180° → 270°"),
            Screenshot => response
                .on_hover_text("Save the displayed game pixels, with filtering and rotation"),
            Recording => response
                .on_hover_text("Record the displayed game and audio; activate again to stop"),
            _ => response,
        };
        response.clicked()
    })
    .inner
}

fn group<const N: usize>(
    ui: &mut Ui,
    title: &str,
    rows: &[[Action; N]],
    width: f32,
    state: &State<'_>,
    selected: &mut Option<Action>,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        ui.set_width(width * N as f32 + 6.0 * (N - 1) as f32);
        ui.label(
            RichText::new(title)
                .size(11.0)
                .color(Color32::from_gray(163)),
        );
        for row in rows {
            ui.horizontal(|ui| {
                for action in row {
                    if button(ui, *action, state, width) {
                        *selected = Some(*action);
                    }
                }
            });
        }
    });
}

fn separator(ui: &mut Ui) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 88.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_top(), rect.left_bottom()],
        Stroke::new(1.0, Color32::from_gray(65)),
    );
    ui.add_space(6.0);
}

pub const NAV_HINT: &str = "D-pad / left stick: navigate   {South}: open   {East}: back   L3: switch section";

pub struct Output {
    pub action: Option<Action>,
}

pub fn show(ui: &mut Ui, state: &State<'_>) -> Output {
    use Action::*;
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("PocketHLE {}", state.version)).size(17.0));
        if state.resume && button(ui, Resume, state, 120.0) {
            action = Some(Resume);
        }
    });
    ui.add_space(3.0);
    egui::ScrollArea::horizontal()
        .id_source("main_toolbar_buttons")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                group(
                    ui,
                    "GAME LIBRARY",
                    &[[Library, Import], [Backup, Restore]],
                    80.0,
                    state,
                    &mut action,
                );
                separator(ui);
                group(
                    ui,
                    "SETTINGS",
                    &[[Emulator, Game, Keyboard], [Controller, PocketPc, Gizmondo]],
                    98.0,
                    state,
                    &mut action,
                );
                separator(ui);
                group(
                    ui,
                    "FEATURES",
                    &[
                        [Upscale, Filter, Rotation],
                        [Fullscreen, Screenshot, Recording],
                    ],
                    164.0,
                    state,
                    &mut action,
                );
                separator(ui);
                group(ui, "ABOUT", &[[About]], 80.0, state, &mut action);
            });
        });
    ui.separator();
    Output { action }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller_ui::{self, Navigation};

    fn state() -> State<'static> {
        State {
            version: env!("CARGO_PKG_VERSION"),
            selected: None,
            resume: false,
            import: true,
            backup: true,
            game: true,
            frame: true,
            capture: true,
            recording: false,
            encoding: false,
            upscale: true,
            fullscreen: false,
            filter: "Reconstruction",
            filter_help: "GPU reconstruction",
            rotation: 270,
        }
    }

    fn frame(
        ctx: &egui::Context,
        nav: &mut Navigation,
        state: &State<'_>,
        width: f32,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<Action>, egui::Id) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 400.0),
            )),
            events,
            ..Default::default()
        };
        nav.prepare_input(ctx, &mut raw, true);
        let mut action = None;
        let mut page = egui::Id::NULL;
        let output = ctx.run(raw, |ctx| {
            controller_ui::begin_frame(ctx);
            egui::TopBottomPanel::top("toolbar_test").show(ctx, |ui| {
                controller_ui::toolbar_scope(ui, |ui| action = show(ui, state).action)
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                controller_ui::page_scope(ui, "toolbar_test_page", |ui| {
                    page = controller_ui::button(ui, "Page control").id
                })
            });
            nav.show(ctx);
        });
        (output, action, page)
    }

    fn text_rect(shapes: &[egui::epaint::ClippedShape], text: &str) -> egui::Rect {
        fn find(shape: &egui::Shape, text: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == text => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, text)),
                _ => None,
            }
        }
        shapes
            .iter()
            .find_map(|shape| find(&shape.shape, text))
            .unwrap_or_else(|| panic!("missing toolbar text: {text}"))
    }

    fn click(
        ctx: &egui::Context,
        nav: &mut Navigation,
        state: &State<'_>,
        point: egui::Pos2,
    ) -> Option<Action> {
        let event = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(
            ctx,
            nav,
            state,
            1280.0,
            vec![egui::Event::PointerMoved(point), event(true)],
        );
        frame(ctx, nav, state, 1280.0, vec![event(false)]).1
    }

    #[test]
    fn grouped_buttons_have_current_modes_and_no_overlapping_labels() {
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let (output, _, _) = frame(&ctx, &mut nav, &state(), 1280.0, vec![]);
        let labels = [
            "Library (F8)",
            "Import",
            "Backup",
            "Restore",
            "Emulator",
            "Game options",
            "Keyboard",
            "Controller",
            "PocketPC",
            "Gizmondo",
            "Upscale ×2",
            "Filter: Reconstruction (F9)",
            "Rotation: 270° (F7)",
            "Fullscreen (F12)",
            "Screenshot (F10)",
            "Record video (F11)",
            "About",
        ];
        let rects: Vec<_> = labels
            .iter()
            .map(|label| text_rect(&output.shapes, label))
            .collect();
        for (i, rect) in rects.iter().enumerate() {
            for (j, other) in rects.iter().enumerate().skip(i + 1) {
                assert!(
                    !rect.intersects(*other),
                    "{} overlaps {}",
                    labels[i],
                    labels[j]
                );
            }
        }
        assert!(
            text_rect(&output.shapes, "GAME LIBRARY").left()
                < text_rect(&output.shapes, "SETTINGS").left()
        );
        assert!(
            text_rect(&output.shapes, "FEATURES").left()
                < text_rect(&output.shapes, "ABOUT").left()
        );
        assert_eq!(
            click(
                &ctx,
                &mut nav,
                &state(),
                text_rect(&output.shapes, "Filter: Reconstruction (F9)").center()
            ),
            Some(Action::Filter)
        );
    }

    #[test]
    fn recording_state_is_red_and_saving_state_cannot_start_another_recording() {
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let mut state = state();
        state.recording = true;
        let (output, _, _) = frame(&ctx, &mut nav, &state, 1280.0, vec![]);
        let point = text_rect(&output.shapes, "Stop video (F11)").center();
        fn red(shape: &egui::Shape, point: egui::Pos2) -> bool {
            match shape {
                egui::Shape::Rect(rect) => {
                    rect.rect.contains(point) && rect.fill == Color32::from_rgb(78, 32, 38)
                }
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| red(shape, point)),
                _ => false,
            }
        }
        assert!(output.shapes.iter().any(|shape| red(&shape.shape, point)));
        assert_eq!(
            click(&ctx, &mut nav, &state, point),
            Some(Action::Recording)
        );
        state.recording = false;
        state.encoding = true;
        let (output, _, _) = frame(&ctx, &mut nav, &state, 1280.0, vec![]);
        assert_eq!(
            click(
                &ctx,
                &mut nav,
                &state,
                text_rect(&output.shapes, "Saving video…").center()
            ),
            None
        );
    }

    #[test]
    fn controller_moves_between_real_rows_and_l3_returns_to_page() {
        let ctx = egui::Context::default();
        let mut nav = Navigation::default();
        let state = state();
        let (_, _, page) = frame(&ctx, &mut nav, &state, 1280.0, vec![]);
        ctx.memory_mut(|m| m.request_focus(page));
        frame(&ctx, &mut nav, &state, 1280.0, vec![]);
        nav.press("LeftThumb");
        let (output, _, _) = frame(&ctx, &mut nav, &state, 1280.0, vec![]);
        let focused = ctx
            .read_response(ctx.memory(|m| m.focused()).unwrap())
            .unwrap();
        assert!(focused
            .rect
            .contains(text_rect(&output.shapes, "Library (F8)").center()));
        nav.press("DPadDown");
        let (output, _, _) = frame(&ctx, &mut nav, &state, 1280.0, vec![]);
        nav.release("DPadDown");
        let focused = ctx
            .read_response(ctx.memory(|m| m.focused()).unwrap())
            .unwrap();
        assert!(focused
            .rect
            .contains(text_rect(&output.shapes, "Backup").center()));
        nav.press("DPadRight");
        let (output, _, _) = frame(&ctx, &mut nav, &state, 1280.0, vec![]);
        nav.release("DPadRight");
        let focused = ctx
            .read_response(ctx.memory(|m| m.focused()).unwrap())
            .unwrap();
        assert!(focused
            .rect
            .contains(text_rect(&output.shapes, "Restore").center()));
        nav.press("LeftThumb");
        frame(&ctx, &mut nav, &state, 1280.0, vec![]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(page));
    }

    #[test]
    fn narrow_windows_keep_the_toolbar_bounded_and_reveal_controller_focus() {
        for width in [360.0, 800.0, 1280.0] {
            let ctx = egui::Context::default();
            let mut nav = Navigation::default();
            let state = state();
            let (output, _, page) = frame(&ctx, &mut nav, &state, width, vec![]);
            assert!(
                text_rect(&output.shapes, "Page control").top() < 180.0,
                "toolbar grew beyond two rows"
            );
            ctx.memory_mut(|m| m.request_focus(page));
            nav.press("LeftThumb");
            frame(&ctx, &mut nav, &state, width, vec![]);
            for _ in 0..8 {
                nav.press("DPadRight");
                frame(&ctx, &mut nav, &state, width, vec![]);
                nav.release("DPadRight");
            }
            for _ in 0..12 {
                frame(&ctx, &mut nav, &state, width, vec![]);
            }
            let (output, _, _) = frame(&ctx, &mut nav, &state, width, vec![]);
            let rect = text_rect(&output.shapes, "About");
            assert!(
                rect.center().x >= 0.0 && rect.center().x <= width,
                "controller focus must reveal the button"
            );
        }
    }
}
