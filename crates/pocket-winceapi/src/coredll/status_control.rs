//! CE status bars have their own client geometry. Explorer's status text must
//! not paint over its separate taskbar at the bottom of the display.
use super::*;

fn ret(value: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value))
}

pub(crate) fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (style, text, parent, id) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    );
    let Some((width, height)) = ctx.kernel.window_client_size(parent) else {
        set_thread_error(ctx, 1400);
        return ret(0);
    };
    let text = if text == 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&read_wstr(ctx, text, 4096)?)
    };
    let bar_height = pocket_kernel::StatusBar::DEFAULT_HEIGHT;
    let hwnd = ctx.kernel.controls.create(
        parent,
        ControlClass::StatusBar,
        id,
        String::new(),
        style | 0x40000000,
        0,
        (height - bar_height).max(0),
        width,
        bar_height,
    );
    ctx.kernel
        .controls
        .get_mut(hwnd)
        .unwrap()
        .status_bar
        .set_part_text(0, text);
    ctx.kernel
        .window_threads
        .insert(hwnd, ctx.kernel.current_thread);
    queue_window_paint(ctx.kernel, hwnd);
    ctx.kernel.composite_controls();
    ret(hwnd)
}

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
        .is_some_and(|c| c.class == ControlClass::StatusBar)
    {
        return Ok(None);
    }
    let mut changed = false;
    let value = match msg {
        0x404 => {
            if wp > 256
                || (wp != 0
                    && (lp == 0 || ctx.cpu.check_guest_access(lp, wp * 4, Prot::READ).is_err()))
            {
                set_thread_error(ctx, 87);
                return Ok(Some(0));
            }
            let bytes = if wp == 0 {
                Vec::new()
            } else {
                ctx.cpu.read_mem(lp, wp * 4)?
            };
            let edges = bytes
                .chunks_exact(4)
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            ctx.kernel
                .controls
                .get_mut(hwnd)
                .unwrap()
                .status_bar
                .set_parts(edges);
            changed = true;
            1
        }
        0x401 | 0x40b => {
            // SBT_OWNERDRAW carries application data, not a string pointer.
            if wp & 0x1000 != 0 {
                set_thread_error(ctx, 50);
                return Ok(Some(0));
            }
            let text = if lp == 0 {
                String::new()
            } else if msg == 0x40b {
                String::from_utf16_lossy(&read_wstr(ctx, lp, 4096)?)
            } else {
                read_cstr_string(ctx, lp, 4096)?
            };
            ctx.kernel
                .controls
                .get_mut(hwnd)
                .unwrap()
                .status_bar
                .set_part_text((wp & 255) as usize, text);
            changed = true;
            1
        }
        0x402 | 0x40d | 0x403 | 0x40c => {
            let text = ctx
                .kernel
                .controls
                .get(hwnd)
                .unwrap()
                .status_bar
                .part_text
                .get((wp & 255) as usize)
                .cloned()
                .unwrap_or_default();
            let wide = matches!(msg, 0x40c | 0x40d);
            let bytes: Vec<_> = if wide {
                text.encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect()
            } else {
                text.bytes().chain(Some(0)).collect()
            };
            if matches!(msg, 0x402 | 0x40d) && lp != 0 {
                if ctx
                    .cpu
                    .check_guest_access(lp, bytes.len() as u32, Prot::WRITE)
                    .is_err()
                {
                    set_thread_error(ctx, 87);
                    return Ok(Some(0));
                }
                ctx.cpu.write_mem(lp, &bytes)?;
            }
            (if wide {
                text.encode_utf16().count()
            } else {
                text.len()
            }) as u32
                & 65535
        }
        0x406 => {
            let edges = &ctx.kernel.controls.get(hwnd).unwrap().status_bar.part_edges;
            let have = edges.len() as u32;
            let bytes: Vec<_> = edges
                .iter()
                .take(wp as usize)
                .flat_map(|e| e.to_le_bytes())
                .collect();
            if lp != 0 && !bytes.is_empty() {
                if ctx
                    .cpu
                    .check_guest_access(lp, bytes.len() as u32, Prot::WRITE)
                    .is_err()
                {
                    set_thread_error(ctx, 87);
                    return Ok(Some(0));
                }
                ctx.cpu.write_mem(lp, &bytes)?;
            }
            have
        }
        0x407 => {
            if lp == 0 || ctx.cpu.check_guest_access(lp, 12, Prot::WRITE).is_err() {
                return Ok(Some(0));
            }
            ctx.cpu.write_mem(
                lp,
                &[2i32, 2, 2]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            1
        }
        0x408 => {
            if wp > 2048 {
                return Ok(Some(0));
            }
            ctx.kernel.controls.get_mut(hwnd).unwrap().status_bar.height = (wp as i32 + 4).max(19);
            0
        }
        0x409 => {
            ctx.kernel.controls.get_mut(hwnd).unwrap().status_bar.simple = wp != 0;
            changed = true;
            0
        }
        0x40e => u32::from(ctx.kernel.controls.get(hwnd).unwrap().status_bar.simple),
        0x40a => {
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            if wp as usize >= c.status_bar.part_edges.len()
                || lp == 0
                || ctx.cpu.check_guest_access(lp, 16, Prot::WRITE).is_err()
            {
                return Ok(Some(0));
            }
            let left = if wp == 0 {
                0
            } else {
                c.status_bar.part_edges[wp as usize - 1].max(0)
            };
            let edge = c.status_bar.part_edges[wp as usize];
            let right = if edge < 0 { c.w } else { edge.min(c.w) };
            ctx.cpu.write_mem(
                lp,
                &[left, 0, right, c.h]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            1
        }
        0x2001 => {
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            let previous = c.background_color.replace(lp).unwrap_or(0x00c0c0c0);
            changed = true;
            previous
        }
        5 => {
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            if c.style & 8 == 0 {
                if let Some((width, height)) = ctx.kernel.window_client_size(c.parent) {
                    let bar_height = c.status_bar.height();
                    let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
                    c.x = 0;
                    c.y = (height - bar_height).max(0);
                    c.w = width;
                    c.h = bar_height;
                    changed = true;
                }
            }
            0
        }
        _ => return Ok(None),
    };
    if changed {
        queue_window_paint(ctx.kernel, hwnd);
        ctx.kernel.composite_controls();
    }
    Ok(Some(value))
}
