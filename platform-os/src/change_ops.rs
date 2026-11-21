//! Read-only F11 identity observations and the unsupported trash adapter.
//!
//! Design items: FR2.HD1.app-host.DD2, FR3.HD1.app-host.DD2,
//! FR6.HD1.app-host.DD2, FR14.HD1.app-host.DD1, AFR16.platform-os.ADD1.
use du_core_contracts::{
    OsPath,
    platform_os_change::{
        CHANGE_TRASH_IDENTITY_BOUND, ChangeIdentity, ChangeItemObservation, ChangeOpsPort,
        ItemIdentityPort, TrashMoveOutcome, TrashOutcomeQuery,
    },
    ports::PortError,
    volume::{Capability, CapabilityState},
};
const OS_UNSUPPORTED: &str = "os.unsupported";

/// Production F11 adapter. Trash moves remain unsupported on every volume.
///
/// The adapter exposes native no-follow identity reads. Its trash operation
/// never invokes an OS change API until a supported identity-bound mechanism
/// has passed the required platform verification.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemChangeOps;

impl ItemIdentityPort for SystemChangeOps {
    fn lookup_no_follow(&self, path: &OsPath) -> Result<ChangeItemObservation, PortError> {
        // shortcut: route through the OS call watch frame (os.call_slow) once platform-os has one (architecture DLD 2.17).
        lookup_platform(path)
    }
}

impl ChangeOpsPort for SystemChangeOps {
    fn capability(&self, _volume_id: &[u8], capability: &str) -> Capability {
        if capability == CHANGE_TRASH_IDENTITY_BOUND {
            Capability {
                state: CapabilityState::Unsupported,
                reason_key: Some(OS_UNSUPPORTED),
            }
        } else {
            Capability {
                state: CapabilityState::NotApplicable,
                reason_key: None,
            }
        }
    }

    fn trash_identity_bound(&self, _path: &OsPath, _expected: &ChangeIdentity) -> TrashMoveOutcome {
        TrashMoveOutcome::Refused(du_core_contracts::platform_os_change::TrashRefusal::Unsupported)
    }

    fn query_trash_outcome(
        &self,
        original_path: &OsPath,
        expected: &ChangeIdentity,
        _prior_location: Option<&du_core_contracts::platform_os_change::TrashLocationRef>,
    ) -> Result<TrashOutcomeQuery, PortError> {
        Ok(match self.lookup_no_follow(original_path) {
            Ok(observed) if observed.identity == *expected => TrashOutcomeQuery::NotMoved,
            _ => TrashOutcomeQuery::OutcomeUnknown,
        })
    }
}

#[cfg(target_os = "windows")]
fn lookup_platform(path: &OsPath) -> Result<ChangeItemObservation, PortError> {
    crate::windows_observation::lookup_change_item(path)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn lookup_platform(path: &OsPath) -> Result<ChangeItemObservation, PortError> {
    crate::unix_change::lookup_change_item(path)
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn lookup_platform(_path: &OsPath) -> Result<ChangeItemObservation, PortError> {
    Err(PortError {
        kind: du_core_contracts::ports::PortErrorKind::Unsupported,
        reason_key: OS_UNSUPPORTED.into(),
        os_code: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{PathBytes, PathEncoding, platform_os_change::TrashRefusal};

    fn path() -> OsPath {
        OsPath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/unused"),
            display: "/unused".into(),
            lossy: false,
        }
    }

    #[test]
    fn trash_capability_is_unsupported_for_known_and_unknown_volumes() {
        let adapter = SystemChangeOps;
        for volume in [&b"volume"[..], &b""[..]] {
            assert_eq!(
                adapter.capability(volume, CHANGE_TRASH_IDENTITY_BOUND),
                Capability {
                    state: CapabilityState::Unsupported,
                    reason_key: Some(OS_UNSUPPORTED),
                }
            );
        }
        assert_eq!(
            adapter.capability(b"volume", "change.other"),
            Capability {
                state: CapabilityState::NotApplicable,
                reason_key: None,
            }
        );
    }

    #[test]
    fn trash_is_refused_before_path_or_identity_is_observed() {
        assert_eq!(
            SystemChangeOps.trash_identity_bound(
                &path(),
                &ChangeIdentity {
                    volume_id: Vec::new(),
                    file_id: Vec::new(),
                    change_token: None,
                },
            ),
            TrashMoveOutcome::Refused(TrashRefusal::Unsupported)
        );
    }
}
