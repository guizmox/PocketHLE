//! One pending image, not an accumulating FIFO. Inputs use their own channel.
use std::sync::{
    mpsc::{SendError, TryRecvError},
    Arc, Mutex,
};
struct State<T> {
    pending: Option<T>,
    receiver_alive: bool,
    senders: usize,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}
pub struct Sender<T>(Arc<Mutex<State<T>>>);
pub struct Receiver<T>(Arc<Mutex<State<T>>>);
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let state = Arc::new(Mutex::new(State {
        pending: None,
        receiver_alive: true,
        senders: 1,
        wake: None,
    }));
    (Sender(state.clone()), Receiver(state))
}
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).senders += 1;
        Self(self.0.clone())
    }
}
impl<T> Sender<T> {
    pub fn send(&self, image: T) -> Result<(), SendError<T>> {
        let (previous, wake) = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if !state.receiver_alive {
                return Err(SendError(image));
            }
            let previous = state.pending.replace(image);
            let wake = if previous.is_none() {
                state.wake.clone()
            } else {
                None
            };
            (previous, wake)
        };
        // Release superseded GPU leases outside the mailbox lock; never call GL.
        drop(previous);
        // A wake can enter egui or consume the image. Never hold the mailbox
        // lock across it; coalesce wakeups while an image is already pending.
        if let Some(wake) = wake {
            wake();
        }
        Ok(())
    }
}
impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).senders -= 1;
    }
}
impl<T> Receiver<T> {
    pub fn set_waker_once(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        let pending = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if state.wake.is_some() {
                return;
            }
            state.wake = Some(wake.clone());
            state.pending.is_some()
        };
        if pending {
            wake();
        }
    }
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.pending.take().ok_or(if state.senders == 0 {
            TryRecvError::Disconnected
        } else {
            TryRecvError::Empty
        })
    }
}
impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let pending = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            state.receiver_alive = false;
            state.pending.take()
        };
        drop(pending);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn wakes_once_per_pending_image_and_runs_outside_the_lock() {
        let (tx, rx) = channel();
        let count = Arc::new(AtomicUsize::new(0));
        let weak = Arc::downgrade(&tx.0);
        let observed = count.clone();
        rx.set_waker_once(Arc::new(move || {
            let state = weak.upgrade().unwrap();
            assert!(state.try_lock().is_ok(), "wake called with mailbox locked");
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        for image in 0..100 {
            tx.send(image).unwrap();
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(rx.try_recv().unwrap(), 99);
        tx.send(100).unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        drop(rx);
        assert!(tx.send(101).is_err());
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn installing_waker_with_pending_image_does_not_lose_the_wake() {
        let (tx, rx) = channel();
        tx.send(3).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        rx.set_waker_once(Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        rx.set_waker_once(Arc::new(|| panic!("must not replace registered wake")));
        assert_eq!(rx.try_recv().unwrap(), 3);
        tx.send(4).unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn stalled_ui_releases_superseded_images_and_receives_newest() {
        let (tx, rx) = channel();
        let old = Arc::new(0);
        tx.send(old.clone()).unwrap();
        std::thread::spawn({
            let tx = tx.clone();
            move || {
                for i in 1..10000 {
                    tx.send(Arc::new(i)).unwrap();
                }
            }
        })
        .join()
        .unwrap();
        assert_eq!(Arc::strong_count(&old), 1, "obsolete GPU lease retained");
        assert_eq!(*rx.try_recv().unwrap(), 9999);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        drop(tx);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }
    #[test]
    fn shutdown_releases_pending_and_rejects_new_frames() {
        let (tx, rx) = channel();
        let frame = Arc::new(1);
        tx.send(frame.clone()).unwrap();
        drop(rx);
        assert_eq!(Arc::strong_count(&frame), 1);
        assert!(tx.send(frame.clone()).is_err());
        assert_eq!(Arc::strong_count(&frame), 1);
    }
    #[test]
    fn producer_clones_preserve_last_image_after_disconnect() {
        let (tx, rx) = channel();
        let other = tx.clone();
        drop(tx);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        other.send(7).unwrap();
        drop(other);
        assert_eq!(rx.try_recv(), Ok(7));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }
}
