//! GUI command contracts for listing saved snapshot metadata.

use crate::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// Requests one page of active snapshots.
///
/// Unknown fields are rejected. A missing limit uses 200 rows; callers may
/// reuse a cursor with a different limit.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotListRequest {
    /// Opaque continuation token returned by the previous page.
    #[serde(default)]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub cursor: Option<String>,
    /// Requested page size; zero and negative values are invalid.
    #[serde(default)]
    #[cfg_attr(feature = "typescript", ts(optional, type = "number"))]
    pub limit: Option<i64>,
}

/// One active snapshot page, including the index sequence it observes.
///
/// A page contains at most 1,000 rows. The stored format bounds each row to at
/// most 65,535 volumes (indices 0 through 65,534), so the theoretical maximum
/// is 65,535,000 volume entries across one full page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotListResponse {
    /// Snapshot-index sequence read with these rows.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub list_seq: i64,
    /// Active snapshot summaries in observation-time order.
    pub items: Vec<SnapshotSummary>,
    /// Opaque continuation token, omitted when this is the final page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub next_cursor: Option<String>,
}

/// Metadata exposed to the GUI for one active snapshot.
///
/// Condition JSON and observed volume identities are sensitive local metadata;
/// custom Debug output redacts them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    /// Snapshot identity.
    pub snapshot_id: Id,
    /// Observation time, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_at: Option<Timestamp>,
    /// Lowercase hexadecimal scope marker; omitted when the marker is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub scope_label: Option<String>,
    /// Stored condition-summary JSON text.
    pub condition_summary: String,
    /// Completeness recorded in the snapshot, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub completeness: Option<SnapshotListCompleteness>,
    /// Snapshot format version.
    pub format_version: u16,
    /// Sealed snapshot body size in bytes.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub body_size: i64,
    /// Whether this snapshot is the current baseline.
    pub pinned: bool,
    /// Whether this snapshot was captured locally or imported.
    pub source_kind: SnapshotListSourceKind,
    /// Volume identities stored with the snapshot, for GUI use only.
    pub volumes: Vec<SnapshotListVolume>,
}

impl std::fmt::Debug for SnapshotSummary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotSummary")
            .field("snapshot_id", &self.snapshot_id)
            .field("observed_at", &self.observed_at)
            .field(
                "scope_label",
                &self.scope_label.as_ref().map(|_| "<redacted>"),
            )
            .field("condition_summary", &"<redacted>")
            .field("completeness", &self.completeness)
            .field("format_version", &self.format_version)
            .field("body_size", &self.body_size)
            .field("pinned", &self.pinned)
            .field("source_kind", &self.source_kind)
            .field("volumes", &self.volumes)
            .finish()
    }
}

/// Completeness recorded in the sealed snapshot metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SnapshotListCompleteness {
    /// All requested observations were available.
    Complete,
    /// Some observations were unavailable.
    Partial,
    /// The observation was missing.
    Missing,
}

/// Origin of the snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SnapshotListSourceKind {
    /// Captured by this application.
    Local,
    /// Imported from another source.
    Imported,
}

/// Volume metadata associated with one snapshot.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotListVolume {
    /// Zero-based position in the snapshot.
    pub volume_index: u16,
    /// Lowercase hexadecimal observed identity, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_volume_id: Option<String>,
}

impl std::fmt::Debug for SnapshotListVolume {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotListVolume")
            .field("volume_index", &self.volume_index)
            .field(
                "observed_volume_id",
                &self.observed_volume_id.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::Error as _;
    use serde_json::json;

    const ID: &str = "018f0000-0000-7000-8000-000000000001";

    #[test]
    fn request_rejects_unknown_fields_and_response_accepts_them() -> Result<(), serde_json::Error> {
        let request: SnapshotListRequest = serde_json::from_value(json!({
            "cursor": "next",
            "limit": 10
        }))?;
        assert_eq!(request.cursor.as_deref(), Some("next"));
        assert_eq!(request.limit, Some(10));
        assert!(
            serde_json::from_value::<SnapshotListRequest>(json!({
                "limit": 10,
                "future": true
            }))
            .is_err()
        );

        let response: SnapshotListResponse = serde_json::from_value(json!({
            "listSeq": 1,
            "future": true,
            "items": [{
                "snapshotId": ID,
                "conditionSummary": "{}",
                "formatVersion": 1,
                "bodySize": 0,
                "pinned": false,
                "sourceKind": "local",
                "volumes": [{"volumeIndex": 0, "future": true}],
                "future": true
            }]
        }))?;
        assert_eq!(response.items.len(), 1);
        Ok(())
    }

    #[test]
    fn debug_redacts_condition_and_observed_volume_identity() -> Result<(), serde_json::Error> {
        let summary = SnapshotSummary {
            snapshot_id: Id::parse(ID).map_err(serde_json::Error::custom)?,
            observed_at: None,
            scope_label: Some("abab".into()),
            condition_summary: r#"{"private":"condition-secret"}"#.into(),
            completeness: None,
            format_version: 1,
            body_size: 0,
            pinned: false,
            source_kind: SnapshotListSourceKind::Local,
            volumes: vec![SnapshotListVolume {
                volume_index: 0,
                observed_volume_id: Some("volume-secret-01ab".into()),
            }],
        };
        let debug = format!("{summary:?}");
        assert!(!debug.contains("condition-secret"));
        assert!(!debug.contains("volume-secret-01ab"));
        assert!(!debug.contains("abab"));
        Ok(())
    }
}
