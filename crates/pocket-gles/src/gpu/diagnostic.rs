//! Optional ES timer queries, confined to the owning current context.
//! Samples draw submissions only. A full ring skips samples, never waits.
use super::{Gl, Host};
use crate::graphics_diagnostic::{self, Profiler, Stage};
use std::{cell::RefCell, ffi::CStr, rc::Rc, time::Duration};
type Generate = unsafe extern "system" fn(i32, *mut u32);
type Delete = unsafe extern "system" fn(i32, *const u32);
type Begin = unsafe extern "system" fn(u32, u32);
type End = unsafe extern "system" fn(u32);
type Available = unsafe extern "system" fn(u32, u32, *mut u32);
type Result64 = unsafe extern "system" fn(u32, u32, *mut u64);
const ELAPSED: u32 = 0x88bf;
const POOL: usize = 16;
const SAMPLE_EVERY: u8 = 16;
pub(super) struct Timer {
    delete: Delete,
    begin: Begin,
    end: End,
    available: Available,
    result: Result64,
    get_integer: unsafe extern "system" fn(u32, *mut i32),
    ids: [u32; POOL],
    pending: [bool; POOL],
    next_sample: u8,
    profiler: Profiler,
}
impl Timer {
    pub(super) unsafe fn new(host: &Host, gl: &Gl) -> Option<Rc<RefCell<Self>>> {
        if !graphics_diagnostic::enabled() {
            return None;
        }
        if !host.es {
            log::info!("GPU_DIAG domain=guest_gpu timer=unavailable reason=non_ES_host");
            return None;
        }
        let ext = (gl.GetString)(0x1f03);
        let supported = !ext.is_null()
            && CStr::from_ptr(ext.cast())
                .to_string_lossy()
                .split_whitespace()
                .any(|e| e == "GL_EXT_disjoint_timer_query");
        if !supported {
            log::info!(
                "GPU_DIAG domain=guest_gpu timer=unavailable reason=no_EXT_disjoint_timer_query"
            );
            return None;
        }
        let names = [
            c"glGenQueriesEXT",
            c"glDeleteQueriesEXT",
            c"glBeginQueryEXT",
            c"glEndQueryEXT",
            c"glGetQueryObjectuivEXT",
            c"glGetQueryObjectui64vEXT",
            c"glGetQueryivEXT",
        ];
        let p = names.map(|n| host.load(n));
        if p.iter().any(|p| p.is_null()) {
            log::info!("GPU_DIAG domain=guest_gpu timer=unavailable reason=missing_entry_point");
            return None;
        }
        let get_query: unsafe extern "system" fn(u32, u32, *mut i32) = std::mem::transmute(p[6]);
        let mut bits = 0;
        get_query(ELAPSED, 0x8864, &mut bits);
        if bits == 0 {
            log::info!("GPU_DIAG domain=guest_gpu timer=unavailable reason=zero_counter_bits");
            return None;
        }
        let generate: Generate = std::mem::transmute(p[0]);
        let mut ids = [0; POOL];
        generate(POOL as i32, ids.as_mut_ptr());
        let mut disjoint = 0;
        (gl.GetIntegerv)(0x8fbb, &mut disjoint);
        log::info!("GPU_DIAG domain=guest_gpu timer=EXT_disjoint_timer_query pool={} coverage=draw_samples sample_every={}", POOL, SAMPLE_EVERY);
        Some(Rc::new(RefCell::new(Self {
            delete: std::mem::transmute::<*mut std::ffi::c_void, Delete>(p[1]),
            begin: std::mem::transmute::<*mut std::ffi::c_void, Begin>(p[2]),
            end: std::mem::transmute::<*mut std::ffi::c_void, End>(p[3]),
            available: std::mem::transmute::<*mut std::ffi::c_void, Available>(p[4]),
            result: std::mem::transmute::<*mut std::ffi::c_void, Result64>(p[5]),
            get_integer: gl.GetIntegerv,
            ids,
            pending: [false; POOL],
            next_sample: 0,
            profiler: Profiler::new(true, "guest_gpu"),
        })))
    }
    pub(super) fn scope(timer: &Option<Rc<RefCell<Self>>>) -> Option<Scope> {
        let timer = timer.as_ref()?;
        let mut t = timer.borrow_mut();
        let sample = t.next_sample == 0;
        t.next_sample = (t.next_sample + 1) % SAMPLE_EVERY;
        if !sample {
            return None;
        }
        unsafe {
            let mut disjoint = 0;
            (t.get_integer)(0x8fbb, &mut disjoint);
            if disjoint != 0 {
                // Invalidate all outstanding results; names remain reusable.
                t.pending.fill(false);
                t.profiler.record(Stage::GpuDisjoint, Duration::ZERO);
            }
            for i in 0..POOL {
                if !t.pending[i] {
                    continue;
                }
                let mut ready = 0;
                (t.available)(t.ids[i], 0x8867, &mut ready);
                if ready == 0 {
                    continue;
                }
                let mut ns = 0;
                (t.result)(t.ids[i], 0x8866, &mut ns);
                t.pending[i] = false;
                t.profiler.record(Stage::GpuDraw, Duration::from_nanos(ns));
            }
            let Some(i) = t.pending.iter().position(|p| !p) else {
                t.profiler.record(Stage::GpuSkipped, Duration::ZERO);
                return None;
            };
            (t.begin)(ELAPSED, t.ids[i]);
            Some(Scope {
                timer: Rc::clone(timer),
                index: i,
            })
        }
    }
    /// Called only after the renderer has made its context current at teardown.
    pub(super) unsafe fn release(&mut self) {
        (self.delete)(POOL as i32, self.ids.as_ptr());
        self.ids.fill(0);
    }
}
pub(super) struct Scope {
    timer: Rc<RefCell<Timer>>,
    index: usize,
}
impl Drop for Scope {
    fn drop(&mut self) {
        let mut t = self.timer.borrow_mut();
        unsafe {
            (t.end)(ELAPSED);
        }
        let i = self.index;
        t.pending[i] = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Driver {
        ready: bool,
        disjoint: bool,
        begins: usize,
        ends: usize,
        reads: usize,
        deletes: usize,
    }
    thread_local! { static DRIVER: RefCell<Driver> = RefCell::new(Driver::default()); }
    unsafe extern "system" fn delete(n: i32, _: *const u32) {
        DRIVER.with(|d| d.borrow_mut().deletes += n as usize);
    }
    unsafe extern "system" fn begin(_: u32, _: u32) {
        DRIVER.with(|d| d.borrow_mut().begins += 1);
    }
    unsafe extern "system" fn end(_: u32) {
        DRIVER.with(|d| d.borrow_mut().ends += 1);
    }
    unsafe extern "system" fn available(_: u32, _: u32, out: *mut u32) {
        *out = DRIVER.with(|d| d.borrow().ready as u32);
    }
    unsafe extern "system" fn result(_: u32, _: u32, out: *mut u64) {
        DRIVER.with(|d| {
            let mut d = d.borrow_mut();
            assert!(d.ready, "must never wait for an unavailable result");
            d.reads += 1;
        });
        *out = 1_000;
    }
    unsafe extern "system" fn integer(_: u32, out: *mut i32) {
        *out = DRIVER.with(|d| d.borrow().disjoint as i32);
    }
    fn timer() -> Option<Rc<RefCell<Timer>>> {
        DRIVER.with(|d| *d.borrow_mut() = Driver::default());
        Some(Rc::new(RefCell::new(Timer {
            delete,
            begin,
            end,
            available,
            result,
            get_integer: integer,
            ids: std::array::from_fn(|i| i as u32 + 1),
            pending: [false; POOL],
            next_sample: 0,
            profiler: Profiler::new(true, "test"),
        })))
    }
    fn sample(t: &Option<Rc<RefCell<Timer>>>) -> Option<Scope> {
        t.as_ref().unwrap().borrow_mut().next_sample = 0;
        Timer::scope(t)
    }
    #[test]
    fn unsampled_draws_issue_no_query_calls() {
        let t = timer();
        drop(Timer::scope(&t).unwrap());
        for _ in 1..SAMPLE_EVERY {
            assert!(Timer::scope(&t).is_none());
        }
        DRIVER.with(|d| assert_eq!(d.borrow().begins, 1));
        drop(Timer::scope(&t).unwrap());
        DRIVER.with(|d| assert_eq!(d.borrow().begins, 2));
    }
    #[test]
    fn full_pool_skips_without_reading_or_waiting() {
        let t = timer();
        for _ in 0..POOL {
            drop(sample(&t).unwrap());
        }
        assert!(sample(&t).is_none());
        DRIVER.with(|d| {
            let d = d.borrow();
            assert_eq!((d.begins, d.ends, d.reads), (POOL, POOL, 0));
        });
        DRIVER.with(|d| d.borrow_mut().ready = true);
        drop(sample(&t).unwrap());
        DRIVER.with(|d| assert_eq!(d.borrow().reads, POOL));
    }
    #[test]
    fn disjoint_discards_results_and_reuses_pool() {
        let t = timer();
        for _ in 0..POOL {
            drop(sample(&t).unwrap());
        }
        DRIVER.with(|d| {
            let mut d = d.borrow_mut();
            d.disjoint = true;
            d.ready = true;
        });
        drop(sample(&t).unwrap());
        DRIVER.with(|d| assert_eq!(d.borrow().reads, 0));
        unsafe {
            t.as_ref().unwrap().borrow_mut().release();
        }
        DRIVER.with(|d| assert_eq!(d.borrow().deletes, POOL));
    }
    #[test]
    fn absent_timer_issues_no_query() {
        assert!(Timer::scope(&None).is_none());
    }
}
