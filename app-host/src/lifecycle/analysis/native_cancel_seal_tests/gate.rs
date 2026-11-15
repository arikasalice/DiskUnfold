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
    sync::{Arc, Condvar, Mutex},
    thread::{self, ThreadId},
    time::Duration,
};

const GATE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct State {
    armed: bool,
    cancelled: bool,
    released: bool,
    late_reads: usize,
    started: Vec<(RelativePath, ThreadId)>,
    returned: Vec<(RelativePath, ThreadId)>,
    parked: Vec<(RelativePath, ThreadId)>,
    entries: BTreeMap<RelativePath, usize>,
}

#[derive(Default)]
pub(super) struct Probe {
    state: Mutex<State>,
    changed: Condvar,
}

impl Probe {
    pub(super) fn arm(&self) -> Result<(), String> {
        self.state.lock().map_err(|_| "probe lock")?.armed = true;
        Ok(())
    }

    /// Records entry to the adapter port immediately before its real read call.
    fn read_started(&self, path: &[Vec<u8>]) -> Result<bool, PortError> {
        let mut state = self.state.lock().map_err(|_| gate_error())?;
        if !state.armed {
            return Ok(false);
        }
        if state.cancelled {
            state.late_reads += 1;
        }
        state.started.push((path.to_vec(), thread::current().id()));
        Ok(true)
    }

    /// The inner adapter has returned into this wrapper; the collector has not received the batch.
    fn adapter_returned(
        &self,
        path: &[Vec<u8>],
        entries: Option<&[TimedObservedEntry]>,
    ) -> Result<(), PortError> {
        let mut state = self.state.lock().map_err(|_| gate_error())?;
        state.returned.push((path.to_vec(), thread::current().id()));
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
        let mut state = self.state.lock().map_err(|_| gate_error())?;
        state.parked.push((path.to_vec(), thread::current().id()));
        self.changed.notify_all();
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, GATE_TIMEOUT, |state| !state.released)
            .map_err(|_| gate_error())?;
        if timeout.timed_out() && !state.released {
            return Err(gate_error());
        }
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

    /// Records the test's cancel point and releases parked wrappers. JobContext cancellation is
    /// requested separately through app-host's `job.cancel` use case.
    pub(super) fn cancel_and_release(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.cancelled = true;
            state.released = true;
            self.changed.notify_all();
        }
    }

    pub(super) fn snapshot(
        &self,
    ) -> Result<(usize, usize, usize, BTreeMap<RelativePath, usize>), String> {
        let state = self.state.lock().map_err(|_| "probe lock")?;
        Ok((
            state.started.len(),
            state.returned.len(),
            state.late_reads,
            state.entries.clone(),
        ))
    }
}

pub(super) struct ReleaseOnDrop(pub(super) Arc<Probe>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.cancel_and_release();
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
        let result = self.inner.next_batch_timed(clock);
        if instrumented {
            self.probe
                .adapter_returned(&self.path, result.as_ref().ok().map(Vec::as_slice))?;
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
