//! Narrow inputs used by the plan domain and its deterministic test doubles.
mod plan_version;
mod protection_facts;
pub use plan_version::*;
pub use protection_facts::*;

use du_core_contracts::{
    Id, ItemRef, OsPath, SizeValue, Timestamp, data_source::DataSource, ports::PortError,
    result::ResultItemKind, volume::CapabilityState,
};
use std::collections::BTreeMap;

/// The operation requested for one selected target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanAction {
    /// Move the target into the operating system trash.
    Trash,
    /// Move the target within its current volume.
    Move,
    /// Permanently delete the target.
    Delete,
}

/// The trusted origin kind attached by the request adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanSourceKind {
    /// A user-selected item list.
    UserSelection,
    /// A target suggested by the goal planner.
    GoalAlternative,
    /// A target selected by duplicate retention guidance.
    KeepSuggestion,
    /// A target from a hypothetical scenario.
    Scenario,
}

/// The request channel used only for admission and display metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestChannel {
    /// The desktop GUI.
    Gui,
    /// The command-line interface.
    Cli,
    /// The local MCP gateway.
    Mcp,
}

/// Confirmation choices supplied by the caller before plan construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmationChoice {
    /// Release default protection for eligible classifications in this plan.
    ReleaseDefaultProtection,
    /// Confirm replacement of existing destination entries.
    Overwrite,
}

/// One selected item and its source evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanSelection {
    /// Item in the pinned source result version.
    pub item: ItemRef,
    /// Requested operation.
    pub action: PlanAction,
    /// Opaque condition bytes used to detect overlapping selected trees.
    pub condition: Vec<u8>,
    /// Every reason that selected this target; duplicates are preserved.
    pub selection_evidence: Vec<String>,
    /// Destination for a move operation, if one was supplied.
    pub destination: Option<OsPath>,
    /// Whether the destination entry may be replaced.
    pub overwrite: bool,
    /// Observed size of the destination entry that would be displaced.
    pub displaced_size: Option<SizeValue>,
}

/// Inputs copied from one immutable analysis result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisItemFacts {
    /// Original native path; display text is never used for identity or ordering.
    pub path: OsPath,
    /// Native path components from the pinned result, used only for ancestry checks.
    pub path_components: Vec<Vec<u8>>,
    /// Volume identity associated with the selected result item.
    pub volume_id: Vec<u8>,
    /// File identity, absent when the source did not establish one.
    pub identity: Option<TargetIdentity>,
    /// Kind recorded by the source result.
    pub kind: ResultItemKind,
    /// Logical size observation, when available.
    pub size: Option<SizeValue>,
    /// Last-modified observation, when available.
    pub modified_at: Option<Timestamp>,
    /// Classification identifier from the pinned source result.
    pub classification_id: Option<String>,
}

/// Stable identity used for duplicate detection and effect aggregation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetIdentity {
    /// Native volume identity bytes.
    pub volume_id: Vec<u8>,
    /// Native file identity bytes.
    pub file_id: Vec<u8>,
}

/// Reads facts from a pinned analysis result.
pub trait AnalysisFactsPort: Send + Sync {
    /// Returns the source of the pinned result version.
    fn data_source(&self, version: &Id) -> Result<DataSource, PortError>;

    /// Reads one item from that immutable result version.
    fn read_item(&self, item: &ItemRef) -> Result<AnalysisItemFacts, PortError>;
}

/// Result of comparing current metadata with the pinned item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetCheck {
    /// Current metadata still matches the pinned target.
    Matches,
    /// The target changed since the pinned observation.
    Changed,
    /// The comparison could not be completed.
    Unavailable,
}

/// Rechecks the selected target without changing the filesystem.
pub trait TargetCheckPort: Send + Sync {
    /// Compares current metadata with the supplied pinned facts.
    fn check(&self, item: &AnalysisItemFacts) -> Result<TargetCheck, PortError>;
}

/// The one ruleset snapshot used for a complete plan build or re-judgement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesetSnapshot {
    /// Monotonic ruleset version.
    pub version: u64,
    /// Canonical ruleset digest.
    pub hash: Vec<u8>,
    /// Classification table version.
    pub classification_version: u32,
    /// Known-location table version.
    pub known_location_version: u32,
    /// Protection action by classification identifier.
    pub class_actions: BTreeMap<String, ProtectionAction>,
}

/// One item's protection facts read from platform and analysis adapters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformProtectionFacts {
    /// Current identity check; an absent expected identity is a deny.
    pub identity: Fact<IdentityObservation>,
    /// Link and mount boundary check.
    pub link_boundary: Fact<LinkBoundary>,
    /// Volume role.
    pub volume_role: Fact<ProtectionVolumeRole>,
    /// Known-location lookup.
    pub known_location: Fact<KnownLocation>,
    /// OS component check.
    pub os_component: Fact<bool>,
    /// Age guard result.
    pub age_guard: Fact<GuardResult>,
    /// Permission guard result.
    pub permission_guard: Fact<GuardResult>,
    /// Disk-backed guard result.
    pub disk_backed_guard: Fact<GuardResult>,
    /// User protection rule result.
    pub user_rule: Fact<GuardResult>,
    /// Descendant protection result.
    pub descendants: Fact<DescendantResult>,
    /// Current trash capability; absent means unknown.
    pub trash_capability: Option<CapabilityState>,
}

/// Reads current platform facts using the pinned item and one ruleset version.
pub trait PlatformFactsPort: Send + Sync {
    /// Reads facts without performing a file change.
    fn read(
        &self,
        item: &AnalysisItemFacts,
        ruleset: &RulesetSnapshot,
    ) -> Result<PlatformProtectionFacts, PortError>;
}

/// Reads one immutable protection ruleset snapshot.
pub trait ProtectionRulesetPort: Send + Sync {
    /// Returns the latest complete ruleset snapshot or a read error.
    fn read_current(&self) -> Result<RulesetSnapshot, PortError>;
}

/// Source of deterministic time for plan metadata and age evaluation.
pub trait ClockPort: Send + Sync {
    /// Returns the current UTC time, or none when the clock is unavailable.
    fn now(&self) -> Option<Timestamp>;
}

/// Source of UUID v7 identifiers for plan versions.
pub trait PlanIdPort: Send + Sync {
    /// Allocates one identifier.
    fn next_id(&self) -> Result<Id, PortError>;
}

/// Reads stored immutable versions for lineage edits and re-judgement.
pub trait PlanVersionPort: Send + Sync {
    /// Reads one stored version by its version ID.
    fn read(&self, version_id: &Id) -> Result<PlanVersion, PortError>;
}

/// One protection evaluator in its stable execution sequence.
pub trait ProtectionEvaluator: Send + Sync {
    /// Evaluator's stable identity.
    fn id(&self) -> EvaluatorId;

    /// Evaluates one item. Failures are converted to undetermined by the runner.
    fn evaluate(
        &self,
        input: &ProtectionEvaluatorInput<'_>,
    ) -> Result<ProtectionDecision, EvaluatorError>;
}

/// Stable identity of one of the twelve registered protection evaluators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvaluatorId {
    /// Expected file identity.
    Identity,
    /// Link and mount boundary.
    LinkBoundary,
    /// Volume role.
    VolumeRole,
    /// Known-location table.
    KnownLocation,
    /// Operating-system component.
    OsComponent,
    /// Classification action.
    ClassAction,
    /// Default-protection release.
    ReleaseState,
    /// Age guard.
    GuardAge,
    /// Permission guard.
    GuardPermission,
    /// Disk-backed guard.
    GuardDiskBacked,
    /// User protection rule.
    UserRule,
    /// Descendant protection.
    Descendant,
}

impl EvaluatorId {
    /// The architecture-ordered evaluator IDs used for F10 plans.
    pub const ORDERED: [Self; 12] = [
        Self::Identity,
        Self::LinkBoundary,
        Self::VolumeRole,
        Self::KnownLocation,
        Self::OsComponent,
        Self::ClassAction,
        Self::ReleaseState,
        Self::GuardAge,
        Self::GuardPermission,
        Self::GuardDiskBacked,
        Self::UserRule,
        Self::Descendant,
    ];

    /// Stable lowercase identifier used in evidence and hashes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::LinkBoundary => "link_boundary",
            Self::VolumeRole => "volume_role",
            Self::KnownLocation => "known_location",
            Self::OsComponent => "os_component",
            Self::ClassAction => "class_action",
            Self::ReleaseState => "release_state",
            Self::GuardAge => "guard_age",
            Self::GuardPermission => "guard_permission",
            Self::GuardDiskBacked => "guard_disk_backed",
            Self::UserRule => "user_rule",
            Self::Descendant => "descendant",
        }
    }
}

/// Protection decision emitted by one evaluator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionVerdict {
    /// No protection applies.
    Allow,
    /// Allowed after releasing a default protection.
    AllowReleased,
    /// Guidance is required before an operation can be included.
    GuidanceOnly,
    /// The evaluator could not establish a safe answer.
    Undetermined,
    /// The operation is denied.
    Deny,
}

/// One evaluator result, with its original protection reason when available.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectionDecision {
    /// Evaluator result.
    pub verdict: ProtectionVerdict,
    /// Whether the known-location evaluator skips rows 5 through 7.
    pub skip_class_rows: bool,
    /// Original protection reason key retained as evidence.
    pub reason_key: Option<String>,
}

/// A failed evaluator; only its reason key is retained in the plan evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluatorError {
    /// Original protection reason key.
    pub reason_key: String,
}

/// Read-only context supplied to one evaluator.
pub struct ProtectionEvaluatorInput<'a> {
    /// Pinned item facts.
    pub analysis: &'a AnalysisItemFacts,
    /// Current platform facts.
    pub platform: &'a PlatformProtectionFacts,
    /// One fixed ruleset snapshot.
    pub ruleset: &'a RulesetSnapshot,
    /// Requested action.
    pub action: PlanAction,
    /// Whether the request explicitly confirms default-protection release.
    pub release_confirmed: bool,
    /// Current UTC time, when available.
    pub now: Option<&'a Timestamp>,
}

/// Current summary of the protection evaluation stored with one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectionSummary {
    /// Combined verdict.
    pub verdict: ProtectionVerdict,
    /// Classification used by the class and release evaluators.
    pub classification_id: Option<String>,
    /// Released classification marker.
    pub released: bool,
    /// Evaluators actually run, in execution order.
    pub evaluators: Vec<(EvaluatorId, u16)>,
    /// Original protection reasons, in evaluation order.
    pub evidence: Vec<String>,
}

/// Final item state in the immutable plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanItemState {
    /// Included in the proposed operation set.
    Included,
    /// Requires review or another confirmation.
    Held,
    /// Rejected from the proposed operation set.
    Rejected,
}

/// Restore limitation attached to a planned operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreRestriction {
    /// Restore is supported or partially supported.
    Supported,
    /// Restore is unsupported for this operation.
    Unsupported,
    /// Restore capability is unknown.
    Unconfirmed,
}

/// Per-volume estimate quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstimateQuality {
    /// Every included item had an exact, established size.
    Exact,
    /// At least one included item had a lower-bound size.
    LowerBound,
    /// At least one included item's size could not be established.
    Unconfirmed,
}

/// Estimated change to one volume under change_estimate version 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeEstimate {
    /// Volume identity bytes.
    pub volume_id: Vec<u8>,
    /// Sum of established exact or lower-bound logical sizes.
    pub known_bytes: u64,
    /// Count of distinct included targets whose size is not established.
    pub unconfirmed_items: u64,
    /// Quality of the estimate.
    pub quality: EstimateQuality,
}

/// One plan item, including retained selection and protection evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanItem {
    /// Stable one-based position in the path-sorted item list.
    pub item_no: u32,
    /// Reference in the pinned result version.
    pub item: ItemRef,
    /// Native path used for ordering and identity.
    pub path: OsPath,
    /// Path components used for nested-target checks.
    pub path_components: Vec<Vec<u8>>,
    /// Volume identity.
    pub volume_id: Vec<u8>,
    /// Expected file identity, if observed.
    pub expected_identity: Option<TargetIdentity>,
    /// Expected item kind.
    pub expected_kind: ResultItemKind,
    /// Expected logical size.
    pub expected_size: Option<SizeValue>,
    /// Expected modification time.
    pub expected_modified_at: Option<Timestamp>,
    /// Classification identifier from the pinned result.
    pub classification_id: Option<String>,
    /// Requested action.
    pub action: PlanAction,
    /// Move destination, if supplied.
    pub destination: Option<OsPath>,
    /// Whether this item replaces a destination entry.
    pub overwrite: bool,
    /// Size of the displaced destination entry, when known.
    pub displaced_size: Option<SizeValue>,
    /// Condition that determines nested-target equivalence.
    pub condition: Vec<u8>,
    /// All source-selection evidence merged into this item.
    pub selection_evidence: Vec<String>,
    /// Included, held, or rejected.
    pub state: PlanItemState,
    /// Stable plan.item reason key, if one applies.
    pub reason_key: Option<String>,
    /// Earlier item that absorbed this rejected duplicate or nested target.
    pub absorbed_by: Option<u32>,
    /// Protection summary, absent when normalization rejected this item.
    pub protection: Option<ProtectionSummary>,
    /// Operation-specific restore capability.
    pub restore: RestoreRestriction,
}
