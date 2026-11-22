//! Read-only facts used to evaluate protection for one observed filesystem item.

use std::fmt;

use crate::{OsPath, Timestamp, result::ResultItemKind};

/// Standard Unknown reason keys for protection facts.
pub mod fact_reason {
    /// The provider completed only part of the required lookup.
    pub const PARTIAL: &str = "protect.fact.partial";
    /// The provider cannot establish this fact on the current platform.
    pub const UNSUPPORTED: &str = "protect.fact.unsupported";
    /// The operating system denied the read-only access check.
    pub const ACCESS_DENIED: &str = "protect.fact.access_denied";
    /// The bounded lookup exceeded its time limit.
    pub const TIMED_OUT: &str = "protect.fact.timed_out";
    /// The lookup was cancelled before it completed.
    pub const CANCELLED: &str = "protect.fact.cancelled";
    /// The pinned classification or known-location table is stale or unsupported.
    pub const STALE_TABLE: &str = "protect.fact.stale_table";
    /// The provider failed unexpectedly or could not classify the failure.
    pub const PROVIDER_ERROR: &str = "protect.fact.provider_error";
}

/// Filesystem operation whose effective permission is being checked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProtectionOperation {
    /// Permanently delete the item.
    Delete,
    /// Move the item on its current volume.
    Move,
    /// Move the item into the operating system trash.
    Trash,
}

impl fmt::Debug for ProtectionOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProtectionOperation([REDACTED])")
    }
}

/// A protection fact that is either established or unknown for a stable reason.
///
/// Debug output deliberately hides known values because they may contain paths
/// or native file identifiers; unknown values expose only their reason key.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProtectionFact<T> {
    /// The owning adapter established this fact.
    Known(T),
    /// The fact could not be established for this reason key.
    Unknown(&'static str),
}

impl<T> fmt::Debug for ProtectionFact<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Known(_) => formatter.write_str("Known([REDACTED])"),
            Self::Unknown(reason) => formatter.debug_tuple("Unknown").field(reason).finish(),
        }
    }
}

/// Native identity observed from the current filesystem object.
#[derive(Clone, PartialEq, Eq)]
pub struct ObservedFileIdentity {
    /// Native volume identifier.
    pub volume_id: Vec<u8>,
    /// Native file identifier.
    pub file_id: Vec<u8>,
    /// Windows volume serial, when returned alongside its volume GUID.
    pub volume_serial: Option<u64>,
    /// Linux mount ID, when the kernel exposes one.
    pub mount_id: Option<Vec<u8>>,
}

impl fmt::Debug for ObservedFileIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ObservedFileIdentity([REDACTED])")
    }
}

/// Input copied from a pinned analysis observation for a protection lookup.
#[derive(Clone, PartialEq, Eq)]
pub struct ObservedProtectionItem {
    /// Native path observed by the scan.
    pub path: OsPath,
    /// Root path used by the pinned scan, when available.
    pub scan_root_path: Option<OsPath>,
    /// Item kind recorded by the pinned scan.
    pub kind: ResultItemKind,
    /// Volume identity observed by the scan.
    pub volume_id: Vec<u8>,
    /// File identity observed by the scan, if available.
    pub identity: Option<ObservedFileIdentity>,
    /// Modification time observed by the scan, if available.
    pub modified_at: Option<Timestamp>,
}

impl fmt::Debug for ObservedProtectionItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ObservedProtectionItem([REDACTED])")
    }
}

/// Whether a native path or mount remains on the observed volume.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinkBoundaryFact {
    /// All resolved links and mounts stay on the observed volume.
    WithinVolume,
    /// A resolved link or mount crosses the observed volume boundary.
    OutsideVolume,
}

impl fmt::Debug for LinkBoundaryFact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LinkBoundaryFact([REDACTED])")
    }
}

/// Role of the filesystem containing the observed item.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VolumeRoleFact {
    /// Ordinary persistent storage.
    Ordinary,
    /// Operating-system-only storage.
    OsOnly,
    /// A system partition.
    SystemPartition,
    /// A virtual filesystem.
    VirtualFilesystem,
    /// Memory-backed temporary storage.
    MemoryTemporary,
}

impl fmt::Debug for VolumeRoleFact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VolumeRoleFact([REDACTED])")
    }
}

/// F16 known-location key supported by the v1 platform lookup.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KnownLocationId {
    /// `loc.app_data`.
    AppData,
    /// `loc.user_cache`.
    UserCache,
    /// `loc.temp`.
    Temp,
    /// `loc.downloads`.
    Downloads,
    /// `loc.memory_managed`.
    MemoryManaged,
    /// `loc.trash_store`.
    TrashStore,
    /// `loc.user_home`.
    UserHome,
}

impl fmt::Debug for KnownLocationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KnownLocationId([REDACTED])")
    }
}

impl KnownLocationId {
    /// Returns the F16 key associated with this location.
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::AppData => "loc.app_data",
            Self::UserCache => "loc.user_cache",
            Self::Temp => "loc.temp",
            Self::Downloads => "loc.downloads",
            Self::MemoryManaged => "loc.memory_managed",
            Self::TrashStore => "loc.trash_store",
            Self::UserHome => "loc.user_home",
        }
    }
}

/// Result of a complete lookup against the pinned F16 location table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KnownLocationFact {
    /// The item is at or below one listed location.
    Listed(KnownLocationId),
    /// The completed lookup found no listed location.
    NotListed,
    /// The lookup did not complete or the table version is unsupported.
    Unknown(&'static str),
}

impl fmt::Debug for KnownLocationFact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Listed(_) => formatter.write_str("Listed([REDACTED])"),
            Self::NotListed => formatter.write_str("NotListed"),
            Self::Unknown(reason) => formatter.debug_tuple("Unknown").field(reason).finish(),
        }
    }
}

/// Whether a required safety guard permits an operation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProtectionGuardFact {
    /// The guard permits the operation.
    Clear,
    /// The guard protects the item.
    Protected,
}

impl fmt::Debug for ProtectionGuardFact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProtectionGuardFact([REDACTED])")
    }
}

/// Read-only source of native protection facts.
pub trait ProtectionFactsPort: Send + Sync {
    /// Reads the current file identity without changing the item.
    fn identity(&self, item: &ObservedProtectionItem) -> ProtectionFact<ObservedFileIdentity>;

    /// Checks resolved links and mounts against the pinned observed volume.
    fn link_boundary(&self, item: &ObservedProtectionItem) -> ProtectionFact<LinkBoundaryFact>;

    /// Classifies the containing filesystem when its role can be established.
    fn volume_role(&self, item: &ObservedProtectionItem) -> ProtectionFact<VolumeRoleFact>;

    /// Looks up the item against the pinned known-location table version.
    fn known_location(
        &self,
        item: &ObservedProtectionItem,
        table_version: crate::ruleset::KnownLocationTableVersion,
    ) -> KnownLocationFact;

    /// Reports whether the item is an operating-system component when provable.
    fn os_component(&self, item: &ObservedProtectionItem) -> ProtectionFact<bool>;

    /// Checks the age guard using the observed modification time.
    fn guard_age(&self, item: &ObservedProtectionItem) -> ProtectionFact<ProtectionGuardFact>;

    /// Checks whether the current user can perform the requested file operation.
    fn guard_permission(
        &self,
        item: &ObservedProtectionItem,
        operation: ProtectionOperation,
    ) -> ProtectionFact<ProtectionGuardFact>;

    /// Checks whether the containing filesystem is proven to be disk-backed.
    fn guard_disk_backed(
        &self,
        item: &ObservedProtectionItem,
    ) -> ProtectionFact<ProtectionGuardFact>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PathBytes, PathEncoding};

    #[test]
    fn debug_output_redacts_known_values_and_keeps_only_unknown_reason_keys() {
        let identity = ObservedFileIdentity {
            volume_id: b"private-volume".to_vec(),
            file_id: b"private-file".to_vec(),
            volume_serial: Some(42),
            mount_id: Some(b"private-mount".to_vec()),
        };
        let fact = ProtectionFact::Known(identity.clone());
        let item = ObservedProtectionItem {
            path: OsPath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/private/home/file"),
                display: "/private/home/file".into(),
                lossy: false,
            },
            scan_root_path: None,
            kind: ResultItemKind::File,
            volume_id: b"private-volume".to_vec(),
            identity: Some(identity),
            modified_at: None,
        };
        let debug = format!(
            "{fact:?} {item:?} {:?}",
            KnownLocationFact::Listed(KnownLocationId::UserHome)
        );
        assert!(!debug.contains("private"));
        assert!(!debug.contains("42"));
        assert!(
            format!(
                "{:?}",
                ProtectionFact::<u8>::Unknown(fact_reason::ACCESS_DENIED)
            )
            .contains(fact_reason::ACCESS_DENIED)
        );
    }
}
