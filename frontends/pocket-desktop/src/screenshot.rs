//! Read only the rendered game rectangle, after its filter and before later UI.
use eframe::{
    egui,
    glow::{self, HasContext},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[derive(Default)]
pub struct Screenshot {
    pub pending: Option<PathBuf>,
    pub busy: bool,
    pub completed: Option<Result<PathBuf, String>>,
}
pub type SharedScreenshot = Arc<Mutex<Screenshot>>;
fn finish(state: &SharedScreenshot, ctx: &egui::Context, result: Result<PathBuf, String>) {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.busy = false;
    state.completed = Some(result);
    ctx.request_repaint();
}
/// Returns top-to-bottom RGBA pixels, matching the viewport and DPI actually painted.
pub fn read_game_pixels(
    gl: &glow::Context,
    info: egui::PaintCallbackInfo,
) -> Result<image::RgbaImage, String> {
    let info = egui::PaintCallbackInfo {
        viewport: info.viewport.intersect(info.clip_rect),
        ..info
    };
    let crop = info.viewport_in_pixels();
    if crop.width_px <= 0 || crop.height_px <= 0 {
        return Err("game screen is not visible".into());
    }
    let width = crop.width_px as u32;
    let height = crop.height_px as u32;
    let length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("screenshot dimensions overflow")?;
    let mut rgba = vec![0; length];
    unsafe {
        let read =
            std::num::NonZeroU32::new(gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING) as u32)
                .map(glow::NativeFramebuffer);
        let draw =
            std::num::NonZeroU32::new(gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) as u32)
                .map(glow::NativeFramebuffer);
        let buffer =
            std::num::NonZeroU32::new(gl.get_parameter_i32(glow::PIXEL_PACK_BUFFER_BINDING) as u32)
                .map(glow::NativeBuffer);
        let settings = [
            glow::PACK_ALIGNMENT,
            glow::PACK_ROW_LENGTH,
            glow::PACK_SKIP_ROWS,
            glow::PACK_SKIP_PIXELS,
        ];
        let saved = settings.map(|name| gl.get_parameter_i32(name));
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, draw);
        gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
        for (name, value) in settings.into_iter().zip([1, 0, 0, 0]) {
            gl.pixel_store_i32(name, value);
        }
        gl.read_pixels(
            crop.left_px,
            crop.from_bottom_px,
            crop.width_px,
            crop.height_px,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(&mut rgba),
        );
        let error = gl.get_error();
        for (name, value) in settings.into_iter().zip(saved) {
            gl.pixel_store_i32(name, value);
        }
        gl.bind_buffer(glow::PIXEL_PACK_BUFFER, buffer);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
        if error != glow::NO_ERROR {
            return Err(format!("GPU screenshot readback failed (0x{error:04x})"));
        }
    }
    let mut pixels =
        image::RgbaImage::from_raw(width, height, rgba).ok_or("invalid screenshot pixels")?;
    image::imageops::flip_vertical_in_place(&mut pixels);
    // The game image is opaque; framebuffer alpha can reflect compositor state.
    for pixel in pixels.pixels_mut() {
        pixel.0[3] = 255;
    }
    Ok(pixels)
}
pub fn capture(
    gl: &glow::Context,
    info: egui::PaintCallbackInfo,
    state: &SharedScreenshot,
    ctx: &egui::Context,
) {
    let path = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .pending
        .take();
    let Some(path) = path else {
        return;
    };
    let pixels = match read_game_pixels(gl, info) {
        Ok(p) => p,
        Err(e) => {
            finish(state, ctx, Err(e));
            return;
        }
    };
    let state_worker = Arc::clone(state);
    let ctx_worker = ctx.clone();
    let spawned = std::thread::Builder::new()
        .name("screenshot-png".into())
        .spawn(move || {
            let result = pixels
                .save_with_format(&path, image::ImageFormat::Png)
                .map(|_| path.clone())
                .map_err(|e| format!("could not write {}: {e}", path.display()));
            finish(&state_worker, &ctx_worker, result);
        });
    if let Err(e) = spawned {
        finish(state, ctx, Err(format!("could not start PNG writer: {e}")));
    }
}
