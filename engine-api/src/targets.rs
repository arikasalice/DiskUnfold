//! List observed target paths and preserve the kind only when it is established.
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use du_core_contracts::{
    OsPath, PathBytes, PathEncoding, Timestamp,
    analysis_ipc::{
        AnalysisTarget, IdentitySource, TargetAccess, TargetKind, TargetList, TargetListRequest,
    },
    volume::{CapabilityState, VolumeEnumeration, VolumeMount, VolumePort, VolumeTargetKind},
};
use du_job_runtime::{JobClock, JobPriority, RuntimeError, Submission, SubmitResult};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    target_enumeration::{EnumerationWorker, StartError, WaitResult},
};

/// Independent limits for the first listing and each subsequent device probe.
pub(crate) struct Deadlines {
    pub enumeration: Duration,
    pub probe: Duration,
}

/// Registers target discovery with a shared job worker and injected deadlines.
pub(crate) fn register_with_probe(
    builder: &mut EngineBuilder,
    volumes: Arc<dyn VolumePort>,
    clock: Arc<dyn JobClock>,
    workers: Arc<crate::workers::Workers>,
    deadlines: Deadlines,
) -> Result<(), RuntimeError> {
    workers
        .runtime
        .register(du_scan_engine::target_probe::descriptor())?;
    builder.register(List {
        enumeration: EnumerationWorker::new(volumes.clone()),
        enumeration_timeout: deadlines.enumeration,
        volumes,
        clock,
        probe: Probe {
            workers,
            timeout: deadlines.probe,
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
    enumeration: EnumerationWorker,
    enumeration_timeout: Duration,
    volumes: Arc<dyn VolumePort>,
    clock: Arc<dyn JobClock>,
    probe: Probe,
}

impl List {
    fn enumerate(&self) -> Result<Box<VolumeEnumeration>, &'static str> {
        let pending = self.enumeration.start().map_err(|error| match error {
            StartError::Busy => "os.call_slow",
            StartError::Shutdown | StartError::SpawnFailed => "os.query_failed",
        })?;
        match pending.wait(self.enumeration_timeout) {
            WaitResult::Ready(value) => Ok(value),
            WaitResult::TimedOut => Err("os.call_slow"),
            WaitResult::Shutdown | WaitResult::Failed => Err("os.query_failed"),
        }
    }
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
        let observation = self.enumerate();
        let observed_at = Timestamp::from_unix_millis(self.clock.now_ms())
            .ok_or_else(|| EngineError::reason("support.internal_error"))?;
        let observation = match observation {
            Ok(value) => value,
            Err(reason) => return Ok(unavailable_list(observed_at, reason)),
        };
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

fn unavailable_list(observed_at: Timestamp, reason: &str) -> TargetList {
    TargetList {
        targets: Vec::new(),
        probe_job_ref: None,
        observed_at,
        unavailable_reason: Some(reason.into()),
    }
}

fn target(mount: VolumeMount, identity_supported: bool) -> Option<AnalysisTarget> {
    let kind = mount.target_kind.map(|kind| match kind {
        VolumeTargetKind::Internal => TargetKind::Internal,
        VolumeTargetKind::External => TargetKind::External,
        VolumeTargetKind::NetworkMount => TargetKind::NetworkMount,
        VolumeTargetKind::FolderMount => TargetKind::FolderMount,
    });
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
mod tests;
