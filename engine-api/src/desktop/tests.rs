use super::*;
use du_core_contracts::result_events::{
    PublishedCompleteness, ResultVersionChange, ResultVersionChanged, ResultVersionNotice,
};
use du_core_contracts::{OsPath, PathBytes, PathEncoding, analysis_ipc::TargetAccess};

fn observation(observed_at: Timestamp) -> TargetStatusChanged {
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
        observed_at,
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
    let observed_at = Timestamp::from_unix_millis(42).ok_or("invalid observation time")?;
    let payload = serde_json::to_vec(&observation(observed_at.clone()))?;
    assert!(envelope(input("scan.metadata", payload.clone()), now.clone()).is_none());
    let event = envelope(input(kind, payload), now.clone()).ok_or("missing probe event")?;
    assert_eq!(event.topic, "target.status_changed");
    assert_eq!(event.job_ref, Some(job));
    assert_eq!(event.occurred_at, now);
    let body: TargetProbeEvent = serde_json::from_value(event.body)?;
    assert_eq!(body.sequence, 7);
    assert_eq!(body.observation.access, TargetAccess::Accessible);
    assert_eq!(body.observation.total_bytes, Some(200.into()));
    assert_eq!(body.observation.observed_at, observed_at);
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

#[test]
fn history_change_uses_the_public_revision_and_omits_internal_references()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::job::HistoryRevision;

    let job = Id::from_v7_bytes([42; 16]);
    let events = DesktopEvents::default();
    events.publish(JobEvent::StateChanged {
        job_id: job.clone(),
        state: du_core_contracts::job::JobState::Done,
        sequence: 7,
    });
    events.publish_history_changed(HistoryChanged {
        revision: HistoryRevision {
            delete_generation: 2,
            max_record_seq: 9_007_199_254_740_993,
        },
        reason: HistoryChangeReason::Appended,
        count: 1,
        observed_at_ms: 42,
        deleted_job_ids: vec![job.clone()],
    });
    let mut pending = events.0.lock().map_err(|_| "pending lock failed")?;
    let after_batch = Instant::now() + EVENT_INTERVAL;
    assert_eq!(
        pending
            .take(after_batch)
            .ok_or("missing prior event")?
            .topic,
        "job.state_changed"
    );
    let envelope = pending.take(after_batch).ok_or("missing history event")?;
    assert_eq!(envelope.topic, HISTORY_CHANGED);
    assert_eq!(envelope.seq, 2);
    let wire = serde_json::to_value(envelope)?;
    assert_eq!(
        wire["body"],
        json!({
            "revision": {"deleteGeneration": "2", "maxRecordSeq": "9007199254740993"},
            "reason": "appended",
            "count": "1",
            "observedAt": wire["occurredAt"],
        })
    );
    assert!(!wire.to_string().contains(job.as_str()));
    assert!(pending.take(after_batch).is_none());
    Ok(())
}

#[test]
fn preserves_lineage_sequence_and_withdrawal_without_a_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let lineage_id = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let partial = Id::parse("0195f856-ab84-7a59-9343-10d29775c9a9")?;
    let observed = Timestamp::from_unix_millis(100).ok_or("invalid observation time")?;
    let occurred = Timestamp::from_unix_millis(200).ok_or("invalid event time")?;
    let body = ResultVersionChanged {
        source_ref: lineage_id.clone(),
        lineage_id,
        origin: "scan".into(),
        sequence: 7,
        change: ResultVersionChange::Published,
        previous_result_version: None,
        completeness: Some(PublishedCompleteness::Partial),
        observed_at: Some(observed.clone()),
    };
    let publish = ResultVersionNotice {
        result_version: Some(partial.clone()),
        body: body.clone(),
    };
    let withdrawal = ResultVersionNotice {
        result_version: None,
        body: ResultVersionChanged {
            sequence: 8,
            change: ResultVersionChange::Withdrawn,
            previous_result_version: Some(partial.clone()),
            completeness: None,
            observed_at: None,
            ..body
        },
    };
    let now = Instant::now();
    let mut pending = Pending::default();
    for notice in [publish, withdrawal] {
        assert!(notice.validate());
        let event = envelope(
            JobEvent::ResultVersionChanged(Box::new(notice)),
            occurred.clone(),
        )
        .ok_or("missing result version event")?;
        pending.push(event, now);
    }
    let published = pending
        .take(now + EVENT_INTERVAL)
        .ok_or("missing publish")?;
    assert_eq!(published.seq, 1);
    assert_eq!(published.result_version.as_ref(), Some(&partial));
    assert_eq!(published.body["sequence"], 7);
    assert_eq!(published.body["observedAt"], observed.as_str());
    assert_eq!(published.occurred_at, occurred);

    let withdrawn = pending
        .take(now + EVENT_INTERVAL)
        .ok_or("missing withdrawal")?;
    assert_eq!(withdrawn.topic, RESULT_VERSION_CHANGED);
    assert_eq!(withdrawn.seq, 2);
    let wire = serde_json::to_value(withdrawn)?;
    assert!(wire.get("resultVersion").is_none());
    assert!(wire.get("jobRef").is_none());
    assert_eq!(wire["body"]["sequence"], 8);
    assert_eq!(wire["body"]["change"], "withdrawn");
    assert_eq!(wire["body"]["previousResultVersion"], partial.as_str());
    assert!(wire["body"].get("completeness").is_none());
    assert!(wire["body"].get("observedAt").is_none());
    Ok(())
}
