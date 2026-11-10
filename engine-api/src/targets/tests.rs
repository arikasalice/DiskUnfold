use super::*;
use du_core_contracts::{
    ByteSize, CallEnvelope, ENVELOPE_VERSION, EventEnvelope, Id,
    analysis_ipc::{TargetProbeEvent, TargetStatusChanged},
    job::{HistoryAppend, JobBody, JobContext, JobHistory, JobOutcome, JobState, JobSummary},
    ports::{PortError, PortErrorKind, Random},
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
    volume::{Capability, VolumeCapabilities, VolumeCapacity, VolumeEnumeration},
};
use du_job_runtime::{JobEvent, JobEvents, JobIdentity, JobRuntime};
use std::time::Instant;

#[test]
fn slow_discovery_returns_empty_listing_without_launching_a_probe()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::mpsc;

    struct Blocked {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl VolumePort for Blocked {
        fn enumerate(&self) -> VolumeEnumeration {
            let _ = self.entered.send(());
            if let Ok(release) = self.release.lock() {
                let _ = release.recv_timeout(Duration::from_secs(5));
            }
            Volumes(None).enumerate()
        }
        fn path_access(&self, path: &[u8]) -> Result<(), PortError> {
            Volumes(None).path_access(path)
        }
        fn capacity(&self, path: &[u8]) -> Result<VolumeCapacity, PortError> {
            Volumes(None).capacity(path)
        }
    }

    let (_, workers) = setup()?;
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let volumes = Arc::new(Blocked {
        entered,
        release: Mutex::new(gate),
    });
    let mut list = list(workers, None);
    list.enumeration = EnumerationWorker::new(volumes);
    list.enumeration_timeout = Duration::from_millis(100);
    let request = TargetListRequest { refresh: true };
    let reply = list
        .execute(request.clone(), &caller())
        .map_err(|e| format!("{e:?}"))?;
    started.recv_timeout(Duration::from_secs(5))?;
    let retry = list
        .execute(request, &caller())
        .map_err(|e| format!("{e:?}"))?;
    for observed in [reply, retry] {
        assert!(observed.targets.is_empty());
        assert!(observed.probe_job_ref.is_none());
        assert_eq!(observed.unavailable_reason.as_deref(), Some("os.call_slow"));
    }
    assert!(started.try_recv().is_err());
    release.send(())?;
    Ok(())
}

struct Fixture;
impl Random for Fixture {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(7);
        Ok(())
    }
}
impl JobIdentity for Fixture {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes())
    }
}
impl JobClock for Fixture {
    fn now_ms(&self) -> i64 {
        1
    }
}
impl JobEvents for Fixture {
    fn publish(&self, _: JobEvent) {}
}
impl JobHistory for Fixture {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Err("unexpected history".into())
    }
}
impl JobBody for Fixture {
    fn kind_id(&self) -> &str {
        du_scan_engine::target_probe::TARGET_PROBE_KIND
    }
    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        JobOutcome {
            counts: Default::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
struct Volumes(Option<VolumeTargetKind>);
impl VolumePort for Volumes {
    fn enumerate(&self) -> VolumeEnumeration {
        let supported = Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        };
        let partial = Capability {
            state: CapabilityState::Partial,
            reason_key: Some("os.unsupported"),
        };
        VolumeEnumeration {
            mounts: Ok(vec![VolumeMount {
                mount_path: vec![67, 0, 58, 0, 92, 0],
                display_path: "C:\\".into(),
                filesystem: vec![],
                mount_source: vec![],
                role: None,
                topology: None,
                target_kind: self.0,
                volume_id: Some(vec![1, 2]),
            }]),
            capabilities: VolumeCapabilities {
                enumerate: supported.clone(),
                identity: partial,
                capacity: supported.clone(),
                path_access: supported.clone(),
                path_compare_rule: supported.clone(),
                mount_boundary: supported.clone(),
                alias: supported,
            },
        }
    }
    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        Ok(())
    }
    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Ok(VolumeCapacity {
            total_bytes: 100,
            available_bytes: 50,
        })
    }

    fn observe(
        &self,
        path: &[u8],
        expected: Option<&[u8]>,
        should_read_capacity: &dyn Fn() -> bool,
    ) -> Result<du_core_contracts::volume::VolumeProbe, PortError> {
        assert_eq!(expected, Some([1u8, 2].as_slice()));
        self.path_access(path)?;
        Ok(du_core_contracts::volume::VolumeProbe {
            capacity: should_read_capacity().then(|| self.capacity(path)),
        })
    }
}

struct NoRoots;
impl ScopeRootPort for NoRoots {
    fn open_root(&self, _: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}
fn submission() -> Submission {
    Submission {
        kind_id: du_scan_engine::target_probe::TARGET_PROBE_KIND.into(),
        deduplication_key: None,
        body: Arc::new(Fixture),
        priority: JobPriority::Normal,
        scope_summary: String::new(),
        correlation_id: Fixture.next_id(),
        sample: false,
    }
}
fn setup() -> Result<(Arc<JobRuntime>, Arc<crate::workers::Workers>), Box<dyn std::error::Error>> {
    let fixture = Arc::new(Fixture);
    let runtime = Arc::new(JobRuntime::new(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture,
    )?);
    runtime.register(du_scan_engine::target_probe::descriptor())?;
    let workers = Arc::new(crate::workers::Workers::start(runtime.clone())?);
    Ok((runtime, workers))
}
fn list(workers: Arc<crate::workers::Workers>, previous: Option<Id>) -> List {
    let volumes: Arc<dyn VolumePort> = Arc::new(Volumes(Some(VolumeTargetKind::Internal)));
    List {
        enumeration: EnumerationWorker::new(volumes.clone()),
        enumeration_timeout: Duration::from_secs(2),
        volumes,
        clock: Arc::new(Fixture),
        probe: Probe {
            workers,
            timeout: Duration::from_millis(50),
            previous: Mutex::new(previous),
        },
    }
}
fn caller() -> CallerCtx {
    CallerCtx::new(Channel::Gui, Fixture.next_id(), None)
}
#[test]
fn new_list_cancels_previous_and_keeps_partial_identity() -> Result<(), Box<dyn std::error::Error>>
{
    let (runtime, workers) = setup()?;
    let old = match runtime.submit(submission())? {
        SubmitResult::Created(id) => id,
        _ => return Err("unexpected existing job".into()),
    };
    let result = list(workers, Some(old.clone()))
        .execute(TargetListRequest { refresh: true }, &caller())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(runtime.state(&old)?, JobState::Cancelled);
    assert!(result.probe_job_ref.is_some());
    assert!(result.targets[0].volume_id.is_some());
    assert_eq!(result.targets[0].identity_source, IdentitySource::Confirmed);
    Ok(())
}
#[test]
fn full_queue_retains_targets_and_marks_probe_deferred() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, workers) = setup()?;
    for _ in 0..64 {
        runtime.submit(submission())?;
    }
    let result = list(workers, None)
        .execute(TargetListRequest { refresh: false }, &caller())
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.probe_job_ref.is_none());
    assert_eq!(result.targets.len(), 1);
    assert_eq!(
        result.targets[0].reason_key.as_deref(),
        Some("job.queue_full")
    );
    Ok(())
}

#[test]
fn unclassified_target_is_listed_and_delivers_probe_then_terminal_on_the_desktop_bus()
-> Result<(), Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    let events = crate::desktop::register(
        &mut builder,
        Arc::new(NoRoots),
        Arc::new(Volumes(None)),
        Arc::new(Fixture),
        &Fixture,
    )?;
    let engine = builder.build()?;
    let ctx = caller();
    let reply = engine.call(
        CallEnvelope {
            name: "target.list".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: ctx.correlation_id.clone(),
            body: serde_json::json!({ "refresh": false }),
        },
        &ctx,
    );
    assert!(reply.error.is_none());
    assert!(
        reply.body["targets"][0]
            .get("kind")
            .is_some_and(serde_json::Value::is_null)
    );
    let list: TargetList = serde_json::from_value(reply.body)?;
    let job = list.probe_job_ref.ok_or("missing probe job")?;
    assert_eq!(list.targets.len(), 1);
    assert_eq!(list.unavailable_reason, None);
    assert_eq!(list.targets[0].kind, None);
    assert_eq!(list.targets[0].identity_source, IdentitySource::Confirmed);
    assert_eq!(list.targets[0].access, TargetAccess::Checking);
    let mut reports = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let terminal = loop {
        if Instant::now() >= deadline {
            return Err("target probe did not finish".into());
        }
        let Some(json) = events.next_json() else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        let event: EventEnvelope = serde_json::from_str(&json)?;
        assert_eq!(event.job_ref.as_ref(), Some(&job));
        if event.topic == "job.finished" {
            break event;
        }
        if event.topic == "target.status_changed" {
            reports.push(serde_json::from_value::<TargetProbeEvent>(event.body)?);
        }
    };
    assert_eq!(reports.len(), 1);
    let TargetStatusChanged {
        access,
        total_bytes,
        available_bytes,
        observed_at,
        ..
    } = &reports[0].observation;
    assert_eq!(*access, TargetAccess::Accessible);
    assert_eq!(total_bytes.as_ref().map(ByteSize::as_str), Some("100"));
    assert_eq!(available_bytes.as_ref().map(ByteSize::as_str), Some("50"));
    assert_ne!(observed_at.as_str(), "1970-01-01T00:00:00.000Z");
    assert_eq!(terminal.body["historyRecorded"], false);
    assert!(terminal.body["sequence"].as_u64() > Some(reports[0].sequence));
    assert!(events.next_json().is_none());
    Ok(())
}
