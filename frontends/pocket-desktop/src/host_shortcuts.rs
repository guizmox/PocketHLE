//! Reserved keys are removed before any UI/editor or guest input consumer.
use egui::{Context, Event, Key};

pub fn take(ctx: &Context) -> Vec<Key> {
    ctx.input_mut(|input| {
        let keys = input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } if pocket_library::keybindings::reserved_host_key(key.name()) => Some(*key),
                _ => None,
            })
            .collect();
        input.events.retain(|event| {
            !matches!(event, Event::Key {key,..}
            if pocket_library::keybindings::reserved_host_key(key.name()))
        });
        if input.viewport().focused.unwrap_or(true) {
            keys
        } else {
            Vec::new()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(key: Key, pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn host_keys_are_taken_before_editor_capture_and_repeats_do_not_trigger_actions() {
        let ctx = Context::default();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    event(Key::F8, true, false),
                    event(Key::F9, true, false),
                    event(Key::F12, true, false),
                    event(Key::F12, true, true),
                    event(Key::F12, false, false),
                    event(Key::X, true, false),
                ],
                ..Default::default()
            },
            |ctx| {
                assert_eq!(take(ctx), vec![Key::F8, Key::F9, Key::F12]);
                assert!(ctx.input(|i| i
                    .events
                    .iter()
                    .all(|e| matches!(e, Event::Key { key: Key::X, .. }))));
                assert!(take(ctx).is_empty());
            },
        );
    }
}
