/// A fact that can be known or unavailable; unavailable never means false.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fact<T> {
    /// A fact was established by its owning port.
    Known(T),
    /// The fact could not be established.
    Unknown,
}

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

/// Kind key from the platform known-location table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownLocationKind {
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

/// Protection action assigned to a classification by the fixed ruleset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionAction {
    /// The target must never be changed.
    Locked,
    /// The requested operation has no supported change behavior.
    NoChange,
    /// The target is guidance-only until explicitly released.
    GuidanceOnly,
    /// The target may be changed after explicit confirmation.
    Cleanable,
}

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
