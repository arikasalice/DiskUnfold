use super::*;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
struct GateState {
    cancelled: bool,
    aborted: bool,
    at_boundary: bool,
}

#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl Gate {
    fn wait_for_boundary(&self) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "gate lock")?;
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, TEST_TIMEOUT, |state| !state.at_boundary)
            .map_err(|_| "gate wait")?;
        if timeout.timed_out() && !state.at_boundary {
            return Err("worker did not reach the post-return boundary".into());
        }
        Ok(())
    }

    fn cancel(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "gate lock")?;
        state.cancelled = true;
        self.changed.notify_all();
        Ok(())
    }
}

struct UnblockOnDrop {
    gate: Arc<Gate>,
    progress_release: Option<mpsc::Sender<()>>,
}

impl UnblockOnDrop {
    fn release(&mut self) {
        let _ = self.gate.cancel();
        if let Some(sender) = self.progress_release.take() {
            let _ = sender.send(());
        }
    }
}

impl Drop for UnblockOnDrop {
    fn drop(&mut self) {
        self.release();
    }
}

struct SaturatedContext {
    gate: Arc<Gate>,
    returned_batches: Arc<AtomicUsize>,
    progress_entered: mpsc::Sender<()>,
    progress_release: Mutex<mpsc::Receiver<()>>,
    progress_waited: AtomicBool,
}

struct SaturatedFlow {
    gate: Arc<Gate>,
    returned_batches: Arc<AtomicUsize>,
}

impl JobContext for SaturatedContext {
    fn checkpoint(&self) -> Checkpoint {
        if self.gate.state.lock().is_ok_and(|state| state.cancelled) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }

    fn abort_flows(&self) {
        if let Ok(mut state) = self.gate.state.lock() {
            state.aborted = true;
            self.gate.changed.notify_all();
        }
    }

    fn report_progress(&self, _: du_core_contracts::job::JobProgress) {
        if !self.progress_waited.swap(true, Ordering::SeqCst) {
            let _ = self.progress_entered.send(());
            if let Ok(release) = self.progress_release.lock() {
                let _ = release.recv_timeout(TEST_TIMEOUT);
            }
        }
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(SaturatedFlow {
            gate: self.gate.clone(),
            returned_batches: self.returned_batches.clone(),
        }))
    }
}

impl JobFlow for SaturatedFlow {
    fn safe_point(&self) -> Checkpoint {
        if self.returned_batches.load(Ordering::Acquire) < 2 {
            return Checkpoint::Continue;
        }
        let Ok(mut state) = self.gate.state.lock() else {
            return Checkpoint::Cancel;
        };
        state.at_boundary = true;
        self.gate.changed.notify_all();
        let (state, timeout) =
            match self
                .gate
                .changed
                .wait_timeout_while(state, TEST_TIMEOUT, |state| {
                    !state.cancelled && !state.aborted
                }) {
                Ok(result) => result,
                Err(_) => return Checkpoint::Cancel,
            };
        if timeout.timed_out() && !state.cancelled && !state.aborted {
            return Checkpoint::Cancel;
        }
        Checkpoint::Cancel
    }

    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }

    fn finish(self: Box<Self>, _: JobCounts) {}
}

struct ReturnedBatches {
    calls: Arc<AtomicUsize>,
    returned_batches: Arc<AtomicUsize>,
}

impl DirectoryObservations for ReturnedBatches {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let entries = match call {
            0 => (0..BUFFERED_OBSERVATIONS).map(file_entry).collect(),
            1 => vec![file_entry(BUFFERED_OBSERVATIONS)],
            _ => Vec::new(),
        };
        self.returned_batches.store(call + 1, Ordering::Release);
        Ok(entries)
    }
}

impl AnchoredDirectory for ReturnedBatches {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }

    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
    }
}

fn file_entry(index: usize) -> ObservedEntry {
    let inode = index as u64 + 1;
    ObservedEntry {
        name: format!("batch-{index:04}").into_bytes(),
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
fn cancellation_at_full_channel_drains_returned_observations_and_seals_that_version()
-> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let returned_batches = Arc::new(AtomicUsize::new(0));
    let read_calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Gate::default());
    let (progress_entered_tx, progress_entered_rx) = mpsc::channel();
    let (progress_release_tx, progress_release_rx) = mpsc::channel();
    let mut cleanup = UnblockOnDrop {
        gate: gate.clone(),
        progress_release: Some(progress_release_tx),
    };
    let context = Arc::new(SaturatedContext {
        gate: gate.clone(),
        returned_batches: returned_batches.clone(),
        progress_entered: progress_entered_tx,
        progress_release: Mutex::new(progress_release_rx),
        progress_waited: AtomicBool::new(false),
    });
    let ids = Arc::new(Ids(AtomicUsize::new(1)));
    let store = IngestSink::default();
    let build = store
        .open_forest(
            Ids(AtomicUsize::new(99)).next_id(),
            vec![observed_root("C:\\A", 1)],
        )
        .map_err(|error| format!("{error:?}"))?;
    let (done_tx, done_rx) = mpsc::channel();
    let run_context = context.clone();
    let run_ids = ids.clone();
    let run_calls = read_calls.clone();
    let run_returned_batches = returned_batches.clone();
    let worker = std::thread::spawn(move || {
        let include = |_: &[Vec<u8>]| true;
        let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(ReturnedBatches {
            calls: run_calls,
            returned_batches: run_returned_batches,
        })];
        let control = crate::scan::parallel::Control {
            ids: run_ids.as_ref(),
            context: run_context.as_ref(),
            limit: 1,
            policy: Policy::Legacy(&include),
            scan_control: None,
        };
        let clock = ContextClock(run_context.as_ref());
        let result = body::run(
            build,
            roots,
            control,
            &mut JobCounts::default(),
            DevicePlan {
                groups: vec![0],
                limit: 1,
            },
            &clock,
            None,
        )
        .map_err(|error| format!("{error:?}"));
        let _ = done_tx.send(result);
    });

    progress_entered_rx
        .recv_timeout(TEST_TIMEOUT)
        .map_err(|error| format!("waiting for publisher stop: {error}"))?;
    gate.wait_for_boundary()?;
    assert_eq!(returned_batches.load(Ordering::Acquire), 2);
    assert_eq!(read_calls.load(Ordering::SeqCst), 2);

    // The consumer is blocked after the first item. The worker sends the
    // first batch and RootBatch into the 8,192-slot channel before its second
    // read returns one more entry and reaches this safe point.
    cleanup.release();

    let result = done_rx
        .recv_timeout(TEST_TIMEOUT)
        .map_err(|error| format!("collector did not finish: {error}"))?;
    worker.join().map_err(|_| "collector panicked")?;
    let (sealed_id, cancelled, _) = result?;
    assert!(cancelled);
    assert_eq!(read_calls.load(Ordering::SeqCst), 2);

    let final_version = store
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing sealed version")?;
    assert_eq!(final_version.version_id(), &sealed_id);
    assert_eq!(final_version.completeness(), ResultCompleteness::Cancelled);

    let actual: BTreeSet<_> = final_version
        .items()
        .iter()
        .map(|item| item.name.clone())
        .collect();
    let expected: BTreeSet<_> = std::iter::once(Vec::new())
        .chain((0..=BUFFERED_OBSERVATIONS).map(|index| format!("batch-{index:04}").into_bytes()))
        .collect();
    assert_eq!(final_version.items().len(), expected.len());
    assert_eq!(actual, expected);
    Ok(())
}
