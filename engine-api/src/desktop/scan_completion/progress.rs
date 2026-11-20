//! Relays batched progress and per-lane status to the desktop event queue.
use crate::{desktop::DesktopEvents, jobs::wait_reason};
use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Timestamp,
    analysis_ipc::{
        AnalysisJobProgress, JOB_PROGRESS, MAX_LANE_ROOTS, MAX_LANES, SCAN_LANE_STATUS, ScanLane,
        ScanLaneStatus, ScanLaneWait,
    },
    job::LaneWait,
};
use du_job_runtime::{JobEvents, JobLaneStatusEvent, JobProgressEvent};

/// A runtime event sink that can also relay batched job progress.
pub(crate) trait ProgressEvents: JobEvents {
    /// Relays one progress value on the delivery thread, in job-sequence order.
    /// The default hands the value to [`JobEvents::publish_progress`].
    fn relay_progress(&self, event: JobProgressEvent) {
        self.publish_progress(event);
    }

    /// Relays one lane status value on the delivery thread, in job-sequence order.
    fn relay_lane_status(&self, event: JobLaneStatusEvent) {
        self.publish_lane_status(event);
    }
}

impl ProgressEvents for DesktopEvents {
    fn relay_progress(&self, event: JobProgressEvent) {
        if let Some(event) = envelope(event) {
            self.enqueue(event);
        }
    }

    fn relay_lane_status(&self, event: JobLaneStatusEvent) {
        if let Some(event) = lane_status_envelope(event) {
            self.enqueue(event);
        }
    }
}

/// Maps a runtime value to the `job.progress` envelope; the relay batches it.
fn envelope(event: JobProgressEvent) -> Option<EventEnvelope> {
    let observed_at = Timestamp::from_unix_millis(event.observed_at_ms)?;
    let body = AnalysisJobProgress {
        sequence: event.sequence,
        unit: event.unit,
        completed: event.completed,
        total: event.total,
        total_known: event.total.is_some(),
        wait_reason: wait_reason(event.wait_reason),
        observed_at: observed_at.clone(),
    };
    Some(EventEnvelope {
        topic: JOB_PROGRESS.into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: Some(event.job_id),
        // The optional latest partial result version is not carried yet.
        result_version: None,
        occurred_at: observed_at,
        body: serde_json::to_value(body).ok()?,
    })
}

/// Maps a runtime value to the bounded `scan.lane_status` wire envelope.
fn lane_status_envelope(event: JobLaneStatusEvent) -> Option<EventEnvelope> {
    let observed_at = Timestamp::from_unix_millis(event.observed_at_ms)?;
    let mut lanes = event.status.lanes;
    lanes.sort_by_key(|lane| lane.lane_index);
    let lane_count = (lanes.len() > MAX_LANES).then(|| lanes.len().to_string());
    let lanes = lanes
        .into_iter()
        .take(MAX_LANES)
        .map(|lane| {
            let mut root_indexes = lane.root_indexes;
            root_indexes.sort_unstable();
            let root_count =
                (root_indexes.len() > MAX_LANE_ROOTS).then(|| root_indexes.len().to_string());
            ScanLane {
                lane_index: lane.lane_index,
                root_indexes: root_indexes.into_iter().take(MAX_LANE_ROOTS).collect(),
                root_count,
                in_flight: lane.in_flight,
                limit: lane.limit,
                mode: lane.mode,
                permit: lane.permit,
                strategy: lane.strategy,
                wait_reason: match lane.wait {
                    LaneWait::IoWait => ScanLaneWait::IoWait,
                    LaneWait::Backpressure => ScanLaneWait::Backpressure,
                    LaneWait::DeviceCap => ScanLaneWait::DeviceCap,
                    LaneWait::None => ScanLaneWait::None,
                },
                wait_confirmed: lane.wait_confirmed,
                remaining_directories: lane.remaining_directories.to_string(),
                failures: lane.failures.to_string(),
            }
        })
        .collect();
    let body = ScanLaneStatus {
        sequence: event.sequence,
        lanes,
        lane_count,
        observed_at: observed_at.clone(),
    };
    Some(EventEnvelope {
        topic: SCAN_LANE_STATUS.into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: Some(event.job_id),
        result_version: None,
        occurred_at: observed_at,
        body: serde_json::to_value(body).ok()?,
    })
}

#[cfg(test)]
mod tests;
