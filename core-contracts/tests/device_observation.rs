use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{AnchoredDirectory, DeviceKind, DirectoryObservations, VolumeDevice},
    volume::{Capability, CapabilityState},
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Directory(AtomicUsize);

impl DirectoryObservations for Directory {
    fn next_batch(
        &mut self,
    ) -> Result<Vec<du_core_contracts::scan_observation::ObservedEntry>, PortError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for Directory {
    fn open_child(
        &self,
        _: &du_core_contracts::scan_observation::ObservedEntry,
    ) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }

    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        None
    }
}

#[test]
fn default_device_is_unknown_and_does_not_touch_the_directory() {
    let directory = Directory(AtomicUsize::new(0));
    let device = directory.device();
    assert_eq!(device.kind, DeviceKind::Unknown);
    assert_eq!(directory.0.load(Ordering::Relaxed), 0);
    for capability in [device.kind_capability, device.disk_capability] {
        assert_eq!(capability.state, CapabilityState::Unsupported);
        assert_eq!(capability.reason_key, Some("os.unsupported"));
    }
}

#[test]
fn volume_device_debug_redacts_disk_key_bytes() {
    let device = VolumeDevice {
        kind: DeviceKind::Ssd,
        kind_capability: Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        },
        disk_key: Some(b"disk-key-secret-value".to_vec()),
        disk_capability: Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        },
    };
    let debug = format!("{device:?}");
    assert!(debug.contains("Some(<redacted>)"));
    assert!(!debug.contains("disk-key-secret-value"));
    assert!(!format!("{:?}", VolumeDevice::unsupported()).contains("Some(<redacted>)"));
}
