//! Opt-in per-scan collection-buffer diagnostics.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

/// Read-only scan measurements after the collection workers have stopped.
#[derive(Default)]
pub struct ScanDiagnostics {
    ready_peak: Mutex<Option<usize>>,
    ended_folder_attempts: AtomicU64,
    observations: Arc<ObservationMetrics>,
    frames: Arc<DirectoryFrameMetrics>,
    visited: Arc<VisitedMetrics>,
    correlated_budget: Arc<CorrelatedBudgetMetrics>,
}

impl ScanDiagnostics {
    /// Greatest number of frames held in all shared lane queues at once.
    ///
    /// Initial roots count. Worker overflow stacks and in-flight frames do not.
    pub fn shared_ready_peak(&self) -> Option<usize> {
        self.ready_peak.lock().ok().and_then(|peak| *peak)
    }

    /// Greatest number of events waiting in the observation channel, sampled by the collector
    /// right before it takes the next event. Only the collector removes events, so the channel
    /// only grows between two of its receives and each sample sees that interval's largest
    /// value; the collector then holds no event, so the value never exceeds the channel
    /// capacity. A send still blocked for capacity, or one whose success is not yet counted, is
    /// not included.
    pub fn observation_channel_peak_events(&self) -> usize {
        self.observations.peak_events.load(Ordering::Acquire)
    }

    /// Greatest estimated bytes held by events in the observation channel, sampled like the
    /// event peak above. The estimate adds
    /// `size_of::<Event>()`, path component vector capacities and byte capacities, boxed
    /// observation/input values and their known owned capacities, and directory-case reason-key
    /// capacity. It excludes allocator bookkeeping, pending sends and scan-side values outside
    /// the channel.
    pub fn observation_channel_peak_bytes(&self) -> usize {
        self.observations.peak_bytes.load(Ordering::Acquire)
    }

    /// Greatest number of directory frames in lane queues, worker overflow stacks and worker
    /// turns combined. A claimed frame remains counted while in flight. This excludes each
    /// frame's entries, deferred observations, directory handle state and scheduler indexes.
    pub fn directory_frames_peak_count(&self) -> usize {
        self.frames.peak_count.load(Ordering::Acquire)
    }

    /// Greatest estimated path heap capacity held by the directory frames counted above. The
    /// estimate is `path.capacity() * size_of::<Vec<u8>>()` plus each component's byte capacity;
    /// it excludes the `Frame` struct, handles, entry batches, allocator bookkeeping and other
    /// scan state.
    pub fn directory_frames_peak_bytes(&self) -> usize {
        self.frames.peak_bytes.load(Ordering::Acquire)
    }

    /// Greatest number of unique directory identities in the visited set. Scope-root identities
    /// count; verified ancestor identities do not. Files are not inserted.
    pub fn visited_entries_peak(&self) -> usize {
        self.visited.peak.load(Ordering::Acquire)
    }

    /// Number of unique directory identities in the visited set after scan workers stop, if
    /// preparation created that set. Scope-root identities count; verified ancestor identities
    /// and files do not.
    pub fn visited_entries_final(&self) -> Option<usize> {
        self.visited
            .final_recorded
            .load(Ordering::Acquire)
            .then(|| self.visited.final_count.load(Ordering::Acquire))
    }

    /// Greatest number of bytes reserved from the correlated-collection budget. This includes
    /// its fixed observation-channel allowance and active attempt/event permits; it excludes
    /// allocations that were not charged to that budget.
    pub fn correlated_budget_peak_managed_bytes(&self) -> usize {
        self.correlated_budget.peak.load(Ordering::Acquire)
    }

    /// Folder attempts whose end the receiving build accepted, once the workers stopped.
    pub fn ended_folder_attempts(&self) -> u64 {
        self.ended_folder_attempts.load(Ordering::Acquire)
    }

    pub(crate) fn record_ready_peak(&self, value: usize) {
        if let Ok(mut peak) = self.ready_peak.lock() {
            *peak = Some(value);
        }
    }

    pub(crate) fn record_ended_folder_attempts(&self, value: u64) {
        self.ended_folder_attempts.store(value, Ordering::Release);
    }

    pub(crate) fn observation_metrics(&self) -> Arc<ObservationMetrics> {
        Arc::clone(&self.observations)
    }

    pub(crate) fn frame_metrics(&self) -> Arc<DirectoryFrameMetrics> {
        Arc::clone(&self.frames)
    }

    pub(crate) fn visited_metrics(&self) -> Arc<VisitedMetrics> {
        Arc::clone(&self.visited)
    }

    pub(crate) fn correlated_budget_metrics(&self) -> Arc<CorrelatedBudgetMetrics> {
        Arc::clone(&self.correlated_budget)
    }
}

/// Sent and received totals of the observation channel. Senders count a send after it
/// succeeds; only the collector counts receives and samples the peaks, right before it takes
/// the next event. Every counted send is then either in the channel or already counted as
/// received, so a sample never exceeds the channel capacity.
#[derive(Default)]
pub(crate) struct ObservationMetrics {
    sent_events: AtomicUsize,
    received_events: AtomicUsize,
    sent_bytes: AtomicUsize,
    received_bytes: AtomicUsize,
    peak_events: AtomicUsize,
    peak_bytes: AtomicUsize,
}

impl ObservationMetrics {
    pub(crate) fn sent(&self, bytes: usize) {
        self.sent_events.fetch_add(1, Ordering::Relaxed);
        self.sent_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Called by the collector only, right after it takes an event.
    pub(crate) fn received(&self, bytes: usize) {
        self.received_events.fetch_add(1, Ordering::Relaxed);
        self.received_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Called by the collector only, while it holds no event, before each receive.
    pub(crate) fn sample(&self) {
        let events = self
            .sent_events
            .load(Ordering::Relaxed)
            .saturating_sub(self.received_events.load(Ordering::Relaxed));
        let bytes = self
            .sent_bytes
            .load(Ordering::Relaxed)
            .saturating_sub(self.received_bytes.load(Ordering::Relaxed));
        self.peak_events.fetch_max(events, Ordering::Relaxed);
        self.peak_bytes.fetch_max(bytes, Ordering::Relaxed);
    }
}

/// Atomic live and peak frame accounting shared by the scan's frame guards.
#[derive(Default)]
pub(crate) struct DirectoryFrameMetrics {
    current_count: AtomicUsize,
    current_bytes: AtomicUsize,
    peak_count: AtomicUsize,
    peak_bytes: AtomicUsize,
}

impl DirectoryFrameMetrics {
    pub(crate) fn add(&self, path_bytes: usize) {
        let count = self.current_count.fetch_add(1, Ordering::Relaxed) + 1;
        let bytes = self.current_bytes.fetch_add(path_bytes, Ordering::Relaxed) + path_bytes;
        self.peak_count.fetch_max(count, Ordering::Relaxed);
        self.peak_bytes.fetch_max(bytes, Ordering::Relaxed);
    }

    pub(crate) fn remove(&self, path_bytes: usize) {
        self.current_count.fetch_sub(1, Ordering::Relaxed);
        self.current_bytes.fetch_sub(path_bytes, Ordering::Relaxed);
    }
}

/// Holds a frame in the live and peak totals until the frame is dropped.
pub(crate) struct FrameAccounting {
    metrics: Arc<DirectoryFrameMetrics>,
    path_bytes: usize,
}

impl FrameAccounting {
    pub(crate) fn new(metrics: Arc<DirectoryFrameMetrics>, path_bytes: usize) -> Self {
        metrics.add(path_bytes);
        Self {
            metrics,
            path_bytes,
        }
    }
}

impl Drop for FrameAccounting {
    fn drop(&mut self) {
        self.metrics.remove(self.path_bytes);
    }
}

/// Final and peak counts for unique entered directory identities.
#[derive(Default)]
pub(crate) struct VisitedMetrics {
    peak: AtomicUsize,
    final_count: AtomicUsize,
    final_recorded: AtomicBool,
}

impl VisitedMetrics {
    pub(crate) fn record_peak(&self, entries: usize) {
        self.peak.fetch_max(entries, Ordering::Relaxed);
    }

    pub(crate) fn record_final(&self, entries: usize) {
        self.final_count.store(entries, Ordering::Release);
        self.final_recorded.store(true, Ordering::Release);
    }
}

/// Peak bytes charged to correlated collection during this scan.
#[derive(Default)]
pub(crate) struct CorrelatedBudgetMetrics {
    peak: AtomicUsize,
}

impl CorrelatedBudgetMetrics {
    pub(crate) fn record_peak(&self, bytes: usize) {
        self.peak.fetch_max(bytes, Ordering::Relaxed);
    }
}
