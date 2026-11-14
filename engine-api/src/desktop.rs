//! Compose the desktop analysis use cases with injected OS and storage ports.
//!
//! Registration lives in `assembly`; this module owns the bounded desktop event stream.
use std::{
    sync::{Mutex, atomic::AtomicU64},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) use crate::recent_scope::recent_scope_admissions;
pub use crate::recent_scope::{
    RecentScopeAdmission, RecentScopeAdmissions, RecentScopeDelivery, RecentScopeDeliveryReceipt,
};
use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Timestamp,
    analysis_ipc::{TargetProbeEvent, TargetStatusChanged},
    history_ipc::{HISTORY_CHANGED, HistoryChangedBody},
    job::{HistoryChangeReason, HistoryChanged, HistoryRecovery},
    result_events::RESULT_VERSION_CHANGED,
};
use du_job_runtime::{JobEvent, JobEvents};
use serde_json::json;

mod assembly;
mod sample_sink;
pub(crate) mod scan_completion;
mod shutdown;
#[cfg(feature = "shutdown-test-support")]
pub mod shutdown_fixture;
mod visualization;
use crate::jobs;
pub use crate::scan::{ScanDiagnosticsFactory, SharedTableLimit, SharedTableLimitSource};
pub use assembly::{
    DesktopServices, register, register_services, register_services_with_recent_scope_worker,
    register_with_scan_diagnostics, register_with_shared_table_limit,
};
pub use du_scan_engine::scan_job::ScanDiagnostics;
pub use scan_completion::CompletedScanSource;
pub use visualization::{VisualizationPreview, VisualizationServices};

mod locale_events;
mod pending;
#[cfg(test)]
use du_core_contracts::analysis_ipc::{JobResyncReason, JobResyncRequired};
#[cfg(test)]
use pending::{EVENT_INTERVAL, EVENT_LIMIT};
use pending::{Pending, next_loss_order};

fn utc_now_ms() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

fn utc_now_timestamp() -> Option<Timestamp> {
    let now = SystemTime::now();
    let millis = match now.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).ok()?,
        Err(error) => i64::try_from(error.duration().as_millis())
            .ok()?
            .checked_neg()?,
    };
    Timestamp::from_unix_millis(millis)
}

#[cfg(test)]
mod delivery_failure_tests;
#[cfg(test)]
mod sample_tests;
#[cfg(test)]
mod tests;

/// A bounded, nonblocking event source for the desktop window.
#[derive(Default)]
pub struct DesktopEvents(Mutex<Pending>, AtomicU64);

impl DesktopEvents {
    /// Returns the next serialized envelope without waiting for any job.
    pub fn next_json(&self) -> Option<String> {
        let event = self.0.try_lock().ok()?.take(Instant::now())?;
        serde_json::to_string(&event).ok()
    }

    /// Queues a host-owned application event through the desktop delivery stream.
    ///
    /// The host owns the event's topic and body. This queue assigns the shared
    /// envelope sequence and applies the same bounded delivery and resync policy
    /// used for engine events. Events with another contract version are ignored.
    pub fn publish_host_event(&self, event: EventEnvelope) {
        if event.contract_version == ENVELOPE_VERSION {
            self.enqueue(event);
        }
    }

    /// Records native event post loss and queues one resynchronization notice.
    pub fn report_post_failure(&self, failed: String, discarded: Vec<String>) {
        let failure_order = next_loss_order(&self.1);
        let observed_at = utc_now_timestamp();
        let mut pending = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.record_delivery_failure(&failed, &discarded, observed_at, failure_order);
    }

    /// Starts a new top-level document generation and discards prior notifications.
    pub fn begin_document_generation(&self) {
        self.with_pending(Pending::begin_document_generation);
    }

    /// Enables notifications after the current top-level document is ready.
    pub fn mark_document_ready(&self) {
        self.with_pending(Pending::mark_document_ready);
    }

    /// Ends the current document generation and discards its pending notifications.
    pub fn close_document_generation(&self) {
        self.with_pending(Pending::close_document_generation);
    }

    fn with_pending(&self, update: impl FnOnce(&mut Pending)) {
        let mut pending = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        update(&mut pending);
    }

    /// Runs startup recovery and queues its committed notice before the UI is ready.
    pub fn recover_history(
        &self,
        storage: &dyn HistoryRecovery,
        now_ms: i64,
    ) -> Result<(), String> {
        du_job_runtime::recover_history(storage, self, now_ms)
    }

    fn enqueue(&self, event: EventEnvelope) {
        let observed_at =
            Timestamp::from_unix_millis(utc_now_ms()).unwrap_or_else(|| event.occurred_at.clone());
        if let Ok(mut pending) = self.0.lock() {
            pending.push_with_loss_order(event, Instant::now(), observed_at, || {
                next_loss_order(&self.1)
            });
        }
    }
}

impl JobEvents for DesktopEvents {
    fn publish(&self, event: JobEvent) {
        let Some(occurred_at) = Timestamp::from_unix_millis(utc_now_ms()) else {
            return;
        };
        let Some(event) = envelope(event, occurred_at) else {
            return;
        };
        self.enqueue(event);
    }

    fn publish_history_changed(&self, change: HistoryChanged) {
        if let Some(event) = history_envelope(change) {
            self.enqueue(event);
        }
    }
}

fn history_envelope(change: HistoryChanged) -> Option<EventEnvelope> {
    let observed_at = Timestamp::from_unix_millis(change.observed_at_ms)?;
    let reason = match change.reason {
        HistoryChangeReason::Appended => "appended",
        HistoryChangeReason::Interrupted => "interrupted",
    };
    Some(EventEnvelope {
        topic: HISTORY_CHANGED.into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at: observed_at.clone(),
        body: serde_json::to_value(HistoryChangedBody {
            revision: change.revision.into(),
            reason: reason.into(),
            count: change.count.to_string(),
            observed_at,
        })
        .ok()?,
    })
}

fn envelope(event: JobEvent, occurred_at: Timestamp) -> Option<EventEnvelope> {
    let (topic, job_ref, correlation_id, result_version, body) = match event {
        JobEvent::ResultVersionChanged(notice) => (
            RESULT_VERSION_CHANGED,
            None,
            None,
            notice.result_version,
            serde_json::to_value(notice.body).ok()?,
        ),
        JobEvent::StateChanged {
            job_id,
            state,
            sequence,
        } => (
            "job.state_changed",
            Some(job_id),
            None,
            None,
            json!({ "state": state, "sequence": sequence }),
        ),
        JobEvent::Finished {
            summary,
            sequence,
            history_recorded,
            warning,
        } => {
            let job_ref = summary.job_id.clone();
            let correlation_id = summary.correlation_id.clone();
            let terminal = jobs::terminal_outcome(*summary, sequence, history_recorded, warning);
            let result_version = terminal.result_version.clone();
            let mut body = serde_json::to_value(terminal).ok()?;
            let fields = body.as_object_mut()?;
            fields.remove("resultVersion");
            // Retain the existing event's explicit nulls for these fields.
            for key in ["reasonKey", "unprocessedSummary", "warning"] {
                fields.entry(key).or_insert(serde_json::Value::Null);
            }
            (
                "job.finished",
                Some(job_ref),
                Some(correlation_id),
                result_version,
                body,
            )
        }
        JobEvent::ItemResult {
            job_id,
            kind_id,
            sequence,
            payload,
        } if kind_id == du_scan_engine::target_probe::TARGET_PROBE_KIND => {
            let observation: TargetStatusChanged = serde_json::from_slice(&payload).ok()?;
            (
                "target.status_changed",
                Some(job_id),
                None,
                None,
                serde_json::to_value(TargetProbeEvent {
                    observation,
                    sequence,
                })
                .ok()?,
            )
        }
        JobEvent::ItemResult { .. } => return None,
    };
    Some(EventEnvelope {
        topic: topic.into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id,
        job_ref,
        result_version,
        occurred_at,
        body,
    })
}
