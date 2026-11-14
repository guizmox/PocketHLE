//! Portable Explorer uses character-counted strsafe imports for its settings
//! paths. Returning S_OK without writing them leaves native shell paths empty.
use super::*;

const INVALID: u32 = 0x8007_0057;
const TOO_SMALL: u32 = 0x8007_007a;
const MAX_CCH: u32 = 0x7fff_ffff;

pub(super) fn register(dispatcher: &mut WinCeDispatcher, dll: &str) {
    for (name, handler) in [
        ("StringCchCopyA", ansi_copy as crate::Handler),
        ("StringCbCopyA", ansi_copy),
        ("StringCchCopyNA", ansi_copy_n),
        ("StringCbCopyNA", ansi_copy_n),
        ("StringCchCatA", ansi_cat),
        ("StringCbCatA", ansi_cat),
        ("StringCchCatNA", ansi_cat_n),
        ("StringCbCatNA", ansi_cat_n),
        ("StringCchLengthA", ansi_length),
        ("StringCbLengthA", ansi_length),
    ] {
        dispatcher.register_handler(dll, name, handler);
    }
    dispatcher.register_handler(dll, "StringCchCopyW", copy);
    dispatcher.register_handler(dll, "StringCchCopyNW", copy_n);
    dispatcher.register_handler(dll, "StringCchCatW", cat);
    dispatcher.register_handler(dll, "StringCchCatNW", cat_n);
    dispatcher.register_handler(dll, "StringCchLengthW", length);
    dispatcher.register_handler(dll, "StringCchCopyExW", copy_ex);
    dispatcher.register_handler(dll, "StringCchCopyNExW", copy_n_ex);
    dispatcher.register_handler(dll, "StringCchCatExW", cat_ex);
    dispatcher.register_handler(dll, "StringCchCatNExW", cat_n_ex);
    dispatcher.register_handler(dll, "StringCchPrintfExW", printf_ex);
    dispatcher.register_handler(dll, "StringCchVPrintfExW", vprintf_ex);
    dispatcher.register_handler(dll, "StringCchPrintfW", printf);
    dispatcher.register_handler(dll, "StringCchVPrintfW", vprintf);
    for (name, handler) in [
        ("StringCbCopyW", cb_copy as crate::Handler),
        ("StringCbCopyNW", cb_copy_n),
        ("StringCbCatW", cb_cat),
        ("StringCbCatNW", cb_cat_n),
        ("StringCbLengthW", cb_length),
        ("StringCbCopyExW", cb_copy_ex),
        ("StringCbCopyNExW", cb_copy_n_ex),
        ("StringCbCatExW", cb_cat_ex),
        ("StringCbCatNExW", cb_cat_n_ex),
        ("StringCbPrintfW", cb_printf),
        ("StringCbVPrintfW", cb_vprintf),
        ("StringCbPrintfExW", cb_printf_ex),
        ("StringCbVPrintfExW", cb_vprintf_ex),
    ] {
        dispatcher.register_handler(dll, name, handler);
    }
}

fn ansi_read(
    ctx: &mut CallCtx<'_>,
    pointer: u32,
    maximum: u32,
) -> Result<Option<Vec<u8>>, KernelError> {
    let mut bytes = Vec::new();
    for index in 0..maximum {
        let Some(address) = pointer.checked_add(index) else {
            return Ok(None);
        };
        if ctx.cpu.check_guest_access(address, 1, Prot::READ).is_err() {
            return Ok(None);
        }
        let byte = ctx.cpu.read_mem(address, 1)?[0];
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    Ok(Some(bytes))
}
fn ansi_copy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ansi_copy_cat(ctx, false, None)
}
fn ansi_copy_n(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let maximum = ctx.arg_u32(3)?;
    ansi_copy_cat(ctx, false, Some(maximum))
}
fn ansi_cat(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ansi_copy_cat(ctx, true, None)
}
fn ansi_cat_n(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let maximum = ctx.arg_u32(3)?;
    ansi_copy_cat(ctx, true, Some(maximum))
}
fn ansi_copy_cat(
    ctx: &mut CallCtx<'_>,
    append: bool,
    maximum: Option<u32>,
) -> Result<DispatchOutcome, KernelError> {
    let (dst, capacity, src) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if dst == 0
        || src == 0
        || capacity == 0
        || capacity > MAX_CCH
        || maximum.is_some_and(|n| n > MAX_CCH)
    {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let mut bytes = if append {
        let Some(bytes) = ansi_read(ctx, dst, capacity)? else {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        };
        if bytes.len() as u32 == capacity {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        }
        bytes
    } else {
        Vec::new()
    };
    let remaining = capacity - bytes.len() as u32;
    let Some(source) = ansi_read(ctx, src, maximum.unwrap_or(remaining).min(remaining))? else {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    };
    let truncated = source.len() as u32 >= remaining;
    bytes.extend(source.into_iter().take(remaining as usize - 1));
    bytes.push(0);
    if ctx
        .cpu
        .check_guest_access(dst, bytes.len() as u32, Prot::WRITE)
        .is_err()
    {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    ctx.cpu.write_mem(dst, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(if truncated {
        TOO_SMALL
    } else {
        0
    }))
}
fn ansi_length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (src, maximum, out) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if out != 0 && ctx.cpu.check_guest_access(out, 4, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let length = if src != 0 && maximum != 0 && maximum <= MAX_CCH {
        ansi_read(ctx, src, maximum)?
            .filter(|bytes| bytes.len() < maximum as usize)
            .map(|bytes| bytes.len() as u32)
    } else {
        None
    };
    if out != 0 {
        ctx.cpu.write_mem(out, &length.unwrap_or(0).to_le_bytes())?;
    }
    Ok(DispatchOutcome::ReturnedR0(if length.is_some() {
        0
    } else {
        INVALID
    }))
}

#[cfg(test)]
mod ansi_tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn ansi_paths_are_copied_as_bytes_with_a_terminator_and_a_truncation_result() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.write_mem(0x1100, b"folder\0").unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "StringCchCopyA",
                &[0x1200, 8, 0x1100]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_mem(0x1200, 7).unwrap(), b"folder\0");
        cpu.write_mem(0x1300, &[0xa5; 5]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "StringCchCopyA",
                &[0x1300, 4, 0x1100]
            ),
            DispatchOutcome::ReturnedR0(TOO_SMALL)
        );
        assert_eq!(
            cpu.read_mem(0x1300, 5).unwrap(),
            [b'f', b'o', b'l', 0, 0xa5]
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "StringCchLengthA",
                &[0x1200, 8, 0x1400]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1400).unwrap(), 6);
    }

    #[test]
    fn bounded_ansi_copy_stops_at_its_component_and_does_not_read_the_next_page() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.write_mem(0x4fff, &[0xe9]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "StringCchCopyNA",
                &[0x1200, 8, 0x4fff, 1]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_mem(0x1200, 2).unwrap(), [0xe9, 0]);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "StringCchCopyA",
                &[0x1300, 1, 0x4fff]
            ),
            DispatchOutcome::ReturnedR0(TOO_SMALL)
        );
        assert_eq!(cpu.read_mem(0x1300, 1).unwrap(), [0]);
        cpu.write_mem(0x1300, &[0xa5; 8]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "StringCchCopyA",
                &[0x1300, 8, 0x5000]
            ),
            DispatchOutcome::ReturnedR0(INVALID)
        );
        assert_eq!(cpu.read_mem(0x1300, 8).unwrap(), [0xa5; 8]);
    }
}

fn printf_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_ex_impl(ctx, false, false)
}
fn vprintf_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_ex_impl(ctx, true, false)
}
fn cb_printf_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_ex_impl(ctx, false, true)
}
fn cb_vprintf_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_ex_impl(ctx, true, true)
}
fn printf_ex_impl(
    ctx: &mut CallCtx<'_>,
    va_list: bool,
    byte_counted: bool,
) -> Result<DispatchOutcome, KernelError> {
    // CE 6 strsafe.h puts the format after five fixed arguments. Explorer
    // uses this ABI for folder captions and its file-backed settings paths.
    let (dst, raw_capacity, end, remaining, flags, format) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
        ctx.arg_u32(5)?,
    );
    let (capacity, tail) = count(raw_capacity, byte_counted);
    if capacity > MAX_CCH
        || [end, remaining]
            .into_iter()
            .any(|p| p != 0 && ctx.cpu.check_guest_access(p, 4, Prot::WRITE).is_err())
    {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    if capacity != 0
        && (dst == 0
            || dst.checked_add(capacity * 2 - 1).is_none()
            || ctx
                .cpu
                .check_guest_access(dst, capacity * 2, Prot::WRITE)
                .is_err())
    {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let ignore_nulls = flags & 0x100 != 0;
    let mut status = if flags & !0x1fff != 0 || format == 0 && !ignore_nulls {
        INVALID
    } else {
        0
    };
    let mut used = 0;
    if status == 0 {
        let fmt = if format == 0 {
            Some(String::new())
        } else {
            read_wstr(ctx, format, 0x4000)
                .ok()
                .and_then(|units| (units.len() < 0x4000).then(|| String::from_utf16_lossy(&units)))
        };
        if let Some(fmt) = fmt {
            if capacity == 0 {
                // The SDK tests the format itself and does not consume varargs
                // when no destination space exists, even for an empty %s.
                if !fmt.is_empty() {
                    status = if dst == 0 { INVALID } else { TOO_SMALL };
                }
            } else {
                let text = if va_list {
                    let va = ctx.arg_u32(6)?;
                    render_printf_va(ctx, &fmt, true, va)?
                } else {
                    render_printf(ctx, &fmt, true, 6)?
                };
                let units: Vec<_> = text.encode_utf16().collect();
                used = (units.len() as u32).min(capacity - 1);
                status = write(ctx, dst, &units, capacity)?;
                if status == 0 && flags & 0x200 != 0 && (capacity - used > 1 || tail != 0) {
                    let size = (capacity - used - 1) * 2 + tail;
                    if ctx
                        .cpu
                        .check_guest_access(dst + (used + 1) * 2, size, Prot::WRITE)
                        .is_err()
                    {
                        return Ok(DispatchOutcome::ReturnedR0(INVALID));
                    }
                    ctx.cpu
                        .write_mem(dst + (used + 1) * 2, &vec![flags as u8; size as usize])?;
                }
            }
        } else {
            status = INVALID;
        }
    }
    if status != 0 && dst != 0 && capacity != 0 {
        if flags & 0x400 != 0 {
            let size = capacity * 2 + tail;
            if ctx.cpu.check_guest_access(dst, size, Prot::WRITE).is_err() {
                return Ok(DispatchOutcome::ReturnedR0(INVALID));
            }
            ctx.cpu.write_mem(dst, &vec![flags as u8; size as usize])?;
            used = if flags as u8 == 0 { 0 } else { capacity - 1 };
            ctx.cpu.write_mem(dst + used * 2, &[0, 0])?;
        }
        if flags & (0x800 | 0x1000) != 0 {
            used = 0;
            ctx.cpu.write_mem(dst, &[0, 0])?;
        }
    }
    if status == 0 || status == TOO_SMALL {
        if end != 0 {
            ctx.cpu.write_mem(end, &(dst + used * 2).to_le_bytes())?;
        }
        if remaining != 0 {
            ctx.cpu.write_mem(
                remaining,
                &remaining_count(capacity - used, tail, byte_counted).to_le_bytes(),
            )?;
        }
    }
    Ok(DispatchOutcome::ReturnedR0(status))
}

fn count(value: u32, bytes: bool) -> (u32, u32) {
    if bytes {
        (value / 2, value % 2)
    } else {
        (value, 0)
    }
}
fn remaining_count(characters: u32, tail: u32, bytes: bool) -> u32 {
    if bytes {
        characters * 2 + tail
    } else {
        characters
    }
}
fn printf(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_impl(ctx, false, false)
}
fn vprintf(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_impl(ctx, true, false)
}
fn cb_printf(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_impl(ctx, false, true)
}
fn cb_vprintf(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_impl(ctx, true, true)
}
fn printf_impl(
    ctx: &mut CallCtx<'_>,
    va_list: bool,
    bytes: bool,
) -> Result<DispatchOutcome, KernelError> {
    let (dst, raw_capacity, format) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let (capacity, _) = count(raw_capacity, bytes);
    if dst == 0
        || format == 0
        || capacity == 0
        || capacity > MAX_CCH
        || dst.checked_add(capacity * 2 - 1).is_none()
        || ctx
            .cpu
            .check_guest_access(dst, capacity * 2, Prot::WRITE)
            .is_err()
    {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let fmt = String::from_utf16_lossy(&read_wstr(ctx, format, 0x4000)?);
    let text = if va_list {
        let va = ctx.arg_u32(3)?;
        render_printf_va(ctx, &fmt, true, va)?
    } else {
        render_printf(ctx, &fmt, true, 3)?
    };
    Ok(DispatchOutcome::ReturnedR0(write(
        ctx,
        dst,
        &text.encode_utf16().collect::<Vec<_>>(),
        capacity,
    )?))
}

fn write(
    ctx: &mut CallCtx<'_>,
    dst: u32,
    units: &[u16],
    capacity: u32,
) -> Result<u32, KernelError> {
    let room = capacity as usize - 1;
    let count = units.len().min(room);
    let mut bytes: Vec<_> = units[..count]
        .iter()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    bytes.extend_from_slice(&[0, 0]);
    if ctx
        .cpu
        .check_guest_access(dst, bytes.len() as u32, Prot::WRITE)
        .is_err()
    {
        return Ok(INVALID);
    }
    ctx.cpu.write_mem(dst, &bytes)?;
    Ok(if units.len() > room { TOO_SMALL } else { 0 })
}

fn copy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    copy_impl(ctx, None, false)
}
fn copy_n(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let count = ctx.arg_u32(3)?;
    copy_impl(ctx, Some(count), false)
}
fn cb_copy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    copy_impl(ctx, None, true)
}
fn cb_copy_n(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let maximum = ctx.arg_u32(3)? / 2;
    copy_impl(ctx, Some(maximum), true)
}
fn copy_impl(
    ctx: &mut CallCtx<'_>,
    maximum: Option<u32>,
    bytes: bool,
) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let (capacity, _) = count(ctx.arg_u32(1)?, bytes);
    let src = ctx.arg_u32(2)?;
    if dst == 0
        || src == 0
        || capacity == 0
        || capacity > MAX_CCH
        || maximum.is_some_and(|n| n > MAX_CCH)
    {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let units = read_wstr(ctx, src, maximum.unwrap_or(capacity).min(capacity))?;
    Ok(DispatchOutcome::ReturnedR0(write(
        ctx, dst, &units, capacity,
    )?))
}
fn cat(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    cat_impl(ctx, None, false)
}
fn cat_n(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let count = ctx.arg_u32(3)?;
    cat_impl(ctx, Some(count), false)
}
fn cb_cat(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    cat_impl(ctx, None, true)
}
fn cb_cat_n(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let maximum = ctx.arg_u32(3)? / 2;
    cat_impl(ctx, Some(maximum), true)
}
fn cat_impl(
    ctx: &mut CallCtx<'_>,
    maximum: Option<u32>,
    bytes: bool,
) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let (capacity, _) = count(ctx.arg_u32(1)?, bytes);
    let src = ctx.arg_u32(2)?;
    if dst == 0
        || src == 0
        || capacity == 0
        || capacity > MAX_CCH
        || maximum.is_some_and(|n| n > MAX_CCH)
    {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let mut units = read_wstr(ctx, dst, capacity)?;
    if units.len() >= capacity as usize {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let remaining = capacity - units.len() as u32;
    units.extend(read_wstr(
        ctx,
        src,
        maximum.unwrap_or(remaining).min(remaining),
    )?);
    Ok(DispatchOutcome::ReturnedR0(write(
        ctx, dst, &units, capacity,
    )?))
}
fn length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    length_impl(ctx, false)
}
fn cb_length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    length_impl(ctx, true)
}
fn length_impl(ctx: &mut CallCtx<'_>, bytes: bool) -> Result<DispatchOutcome, KernelError> {
    let src = ctx.arg_u32(0)?;
    let (capacity, _) = count(ctx.arg_u32(1)?, bytes);
    let output = ctx.arg_u32(2)?;
    if output != 0 && ctx.cpu.check_guest_access(output, 4, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let units = if src == 0 || capacity == 0 || capacity > MAX_CCH {
        None
    } else {
        Some(read_wstr(ctx, src, capacity)?)
    };
    let length = units
        .as_ref()
        .filter(|u| u.len() < capacity as usize)
        .map(|u| u.len() as u32);
    if output != 0 {
        ctx.cpu.write_mem(
            output,
            &remaining_count(length.unwrap_or(0), 0, bytes).to_le_bytes(),
        )?;
    }
    Ok(DispatchOutcome::ReturnedR0(if length.is_some() {
        0
    } else {
        INVALID
    }))
}

fn copy_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ex_impl(ctx, false, false, false)
}
fn cat_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ex_impl(ctx, true, false, false)
}
fn cb_copy_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ex_impl(ctx, false, true, false)
}
fn cb_cat_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ex_impl(ctx, true, true, false)
}
fn copy_n_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ex_impl(ctx, false, false, true)
}
fn cat_n_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ex_impl(ctx, true, false, true)
}
fn cb_copy_n_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ex_impl(ctx, false, true, true)
}
fn cb_cat_n_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ex_impl(ctx, true, true, true)
}
fn ex_impl(
    ctx: &mut CallCtx<'_>,
    append: bool,
    byte_counted: bool,
    limited: bool,
) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let (capacity, tail) = count(ctx.arg_u32(1)?, byte_counted);
    let src = ctx.arg_u32(2)?;
    // Original CE6 ceshell.dll imports CopyNEx for bounded path components.
    // Its extra source count shifts all three output/flags arguments.
    let maximum = if limited {
        Some(count(ctx.arg_u32(3)?, byte_counted).0)
    } else {
        None
    };
    let first_output = if limited { 4 } else { 3 };
    let end = ctx.arg_u32(first_output)?;
    let remaining = ctx.arg_u32(first_output + 1)?;
    let flags = ctx.arg_u32(first_output + 2)?;
    if [end, remaining]
        .iter()
        .any(|&p| p != 0 && ctx.cpu.check_guest_access(p, 4, Prot::WRITE).is_err())
    {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let ignore_nulls = flags & 0x100 != 0;
    if capacity > MAX_CCH
        || maximum.is_some_and(|n| n > MAX_CCH)
        || (dst == 0 && (capacity != 0 || !ignore_nulls))
        || (capacity == 0 && !ignore_nulls)
    {
        if dst != 0 && capacity != 0 && ctx.cpu.check_guest_access(dst, 2, Prot::WRITE).is_ok() {
            ctx.cpu.write_mem(dst, &[0, 0])?;
        }
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let mut units = if append && capacity != 0 {
        read_wstr(ctx, dst, capacity)?
    } else {
        Vec::new()
    };
    if units.len() >= capacity as usize && capacity != 0 {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let original = units.len() as u32;
    let status;
    let mut count = original;
    if flags & !0x1fff != 0 || src == 0 && !ignore_nulls {
        status = INVALID;
        if capacity != 0 {
            ctx.cpu.write_mem(dst, &[0, 0])?;
            count = 0;
        }
    } else {
        let source = if src == 0 {
            Vec::new()
        } else {
            let available = (capacity - original).max(1);
            read_wstr(ctx, src, maximum.unwrap_or(available).min(available))?
        };
        if capacity == 0 {
            status = if source.is_empty() {
                0
            } else if dst == 0 {
                INVALID
            } else {
                TOO_SMALL
            };
        } else {
            units.extend(source);
            count = (units.len() as u32).min(capacity - 1);
            status = write(ctx, dst, &units, capacity)?;
        }
    }
    if status == 0
        && capacity != 0
        && flags & 0x200 != 0
        && (capacity.saturating_sub(count) > 1 || tail != 0)
    {
        let start = dst
            .checked_add((count + 1) * 2)
            .ok_or(KernelError::Loader("strsafe destination overflow".into()))?;
        let size = (capacity - count - 1) * 2 + tail;
        if ctx
            .cpu
            .check_guest_access(start, size, Prot::WRITE)
            .is_err()
        {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        }
        ctx.cpu
            .write_mem(start, &vec![flags as u8; size as usize])?;
    }
    if status != 0 && capacity != 0 {
        if flags & 0x1000 != 0 {
            count = original;
            ctx.cpu.write_mem(dst + count * 2, &[0, 0])?;
        }
        if flags & 0x400 != 0 {
            let size = capacity * 2 + tail;
            if ctx.cpu.check_guest_access(dst, size, Prot::WRITE).is_err() {
                return Ok(DispatchOutcome::ReturnedR0(INVALID));
            }
            ctx.cpu.write_mem(dst, &vec![flags as u8; size as usize])?;
            count = if flags as u8 == 0 { 0 } else { capacity - 1 };
            ctx.cpu.write_mem(dst + count * 2, &[0, 0])?;
        }
        if flags & 0x800 != 0 {
            count = 0;
            ctx.cpu.write_mem(dst, &[0, 0])?;
        }
    }
    if status == 0 || status == TOO_SMALL {
        let Some(pointer) = dst.checked_add(count * 2) else {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        };
        if end != 0 {
            ctx.cpu.write_mem(end, &pointer.to_le_bytes())?;
        }
        if remaining != 0 {
            ctx.cpu.write_mem(
                remaining,
                &remaining_count(capacity - count, tail, byte_counted).to_le_bytes(),
            )?;
        }
    }
    Ok(DispatchOutcome::ReturnedR0(status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::{stub::StubCpu, Cpu};

    #[test]
    fn bounded_ex_reads_only_the_requested_source_and_uses_seven_argument_abi() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut state = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        cpu.map_region(0x6000, 0x1000, Prot::READ).unwrap();
        // One character at the last readable address, deliberately no NUL.
        cpu.write_mem(0x6ffe, &u16::from(b'Q').to_le_bytes())
            .unwrap();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        for (handler, append, bytes) in [
            (copy_n_ex as crate::Handler, false, false),
            (cat_n_ex, true, false),
            (cb_copy_n_ex, false, true),
            (cb_cat_n_ex, true, true),
        ] {
            ctx.cpu.write_mem(0x1200, &[0xcc; 12]).unwrap();
            ctx.cpu.write_mem(0x1200, &[b'A', 0, 0, 0]).unwrap();
            let capacity = if bytes { 9 } else { 4 };
            let limit = if bytes { 3 } else { 1 };
            arguments(
                &mut ctx,
                &[0x1200, capacity, 0x6ffe, limit, 0x1800, 0x1804, 0x2a5],
            );
            assert_eq!(handler(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
            let expected = if append { "AQ\0" } else { "Q\0" };
            let expected: Vec<_> = expected.encode_utf16().flat_map(u16::to_le_bytes).collect();
            assert_eq!(
                ctx.cpu.read_mem(0x1200, expected.len() as u32).unwrap(),
                expected
            );
            let used = if append { 2 } else { 1 };
            assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0x1200 + used * 2);
            assert_eq!(
                ctx.cpu.read_u32_le(0x1804).unwrap(),
                if bytes { 9 - used * 2 } else { 4 - used }
            );
            let end = if bytes { 9 } else { 8 };
            assert_eq!(
                ctx.cpu.read_mem(0x1200 + end, 12 - end).unwrap(),
                vec![0xcc; (12 - end) as usize]
            );
            assert_eq!(
                ctx.cpu
                    .read_mem(0x1200 + (used + 1) * 2, end - (used + 1) * 2)
                    .unwrap(),
                vec![0xa5; (end - (used + 1) * 2) as usize]
            );
        }
        // A zero source count does not require any source bytes.
        arguments(&mut ctx, &[0x1200, 4, 0xffff0000, 0, 0x1800, 0x1804, 0]);
        assert_eq!(copy_n_ex(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.cpu.read_mem(0x1200, 2).unwrap(), vec![0, 0]);
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0x1200);
        assert_eq!(ctx.cpu.read_u32_le(0x1804).unwrap(), 4);
    }

    #[test]
    fn byte_counted_formatting_preserves_odd_byte_bounds_and_reports_byte_lengths() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut state = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        ctx.cpu
            .write_mem(
                0x1100,
                &"%s\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        ctx.cpu
            .write_mem(
                0x1500,
                &"CE\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        ctx.cpu.write_mem(0x1200, &[0xcc; 12]).unwrap();
        arguments(
            &mut ctx,
            &[0x1200, 9, 0x1800, 0x1804, 0x2a5, 0x1100, 0x1500],
        );
        assert_eq!(
            cb_printf_ex(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            ctx.cpu.read_mem(0x1200, 6).unwrap(),
            "CE\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            ctx.cpu.read_mem(0x1206, 6).unwrap(),
            vec![0xa5, 0xa5, 0xa5, 0xcc, 0xcc, 0xcc]
        );
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0x1204);
        assert_eq!(ctx.cpu.read_u32_le(0x1804).unwrap(), 5);
        arguments(&mut ctx, &[0x1200, 9, 0x1804]);
        assert_eq!(cb_length(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.cpu.read_u32_le(0x1804).unwrap(), 4);
        arguments(&mut ctx, &[0x1200, 9, 0x1500, 3]);
        assert_eq!(cb_copy_n(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(
            ctx.cpu.read_mem(0x1200, 4).unwrap(),
            "C\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
    }

    fn arguments(ctx: &mut CallCtx<'_>, values: &[u32]) {
        for (index, &value) in values.iter().enumerate() {
            if index < 4 {
                ctx.cpu.write_arg(index as u8, value).unwrap();
            } else {
                let sp = ctx.cpu.read_reg(ArmReg::Sp).unwrap();
                ctx.cpu
                    .write_mem(sp + (index as u32 - 4) * 4, &value.to_le_bytes())
                    .unwrap();
            }
        }
    }

    #[test]
    fn formatted_ex_uses_the_ce_stack_arguments_and_reports_end_and_remaining() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut state = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        ctx.cpu
            .write_mem(
                0x1100,
                &"%s %u\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        ctx.cpu
            .write_mem(
                0x1500,
                &"CE\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        ctx.cpu.write_mem(0x1200, &[0xcc; 20]).unwrap();
        arguments(
            &mut ctx,
            &[0x1200, 8, 0x1800, 0x1804, 0x2a5, 0x1100, 0x1500, 6],
        );
        assert_eq!(printf_ex(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(
            ctx.cpu.read_mem(0x1200, 10).unwrap(),
            "CE 6\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(ctx.cpu.read_mem(0x120a, 6).unwrap(), vec![0xa5; 6]);
        assert_eq!(ctx.cpu.read_mem(0x1210, 4).unwrap(), vec![0xcc; 4]);
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0x1208);
        assert_eq!(ctx.cpu.read_u32_le(0x1804).unwrap(), 4);
        ctx.cpu
            .write_mem(
                0x1600,
                &[0x1500u32, 6]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        arguments(&mut ctx, &[0x1200, 8, 0x1800, 0x1804, 0, 0x1100, 0x1600]);
        assert_eq!(
            vprintf_ex(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        arguments(
            &mut ctx,
            &[0x1200, 4, 0x1800, 0x1804, 0x1401, 0x1100, 0x1500, 6],
        );
        assert_eq!(
            printf_ex(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(TOO_SMALL)
        );
        assert_eq!(
            ctx.cpu.read_mem(0x1200, 8).unwrap(),
            vec![0, 0, 1, 1, 1, 1, 0, 0]
        );
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0x1200);
        assert_eq!(ctx.cpu.read_u32_le(0x1804).unwrap(), 4);
    }

    #[test]
    fn formatted_ex_zero_capacity_and_invalid_outputs_do_not_consume_or_modify_buffers() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut state = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        arguments(&mut ctx, &[0, 0, 0x1800, 0x1804, 0x100, 0]);
        assert_eq!(printf_ex(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.cpu.read_u32_le(0x1800).unwrap(), 0);
        assert_eq!(ctx.cpu.read_u32_le(0x1804).unwrap(), 0);
        ctx.cpu
            .write_mem(
                0x1100,
                &"%s\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        ctx.cpu.write_mem(0x1200, &[0xcc; 16]).unwrap();
        arguments(&mut ctx, &[0x1200, 0, 0x1800, 0x1804, 0, 0x1100, u32::MAX]);
        assert_eq!(
            printf_ex(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(TOO_SMALL)
        );
        ctx.cpu.map_region(0x6000, 0x1000, Prot::READ).unwrap();
        arguments(&mut ctx, &[0x1200, 8, 0x6000, 0, 0, 0x1100, u32::MAX]);
        assert_eq!(
            printf_ex(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(INVALID)
        );
        arguments(&mut ctx, &[0x1200, MAX_CCH + 1, 0, 0, 0x400, 0x1100]);
        assert_eq!(
            printf_ex(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(INVALID)
        );
        assert_eq!(ctx.cpu.read_mem(0x1200, 16).unwrap(), vec![0xcc; 16]);
    }
    #[test]
    fn character_capacity_truncates_terminates_and_reports_utf16_length() {
        let mut cpu = StubCpu::new();
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        let mut state = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut state,
            thunk: &thunk,
        };
        ctx.cpu
            .write_mem(
                0x1100,
                &"A😀B\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        ctx.cpu.write_mem(0x1200, &[0xa5; 16]).unwrap();
        for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2]
            .into_iter()
            .zip([0x1200, 4, 0x1100])
        {
            ctx.cpu.write_reg(reg, value).unwrap();
        }
        assert_eq!(
            copy(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(TOO_SMALL)
        );
        assert_eq!(
            ctx.cpu.read_mem(0x1200, 8).unwrap(),
            "A😀\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(ctx.cpu.read_mem(0x1208, 2).unwrap(), vec![0xa5; 2]);
        ctx.cpu.write_reg(ArmReg::R0, 0x1200).unwrap();
        ctx.cpu.write_reg(ArmReg::R1, 4).unwrap();
        ctx.cpu.write_reg(ArmReg::R2, 0x1300).unwrap();
        assert_eq!(length(&mut ctx).unwrap(), DispatchOutcome::ReturnedR0(0));
        assert_eq!(ctx.cpu.read_u32_le(0x1300).unwrap(), 3);
        ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            length(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(INVALID)
        );
        assert_eq!(ctx.cpu.read_u32_le(0x1300).unwrap(), 0);
    }
}
