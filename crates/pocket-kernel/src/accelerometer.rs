//! Latest physical acceleration from the active frontend, in m/s².
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
