//! Host battery projection. No timers, device handles or subscriptions.
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub ac: u8,
    pub flags: u8,
    pub percent: u8,
    pub remaining: u32,
    pub full: u32,
}
impl Status {
    pub const UNKNOWN: Self = Self { ac: 255, flags: 255, percent: 255,
        remaining: u32::MAX, full: u32::MAX };
    pub fn ce_bytes(self) -> [u8; 56] {
        let mut b = [0; 56];
        b[0] = self.ac; b[1] = self.flags; b[2] = self.percent;
        b[4..8].copy_from_slice(&self.remaining.to_le_bytes());
        b[8..12].copy_from_slice(&self.full.to_le_bytes());
        b[13] = 128; b[14] = 255;
        b[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        b[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        b
    }
}
pub trait Backend: Send + Sync { fn read(&self) -> Option<Status>; }
struct Cache { backend: Option<Arc<dyn Backend>>, sample: Status, at: Option<Instant> }
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache { backend: None, sample: Status::UNKNOWN, at: None }))
}
pub fn install_host(backend: Arc<dyn Backend>) {
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    c.backend = Some(backend); c.at = None; c.sample = Status::UNKNOWN;
}
pub fn status(refresh: bool) -> Status {
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if refresh || c.at.map_or(true, |t| t.elapsed() >= Duration::from_secs(1)) {
        c.sample = c.backend.as_ref().map_or_else(native, |b| b.read()).unwrap_or(Status::UNKNOWN);
        c.at = Some(Instant::now());
    }
    c.sample
}
#[cfg(windows)]
fn native() -> Option<Status> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 { return None; }
    Some(Status { ac: s.ACLineStatus, flags: s.BatteryFlag, percent: s.BatteryLifePercent,
        remaining: s.BatteryLifeTime, full: s.BatteryFullLifeTime })
}
#[cfg(not(windows))]
fn native() -> Option<Status> { None }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn ce_layout() {
        let s = Status { ac: 0, flags: 4, percent: 3, remaining: 120, full: 3600 };
        let b = s.ce_bytes();
        assert_eq!(&b[..4], &[0,4,3,0]);
        assert_eq!(&b[4..8], &120u32.to_le_bytes());
        assert_eq!(&b[8..12], &3600u32.to_le_bytes());
        assert_eq!(&b[13..15], &[128,255]);
        assert_eq!(&b[16..24], &[255;8]);
        assert_eq!(&b[24..], &[0;32]);
    }
    #[test] fn zero_percent_is_valid() {
        assert_eq!(Status { percent: 0, ..Status::UNKNOWN }.ce_bytes()[2], 0);
        assert_eq!(Status::UNKNOWN.ce_bytes()[2], 255);
    }
    #[test] fn cache_and_forced_refresh() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Fake(AtomicUsize);
        impl Backend for Fake { fn read(&self) -> Option<Status> {
            Some(Status { percent: self.0.fetch_add(1, Ordering::SeqCst) as u8, ..Status::UNKNOWN })
        }}
        let f = Arc::new(Fake(AtomicUsize::new(0))); install_host(f.clone());
        assert_eq!(status(false).percent, 0); assert_eq!(status(false).percent, 0);
        assert_eq!(status(true).percent, 1); assert_eq!(f.0.load(Ordering::SeqCst), 2);
        struct Failed;
        impl Backend for Failed { fn read(&self) -> Option<Status> { None } }
        install_host(Arc::new(Failed));
        assert_eq!(status(false), Status::UNKNOWN);
        assert_eq!(Arc::strong_count(&f), 1); // Replaced backend is released.
    }
}
