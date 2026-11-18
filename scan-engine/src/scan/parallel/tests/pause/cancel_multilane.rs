use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread::ThreadId,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(20);
const PROGRESS_TIMEOUT: Duration = Duration::from_secs(60);
const LANE_COUNT: usize = 2;
const FIRST_BATCH_ITEMS: usize = BUFFERED_OBSERVATIONS / LANE_COUNT - 1;
const CANCEL_BATCH_ITEMS: usize = 2;

#[derive(Default)]
struct GateState {
    cancelled: bool,
    aborted: bool,
    release_reads: bool,
    progress_entered: bool,
    release_progress: bool,
    progress_wait_timed_out: bool,
    reader_threads: [Option<ThreadId>; LANE_COUNT],
    second_reads: [bool; LANE_COUNT],
    returned_cancel_batches: [bool; LANE_COUNT],
    parked_after_return: [bool; LANE_COUNT],
    cancel_boundaries: [bool; LANE_COUNT],
    allow_root_zero_item: bool,
    root_zero_item_sent: bool,
    progress_released_after_boundaries: bool,
    calls: [usize; LANE_COUNT],
    calls_after_cancel: [usize; LANE_COUNT],
    worker_finishes: usize,
}

#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl Gate {
    fn wait_for(&self, ready: impl Fn(&GateState) -> bool, message: &str) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "gate lock")?;
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, TEST_TIMEOUT, |state| !ready(state))
            .map_err(|_| "gate wait")?;
        if timeout.timed_out() && !ready(&state) {
            return Err(message.into());
        }
        Ok(())
    }

    fn release_reads(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.release_reads = true;
            self.changed.notify_all();
        }
    }

    fn cancel(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.cancelled = true;
            self.changed.notify_all();
        }
    }

    fn release_progress(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.progress_released_after_boundaries =
                state.root_zero_item_sent && state.cancel_boundaries.iter().all(|crossed| *crossed);
            state.release_progress = true;
            self.changed.notify_all();
        }
    }

    fn allow_root_zero_item(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.allow_root_zero_item = true;
            self.changed.notify_all();
        }
    }

    fn cleanup(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.cancelled = true;
            state.release_reads = true;
            state.release_progress = true;
            self.changed.notify_all();
        }
    }
}

struct Cleanup(Arc<Gate>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.0.cleanup();
    }
}

struct MultiContext {
    gate: Arc<Gate>,
    flow_id: AtomicUsize,
}

struct MultiFlow {
    gate: Arc<Gate>,
    flow_id: usize,
}

impl JobContext for MultiContext {
    fn checkpoint(&self) -> Checkpoint {
        if self
            .gate
            .state
            .lock()
            .is_ok_and(|state| state.cancelled || state.aborted)
        {
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
        let Ok(mut state) = self.gate.state.lock() else {
            return;
        };
        if state.progress_entered {
            return;
        }
        state.progress_entered = true;
        self.gate.changed.notify_all();
        while !state.release_progress && !state.aborted {
            let (next, timeout) = match self.gate.changed.wait_timeout(state, PROGRESS_TIMEOUT) {
                Ok(waited) => waited,
                Err(_) => return,
            };
            state = next;
            if timeout.timed_out() && !state.release_progress && !state.aborted {
                state.progress_wait_timed_out = true;
                state.aborted = true;
                self.gate.changed.notify_all();
                return;
            }
        }
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(MultiFlow {
            gate: self.gate.clone(),
            flow_id: self.flow_id.fetch_add(1, Ordering::SeqCst),
        }))
    }
}

fn wait_at_cancel_boundary(
    gate: &Gate,
    mut state: std::sync::MutexGuard<'_, GateState>,
    root: usize,
) -> Checkpoint {
    state.parked_after_return[root] = true;
    gate.changed.notify_all();
    if root == 0 && !state.allow_root_zero_item {
        while !state.allow_root_zero_item && !state.cancelled && !state.aborted {
            state = match gate.changed.wait(state) {
                Ok(state) => state,
                Err(_) => return Checkpoint::Cancel,
            };
        }
        if !state.cancelled && !state.aborted {
            return Checkpoint::Continue;
        }
    } else if root == 0 && !state.root_zero_item_sent {
        state.root_zero_item_sent = true;
        gate.changed.notify_all();
    }
    while !state.cancelled && !state.aborted {
        state = match gate.changed.wait(state) {
            Ok(state) => state,
            Err(_) => return Checkpoint::Cancel,
        };
    }
    state.cancel_boundaries[root] = true;
    gate.changed.notify_all();
    Checkpoint::Cancel
}

impl JobFlow for MultiFlow {
    fn safe_point(&self) -> Checkpoint {
        let Ok(state) = self.gate.state.lock() else {
            return Checkpoint::Cancel;
        };
        if self.flow_id > 0 {
            let thread = std::thread::current().id();
            let root = state
                .reader_threads
                .iter()
                .position(|reader| *reader == Some(thread));
            if let Some(root) = root
                && state.returned_cancel_batches[root]
            {
                return wait_at_cancel_boundary(&self.gate, state, root);
            }
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

    fn finish(self: Box<Self>, _: JobCounts) {
        if self.flow_id > 0
            && let Ok(mut state) = self.gate.state.lock()
        {
            state.worker_finishes += 1;
            self.gate.changed.notify_all();
        }
    }
}

struct MultiBatch {
    root: usize,
    gate: Arc<Gate>,
}

impl DirectoryObservations for MultiBatch {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let thread = std::thread::current().id();
        let mut state = self.gate.state.lock().map_err(|_| fail())?;
        let call = state.calls[self.root];
        state.calls[self.root] += 1;
        if call == 0 {
            state.reader_threads[self.root] = Some(thread);
            drop(state);
            return Ok(entries(self.root, b'F', FIRST_BATCH_ITEMS));
        }
        if call == 1 {
            state.second_reads[self.root] = true;
            self.gate.changed.notify_all();
            let (mut state, timeout) = self
                .gate
                .changed
                .wait_timeout_while(state, TEST_TIMEOUT, |state| {
                    !state.release_reads && !state.cancelled && !state.aborted
                })
                .map_err(|_| fail())?;
            if timeout.timed_out() && !state.release_reads {
                return Err(fail());
            }
            state.returned_cancel_batches[self.root] = true;
            self.gate.changed.notify_all();
            drop(state);
            return Ok(entries(self.root, b'C', CANCEL_BATCH_ITEMS));
        }
        if state.cancelled {
            state.calls_after_cancel[self.root] += 1;
        }
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for MultiBatch {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }

    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
    }
}

fn entries(root: usize, phase: u8, count: usize) -> Vec<ObservedEntry> {
    (0..count)
        .map(|index| {
            let name = format!("r{root}-{}-{index:04}", char::from(phase)).into_bytes();
            let phase_offset = if phase == b'C' { FIRST_BATCH_ITEMS } else { 0 };
            let inode =
                (root * (FIRST_BATCH_ITEMS + CANCEL_BATCH_ITEMS) + phase_offset + index + 1) as u64;
            ObservedEntry {
                name,
                enumerated_inode: inode,
                enumerated_kind: ObservedKind::File,
                metadata: Ok(MetadataObservation {
                    kind: ObservedKind::File,
                    device: root as u64 + 1,
                    inode,
                    size: Ok(1),
                    allocated: None,
                    link_count: None,
                    space_identity: None,
                    modified_at: None,
                    native_modified_at: None,
                }),
            }
        })
        .collect()
}

fn fail() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn coordinate_two_lane_cancel(gate: &Gate) -> Result<(bool, bool), String> {
    gate.wait_for(|state| state.progress_entered, "publisher did not pause")?;
    gate.wait_for(
        |state| state.second_reads.iter().all(|entered| *entered),
        "two lane reads did not overlap",
    )?;
    let distinct_workers = {
        let state = gate.state.lock().map_err(|_| "gate lock")?;
        state.reader_threads[0].is_some()
            && state.reader_threads[1].is_some()
            && state.reader_threads[0] != state.reader_threads[1]
    };
    gate.release_reads();
    gate.wait_for(
        |state| state.parked_after_return.iter().all(|parked| *parked),
        "both returned batches did not reach worker safe points",
    )?;
    // The first batches emitted 8,192 events. The publisher consumed one
    // before blocking, leaving 8,191 of 8,192 channel slots occupied.
    // One more item from root zero fills the final slot before cancellation.
    gate.allow_root_zero_item();
    gate.wait_for(
        |state| state.root_zero_item_sent,
        "root zero did not fill the final publisher slot",
    )?;
    gate.cancel();
    gate.wait_for(
        |state| state.cancel_boundaries.iter().all(|crossed| *crossed),
        "both workers did not cross the cancellation boundary",
    )?;
    let pressure_held_until_both_boundaries =
        gate.state
            .lock()
            .map_err(|_| "gate lock")
            .is_ok_and(|state| {
                state.root_zero_item_sent
                    && state.progress_entered
                    && !state.release_progress
                    && !state.aborted
                    && state.cancel_boundaries.iter().all(|crossed| *crossed)
            });
    Ok((distinct_workers, pressure_held_until_both_boundaries))
}

fn assert_cancelled_batch_items(items: &[du_core_contracts::result::ResultItem]) {
    let mut names = std::collections::HashMap::<Vec<u8>, usize>::new();
    let mut root_items = 0;
    for item in items {
        if item.name.is_empty() {
            root_items += 1;
        } else {
            *names.entry(item.name.clone()).or_default() += 1;
        }
    }
    assert_eq!(root_items, LANE_COUNT);
    assert_eq!(
        names.len(),
        LANE_COUNT * (FIRST_BATCH_ITEMS + CANCEL_BATCH_ITEMS)
    );
    assert!(names.values().all(|count| *count == 1));
    for root in 0..LANE_COUNT {
        for (phase, count) in [(b'F', FIRST_BATCH_ITEMS), (b'C', CANCEL_BATCH_ITEMS)] {
            for index in 0..count {
                let name = format!("r{root}-{}-{index:04}", char::from(phase)).into_bytes();
                assert_eq!(names.get(&name), Some(&1), "missing {name:?}");
            }
        }
    }
}

#[test]
fn cancel_drains_two_lanes_under_backpressure_once_and_stops_reads() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    assert_eq!(
        LANE_COUNT * (FIRST_BATCH_ITEMS + 1),
        BUFFERED_OBSERVATIONS,
        "two first batches and root reports must leave exactly one slot after one receive"
    );
    // The two fake adapters never call the OS. Their read barrier proves two
    // actual worker threads were inside adapter calls at the same time.
    let gate = Arc::new(Gate::default());
    let _cleanup = Cleanup(gate.clone());
    let context = Arc::new(MultiContext {
        gate: gate.clone(),
        flow_id: AtomicUsize::new(0),
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
    let run_gate = gate.clone();
    let run_context = context.clone();
    let run_ids = ids.clone();
    let collector = std::thread::spawn(move || {
        let include = |_: &[Vec<u8>]| true;
        let roots: Vec<Box<dyn AnchoredDirectory>> = (0..LANE_COUNT)
            .map(|root| {
                Box::new(MultiBatch {
                    root,
                    gate: run_gate.clone(),
                }) as Box<dyn AnchoredDirectory>
            })
            .collect();
        let control = crate::scan::parallel::Control {
            ids: run_ids.as_ref(),
            context: run_context.as_ref(),
            limit: LANE_COUNT,
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
                groups: vec![0, 1],
                limit: 1,
            },
            &clock,
            None,
            None,
        )
        .map_err(|error| format!("{error:?}"));
        let _ = done_tx.send(result);
    });

    let coordinated = coordinate_two_lane_cancel(&gate);

    // On every assertion, timeout, or early return, unblock both fake reads
    // and the publisher before joining the collector.
    if coordinated.is_ok() {
        gate.release_progress();
    } else {
        gate.cleanup();
    }
    let result = match done_rx.recv_timeout(TEST_TIMEOUT) {
        Ok(result) => result,
        Err(_) => {
            gate.cleanup();
            match done_rx.recv_timeout(TEST_TIMEOUT) {
                Ok(result) => result,
                Err(error) => {
                    drop(collector);
                    return Err(format!("collector remained blocked after cleanup: {error}"));
                }
            }
        }
    };
    collector.join().map_err(|_| "collector panicked")?;
    let (distinct_workers, pressure_held_until_both_boundaries) = coordinated?;
    assert!(
        distinct_workers,
        "both roots must be read by distinct workers"
    );
    assert!(pressure_held_until_both_boundaries);
    let (sealed_id, cancelled, _) = result?;
    assert!(cancelled);
    assert_eq!(context.flow_id.load(Ordering::SeqCst), 3); // prep plus two workers
    let state = gate.state.lock().map_err(|_| "gate lock")?;
    assert_eq!(state.calls, [2, 2]);
    assert_eq!(state.calls_after_cancel, [0, 0]);
    assert_eq!(state.cancel_boundaries, [true, true]);
    assert!(!state.progress_wait_timed_out);
    assert!(state.progress_released_after_boundaries);
    assert_eq!(state.worker_finishes, LANE_COUNT);
    drop(state);
    let version = store
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing cancelled version")?;
    assert_eq!(version.version_id(), &sealed_id);
    assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
    assert_cancelled_batch_items(version.items());
    Ok(())
}
