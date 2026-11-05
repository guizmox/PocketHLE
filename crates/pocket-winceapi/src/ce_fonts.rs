//! Process-owned TrueType resources and guest font-enumeration callbacks.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::{regs::ArmReg, Prot};
use pocket_kernel::{
    gdi::{Font, FontEnumFrame, GdiObject},
    text, DispatchOutcome, GuestCallFrame, KernelError,
};
const RECORD_SIZE: u32 = 284 + 76;
pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "AddFontResourceW", add);
    d.register_handler("coredll.dll", "RemoveFontResourceW", remove);
    d.register_handler("coredll.dll", "EnumFontFamiliesW", enumerate);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn ce_fonts_remove_one_registration_at_a_time() {
        let (mut cpu, mut k, mut d) = setup();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("font.ttf"),
            include_bytes!("../../pocket-kernel/assets/fonts/LiberationSerif-Regular.ttf"),
        )
        .unwrap();
        k.vfs.mount("/", dir.path());
        cpu.write_mem(
            0x1000,
            &"/font.ttf"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut invoke = |api| call(&mut cpu, &mut k, &mut d, "coredll.dll", api, &[0x1000]);
        for _ in 0..2 {
            assert_eq!(invoke("AddFontResourceW"), DispatchOutcome::ReturnedR0(1));
        }
        for expected in [1, 1, 0] {
            assert_eq!(
                invoke("RemoveFontResourceW"),
                DispatchOutcome::ReturnedR0(expected)
            );
        }
        assert!(k.gdi.installed_fonts.is_empty());
        assert_eq!(k.gdi.installed_font_bytes, 0);
    }

    #[test]
    fn ce_fonts_real_resource_is_selected_and_enum_stops_on_zero() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.map_region(0x6000, 0x1000, Prot::READ | Prot::EXEC)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("font.ttf"),
            include_bytes!("../../pocket-kernel/assets/fonts/LiberationSerif-Regular.ttf"),
        )
        .unwrap();
        k.vfs.mount("\\", dir.path());
        cpu.write_mem(
            0x1000,
            &"\\font.ttf"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "AddFontResourceW",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(k.vfs.open_handles().is_empty());
        let resource = &k.gdi.installed_fonts[0];
        let family = resource.family.clone();
        let mut font = Font::new(-16);
        put_text(&mut font.logfont[28..], &family);
        let handle = k.gdi.create_logfont(font);
        assert!(text::selected(&k.gdi, handle).unwrap().resource.is_some());
        cpu.write_reg(ArmReg::Lr, 0x1234).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "EnumFontFamiliesW",
                &[pocket_kernel::gdi::GDI_SCREEN_DC, 0, 0x6000, 0x9876]
            ),
            DispatchOutcome::JumpTo(0x6000)
        );
        let ptr = cpu.read_reg(ArmReg::R0).unwrap();
        assert_eq!(cpu.read_reg(ArmReg::R3).unwrap(), 0x9876);
        assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), ptr + 284);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "EnumFontFamiliesW",
                &[0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(k.gdi.font_enum_frames.is_empty());
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
        assert!(pocket_kernel::gdi::GdiState::new()
            .installed_fonts
            .is_empty());
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "EnumFontFamiliesW",
                &[pocket_kernel::gdi::GDI_SCREEN_DC, 0, 0x6000, 0]
            ),
            DispatchOutcome::JumpTo(0x6000)
        );
        let scratch = cpu.read_reg(ArmReg::R0).unwrap();
        assert!(cpu.read_u32_le(scratch + 348).unwrap() > 16);
        k.main_thread.exit_code = Some(0);
        k.reclaim_finished_stacks(&mut cpu).unwrap();
        assert!(k.gdi.font_enum_frames.is_empty());
        assert_eq!(k.heap.alloc(RECORD_SIZE), Some(scratch));
        k.heap.free(scratch);
        k.record_process_exit(0);
        assert!(k.gdi.installed_fonts.is_empty());
        assert!(k.gdi.font_enum_frames.is_empty());
        assert!(text::selected(&k.gdi, handle).unwrap().resource.is_none());
        assert_eq!(k.heap.alloc(RECORD_SIZE), Some(scratch));
    }
    #[test]
    fn ce_fonts_invalid_resource_does_not_mutate_table_or_leak_file() {
        let (mut cpu, mut k, mut d) = setup();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.ttf"), b"invalid").unwrap();
        k.vfs.mount("\\", dir.path());
        cpu.write_mem(
            0x1000,
            &"\\bad.ttf"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "AddFontResourceW",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(k.gdi.installed_fonts.is_empty());
        assert!(k.vfs.open_handles().is_empty());
    }
}
fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let result = (|| {
        let path = crate::ce_shortcut::text(ctx, p, 260)?;
        let size = ctx.kernel.vfs.file_information(&path)?.size;
        let source = ctx.kernel.vfs.resolve(&path).ok_or(2u32)?;
        if size == 0 || size > 8 * 1024 * 1024 {
            return Err(13);
        }
        let file = ctx.kernel.vfs.open_file(&path, 1, 1, 3, false, 0)?;
        let mut bytes = vec![0; size as usize];
        let mut offset = 0;
        while offset < bytes.len() {
            match ctx.kernel.vfs.read(file.handle, &mut bytes[offset..]) {
                Some(n) if n != 0 => offset += n,
                _ => break,
            }
        }
        ctx.kernel.vfs.close(file.handle);
        if offset != bytes.len() {
            return Err(13);
        }
        let fonts = text::parse_resources(bytes).ok_or(13u32)?;
        let count = fonts.len();
        let bytes = size as usize * count;
        if ctx.kernel.gdi.installed_font_bytes.saturating_add(bytes) > 32 * 1024 * 1024
            || ctx.kernel.gdi.installed_fonts.len() + count > 64
        {
            return Err(8);
        }
        ctx.kernel.gdi.install_fonts(source, fonts, bytes);
        Ok(count as u32)
    })();
    match result {
        Ok(n) => Ok(DispatchOutcome::ReturnedR0(n)),
        Err(e) => fail(ctx, e),
    }
}
fn remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let ptr = ctx.arg_u32(0)?;
    let path = match crate::ce_shortcut::text(ctx, ptr, 260) {
        Ok(path) => path,
        Err(error) => return fail(ctx, error),
    };
    let removed = ctx
        .kernel
        .vfs
        .resolve(&path)
        .is_some_and(|source| ctx.kernel.gdi.remove_fonts(&source));
    Ok(DispatchOutcome::ReturnedR0(u32::from(removed)))
}

fn put_text(out: &mut [u8], value: &str) {
    let limit = out.len() / 2 - 1;
    for (i, c) in value.encode_utf16().take(limit).enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
}
fn record(font: Font, full: &str, style: &str) -> Vec<u8> {
    let mut out = vec![0; RECORD_SIZE as usize];
    out[..92].copy_from_slice(&font.logfont);
    put_text(&mut out[92..220], full);
    put_text(&mut out[220..284], style);
    out[284..].copy_from_slice(&text::new_metrics_w(&font));
    out
}
fn advance(
    ctx: &mut CallCtx<'_>,
    mut state: FontEnumFrame,
    last: u32,
) -> Result<DispatchOutcome, KernelError> {
    if state.next == state.records.len() || (state.next != 0 && last == 0) {
        ctx.kernel.heap.free(state.scratch);
        return Ok(DispatchOutcome::ReturnedR0(last));
    }
    ctx.cpu
        .write_mem(state.scratch, &state.records[state.next])?;
    state.next += 1;
    ctx.cpu.write_reg(ArmReg::R0, state.scratch)?;
    ctx.cpu.write_reg(ArmReg::R1, state.scratch + 284)?;
    ctx.cpu.write_reg(ArmReg::R2, 4)?; // TRUETYPE_FONTTYPE
    ctx.cpu.write_reg(ArmReg::R3, state.parameter)?;
    ctx.cpu.write_reg(ArmReg::Lr, state.thunk)?;
    let callback = state.callback;
    ctx.kernel.gdi.font_enum_frames.push(state);
    Ok(DispatchOutcome::JumpTo(callback))
}
fn enumerate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if let Some(index) = ctx.kernel.gdi.font_enum_frames.iter().rposition(|f| {
        f.thread == ctx.kernel.current_thread && f.thunk == ctx.thunk.thunk_va && f.frame.sp == sp
    }) {
        let state = ctx.kernel.gdi.font_enum_frames.remove(index);
        let last = ctx.cpu.read_reg(ArmReg::R0)?;
        for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
            .into_iter()
            .zip(state.frame.args)
        {
            ctx.cpu.write_reg(reg, value)?;
        }
        ctx.cpu.write_reg(ArmReg::Lr, state.frame.lr)?;
        return advance(ctx, state, last);
    }
    let dc = ctx.arg_u32(0)?;
    let family = ctx.arg_u32(1)?;
    let callback = ctx.arg_u32(2)?;
    let parameter = ctx.arg_u32(3)?;
    if !matches!(ctx.kernel.gdi.get(dc), Some(GdiObject::Dc(_))) {
        return fail(ctx, 6);
    }
    if callback == 0
        || ctx
            .cpu
            .check_guest_access(callback & !1, 2, Prot::EXEC)
            .is_err()
    {
        return fail(ctx, 87);
    }
    let family = if family == 0 {
        None
    } else {
        match crate::ce_shortcut::text(ctx, family, 32) {
            Ok(n) => Some(n),
            Err(e) => return fail(ctx, e),
        }
    };
    let mut records = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for resource in &ctx.kernel.gdi.installed_fonts {
        if family
            .as_ref()
            .is_some_and(|name| !name.eq_ignore_ascii_case(&resource.family))
        {
            continue;
        }
        if family.is_none() && !seen.insert(resource.family.to_lowercase()) {
            continue;
        }
        let mut font = Font::new(-16);
        put_text(&mut font.logfont[28..], &resource.family);
        font.logfont[16..20].copy_from_slice(&u32::from(resource.weight).to_le_bytes());
        font.logfont[20] = u8::from(resource.italic);
        font.resource = Some(resource.font.clone());
        records.push(record(font, &resource.full_name, &resource.style));
    }
    for name in ["Arial", "Tahoma", "Times New Roman", "Courier New"] {
        if family
            .as_ref()
            .is_some_and(|f| !name.eq_ignore_ascii_case(f))
            || seen.contains(&name.to_lowercase())
        {
            continue;
        }
        let styles = [
            ("Regular", 400u32),
            ("Bold", 700),
            ("Italic", 400),
            ("Bold Italic", 700),
        ];
        for (index, (style, weight)) in styles.into_iter().enumerate() {
            if family.is_none() && index != 0 {
                continue;
            }
            let mut font = Font::new(-16);
            put_text(&mut font.logfont[28..], name);
            font.logfont[16..20].copy_from_slice(&weight.to_le_bytes());
            font.logfont[20] = u8::from(index >= 2);
            records.push(record(font, name, style));
        }
    }
    if records.is_empty() {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let DispatchOutcome::ReturnedR0(scratch) = crate::coredll::do_alloc(ctx, RECORD_SIZE)? else {
        unreachable!()
    };
    if scratch == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let frame = GuestCallFrame {
        args: [
            dc,
            if family.is_none() { 0 } else { ctx.arg_u32(1)? },
            callback,
            parameter,
        ],
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
        sp,
    };
    advance(
        ctx,
        FontEnumFrame {
            thread: ctx.kernel.current_thread,
            thunk: ctx.thunk.thunk_va,
            frame,
            callback,
            parameter,
            scratch,
            records,
            next: 0,
        },
        0,
    )
}
