//! Reserved keys are removed before any UI/editor or guest input consumer.
use egui::{Context, Event, Key};

/// Keep the number row stable on AZERTY and with Shift; leave text events and
/// letter layout unchanged. Called before egui builds key state/capture events.
pub fn normalize_number_row(raw: &mut egui::RawInput) {
    for event in &mut raw.events {
        if let Event::Key {
            key,
            physical_key: Some(physical),
            ..
        } = event
        {
            if matches!(
                physical,
                Key::Num0
                    | Key::Num1
                    | Key::Num2
                    | Key::Num3
                    | Key::Num4
                    | Key::Num5
                    | Key::Num6
                    | Key::Num7
                    | Key::Num8
                    | Key::Num9
            ) {
                *key = *physical;
            }
        }
    }
}

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
    fn number_row_is_shift_insensitive_before_egui_state_and_capture() {
        let digits = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for digit in digits {
            let ctx = Context::default();
            for (pressed, shift) in [(true, false), (false, true), (true, true), (false, false)] {
                let mut raw = egui::RawInput::default();
                raw.events.push(Event::Key {
                    key: Key::Num7,
                    physical_key: Some(digit),
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        shift,
                        ..Default::default()
                    },
                });
                raw.events.push(Event::Text("&".into()));
                normalize_number_row(&mut raw);
                assert!(matches!(&raw.events[1],Event::Text(text) if text=="&"));
                let _ = ctx.run(raw, |ctx| {
                    assert_eq!(ctx.input(|i| i.key_down(digit)), pressed)
                });
            }
        }
        let mut raw = egui::RawInput::default();
        raw.events.push(Event::Key {
            key: Key::Z,
            physical_key: Some(Key::W),
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        });
        normalize_number_row(&mut raw);
        assert!(matches!(raw.events[0], Event::Key { key: Key::Z, .. }));
    }

    #[test]
    fn host_keys_are_taken_before_editor_capture_and_repeats_do_not_trigger_actions() {
        let ctx = Context::default();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    event(Key::F7, true, false),
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
                assert_eq!(take(ctx), vec![Key::F7, Key::F8, Key::F9, Key::F12]);
                assert!(ctx.input(|i| i
                    .events
                    .iter()
                    .all(|e| matches!(e, Event::Key { key: Key::X, .. }))));
                assert!(take(ctx).is_empty());
            },
        );
    }
}
