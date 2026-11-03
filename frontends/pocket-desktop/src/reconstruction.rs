//! Native GPU reconstruction, cached SMAA, and bounded xBRZ CPU preprocessing.
use crate::runner::FrameSnapshot;
use eframe::glow::{self, HasContext};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentationPath {
    GpuShared,
    Cpu(&'static str),
    Retained(&'static str),
    Failed,
}
pub struct Reconstruction {
    program: glow::Program,
    vao: glow::VertexArray,
    texture: glow::Texture,
    size: [u32; 2],
    staging: Option<glow::Texture>,
    staging_size: [u32; 2],
    pending: Option<FrameSnapshot>,
    smaa: Option<crate::smaa::Smaa>,
    smaa_dirty: bool,
    smaa_mode: i32,
    consumer: Option<pocket_core::winceapi::gles::SharedConsumer>,
    shared_failed: bool,
    current_path: Option<PresentationPath>,
    reported_path: Option<PresentationPath>,
    shared_requested: bool,
    copy_program: Option<glow::Program>,
    copy_fbo: Option<glow::Framebuffer>,
    export_policy: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Reconstruction {
    pub fn new(
        gl: &glow::Context,
        consumer: Option<pocket_core::winceapi::gles::SharedConsumer>,
        export_policy: std::sync::Arc<std::sync::atomic::AtomicBool>,
        shared_requested: bool,
    ) -> Result<Self, String> {
        let prefix = if gl.version().is_embedded {
            "#version 300 es\nprecision highp float;\nprecision highp int;\n"
        } else {
            "#version 140\n"
        };
        unsafe {
            let program = gl.create_program()?;
            let mut shaders = Vec::new();
            for (kind, source) in [
                (
                    glow::VERTEX_SHADER,
                    include_str!("shaders/reconstruction.vert"),
                ),
                (
                    glow::FRAGMENT_SHADER,
                    include_str!("shaders/reconstruction.frag"),
                ),
            ] {
                let shader = match gl.create_shader(kind) {
                    Ok(s) => s,
                    Err(error) => {
                        for s in shaders {
                            gl.delete_shader(s);
                        }
                        gl.delete_program(program);
                        return Err(error);
                    }
                };
                gl.shader_source(shader, &format!("{prefix}{source}"));
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let error = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    for s in shaders {
                        gl.delete_shader(s);
                    }
                    gl.delete_program(program);
                    return Err(error);
                }
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }
            gl.link_program(program);
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }
            if !gl.get_program_link_status(program) {
                let error = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(error);
            }
            let vao = match gl.create_vertex_array() {
                Ok(v) => v,
                Err(e) => {
                    gl.delete_program(program);
                    return Err(e);
                }
            };
            let texture = match gl.create_texture() {
                Ok(t) => t,
                Err(e) => {
                    gl.delete_vertex_array(vao);
                    gl.delete_program(program);
                    return Err(e);
                }
            };
            let smaa = match crate::smaa::Smaa::new(gl) {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("SMAA unavailable, using reconstruction: {e}");
                    None
                }
            };
            let mut copy_program = None;
            let mut copy_fbo = None;
            if consumer.is_some() && shared_requested {
                match shared_program(gl, prefix).and_then(|p| match gl.create_framebuffer() {
                    Ok(f) => Ok((p, f)),
                    Err(e) => {
                        gl.delete_program(p);
                        Err(e)
                    }
                }) {
                    Ok((p, f)) => {
                        copy_program = Some(p);
                        copy_fbo = Some(f);
                    }
                    Err(e) => log::warn!(
                        "Shared display conversion unavailable; retaining CPU upload: {e}"
                    ),
                }
            }
            export_policy.store(copy_program.is_some(), std::sync::atomic::Ordering::Relaxed);
            Ok(Self {
                program,
                vao,
                texture,
                size: [0, 0],
                staging: None,
                staging_size: [0, 0],
                pending: None,
                smaa,
                smaa_dirty: true,
                smaa_mode: -1,
                consumer,
                shared_failed: false,
                current_path: None,
                reported_path: None,
                shared_requested,
                copy_program,
                copy_fbo,
                export_policy,
            })
        }
    }
    pub fn reset_presentation_log(&mut self) {
        self.reported_path = None;
    }
    unsafe fn report_presentation(&mut self, gl: &glow::Context) {
        let Some(path) = self.current_path else {
            return;
        };
        if self.reported_path == Some(path) {
            return;
        }
        // Only check on a path transition, not on every steady-state frame.
        let error = gl.get_error();
        if error != glow::NO_ERROR {
            if self.reported_path != Some(PresentationPath::Failed) {
                log::warn!("PRESENTATION=ERROR gl_error=0x{error:04x}; shared export disabled");
            }
            self.export_policy
                .store(false, std::sync::atomic::Ordering::Relaxed);
            self.shared_failed = true;
            self.current_path = Some(PresentationPath::Failed);
            self.reported_path = self.current_path;
            return;
        }
        match path {
            PresentationPath::GpuShared=>log::info!("PRESENTATION=GPU_SHARED cpu_pixel_upload=NO (shared texture converted and displayed on GPU)"),
            PresentationPath::Cpu(reason)=>log::info!("PRESENTATION=CPU_UPLOAD reason={reason} (presentation uses CPU pixels; GLES rendering may still be GPU)"),
            PresentationPath::Retained("no_valid_image_available")=>log::warn!("PRESENTATION=NO_IMAGE reason=no_valid_image_available"),
            PresentationPath::Retained(reason)=>log::warn!("PRESENTATION=LAST_VALID_FRAME reason={reason} (new frame was not displayed)"),
            PresentationPath::Failed=>{},
        }
        self.reported_path = Some(path);
    }
    pub fn queue(&mut self, frame: &FrameSnapshot, filter: i32) {
        self.export_policy.store(
            self.shared_requested
                && !self.shared_failed
                && self.copy_program.is_some()
                && filter != 6,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.pending = match prepare_frame(frame, filter) {
            Ok(frame) => Some(frame),
            Err(error) => {
                self.export_policy
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                log::warn!("CPU filter preparation failed: {error}");
                self.current_path =
                    Some(PresentationPath::Retained("cpu_filter_preparation_failed"));
                None
            }
        };
        self.smaa_dirty = true;
    }
    pub fn paint(&mut self, gl: &glow::Context, uv: [f32; 6], filter: i32) {
        if let Some(consumer) = self.consumer.as_mut() {
            let _ = consumer.collect_retired_frames();
        }
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            if let Some(frame) = self.pending.take() {
                let mut cpu_reason = if filter == 6 {
                    "xbrz_cpu_filter"
                } else if frame.shared.is_none() {
                    "no_shared_frame_supplied"
                } else {
                    "shared_export_disabled"
                };
                let copied = if frame.shared.is_some()
                    && self
                        .export_policy
                        .load(std::sync::atomic::Ordering::Relaxed)
                {
                    match self.copy_shared(gl, &frame) {
                        Ok(()) => true,
                        Err(e) => {
                            self.shared_failed = true;
                            cpu_reason = "shared_gpu_conversion_failed";
                            self.export_policy
                                .store(false, std::sync::atomic::Ordering::Relaxed);
                            log::warn!(
                                "Shared display conversion failed; retaining CPU upload: {e}"
                            );
                            false
                        }
                    }
                } else {
                    false
                };
                if copied {
                    self.current_path = Some(PresentationPath::GpuShared);
                }
                if !copied {
                    let result = frame
                        .cpu_rgba()
                        .and_then(|rgba| self.upload_cpu(gl, &frame, rgba.as_ref()));
                    match result {
                        Ok(()) => self.current_path = Some(PresentationPath::Cpu(cpu_reason)),
                        Err(error) => {
                            log::warn!("CPU presentation fallback unavailable; retaining last image: {error}");
                            self.current_path =
                                Some(PresentationPath::Retained("gpu_and_cpu_frame_unavailable"));
                        }
                    }
                }
                gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
                for name in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, name, glow::LINEAR as i32);
                }
                for name in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, name, glow::CLAMP_TO_EDGE as i32);
                }
            }
            if self.size[0] == 0 {
                self.current_path = Some(PresentationPath::Retained("no_valid_image_available"));
                self.report_presentation(gl);
                return;
            }
            let mut output_texture = self.texture;
            let output_filter = match filter {
                5 => 3,
                6 | 7 => 1,
                _ => filter,
            };
            if filter == 5 || filter == 7 {
                if let Some(smaa) = self.smaa.as_mut() {
                    if self.smaa_dirty || self.smaa_mode != filter {
                        if let Err(e) = smaa.process(gl, self.texture, self.size, filter == 7) {
                            log::warn!("SMAA render target unavailable: {e}");
                        }
                        self.smaa_dirty = false;
                        self.smaa_mode = filter;
                    }
                    if let Some(texture) = smaa.output(self.size) {
                        output_texture = texture;
                    }
                }
            }
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(output_texture));
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.uniform_1_i32(gl.get_uniform_location(self.program, "u_frame").as_ref(), 0);
            gl.uniform_1_i32(
                gl.get_uniform_location(self.program, "u_filter").as_ref(),
                output_filter,
            );
            gl.uniform_2_f32(
                gl.get_uniform_location(self.program, "u_size").as_ref(),
                self.size[0] as f32,
                self.size[1] as f32,
            );
            for (name, offset) in [("u_origin", 0), ("u_dx", 2), ("u_dy", 4)] {
                gl.uniform_2_f32(
                    gl.get_uniform_location(self.program, name).as_ref(),
                    uv[offset],
                    uv[offset + 1],
                );
            }
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            self.report_presentation(gl);
        }
    }
    unsafe fn staging_target(&mut self, gl: &glow::Context) -> Result<glow::Texture, String> {
        if let Some(texture) = self.staging {
            return Ok(texture);
        }
        let texture = gl.create_texture()?;
        self.staging = Some(texture);
        self.staging_size = [0, 0];
        Ok(texture)
    }
    fn commit_staging(&mut self, size: [u32; 2]) {
        std::mem::swap(
            &mut self.texture,
            self.staging.as_mut().expect("allocated staging target"),
        );
        self.staging_size = self.size;
        self.size = size;
    }
    unsafe fn upload_cpu(
        &mut self,
        gl: &glow::Context,
        frame: &FrameSnapshot,
        rgba: &[u8],
    ) -> Result<(), String> {
        let size = [frame.texture_width, frame.texture_height];
        // The ordinary CPU-only presenter keeps its existing single texture.
        // Once shared conversion is used, both upload paths commit a spare
        // texture only after success, so failed uploads cannot erase the last one.
        let transactional = self.staging.is_some() || frame.shared.is_some();
        let texture = if transactional {
            self.staging_target(gl)?
        } else {
            self.texture
        };
        let previous = if transactional {
            self.staging_size
        } else {
            self.size
        };
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        if previous == size {
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                size[0] as i32,
                size[1] as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(rgba),
            );
        } else {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                size[0] as i32,
                size[1] as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(rgba),
            );
        }
        if transactional {
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                self.staging_size = [0, 0];
                return Err(format!("CPU upload GL error 0x{error:04x}"));
            }
            self.commit_staging(size);
        } else {
            self.size = size;
        }
        Ok(())
    }
    unsafe fn copy_shared(
        &mut self,
        gl: &glow::Context,
        frame: &FrameSnapshot,
    ) -> Result<(), String> {
        let source = frame.shared.as_ref().ok_or("no shared snapshot")?;
        let vao = self.vao;
        let texture = self.staging_target(gl)?;
        let consumer = self.consumer.as_mut().ok_or("no shared UI consumer")?;
        let program = self.copy_program.ok_or("no shared conversion program")?;
        let fbo = self.copy_fbo.ok_or("no shared conversion framebuffer")?;
        let framebuffer =
            std::num::NonZeroU32::new(gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) as u32)
                .map(glow::NativeFramebuffer);
        let mut viewport = [0; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        let caps = [
            glow::BLEND,
            glow::DEPTH_TEST,
            glow::CULL_FACE,
            glow::SCISSOR_TEST,
            glow::DITHER,
        ];
        let enabled = caps.map(|cap| gl.is_enabled(cap));
        let mut mask = [0; 4];
        gl.get_parameter_i32_slice(glow::COLOR_WRITEMASK, &mut mask);
        let size = [frame.texture_width, frame.texture_height];
        let allocate = self.staging_size != size;
        let result = source
            .with_consumer(consumer, |name| -> Result<(), String> {
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                if allocate {
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        size[0] as i32,
                        size[1] as i32,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        None,
                    );
                }
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(fbo));
                gl.framebuffer_texture_2d(
                    glow::DRAW_FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                if gl.check_framebuffer_status(glow::DRAW_FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
                {
                    return Err("shared display framebuffer incomplete".into());
                }
                gl.viewport(0, 0, size[0] as i32, size[1] as i32);
                for cap in caps {
                    gl.disable(cap);
                }
                gl.color_mask(true, true, true, true);
                gl.use_program(Some(program));
                gl.bind_vertex_array(Some(vao));
                gl.bind_texture(
                    glow::TEXTURE_2D,
                    std::num::NonZeroU32::new(name).map(glow::NativeTexture),
                );
                gl.uniform_1_i32(gl.get_uniform_location(program, "u_source").as_ref(), 0);
                gl.uniform_2_f32(
                    gl.get_uniform_location(program, "u_size").as_ref(),
                    size[0] as f32,
                    size[1] as f32,
                );
                gl.uniform_1_i32(
                    gl.get_uniform_location(program, "u_backlight").as_ref(),
                    frame.backlight as i32,
                );
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                gl.bind_texture(glow::TEXTURE_2D, None);
                let error = gl.get_error();
                if error == glow::NO_ERROR {
                    Ok(())
                } else {
                    Err(format!("shared display GL error 0x{error:04x}"))
                }
            })
            .and_then(|r| r);
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, framebuffer);
        gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
        for (cap, on) in caps.into_iter().zip(enabled) {
            if on {
                gl.enable(cap)
            } else {
                gl.disable(cap)
            }
        }
        gl.color_mask(mask[0] != 0, mask[1] != 0, mask[2] != 0, mask[3] != 0);
        if result.is_ok() {
            self.commit_staging(size);
        } else {
            self.staging_size = [0, 0];
        }
        result
    }
    pub fn destroy(&mut self, gl: &glow::Context) {
        if let Some(smaa) = self.smaa.take() {
            smaa.destroy(gl);
        }
        self.pending.take();
        if let Some(consumer) = self.consumer.as_mut() {
            let _ = consumer.collect_retired_frames();
        }
        unsafe {
            if let Some(p) = self.copy_program.take() {
                gl.delete_program(p);
            }
            if let Some(f) = self.copy_fbo.take() {
                gl.delete_framebuffer(f);
            }
            if let Some(texture) = self.staging.take() {
                gl.delete_texture(texture);
            }
            gl.delete_texture(self.texture);
            gl.delete_vertex_array(self.vao);
            gl.delete_program(self.program);
        }
    }
}

unsafe fn shared_program(gl: &glow::Context, prefix: &str) -> Result<glow::Program, String> {
    let program = gl.create_program()?;
    let mut shaders = Vec::new();
    let result = (|| {
        for (kind, source) in [
            (
                glow::VERTEX_SHADER,
                include_str!("shaders/reconstruction.vert"),
            ),
            (
                glow::FRAGMENT_SHADER,
                pocket_core::winceapi::gles::SHARED_DISPLAY_FRAGMENT,
            ),
        ] {
            let shader = gl.create_shader(kind)?;
            shaders.push(shader);
            gl.shader_source(shader, &format!("{prefix}{source}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(gl.get_shader_info_log(shader));
            }
            gl.attach_shader(program, shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        Ok(program)
    })();
    for shader in shaders {
        gl.detach_shader(program, shader);
        gl.delete_shader(shader);
    }
    if result.is_err() {
        gl.delete_program(program);
    }
    result
}

fn prepare_frame(frame: &FrameSnapshot, filter: i32) -> Result<FrameSnapshot, String> {
    let mut output = frame.clone();
    if filter == 6 && frame.texture_width > 0 && frame.texture_height > 0 {
        let rgba = frame.cpu_rgba()?;
        output.shared = None;
        output.deferred = None;
        output.rgba = xbrz::scale_rgba(
            &rgba,
            frame.texture_width as usize,
            frame.texture_height as usize,
            3,
        );
        output.texture_width = output
            .texture_width
            .checked_mul(3)
            .ok_or("CPU filter width overflow")?;
        output.texture_height = output
            .texture_height
            .checked_mul(3)
            .ok_or("CPU filter height overflow")?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xbrz_blends_diagonals_without_mutating_native_frame() {
        let mut rgba = vec![0; 16 * 16 * 4];
        for y in 0..16 {
            for x in 0..16 {
                let c = if x > y { 255 } else { 0 };
                let i = (y * 16 + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[c, c, c, 255]);
            }
        }
        let native = FrameSnapshot {
            presentation_stats: Default::default(),
            text_input: false,
            shared: None,
            deferred: None,
            backlight: 11,
            width: 16,
            height: 16,
            texture_width: 16,
            texture_height: 16,
            rgba,
        };
        let scaled = prepare_frame(&native, 6).unwrap();
        assert_eq!((scaled.width, scaled.height), (16, 16));
        assert_eq!((scaled.texture_width, scaled.texture_height), (48, 48));
        assert_eq!(scaled.rgba.len(), 48 * 48 * 4);
        assert!(scaled.rgba.chunks_exact(4).any(|p| p[0] > 0 && p[0] < 255));
        let pixel = |x: usize, y: usize| &scaled.rgba[(y * 48 + x) * 4..(y * 48 + x) * 4 + 3];
        assert_eq!(pixel(40, 6), [255, 255, 255]);
        assert_eq!(pixel(6, 40), [0, 0, 0]);
        // Switching to SMAA or another GPU mode must upload the original image.
        for mode in [0, 1, 2, 3, 4, 5, 7] {
            let next = prepare_frame(&native, mode).unwrap();
            assert_eq!((next.width, next.height), (16, 16));
            assert_eq!(next.rgba, native.rgba);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "reconstruction_tests.rs"]
mod native_tests;
