//! One virtual PCM-output mixer. Its wave source controls the existing device
//! volume; it neither changes host-system volume nor aliases a wave/file handle.
use super::*;

const INVALID_HANDLE: u32 = 5;
const INVALID_PARAM: u32 = 11;
const INVALID_LINE: u32 = 1024;
const INVALID_CONTROL: u32 = 1025;
const INVALID_VALUE: u32 = 1026;
const DESTINATION: u32 = 0x1000;
const SOURCE: u32 = 0x1001;
const VOLUME: u32 = 1;
const VOLUME_TYPE: u32 = 0x5003_0001;

fn result(code: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(code))
}
fn device(ctx: &CallCtx<'_>, object: u32, flags: u32) -> Result<(), u32> {
    match flags & 0xf000_0000 {
        // WinMM documents MIXER_OBJECTF_HMIXER as optional: ordinary
        // line/control queries accept a live mixer handle without that bit.
        0 if object == 0 || ctx.kernel.wave_out.mixer_handles.contains(&object) => Ok(()),
        0x1000_0000 if object == 0 => Ok(()),
        0x8000_0000 if ctx.kernel.wave_out.mixer_handles.contains(&object) => Ok(()),
        0x9000_0000 if ctx.kernel.wave_out.devices.contains_key(&object) => Ok(()),
        0x8000_0000 | 0x9000_0000 => Err(INVALID_HANDLE),
        0 | 0x1000_0000 => Err(2), // MMSYSERR_BADDEVICEID
        _ => Err(10),              // MMSYSERR_INVALFLAG; no input/MIDI devices are advertised.
    }
}
fn put(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn text(buf: &mut [u8], at: usize, chars: usize, wide: bool, value: &str) {
    for (i, ch) in value.encode_utf16().take(chars - 1).enumerate() {
        if wide {
            buf[at + i * 2..at + i * 2 + 2].copy_from_slice(&ch.to_le_bytes());
        } else {
            buf[at + i] = ch as u8;
        }
    }
}
fn num_devices(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    result(1)
}
fn get_id(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (object, out, flags) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if flags & 0x0fff_ffff != 0 {
        return result(10);
    }
    if let Err(e) = device(ctx, object, flags) {
        return result(e);
    }
    if out == 0 {
        return result(INVALID_PARAM);
    }
    ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
    result(0)
}
fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (out, object, flags) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(4)?);
    if let Err(e) = device(ctx, object, flags) {
        return result(e);
    }
    // No callback support is advertised by this virtual mixer. Reject rather
    // than accept a callback and then never deliver control-change messages.
    if flags & 0x0fff_ffff != 0 {
        return result(8);
    }
    if out == 0 {
        return result(INVALID_PARAM);
    }
    let Some(handle) = ctx
        .kernel
        .wave_out
        .next_mixer_handle
        .max(0xdead_4600)
        .checked_add(1)
    else {
        return result(4);
    };
    ctx.cpu.write_mem(out, &handle.to_le_bytes())?;
    ctx.kernel.wave_out.next_mixer_handle = handle;
    ctx.kernel.wave_out.mixer_handles.insert(handle);
    result(0)
}
fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = ctx.arg_u32(0)?;
    result(if ctx.kernel.wave_out.mixer_handles.remove(&h) {
        0
    } else {
        INVALID_HANDLE
    })
}
fn caps(ctx: &mut CallCtx<'_>, wide: bool) -> Result<DispatchOutcome, KernelError> {
    let (object, out, size) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)? as usize);
    if object != 0 && !ctx.kernel.wave_out.mixer_handles.contains(&object) {
        return result(2);
    }
    if out == 0 {
        return result(INVALID_PARAM);
    }
    let mut buf = vec![0; if wide { 80 } else { 48 }];
    put(&mut buf, 4, 0x0001_0000);
    text(&mut buf, 8, 32, wide, "PocketHLE PCM Mixer");
    let n = buf.len();
    put(&mut buf, n - 4, 1);
    ctx.cpu.write_mem(out, &buf[..size.min(n)])?;
    result(0)
}
fn line_info(ctx: &mut CallCtx<'_>, wide: bool) -> Result<DispatchOutcome, KernelError> {
    let (object, ptr, flags) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if let Err(e) = device(ctx, object, flags) {
        return result(e);
    }
    if flags & 0x0fff_fff0 != 0 {
        return result(10);
    }
    let size = if wide { 280 } else { 168 };
    if ptr == 0 || ctx.cpu.read_u32_le(ptr)? != size {
        return result(INVALID_PARAM);
    }
    let source = match flags & 15 {
        0 if ctx.cpu.read_u32_le(ptr + 4)? == 0 => false,
        1 if ctx.cpu.read_u32_le(ptr + 4)? == 0 && ctx.cpu.read_u32_le(ptr + 8)? == 0 => true,
        2 => match ctx.cpu.read_u32_le(ptr + 12)? {
            DESTINATION => false,
            SOURCE => true,
            _ => return result(INVALID_LINE),
        },
        3 => match ctx.cpu.read_u32_le(ptr + 24)? {
            4 => false,
            0x1008 => true,
            _ => return result(INVALID_LINE),
        },
        4 => {
            let at = if wide { 200 } else { 120 };
            let bytes = ctx
                .cpu
                .read_mem(ptr + at + 16, if wide { 64 } else { 32 })?;
            let name = if wide {
                String::from_utf16_lossy(
                    &bytes
                        .chunks_exact(2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .take_while(|&ch| ch != 0)
                        .collect::<Vec<_>>(),
                )
            } else {
                String::from_utf8_lossy(bytes.split(|&ch| ch == 0).next().unwrap_or(&[]))
                    .into_owned()
            };
            if ctx.cpu.read_u32_le(ptr + at)? != 1
                || ctx.cpu.read_u32_le(ptr + at + 8)? != 0
                || ctx.cpu.read_u32_le(ptr + at + 12)? != 0
                || name != "PocketHLE Wave Output"
            {
                return result(INVALID_LINE);
            }
            true
        }
        0 | 1 => return result(INVALID_LINE),
        _ => return result(10),
    };
    let mut buf = vec![0; size as usize];
    put(&mut buf, 0, size);
    put(&mut buf, 12, if source { SOURCE } else { DESTINATION });
    put(&mut buf, 16, if source { 0x8000_0001 } else { 1 });
    put(&mut buf, 24, if source { 0x1008 } else { 4 });
    put(&mut buf, 28, 2);
    put(&mut buf, 32, u32::from(!source));
    // Only the PCM source has a volume control. Do not invent a second host
    // master-volume knob or apply the same gain twice at different lines.
    put(&mut buf, 36, u32::from(source));
    let unit = if wide { 2 } else { 1 };
    text(
        &mut buf,
        40,
        16,
        wide,
        if source { "Wave Out" } else { "Speakers" },
    );
    text(
        &mut buf,
        40 + 16 * unit,
        64,
        wide,
        if source {
            "PocketHLE PCM Output"
        } else {
            "PocketHLE Speakers"
        },
    );
    let target = 40 + 80 * unit;
    put(&mut buf, target, 1);
    text(&mut buf, target + 16, 32, wide, "PocketHLE Wave Output");
    ctx.cpu.write_mem(ptr, &buf)?;
    result(0)
}
fn line_controls(ctx: &mut CallCtx<'_>, wide: bool) -> Result<DispatchOutcome, KernelError> {
    let (object, ptr, flags) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if let Err(e) = device(ctx, object, flags) {
        return result(e);
    }
    if flags & 0x0fff_fff0 != 0 {
        return result(10);
    }
    if ptr == 0 || ctx.cpu.read_u32_le(ptr)? != 24 {
        return result(INVALID_PARAM);
    }
    let query = flags & 15;
    if query > 2 {
        return result(10);
    }
    // ONEBYID deliberately ignores dwLineID (WinMM's union query contract).
    if query != 1 {
        let line = ctx.cpu.read_u32_le(ptr + 4)?;
        if !matches!(line, SOURCE | DESTINATION) {
            return result(INVALID_LINE);
        }
        if line == DESTINATION {
            return result(if query == 0 && ctx.cpu.read_u32_le(ptr + 12)? == 0 {
                0
            } else {
                INVALID_CONTROL
            });
        }
    }
    let selector = ctx.cpu.read_u32_le(ptr + 8)?;
    if (query == 1 && selector != VOLUME) || (query == 2 && selector != VOLUME_TYPE) {
        return result(INVALID_CONTROL);
    }
    let size = if wide { 228 } else { 148 };
    if ctx.cpu.read_u32_le(ptr + 12)? != 1 || ctx.cpu.read_u32_le(ptr + 16)? != size {
        return result(INVALID_PARAM);
    }
    let out = ctx.cpu.read_u32_le(ptr + 20)?;
    if out == 0 {
        return result(INVALID_PARAM);
    }
    let mut buf = vec![0; size as usize];
    put(&mut buf, 0, size);
    put(&mut buf, 4, VOLUME);
    put(&mut buf, 8, VOLUME_TYPE);
    let unit = if wide { 2 } else { 1 };
    text(&mut buf, 20, 16, wide, "Volume");
    text(&mut buf, 20 + 16 * unit, 64, wide, "PCM Output Volume");
    let bounds = 20 + 80 * unit;
    put(&mut buf, bounds + 4, 65535);
    put(&mut buf, bounds + 24, 65536);
    ctx.cpu.write_mem(out, &buf)?;
    result(0)
}
fn details(ctx: &mut CallCtx<'_>, set: bool) -> Result<DispatchOutcome, KernelError> {
    let (object, ptr, flags) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if let Err(e) = device(ctx, object, flags) {
        return result(e);
    }
    if flags & 0x0fff_ffff != 0 {
        return result(10);
    }
    if ptr == 0 || ctx.cpu.read_u32_le(ptr)? != 24 {
        return result(INVALID_PARAM);
    }
    if ctx.cpu.read_u32_le(ptr + 4)? != VOLUME {
        return result(INVALID_CONTROL);
    }
    let channels = ctx.cpu.read_u32_le(ptr + 8)?;
    if !(1..=2).contains(&channels)
        || ctx.cpu.read_u32_le(ptr + 12)? != 0
        || ctx.cpu.read_u32_le(ptr + 16)? != 4
    {
        return result(INVALID_PARAM);
    }
    let out = ctx.cpu.read_u32_le(ptr + 20)?;
    if out == 0 {
        return result(INVALID_PARAM);
    }
    if set {
        let left = ctx.cpu.read_u32_le(out)?;
        let right = if channels == 2 {
            ctx.cpu.read_u32_le(out + 4)?
        } else {
            left
        };
        if left > 65535 || right > 65535 {
            return result(INVALID_VALUE);
        }
        if !ctx.kernel.audio.set_wave_volume(0, left | (right << 16)) {
            return result(1);
        }
    } else {
        let Some(volume) = ctx.kernel.audio.wave_volume(0) else {
            return result(1);
        };
        ctx.cpu.write_mem(out, &(volume & 65535).to_le_bytes())?;
        if channels == 2 {
            ctx.cpu.write_mem(out + 4, &(volume >> 16).to_le_bytes())?;
        }
    }
    result(0)
}
fn caps_w(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    caps(c, true)
}
fn caps_a(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    caps(c, false)
}
fn line_w(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    line_info(c, true)
}
fn line_a(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    line_info(c, false)
}
fn controls_w(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    line_controls(c, true)
}
fn controls_a(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    line_controls(c, false)
}
fn get_details(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    details(c, false)
}
fn set_details(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    details(c, true)
}
pub(super) fn register(d: &mut WinCeDispatcher, dll: &str) {
    for (name, handler) in [
        ("mixerGetNumDevs", num_devices as crate::Handler),
        ("mixerGetID", get_id),
        ("mixerOpen", open),
        ("mixerClose", close),
        ("mixerSetControlDetails", set_details),
        ("mixerGetControlDetails", get_details),
        ("mixerGetControlDetailsW", get_details),
        ("mixerGetControlDetailsA", get_details),
        ("mixerGetDevCaps", caps_w),
        ("mixerGetDevCapsW", caps_w),
        ("mixerGetDevCapsA", caps_a),
        ("mixerGetLineInfo", line_w),
        ("mixerGetLineInfoW", line_w),
        ("mixerGetLineInfoA", line_a),
        ("mixerGetLineControls", controls_w),
        ("mixerGetLineControlsW", controls_w),
        ("mixerGetLineControlsA", controls_a),
    ] {
        d.register_handler(dll, name, handler);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn wave_mixer_handle_line_volume_and_cleanup_round_trip() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let format: Vec<u8> = [1u16, 2]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .chain([22050u32, 88200].into_iter().flat_map(u32::to_le_bytes))
            .chain([4u16, 16, 0].into_iter().flat_map(u16::to_le_bytes))
            .collect();
        cpu.write_mem(0x1100, &format).unwrap();
        macro_rules! invoke {
            ($name:expr, $args:expr) => {
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    $name,
                    $args,
                )
            };
        }
        assert_eq!(
            invoke!("waveOutOpen", &[0x1200, u32::MAX, 0x1100, 0, 0, 0]),
            DispatchOutcome::ReturnedR0(0)
        );
        let wave = cpu.read_u32_le(0x1200).unwrap();
        assert_eq!(
            invoke!("mixerGetID", &[wave, 0x1204, 0x9000_0000]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1204).unwrap(), 0);
        assert_eq!(
            invoke!("mixerOpen", &[0x1208, wave, 0, 0, 0x9000_0000]),
            DispatchOutcome::ReturnedR0(0)
        );
        let mixer = cpu.read_u32_le(0x1208).unwrap();
        assert_ne!(mixer, wave);
        cpu.write_mem(0x1300, &vec![0xa5; 284]).unwrap();
        cpu.write_mem(0x1300, &280u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1318, &0x1008u32.to_le_bytes()).unwrap();
        assert_eq!(
            invoke!("mixerGetLineInfo", &[mixer, 0x1300, 3]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x130c).unwrap(), SOURCE);
        assert_eq!(cpu.read_u32_le(0x1324).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(0x1418).unwrap(), 0xa5a5_a5a5);
        // TARGETTYPE matches the wave capabilities, not its output-only device
        // ID. Preserve the caller's canary beyond MIXERLINEW throughout.
        cpu.write_mem(0x13cc, &0x1234_5678u32.to_le_bytes())
            .unwrap();
        assert_eq!(
            invoke!("mixerGetLineInfoW", &[mixer, 0x1300, 4]),
            DispatchOutcome::ReturnedR0(0)
        );
        cpu.write_mem(0x13d0, &1u32.to_le_bytes()).unwrap();
        assert_eq!(
            invoke!("mixerGetLineInfoW", &[mixer, 0x1300, 4]),
            DispatchOutcome::ReturnedR0(INVALID_LINE)
        );
        assert_eq!(cpu.read_u32_le(0x1418).unwrap(), 0xa5a5_a5a5);

        let query: Vec<u8> = [24u32, SOURCE, VOLUME_TYPE, 1, 228, 0x1600]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        cpu.write_mem(0x1500, &query).unwrap();
        assert_eq!(
            invoke!("mixerGetLineControls", &[mixer, 0x1500, 2]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1604).unwrap(), VOLUME);
        assert_eq!(cpu.read_u32_le(0x16b8).unwrap(), 65535);
        // A by-ID request must not interpret the otherwise unused line ID.
        cpu.write_mem(0x1504, &0xffff_ffffu32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x1508, &VOLUME.to_le_bytes()).unwrap();
        assert_eq!(
            invoke!("mixerGetLineControls", &[mixer, 0x1500, 1]),
            DispatchOutcome::ReturnedR0(0)
        );
        let details: Vec<u8> = [24u32, VOLUME, 2, 0, 4, 0x1900]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        cpu.write_mem(0x1800, &details).unwrap();
        cpu.write_mem(0x1900, &0x1234u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1904, &0x5678u32.to_le_bytes()).unwrap();
        assert_eq!(
            invoke!("mixerSetControlDetails", &[mixer, 0x1800, 0]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.audio.wave_volume(0), Some(0x5678_1234));
        assert_eq!(kernel.audio.wave_volume(wave), Some(0x5678_1234));
        cpu.write_mem(0x1900, &[0; 8]).unwrap();
        assert_eq!(
            invoke!("mixerGetControlDetailsW", &[mixer, 0x1800, 0x8000_0000]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1900).unwrap(), 0x1234);
        assert_eq!(cpu.read_u32_le(0x1904).unwrap(), 0x5678);
        assert_eq!(
            invoke!("mixerClose", &[mixer]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            invoke!("mixerClose", &[mixer]),
            DispatchOutcome::ReturnedR0(INVALID_HANDLE)
        );
        assert!(kernel.wave_out.devices.contains_key(&wave));
    }

    #[test]
    fn invalid_mixer_requests_do_not_change_volume_or_last_error() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        kernel.thread_last_errors.insert(kernel.current_thread, 123);
        let before = kernel.audio.wave_volume(0);
        for (name, args, expected) in [
            (
                "mixerGetID",
                vec![0xdead_4100, 0x1200, 0x9000_0000],
                INVALID_HANDLE,
            ),
            ("mixerOpen", vec![0x1200, 4, 0, 0, 0], 2),
            ("mixerOpen", vec![0x1200, 0, 0, 0, 0x30000], 8),
            ("mixerGetID", vec![0, 0, 0], INVALID_PARAM),
            ("mixerClose", vec![0], INVALID_HANDLE),
        ] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    name,
                    &args
                ),
                DispatchOutcome::ReturnedR0(expected)
            );
        }
        assert_eq!(kernel.audio.wave_volume(0), before);
        assert_eq!(kernel.thread_last_errors[&kernel.current_thread], 123);
        assert!(kernel.wave_out.mixer_handles.is_empty());
    }
}
