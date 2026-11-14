//! F24 IF-06 and IF-14: `job.progress` coalescing in the desktop batch.
use super::*;
use du_core_contracts::Id;
use serde_json::json;

const JOB_A: &str = "00000000-0000-7000-8000-00000000000a";
const JOB_B: &str = "00000000-0000-7000-8000-00000000000b";
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn event(topic: &str, job: &str, sequence: u64) -> Result<EventEnvelope, String> {
    Ok(EventEnvelope {
        topic: topic.into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: Some(Id::parse(job)?),
        result_version: None,
        occurred_at: observed()?,
        body: json!({ "sequence": sequence }),
    })
}

fn observed() -> Result<Timestamp, String> {
    Timestamp::from_unix_millis(0).ok_or_else(|| "time".to_owned())
}

fn drain(pending: &mut Pending, now: Instant) -> Vec<(String, String, u64)> {
    std::iter::from_fn(|| pending.take(now))
        .map(|event| {
            (
                event.topic,
                event
                    .job_ref
                    .map(|id| id.as_str().to_owned())
                    .unwrap_or_default(),
                event.body["sequence"].as_u64().unwrap_or_default(),
            )
        })
        .collect()
}

fn row(topic: &str, job: &str, sequence: u64) -> (String, String, u64) {
    (topic.to_owned(), job.to_owned(), sequence)
}

#[test]
fn a_batch_keeps_the_latest_progress_per_job_after_its_other_events() -> TestResult {
    let mut pending = Pending::default();
    let now = Instant::now();
    pending.push(event(JOB_PROGRESS, JOB_A, 3)?, now, observed()?);
    pending.push(event("job.state_changed", JOB_A, 4)?, now, observed()?);
    pending.push(event(JOB_PROGRESS, JOB_B, 3)?, now, observed()?);
    pending.push(event(JOB_PROGRESS, JOB_A, 5)?, now, observed()?);
    pending.push(event(JOB_PROGRESS, JOB_A, 6)?, now, observed()?);
    assert!(
        pending.take(now).is_none(),
        "the batch interval has not elapsed"
    );
    assert_eq!(
        drain(&mut pending, now + EVENT_INTERVAL),
        vec![
            row("job.state_changed", JOB_A, 4),
            row(JOB_PROGRESS, JOB_B, 3),
            row(JOB_PROGRESS, JOB_A, 6),
        ]
    );
    Ok(())
}

#[test]
fn released_progress_is_not_replaced_by_the_next_batch() -> TestResult {
    let mut pending = Pending::default();
    let now = Instant::now();
    pending.push(event("job.state_changed", JOB_B, 2)?, now, observed()?);
    pending.push(event(JOB_PROGRESS, JOB_A, 3)?, now, observed()?);
    let released = now + EVENT_INTERVAL;
    // The first batch is released; its progress value is still queued.
    assert!(pending.take(released).is_some());
    pending.push(event(JOB_PROGRESS, JOB_A, 4)?, released, observed()?);
    assert_eq!(
        drain(&mut pending, released + EVENT_INTERVAL),
        vec![row(JOB_PROGRESS, JOB_A, 3), row(JOB_PROGRESS, JOB_A, 4)]
    );
    Ok(())
}

#[test]
fn coalesced_progress_stays_below_the_limit_until_one_more_event_arrives() -> TestResult {
    let mut pending = Pending::default();
    let now = Instant::now();
    for sequence in 1..EVENT_LIMIT as u64 - 1 {
        pending.push(
            event("job.state_changed", JOB_B, sequence)?,
            now,
            observed()?,
        );
    }
    for sequence in 1..=50 {
        pending.push(event(JOB_PROGRESS, JOB_A, sequence)?, now, observed()?);
    }
    assert_eq!(pending.queue.len(), EVENT_LIMIT - 1);
    assert_eq!(pending.discarded_events, None);
    assert_eq!(
        pending
            .queue
            .back()
            .map(|event| event.body["sequence"].clone()),
        Some(json!(50))
    );
    pending.push(event("job.state_changed", JOB_B, 2_000)?, now, observed()?);
    assert_eq!(pending.discarded_events, Some(EVENT_LIMIT as u64));
    let resync = pending
        .take(now + EVENT_INTERVAL)
        .ok_or("missing resync notice")?;
    assert_eq!(resync.topic, "job.resync_required");
    assert_eq!(resync.body["discardedEvents"], json!(EVENT_LIMIT));
    assert_eq!(resync.body["reason"], json!("subscriber_queue_limit"));
    assert!(pending.take(now + EVENT_INTERVAL).is_none());
    Ok(())
}
