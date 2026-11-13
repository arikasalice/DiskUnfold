use du_core_contracts::ports::{PortError, PortErrorKind};
use du_core_contracts::volume::{
    Capability, CapabilityState, VolumeCapacity, VolumeEnumeration, VolumePort,
};

/// An observed entry point. The port currently supplies neither target kind nor identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub path: Vec<u8>,
    pub display_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    pub targets: Vec<Target>,
    pub unavailable_reason: Option<String>,
}

/// Convert a volume observation without probing paths or enumerating file entries.
/// Raw byte ordering is deterministic; it is not a substitute for an OS comparison rule.
pub fn discover(observation: VolumeEnumeration) -> Discovery {
    if observation.capabilities.enumerate.state == CapabilityState::Unsupported {
        return unavailable(&observation.capabilities.enumerate);
    }
    let mounts = match observation.mounts {
        Ok(mounts) => mounts,
        Err(error) => {
            return Discovery {
                targets: Vec::new(),
                unavailable_reason: Some(error.reason_key),
            };
        }
    };
    let mut targets: Vec<_> = mounts
        .into_iter()
        .map(|mount| Target {
            path: mount.mount_path,
            display_path: mount.display_path,
        })
        .collect();
    targets.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.display_path.cmp(&b.display_path))
    });
    // A raw entry path is the only target key available, not a volume identity.
    targets.dedup_by(|a, b| a.path == b.path);
    Discovery {
        targets,
        unavailable_reason: None,
    }
}

fn unavailable(capability: &Capability) -> Discovery {
    Discovery {
        targets: Vec::new(),
        unavailable_reason: Some(
            capability
                .reason_key
                .unwrap_or("os.query_failed")
                .to_owned(),
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessStatus {
    Accessible,
    Denied,
    Disconnected,
    Missing,
    Unresponsive,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessResult {
    pub status: AccessStatus,
    pub reason: Option<String>,
}

/// Timeout is supplied by the caller responsible for enforcing the deadline.
pub enum AccessObservation {
    Completed(Result<(), PortError>),
    TimedOut,
}

pub fn access_result(observation: AccessObservation) -> AccessResult {
    match observation {
        AccessObservation::Completed(Ok(())) => AccessResult {
            status: AccessStatus::Accessible,
            reason: None,
        },
        AccessObservation::TimedOut => access(AccessStatus::Unresponsive, "os.call_slow"),
        AccessObservation::Completed(Err(error)) => match error.kind {
            PortErrorKind::PermissionDenied => access(AccessStatus::Denied, "scan.path_denied"),
            PortErrorKind::NotFound => access(AccessStatus::Missing, "scan.path_vanished"),
            PortErrorKind::IdentityChanged => {
                access(AccessStatus::Disconnected, "scan.disconnected")
            }
            PortErrorKind::Io if error.reason_key == "scan.disconnected" => {
                access(AccessStatus::Disconnected, "scan.disconnected")
            }
            _ => AccessResult {
                status: AccessStatus::Unknown,
                reason: Some(error.reason_key),
            },
        },
    }
}

fn access(status: AccessStatus, reason: &str) -> AccessResult {
    AccessResult {
        status,
        reason: Some(reason.to_owned()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeResult {
    pub access: AccessResult,
    pub capacity: Result<VolumeCapacity, String>,
    /// The current port exposes no verified alias identity.
    pub aggregation_uncertain: bool,
    /// UTC epoch milliseconds when the probe finished, if already observed.
    pub observed_at: Option<i64>,
}

/// Run one probe on the caller's worker. This function does not impose a deadline.
/// The port cannot establish that separate successful access and capacity calls
/// observed the same mounted volume. The clock is read after the port calls.
pub fn probe_one(
    port: &dyn VolumePort,
    target: &Target,
    capacity: &Capability,
    clock: impl FnOnce() -> i64,
) -> ProbeResult {
    let access = access_result(AccessObservation::Completed(port.path_access(&target.path)));
    let capacity = if access.status != AccessStatus::Accessible {
        Err(access
            .reason
            .clone()
            .unwrap_or_else(|| "os.query_failed".to_owned()))
    } else if capacity.state == CapabilityState::Unsupported {
        Err(capacity.reason_key.unwrap_or("os.query_failed").to_owned())
    } else {
        port.capacity(&target.path)
            .map_err(|error| error.reason_key)
    };
    ProbeResult {
        access,
        capacity,
        aggregation_uncertain: true,
        observed_at: Some(clock()),
    }
}
