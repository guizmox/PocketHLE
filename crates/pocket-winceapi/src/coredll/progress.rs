//! Stateful WinCE progress-bar messages. Ranges, positions and step are signed.
use super::*;
pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    let Some(c) = ctx
        .kernel
        .controls
        .get_mut(hwnd)
        .filter(|c| c.class == ControlClass::Progress)
    else {
        return Ok(None);
    };
    let old_position = c.track_pos;
    let result = match msg {
        0x401 | 0x406 => {
            let (low, high) = if msg == 0x401 {
                (lp as u16 as i16 as i32, (lp >> 16) as u16 as i16 as i32)
            } else {
                (wp as i32, lp as i32)
            };
            let old = (c.track_range.0 as u16 as u32) | ((c.track_range.1 as u16 as u32) << 16);
            // A reversed range has no sensible progress interval; preserve the old one.
            if low > high {
                return Ok(Some(0));
            }
            c.track_range = (low, high);
            c.track_pos = c.track_pos.clamp(low, high);
            old
        }
        0x402 => {
            c.track_pos = (wp as i32).clamp(c.track_range.0, c.track_range.1);
            old_position as u32
        }
        0x403 => {
            c.track_pos = (i64::from(c.track_pos) + i64::from(wp as i32))
                .clamp(i64::from(c.track_range.0), i64::from(c.track_range.1))
                as i32;
            old_position as u32
        }
        0x404 => {
            let old = c.track_line;
            c.track_line = wp as i32;
            old as u32
        }
        0x405 => {
            let (low, high) = (i64::from(c.track_range.0), i64::from(c.track_range.1));
            let next = i64::from(c.track_pos) + i64::from(c.track_line);
            c.track_pos = if high == low {
                low
            } else if next > high || next < low {
                (next - low).rem_euclid(high - low) + low
            } else {
                next
            } as i32;
            old_position as u32
        }
        0x407 => {
            let (low, high) = c.track_range;
            if lp != 0 {
                let mut bytes = [0; 8];
                bytes[..4].copy_from_slice(&low.to_le_bytes());
                bytes[4..].copy_from_slice(&high.to_le_bytes());
                ctx.cpu.write_mem(lp, &bytes)?;
            }
            return Ok(Some(if wp != 0 { low as u32 } else { high as u32 }));
        }
        0x408 => return Ok(Some(c.track_pos as u32)),
        0x409 => {
            let old = c.progress_colors.0;
            c.progress_colors.0 = lp;
            old
        }
        0x2001 => {
            let old = c.progress_colors.1;
            c.progress_colors.1 = lp;
            old
        }
        _ => return Ok(None),
    };
    ctx.kernel.framebuffer.mark_dirty();
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    fn value(out: DispatchOutcome) -> u32 {
        let DispatchOutcome::ReturnedR0(n) = out else {
            panic!()
        };
        n
    }
    #[test]
    fn progress_range_queries_clamping_steps_and_colors_have_real_state() {
        let (mut cpu, mut k, mut d) = setup();
        let h = k.controls.create(
            0xdead0001,
            ControlClass::Progress,
            1029,
            String::new(),
            0x50000001,
            0,
            0,
            100,
            12,
        );
        assert_eq!(
            ControlClass::from_class_name("MSCTLS_PROGRESS32"),
            Some(ControlClass::Progress)
        );
        assert_eq!(k.controls.by_id(0xdead0001, 1029).unwrap().hwnd, h);
        let send = |cpu: &mut pocket_cpu::stub::StubCpu,
                    k: &mut KernelState,
                    d: &mut WinCeDispatcher,
                    m,
                    w,
                    l| {
            value(call(
                cpu,
                k,
                d,
                "coredll.dll",
                "SendMessageW",
                &[h, m, w, l],
            ))
        };
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x406, 10, 30), 100 << 16);
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x407, 1, 0x2000), 10);
        assert_eq!(cpu.read_mem(0x2000, 8).unwrap(), [10, 0, 0, 0, 30, 0, 0, 0]);
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x402, 100, 0), 10);
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x408, 0, 0), 30);
        assert_eq!(
            send(&mut cpu, &mut k, &mut d, 0x403, (-100i32) as u32, 0),
            30
        );
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x408, 0, 0), 10);
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x404, 7, 0), 10);
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x405, 0, 0), 10);
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x408, 0, 0), 17);
        send(&mut cpu, &mut k, &mut d, 0x402, 28, 0);
        send(&mut cpu, &mut k, &mut d, 0x405, 0, 0);
        assert_eq!(send(&mut cpu, &mut k, &mut d, 0x408, 0, 0), 15);
        assert_eq!(
            send(&mut cpu, &mut k, &mut d, 0x409, 0, 0x00ff00),
            0xff000000
        );
        assert_eq!(k.controls.get(h).unwrap().progress_colors.0, 0x00ff00);
    }
}
