//! Guest WinCE thread scheduler state and asynchronous continuations.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct ScheduleSnapshot {
    main_thread: MainThreadState,
    threads: Vec<GuestThread>,
    current_thread: usize,
    thread_last_errors: HashMap<usize, u32>,
    thread_proc_permissions: HashMap<usize, u32>,
    worker_schedule_cursor: usize,
    worker_round_seen: Vec<usize>,
    worker_preempt_after_ms: u64,
    wait_retry_delay: Option<std::time::Duration>,
    guest_fpscr: HashMap<usize, u32>,
    timer_period_requests: std::collections::BTreeMap<u32, u32>,
    critical_sections: HashMap<u32, (usize, u32)>,
    tls_slots_used: u64,
    tls_owner: usize,
    multimedia_timers: HashMap<u32, MultimediaTimer>,
    crt_new_handler: u32,
    cpp_new_handler: u32,
    crt_new_mode: u32,
    qsort_frames: HashMap<u32, QsortFrame>,
    vector_iter_stack: Vec<VectorIterFrame>,
    crt_pushback: HashMap<u32, u8>,
    crt_stream_errors: std::collections::HashSet<u32>,
    strtok_pos: u32,
    security_cookie: u32,
    wave_out_format: GuestFormat,
    wave_out: WaveOutState,
    pending_message: Option<(u32, u32, u32, u32)>,
    posted_messages: VecDeque<(u32, u32, u32, u32)>,
    synthetic_peek_paint_yield: std::collections::HashSet<usize>,
    synthetic_message_count: u64,
    synthetic_message_budget: u64,
    synthetic_paint_next_ms: u64,
    message_frames: HashMap<usize, Vec<DispatchMessageFrame>>,
    notify_message_frames: Vec<NotifyMessageFrame>,
    wait_deadlines: Vec<((usize, u32, u32), u64)>,
    multimedia_timer_joins: Vec<((usize, u32, u32), u32)>,
    timers: Vec<((usize, u32, u32), (u32, u64, u32))>,
    tls: HashMap<usize, Vec<u32>>,
    events: HashMap<u32, EventObject>,
    semaphores: HashMap<u32, SemaphoreObject>,
    mutexes: HashMap<u32, MutexObject>,
}

impl KernelState {
    /// Capture between CPU slices, under the execution gate.
    pub fn capture_schedule(&self) -> ScheduleSnapshot {
        ScheduleSnapshot {
            main_thread: self.main_thread.clone(),
            threads: self.threads.clone(),
            current_thread: self.current_thread,
            thread_last_errors: self.thread_last_errors.clone(),
            thread_proc_permissions: self.thread_proc_permissions.clone(),
            worker_schedule_cursor: self.worker_schedule_cursor,
            worker_round_seen: self.worker_round_seen.clone(),
            worker_preempt_after_ms: self.worker_preempt_after_ms,
            wait_retry_delay: self.wait_retry_delay,
            guest_fpscr: self.guest_fpscr.clone(),
            timer_period_requests: self.timer_period_requests.clone(),
            critical_sections: self.critical_sections.clone(),
            tls_slots_used: self.tls_slots_used,
            tls_owner: self.tls_owner,
            multimedia_timers: self.multimedia_timers.clone(),
            crt_new_handler: self.crt_new_handler,
            cpp_new_handler: self.cpp_new_handler,
            crt_new_mode: self.crt_new_mode,
            qsort_frames: self.qsort_frames.clone(),
            vector_iter_stack: self.vector_iter_stack.clone(),
            crt_pushback: self.crt_pushback.clone(),
            crt_stream_errors: self.crt_stream_errors.clone(),
            strtok_pos: self.strtok_pos,
            security_cookie: self.security_cookie,
            wave_out_format: self.wave_out_format,
            wave_out: self.wave_out.clone(),
            pending_message: self.pending_message,
            posted_messages: self.posted_messages.clone(),
            synthetic_peek_paint_yield: self.synthetic_peek_paint_yield.clone(),
            synthetic_message_count: self.synthetic_message_count,
            synthetic_message_budget: self.synthetic_message_budget,
            synthetic_paint_next_ms: self.synthetic_paint_next_ms,
            message_frames: self.message_frames.clone(),
            notify_message_frames: self.notify_message_frames.clone(),
            wait_deadlines: self
                .wait_deadlines
                .iter()
                .map(|(&key, &value)| (key, value))
                .collect(),
            multimedia_timer_joins: self
                .multimedia_timer_joins
                .iter()
                .map(|(&key, &value)| (key, value))
                .collect(),
            timers: self
                .timers
                .iter()
                .map(|(&key, &value)| (key, value))
                .collect(),
            tls: self
                .thread_tls
                .iter()
                .map(|(&thread, slots)| (thread, slots.to_vec()))
                .collect(),
            events: self.events.copy_objects(),
            semaphores: self.semaphores.copy_objects(),
            mutexes: self.mutexes.copy_objects(),
        }
    }
}

impl ScheduleSnapshot {
    pub(crate) fn contains_thread(&self, thread: usize) -> bool {
        thread <= self.threads.len()
    }

    pub(crate) fn omit_device_objects(&mut self) {
        self.events.clear();
        self.semaphores.clear();
        self.mutexes.clear();
    }
    /// Reconstruct the scheduler and waitable objects in a detached kernel.
    /// The receiving process uses the saved GuestClockSnapshot epoch.
    pub fn restore(self, state: &mut KernelState) -> Result<(), KernelError> {
        let invalid = || KernelError::Loader("invalid saved guest scheduler state".into());
        if self.current_thread > self.threads.len()
            || self.tls_owner > self.threads.len()
            || self
                .worker_round_seen
                .iter()
                .any(|&thread| thread > self.threads.len())
            || self
                .semaphores
                .values()
                .any(|semaphore| semaphore.count > semaphore.max_count)
        {
            return Err(invalid());
        }
        let tls = self
            .tls
            .into_iter()
            .map(|(thread, slots)| {
                if thread > self.threads.len() {
                    return Err(invalid());
                }
                Ok((thread, slots.try_into().map_err(|_| invalid())?))
            })
            .collect::<Result<_, KernelError>>()?;

        state.main_thread = self.main_thread;
        state.threads = self.threads;
        state.current_thread = self.current_thread;
        state.thread_last_errors = self.thread_last_errors;
        state.thread_proc_permissions = self.thread_proc_permissions;
        state.worker_schedule_cursor = self.worker_schedule_cursor;
        state.worker_round_seen = self.worker_round_seen;
        state.worker_preempt_after_ms = self.worker_preempt_after_ms;
        state.wait_retry_delay = self.wait_retry_delay;
        state.guest_fpscr = self.guest_fpscr;
        state.timer_period_requests = self.timer_period_requests;
        state.critical_sections = self.critical_sections;
        state.tls_slots_used = self.tls_slots_used;
        state.tls_owner = self.tls_owner;
        state.multimedia_timers = self.multimedia_timers;
        state.crt_new_handler = self.crt_new_handler;
        state.cpp_new_handler = self.cpp_new_handler;
        state.crt_new_mode = self.crt_new_mode;
        state.qsort_frames = self.qsort_frames;
        state.vector_iter_stack = self.vector_iter_stack;
        state.crt_pushback = self.crt_pushback;
        state.crt_stream_errors = self.crt_stream_errors;
        state.strtok_pos = self.strtok_pos;
        state.security_cookie = self.security_cookie;
        state.wave_out_format = self.wave_out_format;
        state.wave_out = self.wave_out;
        state.pending_message = self.pending_message;
        state.posted_messages = self.posted_messages;
        state.synthetic_peek_paint_yield = self.synthetic_peek_paint_yield;
        state.peek_idle_observed = false;
        state.synthetic_message_count = self.synthetic_message_count;
        state.synthetic_message_budget = self.synthetic_message_budget;
        state.synthetic_paint_next_ms = self.synthetic_paint_next_ms;
        state.message_frames = self.message_frames;
        state.notify_message_frames = self.notify_message_frames;
        state.wait_deadlines = self.wait_deadlines.into_iter().collect();
        state.multimedia_timer_joins = self.multimedia_timer_joins.into_iter().collect();
        state.timers = self.timers.into_iter().collect();
        state.thread_tls = tls;
        state.events = shared_objects::SharedObjects::from_objects(self.events);
        state.semaphores = shared_objects::SharedObjects::from_objects(self.semaphores);
        state.mutexes = shared_objects::SharedObjects::from_objects(self.mutexes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_process() -> Process {
        Process::map_into(
            crate::tests::image_based_at(0x10000, 0x1000),
            &mut pocket_cpu::stub::StubCpu::new(),
            &|_, _| None,
            &NullDispatcher,
        )
        .unwrap()
    }

    #[test]
    fn scheduler_snapshot_keeps_wait_state_and_tls_of_another_thread() {
        let mut process = test_process();
        let state = &mut process.state;
        state.threads.push(GuestThread::new(
            0x2001, 7, 0x5000, 0x1000, 0, 0, 123, [0; 17],
        ));
        state.threads[0].id = 2;
        state.threads[0].parked_wait_handles.push(555);
        state.threads[0].sleep_until_ms = 60_000;
        state.wait_deadlines.insert((1, 5, 6), 59_000);
        state.events.insert(
            555,
            EventObject {
                signalled: true,
                manual_reset: false,
            },
        );
        state
            .thread_tls
            .entry(1)
            .or_insert([0; TLS_SLOT_COUNT as usize])[5] = 0x1234;

        let bytes = serde_json::to_vec(&state.capture_schedule()).unwrap();
        drop(process);
        let mut process = test_process();
        let restored = &mut process.state;
        let snapshot: ScheduleSnapshot = serde_json::from_slice(&bytes).unwrap();
        snapshot.restore(restored).unwrap();
        assert_eq!(restored.threads[0].parked_wait_handles, [555]);
        assert_eq!(restored.threads[0].sleep_until_ms, 60_000);
        assert_eq!(restored.wait_deadlines[&(1, 5, 6)], 59_000);
        assert!(restored.events.get(&555).unwrap().signalled);
        assert_eq!(restored.thread_tls[&1][5], 0x1234);
    }
}
