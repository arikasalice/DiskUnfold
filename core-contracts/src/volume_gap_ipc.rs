//! F03 wire contracts for explaining volume space against a pinned scan.
use crate::result::ResultCompleteness;
use crate::{ByteSize, Id, ItemRef, PathBytes, SizeEvidence, SizeValue, Timestamp};
use serde::{Deserialize, Serialize};

/// Selects one observed volume or one actual result root.
/// The use case validates that exactly one of these selectors is present.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VolumeGapSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional = nullable))]
    pub volume_id: Option<PathBytes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional = nullable))]
    pub root: Option<ItemRef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VolumeGapSelectionFields {
    volume_id: Option<PathBytes>,
    #[serde(
        default,
        deserialize_with = "crate::analysis_ipc::deserialize_optional_f03_request_item_ref"
    )]
    root: Option<ItemRef>,
}

impl<'de> Deserialize<'de> for VolumeGapSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields: VolumeGapSelectionFields =
            crate::analysis_ipc::deserialize_f03_request_object(deserializer)?;
        Ok(Self {
            volume_id: fields.volume_id,
            root: fields.root,
        })
    }
}

/// Requests a volume explanation for one pinned scan result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VolumeGapRequest {
    pub result_version: Id,
    pub request_seq: u32,
    pub target: VolumeGapSelection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VolumeGapRequestFields {
    result_version: Id,
    request_seq: u32,
    target: VolumeGapSelection,
}

impl<'de> Deserialize<'de> for VolumeGapRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields: VolumeGapRequestFields =
            crate::analysis_ipc::deserialize_f03_request_object(deserializer)?;
        Ok(Self {
            result_version: fields.result_version,
            request_seq: fields.request_seq,
            target: fields.target,
        })
    }
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
