use super::*;
use du_core_contracts::{Timestamp, scan_observation::TimedObservedEntry};

#[cfg(windows)]
mod native_files;

struct LateBatch {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    calls: Arc<AtomicUsize>,
}
impl DirectoryObservations for LateBatch {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Err(fail())
    }
    fn next_batch_timed(
        &mut self,
        _: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<Vec<TimedObservedEntry>, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.send(()).map_err(|_| fail())?;
        self.release.recv().map_err(|_| fail())?;
        Ok((1..=8192)
            .map(|n| TimedObservedEntry {
                entry: ObservedEntry {
                    name: format!("f{n:04}").into_bytes(),
                    enumerated_inode: n,
                    enumerated_kind: ObservedKind::File,
                    metadata: Ok(MetadataObservation {
                        kind: ObservedKind::File,
                        device: 1,
                        inode: n,
                        size: Ok(1),
                        allocated: None,
                        link_count: None,
                        space_identity: None,
                        modified_at: None,
                        native_modified_at: None,
                    }),
                },
                observed_at: Timestamp::parse("2026-01-02T03:04:05.000Z").ok(),
                placeholder_observation: None,
            })
            .collect())
    }
}
impl AnchoredDirectory for LateBatch {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
    }
}

#[test]
fn cancel_during_inflight_batch_retains_returned_observations_without_more_reads()
-> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let control = Arc::new(Control::default());
    let context = Arc::new(PauseContext(control.clone()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let store = IngestSink::default();
    let build = store.open_forest(
        Ids(AtomicUsize::new(99)).next_id(),
        vec![observed_root("C:\\A", 1)],
    );
    let ctx = context.clone();
    let probe = calls.clone();
    let worker = std::thread::spawn(move || {
        let ids = Ids(AtomicUsize::new(1));
        let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(LateBatch {
            entered: entered_tx,
            release: release_rx,
            calls: probe,
        })];
        collect_open_forest(build, roots, &ids, &*ctx, 1, &|_| true)
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    {
        let mut state = control.state.lock().map_err(|_| "state")?;
        state.cancelled = true;
        control.changed.notify_all();
    }
    release_tx.send(()).map_err(|e| e.to_string())?;
    let outcome = worker.join().map_err(|_| "join")?;
    assert!(!outcome.completed_scope);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let version = store
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing version")?;
    assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
    assert_ne!(
        version
            .space(0)
            .ok_or("missing root aggregate")?
            .completeness,
        ResultCompleteness::Complete
    );
    assert_eq!(version.items().len(), 8193);
    let observed = Timestamp::parse("2026-01-02T03:04:05.000Z")?;
    assert_eq!(version.item_observed_at(1), Some(&observed));
    assert_eq!(version.item_observed_at(8192), Some(&observed));
    Ok(())
}

struct TwoBatches {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    calls: Arc<AtomicUsize>,
}
impl DirectoryObservations for TwoBatches {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 1 {
            self.entered.send(()).map_err(|_| fail())?;
            self.release.recv().map_err(|_| fail())?;
        }
        let name = match call {
            0 => b"first".as_slice(),
            1 => b"second".as_slice(),
            _ => return Ok(Vec::new()),
        };
        let inode = (call + 1) as u64;
        Ok(vec![ObservedEntry {
            name: name.to_vec(),
            enumerated_inode: inode,
            enumerated_kind: ObservedKind::File,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::File,
                device: 1,
                inode,
                size: Ok(1),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
                native_modified_at: None,
            }),
        }])
    }
}
impl AnchoredDirectory for TwoBatches {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
    }
}

#[test]
fn cancel_while_parked_drains_batch_without_mutating_previous_partial() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let control = Arc::new(Control::default());
    let context = Arc::new(PauseContext(control.clone()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let store = IngestSink::default();
    let build = store.open_forest(
        Ids(AtomicUsize::new(99)).next_id(),
        vec![observed_root("C:\\A", 1)],
    );
    let ctx = context.clone();
    let probe = calls.clone();
    let worker = std::thread::spawn(move || {
        let ids = Ids(AtomicUsize::new(1));
        let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(TwoBatches {
            entered: entered_tx,
            release: release_rx,
            calls: probe,
        })];
        collect_open_forest(build, roots, &ids, &*ctx, 1, &|_| true)
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let old = loop {
        if let Some(version) = store.pin().map_err(|e| format!("{e:?}"))? {
            break version;
        }
        if std::time::Instant::now() >= deadline {
            return Err("missing partial".into());
        }
        std::thread::yield_now();
    };
    assert_eq!(old.items().len(), 2);
    {
        let mut s = control.state.lock().map_err(|_| "state")?;
        s.paused = true;
    }
    release_tx.send(()).map_err(|e| e.to_string())?;
    let state = control.state.lock().map_err(|_| "state")?;
    let (mut state, timeout) = control
        .changed
        .wait_timeout_while(state, Duration::from_secs(3), |s| s.waiting == 0)
        .map_err(|_| "state")?;
    assert!(!timeout.timed_out());
    state.cancelled = true;
    control.changed.notify_all();
    drop(state);
    let outcome = worker.join().map_err(|_| "join")?;
    assert!(!outcome.completed_scope);
    let result = store
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing final")?;
    assert_eq!(result.completeness(), ResultCompleteness::Cancelled);
    assert_eq!(result.items().len(), 3);
    assert_eq!(old.items().len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}
