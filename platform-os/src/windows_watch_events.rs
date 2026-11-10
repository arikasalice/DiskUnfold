//! Bounded delivery for Windows change notification callbacks.
//!
//! This module does not own an OS watch. The registration adapter must stop
//! the stream before releasing its OS resources.
use du_core_contracts::change_notify::{WatchEvent, WatchEvents};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
};

/// F18's provisional per-registration signal channel capacity.
pub const WATCH_CHANNEL_CAPACITY: usize = 8192;

struct Shared {
    active: AtomicBool,
    lost: AtomicBool,
    lost_at: AtomicU64,
}

/// Nonblocking callback endpoint for one OS registration.
#[derive(Clone)]
pub struct WatchEventSender {
    sender: SyncSender<WatchEvent>,
    shared: Arc<Shared>,
}

/// Consumer endpoint for one OS registration.
pub struct WatchEventReceiver {
    receiver: Receiver<WatchEvent>,
    shared: Arc<Shared>,
}

/// Creates a bounded channel. The capacity must be nonzero.
pub fn watch_event_channel(capacity: usize) -> Option<(WatchEventSender, WatchEventReceiver)> {
    if capacity == 0 {
        return None;
    }
    let (sender, receiver) = sync_channel(capacity);
    let shared = Arc::new(Shared {
        active: AtomicBool::new(true),
        lost: AtomicBool::new(false),
        lost_at: AtomicU64::new(0),
    });
    Some((
        WatchEventSender {
            sender,
            shared: Arc::clone(&shared),
        },
        WatchEventReceiver { receiver, shared },
    ))
}

impl WatchEventSender {
    /// Records a loss reported by the OS or by record validation.
    pub fn loss(&self, observed_at: u64) {
        if self.shared.active.load(Ordering::Acquire) && !self.shared.lost.load(Ordering::Acquire) {
            self.shared.lost_at.store(observed_at, Ordering::Relaxed);
            self.shared.lost.store(true, Ordering::Release);
        }
    }
    /// Enqueues a signal without waiting. Callbacks for one watch are serialized.
    /// Once a signal is lost, later signals are dropped until loss is consumed.
    pub fn signal(&self, event: WatchEvent, observed_at: u64) {
        if !self.shared.active.load(Ordering::Acquire) {
            return;
        }
        if self.shared.lost.load(Ordering::Acquire) {
            return;
        }
        if let Err(TrySendError::Full(_)) = self.sender.try_send(event) {
            self.loss(observed_at);
        }
    }

    /// Fences all queued and future events before resource release.
    pub fn stop(&self) {
        self.shared.active.store(false, Ordering::Release);
    }
}

impl WatchEvents for WatchEventReceiver {
    fn try_next(&mut self) -> Option<WatchEvent> {
        if !self.shared.active.load(Ordering::Acquire) {
            return None;
        }
        if let Ok(event) = self.receiver.try_recv() {
            return Some(event);
        }
        if self.shared.lost.load(Ordering::Acquire) {
            // A producer may have filled the queue after the first empty read.
            if let Ok(event) = self.receiver.try_recv() {
                return Some(event);
            }
            let lost_at = self.shared.lost_at.load(Ordering::Relaxed);
            self.shared.lost.store(false, Ordering::Release);
            return Some(WatchEvent::Loss {
                observed_at: lost_at,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::change_notify::ChangeHint;

    fn signal(time: u64) -> WatchEvent {
        WatchEvent::Signal {
            path: None,
            hint: ChangeHint::Modify,
            observed_at: time,
        }
    }

    #[test]
    fn bounded_loss_preserves_order_and_recovers() -> Result<(), &'static str> {
        let (tx, mut rx) = watch_event_channel(2).ok_or("capacity")?;
        tx.signal(signal(1), 1);
        tx.signal(signal(2), 2);
        tx.signal(signal(3), 3);
        tx.signal(signal(4), 4);
        assert_eq!(rx.try_next(), Some(signal(1)));
        assert_eq!(rx.try_next(), Some(signal(2)));
        assert_eq!(rx.try_next(), Some(WatchEvent::Loss { observed_at: 3 }));
        tx.signal(signal(5), 5);
        assert_eq!(rx.try_next(), Some(signal(5)));
        assert_eq!(rx.try_next(), None);
        Ok(())
    }

    #[test]
    fn stopped_stream_discards_queued_and_late_signals() -> Result<(), &'static str> {
        let (tx, mut rx) = watch_event_channel(1).ok_or("capacity")?;
        tx.signal(signal(1), 1);
        tx.stop();
        tx.signal(signal(2), 2);
        assert_eq!(rx.try_next(), None);
        assert!(watch_event_channel(0).is_none());
        Ok(())
    }

    #[test]
    fn published_capacity_produces_one_loss_marker() -> Result<(), &'static str> {
        let (tx, mut rx) = watch_event_channel(WATCH_CHANNEL_CAPACITY).ok_or("capacity")?;
        for time in 0..WATCH_CHANNEL_CAPACITY as u64 {
            tx.signal(signal(time), time);
        }
        tx.signal(signal(8192), 8192);
        for time in 0..WATCH_CHANNEL_CAPACITY as u64 {
            assert_eq!(rx.try_next(), Some(signal(time)));
        }
        assert_eq!(rx.try_next(), Some(WatchEvent::Loss { observed_at: 8192 }));
        assert_eq!(rx.try_next(), None);
        Ok(())
    }

    #[test]
    fn reported_os_loss_is_delivered_after_prior_signals() -> Result<(), &'static str> {
        let (tx, mut rx) = watch_event_channel(2).ok_or("capacity")?;
        tx.signal(signal(1), 1);
        tx.loss(2);
        tx.signal(signal(3), 3);
        assert_eq!(rx.try_next(), Some(signal(1)));
        assert_eq!(rx.try_next(), Some(WatchEvent::Loss { observed_at: 2 }));
        assert_eq!(rx.try_next(), None);
        Ok(())
    }
}
