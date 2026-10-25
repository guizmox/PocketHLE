//! PocketPC keyboard input method. One layout drives drawing and hit testing.
use crate::{
    gdi::{Font, Surface},
    shell_keyboard::ShellKeyboard,
    text,
};
pub const VIRTUAL_KEY: u32 = 0x10000;
pub const CONTROL_KEY: u32 = 0x20000;
#[derive(Clone, Copy)]
enum Key {
    Char(char),
    Vk(u16),
    Caps,
    Shift,
    Ctrl,
    Symbols,
    Accents,
}
struct Cell {
    label: String,
    key: Key,
    rect: [i32; 4],
    index: usize,
}
pub fn height(w: i32, h: i32) -> i32 {
    (80 * (w.min(h) / 240).max(1)).min((h - 26).max(5))
}
pub fn top(w: i32, h: i32) -> i32 {
    h - 26 - height(w, h)
}
fn cells(k: &ShellKeyboard, w: i32, h: i32) -> Vec<Cell> {
    use Key::*;
    let mut rows: Vec<Vec<(String, Key, i32)>> = vec![vec![]; 5];
    rows[0].push(("123".into(), Symbols, 1));
    for ch in (if k.shift {
        "!@#$%^&*()_+"
    } else {
        "1234567890-="
    })
    .chars()
    {
        rows[0].push((ch.to_string(), Char(ch), 1));
    }
    rows[0].push(("⌫".into(), Vk(8), 1));
    rows[1].push(("Tab".into(), Vk(9), 2));
    rows[2].push(("CAP".into(), Caps, 2));
    rows[3].push(("Shift".into(), Shift, 2));
    let letters = if k.accents {
        ["áéíóúàèìòù[]", "äëïöüâêîô;'\\", "çñßæœøå,./"]
    } else if k.symbols {
        ["!@#$%^&*()[]", "`~|{}<>?_;'\\", "+-=€£¥©,./"]
    } else {
        ["qwertyuiop[]", "asdfghjkl;'\\", "zxcvbnm,./"]
    };
    for (r, chars) in letters.into_iter().enumerate() {
        for mut ch in chars.chars() {
            if ch.is_alphabetic() && !(k.lowercase ^ k.shift) {
                ch = ch.to_uppercase().next().unwrap_or(ch);
            } else if k.shift {
                ch = match ch {
                    '1' => '!',
                    '2' => '@',
                    '3' => '#',
                    '4' => '$',
                    '5' => '%',
                    '6' => '^',
                    '7' => '&',
                    '8' => '*',
                    '9' => '(',
                    '0' => ')',
                    '-' => '_',
                    '=' => '+',
                    '[' => '{',
                    ']' => '}',
                    ';' => ':',
                    '\'' => '"',
                    '\\' => '|',
                    ',' => '<',
                    '.' => '>',
                    '/' => '?',
                    v => v,
                };
            }
            rows[r + 1].push((ch.to_string(), Char(ch), 1));
        }
    }
    rows[3].push(("↵".into(), Vk(13), 2));
    rows[4] = vec![
        ("Ctrl".into(), Ctrl, 2),
        ("áü".into(), Accents, 2),
        ("".into(), Char(' '), 6),
        ("←".into(), Vk(0x25), 1),
        ("↓".into(), Vk(0x28), 1),
        ("↑".into(), Vk(0x26), 1),
        ("→".into(), Vk(0x27), 1),
    ];
    let mut out = vec![];
    for (row, keys) in rows.into_iter().enumerate() {
        let total: i32 = keys.iter().map(|k| k.2).sum();
        let mut used = 0;
        let y = top(w, h) + height(w, h) * row as i32 / 5;
        let bottom = top(w, h) + height(w, h) * (row as i32 + 1) / 5;
        for (label, key, units) in keys {
            let left = w * used / total;
            used += units;
            let right = w * used / total;
            let index = out.len();
            out.push(Cell {
                label,
                key,
                rect: [left, y, right, bottom],
                index,
            });
        }
    }
    out
}
pub fn key_center(label: &str, w: i32, h: i32) -> Option<(i32, i32)> {
    cells(&ShellKeyboard::default(), w, h)
        .into_iter()
        .find(|c| c.label.eq_ignore_ascii_case(label))
        .map(|c| ((c.rect[0] + c.rect[2]) / 2, (c.rect[1] + c.rect[3]) / 2))
}
pub fn tap(k: &mut ShellKeyboard, x: i32, y: i32, w: i32, h: i32) -> Option<u32> {
    k.pressed_key = None;
    if x < 0 || x >= w || y < top(w, h) || y >= h {
        return None;
    }
    if y >= h - 26 {
        return (x >= w - 26).then_some(0);
    }
    let c = cells(k, w, h)
        .into_iter()
        .find(|c| x >= c.rect[0] && x < c.rect[2] && y >= c.rect[1] && y < c.rect[3])?;
    k.pressed_key = Some(c.index);
    use Key::*;
    Some(match c.key {
        Caps => {
            k.lowercase = !k.lowercase;
            u32::MAX
        }
        Shift => {
            k.shift = !k.shift;
            u32::MAX
        }
        Ctrl => {
            k.control = !k.control;
            u32::MAX
        }
        Symbols => {
            k.symbols = !k.symbols;
            k.accents = false;
            u32::MAX
        }
        Accents => {
            k.accents = !k.accents;
            k.symbols = false;
            u32::MAX
        }
        Vk(v) => VIRTUAL_KEY | u32::from(v),
        Char(ch) => {
            k.shift = false;
            if k.control && ch.is_ascii_alphabetic() {
                k.control = false;
                CONTROL_KEY | u32::from(ch.to_ascii_uppercase())
            } else {
                ch as u32
            }
        }
    })
}
pub fn render(k: &ShellKeyboard, s: &mut Surface<'_>) {
    let (w, h) = s.dimensions();
    let (w, h) = (w as i32, h as i32);
    let scale = (w.min(h) / 240).max(1);
    let font = Font::new(-11 * scale);
    s.fill_rect(0, top(w, h), w, height(w, h), 0xbdf7);
    for c in cells(k, w, h) {
        let [x, y, right, bottom] = c.rect;
        let pressed = k.pointer_captured && k.pressed_key == Some(c.index);
        s.fill_rect(x, y, right - x, bottom - y, 0x8410);
        s.fill_rect(
            x + 1,
            y + 1,
            right - x - 2,
            bottom - y - 2,
            if pressed { 0x001f } else { 0xffff },
        );
        if let Key::Vk(vk @ (8 | 13 | 0x25..=0x28)) = c.key {
            let (cx, cy) = ((x + right) / 2, (y + bottom) / 2);
            let color = if pressed { 0xffff } else { 0 };
            let (dx, dy) = match vk {
                0x27 => (1, 0),
                0x26 => (0, -1),
                0x28 => (0, 1),
                _ => (-1, 0),
            };
            let head = (cx + dx * 4 * scale, cy + dy * 4 * scale);
            if vk == 13 {
                s.line(
                    (cx + 4 * scale, cy - 3 * scale),
                    (cx + 4 * scale, cy),
                    color,
                    scale as u32,
                );
                s.line((cx + 4 * scale, cy), head, color, scale as u32);
            } else {
                s.line(
                    (cx - dx * 4 * scale, cy - dy * 4 * scale),
                    head,
                    color,
                    scale as u32,
                );
            }
            for sign in [-1, 1] {
                s.line(
                    head,
                    (
                        head.0 - dx * 3 * scale - dy * sign * 3 * scale,
                        head.1 - dy * 3 * scale + dx * sign * 3 * scale,
                    ),
                    color,
                    scale as u32,
                );
            }
            continue;
        }
        let label: Vec<_> = c.label.encode_utf16().collect();
        let layout = text::layout(Some(&font), &label);
        layout.draw(
            s,
            x + (right - x - layout.width) / 2,
            y + (bottom - y - layout.height) / 2,
            if pressed { 0xffff } else { 0 },
            c.rect,
        );
    }
    // The input-panel button remains in the shell's bottom command strip.
    s.fill_rect(w - 26, h - 26, 26, 26, 0xc618);
    for r in 0..3 {
        for c in 0..5 {
            s.fill_rect(w - 23 + c * 4, h - 19 + r * 4, 3, 3, 0x4208);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classic_layout_types_lowercase_caps_shift_and_virtual_keys() {
        let mut k = ShellKeyboard {
            sip: true,
            ..Default::default()
        };
        for (label, expected) in [
            ("a", 'a' as u32),
            ("CAP", u32::MAX),
            ("a", 'A' as u32),
            ("Shift", u32::MAX),
            ("a", 'a' as u32),
            ("Tab", VIRTUAL_KEY | 9),
            ("⌫", VIRTUAL_KEY | 8),
            ("↵", VIRTUAL_KEY | 13),
        ] {
            let (x, y) = key_center(label, 240, 320).unwrap();
            assert_eq!(tap(&mut k, x, y, 240, 320), Some(expected));
        }
    }
    #[test]
    fn input_panel_is_above_the_command_bar_and_shares_hit_geometry() {
        for (w, h) in [(240, 320), (320, 240), (480, 640)] {
            assert_eq!(top(w, h) + height(w, h), h - 26);
            let (x, y) = key_center("q", w, h).unwrap();
            let mut k = ShellKeyboard {
                sip: true,
                ..Default::default()
            };
            assert_eq!(tap(&mut k, x, y, w, h), Some('q' as u32));
            assert_eq!(tap(&mut k, w - 8, h - 8, w, h), Some(0));
            assert_eq!(tap(&mut k, 5, top(w, h) - 1, w, h), None);
        }
    }
}
