use super::*;
use du_core_contracts::ports::PortErrorKind;

struct CapacityError(PortError);

impl VolumePort for CapacityError {
    fn enumerate(&self) -> VolumeEnumeration {
        unreachable!()
    }

    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        Ok(())
    }

    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Err(self.0.clone())
    }
}

#[test]
fn confirmed_replacement_supersedes_earlier_access_but_uncertainty_does_not() {
    let context = Context {
        cancelled: AtomicBool::new(false),
        events: Arc::new(Mutex::new(Vec::new())),
    };
    let supported = Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    };
    for (kind, reason, access) in [
        (
            PortErrorKind::IdentityChanged,
            "scan.disconnected",
            AccessStatus::Disconnected,
        ),
        (
            PortErrorKind::PermissionDenied,
            "os.query_failed",
            AccessStatus::Accessible,
        ),
    ] {
        let observed = observation(
            &CapacityError(PortError {
                kind,
                reason_key: reason.into(),
                os_code: None,
            }),
            &path("drive"),
            &supported,
            &supported,
            &context,
            Instant::now() + Duration::from_secs(5),
        );
        assert_eq!(observed.access.status, access);
        assert_eq!(observed.capacity, Err(reason.into()));
    }
}
