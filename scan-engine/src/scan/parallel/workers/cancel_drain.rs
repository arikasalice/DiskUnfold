//! Deliver observations already returned by the OS, without opening more paths.
use super::*;

/// Drains only still-owned returned entries; deferred entries were sent already.
pub(super) fn frame(mut frame: Frame, sender: &SyncSender<Event>, policy: Policy<'_>) -> bool {
    while let Some(entry) = frame.entries.pop_front() {
        let mut path = frame.path.clone();
        path.push(entry.entry.name.clone());
        if let Policy::Legacy(include) = policy
            && !include(&path)
        {
            continue;
        }
        if sender
            .send(Event::Item(frame.root_index, path, Box::new(entry)))
            .is_err()
        {
            return false;
        }
    }
    true
}

/// Owns frames outside the queue lock before sending to the bounded publisher.
pub(super) fn drain(
    shared: &Shared,
    local: &mut Vec<Frame>,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> bool {
    loop {
        if shared.abort.load(Ordering::Acquire) {
            return false;
        }
        let next = if let Some(frame) = local.pop() {
            Some(frame)
        } else {
            match shared.queue.lock() {
                Ok(mut queue) => queue.ready.pop_front(),
                Err(error) => {
                    drop(error.into_inner());
                    shared.abort(context);
                    return false;
                }
            }
        };
        match next {
            Some(owned) => {
                if !frame(owned, sender, policy) {
                    return false;
                }
            }
            None => return true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{
        ports::{PortError, PortErrorKind},
        scan_observation::DirectoryObservations,
    };
    use std::{sync::mpsc, time::Duration};
    struct Empty;
    impl DirectoryObservations for Empty {
        fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
            Ok(Vec::new())
        }
    }
    impl AnchoredDirectory for Empty {
        fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        }
    }
    struct Flip(std::sync::atomic::AtomicUsize);
    impl JobContext for Flip {
        fn checkpoint(&self) -> Checkpoint {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Checkpoint::Continue
            } else {
                Checkpoint::Cancel
            }
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Err(JobFlowError::Closed)
        }
    }
    #[test]
    fn cancel_after_entry_admission_keeps_current_and_remaining_items() -> Result<(), String> {
        let entry = |name: &[u8]| {
            TimedObservedEntry::from(ObservedEntry {
                name: name.to_vec(),
                enumerated_inode: 0,
                enumerated_kind: ObservedKind::Unknown,
                metadata: Err(PortError {
                    kind: PortErrorKind::Io,
                    reason_key: "os.query_failed".into(),
                    os_code: None,
                }),
            })
        };
        let pending = Frame {
            root_index: 0,
            path: Vec::new(),
            directory: Box::new(Empty),
            entries: VecDeque::from([entry(b"first"), entry(b"second")]),
            deferred: VecDeque::new(),
            root: true,
            first_batch: false,
            root_published: false,
            batch_pending: false,
            case_observed: false,
        };
        let shared = Shared {
            queue: Mutex::new(Queue {
                ready: VecDeque::new(),
                ready_high_water: 0,
                last_root: None,
                remaining: 1,
                root_remaining: vec![1],
                root_groups: vec![0],
                active_groups: vec![0],
                device_limit: 1,
                started: true,
            }),
            wake: Condvar::new(),
            abort: AtomicBool::new(false),
            disconnects: disconnect::Disconnects::new(1, &[]),
        };
        let context = Flip(std::sync::atomic::AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel(4);
        let mut local = Vec::new();
        let include = |_: &[Vec<u8>]| true;
        assert!(super::super::process(
            pending,
            &shared,
            &sender,
            &context,
            Policy::Legacy(&include),
            &mut local
        ));
        assert!(drain(
            &shared,
            &mut local,
            &sender,
            &context,
            Policy::Legacy(&include)
        ));
        assert_eq!(
            receiver
                .try_iter()
                .filter(|v| matches!(v, Event::Item(..)))
                .count(),
            2
        );
        Ok(())
    }

    #[test]
    fn deferred_entries_are_not_sent_twice() -> Result<(), String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let entry = TimedObservedEntry::from(ObservedEntry {
            name: b"already-sent".to_vec(),
            enumerated_inode: 0,
            enumerated_kind: ObservedKind::Unknown,
            metadata: Err(PortError {
                kind: PortErrorKind::Io,
                reason_key: "os.query_failed".into(),
                os_code: None,
            }),
        });
        let pending = Frame {
            root_index: 0,
            path: Vec::new(),
            directory: Box::new(Empty),
            entries: VecDeque::new(),
            deferred: VecDeque::from([entry]),
            root: true,
            first_batch: false,
            root_published: false,
            batch_pending: false,
            case_observed: false,
        };
        assert!(frame(pending, &sender, Policy::Legacy(&|_| true)));
        assert!(receiver.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn full_channel_applies_backpressure_until_consumer_drains() -> Result<(), String> {
        let (sender, receiver) = mpsc::sync_channel(8192);
        for _ in 0..8192 {
            sender
                .send(Event::RootBatch(0, None, None))
                .map_err(|_| "fill")?;
        }
        let entry = TimedObservedEntry::from(ObservedEntry {
            name: b"file".to_vec(),
            enumerated_inode: 0,
            enumerated_kind: ObservedKind::Unknown,
            metadata: Err(PortError {
                kind: PortErrorKind::Io,
                reason_key: "os.query_failed".into(),
                os_code: None,
            }),
        });
        let item = Frame {
            root_index: 0,
            path: Vec::new(),
            directory: Box::new(Empty),
            entries: VecDeque::from([entry]),
            deferred: VecDeque::new(),
            root: true,
            first_batch: false,
            root_published: false,
            batch_pending: false,
            case_observed: false,
        };
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let ok = frame(item, &sender, Policy::Legacy(&|_| true));
            let _ = done_tx.send(ok);
        });
        assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
        receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "receive")?;
        assert!(
            done_rx
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| "blocked")?
        );
        let remaining: Vec<_> = receiver.try_iter().collect();
        assert_eq!(remaining.len(), 8192);
        assert_eq!(
            remaining
                .iter()
                .filter(|event| matches!(event, Event::Item(..)))
                .count(),
            1
        );
        worker.join().map_err(|_| "join")?;
        Ok(())
    }
}
