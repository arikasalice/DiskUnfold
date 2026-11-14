//! Published GUI success bodies for folder browsing and query pages.
use crate::{
    Id, ItemRef, PathBytes, SizeValue, Timestamp,
    browse_ipc::BrowseValue,
    result::{ResultCompleteness, ResultItemKind},
    space_detail_ipc::SpaceCapabilityState,
};
use serde::{Deserialize, Serialize};

/// Open result-origin identifier from the pinned result version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(transparent)]
pub struct ResultOrigin(pub String);

/// A selected strategy and its published revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryStrategy {
    pub id: String,
    pub revision: u32,
}

/// An observed file-identity capability with an optional explanation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityCapability {
    pub state: SpaceCapabilityState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_key: Option<String>,
}

/// Stable correspondence evidence from the pinned observation, not a live check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorrespondenceKey {
    pub volume_id: PathBytes,
    pub path_compare_key: PathBytes,
    #[serde(deserialize_with = "required_nullable")]
    pub file_id: Option<PathBytes>,
    pub identity_capability: IdentityCapability,
}

/// Independent evidence for unenumerated and excluded entries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FolderFlags {
    pub unenumerated: BrowseValue<bool>,
    pub excluded: BrowseValue<bool>,
}

/// One observed folder-query row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FolderQueryRow {
    pub item: ItemRef,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchQueryRow {
    pub item: ItemRef,
    pub display_name: BrowseValue<String>,
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

/// Exact or lower-bound number of rows in a pinned query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowseBreadcrumb {
    pub item: ItemRef,
    pub display_name: BrowseValue<String>,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
}

/// Immediate browse result or the job evaluating the request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
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
