//! Native controller events are collected independently of guest/render frames.
use std::{collections::HashSet, sync::{mpsc::{self, Receiver}, Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
use gilrs::{Axis, Button, EventType, Gilrs};

pub enum Event {
    Devices(Vec<(usize, String)>),
    Input { device: usize, control: String, down: bool },
    Error(String),
}

pub fn label(control: &str) -> String {
    match control {
        "South" => "South (Switch B / Xbox A)".into(),
        "East" => "East (Switch A / Xbox B)".into(),
        "North" => "North (Switch X / Xbox Y)".into(),
        "West" => "West (Switch Y / Xbox X)".into(),
        "LeftTrigger" => "L / LB".into(), "RightTrigger" => "R / RB".into(),
        "LeftTrigger2" => "ZL / LT".into(), "RightTrigger2" => "ZR / RT".into(),
        other => other.into(),
    }
}

/// Hysteresis prevents stick jitter generating extra menu presses.
fn axis_down(value: f32, held: bool) -> bool {
    value >= if held { 0.35 } else { 0.60 }
}

pub struct Monitor { pub rx: Receiver<Event>, alive: Arc<AtomicBool> }
impl Drop for Monitor { fn drop(&mut self) { self.alive.store(false, Ordering::Relaxed); } }
pub fn start(ctx: egui::Context) -> Monitor {
    let (tx, rx) = mpsc::channel();
    let alive=Arc::new(AtomicBool::new(true)); let worker_alive=alive.clone();
    std::thread::Builder::new().name("gamepad-input".into()).spawn(move || {
        let mut pads = match Gilrs::new() {
            Ok(pads) => pads,
            Err(e) => { let _ = tx.send(Event::Error(e.to_string())); ctx.request_repaint(); return; }
        };
        let devices = |pads: &Gilrs| pads.gamepads()
            .map(|(id, pad)| (usize::from(id), pad.name().to_owned())).collect();
        if tx.send(Event::Devices(devices(&pads))).is_err() { return; }
        ctx.request_repaint();
        let mut held = HashSet::<(usize, String)>::new();
        while worker_alive.load(Ordering::Relaxed) {
            while let Some(event) = pads.next_event() {
                let device = usize::from(event.id);
                let changes: Vec<(String, bool)> = match event.event {
                    EventType::Connected => {
                        if tx.send(Event::Devices(devices(&pads))).is_err() { return; }
                        ctx.request_repaint(); Vec::new()
                    }
                    EventType::Disconnected => {
                        let releases = held.iter().filter(|(id, _)| *id == device)
                            .map(|(_, control)| (control.clone(), false)).collect();
                        if tx.send(Event::Devices(devices(&pads))).is_err() { return; }
                        ctx.request_repaint(); releases
                    }
                    EventType::ButtonPressed(button, code) | EventType::ButtonReleased(button, code) => {
                        let name = if button == Button::Unknown { format!("RawButton:{}", code.into_u32()) }
                            else { format!("{button:?}") };
                        vec![(name, matches!(event.event, EventType::ButtonPressed(..)))]
                    }
                    EventType::AxisChanged(axis, value, _) => {
                        let (negative, positive) = match axis {
                            Axis::LeftStickX => ("LeftStickLeft", "LeftStickRight"),
                            Axis::LeftStickY => ("LeftStickDown", "LeftStickUp"),
                            Axis::RightStickX => ("RightStickLeft", "RightStickRight"),
                            Axis::RightStickY => ("RightStickDown", "RightStickUp"),
                            _ => continue,
                        };
                        // Releases precede presses when crossing straight from -1 to +1.
                        let mut changes = vec![(negative.into(), axis_down(-value, held.contains(&(device, negative.into())))),
                            (positive.into(), axis_down(value, held.contains(&(device, positive.into()))))];
                        changes.sort_by_key(|(_, down)| *down); changes
                    }
                    _ => Vec::new(), // Ignore OS/controller repeat events.
                };
                for (control, down) in changes {
                    let key = (device, control.clone());
                    let changed = if down { held.insert(key) } else { held.remove(&key) };
                    if changed {
                        if tx.send(Event::Input { device, control, down }).is_err() { return; }
                        ctx.request_repaint();
                    }
                }
            }
            // Idle polling and shutdown never depend on guest rendering.
            std::thread::sleep(Duration::from_millis(4));
        }
    }).expect("spawn controller worker");
    Monitor { rx, alive }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stick_hysteresis() {
        assert!(!axis_down(0.59, false)); assert!(axis_down(0.61, false));
        assert!(axis_down(0.40, true)); assert!(!axis_down(0.34, true));
        assert!(!axis_down(f32::NAN, true));
    }
}
