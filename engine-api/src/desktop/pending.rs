use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Timestamp,
    analysis_ipc::{JOB_PROGRESS, JobResyncReason, JobResyncRequired},
};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub(super) const EVENT_LIMIT: usize = 1_024;
pub(super) const EVENT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub(super) struct Pending {
    pub(super) queue: VecDeque<EventEnvelope>,
    pub(super) sequence: u64,
    pub(super) delivery_resync: Option<JobResyncRequired>,
    pub(super) discarded_events: Option<u64>,
    pub(super) overflow_order: Option<u64>,
    pub(super) delivery_unreportable: bool,
    pub(super) unreportable_discarded_events: u64,
    pub(super) document_ready: bool,
    pub(super) document_suspended: bool,
    pub(super) release_at: Option<Instant>,
    pub(super) ready_count: usize,
}

impl Pending {
    /// Queues an event or replaces the queue with a resync notice at capacity.
    #[cfg(test)]
    pub(super) fn push(&mut self, event: EventEnvelope, now: Instant, observed_at: Timestamp) {
        self.push_with_loss_order(event, now, observed_at, || Some(0));
    }

    /// Retains only the newest queued locale profile notification before sharing
    /// the normal bounded desktop event stream.
    pub(super) fn push_latest_locale_changed(
        &mut self,
        event: EventEnvelope,
        now: Instant,
        observed_at: Timestamp,
        next_loss_order: impl FnMut() -> Option<u64>,
    ) {
        if self.discarded_events.is_none() {
            let previous_len = self.queue.len();
            self.queue.retain(|queued| queued.topic != "locale.changed");
            let removed = previous_len.saturating_sub(self.queue.len());
            self.ready_count = self.ready_count.saturating_sub(removed);
        }
        self.push_with_loss_order(event, now, observed_at, next_loss_order);
    }

    pub(super) fn push_with_loss_order(
        &mut self,
        mut event: EventEnvelope,
        now: Instant,
        observed_at: Timestamp,
        mut next_loss_order: impl FnMut() -> Option<u64>,
    ) {
        if let Some(discarded) = &mut self.discarded_events {
            *discarded = discarded.saturating_add(1);
            return;
        }
        if event.topic == JOB_PROGRESS {
            self.drop_superseded_progress(&event);
        }
        if self.queue.len() + 1 >= EVENT_LIMIT {
            self.discarded_events = Some(self.queue.len() as u64 + 1);
            self.overflow_order = next_loss_order();
            self.queue.clear();
            self.ready_count = 0;
            event.topic = "job.resync_required".into();
            event.job_ref = None;
            event.correlation_id = None;
            event.result_version = None;
            event.occurred_at = observed_at;
            event.body = serde_json::Value::Null;
        }
        self.release_at.get_or_insert(now + EVENT_INTERVAL);
        self.queue.push_back(event);
    }

    /// Keeps one unreleased `job.progress` per job in the current batch.
    ///
    /// The newer value is appended behind any event of the same job queued
    /// after the older value, so per-job sequence order is preserved. Events
    /// already released for delivery are never replaced.
    fn drop_superseded_progress(&mut self, event: &EventEnvelope) {
        let Some(job) = event.job_ref.as_ref() else {
            return;
        };
        let released = self.ready_count.min(self.queue.len());
        let previous = self.queue.iter().skip(released).position(|queued| {
            queued.topic == JOB_PROGRESS && queued.job_ref.as_ref() == Some(job)
        });
        if let Some(offset) = previous {
            self.queue.remove(released + offset);
        }
    }

    pub(super) fn take(&mut self, now: Instant) -> Option<EventEnvelope> {
        if self.delivery_unreportable || self.document_suspended {
            return None;
        }
        if let Some(resync) = self.delivery_resync.clone() {
            let event = self.resync_envelope(&resync)?;
            self.delivery_resync = None;
            return Some(event);
        }
        if self.ready_count == 0 {
            self.release_at.filter(|deadline| now >= *deadline)?;
            self.ready_count = self.queue.len();
            self.release_at = None;
        }
        if self.queue.is_empty() {
            return None;
        }
        let sequence = self.next_sequence()?;
        let mut event = self.queue.pop_front()?;
        self.ready_count -= 1;
        if let Some(discarded_events) = self.discarded_events.take() {
            self.overflow_order = None;
            event.body = serde_json::to_value(JobResyncRequired {
                reason: JobResyncReason::SubscriberQueueLimit,
                discarded_events,
                observed_at: event.occurred_at.clone(),
            })
            .ok()?;
        }
        event.seq = sequence;
        Some(event)
    }

    fn resync_envelope(&mut self, resync: &JobResyncRequired) -> Option<EventEnvelope> {
        let body = serde_json::to_value(resync).ok()?;
        let sequence = self.next_sequence()?;
        Some(EventEnvelope {
            topic: "job.resync_required".into(),
            contract_version: ENVELOPE_VERSION,
            seq: sequence,
            correlation_id: None,
            job_ref: None,
            result_version: None,
            occurred_at: resync.observed_at.clone(),
            body,
        })
    }

    fn next_sequence(&mut self) -> Option<u64> {
        let next = self.sequence.checked_add(1)?;
        if next >= (1_u64 << 53) {
            return None;
        }
        self.sequence = next;
        Some(next)
    }

    pub(super) fn begin_document_generation(&mut self) {
        if !self.document_ready {
            return;
        }
        self.document_ready = false;
        self.document_suspended = true;
        self.clear_generation_events();
    }

    pub(super) fn mark_document_ready(&mut self) {
        self.document_ready = true;
        self.document_suspended = false;
    }

    pub(super) fn close_document_generation(&mut self) {
        self.document_ready = false;
        self.document_suspended = true;
        self.clear_generation_events();
    }

    fn clear_generation_events(&mut self) {
        self.queue.clear();
        self.delivery_resync = None;
        self.discarded_events = None;
        self.overflow_order = None;
        self.ready_count = 0;
        self.release_at = None;
        self.delivery_unreportable = false;
        self.unreportable_discarded_events = 0;
    }

    pub(super) fn record_delivery_failure(
        &mut self,
        failed: &str,
        discarded: &[String],
        observed_at: Option<Timestamp>,
        failure_order: Option<u64>,
    ) {
        let failed_envelope = serde_json::from_str::<EventEnvelope>(failed).ok();
        let failed_resync = failed_envelope.as_ref().and_then(parse_resync_event);
        let batch_resync = discarded.iter().find_map(|wire| parse_resync_wire(wire));
        let overflow_count = self.discarded_events.take();
        let overflow_order = self.overflow_order.take();
        let overflow_marker = self
            .queue
            .front()
            .filter(|event| event.topic == "job.resync_required");
        let overflow_time = overflow_marker.map(|event| event.occurred_at.clone());
        let overflow_precedes_delivery = match (overflow_order, failure_order) {
            (Some(overflow), Some(failure)) => overflow < failure,
            (Some(_), None) => true,
            _ => false,
        };

        let mut discarded_count = failed_resync
            .as_ref()
            .map_or(1, |marker| marker.discarded_events);
        for wire in discarded {
            discarded_count = discarded_count.saturating_add(discarded_wire_count(wire));
        }
        if let Some(count) = overflow_count {
            discarded_count = discarded_count.saturating_add(count);
            self.queue.clear();
            self.ready_count = 0;
            self.release_at = None;
        }

        let previous = self.delivery_resync.take();
        let order_is_ambiguous = overflow_count.is_some()
            && overflow_order.is_none()
            && failure_order.is_none()
            && failed_resync.is_none()
            && batch_resync.is_none()
            && previous.is_none();
        if order_is_ambiguous {
            self.delivery_unreportable = true;
            self.unreportable_discarded_events = self
                .unreportable_discarded_events
                .saturating_add(discarded_count);
            self.queue.clear();
            self.ready_count = 0;
            self.release_at = None;
            return;
        }
        let (reason, first_time) = if let Some(marker) = failed_resync {
            (marker.reason, Some(marker.observed_at))
        } else if let Some(marker) = previous.as_ref() {
            (marker.reason, Some(marker.observed_at.clone()))
        } else if let Some(marker) = batch_resync {
            (marker.reason, Some(marker.observed_at))
        } else if overflow_precedes_delivery {
            (JobResyncReason::SubscriberQueueLimit, overflow_time)
        } else {
            (JobResyncReason::SubscriberDeliveryFailed, observed_at)
        };

        if let Some(first_time) = first_time {
            if let Some(previous) = previous {
                discarded_count = discarded_count.saturating_add(previous.discarded_events);
            }
            self.delivery_resync = Some(JobResyncRequired {
                reason,
                discarded_events: discarded_count,
                observed_at: first_time,
            });
        } else {
            self.delivery_unreportable = true;
            self.unreportable_discarded_events = self
                .unreportable_discarded_events
                .saturating_add(discarded_count);
            self.queue.clear();
            self.ready_count = 0;
            self.release_at = None;
        }
    }
}

fn discarded_wire_count(wire: &str) -> u64 {
    let marker = parse_resync_wire(wire);
    if let Some(marker) = marker {
        marker.discarded_events
    } else {
        1
    }
}

fn parse_resync_wire(wire: &str) -> Option<JobResyncRequired> {
    let envelope = serde_json::from_str::<EventEnvelope>(wire).ok()?;
    parse_resync_event(&envelope)
}

fn parse_resync_event(event: &EventEnvelope) -> Option<JobResyncRequired> {
    (event.topic == "job.resync_required")
        .then(|| serde_json::from_value::<JobResyncRequired>(event.body.clone()).ok())
        .flatten()
}

pub(super) fn next_loss_order(sequence: &AtomicU64) -> Option<u64> {
    sequence
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            value.checked_add(1)
        })
        .ok()
        .and_then(|previous| previous.checked_add(1))
}

#[cfg(test)]
mod progress_tests;
