//! Read-only volume discovery contracts.
use crate::ports::{PortError, PortErrorKind};

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

/// Role observed from a filesystem type with an explicit OS mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeRole {
    VirtualFs,
    MemoryTemp,
}

/// The target kind established by an operating-system observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeTargetKind {
    Internal,
    External,
    NetworkMount,
    FolderMount,
}

/// A mount observation, retaining the OS path bytes without loss.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeMount {
    pub mount_path: Vec<u8>,
    pub display_path: String,
    pub filesystem: Vec<u8>,
    /// mountinfo source, which may be a device, network source, or non-path label.
    pub mount_source: Vec<u8>,
    /// None means the available observation does not establish a role.
    pub role: Option<VolumeRole>,
    pub topology: Option<MountTopology>,
    /// None when the operating system did not establish the target kind.
    pub target_kind: Option<VolumeTargetKind>,
    /// Confirmed OS volume identity, never a path-derived substitute. Windows
    /// volume GUID names are encoded as UTF-16LE without a terminating NUL.
    pub volume_id: Option<Vec<u8>>,
}

/// One observation: a failed enumeration is not an empty successful list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeEnumeration {
    pub mounts: Result<Vec<VolumeMount>, PortError>,
    pub capabilities: VolumeCapabilities,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeCapacity {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// A successful access check and the capacity observation, if requested.
pub struct VolumeProbe {
    pub capacity: Option<Result<VolumeCapacity, PortError>>,
}

pub trait VolumePort: Send + Sync {
    fn enumerate(&self) -> VolumeEnumeration;
    /// Verify read and search access to a directory without listing its entries.
    fn path_access(&self, path: &[u8]) -> Result<(), PortError>;
    /// Read current capacity for the requested directory, without caching observations.
    fn capacity(&self, path: &[u8]) -> Result<VolumeCapacity, PortError>;

    /// Checks access and, when allowed by the caller's checkpoint, reads capacity.
    ///
    /// A supplied identity must be verified against the volume being observed.
    /// Adapters that cannot verify it must decline the observation.
    fn observe(
        &self,
        path: &[u8],
        expected_volume: Option<&[u8]>,
        should_read_capacity: &dyn Fn() -> bool,
    ) -> Result<VolumeProbe, PortError> {
        if expected_volume.is_some() {
            return Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            });
        }
        self.path_access(path)?;
        Ok(VolumeProbe {
            capacity: should_read_capacity().then(|| self.capacity(path)),
        })
    }
}
