use super::*;
use crate::desktop::scan_completion::ScanEvents;
use du_core_contracts::{
    Id,
    analysis_ipc::{ScanLaneStrategy, ScanResourceMode},
    job::{LaneReport, LaneStatusReport, LaneWait, ProgressUnit},
};
use du_job_runtime::{JobEvent, JobLaneStatusEvent, JobWaitReason};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn assert_resource_fields(lane: &serde_json::Value) {
    assert_eq!(lane["mode"], json!("quiet"));
    assert_eq!(lane["permit"], json!(16));
    assert_eq!(lane["strategy"]["id"], json!("resource.static"));
    assert_eq!(lane["strategy"]["version"], json!(1));
}

fn assert_priority_fields(active: &serde_json::Value, inactive: &serde_json::Value) {
    assert_eq!(active["priorityArea"], json!("opaque:priority-key"));
    assert_eq!(active["remainingPrioritizedDirectories"], json!("12"));
    assert!(inactive.get("priorityArea").is_none());
    assert!(inactive.get("remainingPrioritizedDirectories").is_none());
}

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

fn lane_status(job: &Id) -> JobLaneStatusEvent {
    let lanes = (0..65)
        .rev()
        .map(|lane_index| LaneReport {
            lane_index,
            root_indexes: if lane_index == 0 {
                (0..65).rev().collect()
            } else {
                vec![lane_index]
            },
            in_flight: 1,
            limit: 2,
            mode: (lane_index == 0).then_some(ScanResourceMode::Quiet),
            permit: (lane_index == 0).then_some(16),
            strategy: (lane_index == 0).then(|| ScanLaneStrategy {
                id: "resource.static".into(),
                version: 1,
            }),
            priority_area: (lane_index == 0).then(|| "opaque:priority-key".into()),
            remaining_prioritized_directories: (lane_index == 0).then(|| "12".into()),
            wait: if lane_index == 0 {
                LaneWait::Backpressure
            } else {
                LaneWait::None
            },
            wait_confirmed: lane_index != 0,
            remaining_directories: u64::MAX,
            failures: u64::MAX - 1,
        })
        .collect();
    JobLaneStatusEvent {
        job_id: job.clone(),
        sequence: 23,
        observed_at_ms: 0,
        status: LaneStatusReport { lanes },
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
    assert_eq!(first.occurred_at.to_string(), "1970-01-01T00:00:00.000Z");
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

#[test]
fn the_desktop_relay_sorts_and_caps_lane_status_with_decimal_counts() -> TestResult {
    let desktop = Arc::new(DesktopEvents::default());
    let events = ScanEvents::new(desktop.clone());
    let job = Id::parse("00000000-0000-7000-8000-000000000002")?;
    events.publish_lane_status(lane_status(&job));

    let event = next_event(&desktop)?;
    assert_eq!(event.topic, "scan.lane_status");
    assert_eq!(event.job_ref.as_ref(), Some(&job));
    assert_eq!(event.result_version, None);
    assert_eq!(event.body["sequence"], json!(23));
    assert_eq!(event.body["laneCount"], json!("65"));
    let lanes = event.body["lanes"].as_array().ok_or("lanes array")?;
    assert_eq!(lanes.len(), 64);
    assert_eq!(lanes[0]["laneIndex"], json!(0));
    assert_eq!(lanes[63]["laneIndex"], json!(63));
    assert_resource_fields(&lanes[0]);
    assert_priority_fields(&lanes[0], &lanes[1]);
    assert_eq!(lanes[0]["rootCount"], json!("65"));
    let roots = lanes[0]["rootIndexes"].as_array().ok_or("root indexes")?;
    assert_eq!(roots.len(), 64);
    assert_eq!(roots[0], json!(0));
    assert_eq!(roots[63], json!(63));
    assert_eq!(
        lanes[0]["remainingDirectories"],
        json!(u64::MAX.to_string())
    );
    assert_eq!(lanes[0]["failures"], json!((u64::MAX - 1).to_string()));
    assert_eq!(lanes[0]["waitReason"], json!("backpressure"));
    assert_eq!(lanes[0]["waitConfirmed"], json!(false));
    assert_eq!(event.body["observedAt"], json!("1970-01-01T00:00:00.000Z"));
    Ok(())
}

#[derive(Default)]
struct NonDesktopSink(std::sync::Mutex<Vec<JobEvent>>);

impl JobEvents for NonDesktopSink {
    fn publish(&self, event: JobEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

impl ProgressEvents for NonDesktopSink {}

#[test]
fn non_desktop_job_event_sinks_ignore_lane_status_by_default() -> TestResult {
    let sink = Arc::new(NonDesktopSink::default());
    let events = ScanEvents::new(sink.clone());
    let job = Id::parse("00000000-0000-7000-8000-000000000003")?;
    events.publish_lane_status(lane_status(&job));
    assert!(sink.0.lock().map_err(|_| "event sink lock")?.is_empty());
    Ok(())
}
