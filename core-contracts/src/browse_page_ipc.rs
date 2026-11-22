//! Published GUI success bodies for folder browsing and query pages.
//!
//! Response bodies accept unknown fields so that a later contract revision can
//! add optional values without breaking an older reader. `QueryStrategy` stays
//! strict because request bodies and stored group rules also decode it.
use crate::{
    Id, ItemRef, PathBytes, SizeValue, Timestamp,
    browse_value::BrowseValue,
    result::{ResultCompleteness, ResultItemKind},
    space_detail_ipc::SpaceCapabilityState,
};
use serde::{Deserialize, Serialize};

pub(crate) use crate::folder_flags::non_null;
pub use crate::folder_flags::{FolderBoundary, FolderFlags, UnreadInside};

/// Open result-origin identifier from the pinned result version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(transparent)]
pub struct ResultOrigin(pub String);

/// A selected strategy and its published revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
// Kept strict: the `viz.render` request (`strategy`) and stored F36 group rules
// (`analysis_index_group_rule.predicates_json`) decode this shape. Adding a field
// needs a strict request type split from a response type, and a decision on the
// stored `analysis_index_group_ruleset.format_version` (`RULESET_FORMAT_VERSION`).
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryStrategy {
    pub id: String,
    pub revision: u32,
}

/// An observed file-identity capability with an optional explanation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct IdentityCapability {
    pub state: SpaceCapabilityState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_key: Option<String>,
}

/// Stable correspondence evidence from the pinned observation, not a live check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceKey {
    pub volume_id: PathBytes,
    pub path_compare_key: PathBytes,
    #[serde(deserialize_with = "required_nullable")]
    pub file_id: Option<PathBytes>,
    pub identity_capability: IdentityCapability,
}

/// One observed folder-query row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FolderQueryRow {
    pub item: ItemRef,
    /// Human-readable display text; never reopen or compare a path from it.
    pub display_name: BrowseValue<String>,
    pub kind: ResultItemKind,
    pub logical_size: BrowseValue<SizeValue>,
    pub allocated_size: BrowseValue<SizeValue>,
    pub modified_at: BrowseValue<Timestamp>,
    pub has_child_folder: BrowseValue<bool>,
    pub flags: FolderFlags,
    pub correspondence: BrowseValue<CorrespondenceKey>,
}

/// One observed search-query row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SearchQueryRow {
    pub item: ItemRef,
    /// Human-readable display text; never reopen or compare a path from it.
    pub display_name: BrowseValue<String>,
    /// Human-readable display text; never reopen or compare a path from it.
    pub display_path: BrowseValue<String>,
    pub kind: ResultItemKind,
    pub logical_size: BrowseValue<SizeValue>,
    pub allocated_size: BrowseValue<SizeValue>,
    pub modified_at: BrowseValue<Timestamp>,
    pub created_at: BrowseValue<Timestamp>,
    #[serde(deserialize_with = "unique_strings")]
    pub missing_fields: Vec<String>,
    #[serde(deserialize_with = "unique_strings")]
    pub reason_keys: Vec<String>,
}

/// A count that is exact or a lower bound: the rows of a pinned query, or the
/// excluded entries directly under a folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum QueryTotal {
    Exact { count: u32 },
    LowerBound { count: u32 },
}

/// Literal row kind for folder pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FolderRowKind {
    Folder,
}
/// Literal row kind for search pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SearchRowKind {
    Search,
}

/// Page fields common to folder and search query results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct QueryPage<R> {
    pub query_ref: Id,
    pub result_version: Id,
    pub rows: Vec<R>,
    pub total: QueryTotal,
    pub unknown_count: u32,
    #[serde(deserialize_with = "required_nullable")]
    pub observed_at: Option<Timestamp>,
    pub completeness: ResultCompleteness,
    #[serde(deserialize_with = "required_nullable")]
    pub next_cursor: Option<String>,
}

/// A folder-only page used by the first browse response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FolderQueryPage {
    pub row_kind: FolderRowKind,
    #[serde(flatten)]
    pub page: QueryPage<FolderQueryRow>,
    /// Location context for a directly requested first page, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub browse_context: Option<BrowseContext>,
    /// Why lower bounds on this page have no row reason (#226), as a reason key:
    /// `scan.evidence_not_recorded` (analyzing again gives exact values),
    /// `scan.evidence_budget_exceeded` or `scan.lower_bound_unexplained`. Present only
    /// when a size or the item count on this page is a lower bound that no row mark,
    /// unread range, boundary or measurement limit explains, and never while the scan
    /// runs. An unknown key reads as `scan.lower_bound_unexplained`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub unexplained_lower_bound: Option<String>,
}
/// A search-only page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SearchQueryPage {
    pub row_kind: SearchRowKind,
    #[serde(flatten)]
    pub page: QueryPage<SearchQueryRow>,
}
/// A query page selects the row contract registered with the query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum QueryPageResponse {
    Folder(FolderQueryPage),
    Search(SearchQueryPage),
}

/// A breadcrumb item and its observed display name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BrowseBreadcrumb {
    pub item: ItemRef,
    /// Human-readable display text; never reopen or compare a path from it.
    pub display_name: BrowseValue<String>,
    /// The root item of a confirmed macOS startup volume group. GUI channel only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "non_null"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub is_startup_volume: Option<bool>,
}
/// The opened location's relationship to its scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum BrowseRootKind {
    VirtualRoot,
    ScopeRoot,
    Folder,
}

/// Location and strategy evidence from the same pinned folder query as its page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BrowseContext {
    pub origin: ResultOrigin,
    /// Data source of the pinned version; absent means a sample (F32 FR2.HD2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub data_source: Option<crate::data_source::DataSource>,
    #[serde(deserialize_with = "required_nullable")]
    pub location: Option<ItemRef>,
    pub breadcrumbs: Vec<BrowseBreadcrumb>,
    pub root_kind: BrowseRootKind,
    pub current_correspondence: bool,
    pub sort_strategies: Vec<QueryStrategy>,
    pub predicate_strategies: Vec<QueryStrategy>,
    /// Entries directly under the opened folder that an exclusion rule left out.
    ///
    /// Exact zero means confirmed none. A lower bound means the folder's listing was
    /// cut short. Names an exclusion rule could not decide are not counted. Omitted
    /// when the source does not supply this fact, when it cannot be decided yet (a
    /// result of a scan in progress where it is not known that the folder's listing
    /// has ended), and for the virtual top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub excluded_children: Option<QueryTotal>,
}

/// Immediate browse result or the job evaluating the request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BrowseOpenResponse {
    Opened {
        request_seq: u32,
        origin: ResultOrigin,
        /// Data source of the pinned version; absent means a sample (F32 FR2.HD2).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        data_source: Option<crate::data_source::DataSource>,
        #[serde(deserialize_with = "required_nullable")]
        location: Option<ItemRef>,
        breadcrumbs: Vec<BrowseBreadcrumb>,
        root_kind: BrowseRootKind,
        current_correspondence: bool,
        page: Box<FolderQueryPage>,
        sort_strategies: Vec<QueryStrategy>,
        predicate_strategies: Vec<QueryStrategy>,
        /// Entries directly under the opened folder that an exclusion rule left out;
        /// see `BrowseContext::excluded_children`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        excluded_children: Option<QueryTotal>,
    },
    Job {
        request_seq: u32,
        job_ref: Id,
        /// Source result version pinned when the request was admitted.
        result_version: Id,
    },
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn unique_strings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    use serde::de::Error;
    let values = Vec::<String>::deserialize(deserializer)?;
    let mut seen = std::collections::HashSet::new();
    if values.iter().any(|value| !seen.insert(value)) {
        return Err(D::Error::custom("duplicate value"));
    }
    Ok(values)
}

#[cfg(test)]
mod tests;
