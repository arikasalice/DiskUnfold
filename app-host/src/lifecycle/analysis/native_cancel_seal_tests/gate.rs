//! Port-level instrumentation around the real platform directory adapter.
use super::{
    Engine, FILES_PER_DIRECTORY, GATE_TIMEOUT, Id, RelativePath, WORKER_COUNT, native_name,
};
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
    collections::{BTreeMap, HashSet},
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

// Keep worker fallback later than the main barrier, which requires active parked evidence.
const WORKER_GATE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Default)]
struct State {
    armed: bool,
    gate_released: bool,
    post_release_read_starts: usize,
    started: Vec<(RelativePath, ThreadId)>,
    returned: Vec<(RelativePath, ThreadId)>,
    parked: Vec<(RelativePath, ThreadId)>,
    gate_exited: Vec<(RelativePath, ThreadId)>,
    gate_timeouts: usize,
    entries: BTreeMap<RelativePath, usize>,
    entry_sizes: BTreeMap<RelativePath, Vec<Option<u64>>>,
}

#[derive(Default)]
pub(super) struct Probe {
    state: Mutex<State>,
    changed: Condvar,
    #[cfg(test)]
    wait_timeout_override: Mutex<Option<bool>>,
}

impl Probe {
    pub(super) fn arm(&self) -> Result<(), String> {
        self.state.lock().map_err(|_| "probe lock")?.armed = true;
        Ok(())
    }

    /// Records a wrapper read-start observation immediately before its real read call.
    fn read_started(&self, path: &[Vec<u8>]) -> Result<bool, PortError> {
        let mut state = self.state.lock().map_err(|_| gate_error())?;
        if !state.armed {
            return Ok(false);
        }
        if state.gate_released {
            state.post_release_read_starts += 1;
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
                *state.entries.entry(item_path.clone()).or_default() += 1;
                let size = entry
                    .entry
                    .metadata
                    .as_ref()
                    .ok()
                    .and_then(|metadata| metadata.size.as_ref().ok().copied());
                state.entry_sizes.entry(item_path).or_default().push(size);
            }
        }
        self.changed.notify_all();
        Ok(())
    }

    /// Parks a returned batch inside the wrapper until the caller releases the test gate.
    fn park_after_return(&self, path: &[Vec<u8>]) -> Result<(), PortError> {
        let mut state = self.state.lock().map_err(|_| gate_error())?;
        let worker = thread::current().id();
        state.parked.push((path.to_vec(), worker));
        self.changed.notify_all();
        let (mut state, timeout) = self
            .changed
            .wait_timeout_while(state, WORKER_GATE_TIMEOUT, |state| !state.gate_released)
            .map_err(|_| gate_error())?;
        let gate_timed_out = timeout.timed_out() && !state.gate_released;
        if gate_timed_out {
            state.gate_timeouts += 1;
        }
        state.gate_exited.push((path.to_vec(), worker));
        self.changed.notify_all();
        if gate_timed_out {
            return Err(gate_error());
        }
        Ok(())
    }

    pub(super) fn wait_for_four_workers(&self) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "probe lock")?;
        #[cfg(test)]
        let injected_timeout = self
            .wait_timeout_override
            .lock()
            .map_err(|_| "probe wait")?
            .take();
        #[cfg(not(test))]
        let injected_timeout: Option<bool> = None;
        let (state, timeout_timed_out) = if let Some(timed_out) = injected_timeout {
            (state, timed_out)
        } else {
            let (state, timeout) = self
                .changed
                .wait_timeout_while(state, GATE_TIMEOUT, |state| {
                    !state.gate_released && active_workers(state).len() < WORKER_COUNT
                })
                .map_err(|_| "probe wait")?;
            (state, timeout.timed_out())
        };
        let active = validate_four_worker_wait(&state, timeout_timed_out)?;
        for (directory, _) in &active {
            let observed: BTreeMap<_, _> = state
                .entries
                .iter()
                .filter(|(path, _)| {
                    path.len() == directory.len() + 1 && path.starts_with(directory)
                })
                .filter_map(|(path, count)| path.last().map(|name| (name.clone(), *count)))
                .collect();
            let expected: BTreeMap<_, _> = (0..FILES_PER_DIRECTORY)
                .map(|file| {
                    let name = format!("item-{file:02}.bin");
                    (native_name(&name), 1)
                })
                .collect();
            if observed != expected {
                return Err(format!(
                    "gated adapter read for fixture directory {directory:?} returned {observed:?}, expected {expected:?}"
                ));
            }
            for name in expected.keys() {
                let mut path = directory.clone();
                path.push(name.clone());
                if !matches!(state.entry_sizes.get(&path), Some(sizes) if sizes.len() == 1 && sizes[0] == Some(2))
                {
                    return Err(format!(
                        "gated adapter read for fixture file {path:?} did not return its expected 2-byte metadata"
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn wait_for_parked_workers_to_exit(&self, deadline: Instant) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "probe lock")?;
        let timeout = deadline.saturating_duration_since(Instant::now());
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, timeout, |state| {
                state.gate_exited.len() < state.parked.len()
            })
            .map_err(|_| "probe exit wait")?;
        if timeout.timed_out() && state.gate_exited.len() < state.parked.len() {
            return Err(format!(
                "only {} of {} parked adapter workers exited the test gate before timeout",
                state.gate_exited.len(),
                state.parked.len()
            ));
        }
        if state.gate_timeouts != 0 {
            return Err(format!(
                "{} adapter workers left the gate on worker timeout instead of test release",
                state.gate_timeouts
            ));
        }
        let exited: HashSet<_> = state.gate_exited.iter().cloned().collect();
        if state.parked.iter().any(|worker| !exited.contains(worker)) {
            return Err("a parked adapter worker has no matching gate-exit observation".into());
        }
        Ok(())
    }

    /// Releases parked wrappers. The main test calls this after the `job.cancel` reply, so reads
    /// between the runtime flag update and that reply are outside this observation window.
    pub(super) fn release_test_gate(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.gate_released = true;
        self.changed.notify_all();
    }

    pub(super) fn snapshot(
        &self,
    ) -> Result<(usize, usize, usize, BTreeMap<RelativePath, usize>), String> {
        let state = self.state.lock().map_err(|_| "probe lock")?;
        Ok((
            state.started.len(),
            state.returned.len(),
            state.post_release_read_starts,
            state.entries.clone(),
        ))
    }
}

pub(super) struct ReleaseOnDrop<'a> {
    probe: Arc<Probe>,
    engine: &'a Engine,
    job: Option<Id>,
    finished: bool,
}

impl<'a> ReleaseOnDrop<'a> {
    pub(super) fn new(probe: Arc<Probe>, engine: &'a Engine) -> Self {
        Self {
            probe,
            engine,
            job: None,
            finished: false,
        }
    }

    pub(super) fn track_job(&mut self, job: Id) {
        self.job = Some(job);
    }

    pub(super) fn finish(&mut self) -> Result<(), String> {
        let result = self.cleanup();
        self.finished = true;
        result
    }

    fn cleanup(&mut self) -> Result<(), String> {
        self.probe.release_test_gate();
        let deadline = Instant::now() + GATE_TIMEOUT;
        let mut errors = Vec::new();
        if let Some(job) = self.job.take()
            && let Err(error) = super::stop_job_before(self.engine, &job, deadline)
        {
            errors.push(format!("scan job cleanup failed: {error}"));
        }
        if let Err(error) = self.probe.wait_for_parked_workers_to_exit(deadline) {
            errors.push(error);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        if !self.finished {
            if let Err(error) = self.cleanup() {
                eprintln!("F02_NATIVE_CANCEL_SEAL cleanup incomplete: {error}");
            }
            self.finished = true;
        }
    }
}

fn distinct_workers(parked: &[(RelativePath, ThreadId)]) -> HashSet<ThreadId> {
    parked.iter().map(|(_, worker)| *worker).collect()
}

fn active_parked(state: &State) -> Vec<(RelativePath, ThreadId)> {
    state
        .parked
        .iter()
        .filter(|parked| !state.gate_exited.contains(*parked))
        .cloned()
        .collect()
}

fn active_workers(state: &State) -> HashSet<ThreadId> {
    distinct_workers(&active_parked(state))
}

fn validate_four_worker_wait(
    state: &State,
    timeout_timed_out: bool,
) -> Result<Vec<(RelativePath, ThreadId)>, String> {
    let active = active_parked(state);
    let workers = distinct_workers(&active);
    if !worker_barrier_is_ready(workers.len(), state.gate_released) {
        let boundary = if timeout_timed_out {
            "barrier timeout"
        } else if state.gate_released {
            "gate release"
        } else {
            "wait completion"
        };
        return Err(format!(
            "four-worker evidence is insufficient: {} distinct workers remained actively parked at {boundary}",
            workers.len(),
        ));
    }
    if workers.len() != WORKER_COUNT {
        return Err(format!(
            "expected exactly {WORKER_COUNT} distinct adapter workers, observed {}",
            workers.len()
        ));
    }
    Ok(active)
}

fn worker_barrier_is_ready(active_worker_count: usize, gate_released: bool) -> bool {
    !gate_released && active_worker_count >= WORKER_COUNT
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

#[cfg(test)]
mod tests;
