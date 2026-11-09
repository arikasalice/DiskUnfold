//! Read-only volume discovery contracts.
use crate::ports::PortError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityState {
    Supported,
    Partial,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    pub state: CapabilityState,
    pub reason_key: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeCapabilities {
    pub enumerate: Capability,
    pub identity: Capability,
    pub capacity: Capability,
    pub path_access: Capability,
    pub path_compare_rule: Capability,
    pub mount_boundary: Capability,
    pub alias: Capability,
}

/// Linux mountinfo topology fields as observed, with escaped paths decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountTopology {
    pub mount_id: Vec<u8>,
    pub parent_id: Vec<u8>,
    pub device_major: Vec<u8>,
    pub device_minor: Vec<u8>,
    pub root: Vec<u8>,
}

/// A mount observation, retaining the OS path bytes without loss.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeMount {
    pub mount_path: Vec<u8>,
    pub display_path: String,
    pub filesystem: Vec<u8>,
    pub topology: Option<MountTopology>,
}

/// One observation: a failed enumeration is not an empty successful list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeEnumeration {
    pub mounts: Result<Vec<VolumeMount>, PortError>,
    pub capabilities: VolumeCapabilities,
}

pub trait VolumePort: Send + Sync {
    fn enumerate(&self) -> VolumeEnumeration;
    /// Verify read and search access to a directory without listing its entries.
    fn path_access(&self, path: &[u8]) -> Result<(), PortError>;
}
