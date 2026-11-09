//! Compose the desktop analysis use cases with injected OS and storage ports.
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant, UNIX_EPOCH},
};

use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Id, Timestamp,
    analysis_ipc::{TargetProbeEvent, TargetStatusChanged},
    job::JobHistory,
    scan_observation::ScopeRootPort,
    volume::VolumePort,
};
use du_job_runtime::{FlowLimits, JobClock, JobEvent, JobEvents, JobIdentity, JobRuntime};
use serde_json::json;

use crate::{api::EngineBuilder, jobs, scan, scope, targets, workers::Workers};

const EVENT_LIMIT: usize = 1_024;
const EVENT_INTERVAL: Duration = Duration::from_millis(100);

/// Registers the Windows desktop analysis flow and returns its event source.
pub fn register(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
) -> Result<Arc<DesktopEvents>, Box<dyn std::error::Error>> {
    let ids = Arc::new(SystemIdentity);
    let clock = Arc::new(SystemClock);
    let events = Arc::new(DesktopEvents::default());
    let runtime = Arc::new(JobRuntime::new_with_limits(
        ids.clone(),
        clock.clone(),
        history,
        events.clone(),
        FlowLimits {
            metadata_io: 16,
            queued_items: 64,
            item_bytes: 256 * 1024,
        },
    )?);
    jobs::register(builder, runtime.clone());
    scope::register_review(builder, roots.clone(), clock.clone());
    targets::register(builder, volumes, clock);
    let workers = Arc::new(Workers::start(runtime)?);
    scan::register(builder, workers, roots, ids)?;
    Ok(events)
}

struct SystemIdentity;

impl JobIdentity for SystemIdentity {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes())
    }
}

struct SystemClock;

impl JobClock for SystemClock {
    fn now_ms(&self) -> i64 {
        UNIX_EPOCH
            .elapsed()
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
            .unwrap_or(0)
    }
}

#[derive(Default)]
struct Pending {
    queue: VecDeque<EventEnvelope>,
    sequence: u64,
    overflow: bool,
    release_at: Option<Instant>,
    ready_count: usize,
}

impl Pending {
    fn push(&mut self, mut event: EventEnvelope, now: Instant) {
        if self.overflow {
            return;
        }
        self.sequence += 1;
        event.seq = self.sequence;
        if self.queue.len() == EVENT_LIMIT {
            self.queue.clear();
            self.ready_count = 0;
            self.overflow = true;
            event.topic = "job.resync_required".into();
            event.job_ref = None;
            event.correlation_id = None;
            event.result_version = None;
            event.body = json!({});
        }
        self.release_at.get_or_insert(now + EVENT_INTERVAL);
        self.queue.push_back(event);
    }

    fn take(&mut self, now: Instant) -> Option<EventEnvelope> {
        if self.ready_count == 0 {
            self.release_at.filter(|deadline| now >= *deadline)?;
            self.ready_count = self.queue.len();
            self.release_at = None;
        }
        let event = self.queue.pop_front()?;
        self.ready_count -= 1;
        self.overflow = false;
        Some(event)
    }
}

#[cfg(test)]
mod tests;

/// A bounded, nonblocking event source for the desktop window.
#[derive(Default)]
pub struct DesktopEvents(Mutex<Pending>);

impl DesktopEvents {
    /// Returns the next serialized envelope without waiting for any job.
    pub fn next_json(&self) -> Option<String> {
        let event = self.0.try_lock().ok()?.take(Instant::now())?;
        serde_json::to_string(&event).ok()
    }
}

impl JobEvents for DesktopEvents {
    fn publish(&self, event: JobEvent) {
        let Some(occurred_at) = Timestamp::from_unix_millis(SystemClock.now_ms()) else {
            return;
        };
        let Some(event) = envelope(event, occurred_at) else {
            return;
        };
        let Ok(mut pending) = self.0.lock() else {
            return;
        };
        pending.push(event, Instant::now());
    }
}

fn envelope(event: JobEvent, occurred_at: Timestamp) -> Option<EventEnvelope> {
    let (topic, job_ref, correlation_id, result_version, body) = match event {
        JobEvent::StateChanged {
            job_id,
            state,
            sequence,
        } => (
            "job.state_changed",
            job_id,
            None,
            None,
            json!({ "state": state, "sequence": sequence }),
        ),
        JobEvent::Finished {
            summary,
            sequence,
            history_recorded,
            warning,
        } => (
            "job.finished",
            summary.job_id,
            Some(summary.correlation_id),
            summary.result_version_id,
            json!({
                "state": summary.state,
                "sequence": sequence,
                "completed": summary.counts.completed,
                "overwritten": summary.counts.overwritten,
                "failed": summary.counts.failed,
                "skipped": summary.counts.skipped,
                "unprocessed": summary.counts.unprocessed,
                "reasonKey": summary.failure_reason_key,
                "unprocessedSummary": summary.unprocessed_summary,
                "historyRecorded": history_recorded,
                "warning": warning,
            }),
        ),
        JobEvent::ItemResult {
            job_id,
            sequence,
            payload,
        } => {
            let observation: TargetStatusChanged = serde_json::from_slice(&payload).ok()?;
            (
                "target.status_changed",
                job_id,
                None,
                None,
                serde_json::to_value(TargetProbeEvent {
                    observation,
                    sequence,
                })
                .ok()?,
            )
        }
    };
    Some(EventEnvelope {
        topic: topic.into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id,
        job_ref: Some(job_ref),
        result_version,
        occurred_at,
        body,
    })
}
