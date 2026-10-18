//! WinCE string-backed list boxes and combo boxes. These are real child
//! controls, so MFC DDX can bind them before initializing the rest of a dialog.
use super::*;
pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    let Some(c) = ctx.kernel.controls.get(hwnd) else {
        return Ok(None);
    };
    let combo = c.class == ControlClass::ComboBox;
    if !combo && c.class != ControlClass::ListBox {
        return Ok(None);
    }
    let op = if combo {
        match msg {
            0x143 => 1,
            0x14a => 2,
            0x144 => 3,
            0x14b => 4,
            0x146 => 5,
            0x147 => 6,
            0x14e => 7,
            0x148 => 8,
            0x149 => 9,
            0x150 => 10,
            0x151 => 11,
            0x14c => 12,
            0x158 => 13,
            0x14d => 14,
            0x14f => 15,
            0x157 => 16,
            0x154 => 17,
            0x153 => 18,
            0x15b => 19,
            0x15c => 20,
            _ => return Ok(None),
        }
    } else {
        match msg {
            0x180 => 1,
            0x181 => 2,
            0x182 => 3,
            0x184 => 4,
            0x18b => 5,
            0x188 => 6,
            0x186 => 7,
            0x189 => 8,
            0x18a => 9,
            0x199 => 10,
            0x19a => 11,
            0x18f => 12,
            0x1a2 => 13,
            0x18c => 14,
            0x1a1 => 17,
            0x1a0 => 18,
            0x18e => 19,
            0x197 => 20,
            _ => return Ok(None),
        }
    };
    let mut redraw = false;
    let result = match op {
        1 | 2 => {
            let text = if lp == 0 {
                String::new()
            } else {
                String::from_utf16_lossy(&read_wstr(ctx, lp, 32768)?)
            };
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            let sorted = c.style & if combo { 0x100 } else { 2 } != 0;
            let index = if sorted {
                c.list_items
                    .iter()
                    .position(|(s, _)| s.to_lowercase() > text.to_lowercase())
                    .unwrap_or(c.list_items.len())
            } else if op == 1 || wp == u32::MAX {
                c.list_items.len()
            } else {
                (wp as usize).min(c.list_items.len())
            };
            if c.list_items.len() >= 65536 {
                return Ok(Some(u32::MAX - 1));
            }
            c.list_items.insert(index, (text, 0));
            if let Some(i) = c.list_selected {
                if i >= index {
                    c.list_selected = Some(i + 1);
                }
            }
            redraw = true;
            index as u32
        }
        3 => {
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            if wp as usize >= c.list_items.len() {
                u32::MAX
            } else {
                c.list_items.remove(wp as usize);
                c.list_selected = c.list_selected.and_then(|i| {
                    if i == wp as usize {
                        None
                    } else {
                        Some(i - usize::from(i > wp as usize))
                    }
                });
                redraw = true;
                c.list_items.len() as u32
            }
        }
        4 => {
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            c.list_items.clear();
            c.list_selected = None;
            c.list_scroll_y = 0;
            redraw = true;
            0
        }
        5 => ctx.kernel.controls.get(hwnd).unwrap().list_items.len() as u32,
        6 => ctx
            .kernel
            .controls
            .get(hwnd)
            .unwrap()
            .list_selected
            .map_or(u32::MAX, |i| i as u32),
        7 => {
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            c.list_selected = if (wp as usize) < c.list_items.len() {
                Some(wp as usize)
            } else {
                None
            };
            redraw = true;
            c.list_selected.map_or(u32::MAX, |i| i as u32)
        }
        8 | 9 => {
            let Some((text, _)) = ctx
                .kernel
                .controls
                .get(hwnd)
                .unwrap()
                .list_items
                .get(wp as usize)
            else {
                return Ok(Some(u32::MAX));
            };
            let units: Vec<_> = text.encode_utf16().collect();
            if op == 8 && lp != 0 {
                let mut out = wide_to_bytes(&units);
                out.extend_from_slice(&[0, 0]);
                ctx.cpu.write_mem(lp, &out)?;
            }
            units.len() as u32
        }
        10 => ctx
            .kernel
            .controls
            .get(hwnd)
            .unwrap()
            .list_items
            .get(wp as usize)
            .map_or(u32::MAX, |i| i.1),
        11 => {
            if let Some(item) = ctx
                .kernel
                .controls
                .get_mut(hwnd)
                .unwrap()
                .list_items
                .get_mut(wp as usize)
            {
                item.1 = lp;
                0
            } else {
                u32::MAX
            }
        }
        12 | 13 | 14 => {
            let query = if lp == 0 {
                String::new()
            } else {
                String::from_utf16_lossy(&read_wstr(ctx, lp, 32768)?).to_lowercase()
            };
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            let len = c.list_items.len();
            let start = if wp == u32::MAX {
                0
            } else {
                (wp as usize).saturating_add(1)
            };
            let found = (0..len).map(|i| (start + i) % len).find(|&i| {
                let value = c.list_items[i].0.to_lowercase();
                if op == 13 {
                    value == query
                } else {
                    value.starts_with(&query)
                }
            });
            if op == 14 {
                if let Some(i) = found {
                    c.list_selected = Some(i);
                    redraw = true;
                }
            }
            found.map_or(u32::MAX, |i| i as u32)
        }
        15 => {
            ctx.kernel.controls.get_mut(hwnd).unwrap().choice_open = wp != 0;
            redraw = true;
            1
        }
        16 => u32::from(ctx.kernel.controls.get(hwnd).unwrap().choice_open),
        17 => 16,
        18 => 0,
        19 => ctx.kernel.controls.get(hwnd).unwrap().list_scroll_y.max(0) as u32,
        20 => {
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            c.list_scroll_y = wp.min(c.list_items.len().saturating_sub(1) as u32) as i32;
            redraw = true;
            0
        }
        _ => unreachable!(),
    };
    if redraw {
        repaint_controls(ctx);
    }
    Ok(Some(result))
}
