//! Host recording uses the existing FFmpeg build; guest media decoding is unchanged.
#[cfg(feature = "video-static")]
use std::{
    ffi::{c_char, c_int, c_void, CString},
    ptr::NonNull,
};
use std::{io, path::Path};
#[cfg(feature = "video-static")]
extern "C" {
    fn pocket_recording_available() -> c_int;
    fn pocket_recording_missing() -> c_int;
    fn pocket_recording_open(
        path: *const c_char,
        w: c_int,
        h: c_int,
        fps: c_int,
        quality: c_int,
        error: *mut c_int,
    ) -> *mut c_void;
    fn pocket_recording_video(r: *mut c_void, rgba: *const u8, pts: i64) -> c_int;
    fn pocket_recording_audio(r: *mut c_void, pcm: *const i16, frames: u32) -> c_int;
    fn pocket_recording_close(r: *mut c_void) -> c_int;
    fn pocket_recording_error(error: c_int, buffer: *mut c_char, size: u32);
}
pub fn available() -> bool {
    #[cfg(feature = "video-static")]
    {
        unsafe { pocket_recording_available() != 0 }
    }
    #[cfg(not(feature = "video-static"))]
    {
        false
    }
}
pub fn unavailable_reason() -> String {
    #[cfg(feature = "video-static")]
    {
        let missing = unsafe { pocket_recording_missing() };
        let names: Vec<_> = [(1, "MPEG-4 encoder"), (2, "AAC encoder"), (4, "MP4 muxer")]
            .into_iter().filter_map(|(bit, name)| (missing & bit != 0).then_some(name)).collect();
        if names.is_empty() { "available".into() }
        else { format!("{} missing from linked FFmpeg; rebuild FFmpeg, then PocketHLE", names.join(", ")) }
    }
    #[cfg(not(feature = "video-static"))]
    { "PocketHLE was compiled without video-static".into() }
}
pub struct Mp4Encoder {
    #[cfg(feature = "video-static")]
    handle: Option<NonNull<c_void>>,
    width: u32,
    height: u32,
    frame: i64,
}
impl Mp4Encoder {
    pub fn open(path: &Path, width: u32, height: u32, fps: u32, quality: u8) -> io::Result<Self> {
        #[cfg(feature = "video-static")]
        {
            let path = CString::new(path.to_string_lossy().as_bytes()).map_err(io::Error::other)?;
            let mut error = 0;
            let handle = NonNull::new(unsafe {
                pocket_recording_open(
                    path.as_ptr(),
                    width as i32,
                    height as i32,
                    fps as i32,
                    quality as i32,
                    &mut error,
                )
            })
            .ok_or_else(|| failure(error))?;
            Ok(Self {
                handle: Some(handle),
                width,
                height,
                frame: 0,
            })
        }
        #[cfg(not(feature = "video-static"))]
        {
            let _ = (path, width, height, fps, quality);
            Err(io::Error::other(
                "MP4 recording requires the video-static FFmpeg build",
            ))
        }
    }
    pub fn frame(&mut self, rgba: &[u8], pcm: &[u8]) -> io::Result<()> {
        if rgba.len() != self.width as usize * self.height as usize * 4 || pcm.len() % 4 != 0 {
            return Err(io::Error::other("Invalid recording buffers"));
        }
        #[cfg(feature = "video-static")]
        {
            let ptr = self
                .handle
                .ok_or_else(|| io::Error::other("Encoder is closed"))?
                .as_ptr();
            check(unsafe { pocket_recording_video(ptr, rgba.as_ptr(), self.frame) })?;
            let samples: Vec<i16> = pcm
                .chunks_exact(2)
                .map(|p| i16::from_le_bytes([p[0], p[1]]))
                .collect();
            check(unsafe {
                pocket_recording_audio(ptr, samples.as_ptr(), (samples.len() / 2) as u32)
            })?;
        }
        self.frame += 1;
        Ok(())
    }
    pub fn finish(&mut self) -> io::Result<()> {
        #[cfg(feature = "video-static")]
        {
            if let Some(handle) = self.handle.take() {
                check(unsafe { pocket_recording_close(handle.as_ptr()) })?;
            }
        }
        Ok(())
    }
}
impl Drop for Mp4Encoder {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
#[cfg(feature = "video-static")]
fn check(rc: i32) -> io::Result<()> {
    if rc < 0 {
        Err(failure(rc))
    } else {
        Ok(())
    }
}
#[cfg(feature = "video-static")]
fn failure(rc: i32) -> io::Error {
    let mut text = [0i8; 256];
    unsafe { pocket_recording_error(rc, text.as_mut_ptr(), 256) };
    let detail = unsafe { std::ffi::CStr::from_ptr(text.as_ptr()) }.to_string_lossy();
    io::Error::other(format!(
        "FFmpeg recording: {detail} ({rc}); MPEG-4/AAC encoders and MP4 muxer must be enabled"
    ))
}
