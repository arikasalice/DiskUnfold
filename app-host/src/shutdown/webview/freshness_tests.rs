//! F33 FR3.HD1.app-host.DD3: the webview lifecycle receives retained freshness events.
use super::DesktopEventCallbacks;
use du_core_contracts::{ENVELOPE_VERSION, EventEnvelope, Id, Timestamp};
use du_engine_api::desktop::DesktopEvents;
use du_platform_os::webview::EventPostFailure;
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

type Check = Result<(), Box<dyn std::error::Error>>;

const SCOPE: &str = "018f0000-0000-7000-8000-000000000005";

fn freshness_event(sequence: u64) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    Ok(EventEnvelope {
        topic: "results.freshness_changed".into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: None,
        result_version: Some(Id::parse("018f0000-0000-7000-8000-000000000009")?),
        occurred_at: Timestamp::from_unix_millis(1_000).ok_or("time")?,
        body: json!({
            "lineageId": "018f0000-0000-7000-8000-000000000001",
            "scopeVersionId": SCOPE,
            "sequence": sequence,
            "summary": {
                "representative": "unwatched",
                "counts": {"observed": 0, "unwatched": 2, "changeNotified": 0,
                           "gapUnknown": 0, "refreshing": 0, "unverifiable": 0}
            },
            "changed": {"kind": "ranges", "ranges": [
                {"rootIndex": 1, "state": "unwatched"}
            ]}
        }),
    })
}

/// Waits for the next delivered event, because delivery is batched by a short interval.
fn next(callbacks: &DesktopEventCallbacks) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(wire) = (callbacks.next)() {
            return Ok(serde_json::from_str(&wire)?);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err("no event was delivered".into())
}

fn assert_requery_of(event: &EventEnvelope, sequence: u64) {
    assert_eq!(event.topic, "results.freshness_changed");
    assert_eq!(
        event.body["sequence"], sequence,
        "the original sequence is kept"
    );
    assert_eq!(event.body["changed"], json!({"kind": "requery"}));
    assert_eq!(event.body["summary"]["counts"]["unwatched"], 2);
    assert!(event.result_version.is_some());
}

#[test]
fn a_window_that_subscribes_late_and_a_window_after_resync_both_get_the_last_event() -> Check {
    let desktop = Arc::new(DesktopEvents::default());
    let callbacks = DesktopEventCallbacks::new(desktop.clone());
    (callbacks.document_started)();
    (callbacks.document_ready)();
    desktop.publish_host_event(freshness_event(4)?);
    desktop.publish_host_event(freshness_event(5)?);
    let first = next(&callbacks)?;
    assert_eq!(first.body["sequence"], 4);
    assert_eq!(
        first.body["changed"]["kind"], "ranges",
        "the live event keeps its ranges"
    );
    assert_eq!(next(&callbacks)?.body["sequence"], 5);

    // A reloaded window subscribes after both events were delivered to the old one.
    (callbacks.document_started)();
    assert!((callbacks.next)().is_none());
    (callbacks.document_ready)();
    assert_requery_of(&next(&callbacks)?, 5);

    // Posting to the window fails; the resync notice is followed by the retained event.
    (callbacks.post_failed)(EventPostFailure {
        failed: "not an event".into(),
        discarded: Vec::new(),
    });
    assert_eq!(next(&callbacks)?.topic, "job.resync_required");
    assert_requery_of(&next(&callbacks)?, 5);

    // A ledger that is gone is not replayed to the next window.
    let mut gone = freshness_event(6)?;
    gone.result_version = None;
    gone.body["summary"]["counts"]["unwatched"] = json!(0);
    gone.body["changed"] = json!({"kind": "requery"});
    desktop.publish_host_event(gone);
    assert_eq!(next(&callbacks)?.body["sequence"], 6);
    (callbacks.document_started)();
    (callbacks.document_ready)();
    std::thread::sleep(Duration::from_millis(300));
    assert!((callbacks.next)().is_none());
    Ok(())
}
