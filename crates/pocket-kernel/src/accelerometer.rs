//! Physical and simulated acceleration in m/s².
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
struct HostState {
    present: bool,
    sample: Option<([f32; 3], Instant)>,
}

static HOST: Mutex<HostState> = Mutex::new(HostState {
    present: false,
    sample: None,
});

pub fn publish(present: bool, acceleration: Option<[f32; 3]>) {
    let sample = acceleration
        .filter(|axes| present && axes.iter().all(|value| value.is_finite()))
        .map(|axes| (axes, Instant::now()));
    *HOST.lock().unwrap() = HostState { present, sample };
}

pub fn available() -> bool {
    HOST.lock().unwrap().present
}

pub fn sample() -> Option<[f32; 3]> {
    HOST.lock()
        .unwrap()
        .sample
        .filter(|(_, timestamp)| timestamp.elapsed() < Duration::from_secs(1))
        .map(|(axes, _)| axes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TiltAxes {
    Horizontal,
    Both,
}

/// The frontends inverse-rotate directional keys. Stick positions are
/// already display-relative and use positive Y down.
pub fn simulated_sample(state: &mut crate::KernelState, axes: TiltAxes) -> [f32; 3] {
    use crate::gapi::{VK_DOWN, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP};
    state
        .async_keys
        .sync(&state.pending_input, state.gapi_action_key != VK_RETURN);
    let directions = [VK_UP, VK_RIGHT, VK_DOWN, VK_LEFT];
    simulated_axes(
        axes,
        display_directions(
            directions.map(|key| state.async_keys.is_down(key)),
            usize::from(state.async_keys.display_turns),
        ),
        state.async_keys.tilt_stick,
    )
}

/// Frontends inverse-rotate directional keys before dispatch.
pub fn display_directions(directions: [bool; 4], turns: usize) -> [bool; 4] {
    std::array::from_fn(|direction| directions[(direction + 4 - turns % 4) % 4])
}

pub fn undo_display_rotation([x, y, z]: [f32; 3], turns: usize) -> [f32; 3] {
    match turns % 4 {
        1 => [y, -x, z],
        2 => [-x, -y, z],
        3 => [-y, x, z],
        _ => [x, y, z],
    }
}

/// Convert display-relative Up, Right, Down, Left and stick input to m/s².
pub fn simulated_axes(
    axes: TiltAxes,
    [up, right, down, left]: [bool; 4],
    [stick_x, stick_y]: [i16; 2],
) -> [f32; 3] {
    let axis = |negative: bool, positive: bool, stick: i16| {
        if negative || positive {
            f32::from(i8::from(positive) - i8::from(negative))
        } else {
            // Ignore neutral stick drift and rescale the remaining travel.
            let value = f32::from(stick).clamp(-1000.0, 1000.0);
            value.signum() * (value.abs() - 120.0).max(0.0) / 880.0
        }
    };

    let x = 0.5 * axis(left, right, stick_x);
    let y = match axes {
        TiltAxes::Horizontal => 0.0,
        TiltAxes::Both => 0.5 * axis(down, up, -stick_y.clamp(-1000, 1000)),
    };
    let z = (1.0 - x * x - y * y).sqrt();
    [x * 9.8, y * 9.8, z * 9.8]
}
