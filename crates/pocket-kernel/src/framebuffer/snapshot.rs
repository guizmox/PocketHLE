//! The displayed image is part of a checkpoint. Presentation metrics and
//! host preview resources are reinitialized by the receiving frontend.

use super::*;
use crate::framebuffer::deferred::DeferredFramebuffer;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
pub struct DisplaySnapshot {
    image: SavedImage,
    system_video: Option<SavedImage>,
    timing: DisplayTimingSnapshot,
}

#[derive(Serialize, Deserialize)]
struct DisplayTimingSnapshot {
    phase: Duration,
    last_present: Option<RelativeInstant>,
    fullscreen_owner: Option<(u32, u32)>,
    frame_rate_limit: Option<u32>,
    pending: HashMap<(usize, u32, u32), RelativeInstant>,
    recent_blank: HashMap<usize, RelativeInstant>,
    paced_primary_locks: HashSet<u32>,
    primary_locks: HashSet<u32>,
    overlay: Option<DirectDrawOverlay>,
    surface_dcs: HashMap<u32, (u32, u32)>,
}

#[derive(Serialize, Deserialize)]
enum RelativeInstant {
    Past(Duration),
    Future(Duration),
}

impl RelativeInstant {
    fn capture(time: Instant, now: Instant) -> Self {
        if time <= now {
            Self::Past(now.duration_since(time))
        } else {
            Self::Future(time.duration_since(now))
        }
    }

    fn restore(self, now: Instant) -> Result<Instant, String> {
        match self {
            Self::Past(duration) => now.checked_sub(duration),
            Self::Future(duration) => now.checked_add(duration),
        }
        .ok_or_else(|| "invalid saved DirectDraw time".into())
    }
}

impl DisplayTimingSnapshot {
    fn capture(timing: &DirectDrawTiming) -> Self {
        let now = Instant::now();
        let phase = now.saturating_duration_since(timing.epoch).as_nanos()
            % DirectDrawTiming::PERIOD.as_nanos();
        Self {
            phase: Duration::from_nanos(phase as u64),
            last_present: timing
                .last_present
                .map(|time| RelativeInstant::capture(time, now)),
            fullscreen_owner: timing.fullscreen_owner,
            frame_rate_limit: timing.frame_rate_limit,
            pending: timing
                .pending
                .iter()
                .map(|(&key, &time)| (key, RelativeInstant::capture(time, now)))
                .collect(),
            recent_blank: timing
                .recent_blank
                .iter()
                .map(|(&key, &time)| (key, RelativeInstant::capture(time, now)))
                .collect(),
            paced_primary_locks: timing.paced_primary_locks.clone(),
            primary_locks: timing.primary_locks.clone(),
            overlay: timing.overlay,
            surface_dcs: timing.surface_dcs.clone(),
        }
    }

    fn restore(self) -> Result<DirectDrawTiming, String> {
        let now = Instant::now();
        let epoch = now
            .checked_sub(self.phase)
            .ok_or("invalid saved DirectDraw phase")?;
        Ok(DirectDrawTiming {
            epoch,
            last_present: self
                .last_present
                .map(|time| time.restore(now))
                .transpose()?,
            fullscreen_owner: self.fullscreen_owner,
            frame_rate_limit: self.frame_rate_limit,
            pending: self
                .pending
                .into_iter()
                .map(|(key, time)| Ok((key, time.restore(now)?)))
                .collect::<Result<_, String>>()?,
            recent_blank: self
                .recent_blank
                .into_iter()
                .map(|(key, time)| Ok((key, time.restore(now)?)))
                .collect::<Result<_, String>>()?,
            paced_primary_locks: self.paced_primary_locks,
            primary_locks: self.primary_locks,
            overlay: self.overlay,
            surface_dcs: self.surface_dcs,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct SavedImage {
    width: u32,
    height: u32,
    frame_counter: u64,
    pixels: Vec<u8>,
    backlight: u32,
    render: Option<RenderPresentation>,
}

impl SavedImage {
    fn capture(fb: &Framebuffer) -> Self {
        Self {
            width: fb.width,
            height: fb.height,
            frame_counter: fb.frame_counter,
            pixels: fb.pixels.clone(),
            backlight: fb.backlight_level,
            render: fb.render_presentation.clone(),
        }
    }

    fn restore(self) -> Result<Framebuffer, String> {
        fn valid(w: u32, h: u32, bytes: usize, pixel: usize) -> bool {
            w != 0
                && h != 0
                && (w as usize)
                    .checked_mul(h as usize)
                    .and_then(|count| count.checked_mul(pixel))
                    == Some(bytes)
        }
        if !valid(self.width, self.height, self.pixels.len(), 2)
            || self.render.as_ref().is_some_and(|render| {
                !valid(
                    render.width,
                    render.height,
                    render.pixels.len(),
                    if render.rgba { 4 } else { 2 },
                )
            })
        {
            return Err("invalid saved emulator display".into());
        }
        Ok(Framebuffer {
            metadata: FramebufferMetadata {
                width: self.width,
                height: self.height,
                bpp: FB_BPP,
                frame_counter: self.frame_counter,
                presentation_stats: Default::default(),
                directdraw: Default::default(),
            },
            pixels: self.pixels,
            backlight_level: self.backlight,
            render_presentation: self.render,
        })
    }
}

impl DeferredFramebuffer {
    pub fn capture_snapshot(&mut self) -> Result<DisplaySnapshot, String> {
        let timing = DisplayTimingSnapshot::capture(&self.directdraw);
        let system_video = self.system_video_for_snapshot().map(SavedImage::capture);
        Ok(DisplaySnapshot {
            image: SavedImage::capture(self.cpu()?),
            system_video,
            timing,
        })
    }
}

impl DisplaySnapshot {
    pub fn restore(self) -> Result<DeferredFramebuffer, String> {
        let mut result = DeferredFramebuffer::new(self.image.restore()?)?;
        result.directdraw = self.timing.restore()?;
        result.replace_system_video(self.system_video.map(SavedImage::restore).transpose()?);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_snapshot_preserves_directdraw_cooperative_state_and_waits() {
        let mut framebuffer = DeferredFramebuffer::default();
        let now = Instant::now();
        framebuffer.directdraw.fullscreen_owner = Some((123, 456));
        framebuffer
            .directdraw
            .pending
            .insert((0, 123, 456), now + Duration::from_millis(50));
        framebuffer.directdraw.primary_locks.insert(321);
        let saved = framebuffer.capture_snapshot().unwrap();
        std::thread::sleep(Duration::from_millis(60));
        let restored = saved.restore().unwrap();
        assert_eq!(restored.directdraw.fullscreen_owner, Some((123, 456)));
        assert!(restored.directdraw.pending[&(0, 123, 456)] > Instant::now());
        assert!(restored.directdraw.primary_locks.contains(&321));
    }

    #[test]
    fn display_snapshot_preserves_pixels_and_guest_resolution() {
        let mut framebuffer = DeferredFramebuffer::new(Framebuffer::new(2, 3)).unwrap();
        framebuffer.cpu_mut().unwrap().put_pixel(1, 2, 0x07e0);
        framebuffer.mark_dirty();
        let count = framebuffer.frame_counter;
        let snapshot = framebuffer.capture_snapshot().unwrap();
        let mut restored = snapshot.restore().unwrap();
        assert_eq!(restored.logical_size(), (2, 3));
        assert_eq!(restored.frame_counter, count);
        assert_eq!(&restored.cpu().unwrap().pixels[10..], &[0xe0, 7]);
    }
}
