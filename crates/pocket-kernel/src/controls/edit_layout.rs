//! One ephemeral UTF-16 geometry for EDIT drawing, hit testing and line messages.
//! Text and scrolling remain HWND-owned; no host layout cache is checkpointed.
use crate::{gdi::Font, text};

pub struct Line {
    pub start: usize,
    pub end: usize,
    /// Pixel positions at UTF-16 boundaries, including the final boundary.
    pub positions: Vec<i32>,
}
pub struct Layout {
    pub units: Vec<u16>,
    pub lines: Vec<Line>,
    pub height: i32,
    pub average_width: i32,
}
impl Layout {
    pub fn new(
        value: &str,
        font: Option<&Font>,
        width: i32,
        multiline: bool,
        wrap: bool,
        tabs: &[i32],
    ) -> Self {
        let units: Vec<_> = value.encode_utf16().collect();
        let height = text::layout(font, &[]).height.max(1);
        let alphabet: Vec<_> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
            .encode_utf16()
            .collect();
        let average_width = ((text::layout(font, &alphabet).width + 26) / 52).max(1);
        // Bound temporary glyph allocation, including a single very long line.
        let mut advances = Vec::with_capacity(units.len());
        let mut offset = 0;
        while offset < units.len() {
            let mut end = (offset + 512).min(units.len());
            if end < units.len() && (0xdc00..=0xdfff).contains(&units[end]) {
                end -= 1;
            }
            let measured = text::layout(font, &units[offset..end]);
            let mut previous = 0;
            for current in measured.advances {
                advances.push(current - previous);
                previous = current;
            }
            offset = end;
        }
        let mut lines = Vec::new();
        let mut start = 0;
        loop {
            let end = if multiline {
                (start..units.len())
                    .find(|&i| matches!(units[i], 10 | 13))
                    .unwrap_or(units.len())
            } else {
                units.len()
            };
            let mut row = start;
            loop {
                let mut positions = vec![0];
                let mut x = 0i32;
                let mut at = row;
                let mut last_space = None;
                while at < end {
                    let step = if (0xd800..=0xdbff).contains(&units[at])
                        && at + 1 < end
                        && (0xdc00..=0xdfff).contains(&units[at + 1])
                    {
                        2
                    } else {
                        1
                    };
                    let next = if units[at] == 9 {
                        next_tab(x, average_width, tabs)
                    } else {
                        x.saturating_add(advances[at..at + step].iter().sum::<i32>())
                    };
                    if wrap && at > row && next > width.max(1) {
                        break;
                    }
                    if matches!(units[at], 9 | 32) {
                        last_space = Some(at + step);
                    }
                    for _ in 0..step {
                        positions.push(next);
                    }
                    x = next;
                    at += step;
                }
                // An oversized first glyph still occupies one row.
                if at == row && at < end {
                    let step = if (0xd800..=0xdbff).contains(&units[at])
                        && at + 1 < end
                        && (0xdc00..=0xdfff).contains(&units[at + 1])
                    {
                        2
                    } else {
                        1
                    };
                    at += step;
                    for _ in 0..step {
                        positions.push(advances[row..at].iter().sum());
                    }
                }
                if at < end {
                    if let Some(boundary) = last_space.filter(|boundary| *boundary > row) {
                        at = boundary;
                        positions.truncate(at - row + 1);
                    }
                }
                lines.push(Line {
                    start: row,
                    end: at,
                    positions,
                });
                if at == end {
                    break;
                }
                row = at;
            }
            if end == units.len() {
                break;
            }
            start = end
                + if units[end] == 13 && units.get(end + 1) == Some(&10) {
                    2
                } else {
                    1
                };
        }
        Self {
            units,
            lines,
            height,
            average_width,
        }
    }
    pub fn line_from_char(&self, character: usize) -> usize {
        let character = character.min(self.units.len());
        self.lines
            .partition_point(|line| line.start <= character)
            .saturating_sub(1)
    }
    pub fn position(&self, character: usize) -> (i32, usize) {
        let row = self.line_from_char(character);
        let line = &self.lines[row];
        let at = character.min(line.end).saturating_sub(line.start);
        (line.positions[at], row)
    }
    pub fn hit(&self, row: usize, x: i32) -> usize {
        let line = &self.lines[row.min(self.lines.len() - 1)];
        let mut nearest = line.start;
        let mut distance = i64::MAX;
        for at in line.start..=line.end {
            if at > line.start && at < line.end && (0xdc00..=0xdfff).contains(&self.units[at]) {
                continue;
            }
            let d = (i64::from(line.positions[at - line.start]) - i64::from(x)).abs();
            if d < distance {
                nearest = at;
                distance = d;
            }
        }
        nearest
    }
}
fn next_tab(x: i32, average: i32, stops: &[i32]) -> i32 {
    let pixels = |units: i32| {
        ((i64::from(units) * i64::from(average) + 2) / 4).clamp(1, i64::from(i32::MAX)) as i32
    };
    if stops.len() > 1 {
        if let Some(stop) = stops
            .iter()
            .map(|&stop| pixels(stop))
            .find(|stop| *stop > x)
        {
            return stop;
        }
    }
    let period = pixels(if stops.len() == 1 { stops[0] } else { 32 });
    x.saturating_div(period)
        .saturating_add(1)
        .saturating_mul(period)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crlf_empty_final_lines_wrapping_and_utf16_indices_share_one_geometry() {
        let layout = Layout::new("ab\r\n😀z\n", None, 100, true, false, &[]);
        assert_eq!(
            layout
                .lines
                .iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>(),
            [(0, 2), (4, 7), (8, 8)]
        );
        assert_eq!(layout.position(6), (6, 1));
        assert_eq!(layout.hit(1, 6), 6);
        let wrapped = Layout::new("abc def", None, 24, true, true, &[]);
        assert_eq!(
            wrapped
                .lines
                .iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>(),
            [(0, 4), (4, 7)]
        );
        assert_eq!(wrapped.line_from_char(4), 1);
    }
    #[test]
    fn tabs_use_dialog_units_and_no_glyph_or_surrogate_is_split_by_wrap() {
        let a = Layout::new("a\tb", None, 100, true, false, &[16]);
        assert_eq!(a.position(2).0, 24);
        let b = Layout::new("a\tb", None, 100, true, false, &[]);
        assert_eq!(b.position(2).0, 48);
        let c = Layout::new("😀😀", None, 1, true, true, &[]);
        assert_eq!(
            c.lines
                .iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>(),
            [(0, 2), (2, 4)]
        );
    }
}
