//! F10 plan drafting, immutable version reads, and publication contracts.
//! Design items: FR1.HD2.app-host.DD1, FR3.HD1.app-host.DD1,
//! FR10.HD1.app-host.DD1, FR13.HD1.app-host.DD1.

use crate::{Id, ItemRef, Timestamp, job::ChangeAction, values::OsPath};
use serde::{Deserialize, Deserializer, Serialize, de::Error};

/// F10 GUI command for opening a plan draft (FR1.HD2.app-host.DD1).
pub const PLAN_DRAFT_OPEN: &str = "plan.draft_open";
/// F10 GUI command for editing a plan draft (FR1.HD2.app-host.DD1).
pub const PLAN_DRAFT_EDIT: &str = "plan.draft_edit";
/// F10 GUI command for discarding a plan draft (FR1.HD2.app-host.DD1).
pub const PLAN_DRAFT_CANCEL: &str = "plan.draft_cancel";
/// F10 GUI command for submitting a draft for plan construction (FR1.HD2.app-host.DD1).
pub const PLAN_BUILD: &str = "plan.build";
/// F10 GUI command for reading a fixed plan page (FR1.HD2.app-host.DD1).
pub const PLAN_GET: &str = "plan.get";
/// F10 GUI command for reading one item's review reason (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_REASON: &str = "plan.item_reason";
/// F10 GUI command for reading a plan lineage (FR1.HD2.app-host.DD1).
pub const PLAN_HISTORY: &str = "plan.history";
/// F10 event topic emitted after a plan version is fixed (FR11.HD1.app-host.DD1).
pub const PLAN_VERSION_FIXED: &str = "plan.version_fixed";
/// Canonical immutable-plan hash format (F10 FR10.HD1.app-host.DD1).
pub const PLAN_HASH_FORMAT_V1: &str = "du.plan.hash.v1";

/// Stable F10 request reasons (FR1.HD2.app-host.DD1).
pub const PLAN_SOURCE_NOT_CURRENT: &str = "plan.source_not_current";
/// Stable F10 request reasons (FR1.HD2.app-host.DD1).
pub const PLAN_SOURCE_CHANNEL_DENIED: &str = "plan.source_channel_denied";
/// Stable F10 request reason for mixing sample and non-sample sources (FR1.HD2.app-host.DD1).
pub const PLAN_SAMPLE_MIXED_SOURCE: &str = "sample.mixed_source";
/// Stable F10 request reasons (FR1.HD2.app-host.DD1).
pub const PLAN_EMPTY: &str = "plan.empty";
/// Stable F10 request reasons (FR1.HD2.app-host.DD1).
pub const PLAN_TOO_MANY_ITEMS: &str = "plan.too_many_items";
/// Stable F10 stale-plan reason (FR1.HD2.app-host.DD1).
pub const PLAN_RESULT_STALE: &str = "plan.result_stale";
/// Stable F10 stale-draft reason (FR1.HD2.app-host.DD1).
pub const PLAN_DRAFT_STALE: &str = "plan.draft_stale";
/// Stable F10 reason when a fixed plan version cannot be found (FR10.HD1.app-host.DD1).
pub const PLAN_NOT_FOUND: &str = "plan.not_found";
/// Stable session reason when a bounded draft registry evicts an entry (FR12.HD1.app-host.DD1).
pub const SESSION_EVICTED: &str = "session.evicted";
/// Stable F10 plan-readiness reason (FR11.HD1.app-host.DD1).
pub const PLAN_NOT_READY: &str = "plan.not_ready";
/// Stable F10 plan-construction failure reason (FR10.HD1.app-host.DD1).
pub const PLAN_STORE_FAILED: &str = "plan.store_failed";

/// Stable F10 review reason for protected items (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_PROTECTED: &str = "plan.item.protected";
/// Stable F10 review reason for protected descendants (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_PROTECTED_DESCENDANT: &str = "plan.item.protected_descendant";
/// Stable F10 review reason when protection is undetermined (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_UNDETERMINED: &str = "plan.item.undetermined";
/// Stable F10 review reason for guidance-only items (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_GUIDANCE_ONLY: &str = "plan.item.guidance_only";
/// Stable F10 review reason for duplicate selections (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_DUPLICATE_TARGET: &str = "plan.item.duplicate_target";
/// Stable F10 review reason for nested selections (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_NESTED_TARGET: &str = "plan.item.nested_target";
/// Stable F10 review reason for cross-volume moves (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_CROSS_VOLUME: &str = "plan.item.cross_volume";
/// Stable F10 review reason for a destination name collision (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_DEST_EXISTS: &str = "plan.item.dest_exists";
/// Stable F10 review reason when an overwrite cannot use trash (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_DEST_OVERWRITE_TRASH_UNSUPPORTED: &str =
    "plan.item.dest_overwrite_trash_unsupported";
/// Stable F10 review reason when an overwrite target is protected (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_DEST_OVERWRITE_PROTECTED: &str = "plan.item.dest_overwrite_protected";
/// Stable F10 review reason when overwrite protection is undetermined (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_DEST_OVERWRITE_UNKNOWN: &str = "plan.item.dest_overwrite_unknown";
/// Stable F10 review reason for items requiring confirmation (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_NEEDS_CONFIRMATION: &str = "plan.item.needs_confirmation";
/// Stable F10 review reason for overlapping selections (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_OVERLAP_CONFLICT: &str = "plan.item.overlap_conflict";
/// Stable F10 review reason for a destination inside the source (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_DEST_INSIDE_SELF: &str = "plan.item.dest_inside_self";
/// Stable F10 review reason when the destination cannot be established (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_DEST_UNKNOWN: &str = "plan.item.dest_unknown";
/// Stable F10 review reason for an unsupported action (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_ACTION_UNSUPPORTED: &str = "plan.item.action_unsupported";
/// Stable F10 review reason for a retention-condition conflict (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_KEEP_CONFLICT: &str = "plan.item.keep_conflict";
/// Stable F10 review reason when an item source no longer matches (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_SOURCE_MISMATCH: &str = "plan.item.source_mismatch";
/// Stable F10 review reason for a virtual scenario item (FR13.HD1.app-host.DD1).
pub const PLAN_ITEM_SCENARIO_VIRTUAL: &str = "plan.item.scenario_virtual";

/// The source class accepted by F10 draft creation (FR1.HD2.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PlanSourceKind {
    Manual,
    GoalAlternative,
    DuplicateKeep,
    Scenario,
}

/// Entry channel recorded on an immutable plan version head (FR10.HD1.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PlanOriginChannel {
    Gui,
    Cli,
    Mcp,
}

/// Readiness derived when a plan version is fixed (F10 FR10.HD1.app-host.DD1).
///
/// The F10 DLD includes `experience_plan` for sample sources; the F10 HLD's
/// IF-03 readiness list omits it, so that published cross-document mismatch
/// remains open for design review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PlanReadiness {
    ExperiencePlan,
    NeedsConfirmation,
    NoExecutableItems,
    ApprovalPossible,
}

/// The immutable plan section selected by `plan.get` (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PlanSection {
    Executable,
    Held,
    NeedsConfirmation,
    Absorbed,
}

/// Disposition recorded for one immutable plan item (F10 FR13.HD1.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PlanItemDisposition {
    Included,
    Held,
    Rejected,
}

/// A digest encoded as 64 lowercase hexadecimal characters (F10 FR10.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub struct PlanHash(String);

impl PlanHash {
    /// Validates and stores a normalized BLAKE3 digest.
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value))
        } else {
            Err("plan hash must be 64 lowercase hexadecimal characters".into())
        }
    }

    /// Returns the serialized digest text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PlanHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Count of draft entries by source class (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanSourceCount {
    pub source: PlanSourceKind,
    pub count: u32,
}

/// Summary returned after opening or editing a draft (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanDraftSummary {
    pub item_count: u32,
    pub by_source: Vec<PlanSourceCount>,
    pub conversion_held_count: u32,
}

/// Opens a draft from selected items or one registered alternative (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanDraftOpenRequest {
    pub source: PlanSourceKind,
    pub item_refs: Vec<ItemRef>,
    /// Omitted or null means no alternative reference; the source determines
    /// whether `itemRefs` or this reference is required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub alternative_ref: Option<Id>,
}

/// Result of opening a plan draft (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanDraftOpenResponse {
    pub draft_ref: Id,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub edit_seq: u64,
    pub summary: PlanDraftSummary,
}

/// One atomic edit to a draft, including per-item overwrite selection (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlanDraftEdit {
    AddItem {
        item_ref: ItemRef,
    },
    RemoveItem {
        item_ref: ItemRef,
    },
    SetAction {
        item_ref: ItemRef,
        action: ChangeAction,
    },
    SetDestination {
        item_ref: ItemRef,
        /// Null or omitted clears the destination.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        destination: Option<OsPath>,
    },
    SetRetentionCondition {
        item_ref: ItemRef,
        /// Null or omitted clears the retention condition reference.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        retention_condition_ref: Option<Id>,
    },
    SetOverwrite {
        item_ref: ItemRef,
        overwrite: bool,
    },
}

/// Applies one edit at the caller's expected draft sequence (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanDraftEditRequest {
    pub draft_ref: Id,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub edit_seq: u64,
    pub edit: PlanDraftEdit,
}

/// Result of a draft edit (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanDraftEditResponse {
    pub draft_ref: Id,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub edit_seq: u64,
    pub summary: PlanDraftSummary,
}

/// Discards a draft and any plan build that still belongs to it (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanDraftCancelRequest {
    pub draft_ref: Id,
}

/// Reports the cancellation effect without changing any fixed plan version (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanDraftCancelResponse {
    pub discarded: bool,
    pub build_cancelled: bool,
}

/// Submits a draft at the edit sequence the caller reviewed (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanBuildRequest {
    pub draft_ref: Id,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub edit_seq: u64,
}

/// Immediate acknowledgement of a plan build job (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanBuildResponse {
    pub job_ref: Id,
}

/// Reads a fixed plan section, optionally filtered by protection class (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanGetRequest {
    pub plan_version_id: Id,
    pub section: PlanSection,
    /// Omitted or null means no protection-class filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub protection_filter: Option<String>,
    /// Omitted or null starts at the first page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub cursor: Option<String>,
}

/// Immutable plan version head returned with a page of items (F10 FR10.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanVersionHead {
    pub plan_version_id: Id,
    pub lineage_id: Id,
    /// Null or omission means this is the first version in the lineage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub parent_plan_version_id: Option<Id>,
    pub plan_hash: PlanHash,
    pub hash_format: String,
    pub readiness: PlanReadiness,
    pub protection_ruleset_version: String,
    pub protection_class_table_version: String,
    /// The feature DLD defines this summary's meaning; nested fields remain
    /// opaque to consumers of the common plan head.
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub effect_summary: serde_json::Value,
    /// The feature DLD defines this summary's meaning; nested fields remain
    /// opaque to consumers of the common plan head.
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub restoration_limit_summary: serde_json::Value,
    pub origin_channel: PlanOriginChannel,
    pub fixed_at: Timestamp,
    pub change_summary: String,
}

/// One item from an immutable plan version (F10 FR10.HD1.app-host.DD1, FR13.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanVersionItem {
    pub item_no: u32,
    pub item_ref: ItemRef,
    pub action: ChangeAction,
    /// Omitted or null means the action has no destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub destination: Option<OsPath>,
    /// Omitted or null means no retention condition is attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub retention_condition_ref: Option<Id>,
    pub disposition: PlanItemDisposition,
    /// Whether an existing destination item is selected for overwrite.
    pub overwrite: bool,
    /// Expected identity and baseline facts captured when the version was fixed.
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub expected_identity: serde_json::Value,
    /// Protection evaluation details captured with this immutable item.
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub protection_summary: serde_json::Value,
    /// Omitted or null means the item has no single review reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
    pub selection_basis: Vec<String>,
    /// Omitted or null means this item does not represent absorbed items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub absorbed_by: Option<ItemRef>,
}

/// Page of immutable plan items (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanGetResponse {
    pub head: PlanVersionHead,
    pub items: Vec<PlanVersionItem>,
    /// Omitted or null means there is no next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub next_cursor: Option<String>,
}

/// Reads the selection rationale and restrictions for one plan item (F10 FR13.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanItemReasonRequest {
    pub plan_version_id: Id,
    pub item_no: u32,
}

/// Selection basis and review result stored on one immutable plan item (F10 FR13.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanItemReasonResponse {
    pub item_ref: ItemRef,
    pub selection_basis: Vec<String>,
    pub restrictions: Vec<String>,
    pub disposition: PlanItemDisposition,
    /// Omitted or null means no review reason applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
}

/// Reads fixed versions in one plan lineage (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanHistoryRequest {
    pub lineage_id: Id,
}

/// One immutable version listed by `plan.history` (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanHistoryEntry {
    pub plan_version_id: Id,
    pub fixed_at: Timestamp,
    pub change_summary: String,
}

/// Fixed plan versions in lineage order (F10 FR1.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanHistoryResponse {
    pub versions: Vec<PlanHistoryEntry>,
}

/// Event body sent once after the immutable plan version is committed (F10 FR11.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlanVersionFixed {
    pub plan_version_id: Id,
    pub lineage_id: Id,
    /// Omitted or null means the version did not replace another version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub replaced_plan_version_id: Option<Id>,
    pub plan_hash: PlanHash,
    pub readiness: PlanReadiness,
    pub included_count: u32,
    pub held_count: u32,
    pub needs_confirmation_count: u32,
    pub job_ref: Id,
    pub fixed_at: Timestamp,
}

#[cfg(test)]
mod tests;
