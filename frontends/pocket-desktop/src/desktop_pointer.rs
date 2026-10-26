//! Ordered physical-pointer gestures, independent of guest emulation.
use eframe::egui::{self, Event, Pos2, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stroke {
    Down(u16, u16),
    Move(u16, u16),
    Up(u16, u16),
}

/// Some backends report a disappearing touch pointer without its mouse Up.
/// Complete that lifecycle before egui sees it, so virtual controls cannot
/// become pressed again merely when a mouse later moves over the window.
pub fn finish_missing_releases(ctx: &egui::Context, raw: &mut egui::RawInput) {
    let (mut down, mut position) = ctx.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.latest_pos().or(i.pointer.press_origin()),
        )
    });
    let mut events = Vec::with_capacity(raw.events.len() + 1);
    for event in std::mem::take(&mut raw.events) {
        match &event {
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } => {
                down = *pressed;
                position = Some(*pos);
            }
            Event::PointerMoved(pos) => position = Some(*pos),
            Event::PointerGone | Event::WindowFocused(false) if down => {
                if let Some(pos) = position {
                    events.push(Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: raw.modifiers,
                    });
                    down = false;
                }
            }
            _ => {}
        }
        events.push(event);
    }
    if down && !raw.focused {
        if let Some(pos) = position {
            events.push(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: raw.modifiers,
            });
        }
    }
    raw.events = events;
}

/// Do not infer transitions from the button state at the end of a frame:
/// touch taps can start and end in that frame, and PointerGone does not
/// clear egui's cached primary_down flag when an Up event is missing.
pub fn gestures(
    events: &[Event],
    down: &mut Option<(u16, u16)>,
    hit_rect: Rect,
    focused: bool,
    map: impl Fn(Pos2) -> (u16, u16),
) -> Vec<Stroke> {
    let mut output = Vec::new();
    let release = |down: &mut Option<(u16, u16)>, output: &mut Vec<Stroke>| {
        if let Some((x, y)) = down.take() {
            output.push(Stroke::Up(x, y));
        }
    };
    if !focused {
        release(down, &mut output);
        return output;
    }
    let mut accepting = true;
    for event in events {
        match *event {
            Event::WindowFocused(value) => {
                accepting = value;
                if !value {
                    release(down, &mut output);
                }
            }
            Event::PointerGone => release(down, &mut output),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } => {
                if pressed && accepting {
                    release(down, &mut output);
                    if hit_rect.contains(pos) {
                        let (x, y) = map(pos);
                        *down = Some((x, y));
                        output.push(Stroke::Down(x, y));
                    }
                } else if !pressed && down.is_some() {
                    let (x, y) = map(pos);
                    *down = None;
                    output.push(Stroke::Up(x, y));
                }
            }
            Event::PointerMoved(pos) if down.is_some() && accepting => {
                let (x, y) = map(pos);
                if *down != Some((x, y)) {
                    *down = Some((x, y));
                    output.push(Stroke::Move(x, y));
                }
            }
            _ => {}
        }
    }
    output
}

pub fn held_in(ctx: &egui::Context, rect: &Rect) -> bool {
    ctx.input(|input| {
        input.viewport().focused.unwrap_or(true)
            && input.pointer.latest_pos().is_some()
            && input.pointer.primary_down()
            && input
                .pointer
                .press_origin()
                .is_some_and(|origin| rect.contains(origin))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn button(x: f32, pressed: bool) -> Event {
        Event::PointerButton {
            pos: Pos2::new(x, 20.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    fn hit() -> Rect {
        Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0))
    }
    fn map(p: Pos2) -> (u16, u16) {
        (p.x.clamp(0.0, 99.0) as u16, p.y.clamp(0.0, 99.0) as u16)
    }
    #[test]
    fn completed_release_clears_egui_button_and_prevents_a_ghost_press_after_touch() {
        let ctx = egui::Context::default();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![button(10.0, true)],
                ..Default::default()
            },
            |_| {},
        );
        let mut raw = egui::RawInput {
            events: vec![Event::PointerGone],
            ..Default::default()
        };
        finish_missing_releases(&ctx, &mut raw);
        assert_eq!(raw.events.len(), 2);
        let _ = ctx.run(raw, |_| {});
        assert!(!ctx.input(|i| i.pointer.primary_down()));
        let _ = ctx.run(
            egui::RawInput {
                events: vec![Event::PointerMoved(Pos2::new(10.0, 20.0))],
                ..Default::default()
            },
            |_| {},
        );
        assert!(!held_in(&ctx, &hit()));
    }
    #[test]
    fn short_touch_tap_preserves_down_and_up_in_one_frame() {
        let mut down = None;
        assert_eq!(
            gestures(
                &[button(10.0, true), button(10.0, false), Event::PointerGone],
                &mut down,
                hit(),
                true,
                map
            ),
            [Stroke::Down(10, 20), Stroke::Up(10, 20)]
        );
        assert_eq!(down, None);
    }
    #[test]
    fn missing_button_up_is_released_on_pointer_gone_and_next_touch_starts_fresh() {
        let mut down = None;
        assert_eq!(
            gestures(&[button(10.0, true)], &mut down, hit(), true, map),
            [Stroke::Down(10, 20)]
        );
        assert_eq!(
            gestures(&[Event::PointerGone], &mut down, hit(), true, map),
            [Stroke::Up(10, 20)]
        );
        assert!(gestures(
            &[Event::PointerMoved(Pos2::new(12.0, 20.0))],
            &mut down,
            hit(),
            true,
            map
        )
        .is_empty());
        assert_eq!(
            gestures(&[button(15.0, true)], &mut down, hit(), true, map),
            [Stroke::Down(15, 20)]
        );
    }
    #[test]
    fn drag_keeps_capture_until_release_and_focus_loss_lifts_once() {
        let mut down = None;
        assert_eq!(
            gestures(&[button(-10.0, true)], &mut down, hit(), true, map),
            []
        );
        gestures(&[button(10.0, true)], &mut down, hit(), true, map);
        assert_eq!(
            gestures(
                &[
                    Event::PointerMoved(Pos2::new(110.0, 20.0)),
                    button(110.0, false)
                ],
                &mut down,
                hit(),
                true,
                map
            ),
            [Stroke::Move(99, 20), Stroke::Up(99, 20)]
        );
        gestures(&[button(10.0, true)], &mut down, hit(), true, map);
        assert_eq!(
            gestures(&[], &mut down, hit(), false, map),
            [Stroke::Up(10, 20)]
        );
        assert!(gestures(&[], &mut down, hit(), false, map).is_empty());
    }
    #[test]
    fn egui_pointer_gone_must_not_keep_a_virtual_button_held() {
        let ctx = egui::Context::default();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    Event::PointerMoved(Pos2::new(10.0, 20.0)),
                    button(10.0, true),
                ],
                ..Default::default()
            },
            |_| {},
        );
        assert!(held_in(&ctx, &hit()));
        let _ = ctx.run(
            egui::RawInput {
                events: vec![Event::PointerGone],
                ..Default::default()
            },
            |_| {},
        );
        assert!(ctx.input(|i| i.pointer.primary_down())); // reproduces the stale state
        assert!(!held_in(&ctx, &hit()));
    }
}
