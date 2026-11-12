use super::DesktopEventCallbacks;
use du_core_contracts::{ENVELOPE_VERSION, EventEnvelope, Timestamp};
use du_engine_api::desktop::DesktopEvents;
use du_platform_os::webview::EventPostFailure;
use serde_json::{Value, json};
use std::sync::Arc;

fn wire(topic: &str, sequence: u64, occurred_at: Timestamp) -> Result<String, serde_json::Error> {
    serde_json::to_string(&EventEnvelope {
        topic: topic.into(),
        contract_version: ENVELOPE_VERSION,
        seq: sequence,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at,
        body: json!({"sequence": sequence}),
    })
}

fn marker(wire: &str) -> Result<(Value, u64), Box<dyn std::error::Error>> {
    let event: EventEnvelope = serde_json::from_str(wire)?;
    assert_eq!(event.topic, "job.resync_required");
    Ok((event.body, event.seq))
}

#[test]
fn lifecycle_callbacks_connect_bridge_post_loss_to_desktop_recovery_and_reset_generations()
-> Result<(), Box<dyn std::error::Error>> {
    let observed_at = Timestamp::from_unix_millis(1_000).ok_or("invalid event time")?;
    let desktop = Arc::new(DesktopEvents::default());
    let callbacks = DesktopEventCallbacks::new(desktop);

    (callbacks.document_started)();
    (callbacks.document_ready)();
    (callbacks.post_failed)(EventPostFailure {
        failed: wire("job.progress", 1, observed_at.clone())?,
        discarded: vec![wire("job.finished", 2, observed_at.clone())?],
    });

    let recovery = (callbacks.next)().ok_or("post failure did not reach DesktopEvents")?;
    let (body, first_sequence) = marker(&recovery)?;
    assert_eq!(body["reason"], "subscriber_delivery_failed");
    assert_eq!(body["discardedEvents"], 2);

    // A same-entry reload reuses this callback set. Starting the replacement document
    // clears the old generation's notifications and suspends delivery until load success.
    (callbacks.document_started)();
    assert!((callbacks.next)().is_none());
    (callbacks.document_ready)();
    assert!((callbacks.next)().is_none());

    (callbacks.post_failed)(EventPostFailure {
        failed: wire("job.progress", 3, observed_at)?,
        discarded: Vec::new(),
    });
    let recovery = (callbacks.next)().ok_or("replacement document missed new recovery")?;
    let (body, second_sequence) = marker(&recovery)?;
    assert_eq!(body["discardedEvents"], 1);
    assert!(second_sequence > first_sequence);
    Ok(())
}

#[test]
fn failed_navigation_never_resumes_the_new_document_and_close_discards_its_pending_events()
-> Result<(), Box<dyn std::error::Error>> {
    let observed_at = Timestamp::from_unix_millis(1_000).ok_or("invalid event time")?;
    let desktop = Arc::new(DesktopEvents::default());
    let callbacks = DesktopEventCallbacks::new(desktop);

    (callbacks.document_started)();
    (callbacks.document_ready)();
    (callbacks.document_started)();
    (callbacks.post_failed)(EventPostFailure {
        failed: wire("job.finished", 1, observed_at.clone())?,
        discarded: Vec::new(),
    });
    assert!((callbacks.next)().is_none());

    // NavigationCompleted reports failure by omitting document_ready. Bridge drop
    // then closes this document generation before the UI thread exits.
    (callbacks.document_closed)();
    assert!((callbacks.next)().is_none());
    Ok(())
}
