//! Dedicated SDL2 controller worker. HIDAPI initializes the Switch Pro protocol;
//! only input subsystems are used, leaving egui's window and CPAL audio alone.
use sdl2::{
    controller::{Axis, Button, GameController},
    event::Event as SdlEvent,
    joystick::{HatState, Joystick},
};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        mpsc::SyncSender,
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub enum Event {
    Devices(Vec<(usize, String)>),
    Input {
        device: usize,
        control: String,
        down: bool,
    },
    Error(String),
}
pub fn label(control: &str) -> String {
    match control {
        "South" => "South (Switch B / Xbox A)".into(),
        "East" => "East (Switch A / Xbox B)".into(),
        "North" => "North (Switch X / Xbox Y)".into(),
        "West" => "West (Switch Y / Xbox X)".into(),
        "LeftTrigger" => "L / LB".into(),
        "RightTrigger" => "R / RB".into(),
        "LeftTrigger2" => "ZL / LT".into(),
        "RightTrigger2" => "ZR / RT".into(),
        other => other.into(),
    }
}
fn button_name(button: Button) -> String {
    match button {
        Button::A => "South",
        Button::B => "East",
        Button::X => "West",
        Button::Y => "North",
        Button::Back => "Select",
        Button::Guide => "Mode",
        Button::Start => "Start",
        Button::LeftShoulder => "LeftTrigger",
        Button::RightShoulder => "RightTrigger",
        Button::LeftStick => "LeftThumb",
        Button::RightStick => "RightThumb",
        Button::DPadUp => "DPadUp",
        Button::DPadDown => "DPadDown",
        Button::DPadLeft => "DPadLeft",
        Button::DPadRight => "DPadRight",
        _ => return format!("{button:?}"),
    }
    .into()
}
fn axis_down(value: f32, held: bool) -> bool {
    value >= if held { 0.35 } else { 0.60 }
}
struct Transitions {
    deadzone: pocket_library::DpadDeadzone,
    held: HashSet<(usize, String)>,
}
impl Default for Transitions {
    fn default() -> Self {
        Self {
            deadzone: pocket_library::DpadDeadzone::Medium,
            held: HashSet::new(),
        }
    }
}
impl Transitions {
    fn change(&mut self, id: usize, control: String, down: bool) -> Option<Event> {
        let key = (id, control.clone());
        let changed = if down {
            self.held.insert(key)
        } else {
            self.held.remove(&key)
        };
        changed.then_some(Event::Input {
            device: id,
            control,
            down,
        })
    }
    fn axis(&mut self, id: usize, negative: &str, positive: &str, value: f32) -> Vec<Event> {
        let mut values = vec![
            (
                negative,
                -value
                    >= self
                        .deadzone
                        .stick_threshold(self.held.contains(&(id, negative.into()))),
            ),
            (
                positive,
                value
                    >= self
                        .deadzone
                        .stick_threshold(self.held.contains(&(id, positive.into()))),
            ),
        ];
        values.sort_by_key(|(_, down)| *down);
        values
            .into_iter()
            .filter_map(|(name, down)| self.change(id, name.into(), down))
            .collect()
    }
    fn disconnect(&mut self, id: usize) -> Vec<Event> {
        let names: Vec<_> = self
            .held
            .iter()
            .filter(|(device, _)| *device == id)
            .map(|(_, name)| name.clone())
            .collect();
        names
            .into_iter()
            .filter_map(|name| self.change(id, name, false))
            .collect()
    }
    fn controller_axis(&mut self, id: usize, axis: Axis, value: i16) -> Vec<Event> {
        let value = value as f32 / 32767.0;
        match axis {
            Axis::LeftX => self.axis(id, "LeftStickLeft", "LeftStickRight", value),
            Axis::LeftY => self.axis(id, "LeftStickDown", "LeftStickUp", -value),
            Axis::RightX => self.axis(id, "RightStickLeft", "RightStickRight", value),
            Axis::RightY => self.axis(id, "RightStickDown", "RightStickUp", -value),
            Axis::TriggerLeft | Axis::TriggerRight => {
                let name = if axis == Axis::TriggerLeft {
                    "LeftTrigger2"
                } else {
                    "RightTrigger2"
                };
                let down = axis_down(value, self.held.contains(&(id, name.into())));
                self.change(id, name.into(), down).into_iter().collect()
            }
        }
    }
    fn hat(&mut self, id: usize, index: u8, hat: HatState) -> Vec<Event> {
        let bits = hat.to_raw();
        let mut changes: Vec<_> = [("Up", 1), ("Right", 2), ("Down", 4), ("Left", 8)]
            .into_iter()
            .map(|(dir, bit)| {
                (
                    if index == 0 {
                        format!("DPad{dir}")
                    } else {
                        format!("SdlHat{index}{dir}")
                    },
                    bits & bit != 0,
                )
            })
            .collect();
        changes.sort_by_key(|(_, down)| *down);
        changes
            .into_iter()
            .filter_map(|(name, down)| self.change(id, name, down))
            .collect()
    }
}
// Poll the same normalized controls used by SDL events. Dolphin likewise
// refreshes gamepad state explicitly; keep events as well for short presses.
const POLLED_BUTTONS: [Button; 21] = [
    Button::A,
    Button::B,
    Button::X,
    Button::Y,
    Button::Back,
    Button::Guide,
    Button::Start,
    Button::LeftStick,
    Button::RightStick,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::Misc1,
    Button::Paddle1,
    Button::Paddle2,
    Button::Paddle3,
    Button::Paddle4,
    Button::Touchpad,
];
const POLLED_AXES: [Axis; 6] = [
    Axis::LeftX,
    Axis::LeftY,
    Axis::RightX,
    Axis::RightY,
    Axis::TriggerLeft,
    Axis::TriggerRight,
];
fn poll_controller(controller: &GameController, state: &mut Transitions) -> Vec<Event> {
    let id = controller.instance_id() as usize;
    let mut changes = Vec::new();
    for button in POLLED_BUTTONS {
        changes.extend(state.change(id, button_name(button), controller.button(button)));
    }
    for axis in POLLED_AXES {
        changes.extend(state.controller_axis(id, axis, controller.axis(axis)));
    }
    changes
}
enum Device {
    Controller(GameController),
    Raw(Joystick),
}
impl Device {
    fn id(&self) -> u32 {
        match self {
            Self::Controller(d) => d.instance_id(),
            Self::Raw(d) => d.instance_id(),
        }
    }
    fn name(&self) -> String {
        match self {
            Self::Controller(d) => d.name(),
            Self::Raw(d) => d.name(),
        }
    }
    fn attached(&self) -> bool {
        match self {
            Self::Controller(d) => d.attached(),
            Self::Raw(d) => d.attached(),
        }
    }
    fn has_rumble(&self) -> bool {
        match self {
            Self::Controller(d) => d.has_rumble(),
            Self::Raw(d) => d.has_rumble(),
        }
    }
    fn rumble(&mut self, on: bool) -> Result<(), String> {
        let strength = if on { 32768 } else { 0 };
        match self {
            Self::Controller(d) => d.set_rumble(strength, strength, 100),
            Self::Raw(d) => d.set_rumble(strength, strength, 100),
        }
        .map_err(|e| e.to_string())
    }
    fn raw(&self) -> bool {
        matches!(self, Self::Raw(_))
    }
}
fn scan(
    controllers: &sdl2::GameControllerSubsystem,
    joysticks: &sdl2::JoystickSubsystem,
    devices: &mut BTreeMap<u32, Device>,
) -> Result<bool, String> {
    let mut changed = false;
    for index in 0..joysticks.num_joysticks()? {
        let device = if controllers.is_game_controller(index) {
            Device::Controller(controllers.open(index).map_err(|e| e.to_string())?)
        } else {
            Device::Raw(joysticks.open(index).map_err(|e| e.to_string())?)
        };
        if let std::collections::btree_map::Entry::Vacant(entry) = devices.entry(device.id()) {
            entry.insert(device);
            changed = true;
        }
    }
    Ok(changed)
}
fn publish(tx: &Sender<Event>, ctx: &egui::Context, event: Event) -> Result<(), String> {
    tx.send(event)
        .map_err(|_| "controller receiver closed".to_owned())?;
    ctx.request_repaint();
    Ok(())
}
fn list(devices: &BTreeMap<u32, Device>) -> Event {
    Event::Devices(
        devices
            .iter()
            .map(|(id, d)| (*id as usize, d.name()))
            .collect(),
    )
}
fn worker(
    ctx: &egui::Context,
    tx: &Sender<Event>,
    alive: &AtomicBool,
    profile: &AtomicU8,
    selected: &AtomicUsize,
    enabled: &AtomicBool,
    commands: &Receiver<crate::rumble::Command>,
) -> Result<(), String> {
    // egui's window is not an SDL window. SDL must receive reports even with
    // no SDL keyboard focus; guest input is still gated by egui's real focus.
    sdl2::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
    sdl2::hint::set("SDL_GAMECONTROLLER_USE_BUTTON_LABELS", "0");
    sdl2::hint::set("SDL_JOYSTICK_HIDAPI", "1");
    sdl2::hint::set("SDL_JOYSTICK_HIDAPI_SWITCH", "1");
    if cfg!(feature = "steam-deck") && crate::gamepad_host::is_wine() {
        // Apply after normal defaults and before opening any SDL device.
        // Explicit environment overrides remain available for advanced setups.
        for (name, value) in crate::gamepad_host::PROTON_HINTS {
            sdl2::hint::set(name, value);
        }
        log::info!("Controller profile: Wine/Proton XInput");
    }
    let sdl = sdl2::init()?;
    let controllers = sdl.game_controller()?;
    let joysticks = sdl.joystick()?;
    let mut events = sdl.event_pump()?;
    let mut devices = BTreeMap::new();
    let mut state = Transitions::default();
    scan(&controllers, &joysticks, &mut devices)?;
    publish(tx, ctx, list(&devices))?;
    let mut motors = BTreeMap::<u64, Option<crate::rumble::Pattern>>::new();
    let mut playing: Option<u32> = None;
    let mut rumble_on = false;
    let mut last_rumble = Instant::now() - Duration::from_secs(1);
    let mut next_scan = Instant::now() + Duration::from_secs(1);
    while alive.load(Ordering::Relaxed) {
        state.deadzone = match profile.load(Ordering::Relaxed) {
            0 => pocket_library::DpadDeadzone::Small,
            1 => pocket_library::DpadDeadzone::Medium,
            _ => pocket_library::DpadDeadzone::Large,
        };
        let selected = selected.load(Ordering::Relaxed);
        while let Ok(command) = commands.try_recv() {
            use crate::rumble::Command;
            match command {
                Command::Open(id, reply) => {
                    let result = if motors.len() >= 64 {
                        Err(8)
                    } else if devices
                        .get(&(selected as u32))
                        .is_some_and(Device::has_rumble)
                    {
                        Ok(())
                    } else {
                        Err(50)
                    };
                    if reply.send(result).is_ok() && result.is_ok() {
                        motors.insert(id, None);
                    }
                }
                Command::Apply(id, timings, submitted, reply) => {
                    let result = if submitted.elapsed() >= Duration::from_secs(1) {
                        Err(21)
                    } else if !motors.contains_key(&id) {
                        Err(6)
                    } else if timings.is_empty() {
                        Ok(())
                    } else if let Some(device) = devices.get_mut(&(selected as u32)) {
                        if !device.has_rumble() {
                            Err(50)
                        } else {
                            device
                                .rumble(
                                    enabled.load(Ordering::Relaxed)
                                        && crate::rumble::Pattern {
                                            timings: timings.clone(),
                                            started: Instant::now(),
                                        }
                                        .active(),
                                )
                                .map_err(|_| 21)
                        }
                    } else {
                        Err(21)
                    };
                    if result.is_ok() {
                        motors.insert(
                            id,
                            if timings.is_empty() {
                                None
                            } else {
                                Some(crate::rumble::Pattern {
                                    timings,
                                    started: Instant::now(),
                                })
                            },
                        );
                        // Force the owner loop to apply/stop immediately below.
                        last_rumble = Instant::now() - Duration::from_secs(1);
                    }
                    let _ = reply.send(result);
                }
                Command::Close(id) => {
                    motors.remove(&id);
                }
            }
        }
        let target = devices
            .get(&(selected as u32))
            .filter(|d| d.has_rumble())
            .map(|_| selected as u32);
        let on = enabled.load(Ordering::Relaxed) && motors.values().flatten().any(|p| p.active());
        if target != playing {
            if let Some(id) = playing {
                if let Some(d) = devices.get_mut(&id) {
                    let _ = d.rumble(false);
                }
            }
            playing = target;
            rumble_on = !on; // force the new device into the current state
        }
        if on != rumble_on || (on && last_rumble.elapsed() >= Duration::from_millis(40)) {
            if let Some(id) = playing {
                if let Some(d) = devices.get_mut(&id) {
                    let _ = d.rumble(on);
                }
            }
            rumble_on = on;
            last_rumble = Instant::now();
        }
        joysticks.update();
        for event in events.poll_iter() {
            let mut changes = Vec::new();
            match event {
                SdlEvent::JoyDeviceAdded { .. } | SdlEvent::ControllerDeviceAdded { .. } => {
                    if scan(&controllers, &joysticks, &mut devices)? {
                        publish(tx, ctx, list(&devices))?;
                    }
                }
                SdlEvent::JoyDeviceRemoved { which, .. }
                | SdlEvent::ControllerDeviceRemoved { which, .. } => {
                    if devices.remove(&which).is_some() {
                        changes = state.disconnect(which as usize);
                        publish(tx, ctx, list(&devices))?;
                    }
                }
                SdlEvent::ControllerButtonDown { which, button, .. } => {
                    changes.extend(state.change(which as usize, button_name(button), true));
                }
                SdlEvent::ControllerButtonUp { which, button, .. } => {
                    changes.extend(state.change(which as usize, button_name(button), false));
                }
                SdlEvent::ControllerAxisMotion {
                    which, axis, value, ..
                } => changes = state.controller_axis(which as usize, axis, value),
                SdlEvent::JoyButtonDown {
                    which, button_idx, ..
                }
                | SdlEvent::JoyButtonUp {
                    which, button_idx, ..
                } if devices.get(&which).is_some_and(Device::raw) => {
                    changes.extend(state.change(
                        which as usize,
                        format!("SdlRawButton:{button_idx}"),
                        matches!(event, SdlEvent::JoyButtonDown { .. }),
                    ));
                }
                SdlEvent::JoyAxisMotion {
                    which,
                    axis_idx,
                    value,
                    ..
                } if devices.get(&which).is_some_and(Device::raw) => {
                    changes = state.axis(
                        which as usize,
                        &format!("SdlRawAxis:{axis_idx}-"),
                        &format!("SdlRawAxis:{axis_idx}+"),
                        value as f32 / 32767.0,
                    );
                }
                SdlEvent::JoyHatMotion {
                    which,
                    hat_idx,
                    state: hat,
                    ..
                } if devices.get(&which).is_some_and(Device::raw) => {
                    changes = state.hat(which as usize, hat_idx, hat);
                }
                _ => {}
            }
            for change in changes {
                publish(tx, ctx, change)?;
            }
        }
        // Explicit refresh also works when SDL's automatic event update is
        // disabled. Drain queued transitions first so a short down/up pair is
        // never replaced by its final snapshot. Transitions deduplicates both
        // paths, including releases, and egui still owns the focus policy.
        for device in devices.values() {
            if !device.attached() {
                continue;
            }
            let changes = match device {
                Device::Controller(controller) => poll_controller(controller, &mut state),
                Device::Raw(joystick) => {
                    let id = joystick.instance_id() as usize;
                    let mut changes = Vec::new();
                    for button in 0..joystick.num_buttons().min(256) {
                        if let Ok(down) = joystick.button(button) {
                            changes.extend(state.change(
                                id,
                                format!("SdlRawButton:{button}"),
                                down,
                            ));
                        }
                    }
                    for axis in 0..joystick.num_axes().min(256) {
                        if let Ok(value) = joystick.axis(axis) {
                            changes.extend(state.axis(
                                id,
                                &format!("SdlRawAxis:{axis}-"),
                                &format!("SdlRawAxis:{axis}+"),
                                value as f32 / 32767.0,
                            ));
                        }
                    }
                    for hat in 0..joystick.num_hats().min(256) {
                        if let Ok(value) = joystick.hat(hat) {
                            changes.extend(state.hat(id, hat as u8, value));
                        }
                    }
                    changes
                }
            };
            for change in changes {
                publish(tx, ctx, change)?;
            }
        }
        if Instant::now() >= next_scan {
            let gone: Vec<_> = devices
                .iter()
                .filter(|(_, d)| !d.attached())
                .map(|(id, _)| *id)
                .collect();
            let mut changed = false;
            for id in gone {
                devices.remove(&id);
                for change in state.disconnect(id as usize) {
                    publish(tx, ctx, change)?;
                }
                changed = true;
            }
            changed |= scan(&controllers, &joysticks, &mut devices)?;
            if changed {
                publish(tx, ctx, list(&devices))?;
            }
            next_scan = Instant::now() + Duration::from_secs(1);
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    for device in devices.values_mut() {
        if device.has_rumble() {
            let _ = device.rumble(false);
        }
    }
    Ok(())
}
pub struct Monitor {
    pub rx: Receiver<Event>,
    alive: Arc<AtomicBool>,
    profile: Arc<AtomicU8>,
    selected: Arc<AtomicUsize>,
    enabled: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _commands: SyncSender<crate::rumble::Command>,
}
impl Monitor {
    pub fn set_deadzone(&self, value: pocket_library::DpadDeadzone) {
        self.profile.store(
            match value {
                pocket_library::DpadDeadzone::Small => 0,
                pocket_library::DpadDeadzone::Medium => 1,
                pocket_library::DpadDeadzone::Large => 2,
            },
            Ordering::Relaxed,
        );
    }
    pub fn set_output(&self, device: Option<usize>, enabled: bool) {
        self.selected
            .store(device.unwrap_or(usize::MAX), Ordering::Relaxed);
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub fn start(ctx: egui::Context) -> Monitor {
    let (tx, rx) = mpsc::channel();
    let alive = Arc::new(AtomicBool::new(true));
    let worker_alive = alive.clone();
    let profile = Arc::new(AtomicU8::new(1));
    let worker_profile = profile.clone();
    let selected = Arc::new(AtomicUsize::new(usize::MAX));
    let worker_selected = selected.clone();
    let enabled = Arc::new(AtomicBool::new(false));
    let worker_enabled = enabled.clone();
    let (command_tx, command_rx) = mpsc::sync_channel(32);
    pocket_core::kernel::vibration::install_host(Arc::new(crate::rumble::Bridge::new(
        command_tx.clone(),
    )));
    let thread = std::thread::Builder::new()
        .name("gamepad-input".into())
        .spawn(move || {
            if let Err(error) = worker(
                &ctx,
                &tx,
                &worker_alive,
                &worker_profile,
                &worker_selected,
                &worker_enabled,
                &command_rx,
            ) {
                let _ = publish(&tx, &ctx, Event::Error(error));
            }
        })
        .expect("spawn controller worker");
    Monitor {
        rx,
        alive,
        profile,
        selected,
        enabled,
        thread: Some(thread),
        _commands: command_tx,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directional_presets_leave_digital_hat_contacts_direct() {
        for zone in pocket_library::DpadDeadzone::ALL {
            let mut state = Transitions {
                deadzone: zone,
                ..Transitions::default()
            };
            assert_eq!(state.hat(1, 0, HatState::RightUp).len(), 2);
            assert_eq!(state.hat(1, 0, HatState::Centered).len(), 2);
            assert!(state.controller_axis(1, Axis::LeftX, 0).is_empty());
        }
        let mut small = Transitions {
            deadzone: pocket_library::DpadDeadzone::Small,
            ..Transitions::default()
        };
        let mut large = Transitions {
            deadzone: pocket_library::DpadDeadzone::Large,
            ..Transitions::default()
        };
        assert_eq!(small.controller_axis(1, Axis::LeftX, 16000).len(), 1);
        assert!(large.controller_axis(1, Axis::LeftX, 16000).is_empty());
        assert_eq!(large.controller_axis(1, Axis::LeftX, 26000).len(), 1);
        assert!(large.controller_axis(1, Axis::LeftX, 20000).is_empty());
        assert_eq!(large.controller_axis(1, Axis::LeftX, 0).len(), 1);
    }
    #[test]
    fn stick_hysteresis_and_crossing_release_before_press() {
        let mut state = Transitions::default();
        assert!(state.controller_axis(1, Axis::LeftX, 19000).is_empty());
        let down = state.controller_axis(1, Axis::LeftX, 25000);
        assert!(
            matches!(&down[..],[Event::Input{control,down:true,..}] if control=="LeftStickRight")
        );
        assert!(state.controller_axis(1, Axis::LeftX, 15000).is_empty());
        let cross = state.controller_axis(1, Axis::LeftX, -25000);
        assert!(
            matches!(&cross[..],[Event::Input{control:a,down:false,..},Event::Input{control:b,down:true,..}] if a=="LeftStickRight"&&b=="LeftStickLeft")
        );
        assert_eq!(state.disconnect(1).len(), 1);
        assert!(state.disconnect(1).is_empty());
    }
    #[test]
    fn position_buttons_y_direction_triggers_and_diagonal_hats() {
        assert_eq!(button_name(Button::A), "South");
        assert_eq!(button_name(Button::B), "East");
        assert_eq!(button_name(Button::X), "West");
        assert_eq!(button_name(Button::Y), "North");
        let mut state = Transitions::default();
        assert!(
            matches!(&state.controller_axis(1,Axis::LeftY,-25000)[..],[Event::Input{control,down:true,..}] if control=="LeftStickUp")
        );
        assert!(
            matches!(&state.controller_axis(1,Axis::TriggerLeft,30000)[..],[Event::Input{control,down:true,..}] if control=="LeftTrigger2")
        );
        assert_eq!(state.hat(2, 0, HatState::RightUp).len(), 2);
        assert_eq!(state.hat(2, 0, HatState::Centered).len(), 2);
        assert!(state.change(1, "South".into(), true).is_some());
        assert!(state.change(1, "South".into(), true).is_none());
    }
    #[test]
    fn native_sdl_virtual_controller_queues_short_press_and_release() {
        sdl2::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
        sdl2::hint::set("SDL_GAMECONTROLLER_USE_BUTTON_LABELS", "0");
        let sdl = sdl2::init().unwrap();
        let joysticks = sdl.joystick().unwrap();
        let controllers = sdl.game_controller().unwrap();
        let index = unsafe {
            sdl2::sys::SDL_JoystickAttachVirtual(
                sdl2::sys::SDL_JoystickType::SDL_JOYSTICK_TYPE_GAMECONTROLLER,
                6,
                15,
                0,
            )
        };
        assert!(index >= 0, "{}", sdl2::get_error());
        let joystick = joysticks.open(index as u32).unwrap();
        let controller = controllers.open(index as u32).unwrap();
        let id = controller.instance_id();
        let mut events = sdl.event_pump().unwrap();
        events.poll_iter().for_each(drop);
        let pointer = unsafe { sdl2::sys::SDL_JoystickFromInstanceID(id as i32) };
        assert!(!pointer.is_null());
        assert_eq!(
            unsafe { sdl2::sys::SDL_JoystickSetVirtualButton(pointer, 0, 1) },
            0
        );
        joysticks.update();
        assert_eq!(
            unsafe { sdl2::sys::SDL_JoystickSetVirtualButton(pointer, 0, 0) },
            0
        );
        joysticks.update();
        let queued: Vec<_> = events
            .poll_iter()
            .filter_map(|event| match event {
                SdlEvent::ControllerButtonDown { which, button, .. } if which == id => {
                    Some((button_name(button), true))
                }
                SdlEvent::ControllerButtonUp { which, button, .. } if which == id => {
                    Some((button_name(button), false))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            queued,
            vec![("South".into(), true), ("South".into(), false)]
        );
        // Direct-state fallback: capture still works if no queued
        // button event reaches the consumer, without duplicating a later event.
        let mut state = Transitions::default();
        assert_eq!(
            unsafe { sdl2::sys::SDL_JoystickSetVirtualButton(pointer, 0, 1) },
            0
        );
        joysticks.update();
        events.poll_iter().for_each(drop);
        let down = poll_controller(&controller, &mut state);
        assert!(
            matches!(&down[..], [Event::Input { control, down: true, .. }] if control == "South")
        );
        assert!(poll_controller(&controller, &mut state).is_empty());
        assert!(state.change(id as usize, "South".into(), true).is_none());
        assert_eq!(
            unsafe { sdl2::sys::SDL_JoystickSetVirtualButton(pointer, 0, 0) },
            0
        );
        joysticks.update();
        events.poll_iter().for_each(drop);
        let up = poll_controller(&controller, &mut state);
        assert!(
            matches!(&up[..], [Event::Input { control, down: false, .. }] if control == "South")
        );
        assert!(poll_controller(&controller, &mut state).is_empty());
        drop(controller);
        drop(joystick);
        assert_eq!(unsafe { sdl2::sys::SDL_JoystickDetachVirtual(index) }, 0);
    }
}
