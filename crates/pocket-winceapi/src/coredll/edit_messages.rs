//! EDIT line queries and scrolling use the same geometry as painting/hit tests.
use super::*;
pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    message: u32,
    w: u32,
    l: u32,
) -> Result<Option<u32>, KernelError> {
    if !matches!(
        message,
        0xba | 0xbb | 0xc1 | 0xc4 | 0xc9 | 0xce | 0xd6 | 0xd7 | 0xb5 | 0xb6 | 0xb7 | 0x114 | 0x115
    ) {
        return Ok(None);
    }
    let Some(child) = ctx
        .kernel
        .controls
        .get(hwnd)
        .filter(|c| c.class == ControlClass::Edit)
    else {
        return Ok(None);
    };
    let face = pocket_kernel::text::selected(&ctx.kernel.gdi, child.font);
    let geometry = child.edit_geometry(face.as_ref());
    let multiline = child.style & 4 != 0;
    let caret = child.edit_caret();
    let value = match message {
        0xba => geometry.lines.len() as u32,
        0xc9 => geometry.line_from_char(if w == u32::MAX { caret } else { w as usize }) as u32,
        0xbb => {
            let line = if w == u32::MAX {
                geometry.line_from_char(caret)
            } else {
                w as usize
            };
            geometry
                .lines
                .get(line)
                .map(|line| line.start as u32)
                .unwrap_or(u32::MAX)
        }
        0xc1 => {
            if !multiline {
                if w == u32::MAX || w as usize <= geometry.units.len() {
                    geometry.units.len() as u32
                } else {
                    0
                }
            } else if w == u32::MAX {
                let (a, b) = child.edit.range(&child.text);
                let first = &geometry.lines[geometry.line_from_char(a as usize)];
                let last = &geometry.lines[geometry.line_from_char(b as usize)];
                (a as usize - first.start + last.end.saturating_sub(b as usize)) as u32
            } else {
                let line = &geometry.lines[geometry.line_from_char(w as usize)];
                if w as usize > geometry.units.len() {
                    0
                } else {
                    (line.end - line.start) as u32
                }
            }
        }
        0xc4 => {
            if l == 0 {
                return Ok(Some(0));
            }
            ctx.cpu.check_guest_access(l, 2, Prot::READ)?;
            let capacity = ctx.cpu.read_mem(l, 2)?;
            let capacity = u16::from_le_bytes(capacity.try_into().unwrap()) as usize;
            let Some(line) = geometry.lines.get(if multiline { w as usize } else { 0 }) else {
                return Ok(Some(0));
            };
            let count = capacity.min(line.end - line.start);
            let bytes = geometry.units[line.start..line.start + count]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            if !bytes.is_empty() {
                ctx.cpu
                    .check_guest_access(l, bytes.len() as u32, Prot::WRITE)?;
                ctx.cpu.write_mem(l, &bytes)?;
            }
            count as u32
        }
        0xce => {
            if multiline {
                child.edit.first_line.min(geometry.lines.len() as u32 - 1)
            } else {
                geometry.lines[0]
                    .positions
                    .partition_point(|x| *x <= child.edit.scroll_x)
                    .saturating_sub(1) as u32
            }
        }
        0xd6 => {
            if w as usize >= geometry.units.len() {
                u32::MAX
            } else {
                let (x, row) = geometry.position(w as usize);
                let y = if multiline {
                    (row as i32 - child.edit.first_line as i32) * geometry.height + 1
                } else {
                    ((child.h - geometry.height) / 2).max(1)
                };
                ((x + 3 - child.edit.scroll_x) as u16 as u32) | ((y as u16 as u32) << 16)
            }
        }
        0xd7 => {
            let x = l as u16 as i16 as i32;
            let y = (l >> 16) as u16 as i16 as i32;
            let row = if multiline {
                ((y - 1).max(0) / geometry.height) as usize + child.edit.first_line as usize
            } else {
                0
            };
            let row = row.min(geometry.lines.len() - 1);
            (geometry.hit(row, x - 3 + child.edit.scroll_x) as u32 & 0xffff)
                | ((row as u32 & 0xffff) << 16)
        }
        0xb7 => {
            ctx.kernel
                .controls
                .get_mut(hwnd)
                .unwrap()
                .edit_scroll_caret(face.as_ref());
            repaint_controls(ctx);
            0
        }
        0xb6 => {
            let value = ctx.kernel.controls.get_mut(hwnd).unwrap().edit_scroll_by(
                w as i32,
                l as i32,
                face.as_ref(),
            );
            if value {
                repaint_controls(ctx);
            }
            u32::from(value)
        }
        0xb5 | 0x115 => {
            if !multiline {
                return Ok(Some(0));
            }
            let old = child.edit.first_line;
            let page = (child.h / geometry.height).max(1);
            let delta = match w & 0xffff {
                0 => -1,
                1 => 1,
                2 => -page,
                3 => page,
                4 | 5 => (w >> 16) as i32 - old as i32,
                6 => i32::MIN,
                7 => i32::MAX,
                _ => return Ok(Some(0)),
            };
            let child = ctx.kernel.controls.get_mut(hwnd).unwrap();
            child.edit_scroll_by(0, delta, face.as_ref());
            let moved = child.edit.first_line as i32 - old as i32;
            repaint_controls(ctx);
            (moved as u16 as u32) | 0x10000
        }
        0x114 => {
            if !multiline {
                return Ok(Some(0));
            }
            let page = (child.w / geometry.average_width).max(1);
            let delta = match w & 0xffff {
                0 => -1,
                1 => 1,
                2 => -page,
                3 => page,
                6 => i32::MIN,
                7 => i32::MAX,
                _ => return Ok(Some(0)),
            };
            ctx.kernel
                .controls
                .get_mut(hwnd)
                .unwrap()
                .edit_scroll_by(delta, 0, face.as_ref());
            repaint_controls(ctx);
            0
        }
        _ => unreachable!(),
    };
    Ok(Some(value))
}

pub(super) fn modifiers(state: &KernelState) -> (bool, bool) {
    (
        state.pressed_keys[0x10] || state.pressed_keys[0xa0] || state.pressed_keys[0xa1],
        state.pressed_keys[0x11] || state.pressed_keys[0xa2] || state.pressed_keys[0xa3],
    )
}
