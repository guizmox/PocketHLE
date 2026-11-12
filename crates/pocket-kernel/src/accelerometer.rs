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

/// The frontends inverse-rotate directional inputs before sending guest VKs.
/// Find which guest keys correspond to Left and Right on the displayed screen.
pub fn simulated_sample(state: &mut crate::KernelState) -> [f32; 3] {
    use crate::gapi::{VK_DOWN, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP};
    state
        .async_keys
        .sync(&state.pending_input, state.gapi_action_key != VK_RETURN);
    let directions = [VK_UP, VK_RIGHT, VK_DOWN, VK_LEFT];
    let left = (7 - usize::from(state.async_keys.display_turns)) % 4;
    let right = (left + 2) % 4;

    match (
        state.async_keys.is_down(directions[left]),
        state.async_keys.is_down(directions[right]),
    ) {
        // Rotate a 1 g vector by 30 degrees around the vertical screen axis.
        (true, false) => [-4.9, 0.0, 8.487_049],
        (false, true) => [4.9, 0.0, 8.487_049],
        _ => [0.0, 0.0, 9.8],
    }
}
