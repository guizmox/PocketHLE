//! Drag images are presentation overlays, never saved pixels replayed over a
//! newer guest paint. Portable Explorer also probes these APIs before a drag.
use super::*;
use pocket_kernel::imaging::ImageDrag;

pub(super) fn register(d: &mut WinCeDispatcher, dll: &str) {
    for (name, handler) in [
        ("ImageList_BeginDrag", begin as crate::Handler),
        ("ImageList_EndDrag", end),
        ("ImageList_DragEnter", enter),
        ("ImageList_DragLeave", leave),
        ("ImageList_DragMove", move_image),
        ("ImageList_DragShowNolock", show),
        ("ImageList_GetDragImage", get),
    ] {
        d.register_handler(dll, name, handler);
    }
}
fn ret(v: bool) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(u32::from(v)))
}
fn begin(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (list, index) = (ctx.arg_u32(0)?, ctx.arg_u32(1)? as usize);
    let hotspot = (ctx.arg_u32(2)? as i32, ctx.arg_u32(3)? as i32);
    if ctx.kernel.gdi.image_drag.is_some() {
        return ret(false);
    }
    let Some(image) = ctx
        .kernel
        .gdi
        .image_lists
        .get(&list)
        .and_then(|l| l.image(index, 0))
        .map(|i| i.into_owned())
    else {
        return ret(false);
    };
    ctx.kernel.gdi.image_drag = Some(ImageDrag {
        image,
        hotspot,
        position: (0, 0),
        window: 0,
        entered: false,
        visible: false,
        list_handle: 0,
    });
    ret(true)
}
fn end(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if let Some(drag) = ctx.kernel.gdi.image_drag.take() {
        if drag.list_handle != 0 {
            ctx.kernel.gdi.image_lists.remove(&drag.list_handle);
        }
        if drag.visible {
            ctx.kernel.framebuffer.mark_dirty();
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn enter(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let position = (ctx.arg_u32(1)? as i32, ctx.arg_u32(2)? as i32);
    if hwnd != 0
        && !ctx.kernel.window_rects.contains_key(&hwnd)
        && ctx.kernel.controls.get(hwnd).is_none()
    {
        return ret(false);
    }
    let Some(drag) = ctx.kernel.gdi.image_drag.as_mut() else {
        return ret(false);
    };
    drag.window = hwnd;
    drag.position = position;
    drag.entered = true;
    drag.visible = true;
    ctx.kernel.framebuffer.mark_dirty();
    ret(true)
}
fn leave(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let Some(drag) = ctx.kernel.gdi.image_drag.as_mut() else {
        return ret(false);
    };
    if !drag.entered || hwnd != drag.window {
        return ret(false);
    }
    let visible = drag.visible;
    drag.entered = false;
    drag.visible = false;
    if visible {
        ctx.kernel.framebuffer.mark_dirty();
    }
    ret(true)
}
fn move_image(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let position = (ctx.arg_u32(0)? as i32, ctx.arg_u32(1)? as i32);
    let Some(drag) = ctx.kernel.gdi.image_drag.as_mut() else {
        return ret(false);
    };
    let changed = drag.position != position;
    drag.position = position;
    if changed && drag.visible {
        ctx.kernel.framebuffer.mark_dirty();
    }
    ret(true)
}
fn show(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let show = ctx.arg_u32(0)? != 0;
    let Some(drag) = ctx.kernel.gdi.image_drag.as_mut() else {
        return ret(false);
    };
    if !drag.entered {
        return ret(false);
    }
    if drag.visible != show {
        drag.visible = show;
        ctx.kernel.framebuffer.mark_dirty();
    }
    ret(true)
}
fn get(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (point, hotspot) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    if [point, hotspot]
        .into_iter()
        .any(|p| p != 0 && ctx.cpu.check_guest_access(p, 8, Prot::WRITE).is_err())
    {
        return ret(false);
    }
    let Some(drag) = ctx.kernel.gdi.image_drag.as_ref() else {
        return ret(false);
    };
    let (position, hot, mut handle) = (drag.position, drag.hotspot, drag.list_handle);
    if handle == 0 {
        let image = drag.image.clone();
        handle = ctx.kernel.gdi.create_image_list(image.width, image.height);
        if handle == 0 {
            return ret(false);
        }
        ctx.kernel
            .gdi
            .image_lists
            .get_mut(&handle)
            .unwrap()
            .images
            .push(image);
        ctx.kernel.gdi.image_drag.as_mut().unwrap().list_handle = handle;
    }
    for (pointer, value) in [(point, position), (hotspot, hot)] {
        if pointer != 0 {
            ctx.cpu.write_mem(
                pointer,
                &[value.0.to_le_bytes(), value.1.to_le_bytes()].concat(),
            )?;
        }
    }
    Ok(DispatchOutcome::ReturnedR0(handle))
}
