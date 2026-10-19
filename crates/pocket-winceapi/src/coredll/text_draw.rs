//! DrawText measurement and rendering share TrueType advances.
use super::*;
use pocket_kernel::text;

fn lines(
    chars: &[u16],
    width: i32,
    format: u32,
    face: Option<&pocket_kernel::gdi::Font>,
) -> Vec<Vec<u16>> {
    if chars.is_empty() {
        return Vec::new();
    }
    let single = format & 0x20 != 0;
    let wrap = !single && format & 0x10 != 0;

    let mut result = Vec::new();
    let mut line = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if !single && (c == 10 || c == 13) {
            if c == 13 && chars.get(i) == Some(&10) {
                i += 1;
            }
            result.push(std::mem::take(&mut line));
            continue;
        }
        if wrap && !line.is_empty() && {
            let mut candidate = line.clone();
            candidate.push(c);
            text::layout(face, &candidate).width > width.max(1)
        } {
            if c == 32 {
                result.push(std::mem::take(&mut line));
                continue;
            }
            if let Some(space) = line.iter().rposition(|&u| u == 32) {
                let tail = line.split_off(space + 1);
                line.pop();
                result.push(std::mem::replace(&mut line, tail));
            } else {
                result.push(std::mem::take(&mut line));
            }
        }
        line.push(if single && (c == 10 || c == 13) {
            32
        } else {
            c
        });
    }
    result.push(line);
    result
}

pub(super) fn draw(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dc = ctx.arg_u32(0)?;
    let text = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)? as i32;
    let rect = ctx.arg_u32(3)?;
    let format = ctx.arg_u32(4)?;
    let Some(meta) = ctx.kernel.gdi.dc(dc).cloned() else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let mut chars = Vec::new();
    if text != 0 {
        if count < 0 {
            for i in 0..1024 {
                let raw = ctx.cpu.read_mem(text + i * 2, 2)?;
                let c = u16::from_le_bytes([raw[0], raw[1]]);
                if c == 0 {
                    break;
                }
                chars.push(c);
            }
        } else {
            let raw = ctx.cpu.read_mem(text, (count as u32).min(1024) * 2)?;
            chars.extend(
                raw.chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]])),
            );
        }
    }
    let r = if rect != 0 {
        let raw = ctx.cpu.read_mem(rect, 16)?;
        std::array::from_fn(|i| i32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap()))
    } else {
        [
            0,
            0,
            ctx.kernel.framebuffer.width as i32,
            ctx.kernel.framebuffer.height as i32,
        ]
    };
    if std::env::var_os("POCKETHLE_TEXT_TRACE").is_some() {
        log::info!(
            "DrawText dc={dc:x} format={format:x} rect={r:?} text={:?}",
            String::from_utf16_lossy(&chars)
        );
    }
    let face = selected_text_font(ctx.kernel, dc);
    let cell_h = text::layout(face.as_ref(), &[]).height;
    let layout = lines(&chars, r[2].saturating_sub(r[0]), format, face.as_ref());
    let height = layout.len() as i32 * cell_h;
    if format & 0x400 != 0 {
        if rect != 0 {
            let width = layout
                .iter()
                .map(|l| text::layout(face.as_ref(), l).width)
                .max()
                .unwrap_or(0);
            let measured = [
                r[0],
                r[1],
                r[0].saturating_add(width),
                r[1].saturating_add(height),
            ];
            ctx.cpu.write_mem(
                rect,
                &measured
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
        }
        return Ok(DispatchOutcome::ReturnedR0(height as u32));
    }
    sync_dib_from_guest(ctx, dc)?;
    if let Some(mut surf) = surface_for_dc(ctx.kernel, dc) {
        let (w, h) = surf.dimensions();
        let clip = if format & 0x100 != 0 {
            [0, 0, w as i32, h as i32]
        } else {
            [
                r[0].max(0),
                r[1].max(0),
                r[2].min(w as i32),
                r[3].min(h as i32),
            ]
        };
        let mut y = r[1];
        if format & 0x20 != 0 {
            if format & 4 != 0 {
                y = y.saturating_add((r[3].saturating_sub(r[1]) - height) / 2);
            }
        }
        if format & 0x20 != 0 && format & 8 != 0 {
            y = r[3].saturating_sub(height);
        }
        for line in layout {
            let rendered = text::layout(face.as_ref(), &line);
            let width = rendered.width;
            let x = if format & 1 != 0 {
                r[0].saturating_add((r[2].saturating_sub(r[0]) - width) / 2)
            } else if format & 2 != 0 {
                r[2].saturating_sub(width)
            } else {
                r[0]
            };
            if !meta.bk_transparent {
                let (left, top, right, bottom) = (
                    x.max(clip[0]),
                    y.max(clip[1]),
                    x.saturating_add(width).min(clip[2]),
                    y.saturating_add(cell_h).min(clip[3]),
                );
                if right > left && bottom > top {
                    surf.fill_rect(
                        left,
                        top,
                        right - left,
                        bottom - top,
                        colorref_to_rgb565(meta.bk_color),
                    );
                }
            }
            rendered.draw(&mut surf, x, y, colorref_to_rgb565(meta.text_color), clip);
            y = y.saturating_add(cell_h);
        }
        surf.mark_dirty();
    }
    sync_dst_dib_to_guest(ctx, dc)?;
    Ok(DispatchOutcome::ReturnedR0(height as u32))
}
