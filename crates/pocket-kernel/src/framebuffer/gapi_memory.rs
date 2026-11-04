//! Logical GAPI bytes backed by an immutable GPU snapshot until first access.
use super::deferred::CpuPixelSource;
use pocket_cpu::{Cpu, CpuError, DeferredMemorySource};
use std::sync::Arc;
#[derive(Debug)]
struct Pixels {
    source: Arc<dyn CpuPixelSource>,
    width: u32,
    height: u32,
    columns: bool,
}
impl DeferredMemorySource for Pixels {
    fn materialize(&self) -> Result<Vec<u8>, String> {
        let pixels = self.source.materialize()?;
        let size = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(2))
            .ok_or("GAPI image overflow")?;
        if pixels.logical_size != (self.width, self.height) || pixels.rgb565.len() != size {
            return Err("GAPI image metadata mismatch".into());
        }
        if self.columns {
            let mut encoded = vec![0; size];
            crate::gapi_copy_columns(
                self.width as usize,
                self.height as usize,
                &pixels.rgb565,
                &mut encoded,
                true,
            );
            Ok(encoded)
        } else {
            Ok(pixels.rgb565)
        }
    }
}
pub fn eligible(cpu: &dyn Cpu, state: &crate::KernelState) -> bool {
    cpu.supports_deferred_memory()
        && state.framebuffer.bpp == 16
        && (crate::gapi_uses_columns(state)
            || state.guest_fb_pitch.max(state.framebuffer.stride_bytes())
                == state.framebuffer.stride_bytes())
}
pub fn defer(
    cpu: &mut dyn Cpu,
    state: &crate::KernelState,
    source: Arc<dyn CpuPixelSource>,
) -> Result<bool, CpuError> {
    if !eligible(cpu, state) {
        return Ok(false);
    }
    let (width, height) = state.framebuffer.logical_size();
    cpu.defer_memory(
        crate::SYNTHETIC_FRAMEBUFFER_BASE,
        state.framebuffer.byte_size(),
        Arc::new(Pixels {
            source,
            width,
            height,
            columns: crate::gapi_uses_columns(state),
        }),
    )
}
pub fn discard(cpu: &mut dyn Cpu) -> Result<(), CpuError> {
    if let Some((base, size)) = cpu.deferred_memory_range(crate::SYNTHETIC_FRAMEBUFFER_BASE) {
        cpu.discard_deferred_memory(base, size)?;
    }
    Ok(())
}
/// A GDI write may replace GPU authority before guest execution resumes. Mirror
/// that new CPU image, rather than letting a later fault restore old GPU pixels.
pub(crate) fn before_resume(
    cpu: &mut dyn Cpu,
    state: &mut crate::KernelState,
) -> Result<(), crate::KernelError> {
    if !state.fb_mapped
        || !cpu.deferred_memory_pending(crate::SYNTHETIC_FRAMEBUFFER_BASE)
        || state.framebuffer.gpu_source().is_some()
    {
        return Ok(());
    }
    let pixels = state
        .framebuffer
        .cpu()
        .map_err(crate::KernelError::Dispatch)?
        .pixels
        .clone();
    discard(cpu)?;
    if crate::gapi_uses_columns(state) {
        state.gx_layout_scratch.resize(pixels.len(), 0);
        crate::gapi_copy_columns(
            state.framebuffer.width as usize,
            state.framebuffer.height as usize,
            &pixels,
            &mut state.gx_layout_scratch,
            true,
        );
        cpu.write_mem(crate::SYNTHETIC_FRAMEBUFFER_BASE, &state.gx_layout_scratch)?;
    } else {
        let row = state.framebuffer.stride_bytes() as usize;
        let pitch = state.guest_fb_pitch.max(row as u32) as usize;
        for (i, bytes) in pixels.chunks_exact(row).enumerate() {
            cpu.write_mem(
                crate::SYNTHETIC_FRAMEBUFFER_BASE + (i * pitch) as u32,
                bytes,
            )?;
        }
    }
    state.gx_last_pushed_counter = state.framebuffer.frame_counter;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::deferred::MaterializedPixels;
    #[derive(Debug)]
    struct Pattern;
    impl CpuPixelSource for Pattern {
        fn logical_size(&self) -> (u32, u32) {
            (3, 2)
        }
        fn native_size(&self) -> (u32, u32) {
            (3, 2)
        }
        fn materialize(&self) -> Result<MaterializedPixels, String> {
            Ok(MaterializedPixels {
                logical_size: (3, 2),
                native_size: (3, 2),
                rgb565: (1u16..=6).flat_map(u16::to_le_bytes).collect(),
                native_rgba: None,
            })
        }
    }
    #[test]
    fn deferred_gapi_layout_preserves_scanlines_and_physical_columns() {
        for (columns, expected) in [
            (false, vec![1u16, 2, 3, 4, 5, 6]),
            (true, vec![4, 1, 5, 2, 6, 3]),
        ] {
            let source = Pixels {
                source: Arc::new(Pattern),
                width: 3,
                height: 2,
                columns,
            };
            let bytes = source.materialize().unwrap();
            let words: Vec<_> = bytes
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect();
            assert_eq!(words, expected);
        }
        let wrong = Pixels {
            source: Arc::new(Pattern),
            width: 4,
            height: 2,
            columns: false,
        };
        assert!(wrong.materialize().is_err());
    }
}
