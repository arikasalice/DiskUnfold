//! Port-level instrumentation around the real platform directory adapter.
use super::RelativePath;
use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, DirectoryCaseEvidence, DirectoryObservations, FilterDecision,
        FilteredBatch, HandleIdentity, ObservedEntry, OpenedScopeRoot, ScopeRootPort,
        TimedFilteredBatch, TimedObservedEntry, VolumeSpaceReader,
    },
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, ThreadId},
    time::Duration,
};

const GATE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct State {
    armed: bool,
    gate_released: bool,
    post_release_read_start_observations: usize,
    started: Vec<(RelativePath, ThreadId)>,
    returned: Vec<(RelativePath, ThreadId)>,
    parked: Vec<(RelativePath, ThreadId)>,
    entries: BTreeMap<RelativePath, usize>,
    cancel_response_sequence: Option<usize>,
    gate_release_sequence: Option<usize>,
    events: Vec<TraceEvent>,
}

#[derive(Clone)]
struct TraceEvent {
    sequence: usize,
    name: &'static str,
    path: RelativePath,
    worker: ThreadId,
}

#[derive(Default)]
pub(super) struct Probe {
    sequence: AtomicUsize,
    state: Mutex<State>,
    changed: Condvar,
}

pub(super) struct Snapshot {
    pub(super) started: usize,
    pub(super) returned: usize,
    pub(super) post_release_read_start_observations: usize,
    pub(super) post_release_wrapper_entries: usize,
    pub(super) post_release_adapter_dispatches: usize,
    pub(super) post_reply_wrapper_entries: usize,
    pub(super) post_reply_adapter_dispatches: usize,
    pub(super) native_entries: BTreeMap<RelativePath, usize>,
    pub(super) workers_last_event_at_cancel_reply: Vec<String>,
    pub(super) trace: Vec<String>,
}

impl Probe {
    fn next_sequence(&self) -> usize {
        self.sequence.fetch_add(1, Ordering::SeqCst)
    }

    pub(super) fn arm(&self) -> Result<(), String> {
        self.state.lock().map_err(|_| "probe lock")?.armed = true;
        Ok(())
    }

    /// Records a wrapper read-start observation immediately before its real read call.
    fn read_started(&self, path: &[Vec<u8>]) -> Result<bool, PortError> {
        let sequence = self.next_sequence();
        let worker = thread::current().id();
        let mut state = self.state.lock().map_err(|_| gate_error())?;
        if !state.armed {
            return Ok(false);
        }
        // Keep the old lock-observation count so the trace can identify mutex-order artifacts.
        if state.gate_released {
            state.post_release_read_start_observations += 1;
        }
        state.started.push((path.to_vec(), worker));
        state.events.push(TraceEvent {
            sequence,
            name: "wrapper-entry",
            path: path.to_vec(),
            worker,
        });
        Ok(true)
    }

    /// The inner adapter has returned into this wrapper; the collector has not received the batch.
    fn adapter_returned(
        &self,
        path: &[Vec<u8>],
        adapter_dispatch_sequence: usize,
        entries: Option<&[TimedObservedEntry]>,
    ) -> Result<(), PortError> {
        let sequence = self.next_sequence();
        let worker = thread::current().id();
        let mut state = self.state.lock().map_err(|_| gate_error())?;
        state.events.push(TraceEvent {
            sequence: adapter_dispatch_sequence,
            name: "adapter-dispatch-marker",
            path: path.to_vec(),
            worker,
        });
        state.events.push(TraceEvent {
            sequence,
            name: "adapter-returned",
            path: path.to_vec(),
            worker,
        });
        state.returned.push((path.to_vec(), worker));
        if let Some(entries) = entries {
            for entry in entries {
                let mut item_path = path.to_vec();
                item_path.push(entry.entry.name.clone());
                *state.entries.entry(item_path).or_default() += 1;
            }
        }
        self.changed.notify_all();
        Ok(())
    }

    /// Parks a returned batch inside the wrapper until the caller releases the test gate.
    fn park_after_return(&self, path: &[Vec<u8>]) -> Result<(), PortError> {
        let sequence = self.next_sequence();
        let worker = thread::current().id();
        let mut state = self.state.lock().map_err(|_| gate_error())?;
        state.parked.push((path.to_vec(), worker));
        state.events.push(TraceEvent {
            sequence,
            name: "parked-after-return",
            path: path.to_vec(),
            worker,
        });
        self.changed.notify_all();
        let (mut state, timeout) = self
            .changed
            .wait_timeout_while(state, GATE_TIMEOUT, |state| !state.gate_released)
            .map_err(|_| gate_error())?;
        if timeout.timed_out() && !state.gate_released {
            return Err(gate_error());
        }
        let sequence = self.next_sequence();
        state.events.push(TraceEvent {
            sequence,
            name: "resumed-from-gate",
            path: path.to_vec(),
            worker,
        });
        Ok(())
    }

    pub(super) fn wait_for_two_workers(&self) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "probe lock")?;
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, GATE_TIMEOUT, |state| state.parked.len() < 2)
            .map_err(|_| "probe wait")?;
        if timeout.timed_out() && state.parked.len() < 2 {
            return Err("fewer than two real adapter reads reached the post-return gate".into());
        }
        let first = state.parked.first().ok_or("no gated worker")?.1;
        if !state.parked.iter().any(|(_, worker)| *worker != first) {
            return Err("the gated adapter reads did not run on distinct workers".into());
        }
        for (directory, _) in &state.parked {
            if !state
                .entries
                .keys()
                .any(|path| path.len() == directory.len() + 1 && path.starts_with(directory))
            {
                return Err("a gated native read returned no fixture entries".into());
            }
        }
        Ok(())
    }

    /// Releases parked wrappers. The main test calls this after the `job.cancel` reply, so reads
    /// between the runtime flag update and that reply are outside this observation window.
    pub(super) fn mark_cancel_response(&self) -> Result<(), String> {
        let sequence = self.next_sequence();
        let worker = thread::current().id();
        let mut state = self.state.lock().map_err(|_| "probe lock")?;
        state.cancel_response_sequence = Some(sequence);
        state.events.push(TraceEvent {
            sequence,
            name: "cancel-response-returned",
            path: Vec::new(),
            worker,
        });
        Ok(())
    }

    pub(super) fn release_test_gate(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "probe lock")?;
        if state.gate_released {
            return Ok(());
        }
        let sequence = self.next_sequence();
        let worker = thread::current().id();
        state.gate_release_sequence = Some(sequence);
        state.gate_released = true;
        state.events.push(TraceEvent {
            sequence,
            name: "gate-released",
            path: Vec::new(),
            worker,
        });
        self.changed.notify_all();
        Ok(())
    }

    pub(super) fn snapshot(&self) -> Result<Snapshot, String> {
        let state = self.state.lock().map_err(|_| "probe lock")?;
        let cancel_sequence = state
            .cancel_response_sequence
            .ok_or("missing cancel-response trace marker")?;
        let gate_sequence = state
            .gate_release_sequence
            .ok_or("missing gate-release trace marker")?;
        let mut events = state.events.clone();
        events.sort_by_key(|event| event.sequence);
        let count = |name: &str, after: usize| {
            events
                .iter()
                .filter(|event| event.name == name && event.sequence > after)
                .count()
        };
        let workers_last_event_at_cancel_reply = worker_states(&events, cancel_sequence);
        let trace = events
            .iter()
            .map(|event| {
                format!(
                    "seq={} event={} worker={:?} path={:?}",
                    event.sequence, event.name, event.worker, event.path
                )
            })
            .collect();
        Ok(Snapshot {
            started: state.started.len(),
            returned: state.returned.len(),
            post_release_read_start_observations: state.post_release_read_start_observations,
            post_release_wrapper_entries: count("wrapper-entry", gate_sequence),
            post_release_adapter_dispatches: count("adapter-dispatch-marker", gate_sequence),
            post_reply_wrapper_entries: count("wrapper-entry", cancel_sequence),
            post_reply_adapter_dispatches: count("adapter-dispatch-marker", cancel_sequence),
            native_entries: state.entries.clone(),
            workers_last_event_at_cancel_reply,
            trace,
        })
    }
}

fn worker_states(events: &[TraceEvent], before: usize) -> Vec<String> {
    let mut states: Vec<(ThreadId, &'static str, RelativePath)> = Vec::new();
    for event in events.iter().filter(|event| {
        event.sequence < before
            && matches!(
                event.name,
                "wrapper-entry"
                    | "adapter-dispatch-marker"
                    | "adapter-returned"
                    | "parked-after-return"
                    | "resumed-from-gate"
            )
    }) {
        if let Some(state) = states.iter_mut().find(|state| state.0 == event.worker) {
            state.1 = event.name;
            state.2 = event.path.clone();
        } else {
            states.push((event.worker, event.name, event.path.clone()));
        }
    }
    states
        .into_iter()
        .map(|(worker, event, path)| format!("worker={worker:?} last_event={event} path={path:?}"))
        .collect()
}

pub(super) struct ReleaseOnDrop(pub(super) Arc<Probe>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = self.0.release_test_gate();
    }
}

/// Wraps the platform's ScopeRootPort while preserving its native directory handles.
pub(super) struct ObservedRoots {
    pub(super) native: Arc<dyn ScopeRootPort>,
    pub(super) probe: Arc<Probe>,
}

impl ScopeRootPort for ObservedRoots {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let mut root = self.native.open_root(absolute_path)?;
        root.directory = Box::new(ObservedDirectory {
            inner: root.directory,
            path: Vec::new(),
            root: true,
            gated: false,
            probe: self.probe.clone(),
        });
        Ok(root)
    }
}

struct ObservedDirectory {
    inner: Box<dyn AnchoredDirectory>,
    path: RelativePath,
    root: bool,
    gated: bool,
    probe: Arc<Probe>,
}

impl DirectoryObservations for ObservedDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.next_batch_timed(&|| None)
            .map(|entries| entries.into_iter().map(|entry| entry.entry).collect())
    }

    fn next_batch_timed(
        &mut self,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<Vec<TimedObservedEntry>, PortError> {
        let instrumented = self.probe.read_started(&self.path)?;
        let should_gate = instrumented && !self.root && !self.gated;
        if should_gate {
            self.gated = true;
        }
        // This marker is sequenced immediately before delegation; the adapter call follows it.
        let adapter_dispatch_sequence = instrumented.then(|| self.probe.next_sequence());
        let result = self.inner.next_batch_timed(clock);
        if instrumented {
            self.probe.adapter_returned(
                &self.path,
                adapter_dispatch_sequence.ok_or_else(gate_error)?,
                result.as_ref().ok().map(Vec::as_slice),
            )?;
        }
        if should_gate {
            self.probe.park_after_return(&self.path)?;
        }
        result
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        // Filtered enumeration is delegated directly and is not instrumented by this probe.
        self.inner.next_filtered(include)
    }

    fn next_filtered_timed(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<TimedFilteredBatch, PortError> {
        self.inner.next_filtered_timed(include, clock)
    }
}

impl AnchoredDirectory for ObservedDirectory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let inner = self.inner.open_child(entry)?;
        let mut path = self.path.clone();
        path.push(entry.name.clone());
        Ok(Box::new(Self {
            inner,
            path,
            root: false,
            gated: false,
            probe: self.probe.clone(),
        }))
    }

    fn observe_directory_case(
        &self,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Option<DirectoryCaseEvidence> {
        self.inner.observe_directory_case(clock)
    }

    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        self.inner.volume_space_reader()
    }

    fn observed_modified_at(&self) -> Option<Timestamp> {
        self.inner.observed_modified_at()
    }

    fn observed_modified_at_timed(
        &self,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> (Option<Timestamp>, Option<Timestamp>) {
        self.inner.observed_modified_at_timed(clock)
    }

    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        self.inner.ancestor_file_ids()
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        self.inner.observed_identity()
    }
}

fn gate_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "scan.unclassified".into(),
        os_code: None,
    }
}
