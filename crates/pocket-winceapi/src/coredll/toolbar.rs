//! Stateful 32-bit WinCE toolbar messages for native subclasses.
use super::*;
pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    if !ctx
        .kernel
        .controls
        .get(hwnd)
        .is_some_and(|c| c.class == ControlClass::Toolbar)
        || !(0x400..=0x460).contains(&msg)
    {
        return Ok(None);
    }
    let records = if matches!(msg, 0x414 | 0x415) {
        let n = if msg == 0x415 { 1 } else { wp };
        if n > 4096 {
            return Ok(Some(0));
        }
        let Ok(b) = ctx.cpu.read_mem(lp, n * 20) else {
            return Ok(Some(0));
        };
        b.chunks_exact(20)
            .map(|b| {
                std::array::from_fn(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()))
            })
            .collect::<Vec<[u32; 5]>>()
    } else {
        Vec::new()
    };
    let bitmap = if msg == 0x413 {
        if wp == 0 || wp > 4096 {
            return Ok(Some(u32::MAX));
        }
        let Ok(b) = ctx.cpu.read_mem(lp, 8) else {
            return Ok(Some(u32::MAX));
        };
        let m = u32::from_le_bytes(b[..4].try_into().unwrap());
        let h = u32::from_le_bytes(b[4..].try_into().unwrap());
        Some(if m == 0 {
            h
        } else {
            load_bitmap_resource(ctx, m, &ResourceKey::Id(h), true)?
        })
    } else {
        None
    };
    let bar = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().toolbar;
    let id = bar.buttons.iter().position(|b| b[1] == wp);
    let out = match msg {
        0x41e => u32::from(wp == 20),
        0x414 => {
            if bar.buttons.len() + records.len() > 4096 {
                0
            } else {
                bar.buttons.extend(records);
                1
            }
        }
        0x415 => {
            if wp as usize > bar.buttons.len() || bar.buttons.len() >= 4096 {
                0
            } else {
                bar.buttons.insert(wp as usize, records[0]);
                1
            }
        }
        0x416 => {
            if (wp as usize) < bar.buttons.len() {
                bar.buttons.remove(wp as usize);
                1
            } else {
                0
            }
        }
        0x417 => {
            if let Some(b) = bar.buttons.get(wp as usize) {
                u32::from(
                    ctx.cpu
                        .write_mem(
                            lp,
                            &b.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>(),
                        )
                        .is_ok(),
                )
            } else {
                0
            }
        }
        0x418 => bar.buttons.len() as u32,
        0x419 => id.map_or(u32::MAX, |i| i as u32),
        0x412 => id.map_or(u32::MAX, |i| bar.buttons[i][2] & 255),
        0x411 => {
            if let Some(i) = id {
                bar.buttons[i][2] = (bar.buttons[i][2] & !255) | (lp & 255);
                1
            } else {
                0
            }
        }
        0x401..=0x405 => {
            let mask = match msg {
                0x401 => 4,
                0x402 => 1,
                0x403 => 2,
                0x404 => 8,
                _ => 16,
            };
            if let Some(i) = id {
                bar.buttons[i][2] =
                    (bar.buttons[i][2] & !mask) | if lp & 65535 != 0 { mask } else { 0 };
                1
            } else {
                0
            }
        }
        0x409..=0x40d => {
            let mask = match msg {
                0x409 => 4,
                0x40a => 1,
                0x40b => 2,
                0x40c => 8,
                _ => 16,
            };
            id.map_or(0, |i| u32::from(bar.buttons[i][2] & mask != 0))
        }
        0x41f | 0x420 => {
            let size = ((lp & 65535) as i32, (lp >> 16) as i32);
            if size.0 == 0 || size.1 == 0 {
                0
            } else {
                if msg == 0x41f {
                    bar.button_size = size;
                } else {
                    bar.bitmap_size = size;
                }
                1
            }
        }
        0x43a => (bar.button_size.0 as u32) | ((bar.button_size.1 as u32) << 16),
        0x413 => {
            let start: u32 = bar.bitmaps.iter().map(|b| b.1).sum();
            if start > 4096 - wp {
                u32::MAX
            } else {
                bar.bitmaps.push((bitmap.unwrap(), wp));
                start
            }
        }
        0x41d => {
            if let Some(b) = bar.buttons.get(wp as usize) {
                let x = bar.buttons[..wp as usize]
                    .iter()
                    .filter(|b| b[2] & 8 == 0)
                    .map(|b| {
                        if b[2] & 0x100 != 0 {
                            (b[0] as i32).max(1)
                        } else {
                            bar.button_size.0
                        }
                    })
                    .sum::<i32>();
                let w = if b[2] & 0x100 != 0 {
                    (b[0] as i32).max(1)
                } else {
                    bar.button_size.0
                };
                let bytes: Vec<_> = [x, 0, x + w, bar.button_size.1]
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect();
                u32::from(ctx.cpu.write_mem(lp, &bytes).is_ok())
            } else {
                0
            }
        }
        0x41c | 0x421 | 0x42b => 0,
        _ => return Ok(None),
    };
    Ok(Some(out))
}
