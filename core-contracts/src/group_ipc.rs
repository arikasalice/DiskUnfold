//! F36 virtual-group GUI commands. Groups classify observed items only; no
//! command here changes files or edits observed values.
use crate::{
    Id, ItemRef, SizeBasis, SizeValue, Timestamp,
    analysis_ipc::SpaceTotalSize,
    browse_ipc::BrowseValue,
    browse_open_ipc::{BrowseSortKey, QueryCondition},
    browse_page_ipc::QueryStrategy,
    result::ResultCompleteness,
};
use serde::{Deserialize, Serialize};

/// Current condition-model revision for group rules (the F04 query condition).
pub const GROUP_CONDITION_MODEL_VERSION: u32 = 1;

/// Rule review status attached when a stored ruleset is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GroupRuleReview {
    Ok,
    Converted,
    PredicateUnregistered,
}

/// A stored rule with its pinned predicate strategies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRule {
    pub group_id: Id,
    pub display_name: String,
    pub display_order: u32,
    pub condition: QueryCondition,
    pub condition_model_version: u32,
    pub predicates: Vec<QueryStrategy>,
    pub review: GroupRuleReview,
}

/// A rule edited in the GUI draft. A missing group ID creates a new group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRuleDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub group_id: Option<Id>,
    pub display_name: String,
    pub condition: QueryCondition,
}

/// The stored ruleset. Version and revision are zero before the first save.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRuleset {
    pub ruleset_version: u32,
    pub revision: u32,
    /// Lower-case hexadecimal fingerprint of the canonical rule content.
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub updated_at: Option<Timestamp>,
    pub rules: Vec<GroupRule>,
}

/// Reads the stored ruleset and its per-rule review.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRulesGetRequest {}

/// Replaces the whole ruleset if the stored revision is still `expectedRevision`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRulesSaveRequest {
    pub expected_revision: u32,
    pub rules: Vec<GroupRuleDraft>,
}

/// One failed review check for a draft rule at its zero-based list index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRuleIssue {
    pub index: u32,
    pub reason_key: String,
}

/// Saved ruleset, or a review rejection that stored nothing.
///
/// A rejection carries `reasonKey = "group.rule_invalid"` and the first failing
/// check of each rejected rule. Conflicts and storage failures are error envelopes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "outcome",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GroupRulesSaveResponse {
    Saved {
        ruleset: GroupRuleset,
    },
    Rejected {
        reason_key: String,
        issues: Vec<GroupRuleIssue>,
    },
}

/// The rules used by one classification request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum GroupRuleSource {
    /// The stored ruleset; the request fails if it is no longer this version.
    Saved { ruleset_version: u32 },
    /// An unsaved draft evaluated without storing it.
    Draft { rules: Vec<GroupRuleDraft> },
}

/// Classifies files below `scope` (or the whole result) in one pinned result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupClassifyRequest {
    /// GUI view channel; a larger sequence on the same channel cancels earlier work.
    pub view_ref: Id,
    pub request_seq: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_version: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub scope: Option<ItemRef>,
    pub rules: GroupRuleSource,
}

/// Per-group counts in rule display order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupSummaryRow {
    pub group_id: Id,
    pub display_name: String,
    pub display_order: u32,
    pub member_count: u32,
    /// Members that also matched another group.
    pub overlap_member_count: u32,
    /// Files this rule could not decide.
    pub undecidable_count: u32,
    /// The rule uses a predicate that is not registered.
    pub rule_unevaluable: bool,
}

/// Count for one known reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupReasonCount {
    pub reason_key: String,
    pub count: u32,
}

/// One limitation of the classification evidence. Keys form an open set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupLimitation {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub count: Option<u32>,
}

/// Limitation keys in the order of the engine's limitation table.
pub mod limitation {
    pub const OBSERVED_RECORD: &str = "group.limit.observed_record";
    pub const PARTIAL: &str = "group.limit.partial";
    pub const STALE: &str = "group.limit.stale";
    pub const UNDECIDABLE: &str = "group.limit.undecidable";
    pub const ROLE_UNCONFIRMED: &str = "group.limit.role_unconfirmed";
    pub const SIZE_BOUND: &str = "group.limit.size_bound";
    pub const TIMEZONE: &str = "condition.timezone_unavailable";
    pub const RULESET_CHANGED: &str = "group.limit.ruleset_changed";
}

/// A registered classification bound to its pinned inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupSummary {
    pub classification_ref: Id,
    pub result_version: Id,
    /// Result origin, such as `scan` or `incremental`.
    pub source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_at: Option<Timestamp>,
    pub completeness: ResultCompleteness,
    pub scope: ItemRef,
    /// Zero when the rules came from an unsaved draft.
    pub ruleset_version: u32,
    pub ruleset_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reference_time: Option<Timestamp>,
    pub groups: Vec<GroupSummaryRow>,
    pub scope_file_count: u32,
    pub matched_count: u32,
    pub overlap_count: u32,
    pub unclassified_count: u32,
    pub undecidable_count: u32,
    /// Whether scope = matched + unclassified + undecidable held at registration.
    pub reconciled: bool,
    pub undecidable_reasons: Vec<GroupReasonCount>,
    pub limitations: Vec<GroupLimitation>,
    pub predicates: Vec<QueryStrategy>,
}

/// Summary, background job, or an inapplicable source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GroupClassifyResponse {
    Summary {
        request_seq: u32,
        summary: Box<GroupSummary>,
    },
    /// Read `group.summary` with `classificationRef` after the job finishes as done.
    Job {
        request_seq: u32,
        job_ref: Id,
        classification_ref: Id,
        result_version: Id,
    },
    NotApplicable {
        request_seq: u32,
        reason_key: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupSummaryRequest {
    pub classification_ref: Id,
}

/// Cancels the running classification of a view or a specific job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum GroupCancelRequest {
    View { view_ref: Id, request_seq: u32 },
    Job { job_ref: Id },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GroupCancelOutcome {
    Accepted,
    NoEffect,
    AlreadyTerminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupCancelResponse {
    pub outcome: GroupCancelOutcome,
}

/// A derived member category of one classification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum GroupCategory {
    Group { group_id: Id },
    Overlap {},
    Unclassified {},
    Undecidable {},
}

/// Reads one position range of a category in a fixed order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupMembersRequest {
    pub classification_ref: Id,
    pub result_version: Id,
    pub category: GroupCategory,
    /// Defaults to descending logical size; the path order always breaks ties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub sort: Option<BrowseSortKey>,
    pub start: u32,
    pub count: u32,
}

/// One observed file. A file in several groups is one row with several group IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupMemberRow {
    pub item: ItemRef,
    pub display_name: BrowseValue<String>,
    /// Original observed path; a group name never appears here.
    pub display_path: BrowseValue<String>,
    pub logical_size: BrowseValue<SizeValue>,
    pub allocated_size: BrowseValue<SizeValue>,
    /// Matched groups in rule display order.
    pub groups: Vec<Id>,
    pub undecidable_reason_keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupMembers {
    pub classification_ref: Id,
    pub result_version: Id,
    pub start: u32,
    pub total: u32,
    pub rows: Vec<GroupMemberRow>,
    pub sort: QueryStrategy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupExplainRequest {
    pub classification_ref: Id,
    pub item: ItemRef,
}

/// Three-valued rule or condition-node decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GroupDecision {
    Match,
    NoMatch,
    Undecidable,
}

/// Decision of one condition node. `path` lists child indices from the rule root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupConditionNode {
    pub path: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub predicate: Option<QueryStrategy>,
    pub decision: GroupDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRuleDecision {
    pub group_id: Id,
    pub decision: GroupDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
    pub nodes: Vec<GroupConditionNode>,
}

/// Why one file is (or is not) in each group, re-evaluated with the pinned rules.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupExplanation {
    pub classification_ref: Id,
    pub item: ItemRef,
    pub display_path: BrowseValue<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_at: Option<Timestamp>,
    pub rules: Vec<GroupRuleDecision>,
}

/// Requests set totals; an empty selection selects every group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupTotalsRequest {
    pub classification_ref: Id,
    pub group_ids: Vec<Id>,
}

/// Independent F03 set total for each basis (logical, allocated, unique physical).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupTotal {
    pub sizes: Vec<SpaceTotalSize>,
    pub completeness: ResultCompleteness,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupTotalRow {
    pub group_id: Id,
    pub total: GroupTotal,
}

/// Union share of the scope total, as an exact integer pair or not calculable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum GroupRatio {
    Value {
        basis: SizeBasis,
        numerator: crate::ByteSize,
        denominator: crate::ByteSize,
    },
    Undefined {
        basis: SizeBasis,
        reason_key: String,
    },
}

/// Totals over category sets. No total is derived from another by arithmetic.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupTotals {
    pub classification_ref: Id,
    pub result_version: Id,
    pub groups: Vec<GroupTotalRow>,
    pub union: GroupTotal,
    pub overlap: GroupTotal,
    pub overlap_count: u32,
    pub unclassified: GroupTotal,
    pub undecidable: GroupTotal,
    pub scope: GroupTotal,
    pub ratios: Vec<GroupRatio>,
    pub size_rule: QueryStrategy,
}

#[cfg(test)]
mod tests;
