//! Bounded delivery for Windows change notification callbacks.
//!
//! This module does not own an OS watch. The registration adapter must stop
//! the stream before releasing its OS resources.
use du_core_contracts::change_notify::{WatchError, WatchEvent, WatchEvents};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
};

/// F18's provisional per-registration signal channel capacity.
pub const WATCH_CHANNEL_CAPACITY: usize = 8192;

struct Shared {
    active: AtomicBool,
    lost: AtomicBool,
    lost_at: AtomicU64,
    interruption: AtomicU8,
    interruption_at: AtomicU64,
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
        interruption: AtomicU8::new(0),
        interruption_at: AtomicU64::new(0),
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
        if self.shared.active.load(Ordering::Acquire)
            && self.shared.interruption.load(Ordering::Acquire) == 0
            && !self.shared.lost.load(Ordering::Acquire)
        {
            self.shared.lost_at.store(observed_at, Ordering::Relaxed);
            self.shared.lost.store(true, Ordering::Release);
        }
    }
    /// Enqueues without waiting. Callbacks for one watch are serialized.
    /// A loss fences later signals until consumed; an interruption is retained
    /// even when full and permanently fences later signals. IF-11 has three
    /// interruption reasons, so a resource-limit input becomes `Other` here.
    pub fn signal(&self, event: WatchEvent, observed_at: u64) {
        if !self.shared.active.load(Ordering::Acquire) {
            return;
        }
        if self.shared.interruption.load(Ordering::Acquire) != 0 {
            return;
        }
        match event {
            WatchEvent::Interrupted {
                reason,
                observed_at,
            } => {
                let code = match reason {
                    WatchError::Disconnected => 1,
                    WatchError::AccessLost => 2,
                    WatchError::ResourceLimit | WatchError::Other => 3,
                };
                self.shared
                    .interruption_at
                    .store(observed_at, Ordering::Relaxed);
                self.shared.interruption.store(code, Ordering::Release);
                return;
            }
            WatchEvent::Loss { observed_at } => {
                self.loss(observed_at);
                return;
            }
            WatchEvent::Signal { .. } => {}
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
            return Some(self.take_loss());
        }
        let code = self.shared.interruption.load(Ordering::Acquire);
        if (1..=3).contains(&code) {
            // A producer can queue preceding signals or loss after the first checks.
            if let Ok(event) = self.receiver.try_recv() {
                return Some(event);
            }
            if self.shared.lost.load(Ordering::Acquire) {
                return Some(self.take_loss());
            }
            let observed_at = self.shared.interruption_at.load(Ordering::Relaxed);
            self.shared
                .interruption
                .store(code | 0x80, Ordering::Release);
            let reason = match code {
                1 => WatchError::Disconnected,
                2 => WatchError::AccessLost,
                _ => WatchError::Other,
            };
            return Some(WatchEvent::Interrupted {
                reason,
                observed_at,
            });
        }
        None
    }
}

impl WatchEventReceiver {
    /// Consumes the producer's loss marker after all earlier queued signals.
    fn take_loss(&self) -> WatchEvent {
        let observed_at = self.shared.lost_at.load(Ordering::Relaxed);
        self.shared.lost.store(false, Ordering::Release);
        WatchEvent::Loss { observed_at }
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

    #[test]
    fn interruption_survives_full_channel_and_fences_later_signals() -> Result<(), &'static str> {
        let (tx, mut rx) = watch_event_channel(1).ok_or("capacity")?;
        tx.signal(signal(1), 1);
        tx.signal(signal(2), 2);
        tx.signal(
            WatchEvent::Interrupted {
                reason: WatchError::AccessLost,
                observed_at: 3,
            },
            3,
        );
        tx.signal(
            WatchEvent::Interrupted {
                reason: WatchError::Disconnected,
                observed_at: 4,
            },
            4,
        );
        tx.signal(signal(4), 4);
        assert_eq!(rx.try_next(), Some(signal(1)));
        assert_eq!(rx.try_next(), Some(WatchEvent::Loss { observed_at: 2 }));
        assert_eq!(
            rx.try_next(),
            Some(WatchEvent::Interrupted {
                reason: WatchError::AccessLost,
                observed_at: 3
            })
        );
        assert_eq!(rx.try_next(), None);
        tx.signal(signal(5), 5);
        assert_eq!(rx.try_next(), None);
        Ok(())
    }

    #[test]
    fn interruption_reasons_follow_if11_with_full_queue_and_optional_loss()
    -> Result<(), &'static str> {
        for reason in [
            WatchError::Disconnected,
            WatchError::AccessLost,
            WatchError::ResourceLimit,
            WatchError::Other,
        ] {
            for overflow in [false, true] {
                let expected = if reason == WatchError::ResourceLimit {
                    WatchError::Other
                } else {
                    reason
                };
                let (tx, mut rx) = watch_event_channel(1).ok_or("capacity")?;
                tx.signal(signal(1), 1);
                if overflow {
                    tx.signal(signal(2), 2);
                }
                tx.signal(
                    WatchEvent::Interrupted {
                        reason,
                        observed_at: 3,
                    },
                    3,
                );
                assert_eq!(rx.try_next(), Some(signal(1)));
                if overflow {
                    assert_eq!(rx.try_next(), Some(WatchEvent::Loss { observed_at: 2 }));
                }
                assert_eq!(
                    rx.try_next(),
                    Some(WatchEvent::Interrupted {
                        reason: expected,
                        observed_at: 3
                    })
                );
                assert_eq!(rx.try_next(), None);
            }
        }
        Ok(())
    }
}
