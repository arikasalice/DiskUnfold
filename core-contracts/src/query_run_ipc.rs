//! GUI query.run (F04 IF-01): one condition over the items below a folder or below
//! every scope root, opened as a search page in a search query region.
use crate::{
    Id, ItemRef,
    browse_open_ipc::{BrowseSortKey, BrowseSourceRef, QueryCondition},
    browse_page_ipc::{ResultOrigin, SearchQueryPage},
};
use serde::{Deserialize, Serialize};

/// Evaluates a condition over a subtree. GUI channel only: rows carry display paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryRunRequest {
    /// A query region opened with kind `search`.
    pub region_ref: Id,
    /// Strictly increasing per region; an equal or lower value is `command.body_invalid`.
    pub request_seq: u32,
    /// Absent selects the current analysis, as `browse.open`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub source_ref: Option<BrowseSourceRef>,
    /// The version the caller shows; another current version is `result.requery_required`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_version: Option<Id>,
    /// A folder of that version; candidates are the items below it at any depth.
    /// Absent: below every scope root. The folder itself is not a candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub scope: Option<ItemRef>,
    pub condition: QueryCondition,
    /// Absent: `sort.path` ascending. Ties follow the path, then the item number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub sort_keys: Option<Vec<BrowseSortKey>>,
    /// Default 200, 1 to 1000; anything else is `cursor.limit_exceeded`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub page_size: Option<u32>,
}

/// The first search page, or the `query.evaluate` job that outlived the synchronous wait.
/// The job's completed status reports the query reference for `query.page`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum QueryRunResponse {
    Opened {
        request_seq: u32,
        result_version: Id,
        origin: ResultOrigin,
        /// Data source of the pinned version; absent means a sample (F32 FR2.HD2).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        data_source: Option<crate::data_source::DataSource>,
        /// The requested folder, or null for every scope root.
        #[serde(deserialize_with = "required_nullable")]
        scope: Option<ItemRef>,
        page: Box<SearchQueryPage>,
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

#[cfg(test)]
mod tests;
