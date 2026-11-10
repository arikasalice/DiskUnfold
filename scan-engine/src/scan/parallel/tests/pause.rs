mod cancel;
use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Default)]
struct State {
    paused: bool,
    cancelled: bool,
    aborted: bool,
    active: usize,
    waiting: usize,
}
#[derive(Default)]
struct Control {
    state: Mutex<State>,
    changed: Condvar,
}
struct PauseContext(Arc<Control>);
struct PauseFlow(Arc<Control>);
impl JobContext for PauseContext {
    fn checkpoint(&self) -> Checkpoint {
        if self.0.state.lock().is_ok_and(|s| s.cancelled) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn abort_flows(&self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.aborted = true;
            self.0.changed.notify_all();
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        let mut state = self.0.state.lock().map_err(|_| JobFlowError::Closed)?;
        if state.cancelled {
            return Err(JobFlowError::Cancelled);
        }
        if state.aborted {
            return Err(JobFlowError::Closed);
        }
        if state.paused {
            return Err(JobFlowError::Busy);
        }
        state.active += 1;
        Ok(Box::new(PauseFlow(self.0.clone())))
    }
}
impl JobFlow for PauseFlow {
    fn safe_point(&self) -> Checkpoint {
        let Ok(mut state) = self.0.state.lock() else {
            return Checkpoint::Cancel;
        };
        if state.paused && !state.cancelled && !state.aborted {
            state.waiting += 1;
            self.0.changed.notify_all();
            while state.paused && !state.cancelled && !state.aborted {
                state = match self.0.changed.wait(state) {
                    Ok(s) => s,
                    Err(_) => return Checkpoint::Cancel,
                };
            }
            state.waiting -= 1;
        }
        if state.cancelled || state.aborted {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }
    fn finish(self: Box<Self>, _: JobCounts) {}
}
impl Drop for PauseFlow {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.state.lock() {
            s.active -= 1;
            self.0.changed.notify_all();
        }
    }
}

struct BlockedBatch {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    calls: Arc<AtomicUsize>,
    sent: bool,
}
impl DirectoryObservations for BlockedBatch {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.sent {
            return Ok(Vec::new());
        }
        self.sent = true;
        self.entered.send(()).map_err(|_| fail())?;
        self.release.recv().map_err(|_| fail())?;
        Ok((1..=16)
            .map(|n| ObservedEntry {
                name: format!("item{n}").into_bytes(),
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
                }),
            })
            .collect())
    }
}
impl AnchoredDirectory for BlockedBatch {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
    }
}
fn fail() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

#[test]
fn pause_waits_for_inflight_batch_idle_flow_parks_and_resume_does_not_duplicate()
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
    let task_store = store.clone();
    let task_context = context.clone();
    let task_calls = calls.clone();
    let handle = std::thread::spawn(move || {
        let ids = Ids(AtomicUsize::new(1));
        let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(BlockedBatch {
            entered: entered_tx,
            release: release_rx,
            calls: task_calls,
            sent: false,
        })];
        let outcome = collect_open_forest(build, roots, &ids, &*task_context, 2, &|_| true);
        (outcome, task_store)
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    {
        let mut s = control.state.lock().map_err(|_| "state")?;
        s.paused = true;
    }
    // The blocked OS call has not reached its boundary, so both flows cannot yet be parked.
    {
        let s = control.state.lock().map_err(|_| "state")?;
        assert!(s.waiting < s.active);
    }
    release_tx.send(()).map_err(|e| e.to_string())?;
    let s = control.state.lock().map_err(|_| "state")?;
    let (mut s, timeout) = control
        .changed
        .wait_timeout_while(s, Duration::from_secs(3), |s| {
            s.active == 0 || s.waiting != s.active
        })
        .map_err(|_| "state")?;
    assert!(!timeout.timed_out());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    s.paused = false;
    control.changed.notify_all();
    drop(s);
    let (outcome, store) = handle.join().map_err(|_| "join")?;
    assert!(outcome.completed_scope);
    let version = store
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(version.items().len(), 17);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn abort_wakes_parked_flow_without_user_cancellation() -> Result<(), String> {
    let control = Arc::new(Control::default());
    let context = PauseContext(control.clone());
    let flow = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    {
        let mut s = control.state.lock().map_err(|_| "state")?;
        s.paused = true;
    }
    let (done_tx, done_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let result = flow.safe_point();
        let _ = done_tx.send(result);
        flow.finish(JobCounts::default());
    });
    let s = control.state.lock().map_err(|_| "state")?;
    let (s, timeout) = control
        .changed
        .wait_timeout_while(s, Duration::from_secs(3), |s| s.waiting == 0)
        .map_err(|_| "state")?;
    assert!(!timeout.timed_out());
    drop(s);
    context.abort_flows();
    assert_eq!(
        done_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?,
        Checkpoint::Cancel
    );
    assert_eq!(context.checkpoint(), Checkpoint::Continue);
    assert!(matches!(
        context.try_start_flow(),
        Err(JobFlowError::Closed)
    ));
    handle.join().map_err(|_| "join")?;
    Ok(())
}

struct PauseDuringRegistration {
    control: Arc<Control>,
    attempts: AtomicUsize,
}
impl JobContext for PauseDuringRegistration {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        let mut state = self
            .control
            .state
            .lock()
            .map_err(|_| JobFlowError::Closed)?;
        if attempt == 2 {
            state.paused = true;
            return Err(JobFlowError::Busy);
        }
        state.active += 1;
        Ok(Box::new(PauseFlow(self.control.clone())))
    }
}
#[test]
fn partial_registration_keeps_capacity_and_workers_can_reach_barrier() -> Result<(), String> {
    let control = Arc::new(Control::default());
    let context = PauseDuringRegistration {
        control: control.clone(),
        attempts: AtomicUsize::new(0),
    };
    let flows = acquire_flows(&context, 4).map_err(|e| format!("{e:?}"))?;
    assert_eq!(flows.len(), 2);
    let workers: Vec<_> = flows
        .into_iter()
        .map(|flow| {
            std::thread::spawn(move || {
                let result = flow.safe_point();
                flow.finish(JobCounts::default());
                result
            })
        })
        .collect();
    let state = control.state.lock().map_err(|_| "state")?;
    let (mut state, timeout) = control
        .changed
        .wait_timeout_while(state, Duration::from_secs(3), |s| s.waiting != 2)
        .map_err(|_| "state")?;
    assert!(!timeout.timed_out());
    assert_eq!(state.active, 2);
    state.paused = false;
    control.changed.notify_all();
    drop(state);
    for worker in workers {
        assert_eq!(worker.join().map_err(|_| "join")?, Checkpoint::Continue);
    }
    Ok(())
}

struct VolumeRoot {
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<Option<mpsc::Receiver<()>>>,
    calls: Arc<AtomicUsize>,
}
struct VolumeReader {
    entered: Option<mpsc::Sender<()>>,
    release: Option<mpsc::Receiver<()>>,
    calls: Arc<AtomicUsize>,
}
impl DirectoryObservations for VolumeRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for VolumeRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
    }
    fn volume_space_reader(
        &self,
    ) -> Result<Box<dyn du_core_contracts::scan_observation::VolumeSpaceReader>, PortError> {
        Ok(Box::new(VolumeReader {
            entered: self.entered.lock().map_err(|_| fail())?.take(),
            release: self.release.lock().map_err(|_| fail())?.take(),
            calls: self.calls.clone(),
        }))
    }
}
impl du_core_contracts::scan_observation::VolumeSpaceReader for VolumeReader {
    fn read(
        &self,
        _: &[u8],
    ) -> Result<du_core_contracts::scan_observation::VolumeSpace, PortError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            self.entered
                .as_ref()
                .ok_or_else(fail)?
                .send(())
                .map_err(|_| fail())?;
            self.release
                .as_ref()
                .ok_or_else(fail)?
                .recv()
                .map_err(|_| fail())?;
        }
        Ok(du_core_contracts::scan_observation::VolumeSpace {
            total_bytes: 10,
            used_bytes: 2,
            free_bytes: 8,
        })
    }
}
#[test]
fn initial_volume_io_is_own_flow_and_parks_after_return() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let control = Arc::new(Control::default());
    let context = Arc::new(PauseContext(control.clone()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let sink = IngestSink::default();
    let build = sink.open_forest(
        Ids(AtomicUsize::new(99)).next_id(),
        vec![observed_root("C:\\A", 1)],
    );
    let task_context = context.clone();
    let task_calls = calls.clone();
    let task = std::thread::spawn(move || {
        let ids = Ids(AtomicUsize::new(1));
        let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(VolumeRoot {
            entered: Mutex::new(Some(entered_tx)),
            release: Mutex::new(Some(release_rx)),
            calls: task_calls,
        })];
        collect_open_forest(build, roots, &ids, &*task_context, 2, &|_| true)
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    {
        let mut s = control.state.lock().map_err(|_| "state")?;
        assert_eq!(s.active, 1);
        s.paused = true;
        assert_eq!(s.waiting, 0);
    }
    release_tx.send(()).map_err(|e| e.to_string())?;
    let s = control.state.lock().map_err(|_| "state")?;
    let (mut s, timeout) = control
        .changed
        .wait_timeout_while(s, Duration::from_secs(3), |s| s.waiting != 1)
        .map_err(|_| "state")?;
    assert!(!timeout.timed_out());
    assert_eq!(s.active, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    s.paused = false;
    control.changed.notify_all();
    drop(s);
    assert!(task.join().map_err(|_| "join")?.completed_scope);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

struct PanickingBatch {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl DirectoryObservations for PanickingBatch {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let _ = self.entered.send(());
        let _ = self.release.recv();
        std::panic::resume_unwind(Box::new("injected observation failure"))
    }
}
impl AnchoredDirectory for PanickingBatch {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
    }
}
#[test]
fn worker_panic_wakes_parked_flow_and_restores_previous_version() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let control = Arc::new(Control::default());
    let context = Arc::new(PauseContext(control.clone()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let store = IngestSink::default();
    let mut prior = store
        .open(Ids(AtomicUsize::new(70)).next_id())
        .map_err(|e| format!("{e:?}"))?;
    prior
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let baseline = prior
        .seal(
            Ids(AtomicUsize::new(71)).next_id(),
            ResultCompleteness::Complete,
        )
        .map_err(|e| format!("{e:?}"))?;
    let build = store.open_forest(
        Ids(AtomicUsize::new(72)).next_id(),
        vec![observed_root("C:\\A", 1)],
    );
    let task_context = context.clone();
    let handle = std::thread::spawn(move || {
        let ids = Ids(AtomicUsize::new(1));
        let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(PanickingBatch {
            entered: entered_tx,
            release: release_rx,
        })];
        let outcome = collect_open_forest(build, roots, &ids, &*task_context, 2, &|_| true);
        let _ = done_tx.send(outcome);
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    {
        let mut state = control.state.lock().map_err(|_| "state")?;
        state.paused = true;
    }
    let state = control.state.lock().map_err(|_| "state")?;
    let (state, timeout) = control
        .changed
        .wait_timeout_while(state, Duration::from_secs(3), |s| s.waiting == 0)
        .map_err(|_| "state")?;
    assert!(!timeout.timed_out());
    assert!(state.waiting < state.active);
    drop(state);
    release_tx.send(()).map_err(|e| e.to_string())?;
    let outcome = done_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert!(!outcome.completed_scope);
    assert_eq!(
        outcome.failure_reason_key.as_deref(),
        Some("support.internal_error")
    );
    assert!(outcome.result_version_id.is_none());
    assert_eq!(context.checkpoint(), Checkpoint::Continue);
    assert_eq!(
        store
            .pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing baseline")?
            .version_id(),
        baseline.version_id()
    );
    handle.join().map_err(|_| "join")?;
    Ok(())
}
