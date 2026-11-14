//! The CE CON: device is shared by a command interpreter and its children.
//! Pocket CMD supplies the parser, commands and batch processing; this module
//! owns only the console's bounded screen, line editor and input queue.
use crate::{
    gdi::{Bitmap, Font, Surface},
    KernelState,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

pub type SharedConsole = Arc<Mutex<Device>>;
pub const ECHO_INPUT: u32 = 1;
pub const LINE_INPUT: u32 = 2;
pub const PROCESSED_OUTPUT: u32 = 4;
const SCROLLBACK: usize = 512;
const INPUT_LIMIT: usize = 4096;
const QUEUE_LIMIT: usize = 65536;

#[derive(Debug)]
pub struct Device {
    pub mode: u32,
    pub title: String,
    pub columns: usize,
    pub rows: usize,
    pub revision: u64,
    pub interrupt_revision: u64,
    pub owner_process: Option<u32>,
    lines: VecDeque<Vec<char>>,
    column: usize,
    edit: Vec<char>,
    edit_cursor: usize,
    ready: VecDeque<u16>,
    eof: bool,
    history: VecDeque<Vec<char>>,
    history_index: Option<usize>,
    history_draft: Vec<char>,
    scroll_offset: usize,
    wheel_remainder: i32,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            mode: ECHO_INPUT | LINE_INPUT | PROCESSED_OUTPUT,
            title: "Command Prompt".into(),
            columns: 80,
            rows: 25,
            revision: 0,
            interrupt_revision: 0,
            owner_process: None,
            lines: VecDeque::from([Vec::new()]),
            column: 0,
            edit: Vec::new(),
            edit_cursor: 0,
            ready: VecDeque::new(),
            eof: false,
            history: VecDeque::new(),
            history_index: None,
            history_draft: Vec::new(),
            scroll_offset: 0,
            wheel_remainder: 0,
        }
    }
}

#[derive(Debug)]
pub struct Snapshot {
    pub lines: Vec<Vec<char>>,
    pub caret: Option<(usize, usize)>,
}

impl Device {
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn resize(&mut self, columns: usize, rows: usize) {
        let size = (columns.clamp(1, 256), rows.clamp(1, 200));
        if size != (self.columns, self.rows) {
            (self.columns, self.rows) = size;
            self.column = self.column.min(self.columns);
            self.changed();
        }
    }

    pub fn set_mode(&mut self, mode: u32) {
        if self.mode != mode {
            self.mode = mode;
            self.changed();
        }
    }

    pub fn set_title(&mut self, title: String) {
        if self.title != title {
            self.title = title;
            self.changed();
        }
    }

    fn newline(&mut self) {
        if self.scroll_offset != 0 {
            self.scroll_offset = self.scroll_offset.saturating_add(1);
        }
        self.lines.push_back(Vec::new());
        if self.lines.len() > SCROLLBACK {
            self.lines.pop_front();
        }
        self.column = 0;
    }

    fn put(&mut self, character: char) {
        if self.column >= self.columns {
            self.newline();
        }
        let line = self.lines.back_mut().unwrap();
        line.resize(line.len().max(self.column + 1), ' ');
        line[self.column] = character;
        self.column += 1;
    }

    pub fn write_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        for c in text.chars() {
            if self.mode & PROCESSED_OUTPUT != 0 {
                match c {
                    '\r' => self.column = 0,
                    '\n' => self.newline(),
                    '\x08' => self.column = self.column.saturating_sub(1),
                    '\t' => {
                        let spaces = 8 - self.column % 8;
                        for _ in 0..spaces {
                            self.put(' ');
                        }
                    }
                    '\x07' => {}
                    c => self.put(c),
                }
            } else {
                self.put(c);
            }
        }
        self.changed();
    }

    pub fn clear_screen(&mut self) {
        self.lines = VecDeque::from([Vec::new()]);
        self.column = 0;
        self.scroll_offset = 0;
        self.changed();
    }

    pub fn flush_input(&mut self) {
        self.ready.clear();
        self.edit.clear();
        self.edit_cursor = 0;
        self.eof = false;
        self.history_index = None;
        self.changed();
    }

    pub fn input_available(&self) -> bool {
        !self.ready.is_empty() || self.eof
    }
    pub fn at_eof(&self) -> bool {
        self.eof && self.ready.is_empty()
    }

    /// None means a pending read, rather than EOF. CRT callers must yield with
    /// their arguments intact until input arrives or Ctrl+Z requests EOF.
    pub fn read_utf16(&mut self, capacity: usize, line: bool) -> Option<Vec<u16>> {
        if capacity == 0 {
            return Some(Vec::new());
        }
        if !self.input_available() {
            return None;
        }
        let mut result = Vec::with_capacity(capacity.min(self.ready.len()));
        while result.len() < capacity {
            let Some(unit) = self.ready.pop_front() else {
                break;
            };
            result.push(unit);
            if line && unit == 10 {
                break;
            }
        }
        Some(result)
    }

    fn commit(&mut self, newline: bool) {
        let text: String = self.edit.iter().collect();
        let units: Vec<_> = text.encode_utf16().chain(newline.then_some(10)).collect();
        if self.ready.len() + units.len() > QUEUE_LIMIT {
            return;
        }
        self.ready.extend(units);
        if self.mode & ECHO_INPUT != 0 {
            self.write_text(&text);
            if newline {
                self.write_text("\r\n");
            }
        }
        if !self.edit.is_empty() && self.history.back() != Some(&self.edit) {
            self.history.push_back(self.edit.clone());
            if self.history.len() > 100 {
                self.history.pop_front();
            }
        }
        self.edit.clear();
        self.edit_cursor = 0;
        self.history_index = None;
        self.changed();
    }

    pub fn character(&mut self, c: char) {
        match c {
            '\r' | '\n' => {
                self.key(13, false);
                return;
            }
            '\x08' => {
                self.key(8, false);
                return;
            }
            '\x03' => {
                self.interrupt();
                return;
            }
            '\x1a' => {
                self.key(0x5a, true);
                return;
            }
            c if c.is_control() && c != '\t' => return,
            _ => {}
        }
        self.eof = false;
        self.scroll_offset = 0;
        if self.mode & LINE_INPUT == 0 {
            if self.ready.len() + c.len_utf16() <= QUEUE_LIMIT {
                let mut units = [0; 2];
                self.ready
                    .extend(c.encode_utf16(&mut units).iter().copied());
                if self.mode & ECHO_INPUT != 0 {
                    self.write_text(&c.to_string());
                }
            }
        } else if self.edit.len() < INPUT_LIMIT {
            self.edit.insert(self.edit_cursor, c);
            self.edit_cursor += 1;
        }
        self.changed();
    }

    pub fn interrupt(&mut self) {
        self.edit.clear();
        self.edit_cursor = 0;
        self.history_index = None;
        self.eof = false;
        self.interrupt_revision = self.interrupt_revision.wrapping_add(1);
        if self.ready.len() < QUEUE_LIMIT {
            self.ready.push_back(10);
        }
        self.write_text("^C\r\n");
        self.changed();
    }

    pub fn key(&mut self, vk: u16, control: bool) {
        if matches!(vk, 0x21 | 0x22) {
            let rows = self.rows.saturating_sub(1).max(1) as i32;
            self.scroll(if vk == 0x21 { rows } else { -rows });
            return;
        }
        if control && matches!(vk, 0x24 | 0x23) {
            self.scroll(if vk == 0x24 { i32::MAX } else { i32::MIN });
            return;
        }
        if control {
            if matches!(vk, 0x43 | 0x5a | 0x44) {
                self.scroll_offset = 0;
            }
            match vk {
                0x43 => self.interrupt(),
                0x5a | 0x44 => {
                    self.commit(false);
                    self.eof = true;
                    self.changed();
                }
                _ => {}
            }
            return;
        }
        if vk == 9 {
            self.character('\t');
            return;
        }
        if matches!(vk, 8 | 13 | 0x1b | 0x23..=0x28 | 0x2e) {
            self.scroll_offset = 0;
        }
        if self.mode & LINE_INPUT == 0 {
            if matches!(vk, 8 | 13) && self.ready.len() < QUEUE_LIMIT {
                self.ready.push_back(vk);
                if self.mode & ECHO_INPUT != 0 {
                    self.write_text(if vk == 13 { "\r\n" } else { "\x08 \x08" });
                }
            }
            return;
        }
        match vk {
            13 => self.commit(true),
            8 if self.edit_cursor > 0 => {
                self.edit_cursor -= 1;
                self.edit.remove(self.edit_cursor);
            }
            0x2e if self.edit_cursor < self.edit.len() => {
                self.edit.remove(self.edit_cursor);
            }
            0x25 => self.edit_cursor = self.edit_cursor.saturating_sub(1),
            0x27 => self.edit_cursor = (self.edit_cursor + 1).min(self.edit.len()),
            0x24 => self.edit_cursor = 0,
            0x23 => self.edit_cursor = self.edit.len(),
            0x1b => {
                self.edit.clear();
                self.edit_cursor = 0;
                self.history_index = None;
            }
            0x26 if !self.history.is_empty() => {
                if self.history_index.is_none() {
                    self.history_draft = self.edit.clone();
                }
                let index = self
                    .history_index
                    .unwrap_or(self.history.len())
                    .saturating_sub(1);
                self.history_index = Some(index);
                self.edit = self.history[index].clone();
                self.edit_cursor = self.edit.len();
            }
            0x28 if self.history_index.is_some() => {
                let index = self.history_index.unwrap() + 1;
                if index < self.history.len() {
                    self.history_index = Some(index);
                    self.edit = self.history[index].clone();
                } else {
                    self.history_index = None;
                    self.edit = self.history_draft.clone();
                }
                self.edit_cursor = self.edit.len();
            }
            _ => return,
        }
        self.changed();
    }

    /// Positive rows scroll toward older output. Clamp against bounded history.
    pub fn scroll(&mut self, rows: i32) {
        let max = self.lines.len().saturating_sub(self.rows);
        let offset = (self.scroll_offset as i64 + i64::from(rows)).clamp(0, max as i64) as usize;
        if offset != self.scroll_offset {
            self.scroll_offset = offset;
            self.changed();
        }
    }

    pub fn wheel(&mut self, delta: i16) {
        self.wheel_remainder += i32::from(delta);
        let steps = self.wheel_remainder / 120;
        self.wheel_remainder %= 120;
        self.scroll(steps * 3);
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut lines: Vec<_> = self.lines.iter().cloned().collect();
        let mut row = lines.len() - 1;
        let mut col = self.column;
        let mut caret = (row, col);
        if self.mode & ECHO_INPUT != 0 {
            for (i, c) in self.edit.iter().enumerate() {
                if col >= self.columns {
                    lines.push(Vec::new());
                    row += 1;
                    col = 0;
                }
                if i == self.edit_cursor {
                    caret = (row, col);
                }
                let width = lines[row].len().max(col + 1);
                lines[row].resize(width, ' ');
                lines[row][col] = *c;
                col += 1;
            }
            if self.edit_cursor == self.edit.len() {
                caret = (row, col);
            }
        }
        if caret.1 >= self.columns {
            lines.push(Vec::new());
            caret = (caret.0 + 1, 0);
        }
        let start = lines
            .len()
            .saturating_sub(self.rows)
            .saturating_sub(self.scroll_offset);
        let end = start + self.rows;
        let caret =
            (caret.0 >= start && caret.0 < end).then_some((caret.0.saturating_sub(start), caret.1));
        Snapshot {
            lines: lines.into_iter().skip(start).take(self.rows).collect(),
            caret,
        }
    }
}

pub struct View {
    pub hwnd: u32,
    pub thread: usize,
    revision: Option<u64>,
    size: (u32, u32),
    active: bool,
}

pub struct State {
    pub paths: [String; 3],
    pub streams: [Option<u32>; 3],
    pub closed: [bool; 3],
    pub view: Option<View>,
    pub inherited: bool,
    pub control_event: u32,
    pub control_handler: u32,
    pub interrupt_seen: u64,
    pub callback: Option<(usize, u32, crate::GuestCallFrame)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            paths: std::array::from_fn(|_| "CON:".into()),
            streams: [None; 3],
            closed: [false; 3],
            view: None,
            inherited: false,
            control_event: 0,
            control_handler: 0,
            interrupt_seen: 0,
            callback: None,
        }
    }
}

#[derive(Clone)]
pub struct Context {
    device: Option<SharedConsole>,
    paths: [String; 3],
    files: [Option<crate::vfs::VfsObject>; 3],
    closed: [bool; 3],
    modes: [bool; 3],
}

fn font() -> Font {
    let mut f = Font::new(-14);
    for (i, u) in "Courier New".encode_utf16().enumerate() {
        f.logfont[28 + i * 2..30 + i * 2].copy_from_slice(&u.to_le_bytes());
    }
    f
}

impl KernelState {
    pub(crate) fn release_console_window(&mut self) {
        if self.console.view.take().is_none() {
            return;
        }
        if let Some(device) = self.vfs.console_device() {
            let mut device = device.lock().unwrap_or_else(|e| e.into_inner());
            if device.owner_process == Some(self.object_handles.process_id()) {
                device.owner_process = None;
            }
        }
    }

    pub fn console_context(&self) -> Context {
        Context {
            device: self.vfs.console_device(),
            paths: self.console.paths.clone(),
            files: std::array::from_fn(|i| {
                self.console.streams[i].and_then(|h| self.vfs.export_handle(h))
            }),
            closed: self.console.closed,
            modes: self.vfs.stdio_text_modes,
        }
    }

    pub fn attach_console_context(&mut self, context: Context) {
        self.console.paths = context.paths;
        self.console.closed = context.closed;
        self.vfs.stdio_text_modes = context.modes;
        self.console.inherited = context.device.is_some();
        if let Some(device) = context.device {
            self.console.interrupt_seen = device
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .interrupt_revision;
            self.vfs.attach_console_device(device);
        }
        for (id, file) in context.files.into_iter().enumerate() {
            if let Some(file) = file {
                if let Some(handle) = self.object_handles.reserve() {
                    self.vfs.import_handle(handle, file);
                    let object = if self.vfs.is_open(handle) {
                        crate::handles::HandleObject::File(handle)
                    } else {
                        crate::handles::HandleObject::Device(handle)
                    };
                    self.object_handles.bind(handle, object);
                    self.console.streams[id] = Some(handle);
                }
            }
        }
    }

    pub fn ensure_console_window(&mut self) {
        if self.console.view.is_some() {
            return;
        }
        if self.console.inherited
            && self.vfs.console_device().is_some_and(|d| {
                d.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .owner_process
                    .is_some()
            })
        {
            return;
        }
        let Some(hwnd) = (0xdead_1000..0xdead_2000)
            .rev()
            .find(|h| !self.window_classes.contains_key(h) && !self.window_rects.contains_key(h))
        else {
            return;
        };
        self.vfs
            .ensure_console_device()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .owner_process = Some(self.object_handles.process_id());
        self.ce.windows.enabled = true;
        let w = (self.framebuffer.width as i32 - 16).clamp(112, 650);
        let h = (self.framebuffer.height as i32 - 48).clamp(64, 420);
        self.window_rects.insert(hwnd, (8, 8, w, h));
        self.window_styles.insert(hwnd, (0x10cf_0000, 0));
        self.window_classes
            .insert(hwnd, "ConsoleWindowClass".into());
        self.window_titles.insert(hwnd, "Command Prompt".into());
        self.window_visibility.insert(hwnd, true);
        self.window_threads.insert(hwnd, self.current_thread);
        self.ce.windows.raise(hwnd, false, &self.window_styles);
        self.active_window = hwnd;
        self.controls.focus = hwnd;
        let dc = self.gdi.create_memory_dc();
        let bitmap = self.gdi.create_compatible_bitmap(1, 1);
        self.gdi.dc_mut(dc).unwrap().selected_bitmap = Some(bitmap);
        self.window_dcs.insert(hwnd, dc);
        self.console.view = Some(View {
            hwnd,
            thread: self.current_thread,
            revision: None,
            size: (0, 0),
            active: true,
        });
        self.refresh_console_window();
    }

    /// Paint only a changed, visible backing store. The existing compositor
    /// remains the sole publisher of console pixels and classic decorations.
    pub fn refresh_console_window(&mut self) {
        let Some(mut view) = self.console.view.take() else {
            return;
        };
        if !self.window_rects.contains_key(&view.hwnd) {
            return;
        }
        let size = self.window_client_size(view.hwnd).unwrap_or_default();
        if !self.window_tree_visible(view.hwnd, |_| false) || size.0 <= 0 || size.1 <= 0 {
            self.console.view = Some(view);
            return;
        }
        let size = (size.0.clamp(1, 2048) as u32, size.1.clamp(1, 2048) as u32);
        let device = self.vfs.ensure_console_device();
        let mut f = font();
        crate::text::bind_resource(&self.gdi.installed_fonts, &mut f);
        let cell = crate::text::layout(Some(&f), &[77]);
        let (cw, ch) = (cell.width.max(1), (cell.height + 2).max(1));
        let (revision, title, snapshot) = {
            let mut device = device.lock().unwrap_or_else(|e| e.into_inner());
            device.resize(
                ((size.0 as i32 - 4) / cw).max(1) as usize,
                ((size.1 as i32 - 4) / ch).max(1) as usize,
            );
            if view.revision == Some(device.revision)
                && view.size == size
                && view.active == (self.active_window == view.hwnd)
            {
                self.console.view = Some(view);
                return;
            }
            (device.revision, device.title.clone(), device.snapshot())
        };
        self.window_titles.insert(view.hwnd, title);
        let bitmap = self
            .window_dcs
            .get(&view.hwnd)
            .and_then(|d| self.gdi.dc(*d))
            .and_then(|d| d.selected_bitmap);
        if let Some(bitmap) = bitmap.and_then(|b| self.gdi.bitmap_mut(b)) {
            if (bitmap.width, bitmap.height) != size {
                *bitmap = Bitmap::new(size.0, size.1);
            }
            bitmap.pixels.fill(0);
            if let Some(painted) = &mut bitmap.painted {
                painted.fill(1);
            }
            let mut surface = Surface::Bitmap(bitmap);
            let clip = [2, 2, size.0 as i32 - 2, size.1 as i32 - 2];
            for (y, row) in snapshot.lines.iter().enumerate() {
                let units: Vec<_> = row.iter().collect::<String>().encode_utf16().collect();
                let mut dx = Vec::with_capacity(units.len());
                for c in row {
                    dx.push(cw);
                    if c.len_utf16() == 2 {
                        dx.push(0);
                    }
                }
                crate::text::layout_dx(Some(&f), &units, Some(&dx)).draw(
                    &mut surface,
                    2,
                    2 + y as i32 * ch,
                    0xffff,
                    clip,
                );
            }
            if self.active_window == view.hwnd {
                if let Some((row, col)) = snapshot.caret {
                    surface.fill_rect(2 + col as i32 * cw, row as i32 * ch + ch - 1, cw, 2, 0xffff);
                }
            }
            bitmap.mark_painted();
        }
        view.revision = Some(revision);
        view.size = size;
        view.active = self.active_window == view.hwnd;
        self.console.view = Some(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edited_unicode_lines_wait_for_enter_and_survive_small_reads() {
        let mut c = Device::default();
        assert_eq!(c.read_utf16(8, true), None);
        for ch in "abЖ😀".chars() {
            c.character(ch);
        }
        c.key(0x25, false);
        c.key(8, false);
        c.character('C');
        c.key(13, false);
        let first = c.read_utf16(3, true).unwrap();
        let rest = c.read_utf16(64, true).unwrap();
        assert_eq!(
            String::from_utf16(&[first, rest].concat()).unwrap(),
            "abC😀\n"
        );
        assert_eq!(c.read_utf16(8, true), None);
        c.key(0x5a, true);
        assert_eq!(c.read_utf16(8, true), Some(Vec::new()));
        assert!(c.at_eof());
    }
    #[test]
    fn processed_output_overwrites_wraps_and_keeps_scrollback_bounded() {
        let mut c = Device::default();
        c.resize(8, 3);
        c.write_text("abcd\rX\tY\n123456789\x08Z");
        assert_eq!(
            c.snapshot()
                .lines
                .iter()
                .map(|r| r.iter().collect::<String>())
                .collect::<Vec<_>>(),
            ["Y", "12345678", "Z"]
        );
        for _ in 0..1000 {
            c.write_text("row\n");
        }
        assert_eq!(c.lines.len(), SCROLLBACK);
        c.clear_screen();
        assert_eq!(c.snapshot().lines, vec![Vec::<char>::new()]);
    }
    #[test]
    fn history_raw_input_and_interrupt_are_device_state() {
        let mut c = Device::default();
        for ch in "echo one".chars() {
            c.character(ch);
        }
        c.key(13, false);
        c.read_utf16(100, true);
        c.key(0x26, false);
        c.key(13, false);
        assert_eq!(
            String::from_utf16(&c.read_utf16(100, true).unwrap()).unwrap(),
            "echo one\n"
        );
        c.flush_input();
        c.set_mode(PROCESSED_OUTPUT);
        c.character('Ж');
        assert_eq!(c.read_utf16(8, false).unwrap(), vec!['Ж' as u16]);
        c.interrupt();
        assert_eq!(c.interrupt_revision, 1);
        assert_eq!(c.read_utf16(8, false).unwrap(), [10]);
    }

    #[test]
    fn wheel_and_page_navigation_retain_history_and_typing_returns_to_caret() {
        let mut c = Device::default();
        c.resize(16, 3);
        for row in 0..10 {
            c.write_text(&format!("row{row}\r\n"));
        }
        let bottom = c.snapshot().lines;
        c.wheel(60);
        assert_eq!(c.snapshot().lines, bottom);
        c.wheel(60);
        let older = c.snapshot().lines;
        assert_eq!(older[0].iter().collect::<String>(), "row5");
        assert!(c.snapshot().caret.is_none());
        // Background output preserves the visible history instead of jumping.
        c.write_text("new\r\n");
        assert_eq!(c.snapshot().lines, older);
        c.key(0x21, false);
        assert_eq!(c.snapshot().lines[0].iter().collect::<String>(), "row3");
        c.key(0x23, true);
        assert!(c.snapshot().caret.is_some());
        c.wheel(120);
        c.character('X');
        assert!(c.snapshot().caret.is_some());
        assert_eq!(c.snapshot().lines.last().unwrap(), &vec!['X']);
    }
}
