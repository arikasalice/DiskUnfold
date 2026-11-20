//! Device observations made only after a scope root has been verified open.
use du_core_contracts::{
    scan_observation::{DeviceKind, DeviceKindSource, OpenedScopeRoot},
    volume::CapabilityState,
};
use du_scan_engine::scan_job::RootDevice;

pub(super) fn observe(roots: &[OpenedScopeRoot]) -> Vec<(Vec<u8>, RootDevice)> {
    roots
        .iter()
        .map(|root| {
            let observed = root.directory.device();
            let kind_observed = observed.kind != DeviceKind::Unknown
                && observed.kind_capability.state != CapabilityState::Unsupported;
            let disk_key = (observed.disk_capability.state != CapabilityState::Unsupported)
                .then_some(observed.disk_key)
                .flatten()
                .filter(|key| !key.is_empty());
            let device = RootDevice {
                kind: if kind_observed {
                    observed.kind
                } else {
                    DeviceKind::Unknown
                },
                source: if kind_observed {
                    DeviceKindSource::Detected
                } else {
                    DeviceKindSource::Unknown
                },
                disk_key,
            };
            (root.volume_id.clone(), device)
        })
        .collect()
}
