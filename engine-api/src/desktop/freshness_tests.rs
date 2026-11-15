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
