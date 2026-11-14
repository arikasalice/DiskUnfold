//! F36 set totals over group categories, read with `group.totals`.
use crate::{
    ByteSize, Id, SizeBasis, analysis_ipc::SpaceTotalSize, browse_page_ipc::QueryStrategy,
    data_source::DataSource, result::ResultCompleteness,
};
use serde::{Deserialize, Serialize};

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
#[serde(rename_all = "camelCase")]
pub struct GroupTotal {
    pub sizes: Vec<SpaceTotalSize>,
    pub completeness: ResultCompleteness,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
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
    rename_all_fields = "camelCase"
)]
pub enum GroupRatio {
    Value {
        basis: SizeBasis,
        numerator: ByteSize,
        denominator: ByteSize,
    },
    Undefined {
        basis: SizeBasis,
        reason_key: String,
    },
}

/// Totals over category sets. No total is derived from another by arithmetic.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GroupTotals {
    pub classification_ref: Id,
    pub result_version: Id,
    /// Data source of the pinned version, always set by the producer; absent means a sample (F32 FR2.HD2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub data_source: Option<DataSource>,
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
