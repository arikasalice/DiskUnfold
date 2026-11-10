//! F03 wire contracts for explaining volume space against a pinned scan.
use crate::result::ResultCompleteness;
use crate::{ByteSize, Id, ItemRef, PathBytes, SizeEvidence, SizeValue, Timestamp};
use serde::{Deserialize, Serialize};

/// Selects one observed volume or one actual result root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeGapSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional = nullable))]
    pub volume_id: Option<PathBytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional = nullable))]
    pub root: Option<ItemRef>,
}

/// Requests a volume explanation for one pinned scan result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeGapRequest {
    pub result_version: Id,
    pub request_seq: u32,
    pub target: VolumeGapSelection,
}

/// One scan-phase volume observation, including explicit failures.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VolumeGapObservation {
    NotAttempted,
    Unavailable {
        #[serde(rename = "observedAt")]
        observed_at: Option<Timestamp>,
        #[serde(rename = "reasonKey")]
        reason_key: String,
    },
    Available {
        #[serde(rename = "observedAt")]
        observed_at: Option<Timestamp>,
        #[serde(rename = "totalBytes")]
        total_bytes: ByteSize,
        #[serde(rename = "usedBytes")]
        used_bytes: ByteSize,
        #[serde(rename = "freeBytes")]
        free_bytes: ByteSize,
        evidence: SizeEvidence,
    },
}

/// One scoped size and its basis-specific completeness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct VolumeGapSize {
    pub size: SizeValue,
    pub completeness: ResultCompleteness,
}

/// Exact attributed bytes and evidence, or a reason bytes are unavailable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VolumeGapAmount {
    Exact {
        bytes: ByteSize,
        evidence: SizeEvidence,
    },
    Unknown {
        #[serde(rename = "reasonKey")]
        reason_key: String,
    },
}

/// One explanation factor or OS-reported system category.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct VolumeGapRow {
    pub key: String,
    pub value: VolumeGapAmount,
}

/// Distinguishes unavailable categories from a successful empty report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VolumeGapCategories {
    Unavailable {
        #[serde(rename = "reasonKey")]
        reason_key: String,
    },
    Available {
        rows: Vec<VolumeGapRow>,
    },
}

/// Observations and scoped totals kept together under one immutable result version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VolumeGapResponse {
    pub result_version: Id,
    pub request_seq: u32,
    pub volume_id: Option<PathBytes>,
    pub start: VolumeGapObservation,
    pub end: VolumeGapObservation,
    pub allocated: VolumeGapSize,
    pub unique_physical: VolumeGapSize,
    pub factors: Vec<VolumeGapRow>,
    pub remainder: VolumeGapAmount,
    pub os_categories: VolumeGapCategories,
}
