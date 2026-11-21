//! GUI command and event contracts for saving local snapshots.

use crate::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// Requests a local snapshot of one immutable result version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotSaveRequest {
    /// Result version to capture.
    pub result_version_id: Id,
    /// Whether the new snapshot should become the baseline.
    pub pin: bool,
}

/// Result of admitting a snapshot save request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum SnapshotSaveResponse {
    /// A newly admitted background save.
    Accepted(SnapshotSaveAccepted),
    /// A snapshot for the requested result version already exists.
    ExistingSnapshot(SnapshotSaveExistingSnapshot),
    /// A background save for the requested result version is already active.
    ExistingJob(SnapshotSaveExistingJob),
}

impl<'de> Deserialize<'de> for SnapshotSaveResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireResponse {
            outcome: String,
            #[serde(default, deserialize_with = "deserialize_wire_id")]
            job_id: WireIdField,
            #[serde(default, deserialize_with = "deserialize_wire_id")]
            snapshot_id: WireIdField,
            #[serde(default, deserialize_with = "deserialize_pin_applied")]
            pin_applied: PinAppliedField,
        }

        let wire = WireResponse::deserialize(deserializer)?;
        match (
            wire.outcome.as_str(),
            wire.job_id,
            wire.snapshot_id,
            wire.pin_applied,
        ) {
            (
                "accepted",
                WireIdField::Present(job_id),
                WireIdField::Missing,
                PinAppliedField::Missing,
            ) => Ok(Self::Accepted(SnapshotSaveAccepted {
                outcome: SnapshotSaveAcceptedOutcome::Accepted,
                job_id,
            })),
            (
                "existing",
                WireIdField::Present(job_id),
                WireIdField::Missing,
                PinAppliedField::False,
            ) => Ok(Self::ExistingJob(SnapshotSaveExistingJob {
                outcome: SnapshotSaveExistingOutcome::Existing,
                job_id,
                pin_applied: SnapshotPinApplied::FALSE,
            })),
            (
                "existing",
                WireIdField::Missing,
                WireIdField::Present(snapshot_id),
                PinAppliedField::False,
            ) => Ok(Self::ExistingSnapshot(SnapshotSaveExistingSnapshot {
                outcome: SnapshotSaveExistingOutcome::Existing,
                snapshot_id,
                pin_applied: SnapshotPinApplied::FALSE,
            })),
            _ => Err(serde::de::Error::custom(
                "invalid snapshot save response shape",
            )),
        }
    }
}

#[derive(Default)]
enum WireIdField {
    #[default]
    Missing,
    Present(Id),
}

fn deserialize_wire_id<'de, D>(deserializer: D) -> Result<WireIdField, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Id::deserialize(deserializer).map(WireIdField::Present)
}

#[derive(Default)]
enum PinAppliedField {
    #[default]
    Missing,
    False,
    True,
}

fn deserialize_pin_applied<'de, D>(deserializer: D) -> Result<PinAppliedField, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(if bool::deserialize(deserializer)? {
        PinAppliedField::True
    } else {
        PinAppliedField::False
    })
}

/// Accepted snapshot save response variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSaveAccepted {
    /// Fixed outcome tag.
    pub outcome: SnapshotSaveAcceptedOutcome,
    /// Admitted background job.
    pub job_id: Id,
}

/// Accepted response tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum SnapshotSaveAcceptedOutcome {
    #[serde(rename = "accepted")]
    Accepted,
}

/// Existing snapshot response variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSaveExistingSnapshot {
    /// Fixed outcome tag.
    pub outcome: SnapshotSaveExistingOutcome,
    /// Existing snapshot identity.
    pub snapshot_id: Id,
    /// Existing snapshots are not pinned by a save request.
    pub pin_applied: SnapshotPinApplied,
}

/// Existing save requests never apply pin state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "false"))]
pub struct SnapshotPinApplied(());

impl SnapshotPinApplied {
    /// Represents the wire value `pinApplied: false`.
    pub const FALSE: Self = Self(());
}

impl Serialize for SnapshotPinApplied {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bool(false)
    }
}

impl<'de> Deserialize<'de> for SnapshotPinApplied {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            return Err(serde::de::Error::custom("expected false"));
        }
        Ok(Self::FALSE)
    }
}

/// Existing in-flight job response variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSaveExistingJob {
    /// Fixed outcome tag.
    pub outcome: SnapshotSaveExistingOutcome,
    /// Active save job identity.
    pub job_id: Id,
    /// A duplicate save does not alter pin state.
    pub pin_applied: SnapshotPinApplied,
}

/// Existing response tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum SnapshotSaveExistingOutcome {
    #[serde(rename = "existing")]
    Existing,
}

/// Snapshot list mutations that can be announced on `snapshot.changed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SnapshotChange {
    /// A local save was registered.
    Saved,
    /// An imported snapshot was registered.
    Imported,
    /// A snapshot became the baseline.
    Pinned,
    /// A snapshot ceased to be the baseline.
    Unpinned,
    /// A snapshot was deleted.
    Deleted,
    /// A snapshot was removed by retention.
    Swept,
}

/// Body of the sequenced `snapshot.changed` desktop event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotChangedBody {
    /// Committed snapshot-index list sequence.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub list_seq: i64,
    /// Mutation represented by this event.
    pub change: SnapshotChange,
    /// Snapshot identities affected by the mutation.
    pub snapshot_ids: Vec<Id>,
    /// Time at which the mutation was observed, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::Error as _;

    const ID: &str = "018f0000-0000-7000-8000-000000000001";

    #[test]
    fn save_request_rejects_unknown_fields_and_requires_declared_fields()
    -> Result<(), serde_json::Error> {
        let request: SnapshotSaveRequest = serde_json::from_value(serde_json::json!({
            "resultVersionId": ID,
            "pin": true
        }))?;
        assert!(request.pin);
        assert!(
            serde_json::from_value::<SnapshotSaveRequest>(serde_json::json!({
                "resultVersionId": ID,
                "pin": true,
                "future": "rejected"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SnapshotSaveRequest>(serde_json::json!({
                "resultVersionId": ID
            }))
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn snapshot_changed_event_accepts_unknown_fields() -> Result<(), serde_json::Error> {
        let event: SnapshotChangedBody = serde_json::from_value(serde_json::json!({
            "listSeq": 1,
            "change": "saved",
            "snapshotIds": [ID],
            "future": "ignored"
        }))?;
        assert_eq!(event.list_seq, 1);
        assert_eq!(event.change, SnapshotChange::Saved);
        assert_eq!(
            event.snapshot_ids,
            [Id::parse(ID).map_err(serde_json::Error::custom)?]
        );
        Ok(())
    }

    #[test]
    fn save_response_union_requires_one_identity_and_keeps_existing_unpinned()
    -> Result<(), serde_json::Error> {
        let response: SnapshotSaveResponse = serde_json::from_value(serde_json::json!({
            "outcome": "existing",
            "snapshotId": ID,
            "pinApplied": false,
            "future": "ignored"
        }))?;
        assert_eq!(
            response,
            SnapshotSaveResponse::ExistingSnapshot(SnapshotSaveExistingSnapshot {
                outcome: SnapshotSaveExistingOutcome::Existing,
                snapshot_id: Id::parse(ID).map_err(serde_json::Error::custom)?,
                pin_applied: SnapshotPinApplied::FALSE,
            })
        );
        assert_eq!(serde_json::to_value(&response)?["pinApplied"], false);
        assert!(matches!(
            serde_json::from_value::<SnapshotSaveResponse>(serde_json::json!({
                "outcome": "accepted",
                "jobId": ID,
                "future": true
            }))?,
            SnapshotSaveResponse::Accepted(_)
        ));
        assert!(
            serde_json::from_value::<SnapshotSaveResponse>(serde_json::json!({
                "outcome": "accepted"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SnapshotSaveResponse>(serde_json::json!({
                "outcome": "existing",
                "pinApplied": false
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SnapshotSaveResponse>(serde_json::json!({
                "outcome": "accepted",
                "jobId": null
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SnapshotSaveResponse>(serde_json::json!({
                "outcome": "existing",
                "snapshotId": ID,
                "jobId": ID,
                "pinApplied": false
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SnapshotSaveResponse>(serde_json::json!({
                "outcome": "existing",
                "snapshotId": ID,
                "pinApplied": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SnapshotSaveResponse>(serde_json::json!({
                "outcome": "existing",
                "snapshotId": ID,
                "pinApplied": null
            }))
            .is_err()
        );
        Ok(())
    }
}
