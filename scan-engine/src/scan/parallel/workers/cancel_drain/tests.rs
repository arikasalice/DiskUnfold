use super::*;
use du_core_contracts::scan_observation::ObservedValue;
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{DirectoryObservations, ObservedKind},
};
use std::{sync::mpsc, time::Duration};
struct Empty;
impl DirectoryObservations for Empty {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for Empty {
    // Placed in a frame directly; root identities are never queried here.
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        None
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}

fn make_frame(
    entries: Vec<TimedObservedEntry>,
    deferred: Vec<TimedObservedEntry>,
    boundary: Boundary,
) -> Frame {
    let mut frame = Frame::root(0, Box::new(Empty), boundary);
    frame.entries = entries.into_iter().map(Into::into).collect();
    frame.deferred = deferred.into_iter().map(Into::into).collect();
    frame
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
    let pending = make_frame(
        vec![entry(b"first"), entry(b"second")],
        Vec::new(),
        Boundary::Unknown,
    );
    let shared = single_root();
    let seat = member(&shared)?;
    let context = Flip(std::sync::atomic::AtomicUsize::new(0));
    let (sender, receiver) = mpsc::sync_channel(4);
    let include = |_: &[Vec<u8>]| true;
    let mut turn = Turn {
        shared: &shared,
        seat,
        lane: 0,
        reserved: false,
    };
    assert!(super::super::process(
        pending,
        &shared,
        &sender,
        &context,
        Policy::Legacy(&include),
        &mut turn
    ));
    drop(turn);
    assert!(drain(
        &shared,
        seat,
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

fn single_root() -> Shared {
    let mut queue = Queue::for_test(vec![0], 1);
    queue.remaining = 1;
    queue.root_remaining = vec![1];
    Shared::for_test(queue, false)
}

/// Registers a worker seat that is a member of lane 0.
fn member(shared: &Shared) -> Result<usize, String> {
    let mut queue = shared.queue.lock().map_err(|_| "queue lock")?;
    let seat = queue.lanes.register_seat();
    queue.lanes.join(seat, 0);
    Ok(seat)
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
    let pending = make_frame(Vec::new(), vec![entry], Boundary::Unknown);
    let shared = single_root();
    assert!(frame(pending, &shared, &sender, Policy::Legacy(&|_| true)));
    // The deferred directory is only marked as unread, never sent again as an item.
    assert!(matches!(
        receiver.try_recv(),
        Ok(Event::Unread(0, path)) if path == [b"already-sent".to_vec()]
    ));
    assert!(receiver.try_recv().is_err());
    Ok(())
}

#[test]
fn drained_directory_is_reported_without_being_reserved() -> Result<(), String> {
    let directory = ObservedEntry {
        name: b"never-opened".to_vec(),
        enumerated_inode: 9,
        enumerated_kind: ObservedKind::Directory,
        metadata: Ok(du_core_contracts::scan_observation::MetadataObservation {
            kind: ObservedKind::Directory,
            device: 1,
            inode: 9,
            size: Ok(0),
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
        }),
    };
    let pending = make_frame(
        vec![TimedObservedEntry::from(directory.clone())],
        Vec::new(),
        Boundary::Volume(1),
    );
    // The item, then unread marks for the entered child and the drained root.
    let (sender, receiver) = mpsc::sync_channel(3);
    let shared = single_root();
    assert!(frame(pending, &shared, &sender, Policy::Legacy(&|_| true)));
    assert!(matches!(
        receiver.try_recv(),
        Ok(Event::Item(_, _, _, Admission::Enter, _))
    ));
    // A later admission of the same identity may still enter it.
    let parent = admission::Parent {
        root: 0,
        boundary: Boundary::Volume(1),
    };
    assert_eq!(
        admission::decide(false, &directory, parent, &shared.visited),
        Admission::Enter
    );
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
    let item = make_frame(vec![entry], Vec::new(), Boundary::Unknown);
    let shared = single_root();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let ok = frame(item, &shared, &sender, Policy::Legacy(&|_| true));
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

#[cfg(windows)]
#[test]
fn windows_file_cancel_drain_waits_for_full_consumer_queue() -> Result<(), String> {
    use std::{fs, time::SystemTime};
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.0.join("observed"));
            let _ = fs::remove_dir(&self.0);
        }
    }
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("du-f02-queue-{}-{nonce}", std::process::id()));
    fs::create_dir(&path).map_err(|e| e.to_string())?;
    let fixture = Fixture(path);
    fs::write(fixture.0.join("observed"), b"ntfs").map_err(|e| e.to_string())?;
    let file = fs::read_dir(&fixture.0)
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("file missing")?
        .map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    let name: Vec<u8> = file
        .file_name()
        .to_string_lossy()
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let observed = TimedObservedEntry::from(ObservedEntry {
        name: name.clone(),
        enumerated_inode: 1,
        enumerated_kind: ObservedKind::File,
        metadata: Ok(du_core_contracts::scan_observation::MetadataObservation {
            kind: ObservedKind::File,
            device: 1,
            inode: 1,
            size: Ok(length),
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
        }),
    });
    assert_eq!(BUFFERED_OBSERVATIONS, 8_192);
    let (sender, receiver) = mpsc::sync_channel(BUFFERED_OBSERVATIONS);
    for _ in 0..BUFFERED_OBSERVATIONS {
        sender
            .send(Event::RootBatch(0, None, None))
            .map_err(|_| "fill")?;
    }
    let pending = make_frame(vec![observed], Vec::new(), Boundary::Unknown);
    let (entered, started) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let shared = single_root();
        let Ok(seat) = member(&shared) else {
            let _ = finished.send(false);
            return;
        };
        let cancelled = Flip(std::sync::atomic::AtomicUsize::new(1));
        let _ = entered.send(());
        let result = super::super::process(
            pending,
            &shared,
            &sender,
            &cancelled,
            Policy::Legacy(&|_| true),
            &mut Turn {
                shared: &shared,
                seat,
                lane: 0,
                reserved: false,
            },
        );
        let _ = finished.send(result);
    });
    started
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert!(matches!(
        done.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    receiver
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert!(
        done.recv_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?
    );
    let remaining: Vec<_> = receiver.try_iter().collect();
    assert_eq!(remaining.len(), BUFFERED_OBSERVATIONS);
    let events: Vec<_> = remaining
        .iter()
        .filter_map(|event| match event {
            Event::Item(_, _, entry, _, _) => Some(entry),
            _ => None,
        })
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].entry.name, name);
    assert_eq!(
        events[0]
            .entry
            .metadata
            .as_ref()
            .map_err(|_| "metadata")?
            .size,
        Ok(4)
    );
    worker.join().map_err(|_| "worker panic")?;
    eprintln!(
        "f02 windows cancelled=true queue={BUFFERED_OBSERVATIONS} held_ms>=100 retained_file_bytes={length}"
    );
    Ok(())
}
