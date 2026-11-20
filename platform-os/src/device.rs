//! Device identity observations for an already-open volume.
use du_core_contracts::{
    scan_observation::{DeviceKind, VolumeDevice},
    volume::{Capability, CapabilityState},
};
use std::fmt;

#[cfg(any(target_os = "linux", test))]
pub mod linux;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod macos;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows;

pub const OS_QUERY_FAILED: &str = "os.query_failed";
pub const OS_UNSUPPORTED: &str = "os.unsupported";
pub const OS_DEVICE_NETWORK: &str = "os.device_network";
pub const OS_DEVICE_MULTIPLE_DISKS: &str = "os.device_multiple_disks";

/// Device observations for an already-open volume.
#[derive(Clone, Eq, PartialEq)]
pub struct DeviceProbe {
    pub kind: DeviceKind,
    pub disk_key: Option<Vec<u8>>,
    pub reason_key: Option<&'static str>,
}

/// Maps one OS probe into the shared scan observation contract.
pub(crate) fn map_probe(probe: DeviceProbe) -> VolumeDevice {
    let reason = match probe.reason_key {
        Some(reason) => reason,
        None => OS_QUERY_FAILED,
    };
    let kind_capability = if probe.kind == DeviceKind::Unknown {
        Capability {
            state: CapabilityState::Unsupported,
            reason_key: Some(reason),
        }
    } else {
        Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        }
    };
    let disk_capability = if probe.disk_key.is_some() {
        Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        }
    } else {
        Capability {
            state: CapabilityState::Unsupported,
            reason_key: Some(reason),
        }
    };
    VolumeDevice {
        kind: probe.kind,
        kind_capability,
        disk_key: probe.disk_key,
        disk_capability,
    }
}

impl fmt::Debug for DeviceProbe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct DiskKey(bool);

        impl fmt::Debug for DiskKey {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                if self.0 {
                    formatter.write_str("Some(<redacted>)")
                } else {
                    formatter.write_str("None")
                }
            }
        }

        formatter
            .debug_struct("DeviceProbe")
            .field("kind", &self.kind)
            .field("disk_key", &DiskKey(self.disk_key.is_some()))
            .field("reason_key", &self.reason_key)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn debug_redacts_disk_key_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let probe = DeviceProbe {
            kind: DeviceKind::Ssd,
            disk_key: Some(b"private-disk-id".to_vec()),
            reason_key: None,
        };
        let formatted = format!("{probe:?}");
        if formatted != "DeviceProbe { kind: Ssd, disk_key: Some(<redacted>), reason_key: None }" {
            return Err(io::Error::other("device key was not redacted in Debug output").into());
        }

        let absent = DeviceProbe {
            kind: DeviceKind::Unknown,
            disk_key: None,
            reason_key: Some(OS_QUERY_FAILED),
        };
        if !format!("{absent:?}").contains("disk_key: None") {
            return Err(io::Error::other("absent device key was not formatted as None").into());
        }
        Ok(())
    }

    #[test]
    fn maps_probe_fields_to_capabilities() -> Result<(), Box<dyn std::error::Error>> {
        let detected = map_probe(DeviceProbe {
            kind: DeviceKind::Ssd,
            disk_key: Some(b"win:disk:1".to_vec()),
            reason_key: None,
        });
        if detected.kind != DeviceKind::Ssd
            || detected.kind_capability.state != CapabilityState::Supported
            || detected.kind_capability.reason_key.is_some()
            || detected.disk_capability.state != CapabilityState::Supported
            || detected.disk_capability.reason_key.is_some()
        {
            return Err(
                io::Error::other("detected probe capabilities were mapped incorrectly").into(),
            );
        }

        let partial = map_probe(DeviceProbe {
            kind: DeviceKind::Unknown,
            disk_key: Some(b"win:disk:2".to_vec()),
            reason_key: None,
        });
        if partial.kind_capability.state != CapabilityState::Unsupported
            || partial.kind_capability.reason_key != Some(OS_QUERY_FAILED)
            || partial.disk_capability.state != CapabilityState::Supported
        {
            return Err(io::Error::other("unknown kind capability was mapped incorrectly").into());
        }

        let missing_key = map_probe(DeviceProbe {
            kind: DeviceKind::Hdd,
            disk_key: None,
            reason_key: Some(OS_UNSUPPORTED),
        });
        if missing_key.kind_capability.state != CapabilityState::Supported
            || missing_key.disk_capability.state != CapabilityState::Unsupported
            || missing_key.disk_capability.reason_key != Some(OS_UNSUPPORTED)
        {
            return Err(io::Error::other("missing key capability was mapped incorrectly").into());
        }
        Ok(())
    }
}
