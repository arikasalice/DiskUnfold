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
    other_adapter_entered: bool,
}

#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl Gate {
    fn wait_for_other_adapter(&self) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "gate lock")?;
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, TEST_TIMEOUT, |state| !state.other_adapter_entered)
            .map_err(|_| "gate wait")?;
        if timeout.timed_out() && !state.other_adapter_entered {
            return Err("other worker did not enter its adapter call".into());
        }
        Ok(())
    }

    fn enter_other_adapter(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.other_adapter_entered = true;
            self.changed.notify_all();
        }
    }

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
    adapter_release: Option<mpsc::Sender<()>>,
    progress_release: Option<mpsc::Sender<()>>,
}

impl UnblockOnDrop {
    fn release(&mut self) {
        let _ = self.gate.cancel();
        if let Some(sender) = self.adapter_release.take() {
            let _ = sender.send(());
        }
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
    progress_released: AtomicBool,
    progress_timed_out: AtomicBool,
    progress_disconnected: AtomicBool,
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
                match release.recv_timeout(TEST_TIMEOUT) {
                    Ok(()) => self.progress_released.store(true, Ordering::Release),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        self.progress_timed_out.store(true, Ordering::Release);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        self.progress_disconnected.store(true, Ordering::Release);
                    }
                }
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
    gate: Arc<Gate>,
}

impl DirectoryObservations for ReturnedBatches {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            self.gate.wait_for_other_adapter().map_err(|_| fail())?;
        }
        let entries = match call {
            0 => (0..BUFFERED_OBSERVATIONS).map(file_entry).collect(),
            1 => vec![file_entry(BUFFERED_OBSERVATIONS)],
            _ => Vec::new(),
        };
        self.returned_batches.store(call + 1, Ordering::Release);
        Ok(entries)
    }
}

struct OtherWorkerCall {
    gate: Arc<Gate>,
    release: mpsc::Receiver<()>,
    calls: Arc<AtomicUsize>,
    returned: Arc<AtomicBool>,
    returned_after_cancel: Arc<AtomicBool>,
}

impl DirectoryObservations for OtherWorkerCall {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) != 0 {
            return Ok(Vec::new());
        }
        self.gate.enter_other_adapter();
        self.release
            .recv_timeout(Duration::from_secs(60))
            .map_err(|_| fail())?;
        let cancelled = self.gate.state.lock().is_ok_and(|state| state.cancelled);
        self.returned_after_cancel
            .store(cancelled, Ordering::Release);
        self.returned.store(true, Ordering::Release);
        Ok(vec![file_entry_on_device(BUFFERED_OBSERVATIONS + 1, 2)])
    }
}

impl AnchoredDirectory for OtherWorkerCall {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }

    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
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
    file_entry_on_device(index, 1)
}

fn file_entry_on_device(index: usize, device: u64) -> ObservedEntry {
    let inode = index as u64 + 1;
    ObservedEntry {
        name: format!("batch-{index:04}").into_bytes(),
        enumerated_inode: inode,
        enumerated_kind: ObservedKind::File,
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::File,
            device,
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
    let other_calls = Arc::new(AtomicUsize::new(0));
    let other_returned = Arc::new(AtomicBool::new(false));
    let other_returned_after_cancel = Arc::new(AtomicBool::new(false));
    let gate = Arc::new(Gate::default());
    let (adapter_release_tx, adapter_release_rx) = mpsc::channel();
    let (progress_entered_tx, progress_entered_rx) = mpsc::channel();
    let (progress_release_tx, progress_release_rx) = mpsc::channel();
    let mut cleanup = UnblockOnDrop {
        gate: gate.clone(),
        adapter_release: Some(adapter_release_tx),
        progress_release: Some(progress_release_tx),
    };
    let context = Arc::new(SaturatedContext {
        gate: gate.clone(),
        returned_batches: returned_batches.clone(),
        progress_entered: progress_entered_tx,
        progress_release: Mutex::new(progress_release_rx),
        progress_waited: AtomicBool::new(false),
        progress_released: AtomicBool::new(false),
        progress_timed_out: AtomicBool::new(false),
        progress_disconnected: AtomicBool::new(false),
    });
    let ids = Arc::new(Ids(AtomicUsize::new(1)));
    let store = IngestSink::default();
    let build = store
        .open_forest(
            Ids(AtomicUsize::new(99)).next_id(),
            vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)],
        )
        .map_err(|error| format!("{error:?}"))?;
    let (done_tx, done_rx) = mpsc::channel();
    let run_context = context.clone();
    let run_ids = ids.clone();
    let run_calls = read_calls.clone();
    let run_returned_batches = returned_batches.clone();
    let run_gate = gate.clone();
    let run_other_calls = other_calls.clone();
    let run_other_returned = other_returned.clone();
    let run_other_returned_after_cancel = other_returned_after_cancel.clone();
    let worker = std::thread::spawn(move || {
        let include = |_: &[Vec<u8>]| true;
        let roots: Vec<Box<dyn AnchoredDirectory>> = vec![
            Box::new(ReturnedBatches {
                calls: run_calls,
                returned_batches: run_returned_batches,
                gate: run_gate.clone(),
            }),
            Box::new(OtherWorkerCall {
                gate: run_gate,
                release: adapter_release_rx,
                calls: run_other_calls,
                returned: run_other_returned,
                returned_after_cancel: run_other_returned_after_cancel,
            }),
        ];
        let control = crate::scan::parallel::Control {
            ids: run_ids.as_ref(),
            context: run_context.as_ref(),
            limit: 2,
            policy: Policy::Legacy(&include),
        };
        let clock = ContextClock(run_context.as_ref());
        let result = body::run(
            build,
            roots,
            control,
            &mut JobCounts::default(),
            &clock,
            crate::scan::parallel::ScanOptions {
                devices: DevicePlan {
                    groups: vec![0, 1],
                    limit: 1,
                },
                diagnostics: None,
                scan_control: None,
                correlated_budget: None,
            },
        )
        .map_err(|error| format!("{error:?}"));
        let _ = done_tx.send(result);
    });

    progress_entered_rx
        .recv_timeout(TEST_TIMEOUT)
        .map_err(|error| format!("waiting for publisher stop: {error}"))?;
    gate.wait_for_other_adapter()?;
    gate.wait_for_boundary()?;
    assert_eq!(returned_batches.load(Ordering::Acquire), 2);
    assert_eq!(read_calls.load(Ordering::SeqCst), 2);
    assert_eq!(other_calls.load(Ordering::SeqCst), 1);
    assert!(!other_returned.load(Ordering::Acquire));

    // The consumer is blocked after the first item. The producer fills the
    // remaining slots before its second read returns; the other worker stays
    // inside its adapter until cancellation releases it.
    cleanup.release();

    // shortcut: a collector ignoring release signals may outlive this bounded wait.
    let result = done_rx
        .recv_timeout(TEST_TIMEOUT)
        .map_err(|error| format!("collector did not finish: {error}"))?;
    worker.join().map_err(|_| "collector panicked")?;
    assert!(
        !context.progress_timed_out.load(Ordering::Acquire),
        "publisher wait timed out; the channel may no longer have been saturated at the boundary"
    );
    assert!(
        !context.progress_disconnected.load(Ordering::Acquire),
        "publisher release channel disconnected before explicit release"
    );
    assert!(
        context.progress_released.load(Ordering::Acquire),
        "publisher wait ended without an explicit release"
    );
    let (sealed_id, cancelled, _) = result?;
    assert!(cancelled);
    assert_eq!(read_calls.load(Ordering::SeqCst), 2);
    assert_eq!(other_calls.load(Ordering::SeqCst), 1);
    assert!(other_returned.load(Ordering::Acquire));
    assert!(other_returned_after_cancel.load(Ordering::Acquire));

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
        .chain(std::iter::once(
            format!("batch-{:04}", BUFFERED_OBSERVATIONS + 1).into_bytes(),
        ))
        .collect();
    assert_eq!(final_version.items().len(), expected.len() + 1);
    assert_eq!(actual, expected);
    let second_worker_item = final_version
        .items()
        .iter()
        .find(|item| item.name == format!("batch-{:04}", BUFFERED_OBSERVATIONS + 1).into_bytes())
        .ok_or("missing second worker item")?;
    let second_worker_root = final_version
        .observed_root_for(&ItemRef {
            result_version: final_version.version_id().clone(),
            item_no: second_worker_item.item_no,
        })
        .map_err(|error| format!("{error:?}"))?
        .0;
    assert_eq!(second_worker_root, 1);
    Ok(())
}
