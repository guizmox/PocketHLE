//! Opt-in elapsed-time diagnostics. CPU spans are inclusive, not GPU timers.
use std::cell::RefCell;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Stage {
    GuestDraw,
    Submit,
    ReadPixels,
    Materialize,
    Pack,
    Jni,
    GpuDraw,
    GpuSkipped,
    GpuDisjoint,
    GuestClear,
    Swap,
}
const NAMES: [&str; 11] = [
    "guest_draw_cpu",
    "submit_cpu",
    "read_pixels_wall",
    "materialize_wall",
    "frame_pack_cpu",
    "jni_copy_wall",
    "guest_draw_gpu_sample",
    "gpu_skipped",
    "gpu_disjoint",
    "guest_clear_cpu",
    "egl_swap_wall",
];
#[derive(Clone, Copy, Default)]
struct Sample {
    calls: u64,
    total: Duration,
    max: Duration,
}

pub struct Profiler {
    domain: &'static str,
    start: Option<Instant>,
    samples: [Sample; NAMES.len()],
}
impl Profiler {
    pub fn new(enabled: bool, domain: &'static str) -> Self {
        Self {
            domain,
            start: enabled.then(Instant::now),
            samples: [Sample::default(); NAMES.len()],
        }
    }
    pub fn enabled(&self) -> bool {
        self.start.is_some()
    }
    pub fn record(&mut self, stage: Stage, elapsed: Duration) {
        let Some(start) = self.start else { return };
        let sample = &mut self.samples[stage as usize];
        sample.calls += 1;
        sample.total += elapsed;
        sample.max = sample.max.max(elapsed);
        let now = Instant::now();
        let window = now.duration_since(start);
        if window >= Duration::from_secs(1) {
            for (name, sample) in NAMES.iter().zip(&self.samples) {
                if sample.calls != 0 {
                    log::info!("GPU_DIAG domain={} window_ms={:.1} stage={} calls={} total_ms={:.3} mean_ms={:.3} max_ms={:.3}", self.domain, window.as_secs_f64()*1000., name, sample.calls, sample.total.as_secs_f64()*1000., sample.total.as_secs_f64()*1000./sample.calls as f64, sample.max.as_secs_f64()*1000.);
                }
            }
            self.samples.fill(Sample::default());
            self.start = Some(now);
        }
    }
}
thread_local! {
    static PROFILER: RefCell<Profiler> = RefCell::new(Profiler::new(false, "emulation"));
}
pub fn enabled() -> bool {
    PROFILER.with(|p| p.borrow().enabled())
}
pub fn record(stage: Stage, elapsed: Duration) {
    PROFILER.with(|p| p.borrow_mut().record(stage, elapsed));
}
/// Restores the previous thread-local state on normal return and unwind.
pub fn with_enabled<R>(enabled: bool, run: impl FnOnce() -> R) -> R {
    struct Restore(Option<Profiler>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PROFILER.with(|p| {
                p.replace(self.0.take().unwrap());
            });
        }
    }
    let _restore = Restore(Some(
        PROFILER.with(|p| p.replace(Profiler::new(enabled, "emulation"))),
    ));
    run()
}
pub struct Span(Option<(Stage, Instant)>);
impl Span {
    pub fn new(stage: Stage) -> Self {
        Self(enabled().then(|| (stage, Instant::now())))
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some((stage, start)) = self.0 {
            record(stage, start.elapsed());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_collects_nothing() {
        let mut p = Profiler::new(false, "test");
        p.record(Stage::Jni, Duration::from_millis(10));
        assert_eq!(p.samples[Stage::Jni as usize].calls, 0);
    }
    #[test]
    fn aggregates_count_total_max_and_resets_after_one_second() {
        let mut p = Profiler::new(true, "test");
        p.record(Stage::Jni, Duration::from_millis(2));
        p.record(Stage::Jni, Duration::from_millis(3));
        let s = p.samples[Stage::Jni as usize];
        assert_eq!(
            (s.calls, s.total, s.max),
            (2, Duration::from_millis(5), Duration::from_millis(3))
        );
        p.start = Some(Instant::now() - Duration::from_secs(1));
        p.record(Stage::Jni, Duration::ZERO);
        assert_eq!(p.samples[Stage::Jni as usize].calls, 0);
    }
    #[test]
    fn nested_scopes_and_unwind_restore_opt_in() {
        assert!(!enabled());
        with_enabled(true, || {
            assert!(enabled());
            let _ = std::panic::catch_unwind(|| {
                with_enabled(false, || {
                    assert!(!enabled());
                    panic!("test");
                })
            });
            assert!(enabled());
        });
        assert!(!enabled());
    }
}
