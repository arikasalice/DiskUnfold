//! Read-only device classification attached to an already opened volume.
use crate::volume::{Capability, CapabilityState};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The rotational class of a storage device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    /// Solid-state storage.
    Ssd,
    /// Rotational hard-disk storage.
    Hdd,
    /// Used when the OS cannot establish a device class or sends a newer value.
    #[serde(other)]
    Unknown,
}

/// Where a device classification came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DeviceKindSource {
    /// Reported by the operating system.
    Detected,
    /// Supplied by the user.
    User,
    /// Used when the source is omitted or is not recognized.
    #[serde(other)]
    Unknown,
}

/// Internal reason keys for the optional device observation.
#[doc(hidden)]
pub const OS_QUERY_FAILED: &str = "os.query_failed";
#[doc(hidden)]
pub const OS_UNSUPPORTED: &str = "os.unsupported";
#[doc(hidden)]
pub const OS_DEVICE_NETWORK: &str = "os.device_network";
#[doc(hidden)]
pub const OS_DEVICE_MULTIPLE_DISKS: &str = "os.device_multiple_disks";

/// Device classification and physical disk identity observed for one volume.
#[derive(Clone)]
pub struct VolumeDevice {
    /// Classified device kind; unknown when no class was established.
    pub kind: DeviceKind,
    /// Evidence state and reason for `kind`.
    pub kind_capability: Capability,
    /// Physical disk identity as opaque bytes; compare in process only and never disclose.
    pub disk_key: Option<Vec<u8>>,
    /// Evidence state and reason for `disk_key`.
    pub disk_capability: Capability,
}

impl VolumeDevice {
    /// An unsupported observation for both device class and physical identity.
    pub fn unsupported() -> Self {
        let capability = Capability {
            state: CapabilityState::Unsupported,
            reason_key: Some(OS_UNSUPPORTED),
        };
        Self {
            kind: DeviceKind::Unknown,
            kind_capability: capability.clone(),
            disk_key: None,
            disk_capability: capability,
        }
    }
}

impl fmt::Debug for VolumeDevice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Redacted(bool);
        impl fmt::Debug for Redacted {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                if self.0 {
                    formatter.write_str("Some(<redacted>)")
                } else {
                    formatter.write_str("None")
                }
            }
        }

        formatter
            .debug_struct("VolumeDevice")
            .field("kind", &self.kind)
            .field("kind_capability", &self.kind_capability)
            .field("disk_key", &Redacted(self.disk_key.is_some()))
            .field("disk_capability", &self.disk_capability)
            .finish()
    }
}
