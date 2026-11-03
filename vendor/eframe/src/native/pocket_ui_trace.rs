//! Temporary opt-in CPU wall-time tracing. No GL calls or scheduling changes.
use std::{
    cell::RefCell,
    sync::OnceLock,
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
pub enum Phase {
    Frame,
    Event,
    Context,
    Update,
    Prepare,
    Paint,
    Swap,
    Acquire,
    Clear,
}
const NAMES: [&str; 9] = [
    "frame", "event", "context", "update", "prepare", "paint", "swap", "acquire", "clear",
];
#[derive(Clone, Copy, Default)]
struct Stat {
    calls: u64,
    total: Duration,
    max: Duration,
}
impl Stat {
    fn add(&mut self, elapsed: Duration) {
        self.calls += 1;
        self.total += elapsed;
        self.max = self.max.max(elapsed);
    }
}
struct State {
    since: Instant,
    stats: [Stat; NAMES.len()],
    input: [u64; 6],
    rebinds: u64,
    last_frame: Option<Instant>,
    max_gap: Duration,
}
impl State {
    fn new(now: Instant) -> Self {
        Self {
            since: now,
            stats: [Stat::default(); NAMES.len()],
            input: [0; 6],
            rebinds: 0,
            last_frame: None,
            max_gap: Duration::ZERO,
        }
    }
    fn frame_started(&mut self, now: Instant) {
        if let Some(last) = self.last_frame {
            self.max_gap = self.max_gap.max(now.saturating_duration_since(last));
        }
        self.last_frame = Some(now);
    }
    fn summary(&self, now: Instant) -> String {
        let phases = self
            .stats
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "{}={}/{:.2}/{:.2}",
                    NAMES[i],
                    s.calls,
                    s.total.as_secs_f64() * 1000.0,
                    s.max.as_secs_f64() * 1000.0
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let gap = self
            .last_frame
            .map_or(Duration::ZERO, |last| now.saturating_duration_since(last));
        format!("HOST_UI_TRACE window_ms={:.1} cursor={} raw_mouse={} keys={} key_repeats={} mouse_buttons={} other={} rebinds={} gap_max_ms={:.2} gap_now_ms={:.2} {} (phase=calls/total_ms/max_ms; nested phases overlap)", now.saturating_duration_since(self.since).as_secs_f64()*1000.0,self.input[0],self.input[1],self.input[2],self.input[3],self.input[4],self.input[5],self.rebinds,self.max_gap.as_secs_f64()*1000.0,gap.as_secs_f64()*1000.0,phases)
    }
    fn emit(&mut self, now: Instant, force: bool) {
        if force || now.saturating_duration_since(self.since) >= Duration::from_secs(1) {
            log::info!("{}", self.summary(now));
            self.since = now;
            self.stats = [Stat::default(); NAMES.len()];
            self.input = [0; 6];
            self.rebinds = 0;
            self.max_gap = Duration::ZERO;
        }
    }
}
thread_local! { static TRACE: RefCell<Option<State>> = const { RefCell::new(None) }; }
fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let enabled = std::env::var("POCKETHLE_UI_TRACE").as_deref() == Ok("1");
        if enabled {
            log::info!("HOST_UI_TRACE revision=latest-frame-v4 enabled; upstream eframe context binding restored; CPU wall times only; no added GL operation");
        }
        enabled
    })
}
pub struct Scope {
    phase: Phase,
    start: Option<Instant>,
}
pub fn scope(phase: Phase) -> Scope {
    let start = enabled().then(Instant::now);
    if let Some(now) = start {
        TRACE.with(|cell| {
            let mut trace = cell.borrow_mut();
            let state = trace.get_or_insert_with(|| State::new(now));
            if matches!(phase, Phase::Frame) {
                state.frame_started(now);
            }
        });
    }
    Scope { phase, start }
}
impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let now = Instant::now();
            TRACE.with(|cell| {
                let mut trace = cell.borrow_mut();
                let state = trace.get_or_insert_with(|| State::new(start));
                state.stats[self.phase as usize].add(now.saturating_duration_since(start));
                state.emit(now, false);
            });
        }
    }
}
pub fn event<T: 'static>(event: &winit::event::Event<T>) {
    if !enabled() {
        return;
    }
    use winit::event::{DeviceEvent, Event, WindowEvent};
    let (kind, repeat) = match event {
        Event::WindowEvent {
            event: WindowEvent::CursorMoved { .. },
            ..
        } => (0, false),
        Event::DeviceEvent {
            event: DeviceEvent::MouseMotion { .. },
            ..
        } => (1, false),
        Event::WindowEvent {
            event: WindowEvent::KeyboardInput { event, .. },
            ..
        } => (2, event.repeat),
        Event::WindowEvent {
            event: WindowEvent::MouseInput { .. },
            ..
        } => (4, false),
        _ => (5, false),
    };
    let now = Instant::now();
    TRACE.with(|cell| {
        let mut trace = cell.borrow_mut();
        let state = trace.get_or_insert_with(|| State::new(now));
        state.input[kind] += 1;
        if repeat {
            state.input[3] += 1;
        }
        state.emit(now, false);
    });
}
pub fn rebound() {
    if !enabled() {
        return;
    }
    TRACE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state.rebinds += 1;
        }
    });
}
pub fn flush() {
    if !enabled() {
        return;
    }
    TRACE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state.emit(Instant::now(), true);
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_separates_phase_cost_and_input_and_keeps_idle_gap() {
        let start = Instant::now();
        let mut state = State::new(start);
        state.frame_started(start);
        state.frame_started(start + Duration::from_millis(50));
        state.stats[Phase::Paint as usize].add(Duration::from_millis(20));
        state.stats[Phase::Paint as usize].add(Duration::from_millis(30));
        state.input[3] = 12;
        let line = state.summary(start + Duration::from_secs(1));
        assert!(line.contains("paint=2/50.00/30.00"));
        assert_eq!(Phase::Clear as usize + 1, NAMES.len());
        assert!(line.contains("key_repeats=12"));
        assert!(line.contains("gap_now_ms=950.00"));
    }
    #[test]
    fn disabled_scope_does_not_initialize_state() {
        TRACE.with(|state| *state.borrow_mut() = None);
        drop(Scope {
            phase: Phase::Paint,
            start: None,
        });
        TRACE.with(|state| assert!(state.borrow().is_none()));
    }
}
