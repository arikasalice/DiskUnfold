//! Expose only target kinds supported by the supplied volume observations.
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use du_core_contracts::{
    OsPath, PathBytes, PathEncoding, Timestamp,
    analysis_ipc::{
        AnalysisTarget, IdentitySource, TargetAccess, TargetKind, TargetList, TargetListRequest,
    },
    volume::{CapabilityState, VolumeMount, VolumePort, VolumeTargetKind},
};
use du_job_runtime::{JobClock, JobPriority, RuntimeError, Submission, SubmitResult};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// Registers target discovery with a shared job worker and injected deadline.
pub(crate) fn register_with_probe(
    builder: &mut EngineBuilder,
    volumes: Arc<dyn VolumePort>,
    clock: Arc<dyn JobClock>,
    workers: Arc<crate::workers::Workers>,
    timeout: Duration,
) -> Result<(), RuntimeError> {
    workers
        .runtime
        .register(du_scan_engine::target_probe::descriptor())?;
    builder.register(List {
        volumes,
        clock,
        probe: Probe {
            workers,
            timeout,
            previous: Mutex::new(None),
        },
    });
    Ok(())
}

struct Probe {
    workers: Arc<crate::workers::Workers>,
    timeout: Duration,
    previous: Mutex<Option<du_core_contracts::Id>>,
}

struct List {
    volumes: Arc<dyn VolumePort>,
    clock: Arc<dyn JobClock>,
    probe: Probe,
}

impl UseCase for List {
    const NAME: &'static str = "target.list";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = TargetListRequest;
    type Response = TargetList;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, caller: &CallerCtx) -> Result<Self::Response, EngineError> {
        let probe = &self.probe;
        let mut previous = probe
            .previous
            .lock()
            .map_err(|_| EngineError::reason("support.internal_error"))?;
        if let Some(id) = previous.take() {
            let _ = probe.workers.runtime.cancel(&id);
        }
        let observation = self.volumes.enumerate();
        let capacity = observation.capabilities.capacity.clone();
        let path_access = observation.capabilities.path_access.clone();
        let identity_supported =
            observation.capabilities.identity.state != CapabilityState::Unsupported;
        let (mut targets, unavailable_reason) = if observation.capabilities.enumerate.state
            == CapabilityState::Unsupported
        {
            (
                Vec::new(),
                Some(
                    observation
                        .capabilities
                        .enumerate
                        .reason_key
                        .unwrap_or("os.unsupported")
                        .to_owned(),
                ),
            )
        } else {
            match observation.mounts {
                Ok(mounts) => {
                    let observed_mounts = !mounts.is_empty();
                    let mut targets: Vec<_> = mounts
                        .into_iter()
                        .filter_map(|mount| target(mount, identity_supported))
                        .collect();
                    targets
                        .sort_by_cached_key(|target| target.path.raw.bytes().unwrap_or_default());
                    targets.dedup_by(|a, b| a.path == b.path);
                    let reason = (observed_mounts && targets.is_empty())
                        .then(|| "os.unsupported".to_owned());
                    (targets, reason)
                }
                Err(error) => (Vec::new(), Some(error.reason_key)),
            }
        };
        let observed_at = Timestamp::from_unix_millis(self.clock.now_ms())
            .ok_or_else(|| EngineError::reason("support.internal_error"))?;
        let mut probe_job_ref = None;
        if !targets.is_empty() {
            let body = Arc::new(crate::target_probe::TargetProbe {
                paths: targets.iter().map(|target| target.path.clone()).collect(),
                volumes: self.volumes.clone(),
                capacity,
                path_access,
                timeout: probe.timeout,
            });
            let submission = Submission {
                kind_id: du_scan_engine::target_probe::TARGET_PROBE_KIND.into(),
                deduplication_key: None,
                body,
                priority: JobPriority::Normal,
                scope_summary: String::new(),
                correlation_id: caller.correlation_id.clone(),
                sample: false,
            };
            match probe.workers.runtime.submit(submission) {
                Ok(SubmitResult::Created(id)) => {
                    for target in &mut targets {
                        target.access = TargetAccess::Checking;
                    }
                    *previous = Some(id.clone());
                    probe_job_ref = Some(id);
                    probe.workers.notify();
                }
                Err(RuntimeError::QueueFull | RuntimeError::DeliveryBusy) => {
                    for target in &mut targets {
                        target.reason_key = Some("job.queue_full".into());
                    }
                }
                _ => return Err(EngineError::reason("support.internal_error")),
            }
        }
        Ok(TargetList {
            targets,
            probe_job_ref,
            observed_at,
            unavailable_reason,
        })
    }
}

fn target(mount: VolumeMount, identity_supported: bool) -> Option<AnalysisTarget> {
    let kind = match mount.target_kind? {
        VolumeTargetKind::Internal => TargetKind::Internal,
        VolumeTargetKind::External => TargetKind::External,
        VolumeTargetKind::NetworkMount => TargetKind::NetworkMount,
        VolumeTargetKind::FolderMount => TargetKind::FolderMount,
    };
    let path = crate::scope::windows_path(&OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&mount.mount_path),
        display: mount.display_path,
        lossy: false,
    })
    .ok()?;
    let volume_id = mount
        .volume_id
        .filter(|_| identity_supported)
        .map(|raw| PathBytes::from_bytes(&raw).as_str().into());
    let identity_source = if volume_id.is_some() {
        IdentitySource::Confirmed
    } else {
        IdentitySource::Unavailable
    };
    Some(AnalysisTarget {
        kind,
        path,
        connected: true,
        volume_id,
        identity_source,
        access: TargetAccess::Unknown,
        reason_key: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{
        ByteSize, CallEnvelope, ENVELOPE_VERSION, EventEnvelope, Id,
        analysis_ipc::{TargetProbeEvent, TargetStatusChanged},
        job::{HistoryAppend, JobBody, JobContext, JobHistory, JobOutcome, JobState, JobSummary},
        ports::{PortError, PortErrorKind},
        scan_observation::{OpenedScopeRoot, ScopeRootPort},
        volume::{Capability, VolumeCapabilities, VolumeCapacity, VolumeEnumeration},
    };
    use du_job_runtime::{JobEvent, JobEvents, JobIdentity, JobRuntime};
    use std::time::Instant;

    struct Fixture;
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
    struct Volumes;
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
                    target_kind: Some(VolumeTargetKind::Internal),
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
    fn setup() -> Result<(Arc<JobRuntime>, Arc<crate::workers::Workers>), Box<dyn std::error::Error>>
    {
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
        List {
            volumes: Arc::new(Volumes),
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
    fn new_list_cancels_previous_and_keeps_partial_identity()
    -> Result<(), Box<dyn std::error::Error>> {
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
    fn full_queue_retains_targets_and_marks_probe_deferred()
    -> Result<(), Box<dyn std::error::Error>> {
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
    fn composed_target_list_delivers_probe_then_terminal_on_the_desktop_bus()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut builder = EngineBuilder::default();
        let events = crate::desktop::register(
            &mut builder,
            Arc::new(NoRoots),
            Arc::new(Volumes),
            Arc::new(Fixture),
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
        let list: TargetList = serde_json::from_value(reply.body)?;
        let job = list.probe_job_ref.ok_or("missing probe job")?;
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
            ..
        } = &reports[0].observation;
        assert_eq!(*access, TargetAccess::Accessible);
        assert_eq!(total_bytes.as_ref().map(ByteSize::as_str), Some("100"));
        assert_eq!(available_bytes.as_ref().map(ByteSize::as_str), Some("50"));
        assert_eq!(terminal.body["historyRecorded"], false);
        assert!(terminal.body["sequence"].as_u64() > Some(reports[0].sequence));
        assert!(events.next_json().is_none());
        Ok(())
    }
}
