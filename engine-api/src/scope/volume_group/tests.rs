use super::*;
use du_core_contracts::{
    PathBytes,
    analysis_ipc::{ScopeBoundary, ScopeSource},
    ports::{PortError, PortErrorKind},
    volume::{
        Capability, CapabilityState, RootVolumeGroup, VolumeCapabilities, VolumeCapacity,
        VolumeEnumeration,
    },
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Volumes {
    calls: AtomicUsize,
    observation: RootVolumeGroupObservation,
}

impl VolumePort for Volumes {
    fn enumerate(&self) -> VolumeEnumeration {
        let unsupported = Capability {
            state: CapabilityState::Unsupported,
            reason_key: Some("os.unsupported"),
        };
        VolumeEnumeration {
            mounts: Ok(Vec::new()),
            capabilities: VolumeCapabilities {
                enumerate: unsupported.clone(),
                identity: unsupported.clone(),
                capacity: unsupported.clone(),
                path_access: unsupported.clone(),
                path_compare_rule: unsupported.clone(),
                mount_boundary: unsupported.clone(),
                alias: unsupported,
            },
        }
    }

    fn root_volume_group(&self) -> RootVolumeGroupObservation {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.observation.clone()
    }

    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        Ok(())
    }

    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}

fn draft(raw: &[u8]) -> ScopeDraft {
    ScopeDraft {
        includes: vec![OsPath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(raw),
            display: String::from_utf8_lossy(raw).into_owned(),
            lossy: false,
        }],
        exclusions: Vec::new(),
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: Vec::new(),
        source: ScopeSource::Gui,
    }
}

fn group() -> RootVolumeGroup {
    RootVolumeGroup {
        data_mount: b"/System/Volumes/Data".to_vec(),
        device: 1,
        data_inode: 2,
        firmlinks: Vec::new(),
    }
}

#[test]
fn only_a_unix_root_review_queries_the_volume_port_once() {
    let volumes = Volumes {
        calls: AtomicUsize::new(0),
        observation: RootVolumeGroupObservation::Unconfirmed,
    };
    assert_eq!(
        observe_group(&draft(b"/"), Some(&volumes)),
        RootVolumeGroupObservation::Unconfirmed
    );
    assert_eq!(volumes.calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        observe_group(&draft(b"/Users"), Some(&volumes)),
        RootVolumeGroupObservation::NotApplicable
    );
    assert_eq!(volumes.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn notices_and_gui_projection_cover_all_group_observations() {
    let confirmed = RootVolumeGroupObservation::Confirmed(group());
    assert_eq!(
        group_notice(&confirmed),
        Some(ScopeVolumeGroup::NOTICE_MERGED)
    );
    assert_eq!(
        group_wire(&confirmed).and_then(|wire| wire.data_mount.raw.bytes().ok()),
        Some(b"/System/Volumes/Data".to_vec())
    );
    assert_eq!(
        group_notice(&RootVolumeGroupObservation::Unconfirmed),
        Some(ScopeVolumeGroup::NOTICE_UNVERIFIED)
    );
    assert!(group_wire(&RootVolumeGroupObservation::Unconfirmed).is_none());
    assert_eq!(
        group_notice(&RootVolumeGroupObservation::NotApplicable),
        None
    );
    assert!(group_wire(&RootVolumeGroupObservation::NotApplicable).is_none());
}
