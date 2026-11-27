//! CE3 DDV validates actual guest values; failure reports through the real UI
//! and throws CUserException into the existing per-thread TRY chain.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::Prot;
const FAIL: &str = "__PocketHLE_MFC_DataExchangeFail";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1149, "?Fail@CDataExchange@@QAAXXZ", fail),
    (
        990,
        "?DDV_MaxChars@@YAXPAVCDataExchange@@ABVCString@@H@Z",
        max_chars,
    ),
    (991, "?DDV_MinMaxByte@@YAXPAVCDataExchange@@EEE@Z", byte),
    (
        993,
        "?DDV_MinMaxDWord@@YAXPAVCDataExchange@@KKK@Z",
        unsigned,
    ),
    (
        992,
        "?DDV_MinMaxDouble@@YAXPAVCDataExchange@@ABNNN@Z",
        double,
    ),
    (994, "?DDV_MinMaxFloat@@YAXPAVCDataExchange@@ABMMM@Z", float),
    (995, "?DDV_MinMaxInt@@YAXPAVCDataExchange@@HHH@Z", signed),
    (996, "?DDV_MinMaxLong@@YAXPAVCDataExchange@@JJJ@Z", signed),
    (997, "?DDV_MinMaxShort@@YAXPAVCDataExchange@@FFF@Z", short),
    (998, "?DDV_MinMaxSlider@@YAXPAVCDataExchange@@KKK@Z", slider),
    (999, "?DDV_MinMaxUInt@@YAXPAVCDataExchange@@III@Z", unsigned),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", FAIL, fail);
}
fn fail(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let dx = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu.check_guest_access(dx, 16, Prot::READ)?;
        let control = word(ctx, dx + 8)?;
        if word(ctx, dx)? != 0 && control != 0 {
            return f.call_service(ctx, 1, "SetFocus", &[control]);
        }
    } else if f.phase(ctx)? == 1 && word(ctx, dx + 12)? != 0 {
        let control = word(ctx, dx + 8)?;
        return f.call_service(ctx, 2, "SendMessageW", &[control, 0xb1, 0, u32::MAX]);
    }
    f.call_service(ctx, 3, super::exception_throw::USER, &[])
}
fn signed(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    integer(ctx, |v| v as i32 as i64)
}
fn unsigned(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    integer(ctx, |v| v as i64)
}
fn byte(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    integer(ctx, |v| v as u8 as i64)
}
fn short(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    integer(ctx, |v| v as i16 as i64)
}
fn integer(ctx: &mut CallCtx<'_>, convert: fn(u32) -> i64) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return after_prompt(ctx, f);
    }
    let dx = f.arg(ctx, 0)?;
    let value = convert(f.arg(ctx, 1)?);
    let lo = convert(f.arg(ctx, 2)?);
    let hi = convert(f.arg(ctx, 3)?);
    if word(ctx, dx)? == 0 || (value >= lo && value <= hi) {
        return f.finish(ctx, 0);
    }
    prompt(ctx, f, 0xf112, &[lo.to_string(), hi.to_string()])
}
fn float(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    floating(ctx, false)
}
fn double(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    floating(ctx, true)
}
fn floating(ctx: &mut CallCtx<'_>, double: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return after_prompt(ctx, f);
    }
    let dx = f.arg(ctx, 0)?;
    let pointer = f.arg(ctx, 1)?;
    let (value, lo, hi) = if double {
        let b = ctx.cpu.read_mem(pointer, 8)?;
        let lo = (f.arg(ctx, 2)? as u64) | ((f.arg(ctx, 3)? as u64) << 32);
        let low = ctx.arg_u32(4)? as u64;
        let high = ctx.arg_u32(5)? as u64;
        (
            f64::from_le_bytes(b.try_into().unwrap()),
            f64::from_bits(lo),
            f64::from_bits(low | (high << 32)),
        )
    } else {
        (
            f32::from_bits(word(ctx, pointer)?) as f64,
            f32::from_bits(f.arg(ctx, 2)?) as f64,
            f32::from_bits(f.arg(ctx, 3)?) as f64,
        )
    };
    if word(ctx, dx)? == 0 || (value >= lo && value <= hi) {
        return f.finish(ctx, 0);
    }
    // CE3 uses %.*g (7 float / 15 double significant digits).
    let precision = if double { 15 } else { 7 };
    let a = crate::coredll::format_general_float(lo, precision);
    let b = crate::coredll::format_general_float(hi, precision);
    prompt(ctx, f, 0xf113, &[a, b])
}
fn max_chars(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        if f.phase(ctx)? == 4 {
            return f.finish(ctx, 0);
        }
        return after_prompt(ctx, f);
    }
    let dx = f.arg(ctx, 0)?;
    let string = f.arg(ctx, 1)?;
    let max = f.arg(ctx, 2)? as i32;
    if word(ctx, dx)? != 0 && super::strings::value(ctx, string)?.len() as i64 > max as i64 {
        return prompt(ctx, f, 0xf114, &[max.to_string()]);
    }
    let hwnd = word(ctx, dx + 8)?;
    if hwnd != 0 && word(ctx, dx + 12)? != 0 {
        return f.call_service(ctx, 4, "SendMessageW", &[hwnd, 0xc5, max as u32, 0]);
    }
    f.finish(ctx, 0)
}
fn slider(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let dx = f.arg(ctx, 0)?;
    if !f.fresh {
        if f.phase(ctx)? == 1 {
            let hwnd = word(ctx, dx + 8)?;
            let hi = f.arg(ctx, 3)?;
            return f.call_service(ctx, 2, "SendMessageW", &[hwnd, 0x407, 1, hi]);
        }
        return f.finish(ctx, 0);
    }
    let v = f.arg(ctx, 1)?;
    let lo = f.arg(ctx, 2)?;
    let hi = f.arg(ctx, 3)?;
    if word(ctx, dx)? == 0 && (v < lo || v > hi) {
        return f.finish(ctx, 0);
    }
    // Preserve the two messages in the supplied CE3 implementation, including
    // its repeated 0x407; do not silently substitute desktop-MFC behavior.
    let hwnd = word(ctx, dx + 8)?;
    f.call_service(ctx, 1, "SendMessageW", &[hwnd, 0x407, 0, lo])
}
fn prompt(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    id: u32,
    arguments: &[String],
) -> Result<DispatchOutcome, KernelError> {
    let template = super::resources::framework_string(ctx, id)?;
    let mut result = Vec::new();
    let mut i = 0;
    while i < template.len() {
        if template[i] == b'%' as u16 && i + 1 < template.len() {
            let c = template[i + 1];
            if (b'1' as u16..=b'9' as u16).contains(&c) {
                let index = (c - b'1' as u16) as usize;
                if let Some(s) = arguments.get(index) {
                    result.extend(s.encode_utf16());
                    i += 2;
                    continue;
                }
            }
            if c == b'%' as u16 {
                result.push(c);
                i += 2;
                continue;
            }
        }
        result.push(template[i]);
        i += 1;
    }
    let p = super::application::duplicate_wide(ctx, &result)?;
    f.own(ctx, p)?;
    f.call_service(ctx, 1, super::message_box::TEXT, &[p, 0x30, id])
}
fn after_prompt(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let dx = f.arg(ctx, 0)?;
    f.call_service(ctx, 2, FAIL, &[dx])
}
