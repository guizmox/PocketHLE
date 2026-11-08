//! Presentation only: SDL supplies button labels; serialized input IDs stay positional.
use egui::{Context, Id};
use std::{collections::HashMap, sync::Arc};
type FaceLabels = [Option<String>; 4];
#[derive(Clone, Default)]
struct Labels {
    devices: Arc<HashMap<usize, FaceLabels>>,
    active: Option<usize>,
}
fn key() -> Id {
    Id::new("controller_button_labels")
}
pub fn replace(ctx: &Context, devices: impl Iterator<Item = (usize, FaceLabels)>) {
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<Labels>(key());
        state.devices = Arc::new(devices.collect());
        if !state
            .active
            .is_some_and(|id| state.devices.contains_key(&id))
        {
            state.active = if state.devices.len() == 1 {
                state.devices.keys().next().copied()
            } else {
                None
            };
        }
    });
}
pub fn activate(ctx: &Context, device: usize) {
    ctx.data_mut(|data| data.get_temp_mut_or_default::<Labels>(key()).active = Some(device));
}
pub fn text(ctx: &Context, template: &str) -> String {
    ctx.data(|data| {
        let state = data.get_temp::<Labels>(key()).unwrap_or_default();
        let labels = state.active.and_then(|id| state.devices.get(&id));
        let mut result = template.to_owned();
        for (index, (control, fallback)) in [
            ("South", "Bottom button"),
            ("East", "Right button"),
            ("West", "Left button"),
            ("North", "Top button"),
        ]
        .iter()
        .enumerate()
        {
            let label = labels.and_then(|l| l[index].as_deref()).unwrap_or(fallback);
            result = result.replace(&format!("{{{control}}}"), label);
        }
        result
    })
}
pub fn button(ctx: &Context, control: &str, fallback: &str) -> String {
    if matches!(control, "South" | "East" | "West" | "North") {
        text(ctx, &format!("{{{control}}}"))
    } else {
        fallback.to_owned()
    }
}
pub fn grid(ctx: &Context, input: &pocket_library::stylus_grid::HostInput) -> String {
    use pocket_library::stylus_grid::HostInput;
    match input {
        HostInput::Keyboard(key) => format!("Key {key}"),
        HostInput::Controller(control) => button(ctx, control, control),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn follows_active_device_and_forgets_disconnected_labels() {
        let ctx = Context::default();
        replace(
            &ctx,
            [
                (1, [Some("B".into()), Some("A".into()), None, None]),
                (2, [Some("Cross".into()), Some("Circle".into()), None, None]),
            ]
            .into_iter(),
        );
        activate(&ctx, 1);
        assert_eq!(
            grid(
                &ctx,
                &pocket_library::stylus_grid::HostInput::Controller("South".into())
            ),
            "B"
        );
        assert_eq!(
            grid(
                &ctx,
                &pocket_library::stylus_grid::HostInput::Keyboard("A".into())
            ),
            "Key A"
        );
        assert_eq!(text(&ctx, "{South} / {East}"), "B / A");
        activate(&ctx, 2);
        assert_eq!(text(&ctx, "{South} / {East}"), "Cross / Circle");
        // Mapping updates refresh the same live device without a reconnect.
        replace(
            &ctx,
            [(
                2,
                [Some("Triangle".into()), Some("Square".into()), None, None],
            )]
            .into_iter(),
        );
        assert_eq!(text(&ctx, "{South} / {East}"), "Triangle / Square");
        replace(&ctx, std::iter::empty());
        assert_eq!(text(&ctx, "{South}"), "Bottom button");
    }
}
