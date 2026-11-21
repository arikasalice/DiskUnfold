//! Read-only volume discovery contracts.
use crate::{
    OsPath,
    ports::{PortError, PortErrorKind},
};

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

/// The place of a mount in the macOS root volume group (F01 #263).
///
/// Only the OS establishes it: the data volume carries `MNT_EXT_ROOT_DATA_VOL`
/// and the system volume is mounted at `/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeGroupRole {
    System,
    Data,
}

/// What a system volume next to the root volume group is for (F03 IF-03, #263).
///
/// A display classification decided by the adapter; a screen never derives it from
/// a path. `Other` is a system volume without a known purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemVolumeRole {
    Vm,
    Preboot,
    Recovery,
    Update,
    Other,
}

/// A mount observation, retaining the OS path bytes without loss.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VolumeMount {
    pub mount_path: Vec<u8>,
    /// Human-readable display text; never reopen or compare a path from it.
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
    /// Whether the OS file manager lists this mount (F01 #263). `None` is no
    /// evidence, and such a mount is listed.
    pub browsable: Option<bool>,
    /// Whether the mount is local storage (F01 #266). A mount that is known not to
    /// be local is not opened before the user selects it; `None` keeps probing it.
    pub local: Option<bool>,
    /// Capacity reported with the listing itself, read without opening the mount.
    pub listed_capacity: Option<VolumeCapacity>,
    /// The mount's place in the root volume group; `None` outside it or without evidence.
    pub group_role: Option<VolumeGroupRole>,
    /// The purpose of a system volume (F03 IF-03); `None` for other mounts.
    pub system_role: Option<SystemVolumeRole>,
    /// Opaque identity of the storage container whose space the mount shares, such
    /// as an APFS container. Mounts with equal values share free space. `None` is no
    /// evidence and never matches another mount.
    pub container: Option<Vec<u8>>,
    /// Bytes the OS reports as used by this volume alone, read with the listing.
    pub used_bytes: Option<u64>,
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

/// A firmlink of the root volume group whose two ends were confirmed to be one
/// directory: opening `source` and `target` gave the same device and inode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmedFirmlink {
    /// Absolute path of the firmlink on the system volume, such as `/Users`.
    pub source: Vec<u8>,
    /// Absolute path of the same directory below the data mount, such as
    /// `/System/Volumes/Data/Users`.
    pub target: Vec<u8>,
    pub device: u64,
    pub inode: u64,
}

/// The macOS root volume group as confirmed for one scan (F01 #263).
///
/// Present only when all three conditions hold: exactly one mount carries
/// `MNT_EXT_ROOT_DATA_VOL` and shares the device of `/`, `/usr/share/firmlinks`
/// was read, and every listed pair was confirmed as one directory. Any mismatch or
/// failed read yields no group. It is observed again for every scan and never cached,
/// and no user file is opened to establish it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootVolumeGroup {
    /// Mount path of the data volume, such as `/System/Volumes/Data`.
    pub data_mount: Vec<u8>,
    /// The device that `/` and the data volume both report.
    pub device: u64,
    /// Confirmed firmlinks, sorted by `source` bytes.
    pub firmlinks: Vec<ConfirmedFirmlink>,
}

pub trait VolumePort: Send + Sync {
    fn enumerate(&self) -> VolumeEnumeration;
    /// Confirms the root volume group now; `None` when there is none or it cannot
    /// be confirmed. Adapters without volume groups keep this default.
    fn root_volume_group(&self) -> Option<RootVolumeGroup> {
        None
    }
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

/// One currently confirmed entry for a saved volume identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentVolumeEntry {
    pub path: OsPath,
    /// The provider confirmed whether this is the saved entry.
    pub same_entry: bool,
}

/// Complete current evidence for one recognized, saved volume identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentVolumeObservation {
    /// Kind independently recognized by the provider, for identity comparisons.
    pub identity_kind: &'static [u8],
    /// All verified entries. Empty requires confirmed absence of the saved identity.
    pub entries: Vec<RecentVolumeEntry>,
}

/// Supplies volume evidence for recent-scope review without reopening a saved scope.
pub trait RecentVolumePort: Send + Sync {
    /// Recognizes the saved identity kind and obtains its complete current entry set.
    /// `None` means the kind, identity, entry comparison or completeness is unverified.
    /// Call the checkpoint between OS calls and discard an incomplete observation.
    fn recent_volume(
        &self,
        saved_id: &[u8],
        saved_entry: &OsPath,
        continue_work: &dyn Fn() -> bool,
    ) -> Option<RecentVolumeObservation>;
}
