//! Unicode CString operations verified against the CE3 ARM runtime.
use super::*;

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2473, "?TrimRight@CString@@QAAXXZ", trim_right),
    (2470, "?TrimLeft@CString@@QAAXXZ", trim_left),
    (1120, "?Empty@CString@@QAAXXZ", empty),
    (1156, "?Find@CString@@QBAHG@Z", find_character),
    (1157, "?Find@CString@@QBAHGH@Z", find_character_from),
    (1158, "?Find@CString@@QBAHPBG@Z", find_text),
    (1159, "?Find@CString@@QBAHPBGH@Z", find_text_from),
    (2231, "?ReverseFind@CString@@QBAHG@Z", reverse_find),
    (1733, "?LockBuffer@CString@@QAAPAGXZ", lock_buffer),
    (2485, "?UnlockBuffer@CString@@QAAXXZ", unlock_buffer),
    (365, "??YCString@@QAAABV0@G@Z", append_character),
    (359, "??H@YA?AVCString@@ABV0@0@Z", concatenate_strings),
    (363, "??H@YA?AVCString@@PBGABV0@@Z", concatenate_prefix),
    (1714, "?Left@CString@@QBA?AV1@H@Z", left),
    (2233, "?Right@CString@@QBA?AV1@H@Z", right),
    (
        2450,
        "?SpanExcluding@CString@@QBA?AV1@PBG@Z",
        span_excluding,
    ),
    (
        2451,
        "?SpanIncluding@CString@@QBA?AV1@PBG@Z",
        span_including,
    ),
    (1724, "?LoadStringW@CString@@QAAHI@Z", load_string),
    (176, "??0CString@@QAA@GH@Z", repeat_character),
    (322, "??4CString@@QAAABV0@G@Z", assign_character),
    (323, "??4CString@@QAAABV0@PBD@Z", assign_ansi),
    (1761, "?Mid@CString@@QBA?AV1@H@Z", mid_tail),
    (1762, "?Mid@CString@@QBA?AV1@HH@Z", mid_count),
    (1753, "?MakeUpper@CString@@QAAXXZ", make_upper),
    (1751, "?MakeLower@CString@@QAAXXZ", make_lower),
    (179, "??0CString@@QAA@PBG@Z", construct_wide),
    (180, "??0CString@@QAA@PBGH@Z", construct_wide_len),
    (175, "??0CString@@QAA@ABV0@@Z", copy as crate::Handler),
    (324, "??4CString@@QAAABV0@PBG@Z", assign_wide),
    (361, "??H@YA?AVCString@@ABV0@PBG@Z", concatenate),
    (366, "??YCString@@QAAABV0@PBG@Z", append_wide),
    (364, "??YCString@@QAAABV0@ABV0@@Z", append_string),
    (1173, "?Format@CString@@QAAXPBGZZ", format),
    (1172, "?Format@CString@@QAAXIZZ", format_resource),
    (1229, "?GetBuffer@CString@@QAAPAGH@Z", get_buffer),
    (
        1233,
        "?GetBufferSetLength@CString@@QAAPAGH@Z",
        get_buffer_set_length,
    ),
    (2167, "?ReleaseBuffer@CString@@QAAXH@Z", release_buffer),
];

pub(super) fn wide(ctx: &mut CallCtx<'_>, p: u32) -> Result<Vec<u16>, KernelError> {
    if p == 0 {
        return Ok(Vec::new());
    }
    let mut text = Vec::new();
    for i in 0..=0x100000u32 {
        let c = u16::from_le_bytes(
            ctx.cpu
                .read_mem(
                    p.checked_add(i * 2)
                        .ok_or_else(|| KernelError::Loader("MFC string address overflow".into()))?,
                    2,
                )?
                .try_into()
                .unwrap(),
        );
        if c == 0 {
            return Ok(text);
        }
        text.push(c);
    }
    Err(KernelError::Loader("MFC string unterminated".into()))
}
pub(super) fn value(ctx: &mut CallCtx<'_>, object: u32) -> Result<Vec<u16>, KernelError> {
    ctx.cpu
        .check_guest_access(object, 4, pocket_cpu::Prot::READ)?;
    let p = word(ctx, object)?;
    let header = string_header(p)?;
    ctx.cpu
        .check_guest_access(header, 12, pocket_cpu::Prot::READ)?;
    let len = word(ctx, header + 4)?;
    let cap = word(ctx, header + 8)?;
    if len > cap || cap > 0x100000 {
        return Err(KernelError::Loader("MFC string length invalid".into()));
    }
    ctx.cpu
        .check_guest_access(p, len * 2, pocket_cpu::Prot::READ)?;
    Ok(ctx
        .cpu
        .read_mem(p, len * 2)?
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect())
}
pub(super) fn replace(ctx: &mut CallCtx<'_>, object: u32, text: &[u16]) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 4, pocket_cpu::Prot::WRITE)?;
    let slot = alloc(ctx, 4)?;
    if let Err(e) = new_string(ctx, slot, text) {
        ctx.kernel.heap.free(slot);
        return Err(e);
    }
    let p = word(ctx, slot)?;
    let result = release_string(ctx, object).and_then(|_| put(ctx, object, p));
    if result.is_err() {
        ctx.kernel.heap.free(p - 12);
    }
    ctx.kernel.heap.free(slot);
    result
}
fn trim(ctx: &mut CallCtx<'_>, right: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let text = value(ctx, object)?;
    // Match the shared CRT iswctype SPACE classification on UTF-16 units.
    let space = |unit: &u16| char::from_u32(u32::from(*unit)).is_some_and(char::is_whitespace);
    let (start, end) = if right {
        // CE3 scans to the first terminator, not necessarily m_nDataLength.
        // If no visible suffix was trimmed, embedded-NUL data stays intact.
        let visible = text.iter().position(|&v| v == 0).unwrap_or(text.len());
        let suffix = text[..visible]
            .iter()
            .rev()
            .take_while(|v| space(v))
            .count();
        (
            0,
            if suffix == 0 {
                text.len()
            } else {
                visible - suffix
            },
        )
    } else {
        (text.iter().take_while(|v| space(v)).count(), text.len())
    };
    // Native CopyBeforeWrite runs even for an unchanged result. Detach only
    // shared storage; a unique unchanged string needs no writes at all.
    let pointer = word(ctx, object)?;
    if word(ctx, pointer - 12)? as i32 > 1 {
        reserve(ctx, object, text.len() as u32)?;
    }
    if start != 0 || end != text.len() {
        assign_units(ctx, object, &text[start..end])?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn trim_right(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    trim(ctx, true)
}
fn trim_left(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    trim(ctx, false)
}

fn copy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, other) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    let p = word(ctx, other)?;
    ctx.cpu.check_guest_access(o, 4, pocket_cpu::Prot::WRITE)?;
    let header = string_header(p)?;
    let refs = word(ctx, header)? as i32;
    if refs < 0 {
        let text = value(ctx, other)?;
        new_string(ctx, o, &text)?;
    } else {
        let refs = refs
            .checked_add(1)
            .ok_or_else(|| KernelError::Loader("MFC string reference overflow".into()))?;
        put(ctx, header, refs as u32)?;
        put(ctx, o, p)?;
    }
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn assign_wide(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, p) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    assign_pointer(ctx, o, p)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
pub(super) fn validate_assignment_target(
    ctx: &mut CallCtx<'_>,
    object: u32,
) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let pointer = word(ctx, object)?;
    let header = string_header(pointer)?;
    ctx.cpu
        .check_guest_access(header, 12, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let length = word(ctx, header + 4)?;
    let capacity = word(ctx, header + 8)?;
    if length > capacity || capacity > 0x100000 {
        return Err(KernelError::Loader("MFC string length invalid".into()));
    }
    Ok(())
}
pub(super) fn assign_units(
    ctx: &mut CallCtx<'_>,
    object: u32,
    text: &[u16],
) -> Result<(), KernelError> {
    validate_assignment_target(ctx, object)?;
    let pointer = reserve(ctx, object, text.len() as u32)?;
    let count = (text.len() as u32 + 1) * 2;
    ctx.cpu
        .check_guest_access(pointer, count, pocket_cpu::Prot::WRITE)?;
    let bytes: Vec<_> = text
        .iter()
        .copied()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    ctx.cpu.write_mem(pointer, &bytes)?;
    put(ctx, pointer - 8, text.len() as u32)
}
pub(super) fn assign_pointer(
    ctx: &mut CallCtx<'_>,
    object: u32,
    source: u32,
) -> Result<(), KernelError> {
    if word(ctx, object)? == source {
        return Ok(());
    }
    // Capture interior aliases before the native-style in-place assignment.
    // Keeping an unshared, sufficiently large buffer alive also keeps raw
    // CREATESTRUCT title pointers valid during CFrameWnd::Create.
    let text = wide(ctx, source)?;
    assign_units(ctx, object, &text)
}
enum StringOperand {
    Counted,
    Terminated,
}
fn concatenate_parts(
    ctx: &mut CallCtx<'_>,
    left_kind: StringOperand,
    right_kind: StringOperand,
) -> Result<DispatchOutcome, KernelError> {
    let (out, left, right) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let read = |ctx: &mut CallCtx<'_>, p, kind| match kind {
        StringOperand::Counted => value(ctx, p),
        StringOperand::Terminated => wide(ctx, p),
    };
    // Read both operands before constructing the ABI's hidden result object.
    // Counted CString data can include embedded NULs; raw prefixes cannot.
    let mut text = read(ctx, left, left_kind)?;
    text.extend(read(ctx, right, right_kind)?);
    new_string(ctx, out, &text)?;
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn concatenate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    concatenate_parts(ctx, StringOperand::Counted, StringOperand::Terminated)
}
fn concatenate_prefix(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    concatenate_parts(ctx, StringOperand::Terminated, StringOperand::Counted)
}

fn append_character(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let character = ctx.arg_u32(1)? as u16;
    let mut text = value(ctx, object)?;
    text.push(character);
    replace(ctx, object, &text)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn append_wide(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, p) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    let mut text = value(ctx, o)?;
    text.extend(wide(ctx, p)?);
    replace(ctx, o, &text)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn format(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    format_template(ctx, false)
}
fn format_resource(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    format_template(ctx, true)
}
fn format_template(ctx: &mut CallCtx<'_>, resource: bool) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let source = ctx.arg_u32(1)?;
    let units = if resource {
        super::resources::string(ctx, source)?
    } else {
        wide(ctx, source)?
    };
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    let fmt = String::from_utf16_lossy(&units[..end]);
    let text: Vec<_> = crate::coredll::render_printf(ctx, &fmt, true, 2)?
        .encode_utf16()
        .collect();
    replace(ctx, o, &text)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
pub(super) fn reserve(ctx: &mut CallCtx<'_>, o: u32, minimum: u32) -> Result<u32, KernelError> {
    if minimum > 0x100000 {
        return Err(KernelError::Loader("MFC buffer too large".into()));
    }
    ctx.cpu.check_guest_access(o, 4, pocket_cpu::Prot::WRITE)?;
    let old = word(ctx, o)?;
    let header = string_header(old)?;
    let refs = word(ctx, header)? as i32;
    let len = word(ctx, old - 8)?;
    let capacity = word(ctx, old - 4)?;
    if len > capacity || capacity > 0x100000 {
        return Err(KernelError::Loader("MFC buffer metadata invalid".into()));
    }
    if refs <= 1 && capacity >= minimum {
        return Ok(old);
    }
    let cap = minimum.max(len);
    let p = alloc(ctx, 12 + (cap + 1) * 2)?;
    let result = (|| {
        let bytes = ctx.cpu.read_mem(old, (len + 1) * 2)?;
        ctx.cpu.write_mem(p + 12, &bytes)?;
        put(ctx, p, 1)?;
        put(ctx, p + 4, len)?;
        put(ctx, p + 8, cap)?;
        release_string(ctx, o)?;
        put(ctx, o, p + 12)
    })();
    if let Err(e) = result {
        ctx.kernel.heap.free(p);
        return Err(e);
    }
    Ok(p + 12)
}
fn get_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let n = ctx.arg_u32(1)?;
    Ok(DispatchOutcome::ReturnedR0(reserve(ctx, o, n)?))
}
fn get_buffer_set_length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let n = ctx.arg_u32(1)?;
    let p = reserve(ctx, o, n)?;
    put(ctx, p - 8, n)?;
    ctx.cpu.write_mem(p + n * 2, &[0, 0])?;
    Ok(DispatchOutcome::ReturnedR0(p))
}
pub(super) fn release_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let requested = ctx.arg_u32(1)?;
    let old = word(ctx, o)?;
    let header = string_header(old)?;
    let cap = word(ctx, header + 8)?;
    let p = reserve(ctx, o, cap)?;
    let n = if requested == u32::MAX {
        if cap > 0x100000 {
            return Err(KernelError::Loader("MFC string capacity invalid".into()));
        }
        let bytes = ctx.cpu.read_mem(p, (cap + 1) * 2)?;
        bytes
            .chunks_exact(2)
            .position(|c| c == [0, 0])
            .ok_or_else(|| KernelError::Loader("MFC ReleaseBuffer unterminated".into()))?
            as u32
    } else {
        requested
    };
    if n > cap {
        return Err(KernelError::Loader(
            "MFC ReleaseBuffer exceeds capacity".into(),
        ));
    }
    put(ctx, p - 8, n)?;
    ctx.cpu.write_mem(p + n * 2, &[0, 0])?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{Cpu, Prot};
    #[test]
    fn shared_cstring_buffer_detaches_and_remains_terminated() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let initial = kernel.heap.free_bytes();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:181",
            &[0x2000],
        );
        cpu.write_mem(0x2100, &[b'a', 0, b'b', 0, 0, 0]).unwrap();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:324",
            &[0x2000, 0x2100],
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:175",
            &[0x2010, 0x2000],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        assert_eq!(cpu.read_u32_le(0x2010).unwrap(), original);
        let result = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:1229",
            &[0x2010, 8],
        );
        let DispatchOutcome::ReturnedR0(buffer) = result else {
            panic!("unexpected outcome")
        };
        assert_ne!(buffer, original);
        cpu.write_mem(buffer, &[b'c', 0, 0, 0]).unwrap();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:2167",
            &[0x2010, u32::MAX],
        );
        assert_eq!(cpu.read_mem(original, 6).unwrap(), [b'a', 0, b'b', 0, 0, 0]);
        assert_eq!(cpu.read_u32_le(buffer - 8).unwrap(), 1);
        for object in [0x2000, 0x2010] {
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "mfcce300.dll",
                "ord:297",
                &[object],
            );
        }
        assert_eq!(kernel.heap.free_bytes(), initial);
    }
}

fn construct_wide(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    let text = if pointer != 0 && pointer <= 0xffff {
        let module = super::application::state(ctx)?;
        let resource = word(ctx, module + 12)?;
        crate::coredll::resource_string(ctx, resource, pointer)
    } else {
        wide(ctx, pointer)?
    };
    new_string(ctx, object, &text)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn construct_wide_len(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    let length = ctx.arg_u32(2)?;
    if length > 0x100000 {
        return Err(KernelError::Loader("CString length invalid".into()));
    }
    let bytes = ctx.cpu.read_mem(pointer, length * 2)?;
    let text: Vec<_> = bytes
        .chunks_exact(2)
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect();
    new_string(ctx, object, &text)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}

#[cfg(test)]
mod constructor_tests {
    use super::super::tests::framework_setup as setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn bounded_wide_constructor_preserves_embedded_nul_and_owns_its_copy() {
        let (mut cpu, mut k, mut d) = setup();
        let baseline = k.heap.free_bytes();
        cpu.write_mem(0x1000, &[b'a', 0, 0, 0, b'b', 0]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1000, 3],
        );
        let p = cpu.read_u32_le(0x2000).unwrap();
        cpu.write_mem(0x1000, &[0; 6]).unwrap();
        assert_eq!(cpu.read_u32_le(p - 8).unwrap(), 3);
        assert_eq!(cpu.read_mem(p, 8).unwrap(), [b'a', 0, 0, 0, b'b', 0, 0, 0]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

fn append_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let other = ctx.arg_u32(1)?;
    let mut text = value(ctx, object)?;
    text.extend(value(ctx, other)?);
    replace(ctx, object, &text)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}

#[cfg(test)]
mod resource_constructor_tests {
    use super::super::tests::framework_setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn wide_resource_id_constructor_reuses_the_win32_string_table_and_self_append_is_safe() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let words = [0u16, 3, b'O' as u16, b'n' as u16, b'e' as u16];
        let bytes: Vec<_> = words.into_iter().flat_map(u16::to_le_bytes).collect();
        cpu.write_mem(0x1000, &bytes).unwrap();
        k.resources.push(pocket_pe::ResourceEntry {
            ty: pocket_pe::ResourceKey::Id(6),
            name: pocket_pe::ResourceKey::Id(1),
            data_rva: 0x1000,
            native_leaves: vec![],
            size: bytes.len() as u32,
            codepage: 0,
        });
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:179",
            &[0x2000, 1],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:364",
            &[0x2000, 0x2000],
        );
        let p = cpu.read_u32_le(0x2000).unwrap();
        let expected: Vec<_> = "OneOne"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(cpu.read_u32_le(p - 8).unwrap(), 6);
        assert_eq!(cpu.read_mem(p, 14).unwrap(), expected);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2000],
        );
    }
}

fn make_case(ctx: &mut CallCtx<'_>, upper: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let p = reserve(ctx, object, 0)?;
    crate::coredll::wide_case_string(ctx, p, upper)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn make_upper(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    make_case(ctx, true)
}
fn make_lower(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    make_case(ctx, false)
}

#[cfg(test)]
mod case_tests {
    use super::super::tests::framework_setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn case_conversion_detaches_shared_storage_and_preserves_utf16_capacity() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let baseline = k.heap.free_bytes();
        let input: Vec<_> = "Abéß"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &input).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1000, 4],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:175",
            &[0x2010, 0x2000],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1753",
            &[0x2010],
        );
        let changed = cpu.read_u32_le(0x2010).unwrap();
        assert_ne!(original, changed);
        assert_eq!(cpu.read_mem(original, input.len() as u32).unwrap(), input);
        let upper: Vec<_> = "ABÉß"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(cpu.read_mem(changed, upper.len() as u32).unwrap(), upper);
        assert_eq!(cpu.read_u32_le(changed - 8).unwrap(), 4);
        assert_eq!(cpu.read_u32_le(changed - 4).unwrap(), 4);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1751",
            &[0x2010],
        );
        let lower: Vec<_> = "abéß"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(cpu.read_mem(changed, lower.len() as u32).unwrap(), lower);
        for p in [0x2000, 0x2010] {
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:297", &[p]);
        }
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

fn mid(ctx: &mut CallCtx<'_>, has_count: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let start = (ctx.arg_u32(2)? as i32).max(0) as usize;
    let source = value(ctx, object)?;
    let start = start.min(source.len());
    let count = if has_count {
        (ctx.arg_u32(3)? as i32).max(0) as usize
    } else {
        source.len() - start
    };
    let count = count.min(source.len() - start);
    slice_result(ctx, object, out, &source, start, count)
}
fn slice_result(
    ctx: &mut CallCtx<'_>,
    object: u32,
    out: u32,
    source: &[u16],
    start: usize,
    count: usize,
) -> Result<DispatchOutcome, KernelError> {
    if start == 0 && count == source.len() {
        ctx.cpu.write_arg(0, out)?;
        ctx.cpu.write_arg(1, object)?;
        return copy(ctx);
    }
    new_string(ctx, out, &source[start..start + count])?;
    Ok(DispatchOutcome::ReturnedR0(out))
}

fn mid_tail(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    mid(ctx, false)
}
fn mid_count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    mid(ctx, true)
}

#[cfg(test)]
mod substring_tests {
    use super::super::tests::framework_setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn mid_clamps_ranges_preserves_embedded_nul_and_shares_a_complete_copy() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let baseline = k.heap.free_bytes();
        let input = [65u16, 0, 66, 67];
        let bytes: Vec<_> = input.into_iter().flat_map(u16::to_le_bytes).collect();
        cpu.write_mem(0x1000, &bytes).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1000, 4],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1761",
            &[0x2000, 0x2010, 0],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        assert_eq!(cpu.read_u32_le(0x2010).unwrap(), original);
        assert_eq!(cpu.read_u32_le(original - 12).unwrap(), 2);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1762",
            &[0x2000, 0x2020, 1, 2],
        );
        let middle = cpu.read_u32_le(0x2020).unwrap();
        assert_eq!(cpu.read_u32_le(middle - 8).unwrap(), 2);
        assert_eq!(cpu.read_mem(middle, 6).unwrap(), [0, 0, 66, 0, 0, 0]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1762",
            &[0x2000, 0x2030, u32::MAX, 2],
        );
        let clamped = cpu.read_u32_le(0x2030).unwrap();
        assert_eq!(cpu.read_mem(clamped, 6).unwrap(), [65, 0, 0, 0, 0, 0]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1761",
            &[0x2000, 0x2040, 100],
        );
        let empty = cpu.read_u32_le(0x2040).unwrap();
        assert_eq!(cpu.read_u32_le(empty - 8).unwrap(), 0);
        for p in [0x2000, 0x2010, 0x2020, 0x2030, 0x2040] {
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:297", &[p]);
        }
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

fn concatenate_strings(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    concatenate_parts(ctx, StringOperand::Counted, StringOperand::Counted)
}

fn edge(ctx: &mut CallCtx<'_>, right: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let count = (ctx.arg_u32(2)? as i32).max(0) as usize;
    let text = value(ctx, object)?;
    let count = count.min(text.len());
    slice_result(
        ctx,
        object,
        out,
        &text,
        if right { text.len() - count } else { 0 },
        count,
    )
}
fn left(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    edge(ctx, false)
}
fn right(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    edge(ctx, true)
}
fn span(ctx: &mut CallCtx<'_>, including: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let set = ctx.arg_u32(2)?;
    let set: std::collections::HashSet<_> = wide(ctx, set)?.into_iter().collect();
    let text = value(ctx, object)?;
    let count = text
        .iter()
        .take_while(|c| **c != 0 && (set.contains(*c) == including))
        .count();
    slice_result(ctx, object, out, &text, 0, count)
}
fn span_excluding(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    span(ctx, false)
}
fn span_including(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    span(ctx, true)
}
fn load_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let id = ctx.arg_u32(1)?;
    let text = super::resources::string(ctx, id)?;
    let found = !text.is_empty();
    replace(ctx, object, &text)?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(found)))
}
fn repeat_character(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let unit = ctx.arg_u32(1)? as u16;
    let count = (ctx.arg_u32(2)? as i32).max(0) as u32;
    if count > 0x100000 {
        return Err(KernelError::Loader("CString repetition too long".into()));
    }
    new_string(ctx, object, &vec![unit; count as usize])?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn assign_character(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let unit = ctx.arg_u32(1)? as u16;
    replace(ctx, object, &[unit])?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn assign_ansi(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let source = ctx.arg_u32(1)?;
    let text = crate::crt_ansi::decode(&crate::coredll::read_crt_cstr(ctx, source)?)
        .encode_utf16()
        .collect::<Vec<_>>();
    replace(ctx, object, &text)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}

#[cfg(test)]
mod expression_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    fn check(cpu: &mut pocket_cpu::stub::StubCpu, object: u32, text: &str) {
        let p = cpu.read_u32_le(object).unwrap();
        let units: Vec<_> = text.encode_utf16().collect();
        assert_eq!(cpu.read_u32_le(p - 8).unwrap(), units.len() as u32);
        let bytes: Vec<_> = units
            .into_iter()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(cpu.read_mem(p, bytes.len() as u32).unwrap(), bytes);
    }
    #[test]
    fn string_expression_chain_preserves_prefix_suffix_sharing_and_releases_every_result() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let baseline = k.heap.free_bytes();
        let bytes: Vec<_> = "ABC DEF"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &bytes).unwrap();
        cpu.write_mem(0x1100, &[32, 0, 0, 0]).unwrap();
        cpu.write_mem(0x1200, &[65, 0, 66, 0, 67, 0, 0, 0]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1000, 7],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2450",
            &[0x2000, 0x2010, 0x1100],
        );
        check(&mut cpu, 0x2010, "ABC");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2451",
            &[0x2000, 0x2020, 0x1200],
        );
        check(&mut cpu, 0x2020, "ABC");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1714",
            &[0x2000, 0x2030, 2],
        );
        check(&mut cpu, 0x2030, "AB");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2233",
            &[0x2000, 0x2040, 2],
        );
        check(&mut cpu, 0x2040, "EF");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:359",
            &[0x2050, 0x2030, 0x2040],
        );
        check(&mut cpu, 0x2050, "ABEF");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1714",
            &[0x2000, 0x2060, 100],
        );
        assert_eq!(
            cpu.read_u32_le(0x2060).unwrap(),
            cpu.read_u32_le(0x2000).unwrap()
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:176",
            &[0x2070, 120, 3],
        );
        check(&mut cpu, 0x2070, "xxx");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:322",
            &[0x2070, 0],
        );
        let p = cpu.read_u32_le(0x2070).unwrap();
        assert_eq!(cpu.read_u32_le(p - 8).unwrap(), 1);
        assert_eq!(cpu.read_mem(p, 4).unwrap(), [0; 4]);
        cpu.write_mem(0x1300, b"ansi\0").unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:323",
            &[0x2070, 0x1300],
        );
        check(&mut cpu, 0x2070, "ansi");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:365",
            &[0x2000, 0],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:365",
            &[0x2000, 33],
        );
        check(&mut cpu, 0x2000, "ABC DEF\0!");
        check(&mut cpu, 0x2060, "ABC DEF");
        for object in (0x2000..=0x2070).step_by(16) {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:297",
                &[object],
            );
        }
        assert_eq!(k.heap.free_bytes(), baseline);
    }
    #[test]
    fn load_string_reports_real_resource_presence_and_replaces_owned_text() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:644", &[]);
        let baseline = k.heap.free_bytes();
        cpu.write_mem(0x1000, &[0, 0, 2, 0, 85, 0, 73, 0]).unwrap();
        k.resources.push(pocket_pe::ResourceEntry {
            ty: pocket_pe::ResourceKey::Id(6),
            name: pocket_pe::ResourceKey::Id(2),
            data_rva: 0x1000,
            native_leaves: vec![],
            size: 8,
            codepage: 0,
        });
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:181",
            &[0x2000],
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1724",
                &[0x2000, 17]
            )),
            1
        );
        check(&mut cpu, 0x2000, "UI");
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1724",
                &[0x2000, 999]
            )),
            0
        );
        check(&mut cpu, 0x2000, "");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

pub(super) fn empty(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let pointer = word(ctx, object)?;
    if word(ctx, pointer - 8)? != 0 {
        if (word(ctx, pointer - 12)? as i32) < 0 {
            assign_pointer(ctx, object, 0)?;
        } else {
            replace(ctx, object, &[])?;
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn find_character(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    find(ctx, false, 0)
}
fn find_character_from(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let start = ctx.arg_u32(2)?;
    find(ctx, false, start)
}
fn find_text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    find(ctx, true, 0)
}
fn find_text_from(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let start = ctx.arg_u32(2)?;
    find(ctx, true, start)
}
fn find(
    ctx: &mut CallCtx<'_>,
    substring: bool,
    start: u32,
) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let argument = ctx.arg_u32(1)?;
    let text = value(ctx, object)?;
    let start = start as usize;
    if start > text.len() || (!substring && start == text.len()) {
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    }
    // Native wcschr/wcsstr begin at the supplied offset; an earlier embedded
    // NUL does not affect a search explicitly starting beyond it.
    let suffix = &text[start..];
    let end = suffix.iter().position(|&c| c == 0).unwrap_or(suffix.len());
    let found = if substring {
        let needle = wide(ctx, argument)?;
        if needle.is_empty() {
            Some(0)
        } else {
            suffix[..end]
                .windows(needle.len())
                .position(|s| s == needle)
        }
    } else if argument as u16 == 0 {
        Some(end)
    } else {
        suffix[..end].iter().position(|&c| c == argument as u16)
    };
    Ok(DispatchOutcome::ReturnedR0(
        found.map_or(u32::MAX, |i| (start + i) as u32),
    ))
}

fn reverse_find(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let character = ctx.arg_u32(1)? as u16;
    let text = value(ctx, object)?;
    // CE3 delegates to wcsrchr: an embedded NUL ends the searched prefix,
    // and searching for NUL finds that terminator rather than a later cell.
    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    let result = if character == 0 {
        end as u32
    } else {
        text[..end]
            .iter()
            .rposition(|&c| c == character)
            .map_or(u32::MAX, |index| index as u32)
    };
    Ok(DispatchOutcome::ReturnedR0(result))
}
#[cfg(test)]
mod reverse_find_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn reverse_find_obeys_ce_wcsrchr_termination_and_does_not_mutate_the_string() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x1000, &[65, 0, 66, 0, 65, 0, 0, 0, 65, 0])
            .unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1000, 5],
        );
        let storage = cpu.read_u32_le(0x2000).unwrap();
        let available = k.heap.free_bytes();
        for (character, expected) in [(65, 2), (66, 1), (67, u32::MAX), (0, 3)] {
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:2231",
                    &[0x2000, character]
                )),
                expected
            );
        }
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(), storage);
        assert_eq!(k.heap.free_bytes(), available);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2000],
        );
    }
}

fn lock_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let pointer = reserve(ctx, object, 0)?;
    put(ctx, string_header(pointer)?, u32::MAX)?;
    Ok(DispatchOutcome::ReturnedR0(pointer))
}
fn unlock_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let pointer = word(ctx, object)?;
    // Every HLE empty string owns its allocation; there is no borrowed native
    // empty-string singleton to exclude. Unlock does not recalculate length.
    put(ctx, string_header(pointer)?, 1)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
#[cfg(test)]
mod locked_buffer_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn locking_detaches_shared_data_and_copies_from_locked_storage_remain_independent() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let available = k.heap.free_bytes();
        cpu.write_mem(0x1000, &[65, 0, 66, 0, 0, 0]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1000, 2],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:175",
            &[0x2100, 0x2000],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        let locked = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1733",
            &[0x2100],
        ));
        assert_ne!(original, locked);
        assert_eq!(cpu.read_u32_le(locked - 12).unwrap(), u32::MAX);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:175",
            &[0x2200, 0x2100],
        );
        assert_ne!(cpu.read_u32_le(0x2200).unwrap(), locked);
        cpu.write_mem(locked, &[67, 0]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2485",
            &[0x2100],
        );
        assert_eq!(cpu.read_u32_le(locked - 12).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(locked - 8).unwrap(), 2);
        assert_eq!(cpu.read_mem(original, 4).unwrap(), [65, 0, 66, 0]);
        let copied = cpu.read_u32_le(0x2200).unwrap();
        assert_eq!(cpu.read_mem(copied, 4).unwrap(), [65, 0, 66, 0]);
        for object in [0x2000, 0x2100, 0x2200] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:297",
                &[object],
            );
        }
        assert_eq!(k.heap.free_bytes(), available);
    }
}

#[cfg(test)]
mod pointer_assignment_tests {
    use super::super::tests::framework_setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn raw_assignment_preserves_self_and_interior_aliases_and_detaches_shared_buffers() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x1000, &[65, 0, 66, 0, 67, 0, 68, 0, 0, 0])
            .unwrap();
        let baseline = k.heap.free_bytes();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1000, 4],
        );
        let pointer = cpu.read_u32_le(0x2000).unwrap();
        for source in [pointer, pointer + 2] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:324",
                &[0x2000, source],
            );
            assert_eq!(cpu.read_u32_le(0x2000).unwrap(), pointer);
        }
        assert_eq!(
            cpu.read_mem(pointer, 8).unwrap(),
            [66, 0, 67, 0, 68, 0, 0, 0]
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:175",
            &[0x2010, 0x2000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:324",
            &[0x2000, pointer + 2],
        );
        assert_ne!(cpu.read_u32_le(0x2000).unwrap(), pointer);
        assert_eq!(
            cpu.read_mem(pointer, 8).unwrap(),
            [66, 0, 67, 0, 68, 0, 0, 0]
        );
        for object in [0x2000, 0x2010] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:297",
                &[object],
            );
        }
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

#[cfg(test)]
mod find_tests {
    use super::super::tests::framework_setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    use pocket_kernel::DispatchOutcome;
    #[test]
    fn empty_detaches_shared_strings_but_preserves_locked_buffer_storage() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x1800, &[65, 0, 66, 0, 0, 0]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1800, 2],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:175",
            &[0x2200, 0x2000],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1120",
            &[0x2200],
        );
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(), original);
        assert_eq!(cpu.read_u32_le(original - 8).unwrap(), 2);
        let DispatchOutcome::ReturnedR0(locked) = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1733",
            &[0x2000],
        ) else {
            panic!("no buffer")
        };
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1120",
            &[0x2000],
        );
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(), locked);
        assert_eq!(cpu.read_u32_le(locked - 8).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(locked - 12).unwrap(), u32::MAX);
        assert_eq!(cpu.read_u16_le(locked).unwrap(), 0);
    }
    #[test]
    fn searches_match_ce_offsets_empty_needles_and_embedded_nuls() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let bytes: Vec<u8> = [65u16, 66, 0, 65, 67, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1800, &bytes).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1800, 5],
        );
        let needle: Vec<u8> = [65u16, 67, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1900, &needle).unwrap();
        for (name, arg, start, expected) in [
            ("ord:1156", 65u32, 99u32, 0u32),
            ("ord:1157", 65, 1, u32::MAX),
            ("ord:1157", 65, 3, 3),
            ("ord:1157", 0, 0, 2),
            ("ord:1157", 0, 5, u32::MAX),
            ("ord:1158", 0x1900, 99, u32::MAX),
            ("ord:1159", 0x1900, 3, 3),
            ("ord:1159", 0x1904, 5, 5),
            ("ord:1159", 0x1904, 6, u32::MAX),
        ] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    name,
                    &[0x2000, arg, start]
                ),
                DispatchOutcome::ReturnedR0(expected)
            );
        }
    }
}

#[cfg(test)]
mod trim_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    fn text(cpu: &mut dyn Cpu, s: &str) -> Vec<u16> {
        let units: Vec<_> = s.encode_utf16().collect();
        let bytes: Vec<_> = units
            .iter()
            .copied()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1800, &bytes).unwrap();
        units
    }
    #[test]
    fn trim_whitespace_uses_real_lengths_embedded_terminators_and_releases_all_storage() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let baseline = k.heap.free_bytes();
        for (ordinal, input, expected) in [
            (2473, " \tname\r\n", " \tname"),
            (2470, " \tname\r\n", "name\r\n"),
            (2473, "\t\r\n\u{b}\u{c} ", ""),
            (2470, "\t\r\n\u{b}\u{c} ", ""),
            (2473, "a  \0tail ", "a"),
            (2473, "a\0tail ", "a\0tail "),
            (2470, "  a\0tail ", "a\0tail "),
            (2473, "", ""),
            (2470, "", ""),
            (2473, "é漢\u{2003}", "é漢"),
            (2470, "\u{2003}é漢", "é漢"),
        ] {
            let input = text(&mut cpu, input);
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:180",
                &[0x2000, 0x1800, input.len() as u32],
            );
            let before = cpu.read_u32_le(0x2000).unwrap();
            let capacity = cpu.read_u32_le(before - 4).unwrap();
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    &format!("ord:{ordinal}"),
                    &[0x2000]
                )),
                0
            );
            let ptr = cpu.read_u32_le(0x2000).unwrap();
            let expected: Vec<_> = expected.encode_utf16().collect();
            assert_eq!(cpu.read_u32_le(ptr - 8).unwrap(), expected.len() as u32);
            assert_eq!(cpu.read_u32_le(ptr - 4).unwrap(), capacity);
            assert_eq!(
                cpu.read_mem(ptr, (expected.len() as u32 + 1) * 2).unwrap(),
                expected
                    .into_iter()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:297",
                &[0x2000],
            );
        }
        assert_eq!(k.heap.free_bytes(), baseline);
    }
    #[test]
    fn unchanged_unique_trim_does_not_write_a_protected_buffer() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.map_region(0x8000, 0x1000, pocket_cpu::Prot::READ)
            .unwrap();
        for (p, v) in [(0x8000, 1u32), (0x8004, 4), (0x8008, 4), (0x2000, 0x800c)] {
            cpu.write_mem(p, &v.to_le_bytes()).unwrap();
        }
        let bytes: Vec<_> = "name\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
        cpu.write_mem(0x800c, &bytes).unwrap();
        let free = k.heap.free_bytes();
        for ordinal in [2470, 2473] {
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    &format!("ord:{ordinal}"),
                    &[0x2000]
                )),
                0
            );
        }
        assert_eq!(cpu.read_mem(0x800c, 10).unwrap(), bytes);
        assert_eq!(k.heap.free_bytes(), free);
    }
    #[test]
    fn trim_detaches_shared_data_even_without_changes_and_preserves_locked_buffer_identity() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let baseline = k.heap.free_bytes();
        for source in ["name", "name "] {
            let original = text(&mut cpu, source);
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:180",
                &[0x2000, 0x1800, original.len() as u32],
            );
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:175",
                &[0x2100, 0x2000],
            );
            let shared = cpu.read_u32_le(0x2100).unwrap();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2473",
                &[0x2000],
            );
            assert_ne!(cpu.read_u32_le(0x2000).unwrap(), shared);
            assert_eq!(cpu.read_u32_le(shared - 8).unwrap(), original.len() as u32);
            assert_eq!(cpu.read_u32_le(shared - 12).unwrap(), 1);
            for object in [0x2000, 0x2100] {
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:297",
                    &[object],
                );
            }
        }
        let original = text(&mut cpu, "  name  ");
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2000, 0x1800, original.len() as u32],
        );
        let locked = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1733",
            &[0x2000],
        ));
        for ordinal in [2470, 2473] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                &format!("ord:{ordinal}"),
                &[0x2000],
            );
            assert_eq!(cpu.read_u32_le(0x2000).unwrap(), locked);
            assert_eq!(cpu.read_u32_le(locked - 12).unwrap(), u32::MAX);
            assert_eq!(cpu.read_u32_le(locked - 4).unwrap(), original.len() as u32);
        }
        assert_eq!(cpu.read_u32_le(locked - 8).unwrap(), 4);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

#[cfg(test)]
mod prefix_expression_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{Cpu, Prot};
    #[test]
    fn raw_prefix_plus_counted_string_preserves_embedded_data_and_source_ownership() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let free = k.heap.free_bytes();
        cpu.write_mem(0x1800, &[120, 0, 0, 0, 121, 0, 0, 0])
            .unwrap();
        let prefix: Vec<_> = "é:\0ignored"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1900, &prefix).unwrap();
        for locked in [false, true] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:180",
                &[0x2000, 0x1800, 3],
            );
            if locked {
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:1733",
                    &[0x2000],
                );
            }
            let source = cpu.read_u32_le(0x2000).unwrap();
            let refs = cpu.read_u32_le(source - 12).unwrap();
            for (raw, expected) in [(0x1900, "é:x\0y"), (0, "x\0y"), (source, "xx\0y")] {
                assert_eq!(
                    result(call(
                        &mut cpu,
                        &mut k,
                        &mut d,
                        "mfcce300.dll",
                        "ord:363",
                        &[0x2100, raw, 0x2000]
                    )),
                    0x2100
                );
                let out = cpu.read_u32_le(0x2100).unwrap();
                assert_ne!(out, source);
                let units: Vec<_> = expected.encode_utf16().collect();
                assert_eq!(cpu.read_u32_le(out - 8).unwrap(), units.len() as u32);
                let bytes: Vec<_> = units
                    .into_iter()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                assert_eq!(cpu.read_mem(out, bytes.len() as u32).unwrap(), bytes);
                assert_eq!(cpu.read_u32_le(source - 12).unwrap(), refs);
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:297",
                    &[0x2100],
                );
            }
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:297",
                &[0x2000],
            );
        }
        assert_eq!(k.heap.free_bytes(), free);
    }
    #[test]
    fn protected_concatenation_result_fails_without_releasing_or_leaking_source_storage() {
        use pocket_kernel::Dispatcher;
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:181",
            &[0x2000],
        );
        cpu.map_region(0x8000, 0x1000, Prot::READ).unwrap();
        let free = k.heap.free_bytes();
        let source = cpu.read_u32_le(0x2000).unwrap();
        for (i, v) in [0x8000, 0, 0x2000].into_iter().enumerate() {
            cpu.write_arg(i as u8, v).unwrap();
        }
        assert!(d
            .dispatch(
                &mut cpu,
                &super::super::window_dispatch::tests::entry(363),
                &mut k
            )
            .is_err());
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(), source);
        assert_eq!(cpu.read_u32_le(source - 12).unwrap(), 1);
        assert_eq!(k.heap.free_bytes(), free);
    }
}
