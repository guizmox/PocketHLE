//! GetNumberFormatW for the emulator's English locale and explicit NUMBERFMTW.
use crate::CallCtx;
use pocket_kernel::{DispatchOutcome, KernelError};
fn fail(ctx: &mut CallCtx<'_>, e: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, e);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn wide(ctx: &mut CallCtx<'_>, ptr: u32) -> Option<String> {
    if ptr == 0 {
        return None;
    }
    let mut text = Vec::new();
    for i in 0..1024 {
        let b = ctx.cpu.read_mem(ptr + i * 2, 2).ok()?;
        let c = u16::from_le_bytes(b.try_into().ok()?);
        if c == 0 {
            return String::from_utf16(&text).ok();
        }
        text.push(c);
    }
    None
}
fn format(
    value: &str,
    digits: usize,
    leading: bool,
    group: u32,
    decimal: &str,
    thousand: &str,
    order: u32,
) -> Option<String> {
    if digits > 9 || order > 4 {
        return None;
    }
    let negative = value.starts_with('-');
    let value = value.strip_prefix('-').unwrap_or(value);
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if value.is_empty()
        || (whole.is_empty() && fraction.is_empty())
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut integer = whole.trim_start_matches('0').to_owned();
    if integer.is_empty() {
        integer.push('0');
    }
    let mut fraction_out = fraction.bytes().take(digits).collect::<Vec<_>>();
    fraction_out.resize(digits, b'0');
    if fraction.as_bytes().get(digits).is_some_and(|b| *b >= b'5') {
        let mut carry = true;
        for b in fraction_out.iter_mut().rev() {
            if *b == b'9' {
                *b = b'0';
            } else {
                *b += 1;
                carry = false;
                break;
            }
        }
        if carry {
            let mut b = integer.into_bytes();
            for d in b.iter_mut().rev() {
                if *d == b'9' {
                    *d = b'0';
                } else {
                    *d += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                b.insert(0, b'1');
            }
            integer = String::from_utf8(b).ok()?;
        }
    }
    let grouping: Vec<usize> = group
        .to_string()
        .bytes()
        .map(|b| (b - b'0') as usize)
        .collect();
    if group != 0 && !thousand.is_empty() {
        let (mut end, mut index) = (integer.len(), 0usize);
        let mut chunks = Vec::new();
        while end > 0 {
            let width = *grouping
                .get(index)
                .unwrap_or_else(|| grouping.last().unwrap());
            if width == 0 {
                chunks.push(&integer[..end]);
                break;
            }
            let start = end.saturating_sub(width);
            chunks.push(&integer[start..end]);
            end = start;
            if index + 1 < grouping.len() {
                index += 1;
            }
        }
        chunks.reverse();
        integer = chunks.join(thousand);
    }
    if !leading && integer == "0" && digits != 0 {
        integer.clear();
    }
    if digits != 0 {
        integer.push_str(decimal);
        integer.push_str(std::str::from_utf8(&fraction_out).ok()?);
    }
    Some(if negative {
        match order {
            0 => format!("({integer})"),
            1 => format!("-{integer}"),
            2 => format!("- {integer}"),
            3 => format!("{integer}-"),
            _ => format!("{integer} -"),
        }
    } else {
        integer
    })
}
pub(crate) fn get(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let flags = ctx.arg_u32(1)?;
    let input = ctx.arg_u32(2)?;
    let spec = ctx.arg_u32(3)?;
    let out = ctx.arg_u32(4)?;
    let capacity = ctx.arg_u32(5)?;
    if flags & !0x80000000 != 0 || (spec != 0 && flags != 0) {
        return fail(ctx, 1004);
    }
    let Some(value) = wide(ctx, input) else {
        return fail(ctx, 87);
    };
    let (mut digits, mut leading, mut group, mut decimal, mut thousand, mut order) =
        (2usize, true, 3u32, ".".to_owned(), ",".to_owned(), 1u32);
    if spec != 0 {
        let b = match ctx.cpu.read_mem(spec, 24) {
            Ok(b) => b,
            Err(_) => return fail(ctx, 87),
        };
        let word = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        digits = word(0) as usize;
        if word(4) > 1 {
            return fail(ctx, 87);
        }
        leading = word(4) != 0;
        group = word(8);
        order = word(20);
        let Some(d) = wide(ctx, word(12)) else {
            return fail(ctx, 87);
        };
        let Some(t) = wide(ctx, word(16)) else {
            return fail(ctx, 87);
        };
        decimal = d;
        thousand = t;
    }
    let Some(text) = format(&value, digits, leading, group, &decimal, &thousand, order) else {
        return fail(ctx, 87);
    };
    let encoded: Vec<_> = text.encode_utf16().chain(Some(0)).collect();
    let needed = encoded.len() as u32;
    if capacity == 0 {
        return Ok(DispatchOutcome::ReturnedR0(needed));
    }
    if out == 0 {
        return fail(ctx, 87);
    }
    if capacity < needed {
        return fail(ctx, 122);
    }
    if ctx
        .cpu
        .write_mem(
            out,
            &encoded
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .is_err()
    {
        return fail(ctx, 87);
    }
    Ok(DispatchOutcome::ReturnedR0(needed))
}
#[cfg(test)]
mod tests {
    use super::format;
    #[test]
    fn native_distance_rounding_grouping_and_negative_order() {
        assert_eq!(
            format("999.995", 2, true, 3, ".", ",", 1).as_deref(),
            Some("1,000.00")
        );
        assert_eq!(
            format("-0.125", 2, false, 0, ".", ",", 0).as_deref(),
            Some("(.13)")
        );
        assert_eq!(
            format("12345678", 0, true, 32, ".", ",", 1).as_deref(),
            Some("1,23,45,678")
        );
        assert!(format("1e3", 2, true, 3, ".", ",", 1).is_none());
    }
}
