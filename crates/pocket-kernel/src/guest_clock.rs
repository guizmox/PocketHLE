//! Monotonic guest clocks. A restored session resumes both counters from the
//! checkpoint so an absolute guest deadline remains valid across host restarts.

use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::sync::OnceLock;
use std::time::Instant;

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct GuestClockSnapshot {
    tick_ms: u32,
    performance_us: u64,
}

#[derive(Clone, Copy)]
struct RestoredEpoch {
    snapshot: GuestClockSnapshot,
    resumed: Instant,
}

thread_local! {
    static RESTORED: Cell<Option<RestoredEpoch>> = const { Cell::new(None) };
}

pub fn tick_ms() -> u32 {
    RESTORED.with(|slot| {
        if let Some(epoch) = slot.get() {
            epoch
                .snapshot
                .tick_ms
                .wrapping_add(epoch.resumed.elapsed().as_millis() as u32)
        } else {
            static START: OnceLock<Instant> = OnceLock::new();
            START.get_or_init(Instant::now).elapsed().as_millis() as u32
        }
    })
}

pub fn performance_micros() -> u64 {
    RESTORED.with(|slot| {
        if let Some(epoch) = slot.get() {
            epoch
                .snapshot
                .performance_us
                .saturating_add(epoch.resumed.elapsed().as_micros() as u64)
        } else {
            static START: OnceLock<Instant> = OnceLock::new();
            START.get_or_init(Instant::now).elapsed().as_micros() as u64
        }
    })
}

impl GuestClockSnapshot {
    pub fn capture() -> Self {
        Self {
            tick_ms: tick_ms(),
            performance_us: performance_micros(),
        }
    }

    /// Appropriate for the run loop's execution thread. A nested guest
    /// session can install its own clock while retaining the parent epoch.
    pub fn run<R>(self, resume: impl FnOnce() -> R) -> R {
        self.run_at(Instant::now(), resume)
    }

    /// Restored CPUs use one device epoch even if host scheduling or audio
    /// initialization lets them enter their run loops at different instants.
    pub fn run_at<R>(self, resumed: Instant, resume: impl FnOnce() -> R) -> R {
        struct Restore(Option<RestoredEpoch>);
        impl Drop for Restore {
            fn drop(&mut self) {
                RESTORED.with(|slot| slot.set(self.0));
            }
        }

        let epoch = RestoredEpoch {
            snapshot: self,
            resumed,
        };
        let previous = RESTORED.with(|slot| slot.replace(Some(epoch)));
        let _restore = Restore(previous);
        resume()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_resume_epoch_counts_time_before_a_late_cpu_starts() {
        let snapshot = GuestClockSnapshot {
            tick_ms: 500_000,
            performance_us: 4_000_000_000,
        };
        let resumed = Instant::now() - std::time::Duration::from_millis(30);
        snapshot.run_at(resumed, || {
            assert!(tick_ms() >= 500_030);
            assert!(performance_micros() >= 4_000_030_000);
        });
        std::thread::spawn(move || {
            snapshot.run_at(resumed, || {
                assert!(tick_ms() >= 500_030);
                assert!(performance_micros() >= 4_000_030_000);
            })
        })
        .join()
        .unwrap();
    }
    #[test]
    fn restored_guest_clock_rebases_both_counters_on_this_thread() {
        let snapshot = GuestClockSnapshot {
            tick_ms: 500_000,
            performance_us: 4_000_000_000,
        };
        assert!(performance_micros() < snapshot.performance_us);
        snapshot.run(|| {
            assert!(tick_ms() >= snapshot.tick_ms);
            let before = performance_micros();
            assert!(before >= snapshot.performance_us);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(performance_micros() >= before + 4000);
        });
        assert!(performance_micros() < snapshot.performance_us);
    }
}
