use std::{
    cell::RefCell,
    collections::HashSet,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use minifb::{InputCallback, Key, MouseButton, MouseMode, Window, WindowOptions};
use pocket_core::kernel::{FrameAction, FrameHook, InputEvent, KernelState};

const DISPLAY_UPDATE_INTERVAL: Duration = Duration::from_millis(16);
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(4);

#[derive(Default)]
struct Keyboard {
    held: HashSet<Key>,
    forwarded: HashSet<u16>,
    events: Vec<InputEvent>,
    text_input: bool,
}

impl Keyboard {
    fn set_key(&mut self, key: Key, down: bool) {
        let Some(vk) = virtual_key(key) else {
            return;
        };
        let was_down = self.held.iter().any(|k| virtual_key(*k) == Some(vk));
        if down {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
        let is_down = self.held.iter().any(|k| virtual_key(*k) == Some(vk));
        let control = self.held.contains(&Key::LeftCtrl) || self.held.contains(&Key::RightCtrl);
        let committed_text = self.text_input
            && !control
            && (vk == 0x20
                || (0x30..=0x5a).contains(&vk)
                || (0x60..=0x6f).contains(&vk)
                || (0xba..=0xde).contains(&vk));
        if was_down != is_down {
            if is_down && !committed_text {
                self.forwarded.insert(vk);
                self.events.push(InputEvent::KeyDown { vk });
            } else if !is_down && self.forwarded.remove(&vk) {
                self.events.push(InputEvent::KeyUp { vk });
            }
        }
    }

    fn release(&mut self) {
        let mut keys: Vec<_> = self.forwarded.drain().collect();
        keys.sort_unstable();
        keys.dedup();
        self.held.clear();
        self.events
            .extend(keys.into_iter().map(|vk| InputEvent::KeyUp { vk }));
    }
}

struct KeyboardCallback(Rc<RefCell<Keyboard>>);
impl InputCallback for KeyboardCallback {
    fn add_char(&mut self, character: u32) {
        // Committed text supplies printable characters in native text mode.
        // Enter/Backspace/Ctrl+C already have key edges; do not duplicate them.
        let text_input = self.0.borrow().text_input;
        if let Some(character) =
            char::from_u32(character).filter(|c| !c.is_control() && (text_input || !c.is_ascii()))
        {
            self.0
                .borrow_mut()
                .events
                .push(InputEvent::Text { character });
        }
    }
    fn set_key_state(&mut self, key: Key, down: bool) {
        self.0.borrow_mut().set_key(key, down);
    }
}

pub struct DisplayHook {
    window: Window,
    buffer: Vec<u32>,
    guest_size: (u32, u32),
    image_size: (usize, usize),
    last_frame: u64,
    last_emit: Option<Instant>,
    last_poll: Instant,
    keyboard: Rc<RefCell<Keyboard>>,
    pointer: Option<(u16, u16)>,
    pointer_down: bool,
}

impl DisplayHook {
    pub fn set_text_input(&mut self, enabled: bool) {
        self.keyboard.borrow_mut().text_input = enabled;
    }
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let (w, h) = (width as usize, height as usize);
        let mut window = Window::new(
            "PocketHLE",
            w,
            h,
            WindowOptions {
                resize: true,
                scale: minifb::Scale::X2,
                scale_mode: minifb::ScaleMode::AspectRatioStretch,
                ..WindowOptions::default()
            },
        )
        .context("opening minifb window")?;
        // minifb defaults to a blocking 250 FPS limiter. The emulator owns
        // pacing; input polling must not throttle every CPU slice.
        window.set_target_fps(0);
        let keyboard = Rc::new(RefCell::new(Keyboard::default()));
        window.set_input_callback(Box::new(KeyboardCallback(keyboard.clone())));
        Ok(Self {
            window,
            buffer: vec![0; w * h],
            guest_size: (width, height),
            image_size: (w, h),
            last_frame: u64::MAX,
            last_emit: None,
            last_poll: Instant::now(),
            keyboard,
            pointer: None,
            pointer_down: false,
        })
    }

    pub fn present_pixels(&mut self, width: u32, height: u32, pixels: &[u8]) -> Result<()> {
        let (w, h) = (width as usize, height as usize);
        let count = w.checked_mul(h).context("display image size overflow")?;
        anyhow::ensure!(
            w > 0 && h > 0 && count.checked_mul(2) == Some(pixels.len()),
            "invalid RGB565 display image"
        );
        self.buffer.resize(count, 0);
        for (src, px) in pixels.chunks_exact(2).zip(&mut self.buffer) {
            *px = rgb565_to_minifb(u16::from_le_bytes([src[0], src[1]]));
        }
        self.image_size = (w, h);
        self.window
            .update_with_buffer(&self.buffer, w, h)
            .context("presenting minifb image")?;
        self.last_poll = Instant::now();
        Ok(())
    }

    pub fn poll_input(&mut self) -> (bool, Vec<InputEvent>) {
        if self.last_poll.elapsed() >= INPUT_POLL_INTERVAL {
            self.window.update();
            self.last_poll = Instant::now();
        }
        let active = self.window.is_open() && self.window.is_active();
        if !active {
            self.keyboard.borrow_mut().release();
        }
        let mut events = std::mem::take(&mut self.keyboard.borrow_mut().events);
        let down = active && self.window.get_mouse_down(MouseButton::Left);
        let position = self
            .window
            .get_unscaled_mouse_pos(MouseMode::Pass)
            .and_then(|position| {
                map_pointer(
                    position,
                    self.window.get_size(),
                    self.image_size,
                    self.guest_size,
                    self.pointer_down,
                )
            });
        if let Some((x, y)) = position {
            if down && !self.pointer_down {
                events.push(InputEvent::PointerDown { x, y });
                self.pointer_down = true;
            } else if self.pointer_down && down && self.pointer != position {
                events.push(InputEvent::PointerMove { x, y });
            }
            self.pointer = position;
        }
        if self.pointer_down && !down {
            if let Some((x, y)) = self.pointer {
                events.push(InputEvent::PointerUp { x, y });
            }
            self.pointer_down = false;
        }
        (self.window.is_open(), events)
    }
}

impl FrameHook for DisplayHook {
    fn on_frame(&mut self, state: &mut KernelState) -> FrameAction {
        self.set_text_input(state.accepts_text_input());
        let counter = state.framebuffer.frame_counter;
        if counter != self.last_frame
            && self
                .last_emit
                .is_none_or(|at| at.elapsed() >= DISPLAY_UPDATE_INTERVAL)
        {
            self.last_frame = counter;
            self.last_emit = Some(Instant::now());
            let result = state
                .presentation_framebuffer()
                .map_err(anyhow::Error::msg)
                .and_then(|frame| self.present_pixels(frame.width, frame.height, &frame.pixels));
            if let Err(error) = result {
                log::error!("window presentation failed: {error:#}");
                return FrameAction::Stop;
            }
        }
        let (open, events) = self.poll_input();
        state.pending_input.extend(events);
        if open {
            FrameAction::Continue
        } else {
            FrameAction::Stop
        }
    }
}

fn map_pointer(
    position: (f32, f32),
    window: (usize, usize),
    image: (usize, usize),
    guest: (u32, u32),
    clamp: bool,
) -> Option<(u16, u16)> {
    if window.0 == 0
        || window.1 == 0
        || image.0 == 0
        || image.1 == 0
        || guest.0 == 0
        || guest.1 == 0
    {
        return None;
    }
    // Match minifb's integer aspect-fit rectangle, including letterboxing.
    // Its Windows get_mouse_pos does not account for window resizing.
    let aspect = image.0 as f32 / image.1 as f32;
    let (w, h) = if aspect > window.0 as f32 / window.1 as f32 {
        (window.0, (window.0 as f32 / aspect) as usize)
    } else {
        ((window.1 as f32 * aspect) as usize, window.1)
    };
    if w == 0 || h == 0 {
        return None;
    }
    let (x, y) = (
        position.0 - ((window.0 - w) / 2) as f32,
        position.1 - ((window.1 - h) / 2) as f32,
    );
    if !x.is_finite()
        || !y.is_finite()
        || !clamp && (x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32)
    {
        return None;
    }
    Some((
        (x * guest.0 as f32 / w as f32)
            .floor()
            .clamp(0.0, (guest.0 - 1).min(u16::MAX as u32) as f32) as u16,
        (y * guest.1 as f32 / h as f32)
            .floor()
            .clamp(0.0, (guest.1 - 1).min(u16::MAX as u32) as f32) as u16,
    ))
}

fn virtual_key(key: Key) -> Option<u16> {
    use Key::*;
    Some(match key {
        key if (Key0..=Key9).contains(&key) => b'0' as u16 + key as u16 - Key0 as u16,
        key if (A..=Z).contains(&key) => b'A' as u16 + key as u16 - A as u16,
        key if (F1..=F15).contains(&key) => 0x70 + key as u16 - F1 as u16,
        key if (NumPad0..=NumPad9).contains(&key) => 0x60 + key as u16 - NumPad0 as u16,
        Backspace => 0x08,
        Tab => 0x09,
        Enter | NumPadEnter => 0x0d,
        LeftShift | RightShift => 0x10,
        LeftCtrl | RightCtrl => 0x11,
        LeftAlt | RightAlt => 0x12,
        Pause => 0x13,
        CapsLock => 0x14,
        Escape => 0x1b,
        Space => 0x20,
        PageUp => 0x21,
        PageDown => 0x22,
        End => 0x23,
        Home => 0x24,
        Left => 0x25,
        Up => 0x26,
        Right => 0x27,
        Down => 0x28,
        Insert => 0x2d,
        Delete => 0x2e,
        LeftSuper => 0x5b,
        RightSuper => 0x5c,
        Menu => 0x5d,
        NumPadAsterisk => 0x6a,
        NumPadPlus => 0x6b,
        NumPadMinus => 0x6d,
        NumPadDot => 0x6e,
        NumPadSlash => 0x6f,
        NumLock => 0x90,
        ScrollLock => 0x91,
        Semicolon => 0xba,
        Equal => 0xbb,
        Comma => 0xbc,
        Minus => 0xbd,
        Period => 0xbe,
        Slash => 0xbf,
        Backquote => 0xc0,
        LeftBracket => 0xdb,
        Backslash => 0xdc,
        RightBracket => 0xdd,
        Apostrophe => 0xde,
        _ => return None,
    })
}

fn rgb565_to_minifb(p: u16) -> u32 {
    let (r, g, b) = (
        u32::from((p >> 11) & 31),
        u32::from((p >> 5) & 63),
        u32::from(p & 31),
    );
    (((r << 3) | (r >> 2)) << 16) | (((g << 2) | (g >> 4)) << 8) | (b << 3) | (b >> 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resized_letterboxed_pointer_uses_guest_coordinates() {
        assert_eq!(
            map_pointer((500.0, 350.0), (1000, 700), (640, 480), (640, 480), false),
            Some((320, 240))
        );
        assert_eq!(
            map_pointer((0.0, 350.0), (1000, 700), (640, 480), (640, 480), false),
            None
        );
        assert_eq!(
            map_pointer((0.0, 350.0), (1000, 700), (640, 480), (640, 480), true),
            Some((0, 240))
        );
        assert_eq!(
            map_pointer((999.0, 699.0), (1000, 700), (640, 480), (640, 480), true),
            Some((639, 479))
        );
        assert_eq!(
            map_pointer((640.0, 480.0), (1280, 960), (1280, 960), (640, 480), false),
            Some((320, 240))
        );
    }

    #[test]
    fn keyboard_keeps_edges_and_combines_modifiers() {
        let mut keyboard = Keyboard::default();
        keyboard.set_key(Key::A, true);
        keyboard.set_key(Key::A, true);
        keyboard.set_key(Key::A, false);
        keyboard.set_key(Key::LeftShift, true);
        keyboard.set_key(Key::RightShift, true);
        keyboard.set_key(Key::LeftShift, false);
        keyboard.release();
        assert_eq!(
            keyboard.events,
            vec![
                InputEvent::KeyDown { vk: 0x41 },
                InputEvent::KeyUp { vk: 0x41 },
                InputEvent::KeyDown { vk: 0x10 },
                InputEvent::KeyUp { vk: 0x10 }
            ]
        );
    }
    #[test]
    fn native_console_text_avoids_duplicate_controls_and_releases_ctrl_keys_in_order() {
        let keyboard = Rc::new(RefCell::new(Keyboard {
            text_input: true,
            ..Keyboard::default()
        }));
        let mut callback = KeyboardCallback(keyboard.clone());
        callback.set_key_state(Key::A, true);
        callback.add_char('a' as u32);
        callback.set_key_state(Key::A, false);
        callback.set_key_state(Key::Enter, true);
        callback.add_char(13);
        callback.set_key_state(Key::Enter, false);
        callback.set_key_state(Key::LeftCtrl, true);
        callback.set_key_state(Key::C, true);
        callback.add_char(3);
        callback.set_key_state(Key::LeftCtrl, false);
        callback.set_key_state(Key::C, false);
        assert_eq!(
            keyboard.borrow().events,
            [
                InputEvent::Text { character: 'a' },
                InputEvent::KeyDown { vk: 13 },
                InputEvent::KeyUp { vk: 13 },
                InputEvent::KeyDown { vk: 0x11 },
                InputEvent::KeyDown { vk: 0x43 },
                InputEvent::KeyUp { vk: 0x11 },
                InputEvent::KeyUp { vk: 0x43 },
            ]
        );
        assert!(keyboard.borrow().forwarded.is_empty());
    }
}
