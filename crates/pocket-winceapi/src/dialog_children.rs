//! Custom dialog classes use ordinary USER creation before WM_INITDIALOG.
//! Continuations and their one scratch allocation are guest-owned, so nested
//! dialogs, nonlocal exits and snapshots share the existing callback owner.
use super::{dialog_units, read_dialog_template};
use crate::dlgtemplate::{DialogItem, ItemClass};
use crate::guest_callbacks::{self, Frame, WINCE};
use crate::CallCtx;
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{DispatchOutcome, KernelError};
pub(crate) const INIT: &str = "__PocketHLE_DialogCustomInit";
fn custom(item: &DialogItem) -> Option<&str> {
    match &item.class {
        ItemClass::Named(name)
            if pocket_kernel::controls::ControlClass::from_class_name(name).is_none() =>
        {
            Some(name)
        }
        _ => None,
    }
}
pub(crate) fn has_custom(ctx: &mut CallCtx<'_>, template: u32) -> bool {
    read_dialog_template(ctx, template).is_some_and(|t| {
        t.items.iter().filter_map(custom).any(|name| {
            ctx.kernel
                .window_class_procs
                .get(name)
                .is_some_and(|p| *p != 0)
        })
    })
}
pub(crate) fn begin(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    proc: u32,
    template: u32,
    param: u32,
) -> Result<DispatchOutcome, KernelError> {
    let target = guest_callbacks::service(ctx, INIT)?;
    for (i, arg) in [hwnd, proc, template, param].into_iter().enumerate() {
        ctx.cpu.write_arg(i as u8, arg)?;
    }
    Ok(DispatchOutcome::JumpTo(target))
}
pub(crate) fn initialize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let owner = guest_callbacks::owner(ctx, WINCE.continue_name)?;
    let f = Frame::enter(ctx, WINCE, owner)?;
    let hwnd = f.arg(ctx, 0)?;
    let phase = if f.fresh { 0 } else { f.phase(ctx)? };
    if phase == 2 {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    if phase > 4 {
        return Err(KernelError::Dispatch(
            "Invalid dialog child continuation".into(),
        ));
    }
    let template = f.arg(ctx, 2)?;
    let tmpl = read_dialog_template(ctx, template)
        .ok_or_else(|| KernelError::Dispatch("Custom dialog template is unreadable".into()))?;
    if f.fresh {
        let instance = ctx
            .kernel
            .dialog_frame
            .filter(|_| ctx.kernel.dialog_frame_hwnd == hwnd)
            .map(|frame| frame.args[0])
            .or_else(|| {
                ctx.kernel
                    .modal_dialog
                    .as_ref()
                    .filter(|d| d.hwnd == hwnd)
                    .map(|d| d.frame.args[0])
            })
            .unwrap_or(0);
        f.set(ctx, 2, instance)?;
        let (font, dlu) = dialog_units(ctx, &tmpl);
        f.set(ctx, 5, font.unwrap_or(0))?;
        f.set(ctx, 3, dlu.base_x as u32)?;
        f.set(ctx, 4, dlu.base_y as u32)?;
        let bytes = tmpl
            .items
            .iter()
            .filter(|item| custom(item).is_some())
            .map(|item| {
                (custom(item).unwrap().encode_utf16().count()
                    + 1
                    + item.title.encode_utf16().count()
                    + 1)
                    * 2
                    + item.creation_data.len()
            })
            .max()
            .unwrap_or(0)
            .max(4);
        let p = match super::do_alloc(ctx, bytes as u32)? {
            DispatchOutcome::ReturnedR0(p) if p != 0 => p,
            _ => {
                return Err(KernelError::Dispatch(
                    "Dialog child allocation failed".into(),
                ))
            }
        };
        f.own(ctx, p)?;
        f.set(ctx, 1, p)?;
        let proc = f.arg(ctx, 1)?;
        if let Some(prepare) = crate::mfc::dialog_preparation(ctx, proc) {
            let object = f.arg(ctx, 3)?;
            return f.call_args(ctx, 4, prepare, &[hwnd, object]);
        }
    } else if phase == 1 {
        let child = ctx.cpu.read_reg(ArmReg::R0)?;
        if child == 0 {
            return Err(KernelError::Dispatch(
                "Custom dialog control creation failed".into(),
            ));
        }
        let font = f.get(ctx, 5)?;
        if font != 0 {
            return f.call_service(ctx, 3, "SendMessageW", &[child, 0x30, font, 0]);
        }
    }
    let first = f.get(ctx, 0)? as usize;
    for (index, item) in tmpl.items.iter().enumerate().skip(first) {
        let Some(name) = custom(item).filter(|name| {
            ctx.kernel
                .window_class_procs
                .get(*name)
                .is_some_and(|p| *p != 0)
        }) else {
            continue;
        };
        let scratch = f.get(ctx, 1)?;
        let class: Vec<_> = name
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let title: Vec<_> = item
            .title
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(scratch, &class)?;
        let title_at = scratch + class.len() as u32;
        ctx.cpu.write_mem(title_at, &title)?;
        let data_at = title_at + title.len() as u32;
        ctx.cpu.write_mem(data_at, &item.creation_data)?;
        let data_at = if item.creation_data.is_empty() {
            0
        } else {
            data_at
        };
        let dlu = crate::dlgtemplate::Dlu {
            base_x: f.get(ctx, 3)? as i32,
            base_y: f.get(ctx, 4)? as i32,
        };
        let (x, y, w, h) = dlu.to_px(item.x.into(), item.y.into(), item.cx.into(), item.cy.into());
        let instance = f.get(ctx, 2)?;
        f.set(ctx, 0, index as u32 + 1)?;
        return f.call_service(
            ctx,
            1,
            "CreateWindowExW",
            &[
                item.ex_style,
                scratch,
                title_at,
                item.style | 0x40000000,
                x as u32,
                y as u32,
                w as u32,
                h as u32,
                hwnd,
                item.id.into(),
                instance,
                data_at,
            ],
        );
    }
    let proc = f.arg(ctx, 1)?;
    if proc == 0 {
        return f.finish(ctx, 0);
    }
    let param = f.arg(ctx, 3)?;
    f.call_args(ctx, 2, proc, &[hwnd, 0x110, 0, param])
}
