use super::*;
use du_core_contracts::{OsPath, PathBytes, PathEncoding, analysis_ipc::TargetAccess};

fn observation() -> TargetStatusChanged {
    TargetStatusChanged {
        target: OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[68, 0, 58, 0, 92, 0]),
            display: "D:\\".into(),
            lossy: false,
        },
        access: TargetAccess::Accessible,
        reason_key: None,
        total_bytes: Some(200.into()),
        available_bytes: Some(100.into()),
        capacity_reason: None,
        aggregation_uncertain: true,
    }
}

#[test]
fn probe_event_preserves_job_order_and_only_accepts_typed_results()
-> Result<(), Box<dyn std::error::Error>> {
    let job = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let now = Timestamp::from_unix_millis(1000).ok_or("invalid time")?;
    let input = |kind_id: &str, payload| JobEvent::ItemResult {
        job_id: job.clone(),
        kind_id: kind_id.into(),
        sequence: 7,
        payload,
    };
    let kind = du_scan_engine::target_probe::TARGET_PROBE_KIND;
    assert!(envelope(input(kind, b"{}".to_vec()), now.clone()).is_none());
    let payload = serde_json::to_vec(&observation())?;
    assert!(envelope(input("scan.metadata", payload.clone()), now.clone()).is_none());
    let event = envelope(input(kind, payload), now.clone()).ok_or("missing probe event")?;
    assert_eq!(event.topic, "target.status_changed");
    assert_eq!(event.job_ref, Some(job));
    assert_eq!(event.occurred_at, now);
    let body: TargetProbeEvent = serde_json::from_value(event.body)?;
    assert_eq!(body.sequence, 7);
    assert_eq!(body.observation.access, TargetAccess::Accessible);
    assert_eq!(body.observation.total_bytes, Some(200.into()));
    Ok(())
}

#[test]
fn batches_events_without_dropping_order_and_coalesces_overflow_into_resync()
-> Result<(), Box<dyn std::error::Error>> {
    let job = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let occurred_at = Timestamp::from_unix_millis(0).ok_or("invalid time")?;
    let event = envelope(
        JobEvent::StateChanged {
            job_id: job,
            state: du_core_contracts::job::JobState::Running,
            sequence: 2,
        },
        occurred_at,
    )
    .ok_or("missing state event")?;
    let mut pending = Pending::default();
    let now = Instant::now();
    pending.push(event.clone(), now);
    pending.push(event.clone(), now + Duration::from_millis(50));
    assert!(pending.take(now + Duration::from_millis(99)).is_none());
    let batch_time = now + EVENT_INTERVAL;
    let first = pending.take(batch_time).ok_or("missing first event")?;
    pending.push(event.clone(), batch_time);
    assert_eq!(
        pending.take(batch_time).ok_or("missing second event")?.seq,
        first.seq + 1
    );
    assert!(pending.take(batch_time).is_none());
    for _ in 0..EVENT_LIMIT + 1 {
        pending.push(event.clone(), batch_time);
    }
    let resync = pending
        .take(batch_time + EVENT_INTERVAL)
        .ok_or("missing resync")?;
    assert_eq!(resync.topic, "job.resync_required");
    assert!(resync.job_ref.is_none());
    assert!(resync.seq > first.seq + 1);
    assert!(pending.take(batch_time + EVENT_INTERVAL).is_none());
    pending.push(event, batch_time + EVENT_INTERVAL);
    assert_eq!(
        pending
            .take(batch_time + EVENT_INTERVAL * 2)
            .ok_or("missing recovered event")?
            .seq,
        resync.seq + 1
    );
    Ok(())
}
