//! Device-wide rendezvous for process-local checkpoint callbacks.
//!
//! A checkpoint never waits while holding the WinCE execution gate: another
//! process may need that gate to finish its current API and reach a boundary.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub struct CheckpointTime {
    pub clock: pocket_kernel::guest_clock::GuestClockSnapshot,
    pub host: Instant,
}
impl CheckpointTime {
    pub fn now() -> Self {
        Self {
            clock: pocket_kernel::guest_clock::GuestClockSnapshot::capture(),
            host: Instant::now(),
        }
    }
}

pub struct CheckpointDomain<T>(Arc<Inner<T>>);
struct Inner<T> {
    state: Mutex<State<T>>,
    requested: AtomicBool,
    changed: Condvar,
}
struct State<T> {
    members: BTreeSet<u32>,
    next: u64,
    request: Option<Request<T>>,
}
struct Request<T> {
    epoch: u64,
    time: Option<CheckpointTime>,
    leader: u32,
    members: BTreeSet<u32>,
    arrived: BTreeSet<u32>,
    completed: BTreeSet<u32>,
    values: BTreeMap<u32, T>,
    error: Option<String>,
    deadline: Instant,
    departed: BTreeSet<u32>,
}
pub struct Participant<T> {
    domain: CheckpointDomain<T>,
    pid: u32,
}
impl<T> Clone for CheckpointDomain<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Default for CheckpointDomain<T> {
    fn default() -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                members: BTreeSet::new(),
                next: 0,
                request: None,
            }),
            requested: AtomicBool::new(false),
            changed: Condvar::new(),
        }))
    }
}
impl<T> CheckpointDomain<T> {
    pub fn register(&self, pid: u32) -> Result<Participant<T>, String> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| "checkpoint domain poisoned")?;
        if pid == 0 || state.members.contains(&pid) {
            return Err("duplicate checkpoint process".into());
        }
        if let Some(request) = state.request.as_mut() {
            request
                .error
                .get_or_insert_with(|| "process started during checkpoint rendezvous".into());
            if request.arrived.is_empty() {
                state.request = None;
                self.0.requested.store(false, Ordering::Release);
            }
            self.0.changed.notify_all();
            // Do not start a new CPU while a cancelled capture is still reading
            // shared memory. A parent waiting for startup acknowledgement is
            // released as soon as the already parked callbacks have left.
            while state.request.is_some() {
                state = self
                    .0
                    .changed
                    .wait(state)
                    .map_err(|_| "checkpoint domain poisoned")?;
            }
        }
        state.members.insert(pid);
        self.0.changed.notify_all();
        Ok(Participant {
            domain: self.clone(),
            pid,
        })
    }

    pub fn request(
        &self,
        leader: u32,
        expected: impl IntoIterator<Item = u32>,
        timeout: Duration,
    ) -> Result<(), String> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| "checkpoint domain poisoned")?;
        let members: BTreeSet<_> = expected.into_iter().collect();
        if state.request.is_some() {
            return Err("another checkpoint is still in progress".into());
        }
        if members != state.members || !members.contains(&leader) {
            return Err("process startup or shutdown is still in progress".into());
        }
        state.next = state
            .next
            .checked_add(1)
            .ok_or("checkpoint epoch exhausted")?;
        state.request = Some(Request {
            epoch: state.next,
            time: None,
            leader,
            members,
            arrived: BTreeSet::new(),
            completed: BTreeSet::new(),
            values: BTreeMap::new(),
            error: None,
            deadline: Instant::now() + timeout,
            departed: BTreeSet::new(),
        });
        self.0.requested.store(true, Ordering::Release);
        self.0.changed.notify_all();
        Ok(())
    }

    pub fn requested(&self, pid: u32) -> bool {
        // Ordinary frame/input polls do not need the domain mutex. The flag
        // only skips the locked lookup; the mutex remains authoritative while
        // a capture is live, including cancellation and participant departure.
        if !self.0.requested.load(Ordering::Acquire) {
            return false;
        }
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .request
            .as_ref()
            .is_some_and(|r| r.members.contains(&pid) && !r.departed.contains(&pid))
    }
}
impl<T> Participant<T> {
    pub fn capture(
        &self,
        capture: impl FnOnce(CheckpointTime) -> Result<T, String>,
    ) -> Result<Option<Vec<(u32, T)>>, String> {
        let inner = &self.domain.0;
        let mut state = inner
            .state
            .lock()
            .map_err(|_| "checkpoint domain poisoned")?;
        let request = state
            .request
            .as_mut()
            .ok_or("no device checkpoint requested")?;
        if !request.members.contains(&self.pid) || !request.arrived.insert(self.pid) {
            return Err("process entered checkpoint twice".into());
        }
        if request.arrived == request.members {
            request.time = Some(CheckpointTime::now());
        }
        let epoch = request.epoch;
        inner.changed.notify_all();
        loop {
            let request = state
                .request
                .as_ref()
                .ok_or("device checkpoint was cancelled")?;
            if request.error.is_some() || request.arrived == request.members {
                break;
            }
            let remaining = request.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.request.as_mut().unwrap().error =
                    Some("timed out waiting for every guest process to reach a checkpoint".into());
                inner.changed.notify_all();
                break;
            }
            state = inner
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "checkpoint domain poisoned")?
                .0;
        }
        let cancelled = state.request.as_ref().unwrap().error.clone();
        let result = if let Some(error) = cancelled {
            Err(error)
        } else {
            // Each callback owns its CPU and thread-local graphics/media state.
            // Every participant is parked until all callbacks have completed.
            let at = state
                .request
                .as_ref()
                .and_then(|request| request.time)
                .ok_or("missing common checkpoint epoch")?;
            drop(state);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture(at)))
                .unwrap_or_else(|_| Err("process checkpoint capture panicked".into()));
            state = inner
                .state
                .lock()
                .map_err(|_| "checkpoint domain poisoned")?;
            result
        };
        let request = state
            .request
            .as_mut()
            .filter(|r| r.epoch == epoch)
            .ok_or("checkpoint epoch changed")?;
        match result {
            Ok(value) => {
                request.values.insert(self.pid, value);
            }
            Err(error) => {
                request.error.get_or_insert(error);
            }
        }
        request.completed.insert(self.pid);
        inner.changed.notify_all();
        loop {
            let request = state.request.as_ref().unwrap();
            // A rendezvous timeout can leave a process outside this callback.
            // Wait only for callbacks which actually joined; no guest resumes
            // while one of the already parked participants is still capturing.
            if request.completed == request.arrived {
                break;
            }
            state = inner
                .changed
                .wait(state)
                .map_err(|_| "checkpoint domain poisoned")?;
        }
        let request = state.request.as_mut().unwrap();
        let result = if let Some(error) = &request.error {
            Err(error.clone())
        } else if request.completed != request.members {
            Err("incomplete device checkpoint".into())
        } else if request.leader == self.pid {
            Ok(Some(
                std::mem::take(&mut request.values).into_iter().collect(),
            ))
        } else {
            Ok(None)
        };
        request.departed.insert(self.pid);
        if request.departed == request.arrived {
            state.request = None;
            inner.requested.store(false, Ordering::Release);
        }
        inner.changed.notify_all();
        result
    }
}
impl<T> Drop for Participant<T> {
    fn drop(&mut self) {
        let mut state = self
            .domain
            .0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        state.members.remove(&self.pid);
        if let Some(request) = state.request.as_mut() {
            if request.members.contains(&self.pid) && !request.departed.contains(&self.pid) {
                request
                    .error
                    .get_or_insert_with(|| "process exited during checkpoint rendezvous".into());
                request.members.remove(&self.pid);
            }
        }
        self.domain.0.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_parks_all_processes_until_both_captures_finish() {
        let domain = CheckpointDomain::default();
        let parent = domain.register(1).unwrap();
        let child = domain.register(2).unwrap();
        domain.request(1, [1, 2], Duration::from_secs(2)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            child.capture(|_| {
                tx.send(()).unwrap();
                wait.recv().unwrap();
                Ok(22)
            })
        });
        let leader = std::thread::spawn(move || parent.capture(|_| Ok(11)));
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!leader.is_finished());
        assert!(!thread.is_finished());
        release.send(()).unwrap();
        assert_eq!(
            leader.join().unwrap().unwrap(),
            Some(vec![(1, 11), (2, 22)])
        );
        assert_eq!(thread.join().unwrap().unwrap(), None);
        assert!(!domain.requested(1));
    }
    #[test]
    fn checkpoint_failure_releases_every_participant_and_can_be_retried() {
        let domain = CheckpointDomain::<u32>::default();
        let parent = domain.register(1).unwrap();
        let child = domain.register(2).unwrap();
        domain.request(1, [1, 2], Duration::from_secs(2)).unwrap();
        std::thread::scope(|scope| {
            let remote = scope.spawn(|| child.capture(|_| Err("bad resource".into())));
            assert!(parent.capture(|_| Ok(1)).is_err());
            assert!(remote.join().unwrap().is_err());
        });
        domain.request(1, [1, 2], Duration::from_secs(2)).unwrap();
        std::thread::scope(|scope| {
            let remote = scope.spawn(|| child.capture(|_| Ok(2)));
            assert_eq!(
                parent.capture(|_| Ok(1)).unwrap(),
                Some(vec![(1, 1), (2, 2)])
            );
            assert!(remote.join().unwrap().unwrap().is_none());
        });
    }
    #[test]
    fn checkpoint_missing_process_times_out_without_calling_capture() {
        let domain = CheckpointDomain::<u32>::default();
        let parent = domain.register(1).unwrap();
        let _child = domain.register(2).unwrap();
        domain.request(1, [1, 2], Duration::ZERO).unwrap();
        assert!(parent
            .capture(|_| panic!("must not capture a running child"))
            .is_err());
        assert!(!domain.requested(1));
    }
    #[test]
    fn idle_frame_poll_does_not_wait_for_the_checkpoint_mutex() {
        let domain = CheckpointDomain::<u32>::default();
        let _participant = domain.register(1).unwrap();
        let guard = domain.0.state.lock().unwrap();
        let remote = domain.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || tx.send(remote.requested(1)).unwrap());
        let result = rx.recv_timeout(Duration::from_secs(1));
        drop(guard);
        worker.join().unwrap();
        assert!(!result.unwrap());
    }
}
