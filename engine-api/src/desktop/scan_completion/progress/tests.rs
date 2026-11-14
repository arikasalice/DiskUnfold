use super::*;
use crate::desktop::scan_completion::ScanEvents;
use du_core_contracts::{Id, job::ProgressUnit};
use du_job_runtime::JobWaitReason;
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn next_event(desktop: &DesktopEvents) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(wire) = desktop.next_json() {
            return Ok(serde_json::from_str(&wire)?);
        }
        if Instant::now() >= deadline {
            return Err("no desktop event".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn progress(job: &Id, sequence: u64, completed: u64, total: Option<u64>) -> JobProgressEvent {
    JobProgressEvent {
        job_id: job.clone(),
        sequence,
        unit: ProgressUnit::Items,
        completed,
        total,
        remaining: total.map(|total| total.saturating_sub(completed)),
        wait_reason: JobWaitReason::Unknown,
        observed_at_ms: 0,
    }
}

#[test]
fn the_runtime_sink_relays_progress_as_the_job_progress_event() -> TestResult {
    let desktop = Arc::new(DesktopEvents::default());
    let events = ScanEvents::new(desktop.clone());
    let job = Id::parse("00000000-0000-7000-8000-000000000001")?;
    events.publish_progress(progress(&job, 3, 2, None));
    let first = next_event(&desktop)?;
    assert_eq!(first.topic, "job.progress");
    assert_eq!(first.job_ref.as_ref(), Some(&job));
    assert_eq!(first.result_version, None);
    assert_eq!(first.occurred_at.as_str(), "1970-01-01T00:00:00.000Z");
    assert_eq!(
        first.body,
        json!({
            "sequence": 3,
            "unit": "items",
            "completed": 2,
            "totalKnown": false,
            "waitReason": "unknown",
            "observedAt": "1970-01-01T00:00:00.000Z",
        })
    );
    events.publish_progress(progress(&job, 4, 3, Some(10)));
    let second = next_event(&desktop)?;
    assert!(second.seq > first.seq);
    assert_eq!(second.body["total"], json!(10));
    assert_eq!(second.body["totalKnown"], json!(true));
    assert!(second.body.get("remaining").is_none());
    Ok(())
}
