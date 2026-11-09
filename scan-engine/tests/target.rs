use du_core_contracts::ports::{PortError, PortErrorKind};
use du_core_contracts::volume::{
    Capability, CapabilityState, VolumeCapabilities, VolumeCapacity, VolumeEnumeration,
    VolumeMount, VolumePort,
};
use du_scan_engine::target::{
    AccessObservation, AccessStatus, Target, access_result, discover, probe_one,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn capability(state: CapabilityState) -> Capability {
    Capability {
        state,
        reason_key: Some("os.unsupported"),
    }
}
fn caps(state: CapabilityState) -> VolumeCapabilities {
    let c = capability(state);
    VolumeCapabilities {
        enumerate: c.clone(),
        identity: c.clone(),
        capacity: c.clone(),
        path_access: c.clone(),
        path_compare_rule: c.clone(),
        mount_boundary: c.clone(),
        alias: c,
    }
}
fn mount(path: &[u8]) -> VolumeMount {
    VolumeMount {
        mount_path: path.to_vec(),
        display_path: String::from_utf8_lossy(path).into_owned(),
        filesystem: vec![],
        mount_source: vec![],
        role: None,
        topology: None,
    }
}
fn error(kind: PortErrorKind, reason: &str) -> PortError {
    PortError {
        kind,
        reason_key: reason.into(),
        os_code: None,
    }
}

#[test]
fn discovery_is_deterministic_and_preserves_raw_paths() {
    let make = |mounts| VolumeEnumeration {
        mounts: Ok(mounts),
        capabilities: caps(CapabilityState::Partial),
    };
    let a = discover(make(vec![mount(b"/z"), mount(b"/\xff"), mount(b"/a")]));
    let b = discover(make(vec![mount(b"/a"), mount(b"/z"), mount(b"/\xff")]));
    assert_eq!(a, b);
    assert_eq!(a.targets[2].path, b"/\xff");
    assert_eq!(a.unavailable_reason, None);
}

#[test]
fn duplicate_paths_produce_one_deterministic_entry() {
    let mut alternate = mount(b"/same");
    alternate.display_path = "z".into();
    let mut first = mount(b"/same");
    first.display_path = "a".into();
    let make = |mounts| VolumeEnumeration {
        mounts: Ok(mounts),
        capabilities: caps(CapabilityState::Supported),
    };
    let a = discover(make(vec![alternate.clone(), first.clone()]));
    let b = discover(make(vec![first, alternate]));
    assert_eq!(a, b);
    assert_eq!(a.targets.len(), 1);
    assert_eq!(a.targets[0].display_path, "a");
}

#[test]
fn discovery_distinguishes_empty_and_unavailable() {
    let empty = discover(VolumeEnumeration {
        mounts: Ok(vec![]),
        capabilities: caps(CapabilityState::Supported),
    });
    let failed = discover(VolumeEnumeration {
        mounts: Err(error(PortErrorKind::Io, "os.failed")),
        capabilities: caps(CapabilityState::Supported),
    });
    let unsupported = discover(VolumeEnumeration {
        mounts: Ok(vec![mount(b"/")]),
        capabilities: caps(CapabilityState::Unsupported),
    });
    assert_eq!(empty.unavailable_reason, None);
    assert_eq!(failed.unavailable_reason.as_deref(), Some("os.failed"));
    assert_eq!(
        unsupported.unavailable_reason.as_deref(),
        Some("os.unsupported")
    );
    assert!(unsupported.targets.is_empty());
}

#[test]
fn access_table_distinguishes_failures() {
    for (kind, reason, expected, expected_reason) in [
        (
            PortErrorKind::PermissionDenied,
            "native",
            AccessStatus::Denied,
            "scan.path_denied",
        ),
        (
            PortErrorKind::NotFound,
            "native",
            AccessStatus::Missing,
            "scan.path_vanished",
        ),
        (
            PortErrorKind::IdentityChanged,
            "native",
            AccessStatus::Disconnected,
            "scan.disconnected",
        ),
        (
            PortErrorKind::Io,
            "scan.disconnected",
            AccessStatus::Disconnected,
            "scan.disconnected",
        ),
        (PortErrorKind::Io, "os.io", AccessStatus::Unknown, "os.io"),
        (
            PortErrorKind::Unsupported,
            "os.unsupported",
            AccessStatus::Unknown,
            "os.unsupported",
        ),
    ] {
        let result = access_result(AccessObservation::Completed(Err(error(kind, reason))));
        assert_eq!(result.status, expected);
        assert_eq!(result.reason.as_deref(), Some(expected_reason));
    }
    assert_eq!(
        access_result(AccessObservation::TimedOut).status,
        AccessStatus::Unresponsive
    );
    assert_eq!(
        access_result(AccessObservation::Completed(Ok(()))).status,
        AccessStatus::Accessible
    );
}

struct Fake {
    access_calls: AtomicUsize,
    capacity_calls: AtomicUsize,
    failure: Option<PortError>,
}
impl VolumePort for Fake {
    fn enumerate(&self) -> VolumeEnumeration {
        VolumeEnumeration {
            mounts: Ok(vec![mount(b"/x")]),
            capabilities: caps(CapabilityState::Supported),
        }
    }
    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        self.access_calls.fetch_add(1, Ordering::SeqCst);
        self.failure.clone().map_or(Ok(()), Err)
    }
    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        self.capacity_calls.fetch_add(1, Ordering::SeqCst);
        Ok(VolumeCapacity {
            total_bytes: 0,
            available_bytes: u64::MAX,
        })
    }
}

#[test]
fn discovery_has_no_probe_and_success_reads_clock_after_capacity() {
    let port = Fake {
        access_calls: AtomicUsize::new(0),
        capacity_calls: AtomicUsize::new(0),
        failure: None,
    };
    let listing = discover(port.enumerate());
    assert_eq!(port.access_calls.load(Ordering::SeqCst), 0);
    assert_eq!(port.capacity_calls.load(Ordering::SeqCst), 0);
    let target = &listing.targets[0];
    let result = probe_one(
        &port,
        target,
        &capability(CapabilityState::Supported),
        || {
            assert_eq!(port.access_calls.load(Ordering::SeqCst), 1);
            assert_eq!(port.capacity_calls.load(Ordering::SeqCst), 1);
            42
        },
    );
    assert_eq!(result.access.status, AccessStatus::Accessible);
    assert_eq!(
        result.capacity,
        Ok(VolumeCapacity {
            total_bytes: 0,
            available_bytes: u64::MAX
        })
    );
    assert!(result.aggregation_uncertain);
    assert_eq!(result.observed_at, 42);
    let unavailable = probe_one(
        &port,
        &Target {
            path: vec![],
            display_path: String::new(),
        },
        &capability(CapabilityState::Unsupported),
        || 43,
    );
    assert_eq!(unavailable.capacity, Err("os.unsupported".into()));
    assert_eq!(port.capacity_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_access_skips_capacity_and_reads_clock_after_access() {
    for (kind, input, expected) in [
        (
            PortErrorKind::PermissionDenied,
            "native",
            "scan.path_denied",
        ),
        (
            PortErrorKind::IdentityChanged,
            "native",
            "scan.disconnected",
        ),
        (PortErrorKind::NotFound, "native", "scan.path_vanished"),
        (PortErrorKind::Io, "os.io", "os.io"),
    ] {
        let port = Fake {
            access_calls: AtomicUsize::new(0),
            capacity_calls: AtomicUsize::new(0),
            failure: Some(error(kind, input)),
        };
        let target = Target {
            path: b"/x".to_vec(),
            display_path: "/x".into(),
        };
        let result = probe_one(
            &port,
            &target,
            &capability(CapabilityState::Supported),
            || {
                assert_eq!(port.access_calls.load(Ordering::SeqCst), 1);
                assert_eq!(port.capacity_calls.load(Ordering::SeqCst), 0);
                99
            },
        );
        assert_eq!(result.capacity, Err(expected.into()));
        assert_eq!(result.observed_at, 99);
    }
}
