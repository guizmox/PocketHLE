//! Frontend-neutral session services. UI, JNI and presentation stay in frontends.
use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    Arc, Mutex,
};

#[derive(Clone)]
pub struct Control {
    pub stop: Arc<AtomicBool>,
    pub error: Arc<Mutex<Option<String>>>,
    pub foreground: Arc<AtomicU32>,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            error: Arc::new(Mutex::new(None)),
            foreground: Arc::new(AtomicU32::new(1)),
        }
    }
}
#[derive(Clone)]
pub struct LaunchContext {
    pub library: std::path::PathBuf,
    pub game: pocket_library::GameEntry,
    pub card: std::path::PathBuf,
    pub config: pocket_library::LauncherConfig,
}
pub mod compatibility;
pub mod input;
#[cfg(feature = "snapshots")]
pub mod snapshot;

impl Control {
    /// Preserve the first process failure before notifying all sibling jobs.
    pub fn fail(&self, error: String) {
        self.error
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get_or_insert(error);
        self.stop.store(true, std::sync::atomic::Ordering::Release);
    }
}

pub mod visual;
