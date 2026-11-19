//! GLES2 archive adapters for the GPU transfer implementation shared with GLES1.
use super::*;
use pocket_gles::gpu::checkpoint_transfer::{self as transfer, Api, Plane};
fn moved(image: &mut Image) -> Plane {
    Plane {
        width: image.width,
        height: image.height,
        internal: image.internal,
        format: image.format,
        kind: image.kind,
        bytes: std::mem::take(&mut image.bytes),
        depth_samples: std::mem::take(&mut image.depth_samples),
    }
}
fn copied(image: &Image) -> Plane {
    Plane {
        width: image.width,
        height: image.height,
        internal: image.internal,
        format: image.format,
        kind: image.kind,
        bytes: image.bytes.clone(),
        depth_samples: image.depth_samples.clone(),
    }
}
pub(super) fn select_buffer(r: &mut Renderer, color: u32) -> Result<()> {
    transfer::select_buffer(&mut Api::new(&r.host), color).map_err(error)
}
pub(super) fn read_framebuffer(
    r: &mut Renderer,
    aux: &ProgrammableHost,
    source: u32,
    image: &mut Image,
) -> Result<()> {
    let mut plane = moved(image);
    let result = transfer::read_framebuffer(&mut Api::new(&r.host), aux, source, &mut plane);
    image.bytes = plane.bytes;
    image.depth_samples = plane.depth_samples;
    result.map_err(error)
}
pub(super) fn read_texture(
    r: &mut Renderer,
    aux: &ProgrammableHost,
    texture: u32,
    face: u32,
    level: i32,
    image: &mut Image,
) -> Result<()> {
    let mut plane = moved(image);
    let result = transfer::read_texture(
        &mut Api::new(&r.host),
        aux,
        texture,
        face,
        level,
        &mut plane,
    );
    image.bytes = plane.bytes;
    image.depth_samples = plane.depth_samples;
    result.map_err(error)
}
pub(super) fn upload_depth_stencil(r: &mut Renderer, target: u32, image: &Image) -> Result<()> {
    transfer::upload_depth_stencil(&mut Api::new(&r.host), target, &copied(image)).map_err(error)
}
pub(super) fn upload_texture_depth_stencil(
    r: &mut Renderer,
    face: u32,
    level: i32,
    image: &Image,
) -> Result<()> {
    transfer::upload_texture_depth_stencil(&mut Api::new(&r.host), face, level, &copied(image))
        .map_err(error)
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use pocket_gles::gpu::with_shared_context_source;
    #[test]
    #[ignore = "requires native shared desktop GL and OpenGL ES 3 contexts"]
    fn odd_width_deleted_color_images_preserve_pixels_and_pack_alignment() -> Result<()> {
        use super::super::super::uniforms::native_tests::call;
        use pocket_kernel::Thunk;
        use pocket_pe::ImportBinding;
        for es in [false, true] {
            let host = if es {
                ProgrammableHost::new_programmable()
            } else {
                ProgrammableHost::new()
            }
            .map_err(error)?;
            with_shared_context_source(Some(host.shared_source()), || -> Result<()> {
                let (mut cpu, mut kernel, _) = crate::bluetooth::tests::setup();
                let thunk = Thunk {
                    thunk_va: 0,
                    iat_va: 0,
                    dll: "libglesv2.dll".into(),
                    binding: ImportBinding::Name("glTexImage2D".into()),
                    friendly_name: None,
                };
                let mut ctx = CallCtx {
                    cpu: &mut cpu,
                    kernel: &mut kernel,
                    thunk: &thunk,
                };
                let mut r = Renderer::new(8, 8, 1)?;
                let pixels: Vec<u8> = (0..6).flat_map(|n| [n * 30, 17, 240, 255]).collect();
                ctx.cpu.write_mem(0x2000, &pixels)?;
                call(&mut r, &mut ctx, "glGenTextures", &[1, 0x1000])?;
                let texture = ctx.cpu.read_u32_le(0x1000)?;
                call(&mut r, &mut ctx, "glBindTexture", &[TEXTURE_2D, texture])?;
                call(
                    &mut r,
                    &mut ctx,
                    "glTexImage2D",
                    &[TEXTURE_2D, 0, RGBA, 3, 2, 0, RGBA, UNSIGNED_BYTE, 0x2000],
                )?;
                call(&mut r, &mut ctx, "glGenFramebuffers", &[1, 0x1004])?;
                let fbo = ctx.cpu.read_u32_le(0x1004)?;
                call(&mut r, &mut ctx, "glBindFramebuffer", &[FRAMEBUFFER, fbo])?;
                call(
                    &mut r,
                    &mut ctx,
                    "glFramebufferTexture2D",
                    &[FRAMEBUFFER, 0x8ce0, TEXTURE_2D, texture, 0],
                )?;
                call(&mut r, &mut ctx, "glBindFramebuffer", &[FRAMEBUFFER, 0])?;
                call(&mut r, &mut ctx, "glDeleteTextures", &[1, 0x1000])?;
                call(&mut r, &mut ctx, "glPixelStorei", &[0x0d05, 8])?;
                let saved = r.capture_context().map_err(error)?;
                assert_eq!(saved.native.retained.len(), 1);
                assert_eq!(saved.native.retained[0].image.bytes, pixels);
                call(&mut r, &mut ctx, "glGetIntegerv", &[0x0d05, 0x1008])?;
                assert_eq!(ctx.cpu.read_u32_le(0x1008)?, 8);
                drop(r);
                let mut r = saved.restore(1).map_err(error)?;
                let again = r.capture_context().map_err(error)?;
                assert_eq!(again.native.retained[0].image.bytes, pixels);
                assert_eq!(call(&mut r, &mut ctx, "glGetError", &[])?, 0);
                Ok(())
            })?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires native host OpenGL ES 3"]
    fn es_direct_checkpoint_preserves_every_stencil_value_and_depth() -> Result<()> {
        let ui = ProgrammableHost::new_programmable().map_err(error)?;
        with_shared_context_source(Some(ui.shared_source()), || {
            let mut renderer = Renderer::new(16, 16, 1)?;
            assert!(renderer.host.es);
            invoke!(&mut renderer,"glEnable",(0x0c11=>u32),());
            for value in 0..256 {
                invoke!(&mut renderer,"glScissor",(value%16=>i32,value/16=>i32,1=>i32,1=>i32),());
                invoke!(&mut renderer,"glClearDepthf",((value as f32+0.5)/256.0=>f32),());
                invoke!(&mut renderer,"glClearStencil",(value=>i32),());
                invoke!(&mut renderer,"glClearColor",(value as f32/255.0=>f32,0.25=>f32,0.5=>f32,1.0=>f32),());
                invoke!(&mut renderer,"glClear",(0x4500=>u32),());
            }
            invoke!(&mut renderer,"glDisable",(0x0c11=>u32),());
            let saved = renderer.capture_context().map_err(error)?;
            let expected = saved
                .native
                .renderbuffers
                .iter()
                .find(|p| p.guest == 0)
                .unwrap()
                .image
                .bytes
                .clone();
            for (n, pixel) in expected.chunks_exact(4).enumerate() {
                let packed = u32::from_le_bytes(pixel.try_into().unwrap());
                assert_eq!(packed & 255, n as u32, "stencil {n}");
                assert!(
                    ((packed >> 8) as f64 / 16777215.0 - (n as f64 + 0.5) / 256.0).abs() < 1e-6,
                    "depth {n}: {packed:#x}"
                );
            }
            let color = saved.native.window.bytes.clone();
            let bytes = serde_json::to_vec(&saved).map_err(|e| error(e.to_string()))?;
            drop(saved);
            drop(renderer);
            let saved: ContextSnapshot =
                serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
            let mut restored = saved.restore(1).map_err(error)?;
            for _ in 0..2 {
                let again = restored.capture_context().map_err(error)?;
                assert_eq!(again.native.window.bytes, color);
                for (n, (actual, wanted)) in again
                    .native
                    .renderbuffers
                    .iter()
                    .find(|p| p.guest == 0)
                    .unwrap()
                    .image
                    .bytes
                    .chunks_exact(4)
                    .zip(expected.chunks_exact(4))
                    .enumerate()
                {
                    assert_eq!(actual, wanted, "restored depth/stencil pixel {n}");
                }
                drop(restored);
                restored = again.restore(1).map_err(error)?;
            }
            assert_eq!(invoke!(&mut restored, "glGetError", (), u32), 0);
            Ok(())
        })
    }
    #[test]
    #[ignore = "requires native host OpenGL ES 3"]
    fn es_live_mips_cube_legacy_formats_and_buffers_survive_cold_restore() -> Result<()> {
        use super::super::super::uniforms::native_tests::call;
        use pocket_kernel::Thunk;
        use pocket_pe::ImportBinding;
        let ui = ProgrammableHost::new_programmable().map_err(error)?;
        with_shared_context_source(Some(ui.shared_source()), || {
            let (mut cpu, mut kernel, _) = crate::bluetooth::tests::setup();
            let thunk = Thunk {
                thunk_va: 0,
                iat_va: 0,
                dll: "libglesv2.dll".into(),
                binding: ImportBinding::Name("glTexImage2D".into()),
                friendly_name: None,
            };
            let mut ctx = CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            };
            let mut r = Renderer::new(8, 8, 1)?;
            for format in [RGBA, 0x1907, 0x1906, 0x1909, 0x190a] {
                call(&mut r, &mut ctx, "glGenTextures", &[1, 0x1000])?;
                let texture = ctx.cpu.read_u32_le(0x1000)?;
                call(&mut r, &mut ctx, "glBindTexture", &[TEXTURE_2D, texture])?;
                ctx.cpu.write_mem(0x2000, &[73; 64])?;
                call(
                    &mut r,
                    &mut ctx,
                    "glTexImage2D",
                    &[
                        TEXTURE_2D,
                        0,
                        format,
                        4,
                        4,
                        0,
                        format,
                        UNSIGNED_BYTE,
                        0x2000,
                    ],
                )?;
                call(&mut r, &mut ctx, "glGenerateMipmap", &[TEXTURE_2D])?;
                // Native rejection must neither replace valid metadata nor hide the error.
                call(
                    &mut r,
                    &mut ctx,
                    "glTexImage2D",
                    &[
                        TEXTURE_2D,
                        0,
                        format,
                        4,
                        4,
                        1,
                        format,
                        UNSIGNED_BYTE,
                        0x2000,
                    ],
                )?;
                assert_eq!(call(&mut r, &mut ctx, "glGetError", &[])?, 0x0501);
            }
            call(&mut r, &mut ctx, "glGenTextures", &[1, 0x1000])?;
            let cube = ctx.cpu.read_u32_le(0x1000)?;
            call(&mut r, &mut ctx, "glBindTexture", &[0x8513, cube])?;
            let mut known = Vec::new();
            for face in 0x8515..=0x851a {
                let pixels: Vec<u8> = (0..16)
                    .flat_map(|n| [face as u8, (n % 4 * 50) as u8, (n / 4 * 70) as u8, 255])
                    .collect();
                ctx.cpu.write_mem(0x2000, &pixels)?;
                known.push((face, 0, pixels));
                call(
                    &mut r,
                    &mut ctx,
                    "glTexImage2D",
                    &[face, 0, RGBA, 4, 4, 0, RGBA, UNSIGNED_BYTE, 0x2000],
                )?;
            }
            call(&mut r, &mut ctx, "glGenerateMipmap", &[0x8513])?;
            for face in 0x8515..=0x851a {
                let pixels: Vec<u8> = (0..4)
                    .flat_map(|n| [face as u8, 20 + n * 60, 170 - n * 30, 255])
                    .collect();
                ctx.cpu.write_mem(0x2000, &pixels)?;
                call(
                    &mut r,
                    &mut ctx,
                    "glTexImage2D",
                    &[face, 1, RGBA, 2, 2, 0, RGBA, UNSIGNED_BYTE, 0x2000],
                )?;
                known.push((face, 1, pixels));
            }
            call(&mut r, &mut ctx, "glBindTexture", &[0x8513, 0])?;
            call(&mut r, &mut ctx, "glGenBuffers", &[1, 0x1000])?;
            let buffer = ctx.cpu.read_u32_le(0x1000)?;
            call(&mut r, &mut ctx, "glBindBuffer", &[ARRAY_BUFFER, buffer])?;
            ctx.cpu.write_mem(0x2000, b"native buffer bytes")?;
            call(
                &mut r,
                &mut ctx,
                "glBufferData",
                &[ARRAY_BUFFER, 19, 0x2000, 0x88e4],
            )?;
            let saved = r.capture_context().map_err(error)?;
            let expected: Vec<_> = saved
                .native
                .textures
                .iter()
                .flat_map(|t| {
                    t.levels
                        .iter()
                        .map(move |(f, l, p)| (t.guest, *f, *l, p.bytes.clone()))
                })
                .collect();
            assert_eq!(expected.len(), 33);
            for (face, level, pixels) in known {
                let actual = &saved
                    .native
                    .textures
                    .iter()
                    .find(|t| t.guest == cube)
                    .unwrap()
                    .levels
                    .iter()
                    .find(|(f, l, _)| *f == face && *l == level)
                    .unwrap()
                    .2
                    .bytes;
                assert_eq!(*actual, pixels, "captured cube face {face:#x}, mip {level}");
            }
            drop(r);
            let mut restored = saved.restore(1).map_err(error)?;
            let again = restored.capture_context().map_err(error)?;
            for (guest, face, level, bytes) in expected {
                let image = &again
                    .native
                    .textures
                    .iter()
                    .find(|t| t.guest == guest)
                    .unwrap()
                    .levels
                    .iter()
                    .find(|(f, l, _)| *f == face && *l == level)
                    .unwrap()
                    .2;
                assert_eq!(
                    image.bytes, bytes,
                    "guest texture {guest}, face {face:#x}, mip {level}"
                );
            }
            assert_eq!(
                again
                    .native
                    .buffers
                    .iter()
                    .find(|b| b.guest == buffer)
                    .unwrap()
                    .bytes,
                b"native buffer bytes"
            );
            assert_eq!(call(&mut restored, &mut ctx, "glGetError", &[])?, 0);
            Ok(())
        })
    }
    #[test]
    #[ignore = "requires native host OpenGL ES 3"]
    fn es_failed_link_keeps_installed_executable_through_gpu_capture() -> Result<()> {
        let ui = ProgrammableHost::new_programmable().map_err(error)?;
        with_shared_context_source(Some(ui.shared_source()), || {
            super::super::super::uniforms::native_tests::installed_executable_survives_failed_link_and_two_cold_snapshots()
        })
    }
}
