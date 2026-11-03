//! One pending image, not an accumulating FIFO. Inputs use their own channel.
use std::sync::{
    mpsc::{SendError, TryRecvError},
    Arc, Mutex,
};
struct State<T> {
    pending: Option<T>,
    receiver_alive: bool,
    senders: usize,
}
pub struct Sender<T>(Arc<Mutex<State<T>>>);
pub struct Receiver<T>(Arc<Mutex<State<T>>>);
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let state = Arc::new(Mutex::new(State {
        pending: None,
        receiver_alive: true,
        senders: 1,
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
        let previous = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if !state.receiver_alive {
                return Err(SendError(image));
            }
            state.pending.replace(image)
        };
        // Release superseded GPU leases outside the mailbox lock; never call GL.
        drop(previous);
        Ok(())
    }
}
impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).senders -= 1;
    }
}
impl<T> Receiver<T> {
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
