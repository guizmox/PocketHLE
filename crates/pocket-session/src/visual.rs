//! Host UI state does not manufacture image publications or trigger readback.
use std::sync::atomic::{AtomicU32, Ordering};
#[derive(Debug, Default)]
pub struct VisualState {
    cursor: AtomicU32,
}
impl VisualState {
    pub fn set_cursor(&self, handle: u32) {
        self.cursor.store(handle, Ordering::Release);
    }
    pub fn waiting(&self) -> bool {
        matches!(
            pocket_core::kernel::controls::system_cursor(self.cursor()),
            Some(
                pocket_core::kernel::controls::SystemCursor::Wait
                    | pocket_core::kernel::controls::SystemCursor::Progress
            )
        )
    }
    pub fn cursor(&self) -> u32 {
        self.cursor.load(Ordering::Acquire)
    }
}
