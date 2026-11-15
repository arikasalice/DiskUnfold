//! The relay of `results.freshness_changed` through the bounded desktop event stream.
use super::*;
use du_core_contracts::{
    Id,
    result_freshness::{
        FreshnessChanged, FreshnessCounts, FreshnessRange, FreshnessState, FreshnessSummary,
        RESULT_FRESHNESS_CHANGED, ResultFreshnessChanged, ResultFreshnessNotice,
    },
};
use std::time::Instant;

type Check = Result<(), Box<dyn std::error::Error>>;

fn id(n: u32) -> Result<Id, Box<dyn std::error::Error>> {
    Ok(Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))?)
}

fn notice(
    scope: u32,
    sequence: u64,
    current: bool,
) -> Result<ResultFreshnessNotice, Box<dyn std::error::Error>> {
    let rows = u64::from(current);
    Ok(ResultFreshnessNotice {
        result_version: current.then(|| id(900 + scope)).transpose()?,
        body: ResultFreshnessChanged {
            lineage_id: id(1)?,
            scope_version_id: id(scope)?,
            sequence,
            summary: FreshnessSummary {
                representative: FreshnessState::new(FreshnessState::UNWATCHED),
                counts: FreshnessCounts {
                    unwatched: rows,
                    ..FreshnessCounts::default()
                },
            },
            changed: if current {
                FreshnessChanged::Ranges {
                    ranges: vec![FreshnessRange {
                        root_index: 0,
                        state: FreshnessState::new(FreshnessState::UNWATCHED),
                        reason_key: None,
                    }],
                }
            } else {
                FreshnessChanged::Requery
            },
            observed_at: None,
        },
    })
}

fn event(
    scope: u32,
    sequence: u64,
    current: bool,
) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    let at = Timestamp::from_unix_millis(10).ok_or("time")?;
    envelope(
        JobEvent::ResultFreshnessChanged(Box::new(notice(scope, sequence, current)?)),
        at,
    )
    .ok_or_else(|| "missing freshness event".into())
}

#[test]
fn freshness_events_keep_their_body_sequence_and_current_version_on_the_wire() -> Check {
    let now = Instant::now();
    let mut pending = Pending::default();
    for (sequence, current) in [(7, true), (8, false)] {
        let event = event(5, sequence, current)?;
        let observed_at = event.occurred_at.clone();
        pending.push(event, now, observed_at);
    }
    let first = pending.take(now + EVENT_INTERVAL).ok_or("missing first")?;
    assert_eq!(first.topic, RESULT_FRESHNESS_CHANGED);
    assert_eq!(first.seq, 1);
    assert_eq!(first.result_version, Some(id(905)?));
    assert!(first.job_ref.is_none());
    assert_eq!(first.body["sequence"], 7);
    assert_eq!(first.body["changed"]["kind"], "ranges");
    let second = pending.take(now + EVENT_INTERVAL).ok_or("missing second")?;
    assert_eq!(second.seq, 2);
    let wire = serde_json::to_value(second)?;
    assert!(wire.get("resultVersion").is_none());
    assert_eq!(
        wire["body"]["sequence"], 8,
        "the body sequence is not the envelope seq"
    );
    assert_eq!(wire["body"]["changed"]["kind"], "requery");
    assert_eq!(wire["body"]["summary"]["counts"]["unwatched"], 0);
    Ok(())
}

fn take_all(pending: &mut Pending, now: Instant) -> Vec<EventEnvelope> {
    let mut out = Vec::new();
    while let Some(event) = pending.take(now) {
        out.push(event);
    }
    out
}

fn push(pending: &mut Pending, event: EventEnvelope, now: Instant) {
    let observed_at = event.occurred_at.clone();
    pending.push(event, now, observed_at);
}

fn freshness_of(events: &[EventEnvelope]) -> Vec<&EventEnvelope> {
    events
        .iter()
        .filter(|event| event.topic == RESULT_FRESHNESS_CHANGED)
        .collect()
}

fn assert_requery_copy(
    copy: &EventEnvelope,
    original: &EventEnvelope,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(copy.topic, RESULT_FRESHNESS_CHANGED);
    assert_eq!(copy.result_version, original.result_version);
    assert_eq!(copy.body["sequence"], original.body["sequence"]);
    assert_eq!(copy.body["summary"], original.body["summary"]);
    assert_eq!(copy.body["scopeVersionId"], original.body["scopeVersionId"]);
    assert_eq!(copy.body["changed"], serde_json::json!({"kind": "requery"}));
    let _: ResultFreshnessChanged = serde_json::from_value(copy.body.clone())?;
    Ok(())
}

#[test]
fn a_window_that_opens_late_receives_the_last_event_as_a_requery_with_the_same_sequence() -> Check {
    let now = Instant::now();
    let later = now + EVENT_INTERVAL;
    let mut pending = Pending::default();
    pending.mark_document_ready();
    let original = event(5, 7, true)?;
    push(&mut pending, original.clone(), now);
    push(&mut pending, event(6, 2, true)?, now);
    let first_window = take_all(&mut pending, later);
    assert_eq!(first_window.len(), 2);
    assert_eq!(first_window[0].body["changed"]["kind"], "ranges");
    // The window reloads. Its subscription starts after the event was delivered once.
    pending.begin_document_generation();
    assert!(pending.take(later).is_none());
    pending.mark_document_ready();
    let second_window = take_all(&mut pending, later + EVENT_INTERVAL);
    let copies = freshness_of(&second_window);
    assert_eq!(copies.len(), 2, "one copy per scope version");
    assert_requery_copy(copies[0], &original)?;
    assert_eq!(
        copies[0].body["sequence"], 7,
        "a replay never changes the sequence"
    );
    assert_eq!(copies[1].body["sequence"], 2);
    assert!(copies.iter().all(|copy| copy.seq > first_window[1].seq));
    Ok(())
}

#[test]
fn only_the_newest_event_of_a_scope_version_is_kept() -> Check {
    let now = Instant::now();
    let mut pending = Pending::default();
    for sequence in [7, 9, 8] {
        push(&mut pending, event(5, sequence, true)?, now);
    }
    let _ = take_all(&mut pending, now + EVENT_INTERVAL);
    pending.begin_document_generation();
    pending.mark_document_ready();
    let replay = take_all(&mut pending, now + EVENT_INTERVAL * 2);
    let copies = freshness_of(&replay);
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].body["sequence"], 9);
    Ok(())
}

#[test]
fn a_window_receives_the_retained_events_again_after_job_resync_required() -> Check {
    let now = Instant::now();
    let mut pending = Pending::default();
    pending.mark_document_ready();
    let original = event(5, 7, true)?;
    push(&mut pending, original.clone(), now);
    let delivered = take_all(&mut pending, now + EVENT_INTERVAL);
    assert_eq!(delivered.len(), 1);
    // The queue overflows; the newest freshness event is among the discarded ones.
    let newest = event(5, 8, true)?;
    for number in 0..EVENT_LIMIT {
        let filler = if number == EVENT_LIMIT - 2 {
            newest.clone()
        } else {
            event(6, 1, true)?
        };
        push(&mut pending, filler, now);
    }
    let after = take_all(&mut pending, now + EVENT_INTERVAL * 2);
    assert_eq!(
        after.len(),
        1,
        "the overflowed queue holds only the resync notice"
    );
    assert_eq!(after[0].topic, "job.resync_required");
    let replay = take_all(&mut pending, now + EVENT_INTERVAL * 4);
    let copies = freshness_of(&replay);
    assert_eq!(copies.len(), 2);
    assert_eq!(
        replay[0].topic, RESULT_FRESHNESS_CHANGED,
        "copies follow the resync notice"
    );
    assert_requery_copy(copies[0], &newest)?;
    assert_eq!(
        copies[0].body["sequence"], 8,
        "the dropped event is not lost"
    );
    Ok(())
}

#[test]
fn a_delivery_failure_resync_is_followed_by_the_retained_events() -> Check {
    let now = Instant::now();
    let mut pending = Pending::default();
    pending.mark_document_ready();
    let original = event(5, 7, true)?;
    push(&mut pending, original.clone(), now);
    let _ = take_all(&mut pending, now + EVENT_INTERVAL);
    let at = Timestamp::from_unix_millis(50).ok_or("time")?;
    pending.record_delivery_failure("not an envelope", &[], Some(at), Some(1));
    let after = take_all(&mut pending, now + EVENT_INTERVAL * 2);
    assert_eq!(after[0].topic, "job.resync_required");
    let replay = take_all(&mut pending, now + EVENT_INTERVAL * 4);
    assert_eq!(replay.len(), 1);
    assert_requery_copy(&replay[0], &original)?;
    // A copy is not recorded again with a changed meaning: the next reload sends the same one.
    pending.begin_document_generation();
    pending.mark_document_ready();
    let again = take_all(&mut pending, now + EVENT_INTERVAL * 6);
    assert_requery_copy(&again[0], &original)?;
    Ok(())
}

#[test]
fn an_event_without_a_current_version_drops_the_retained_copy() -> Check {
    let now = Instant::now();
    let mut pending = Pending::default();
    pending.mark_document_ready();
    push(&mut pending, event(5, 7, true)?, now);
    push(&mut pending, event(6, 3, true)?, now);
    push(&mut pending, event(5, 8, false)?, now);
    let delivered = take_all(&mut pending, now + EVENT_INTERVAL);
    assert_eq!(
        delivered.len(),
        3,
        "the removal event itself is still relayed"
    );
    pending.begin_document_generation();
    pending.mark_document_ready();
    let replay = take_all(&mut pending, now + EVENT_INTERVAL * 2);
    let copies = freshness_of(&replay);
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].body["scopeVersionId"], id(6)?.as_str());
    assert!(pending.freshness.replays().len() == 1);
    Ok(())
}
