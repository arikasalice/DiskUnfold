use super::*;
use std::sync::atomic::Ordering;

fn event_wire(
    topic: &str,
    sequence: u64,
    occurred_at: Timestamp,
    body: serde_json::Value,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&EventEnvelope {
        topic: topic.into(),
        contract_version: ENVELOPE_VERSION,
        seq: sequence,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at,
        body,
    })
}

fn ordinary_wire(sequence: u64, occurred_at: Timestamp) -> Result<String, serde_json::Error> {
    event_wire(
        "job.progress",
        sequence,
        occurred_at,
        json!({"sequence": sequence}),
    )
}

fn resync_wire(
    sequence: u64,
    reason: JobResyncReason,
    discarded_events: u64,
    observed_at: Timestamp,
) -> Result<String, serde_json::Error> {
    let body = serde_json::to_value(JobResyncRequired {
        reason,
        discarded_events,
        observed_at: observed_at.clone(),
    })?;
    event_wire("job.resync_required", sequence, observed_at.clone(), body)
}

fn read_marker(
    event: EventEnvelope,
) -> Result<(JobResyncRequired, u64), Box<dyn std::error::Error>> {
    assert_eq!(event.topic, "job.resync_required");
    let marker = serde_json::from_value(event.body)?;
    Ok((marker, event.seq))
}

#[test]
fn delivery_failure_marker_precedes_preserved_events_and_counts_the_failed_batch_once()
-> Result<(), Box<dyn std::error::Error>> {
    let failed_at = Timestamp::from_unix_millis(2_000).ok_or("invalid failed time")?;
    let before = Timestamp::from_unix_millis(1_000).ok_or("invalid event time")?;
    let mut pending = Pending {
        sequence: 40,
        ..Pending::default()
    };
    let failed = ordinary_wire(41, before.clone())?;
    let discarded = vec![
        ordinary_wire(42, before.clone())?,
        ordinary_wire(43, before.clone())?,
    ];
    pending.push(
        EventEnvelope {
            topic: "job.progress".into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id: None,
            job_ref: None,
            result_version: None,
            occurred_at: before,
            body: json!({"sequence": 44}),
        },
        Instant::now(),
        failed_at.clone(),
    );
    pending.record_delivery_failure(&failed, &discarded, Some(failed_at.clone()), Some(10));

    let (marker, marker_seq) = read_marker(pending.take(Instant::now()).ok_or("missing marker")?)?;
    assert_eq!(marker.reason, JobResyncReason::SubscriberDeliveryFailed);
    assert_eq!(marker.discarded_events, 3);
    assert_eq!(marker.observed_at, failed_at);
    let next = pending
        .take(Instant::now() + EVENT_INTERVAL)
        .ok_or("missing preserved event")?;
    assert!(next.seq > marker_seq);
    assert_eq!(next.body["sequence"], 44);
    Ok(())
}

#[test]
fn failed_resync_retry_keeps_its_first_reason_and_time_without_counting_the_marker()
-> Result<(), Box<dyn std::error::Error>> {
    let first_time = Timestamp::from_unix_millis(1_000).ok_or("invalid first time")?;
    let current_time = Timestamp::from_unix_millis(3_000).ok_or("invalid current time")?;
    let failed_marker = resync_wire(
        15,
        JobResyncReason::SubscriberQueueLimit,
        7,
        first_time.clone(),
    )?;
    let discarded = vec![
        ordinary_wire(16, current_time.clone())?,
        resync_wire(
            17,
            JobResyncReason::SubscriberDeliveryFailed,
            2,
            Timestamp::from_unix_millis(2_000).ok_or("invalid nested time")?,
        )?,
    ];
    let mut pending = Pending {
        sequence: 17,
        ..Pending::default()
    };
    pending.record_delivery_failure(
        &failed_marker,
        &discarded,
        Some(current_time.clone()),
        Some(12),
    );
    let (marker, marker_seq) = read_marker(pending.take(Instant::now()).ok_or("missing retry")?)?;
    assert_eq!(marker.reason, JobResyncReason::SubscriberQueueLimit);
    assert_eq!(marker.discarded_events, 10);
    assert_eq!(marker.observed_at, first_time);

    let retry = resync_wire(
        marker_seq,
        marker.reason,
        marker.discarded_events,
        marker.observed_at.clone(),
    )?;
    pending.record_delivery_failure(&retry, &[], Some(current_time), Some(13));
    let (retried, seq) = read_marker(pending.take(Instant::now()).ok_or("missing second retry")?)?;
    assert!(seq > marker_seq);
    assert_eq!(retried, marker);
    Ok(())
}

#[test]
fn an_existing_marker_in_the_failed_batch_contributes_its_loss_count_only_once()
-> Result<(), Box<dyn std::error::Error>> {
    let first_time = Timestamp::from_unix_millis(1_000).ok_or("invalid first time")?;
    let failed_time = Timestamp::from_unix_millis(2_000).ok_or("invalid failed time")?;
    let failed = ordinary_wire(30, failed_time.clone())?;
    let discarded = vec![
        ordinary_wire(31, failed_time.clone())?,
        resync_wire(
            32,
            JobResyncReason::SubscriberQueueLimit,
            5,
            first_time.clone(),
        )?,
        ordinary_wire(33, failed_time.clone())?,
    ];
    let mut pending = Pending {
        sequence: 33,
        ..Pending::default()
    };
    pending.record_delivery_failure(&failed, &discarded, Some(failed_time), Some(10));
    let (marker, _) = read_marker(pending.take(Instant::now()).ok_or("missing marker")?)?;
    assert_eq!(marker.reason, JobResyncReason::SubscriberQueueLimit);
    assert_eq!(marker.discarded_events, 8);
    assert_eq!(marker.observed_at, first_time);
    Ok(())
}

#[test]
fn an_overflow_observed_before_the_native_failure_owns_the_episode()
-> Result<(), Box<dyn std::error::Error>> {
    let overflow_time = Timestamp::from_unix_millis(1_000).ok_or("invalid overflow time")?;
    let failed_time = Timestamp::from_unix_millis(2_000).ok_or("invalid failure time")?;
    let event = EventEnvelope {
        topic: "job.progress".into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at: overflow_time.clone(),
        body: json!({"sequence": 1}),
    };
    let mut pending = Pending::default();
    let now = Instant::now();
    for _ in 0..EVENT_LIMIT - 1 {
        pending.push(event.clone(), now, overflow_time.clone());
    }
    pending.push_with_loss_order(event.clone(), now, overflow_time.clone(), || Some(5));
    pending.push(event.clone(), now, overflow_time.clone());
    pending.push(event, now, overflow_time.clone());
    assert_eq!(pending.discarded_events, Some(EVENT_LIMIT as u64 + 2));

    pending.record_delivery_failure(
        &ordinary_wire(200, failed_time.clone())?,
        &[ordinary_wire(201, failed_time.clone())?],
        Some(failed_time),
        Some(6),
    );
    let (marker, _) = read_marker(pending.take(now).ok_or("missing overlap marker")?)?;
    assert_eq!(marker.reason, JobResyncReason::SubscriberQueueLimit);
    assert_eq!(marker.discarded_events, EVENT_LIMIT as u64 + 4);
    assert_eq!(marker.observed_at, overflow_time);
    assert!(pending.queue.is_empty());
    Ok(())
}

#[test]
fn a_concurrent_overflow_after_the_post_failure_does_not_replace_its_first_cause()
-> Result<(), Box<dyn std::error::Error>> {
    let failed_time = Timestamp::from_unix_millis(2_000).ok_or("invalid failure time")?;
    let later_overflow = Timestamp::from_unix_millis(1_000).ok_or("invalid overflow time")?;
    let event = EventEnvelope {
        topic: "job.progress".into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at: later_overflow.clone(),
        body: json!({"sequence": 1}),
    };
    let mut pending = Pending::default();
    let now = Instant::now();
    for _ in 0..EVENT_LIMIT - 1 {
        pending.push(event.clone(), now, later_overflow.clone());
    }
    pending.push_with_loss_order(event.clone(), now, later_overflow.clone(), || Some(12));
    pending.push(event, now, later_overflow.clone());

    pending.record_delivery_failure(
        &ordinary_wire(200, failed_time.clone())?,
        &[],
        Some(failed_time.clone()),
        Some(11),
    );
    let (marker, _) = read_marker(pending.take(now).ok_or("missing marker")?)?;
    assert_eq!(marker.reason, JobResyncReason::SubscriberDeliveryFailed);
    assert_eq!(marker.discarded_events, EVENT_LIMIT as u64 + 2);
    assert_eq!(marker.observed_at, failed_time);
    Ok(())
}

#[test]
fn a_failed_native_post_without_utc_time_fails_closed_instead_of_fabricating_a_timestamp()
-> Result<(), Box<dyn std::error::Error>> {
    let event_time = Timestamp::from_unix_millis(1_000).ok_or("invalid event time")?;
    let mut pending = Pending::default();
    pending.record_delivery_failure(&ordinary_wire(1, event_time.clone())?, &[], None, Some(1));
    assert!(pending.take(Instant::now()).is_none());
    assert!(pending.delivery_unreportable);
    assert_eq!(pending.unreportable_discarded_events, 1);
    Ok(())
}

#[test]
fn document_replacement_discards_prior_markers_and_suspends_delivery_until_ready()
-> Result<(), Box<dyn std::error::Error>> {
    let first_time = Timestamp::from_unix_millis(1_000).ok_or("invalid first time")?;
    let mut pending = Pending::default();
    let now = Instant::now();
    let initial = EventEnvelope {
        topic: "history.changed".into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at: first_time.clone(),
        body: json!({"revision": 1}),
    };
    pending.push(initial, now, first_time.clone());
    pending.delivery_resync = Some(JobResyncRequired {
        reason: JobResyncReason::SubscriberDeliveryFailed,
        discarded_events: 3,
        observed_at: first_time.clone(),
    });

    pending.begin_document_generation();
    pending.begin_document_generation();
    assert_eq!(
        pending
            .delivery_resync
            .as_ref()
            .map(|notice| notice.discarded_events),
        Some(3)
    );
    pending.mark_document_ready();
    let startup_notice = pending
        .take(now)
        .ok_or("overlapping initial navigation lost startup recovery")?;
    let (startup_marker, _) = read_marker(startup_notice)?;
    assert_eq!(startup_marker.discarded_events, 3);
    let initial = pending
        .take(now + EVENT_INTERVAL)
        .ok_or("initial notice dropped")?;
    let prior_sequence = initial.seq;

    pending.delivery_resync = Some(JobResyncRequired {
        reason: JobResyncReason::SubscriberDeliveryFailed,
        discarded_events: 2,
        observed_at: first_time.clone(),
    });
    pending.push(
        EventEnvelope {
            topic: "job.finished".into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id: None,
            job_ref: None,
            result_version: None,
            occurred_at: first_time.clone(),
            body: json!({"sequence": 2}),
        },
        now,
        first_time.clone(),
    );
    pending.begin_document_generation();
    assert!(pending.take(now + EVENT_INTERVAL).is_none());
    assert!(pending.delivery_resync.is_none());
    assert!(pending.queue.is_empty());
    pending.push(
        EventEnvelope {
            topic: "job.progress".into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id: None,
            job_ref: None,
            result_version: None,
            occurred_at: first_time.clone(),
            body: json!({"sequence": 3}),
        },
        now,
        first_time.clone(),
    );
    assert!(pending.take(now + EVENT_INTERVAL).is_none());
    pending.mark_document_ready();
    pending.push(
        EventEnvelope {
            topic: "job.progress".into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id: None,
            job_ref: None,
            result_version: None,
            occurred_at: first_time.clone(),
            body: json!({"sequence": 4}),
        },
        now,
        first_time.clone(),
    );
    let current = pending
        .take(now + EVENT_INTERVAL)
        .ok_or("event after document readiness unavailable")?;
    assert!(current.seq > prior_sequence);
    assert_eq!(current.topic, "job.progress");
    assert_eq!(current.body["sequence"], 3);
    let following = pending
        .take(now + EVENT_INTERVAL)
        .ok_or("event queued during navigation was lost")?;
    assert!(following.seq > current.seq);
    assert_eq!(following.body["sequence"], 4);
    pending.close_document_generation();
    assert!(pending.take(now + EVENT_INTERVAL).is_none());
    Ok(())
}

#[test]
fn window_sequence_exhaustion_never_wraps_or_resets_for_a_new_document()
-> Result<(), Box<dyn std::error::Error>> {
    let observed_at = Timestamp::from_unix_millis(1_000).ok_or("invalid event time")?;
    let mut pending = Pending {
        sequence: (1_u64 << 53) - 1,
        ..Pending::default()
    };
    let event = EventEnvelope {
        topic: "job.finished".into(),
        contract_version: ENVELOPE_VERSION,
        seq: 99,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at: observed_at.clone(),
        body: json!({"sequence": 99}),
    };
    let now = Instant::now();
    pending.push(event, now, observed_at);

    assert!(pending.take(now + EVENT_INTERVAL).is_none());
    assert_eq!(pending.sequence, (1_u64 << 53) - 1);
    assert_eq!(pending.queue.len(), 1);

    pending.begin_document_generation();
    pending.mark_document_ready();
    assert!(pending.take(now + EVENT_INTERVAL).is_none());
    assert_eq!(pending.sequence, (1_u64 << 53) - 1);
    Ok(())
}

#[test]
fn exhausted_loss_order_does_not_invent_the_first_cause_of_an_overlapping_episode()
-> Result<(), Box<dyn std::error::Error>> {
    let overflow_time = Timestamp::from_unix_millis(1_000).ok_or("invalid overflow time")?;
    let failure_time = Timestamp::from_unix_millis(2_000).ok_or("invalid failure time")?;
    let event = EventEnvelope {
        topic: "job.progress".into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at: overflow_time.clone(),
        body: json!({"sequence": 1}),
    };
    let mut pending = Pending::default();
    let now = Instant::now();
    for _ in 0..EVENT_LIMIT - 1 {
        pending.push(event.clone(), now, overflow_time.clone());
    }
    pending.push_with_loss_order(event, now, overflow_time, || None);
    pending.record_delivery_failure(
        &ordinary_wire(2, failure_time.clone())?,
        &[],
        Some(failure_time),
        None,
    );

    assert!(pending.delivery_unreportable);
    assert_eq!(
        pending.unreportable_discarded_events,
        EVENT_LIMIT as u64 + 1
    );
    assert!(pending.take(now + EVENT_INTERVAL).is_none());

    let loss_order = AtomicU64::new(u64::MAX);
    assert_eq!(next_loss_order(&loss_order), None);
    assert_eq!(loss_order.load(Ordering::SeqCst), u64::MAX);
    Ok(())
}
