//! Observation events and their bounded channel sender.
use super::{correlated_budget, lane_meter};
use crate::scan::admission::Admission;
use du_core_contracts::scan_observation::{
    CorrelatedDirectoryEvent, DirectoryCaseEvidence, TimedObservedEntry,
};
use std::sync::{
    Arc,
    mpsc::{self, SyncSender},
};

pub(super) enum Event {
    /// Registration is independent of the attempt event sequence.
    FolderAttemptBegin {
        root_index: u32,
        path: Vec<Vec<u8>>,
        attempt_id: u64,
        reply: mpsc::Sender<
            Result<du_analysis_index::FolderAttemptToken, du_analysis_index::FolderAttemptError>,
        >,
        _permit: correlated_budget::Permit,
    },
    /// A result is acknowledged only after the receiving build accepts it.
    FolderAttemptRecord {
        token: du_analysis_index::FolderAttemptToken,
        seq: u64,
        input: Box<du_analysis_index::FolderAttemptInput>,
        reply: Option<mpsc::Sender<Result<(), du_analysis_index::FolderAttemptError>>>,
        _permit: Option<correlated_budget::Permit>,
    },
    /// Acknowledged EOF barrier, sent only after every numbered event was acknowledged.
    FolderAttemptEnd {
        token: du_analysis_index::FolderAttemptToken,
        event_watermark_exclusive: u64,
        child_count_exclusive: u64,
        reply: mpsc::Sender<Result<(), du_analysis_index::FolderAttemptError>>,
    },
    Item(
        u32,
        Vec<Vec<u8>>,
        Box<TimedObservedEntry>,
        Admission,
        Option<correlated_budget::Permit>,
    ),
    /// A correlated observation whose dynamic payload and path are charged to the scan budget.
    CorrelatedItem(
        u32,
        Vec<Vec<u8>>,
        Box<TimedObservedEntry>,
        Admission,
        correlated_budget::Permit,
    ),
    /// A name withheld by an undecidable exclusion rule; it was never observed.
    Undecided(u32, Vec<Vec<u8>>),
    DirectoryCase(u32, Vec<Vec<u8>>, DirectoryCaseEvidence),
    /// Entries an exclusion rule dropped directly below a registered directory.
    Excluded {
        root_index: u32,
        path: Vec<Vec<u8>>,
        count: u32,
        complete: bool,
    },
    /// A registered directory that cancellation left unopened or partly read.
    Unread(u32, Vec<Vec<u8>>),
    Failure {
        root_index: u32,
        path: Vec<Vec<u8>>,
        root: bool,
        reason: &'static str,
        stage: du_core_contracts::scan_failures::ScanFailureStage,
        observed_at: Option<du_core_contracts::Timestamp>,
    },
    RootBatch(
        u32,
        Option<du_core_contracts::Timestamp>,
        Option<du_core_contracts::Timestamp>,
    ),
}

impl Event {
    /// Estimates the channel's inline event, owned path buffers and boxed observation payload.
    pub(super) fn estimated_buffer_bytes(&self) -> usize {
        let path_bytes = event_path(self).map_or(0, |path| {
            correlated_budget::path_heap_capacity(path, path.capacity()).unwrap_or(usize::MAX)
        });
        let entry_bytes = match self {
            Self::Item(_, _, timed, _, _) | Self::CorrelatedItem(_, _, timed, _, _) => {
                std::mem::size_of::<TimedObservedEntry>().saturating_add(
                    correlated_budget::timed_heap_capacity(timed).unwrap_or(usize::MAX),
                )
            }
            Self::FolderAttemptRecord { input, .. } => {
                std::mem::size_of::<du_analysis_index::FolderAttemptInput>()
                    .saturating_add(folder_attempt_input_heap_capacity(input).unwrap_or(usize::MAX))
            }
            _ => 0,
        };
        let reason_bytes = match self {
            Self::DirectoryCase(_, _, evidence) => evidence
                .reason_key
                .as_ref()
                .map_or(0, |reason| reason.capacity()),
            _ => 0,
        };
        std::mem::size_of::<Self>()
            .saturating_add(path_bytes)
            .saturating_add(entry_bytes)
            .saturating_add(reason_bytes)
    }
}

fn folder_attempt_input_heap_capacity(
    input: &du_analysis_index::FolderAttemptInput,
) -> Option<usize> {
    match input {
        du_analysis_index::FolderAttemptInput::Adapter(
            CorrelatedDirectoryEvent::FilterCallback { raw_name, .. },
        ) => Some(raw_name.capacity()),
        du_analysis_index::FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
            entry,
            ..
        }) => correlated_budget::timed_heap_capacity(entry),
        _ => Some(0),
    }
}

fn event_path(event: &Event) -> Option<&Vec<Vec<u8>>> {
    match event {
        Event::FolderAttemptBegin { path, .. }
        | Event::Item(_, path, ..)
        | Event::CorrelatedItem(_, path, ..)
        | Event::Undecided(_, path)
        | Event::DirectoryCase(_, path, _)
        | Event::Unread(_, path)
        | Event::Failure { path, .. }
        | Event::Excluded { path, .. } => Some(path),
        Event::FolderAttemptRecord { .. }
        | Event::FolderAttemptEnd { .. }
        | Event::RootBatch(..) => None,
    }
}

/// The bounded observation sender with optional per-scan occupancy accounting.
#[derive(Clone)]
pub(super) struct EventSender {
    sender: SyncSender<Event>,
    metrics: Option<Arc<crate::scan_diagnostics::ObservationMetrics>>,
}

impl EventSender {
    pub(super) fn new(
        sender: SyncSender<Event>,
        metrics: Option<Arc<crate::scan_diagnostics::ObservationMetrics>>,
    ) -> Self {
        Self { sender, metrics }
    }

    pub(super) fn send_metered(&self, event: Event) -> Result<(), mpsc::SendError<Event>> {
        if matches!(event, Event::Failure { .. }) {
            lane_meter::failed();
        }
        let bytes = self
            .metrics
            .as_ref()
            .map(|_| event.estimated_buffer_bytes());
        let result = lane_meter::send(&self.sender, event);
        if result.is_ok()
            && let (Some(metrics), Some(bytes)) = (&self.metrics, bytes)
        {
            metrics.sent(bytes);
        }
        result
    }

    pub(super) fn try_send(&self, event: Event) -> Result<(), mpsc::TrySendError<Event>> {
        let bytes = self
            .metrics
            .as_ref()
            .map(|_| event.estimated_buffer_bytes());
        let result = self.sender.try_send(event);
        if result.is_ok()
            && let (Some(metrics), Some(bytes)) = (&self.metrics, bytes)
        {
            metrics.sent(bytes);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan_diagnostics::ScanDiagnostics;
    use std::{thread, time::Instant};

    #[test]
    fn slow_collector_counts_the_bounded_channel_without_waiting_sends() -> Result<(), String> {
        let capacity = super::super::BUFFERED_OBSERVATIONS;
        let diagnostics = ScanDiagnostics::default();
        let metrics = diagnostics.observation_metrics();
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let sender = EventSender::new(sender, Some(Arc::clone(&metrics)));
        let (ready, started) = mpsc::sync_channel(1);
        let producer = thread::spawn(move || -> Result<(), String> {
            ready.send(()).map_err(|_| "ready receiver closed")?;
            for _ in 0..=capacity {
                sender
                    .send_metered(Event::RootBatch(0, None, None))
                    .map_err(|_| "observation receiver closed")?;
            }
            Ok(())
        });
        started.recv().map_err(|_| "producer did not start")?;
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while diagnostics.observation_channel_peak_events() < capacity && Instant::now() < deadline
        {
            thread::yield_now();
        }
        assert_eq!(diagnostics.observation_channel_peak_events(), capacity);

        for _ in 0..=capacity {
            let event = receiver.recv().map_err(|_| "observation channel closed")?;
            metrics.received(event.estimated_buffer_bytes());
        }
        producer
            .join()
            .map_err(|_| "observation producer panicked")??;
        assert_eq!(diagnostics.observation_channel_peak_events(), capacity);
        assert_eq!(
            diagnostics.observation_channel_peak_bytes(),
            capacity.saturating_mul(std::mem::size_of::<Event>())
        );
        Ok(())
    }
}
