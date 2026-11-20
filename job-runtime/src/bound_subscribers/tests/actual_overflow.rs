use super::*;
use crate::JobLaneStatusEvent;
use du_core_contracts::job::{LaneReport, LaneStatusReport, LaneWait};
use std::sync::Mutex;

#[derive(Default)]
struct LaneStatusDownstream(Mutex<Vec<JobLaneStatusEvent>>);

impl JobEvents for LaneStatusDownstream {
    fn publish(&self, _: JobEvent) {}

    fn publish_lane_status(&self, event: JobLaneStatusEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

struct LaneStatusBody(mpsc::Sender<Box<dyn JobFlow>>);

impl JobBody for LaneStatusBody {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        context.report_lane_status(LaneStatusReport {
            lanes: vec![LaneReport {
                lane_index: 0,
                root_indexes: vec![0],
                in_flight: 1,
                limit: 2,
                mode: None,
                permit: None,
                strategy: None,
                wait: LaneWait::IoWait,
                wait_confirmed: true,
                remaining_directories: 3,
                failures: 0,
            }],
        });
        if let Ok(flow) = context.try_start_flow() {
            let _ = self.0.send(flow);
        }
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn setup_with_lane_sink()
-> Result<(Arc<RuntimeSubscribers>, Arc<LaneStatusDownstream>), RuntimeError> {
    let downstream = Arc::new(LaneStatusDownstream::default());
    let session = RuntimeSubscribers::new(RuntimeSubscribersConfig {
        identity: Arc::new(Identity(AtomicUsize::new(1))),
        runtime_clock: Arc::new(Clock),
        subscriber_clock: Arc::new(Clock),
        history: Arc::new(History),
        downstream: downstream.clone(),
        limits: FlowLimits {
            metadata_io: 1,
            queued_items: 8,
            item_bytes: 32,
        },
    })?;
    Ok((Arc::new(session), downstream))
}

#[test]
fn real_runtime_isolates_slow_subscriber_and_rejoins_at_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let (session, downstream) = setup_with_lane_sink()?;
    let slow = session.subscribe().ok_or("slow")?;
    let fast = session.subscribe().ok_or("fast")?;
    let runtime = session.runtime();
    register(&runtime)?;
    let (tx, rx) = mpsc::channel();
    let job = match runtime.submit(crate::Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(LaneStatusBody(tx)),
        priority: crate::JobPriority::Normal,
        scope_summary: "scope".into(),
        correlation_id: Identity(AtomicUsize::new(42)).next_id(),
        sample: false,
    })? {
        crate::SubmitResult::Created(id) => id,
        _ => return Err("new job expected".into()),
    };
    let worker = {
        let runtime = runtime.clone();
        thread::spawn(move || runtime.run_next())
    };
    let flow = rx.recv_timeout(Duration::from_secs(2))?;
    runtime.flush()?;
    while slow.try_next(Duration::from_millis(100))?.is_some() {}
    while fast.try_next(Duration::from_millis(100))?.is_some() {}
    for _ in 0..1023 {
        flow.report_item(b"x").map_err(|_| "report")?;
        runtime.flush()?;
        assert!(matches!(
            fast.try_next(Duration::from_millis(100))?,
            Some(SubscriberMessage::Event(JobEvent::ItemResult { .. }))
        ));
    }
    assert!(slow.try_next(Duration::from_millis(99))?.is_none());
    flow.report_item(b"overflow")
        .map_err(|_| "overflow report")?;
    runtime.flush()?;
    assert!(matches!(
        slow.try_next(Duration::from_millis(100))?,
        Some(SubscriberMessage::ResyncRequired { dropped: 1024 })
    ));
    assert!(slow.try_next(Duration::from_millis(100))?.is_none());
    assert!(matches!(
        fast.try_next(Duration::from_millis(100))?,
        Some(SubscriberMessage::Event(JobEvent::ItemResult { .. }))
    ));
    flow.report_item(b"detached")
        .map_err(|_| "detached report")?;
    runtime.flush()?;
    assert!(slow.try_next(Duration::from_secs(1))?.is_none());
    let token = slow.begin_rejoin()?.ok_or("token")?;
    let cut = session.rejoin(&slow, token)?.ok_or("cut")?;
    let sequence = cut.get(&job).ok_or("job")?.snapshot.sequence;
    flow.report_item(b"after").map_err(|_| "after report")?;
    runtime.flush()?;
    assert!(
        matches!(slow.try_next(Duration::from_millis(100))?, Some(SubscriberMessage::Event(JobEvent::ItemResult { sequence: next, .. })) if next > sequence)
    );
    flow.finish(JobCounts::default());
    worker.join().map_err(|_| "worker panic")??;
    runtime.flush()?;
    runtime.flush()?;
    let lane_statuses = downstream
        .0
        .lock()
        .map_err(|_| "downstream lane status lock")?;
    assert_eq!(lane_statuses.len(), 1);
    assert_eq!(lane_statuses[0].job_id, job);
    Ok(())
}
