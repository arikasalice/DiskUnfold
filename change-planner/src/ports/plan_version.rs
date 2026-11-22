use super::*;
use serde::{Deserialize, Serialize};

/// Confirmation token facts stored with a plan version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ConfirmationToken {
    /// Classifications whose default protection was released for included items.
    ReleaseDefaultProtection { classification_ids: Vec<String> },
    /// Count and known displaced size for included overwrite items.
    Overwrite {
        item_count: u64,
        displaced_bytes: u64,
    },
}

/// One immutable plan version returned by the pure domain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanVersion {
    /// UUID v7 version identifier.
    pub version_id: Id,
    /// Stable family identifier inherited from a parent version.
    pub lineage_id: Id,
    /// Parent version, if this is a revised plan.
    pub parent_version_id: Option<Id>,
    /// Pinned analysis result used to construct the plan.
    pub source_version_id: Id,
    /// Source kind attached to the request.
    pub source_kind: PlanSourceKind,
    /// Data source resolved from the pinned result.
    pub data_source: DataSource,
    /// Request channel, deliberately excluded from the plan hash.
    pub request_channel: RequestChannel,
    /// Fixed protection ruleset metadata.
    pub ruleset: Option<RulesetVersion>,
    /// Path-sorted immutable item list.
    pub items: Vec<PlanItem>,
    /// Per-volume expected effect, sorted by volume identity.
    pub expected_effect: Vec<VolumeEstimate>,
    /// Confirmation tokens represented in this plan.
    pub confirmations: Vec<ConfirmationToken>,
    /// Readiness decision.
    pub readiness: crate::readiness::PlanReadiness,
    /// Hash format tag.
    pub hash_format: String,
    /// BLAKE3 hash of the canonical du.plan.hash.v1 scope.
    pub hash: [u8; 32],
    /// Creation time, deliberately excluded from the plan hash.
    pub created_at: Timestamp,
}

/// Protection ruleset values fixed into a plan version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesetVersion {
    /// Monotonic version.
    pub version: u64,
    /// Canonical digest.
    pub hash: Vec<u8>,
    /// Classification table version.
    pub classification_version: u32,
    /// Known-location table version.
    pub known_location_version: u32,
}

/// Re-judgement result for one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejudgementState {
    /// Current protection facts match the stored allow decision.
    Same,
    /// Current class or protection summary changed.
    ClassChanged,
    /// Current protection facts are incomplete.
    Undetermined,
    /// Current protection denies the operation.
    Denied,
}

/// Re-judgement result for one stored plan version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRejudgement {
    /// Version checked.
    pub version_id: Id,
    /// Recomputed hash of the stored content.
    pub recomputed_hash: [u8; 32],
    /// Current protection ruleset version used for every item in this check.
    /// None means the ruleset could not be read and all items fail closed.
    pub ruleset_version: Option<u64>,
    /// Re-judged states in plan item order.
    pub items: Vec<(u32, RejudgementState)>,
}
