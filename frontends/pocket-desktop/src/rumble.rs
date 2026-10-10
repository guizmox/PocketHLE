//! Bounded bridge to the SDL owner thread; no controller handle crosses threads.
use pocket_core::kernel::vibration::{Backend, Motor, Result};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, SyncSender},
    Arc,
};
use std::time::{Duration, Instant};
pub enum Command {
    Open(u64, mpsc::Sender<Result<()>>),
    Apply(u64, Vec<i64>, Instant, mpsc::Sender<Result<()>>),
    Close(u64),
}
#[derive(Clone)]
pub struct Bridge {
    pub tx: SyncSender<Command>,
    next: Arc<AtomicU64>,
}
impl Bridge {
    pub fn new(tx: SyncSender<Command>) -> Self {
        Self {
            tx,
            next: Arc::new(AtomicU64::new(1)),
        }
    }
    fn request(&self, make: impl FnOnce(mpsc::Sender<Result<()>>) -> Command) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.tx.try_send(make(tx)).map_err(|_| 21u32)?;
        rx.recv_timeout(Duration::from_secs(1)).map_err(|_| 21u32)?
    }
}
struct HostMotor {
    bridge: Bridge,
    id: u64,
}
impl Backend for Bridge {
    fn open(&self) -> Result<Box<dyn Motor>> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.request(|reply| Command::Open(id, reply))?;
        Ok(Box::new(HostMotor {
            bridge: self.clone(),
            id,
        }))
    }
}
impl Motor for HostMotor {
    fn apply(&mut self, timings: &[i64]) -> Result<()> {
        if timings.len() > 2050 || timings.iter().any(|t| *t < 0) {
            return Err(87);
        }
        self.bridge
            .request(|reply| Command::Apply(self.id, timings.to_vec(), Instant::now(), reply))
    }
}
impl Drop for HostMotor {
    fn drop(&mut self) {
        let _ = self.bridge.tx.send(Command::Close(self.id));
    }
}
#[derive(Clone)]
pub struct Pattern {
    pub timings: Vec<i64>,
    pub started: Instant,
}
impl Pattern {
    pub fn active_at(&self, elapsed_ms: u64) -> bool {
        let total: u64 = self
            .timings
            .iter()
            .fold(0u64, |sum, t| sum.saturating_add((*t).max(0) as u64));
        if total == 0 {
            return false;
        }
        let mut phase = elapsed_ms % total;
        for (index, duration) in self.timings.iter().enumerate() {
            if phase < *duration as u64 {
                return index % 2 == 1;
            }
            phase -= *duration as u64;
        }
        false
    }
    pub fn active(&self) -> bool {
        self.active_at(self.started.elapsed().as_millis() as u64)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_blink_and_meta_cycles() {
        let pattern = |t: Vec<i64>| Pattern {
            timings: t,
            started: Instant::now(),
        };
        assert!(pattern(vec![0, 1000]).active_at(4000));
        let p = pattern(vec![0, 20, 30, 0]);
        assert!(p.active_at(0));
        assert!(p.active_at(19));
        assert!(!p.active_at(20));
        assert!(p.active_at(50));
        let p = pattern(vec![0, 10, 10, 10, 50, 0]);
        assert!(p.active_at(25));
        assert!(!p.active_at(35));
        assert!(p.active_at(80));
        assert!(!pattern(vec![]).active_at(10));
        assert!(!pattern(vec![0, 0]).active_at(10));
    }
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    #[test]
    fn repeated_motor_open_apply_drop_releases_all_sessions() {
        let (tx, rx) = mpsc::sync_channel(32);
        let bridge = Bridge::new(tx);
        let thread = std::thread::spawn(move || {
            let mut live = std::collections::HashSet::new();
            let mut closed = 0;
            while let Ok(command) = rx.recv() {
                match command {
                    Command::Open(id, reply) => {
                        assert!(live.insert(id));
                        reply.send(Ok(())).unwrap();
                    }
                    Command::Apply(id, timings, _, reply) => {
                        assert!(live.contains(&id));
                        assert!(timings.len() <= 2050);
                        reply.send(Ok(())).unwrap();
                    }
                    Command::Close(id) => {
                        assert!(live.remove(&id));
                        closed += 1;
                    }
                }
                if closed == 100 {
                    assert!(live.is_empty());
                    return;
                }
            }
            panic!("channel closed before motor cleanup");
        });
        for _ in 0..100 {
            let mut motor = bridge.open().unwrap();
            motor.apply(&[0, 20, 30, 0]).unwrap();
            motor.apply(&[]).unwrap();
            drop(motor);
        }
        thread.join().unwrap();
        assert!(bridge.open().is_err());
    }
    #[test]
    fn unsupported_controller_returns_real_error_without_motor() {
        let (tx, rx) = mpsc::sync_channel(32);
        let bridge = Bridge::new(tx);
        let thread = std::thread::spawn(move || {
            if let Command::Open(_, reply) = rx.recv().unwrap() {
                reply.send(Err(50)).unwrap();
            } else {
                panic!("unexpected command");
            }
        });
        assert!(matches!(bridge.open(), Err(50)));
        thread.join().unwrap();
    }
}
