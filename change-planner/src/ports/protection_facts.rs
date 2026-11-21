/// Shared fact contract; unknown values retain their stable reason key.
pub type Fact<T> = du_core_contracts::protection_facts::ProtectionFact<T>;

/// Result of verifying the current file identity against the pinned identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityObservation {
    /// The current identity matches the pinned identity.
    Matches,
    /// The current identity differs from the pinned identity.
    Mismatch,
}

/// Whether link and mount traversal stays inside the expected volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkBoundary {
    /// All observed hops remain within the expected volume.
    WithinVolume,
    /// A hop crosses the expected volume boundary.
    OutsideVolume,
}

/// Volume class used by the protection evaluation table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionVolumeRole {
    /// Ordinary storage.
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

/// Kind key from the shared platform known-location contract.
pub type KnownLocationKind = du_core_contracts::protection_facts::KnownLocationId;

/// Known-location lookup result for the target path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownLocation {
    /// The path is a listed volume, profile, or standard top-level root.
    Root,
    /// The path is below a listed root of this kind.
    Under(KnownLocationKind),
    /// The path is outside the known-location table.
    NotListed,
}

/// Action assigned to a classification by the verified shared ruleset.
pub type ProtectionAction = du_core_contracts::ruleset::RulesetAction;

/// Release state assigned to a classification by the verified ruleset.
pub type ClassReleaseState = du_core_contracts::ruleset::ClassReleaseState;

/// A guard result from the age, permission, or disk-backed check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardResult {
    /// The guard permits the operation.
    Clear,
    /// The guard protects the target.
    Protected,
}

/// Result of checking the selected target's descendants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescendantResult {
    /// All analyzed descendants permit the operation.
    Clear,
    /// At least one descendant denies the operation.
    Protected,
    /// Some descendants were not analyzed.
    Incomplete,
}
