//! Target probes through the actual job runtime and its metadata permit.
use super::*;

struct RuntimeFixture(mpsc::Sender<du_job_runtime::JobEvent>);
impl du_job_runtime::JobIdentity for RuntimeFixture {
    fn next_id(&self) -> du_core_contracts::Id {
        du_core_contracts::Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes())
    }
}
impl du_job_runtime::JobClock for RuntimeFixture {
    fn now_ms(&self) -> i64 {
        1
    }
}
impl du_job_runtime::JobEvents for RuntimeFixture {
    fn publish(&self, event: du_job_runtime::JobEvent) {
        let _ = self.0.send(event);
    }
}
impl du_core_contracts::job::JobHistory for RuntimeFixture {
    fn append(
        &self,
        _: &du_core_contracts::job::JobSummary,
    ) -> Result<du_core_contracts::job::HistoryAppend, String> {
        Err("unexpected history".into())
    }
}
#[test]
fn actual_runtime_reuses_single_permit_for_three_targets() -> Result<(), Box<dyn std::error::Error>>
{
    use du_job_runtime::{FlowLimits, JobEvent, JobIdentity, JobPriority, JobRuntime, Submission};
    let (send, recv) = mpsc::channel();
    let fixture = Arc::new(RuntimeFixture(send));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 16,
            item_bytes: 65536,
        },
    )?);
    runtime.register(target_probe::descriptor())?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let cap = Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    };
    let body = TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![
            probe_target("delay"),
            probe_target("fast"),
            probe_target("third"),
        ],
        volumes: Arc::new(Port(gate)),
        capacity: cap.clone(),
        path_access: cap,
        timeout: Duration::from_secs(2),
    };
    runtime.submit(Submission {
        kind_id: TARGET_PROBE_KIND.into(),
        deduplication_key: None,
        body: Arc::new(body),
        priority: JobPriority::Normal,
        scope_summary: String::new(),
        scope_roots: Vec::new(),
        correlation_id: fixture.next_id(),
        sample: false,
    })?;
    let worker = runtime.clone();
    let thread = std::thread::spawn(move || worker.run_next());
    let mut items = Vec::new();
    let mut finished = 0;
    while finished == 0 {
        match recv.recv_timeout(Duration::from_secs(3))? {
            JobEvent::ItemResult { payload, .. } => {
                items.push(serde_json::from_slice::<TargetStatusChanged>(&payload)?)
            }
            JobEvent::Finished { .. } => finished += 1,
            _ => {}
        }
    }
    thread.join().map_err(|_| "worker panic")??;
    assert_eq!(items.len(), 3);
    assert!(
        items
            .iter()
            .all(|item| item.access == TargetAccess::Accessible)
    );
    Ok(())
}

#[test]
fn actual_runtime_timeout_retains_permit_and_suppresses_late_result()
-> Result<(), Box<dyn std::error::Error>> {
    use du_job_runtime::{FlowLimits, JobEvent, JobIdentity, JobPriority, JobRuntime, Submission};
    let (send, recv) = mpsc::channel();
    let fixture = Arc::new(RuntimeFixture(send));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 16,
            item_bytes: 65536,
        },
    )?);
    runtime.register(target_probe::descriptor())?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let cap = Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    };
    let body = TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("slow"), probe_target("fast")],
        volumes: Arc::new(Port(gate.clone())),
        capacity: cap.clone(),
        path_access: cap,
        // The limit must not expire before "slow" enters the port, or the
        // probe reports it without holding the permit and "fast" can run.
        timeout: IN_TIME_LIMIT,
    };
    runtime.submit(Submission {
        kind_id: TARGET_PROBE_KIND.into(),
        deduplication_key: None,
        body: Arc::new(body),
        priority: JobPriority::Normal,
        scope_summary: String::new(),
        scope_roots: Vec::new(),
        correlation_id: fixture.next_id(),
        sample: false,
    })?;
    let worker = runtime.clone();
    let thread = std::thread::spawn(move || worker.run_next());
    let mut items = Vec::new();
    while items.len() < 2 {
        if let JobEvent::ItemResult { payload, .. } = recv.recv_timeout(Duration::from_secs(3))? {
            items.push(serde_json::from_slice::<TargetStatusChanged>(&payload)?);
        }
    }
    assert!(
        items
            .iter()
            .all(|item| item.access == TargetAccess::Unresponsive)
    );
    release(&gate);
    loop {
        match recv.recv_timeout(Duration::from_secs(3))? {
            JobEvent::ItemResult { .. } => return Err("late item emitted".into()),
            JobEvent::Finished { .. } => break,
            _ => {}
        }
    }
    thread.join().map_err(|_| "worker panic")??;
    assert!(!matches!(
        recv.recv_timeout(Duration::from_millis(50)),
        Ok(JobEvent::ItemResult { .. } | JobEvent::Finished { .. })
    ));
    Ok(())
}

#[test]
fn actual_runtime_foreign_permit_saturation_ends_with_reason()
-> Result<(), Box<dyn std::error::Error>> {
    use du_job_runtime::{
        FlowLimits, JobEvent, JobIdentity, JobPriority, JobRuntime, Submission, SubmitResult,
    };
    let (send, recv) = mpsc::channel();
    let fixture = Arc::new(RuntimeFixture(send));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 16,
            item_bytes: 65536,
        },
    )?);
    runtime.register(target_probe::descriptor())?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (started, wait_started) = mpsc::sync_channel(1);
    let cap = Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    };
    let submit = |body: TargetProbe| Submission {
        kind_id: TARGET_PROBE_KIND.into(),
        deduplication_key: None,
        body: Arc::new(body),
        priority: JobPriority::Normal,
        scope_summary: String::new(),
        scope_roots: Vec::new(),
        correlation_id: fixture.next_id(),
        sample: false,
    };
    runtime.submit(submit(TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("slow")],
        volumes: Arc::new(SignalPort {
            gate: gate.clone(),
            started,
        }),
        capacity: cap.clone(),
        path_access: cap.clone(),
        timeout: Duration::from_secs(5),
    }))?;
    let first_runtime = runtime.clone();
    let first = std::thread::spawn(move || first_runtime.run_next());
    wait_started.recv_timeout(Duration::from_secs(2))?;
    let second = match runtime.submit(submit(TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("fast")],
        volumes: Arc::new(Port(gate.clone())),
        capacity: cap.clone(),
        path_access: cap,
        timeout: Duration::from_millis(80),
    }))? {
        SubmitResult::Created(id) => id,
        _ => return Err("unexpected existing".into()),
    };
    let second_runtime = runtime.clone();
    let second_worker = std::thread::spawn(move || second_runtime.run_next());
    loop {
        if let JobEvent::Finished { summary, .. } = recv.recv_timeout(Duration::from_secs(2))?
            && summary.job_id == second
        {
            assert_eq!(summary.failure_reason_key.as_deref(), Some("os.call_slow"));
            break;
        }
    }
    second_worker.join().map_err(|_| "second panic")??;
    release(&gate);
    first.join().map_err(|_| "first panic")??;
    Ok(())
}
