//! CE3 CCommandLineInfo has one owned CString and two ParseParam virtual
//! overloads. The CE release parser intentionally ignores flag parameters;
//! modern desktop MFC has additional shell/printing behavior and a larger ABI.
use super::callbacks::{virtual_function, Frame};
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (72, "??0CCommandLineInfo@@QAA@XZ", construct),
    (228, "??1CCommandLineInfo@@UAA@XZ", destruct),
    (
        2075,
        "?ParseCommandLine@CWinApp@@QAAXAAVCCommandLineInfo@@@Z",
        parse_command_line,
    ),
    (2076, "?ParseLast@CCommandLineInfo@@IAAXH@Z", parse_last),
    (
        2077,
        "?ParseParam@CCommandLineInfo@@UAAXPBDHH@Z",
        parse_ansi,
    ),
    (
        2078,
        "?ParseParam@CCommandLineInfo@@UAAXPBGHH@Z",
        parse_wide,
    ),
];
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(o, 24, pocket_cpu::Prot::WRITE)?;
    let table = super::classes::vtable(ctx, "CCommandLineInfo")?;
    new_string(ctx, o + 20, &[])?;
    ctx.cpu.write_mem(o, &[0; 20])?;
    put(ctx, o, table)?;
    put(ctx, o + 4, 1)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(o, 24, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    release_string(ctx, o + 20)?;
    let table = super::classes::vtable(ctx, "CObject")?;
    put(ctx, o, table)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn last(ctx: &mut CallCtx<'_>, o: u32, final_parameter: bool) -> Result<(), KernelError> {
    if final_parameter {
        let command = word(ctx, o + 16)?;
        if (command == 0 || command == u32::MAX) && !super::strings::value(ctx, o + 20)?.is_empty()
        {
            put(ctx, o + 16, 1)?;
        }
        // This is the CE3 retail ParseLast behavior, not modern desktop MFC.
        put(ctx, o + 4, 1)?;
    }
    Ok(())
}
fn parse_last(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let final_parameter = ctx.arg_u32(1)? != 0;
    last(ctx, o, final_parameter)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn parse(ctx: &mut CallCtx<'_>, ansi: bool) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    let flag = ctx.arg_u32(2)? != 0;
    let final_parameter = ctx.arg_u32(3)? != 0;
    ctx.cpu
        .check_guest_access(o, 24, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    if !flag && super::strings::value(ctx, o + 20)?.is_empty() {
        let text = if ansi {
            crate::crt_ansi::decode(&crate::coredll::read_crt_cstr(ctx, p)?)
                .encode_utf16()
                .collect()
        } else {
            super::strings::wide(ctx, p)?
        };
        super::strings::replace(ctx, o + 20, &text)?;
    }
    last(ctx, o, final_parameter)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn parse_ansi(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    parse(ctx, true)
}
fn parse_wide(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    parse(ctx, false)
}
/// CE's parser splits spaces/tabs, with a leading quoted argument allowed to
/// contain whitespace. It does not use desktop CRT backslash escape rules.
fn arguments(text: &[u16]) -> Vec<Vec<u16>> {
    let white = |c: u16| c == 32 || c == 9;
    let mut result = Vec::new();
    let mut at = 0;
    while at < text.len() {
        while at < text.len() && white(text[at]) {
            at += 1;
        }
        if at == text.len() {
            break;
        }
        let start = at;
        let search = if text[at] == 34 {
            text[at + 1..]
                .iter()
                .position(|c| *c == 34)
                .map_or(at, |i| at + 1 + i)
        } else {
            at
        };
        let end = text[search..]
            .iter()
            .position(|c| white(*c))
            .map_or(text.len(), |i| search + i);
        let mut token = &text[start..end];
        if token.last() == Some(&34) {
            token = &token[..token.len() - 1];
        }
        if token.first() == Some(&34) {
            token = &token[1..];
        }
        result.push(token.to_vec());
        at = end;
    }
    result
}
fn packed_arguments(ctx: &mut CallCtx<'_>, args: &[Vec<u16>]) -> Result<u32, KernelError> {
    if args.is_empty() {
        return Ok(0);
    }
    let units: usize = args.iter().map(|s| s.len() + 1).sum();
    let bytes = args
        .len()
        .checked_mul(4)
        .and_then(|n| units.checked_mul(2).and_then(|t| n.checked_add(t)))
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| KernelError::Loader("MFC command line too long".into()))?;
    let p = alloc(ctx, bytes)?;
    let result = (|| {
        let mut at = p + args.len() as u32 * 4;
        for (i, s) in args.iter().enumerate() {
            put(ctx, p + i as u32 * 4, at)?;
            let data: Vec<_> = s
                .iter()
                .copied()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            ctx.cpu.write_mem(at, &data)?;
            at += data.len() as u32;
        }
        Ok::<_, KernelError>(())
    })();
    if let Err(e) = result {
        ctx.kernel.heap.free(p);
        return Err(e);
    }
    Ok(p)
}
fn parse_command_line(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if frame.fresh {
        let app = frame.arg(ctx, 0)?;
        let info = frame.arg(ctx, 1)?;
        ctx.cpu
            .check_guest_access(app, 0xb4, pocket_cpu::Prot::READ)?;
        ctx.cpu
            .check_guest_access(info, 24, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
        let p = word(ctx, app + 0x70)?;
        let text = super::strings::wide(ctx, p)?;
        let args = arguments(&text);
        let data = packed_arguments(ctx, &args)?;
        frame.set(ctx, 0, info)?;
        frame.set(ctx, 1, data)?;
        frame.set(ctx, 2, args.len() as u32)?;
        frame.set(ctx, 3, 0)?;
    } else {
        if frame.phase(ctx)? != 1 {
            return Err(KernelError::Dispatch(
                "Invalid MFC command-line continuation".into(),
            ));
        }
        let index = frame.get(ctx, 3)?;
        frame.set(ctx, 3, index + 1)?;
    }
    let index = frame.get(ctx, 3)?;
    let count = frame.get(ctx, 2)?;
    let data = frame.get(ctx, 1)?;
    if index == count {
        if data != 0 {
            ctx.kernel.heap.free(data);
        }
        return frame.finish(ctx, 0);
    }
    let info = frame.get(ctx, 0)?;
    let p = word(ctx, data + index * 4)?;
    let first = u16::from_le_bytes(ctx.cpu.read_mem(p, 2)?.try_into().unwrap());
    let flag = first == 45 || first == 47;
    let target = virtual_function(ctx, info, 6)?;
    frame.call(
        ctx,
        1,
        target,
        [
            info,
            p + if flag { 2 } else { 0 },
            u32::from(flag),
            u32::from(index + 1 == count),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    fn text(cpu: &mut pocket_cpu::stub::StubCpu, p: u32) -> Vec<u16> {
        let mut out = Vec::new();
        let mut at = p;
        loop {
            let b = cpu.read_mem(at, 2).unwrap();
            let c = u16::from_le_bytes(b.try_into().unwrap());
            if c == 0 {
                break;
            }
            out.push(c);
            at += 2;
        }
        out
    }
    #[test]
    fn quoted_arguments_flags_virtual_overrides_and_owned_filename_survive_continuations() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:72",
            &[0x2000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:228",
            &[0x2000],
        );
        let baseline = k.heap.free_bytes();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:72",
            &[0x2000],
        );
        // An application-supplied ParseParam override replaces only slot six.
        let table = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(table, 28).unwrap();
        cpu.write_mem(0x2500, &bytes).unwrap();
        cpu.write_mem(0x2518, &0x6000u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2000, &0x2500u32.to_le_bytes()).unwrap();
        let cmd: Vec<_> = " /ignored \"My Documents\\flight.dat\" second.dat"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &cmd).unwrap();
        cpu.write_mem(0x2270, &0x1000u32.to_le_bytes()).unwrap();
        let mut outcome = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2075",
            &[0x2200, 0x2000],
        );
        let mut observed = Vec::new();
        while let DispatchOutcome::JumpTo(target) = outcome {
            assert_eq!(target, 0x6000);
            let p = cpu.read_reg(ArmReg::R1).unwrap();
            observed.push((
                String::from_utf16(&text(&mut cpu, p)).unwrap(),
                cpu.read_reg(ArmReg::R2).unwrap(),
                cpu.read_reg(ArmReg::R3).unwrap(),
            ));
            // Simulate the guest override calling its real CE3 base method.
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:2078", &[]);
            super::super::callbacks::return_from_callback(&mut cpu, &mut k, &mut d);
            outcome = call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:2075", &[]);
        }
        assert_eq!(result(outcome), 0);
        assert_eq!(
            observed,
            vec![
                ("ignored".into(), 1, 0),
                ("My Documents\\flight.dat".into(), 0, 0),
                ("second.dat".into(), 0, 1)
            ]
        );
        assert_eq!(cpu.read_u32_le(0x2010).unwrap(), 1);
        let filename = cpu.read_u32_le(0x2014).unwrap();
        assert_eq!(
            String::from_utf16(&text(&mut cpu, filename)).unwrap(),
            "My Documents\\flight.dat"
        );
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert!(!k
            .dynamic_exports
            .values()
            .any(|m| m.keys().any(|n| n.starts_with("__PocketHLE_MFC_Frame:"))));
        // CCommandLineInfo has no DECLARE_DYNAMIC: GetRuntimeClass is CObject.
        let a = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_Class:CCommandLineInfo:0",
            &[0x2000],
        ));
        let b = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1501",
            &[0x2000],
        ));
        assert_eq!(a, b);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:228",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), baseline);
    }
    #[test]
    fn empty_command_line_does_not_invent_a_parameter() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:72",
            &[0x2000],
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2075",
                &[0x2200, 0x2000]
            )),
            0
        );
        assert_eq!(cpu.read_u32_le(0x2010).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 1);
    }
}
