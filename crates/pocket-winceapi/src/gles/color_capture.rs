//! Explicit paired capture at a completed window swap. No polling or readback
//! when POCKETHLE_COLOR_CAPTURE is unset; enabled sessions poll once a second.
use super::*;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Requests {
    directory: PathBuf,
    next_poll: Instant,
    sequence: u64,
}
thread_local! {
    static REQUESTS: RefCell<Option<Requests>> = const { RefCell::new(None) };
}

pub(super) fn reset() {
    let requests = std::env::var_os("POCKETHLE_COLOR_CAPTURE")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .and_then(|directory| match std::fs::create_dir_all(&directory) {
            Ok(()) => Some(Requests {
                directory,
                next_poll: Instant::now(),
                sequence: 0,
            }),
            Err(error) => {
                log::warn!("Color capture directory unavailable: {error}");
                None
            }
        });
    REQUESTS.with(|r| r.replace(requests));
}

pub(super) struct Capture {
    stem: PathBuf,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    metadata: serde_json::Value,
}

pub(super) fn begin() -> Option<Capture> {
    let stem = REQUESTS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let requests = slot.as_mut()?;
        let now = Instant::now();
        if now < requests.next_poll {
            return None;
        }
        requests.next_poll = now + Duration::from_secs(1);
        match std::fs::remove_file(requests.directory.join("capture.request")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                log::warn!("Color capture request failed: {error}");
                return None;
            }
        }
        requests.sequence += 1;
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Some(
            requests
                .directory
                .join(format!("capture-{ms}-{}", requests.sequence)),
        )
    })?;
    Some(with_ctx(|c| {
        c.sync_gpu();
        Capture {
            stem,
            rgba: c.target.color.clone(),
            width: c.target.width,
            height: c.target.height,
            metadata: serde_json::json!({
                "render_format": if c.target.rgb565 { "RGB565" } else { "RGBA8888" },
                "gpu_active": c.gpu_active(),
                "guest_dither_enabled": c.is_enabled(pocket_gles::GL_DITHER),
                "raw_size": [c.target.width, c.target.height],
                "scope": "Logical render readback before publication; presentation reference before frontend filters/rotation. Not a host-window screenshot."
            }),
        }
    }))
}

pub(super) fn finish(mut capture: Capture, kernel: &mut pocket_kernel::KernelState) {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let image = kernel.framebuffer.cpu().map_err(std::io::Error::other)?;
        let (w, h) = image.presentation_size();
        let mut rgba = Vec::new();
        image.snapshot_presentation_rgba8888_into(&mut rgba);
        capture.metadata["presentation_size"] = serde_json::json!([w, h]);
        capture.metadata["completed_presentation"] =
            kernel.framebuffer.presentation_stats.completed().into();
        image::save_buffer_with_format(
            capture.stem.with_extension("render.png"),
            &capture.rgba,
            capture.width,
            capture.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        image::save_buffer_with_format(
            capture.stem.with_extension("presentation.png"),
            &rgba,
            w,
            h,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        std::fs::write(
            capture.stem.with_extension("json"),
            serde_json::to_vec_pretty(&capture.metadata)?,
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => log::info!(
            "Color capture saved: {} (render.png / presentation.png)",
            capture.stem.display()
        ),
        Err(error) => log::warn!("Color capture failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gles::tests::{call, fresh_cpu, guard};
    use crate::gx::tests::fresh_kernel;
    use pocket_gles::*;

    fn paired_swap(gpu: bool, shared: bool) {
        let _guard = guard();
        reset_for_session();
        let directory = tempfile::tempdir().unwrap();
        REQUESTS.with(|r| {
            r.replace(Some(Requests {
                directory: directory.path().into(),
                next_poll: Instant::now(),
                sequence: 0,
            }))
        });
        let (mut cpu, mut kernel) = (fresh_cpu(), fresh_kernel());
        kernel.framebuffer.resize(4, 4);
        set_gpu_renderer(gpu);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                egl_initialize,
                &[DISPLAY_HANDLE, 0, 0]
            ),
            EGL_TRUE
        );
        let context = call(
            &mut cpu,
            &mut kernel,
            egl_create_context,
            &[DISPLAY_HANDLE, CONFIG_RGBA_HANDLE, 0, 0],
        );
        let surface = call(
            &mut cpu,
            &mut kernel,
            egl_create_window_surface,
            &[DISPLAY_HANDLE, CONFIG_RGBA_HANDLE, 1, 0],
        );
        assert_ne!(context, 0);
        assert_ne!(surface, 0);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                egl_make_current,
                &[DISPLAY_HANDLE, surface, surface, context]
            ),
            EGL_TRUE
        );
        with_ctx(|c| {
            assert_eq!(c.gpu_active(), gpu);
            c.clear_color = [0.4, 0.41, 0.42, 1.];
            c.clear(GL_COLOR_BUFFER_BIT);
        });
        std::fs::write(directory.path().join("capture.request"), b"").unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                egl_swap_buffers,
                &[DISPLAY_HANDLE, surface]
            ),
            EGL_TRUE
        );
        assert_eq!(kernel.framebuffer.gpu_source().is_some(), shared);
        let files: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|p| p.unwrap().path())
            .collect();
        assert_eq!(files.len(), 3);
        let raw = image::open(
            files
                .iter()
                .find(|p| p.to_string_lossy().ends_with(".render.png"))
                .unwrap(),
        )
        .unwrap()
        .to_rgba8();
        let presented = image::open(
            files
                .iter()
                .find(|p| p.to_string_lossy().ends_with(".presentation.png"))
                .unwrap(),
        )
        .unwrap()
        .to_rgba8();
        assert_eq!(raw.dimensions(), (4, 4));
        assert_eq!(presented.dimensions(), (4, 4));
        assert_eq!(
            raw.as_raw(),
            presented.as_raw(),
            "RGBA presentation must retain rendered shades"
        );
        let logical = kernel.framebuffer.cpu().unwrap().snapshot_rgba8888();
        assert_ne!(
            &logical,
            raw.as_raw(),
            "guest RGB565 memory must remain quantized"
        );
        let mut expected = Vec::new();
        kernel
            .framebuffer
            .cpu()
            .unwrap()
            .snapshot_presentation_rgba8888_into(&mut expected);
        assert_eq!(presented.as_raw(), &expected);
        let metadata: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                files
                    .iter()
                    .find(|p| p.extension().is_some_and(|s| s == "json"))
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["render_format"], "RGBA8888");
        assert_eq!(metadata["completed_presentation"], 1);
        assert!(begin().is_none());
        reset_for_session();
        REQUESTS.with(|r| r.replace(None));
    }

    #[test]
    fn software_swap_preserves_rgba_and_keeps_guest_rgb565() {
        paired_swap(false, false);
    }

    #[test]
    #[ignore = "requires host OpenGL"]
    fn gpu_swap_preserves_rgba_and_keeps_guest_rgb565() {
        let root = pocket_gles::gpu::Renderer::new(4, 4).unwrap();
        with_shared_context_source(Some(root.shared_context_source()), || {
            with_shared_frame_export(
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
                || paired_swap(true, true),
            );
        });
    }
}
