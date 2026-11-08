//! Launcher-owned confirmation: gamepad and keyboard input never leaves egui.
use egui::{Context, Key};

pub struct Prompt {
    pub id: String,
    pub name: String,
}
impl Prompt {
    /// None means the prompt is still open. Cancel wins simultaneous inputs.
    pub fn show(&self, ctx: &Context, actions: &[String]) -> Option<bool> {
        let mut confirm =
            actions.iter().any(|a| a == "South") || ctx.input(|i| i.key_pressed(Key::Enter));
        let mut cancel =
            actions.iter().any(|a| a == "East") || ctx.input(|i| i.key_pressed(Key::Escape));
        crate::controller_ui::claim_modal_input(ctx);
        egui::CentralPanel::default().show(ctx, |_| {});
        let mut open = true;
        egui::Window::new("Remove game?")
            .id(egui::Id::new("remove_game_confirmation"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(400.0)
            .max_width((ctx.screen_rect().width() - 32.0).max(100.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(format!("Remove {} from the library?", self.name));
                ui.label(
                    "This permanently deletes its installed files, saves and per-game settings.",
                );
                ui.horizontal_wrapped(|ui| {
                    cancel |= crate::controller_ui::button(
                        ui,
                        crate::controller_labels::text(ctx, "Cancel ({East})"),
                    )
                    .clicked();
                    confirm |= crate::controller_ui::button(
                        ui,
                        crate::controller_labels::text(ctx, "Remove ({South})"),
                    )
                    .clicked();
                });
                ui.weak("Keyboard: Enter to remove · Escape to cancel");
            });
        if cancel || !open {
            Some(false)
        } else if confirm {
            Some(true)
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removal_waits_for_explicit_confirmation_and_cancel_wins() {
        for (actions, expected) in [
            (vec![], None),
            (vec!["South".into()], Some(true)),
            (vec!["East".into()], Some(false)),
            (vec!["South".into(), "East".into()], Some(false)),
        ] {
            let ctx = Context::default();
            let prompt = Prompt {
                id: "game".into(),
                name: "PocketPC or Gizmondo game".into(),
            };
            let _ = ctx.run(Default::default(), |ctx| {
                assert_eq!(prompt.show(ctx, &actions), expected)
            });
        }
    }
    #[test]
    fn enter_confirms_escape_cancels_and_escape_wins_together() {
        for (keys, expected) in [
            (vec![Key::Enter], true),
            (vec![Key::Escape], false),
            (vec![Key::Enter, Key::Escape], false),
        ] {
            let ctx = Context::default();
            let raw = egui::RawInput {
                events: keys
                    .into_iter()
                    .map(|key| egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    })
                    .collect(),
                ..Default::default()
            };
            let prompt = Prompt {
                id: "game".into(),
                name: "Game".into(),
            };
            let _ = ctx.run(raw, |ctx| assert_eq!(prompt.show(ctx, &[]), Some(expected)));
        }
    }
}
