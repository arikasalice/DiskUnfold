//! Compose the desktop analysis use cases with injected OS and storage ports.
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex, atomic::AtomicU64},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) use crate::recent_scope::recent_scope_admissions;
pub use crate::recent_scope::{
    RecentScopeAdmission, RecentScopeAdmissions, RecentScopeDelivery, RecentScopeDeliveryReceipt,
};
use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Timestamp,
    analysis_ipc::{TargetProbeEvent, TargetStatusChanged},
    history_ipc::{HISTORY_CHANGED, HistoryChangedBody},
    job::{HistoryChangeReason, HistoryChanged, HistoryRecovery, JobHistory},
    ports::Random,
    result_events::RESULT_VERSION_CHANGED,
    scan_observation::ScopeRootPort,
    volume::VolumePort,
};
use du_job_runtime::{FlowLimits, JobEvent, JobEvents, JobRuntime};
use serde_json::json;

mod sample_sink;
pub(crate) mod scan_completion;
mod shutdown;
#[cfg(feature = "shutdown-test-support")]
pub mod shutdown_fixture;
mod system;
mod visualization;
pub use crate::scan::{ScanDiagnosticsFactory, SharedTableLimit, SharedTableLimitSource};
use crate::{api::EngineBuilder, jobs, sample, scan, scope, targets, workers::Workers};
pub use du_scan_engine::scan_job::ScanDiagnostics;
pub use scan_completion::CompletedScanSource;
use system::{SystemClock, SystemIdentity};
pub use visualization::{VisualizationPreview, VisualizationServices};

// Provisional until the target probe deadline is measured across supported devices.
const TARGET_PROBE_DEADLINE: Duration = Duration::from_secs(2);
const TARGET_ENUMERATION_DEADLINE: Duration = Duration::from_secs(2);
mod locale_events;
mod pending;
#[cfg(test)]
use du_core_contracts::analysis_ipc::{JobResyncReason, JobResyncRequired};
#[cfg(test)]
use pending::{EVENT_INTERVAL, EVENT_LIMIT};
use pending::{Pending, next_loss_order};

/// Registers the Windows desktop analysis flow and returns its event source.
pub fn register(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
) -> Result<Arc<DesktopEvents>, Box<dyn std::error::Error>> {
    register_with_shared_table_limit(
        builder,
        roots,
        volumes,
        history,
        random,
        Arc::new(SharedTableLimit::default),
    )
}

/// Registers the desktop flow with a source of validated settings for future scans.
///
/// The source is read at each scan-start attempt; an admitted job retains its value.
pub fn register_with_shared_table_limit(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    shared_table_limit: SharedTableLimitSource,
) -> Result<Arc<DesktopEvents>, Box<dyn std::error::Error>> {
    Ok(register_services(builder, roots, volumes, history, random, shared_table_limit)?.events)
}

/// Registers the desktop flow with an opt-in, per-scan diagnostic factory.
/// This is a Rust composition hook; it does not expose diagnostics over IPC.
pub fn register_with_scan_diagnostics(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    diagnostics: ScanDiagnosticsFactory,
) -> Result<Arc<DesktopEvents>, Box<dyn std::error::Error>> {
    Ok(register_services_inner(
        builder,
        roots,
        volumes,
        history,
        random,
        DesktopRegistrationOptions {
            shared_table_limit: Arc::new(SharedTableLimit::default),
            diagnostics: Some(diagnostics),
            recent_admission_capacity: None,
        },
    )?
    .events)
}

/// Desktop resources used to compose local app-host commands and events.
pub struct DesktopServices {
    pub events: Arc<DesktopEvents>,
    pub path_copy: crate::browse::copy_path::PathCopy,
    /// Internal Rust search entry point sharing the GUI query regions.
    pub search: Option<Arc<crate::search::SearchEntry>>,
    /// Internal composition handle tied to this desktop's scan store and job workers.
    pub visualization: VisualizationServices,
    /// Internal comparison admission tied to the exact completed desktop result.
    pub completed_scans: CompletedScanSource,
    /// Internal app-host worker queue. Only newly admitted GUI scans are sent.
    pub recent_scope_admissions: Option<RecentScopeAdmissions>,
}

struct DesktopRegistrationOptions {
    shared_table_limit: SharedTableLimitSource,
    diagnostics: Option<ScanDiagnosticsFactory>,
    recent_admission_capacity: Option<NonZeroUsize>,
}

/// Registers analysis and exposes the observed-path reader for app-host only.
pub fn register_services(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    shared_table_limit: SharedTableLimitSource,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    register_services_inner(
        builder,
        roots,
        volumes,
        history,
        random,
        DesktopRegistrationOptions {
            shared_table_limit,
            diagnostics: None,
            recent_admission_capacity: None,
        },
    )
}

/// Registers analysis with the opt-in deferred recent-scope worker handoff.
///
/// App-host confirms delivery of each new scan receipt before its storage worker
/// can observe that admission. The scan job may still be running.
pub fn register_services_with_recent_scope_worker(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    shared_table_limit: SharedTableLimitSource,
    recent_admission_capacity: NonZeroUsize,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    register_services_inner(
        builder,
        roots,
        volumes,
        history,
        random,
        DesktopRegistrationOptions {
            shared_table_limit,
            diagnostics: None,
            recent_admission_capacity: Some(recent_admission_capacity),
        },
    )
}

fn register_services_inner(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    options: DesktopRegistrationOptions,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    let DesktopRegistrationOptions {
        shared_table_limit,
        diagnostics,
        recent_admission_capacity,
    } = options;
    let ids = Arc::new(SystemIdentity);
    let clock = Arc::new(SystemClock(Instant::now()));
    let events = Arc::new(DesktopEvents::default());
    let scan_events = Arc::new(scan_completion::ScanEvents::new(events.clone()));
    let sample_events = Arc::new(sample::SampleEvents::new(scan_events.clone()));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        ids.clone(),
        clock.clone(),
        history,
        sample_events.clone(),
        FlowLimits {
            metadata_io: 16,
            queued_items: 64,
            item_bytes: 256 * 1024,
        },
    )?);
    let workers = Arc::new(Workers::start(runtime.clone())?);
    builder.on_shutdown_hooks(shutdown::hooks(runtime.clone(), workers.clone()));
    scope::register_review(builder, roots.clone(), clock.clone());
    targets::register_with_probe(
        builder,
        volumes,
        clock,
        workers.clone(),
        targets::Deadlines {
            enumeration: TARGET_ENUMERATION_DEADLINE,
            probe: TARGET_PROBE_DEADLINE,
        },
    )?;
    let (recent_publisher, recent_scope_admissions) =
        if let Some(capacity) = recent_admission_capacity {
            let (publisher, admissions) = recent_scope_admissions(capacity.get());
            (Some(publisher), Some(admissions))
        } else {
            (None, None)
        };
    let sets = match recent_publisher {
        Some(publisher) => scan::register_with_admissions(
            builder,
            workers.clone(),
            roots,
            ids.clone(),
            shared_table_limit,
            diagnostics,
            publisher,
        )?,
        None => scan::register_with_limit(
            builder,
            workers.clone(),
            roots,
            ids.clone(),
            shared_table_limit,
            diagnostics,
        )?,
    };
    let store = sets.source();
    let completed_scans = scan_events.bind(store.clone())?;
    let sample_ids = ids.clone();
    sample_sink::bind(
        &sample_events,
        builder,
        &workers,
        store.clone(),
        sample_ids,
        &events,
    )?;
    let visualization = VisualizationServices::new(store.clone(), ids.clone(), workers.clone());
    let search = crate::browse::folder::commands::register(builder, sets, ids, workers, random)?;
    crate::scan_failures::register(builder, store.clone(), random);
    let path_copy = crate::browse::copy_path::PathCopy(store.clone());
    jobs::register_with_scan_store(builder, runtime, store);
    Ok(DesktopServices {
        events,
        path_copy,
        search,
        visualization,
        completed_scans,
        recent_scope_admissions,
    })
}

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
