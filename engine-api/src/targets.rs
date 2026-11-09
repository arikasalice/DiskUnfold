//! Expose only target kinds supported by the supplied volume observations.
use std::sync::Arc;

use du_core_contracts::{
    OsPath, PathBytes, PathEncoding, Timestamp,
    analysis_ipc::{
        AnalysisTarget, IdentitySource, TargetAccess, TargetKind, TargetList, TargetListRequest,
    },
    volume::{VolumeMount, VolumePort, VolumeTargetKind},
};
use du_job_runtime::JobClock;

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

pub(crate) fn register(
    builder: &mut EngineBuilder,
    volumes: Arc<dyn VolumePort>,
    clock: Arc<dyn JobClock>,
) {
    builder.register(List { volumes, clock });
}

struct List {
    volumes: Arc<dyn VolumePort>,
    clock: Arc<dyn JobClock>,
}

impl UseCase for List {
    const NAME: &'static str = "target.list";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = TargetListRequest;
    type Response = TargetList;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        let observation = self.volumes.enumerate();
        let (targets, unavailable_reason) = match observation.mounts {
            Ok(mounts) => {
                let mut targets: Vec<_> = mounts.into_iter().filter_map(target).collect();
                targets.sort_by(|a, b| a.path.display.cmp(&b.path.display));
                targets.dedup_by(|a, b| a.path == b.path);
                let reason = targets.is_empty().then(|| "os.unsupported".to_owned());
                (targets, reason)
            }
            Err(error) => (Vec::new(), Some(error.reason_key)),
        };
        let observed_at = Timestamp::from_unix_millis(self.clock.now_ms())
            .ok_or_else(|| EngineError::reason("support.internal_error"))?;
        Ok(TargetList {
            targets,
            probe_job_ref: None,
            observed_at,
            unavailable_reason,
        })
    }
}

fn target(mount: VolumeMount) -> Option<AnalysisTarget> {
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
