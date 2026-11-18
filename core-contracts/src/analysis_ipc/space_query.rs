//! Space query request and response bodies (space.total).
use crate::{ByteSize, Id, ItemRef, SizeValue, Timestamp, browse_page_ipc::QueryStrategy};
use serde::{Deserialize, Serialize};

use super::f03_request::{self, F03RequestItemRef};

/// Selects a scope root, one folder, or one already-admitted search result.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SpaceTarget {
    ScopeRoot {},
    Folder { item: ItemRef },
    QueryResult { query_ref: Id },
}

impl<'de> Deserialize<'de> for SpaceTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            ScopeRoot,
            Folder,
            QueryResult,
        }

        fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
            deserializer: D,
        ) -> Result<Option<T>, D::Error> {
            T::deserialize(deserializer).map(Some)
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Selector {
            kind: Kind,
            #[serde(default, deserialize_with = "present")]
            item: Option<F03RequestItemRef>,
            #[serde(default, deserialize_with = "present")]
            query_ref: Option<Id>,
        }

        struct SelectorObject;
        impl<'de> serde::de::Visitor<'de> for SelectorObject {
            type Value = Selector;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a space target object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                Selector::deserialize(serde::de::value::MapAccessDeserializer::new(map))
            }
        }

        // Keep the existing selector conflict and explicit-null checks after rejecting extensions.
        let selector = deserializer.deserialize_map(SelectorObject)?;
        match (selector.kind, selector.item, selector.query_ref) {
            (Kind::ScopeRoot, None, None) => Ok(Self::ScopeRoot {}),
            (Kind::Folder, Some(item), None) => Ok(Self::Folder { item: item.into() }),
            (Kind::QueryResult, None, Some(query_ref)) => Ok(Self::QueryResult { query_ref }),
            _ => Err(serde::de::Error::custom("invalid space target selectors")),
        }
    }
}

/// Requests a total from one pinned result with the F03 GUI request fields only.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceTotalRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_version: Option<Id>,
    pub target: SpaceTarget,
    pub request_seq: u32,
}

impl<'de> Deserialize<'de> for SpaceTotalRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (result_version, target, request_seq) =
            f03_request::deserialize_space_total_request(deserializer)?;
        Ok(Self {
            result_version,
            target,
            request_seq,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceTotalSize {
    pub size: SizeValue,
    pub completeness: crate::result::ResultCompleteness,
    pub measurement_limits: Vec<String>,
}

/// Observation-time bounds of the items a total read, from the same pinned result version.
///
/// `earliest` and `latest` are the minimum and maximum retained item observation times of
/// the target, in UTC order. Both are present or both are null, and they are equal for a
/// single time. They are never filled from the response time, file modification times or
/// another result version. `complete` is true only when the target's range was fully
/// confirmed and every relevant item has an observation time; partial, missing or cancelled
/// ranges and unknown inputs to the total make it false while the confirmed bounds stay. A
/// fully confirmed empty set is `{earliest: null, latest: null, complete: true}`.
///
/// The constructor and the decoder reject one missing bound and `earliest` after `latest`.
/// Unknown fields inside the object are accepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", try_from = "ObservationRangeWire")]
pub struct SpaceObservationRange {
    earliest: Option<Timestamp>,
    latest: Option<Timestamp>,
    complete: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObservationRangeWire {
    earliest: Option<Timestamp>,
    latest: Option<Timestamp>,
    complete: bool,
}

impl TryFrom<ObservationRangeWire> for SpaceObservationRange {
    type Error = &'static str;

    fn try_from(wire: ObservationRangeWire) -> Result<Self, Self::Error> {
        Self::new(wire.earliest, wire.latest, wire.complete)
    }
}

impl SpaceObservationRange {
    /// Checks that both bounds are present or absent together and in time order.
    ///
    /// # Errors
    /// Returns a message when one bound is missing or `earliest` is after `latest`.
    pub fn new(
        earliest: Option<Timestamp>,
        latest: Option<Timestamp>,
        complete: bool,
    ) -> Result<Self, &'static str> {
        match (&earliest, &latest) {
            (Some(first), Some(last)) if first.unix_millis() > last.unix_millis() => {
                Err("observation range earliest is after latest")
            }
            (Some(_), None) | (None, Some(_)) => {
                Err("observation range bounds must both be present or both be null")
            }
            _ => Ok(Self {
                earliest,
                latest,
                complete,
            }),
        }
    }

    /// The earliest retained observation time, if any.
    pub fn earliest(&self) -> Option<&Timestamp> {
        self.earliest.as_ref()
    }

    /// The latest retained observation time, if any.
    pub fn latest(&self) -> Option<&Timestamp> {
        self.latest.as_ref()
    }

    /// Whether the range was fully confirmed with a time for every relevant item.
    pub fn complete(&self) -> bool {
        self.complete
    }
}

/// A response bound to a single immutable result version and request sequence.
/// Unavailable counts are null. The missing item count covers known observations.
///
/// The response accepts unknown fields. `size_rule`, `cancelled_unread_folders` and
/// `observation_range` are optional additions to contract version 1: a reader written before
/// them still decodes a response that carries them, and a response without them decodes with
/// them absent.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceTotal {
    pub result_version: Id,
    pub request_seq: u32,
    pub sizes: Vec<SpaceTotalSize>,
    pub completeness: crate::result::ResultCompleteness,
    pub missing_items: Option<ByteSize>,
    pub inaccessible_ranges: Option<ByteSize>,
    pub untraversed_links: Option<ByteSize>,
    /// The registered aggregation rule that produced these sizes, read from the pinned
    /// result version. Absent when an older producer answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub size_rule: Option<QueryStrategy>,
    /// The exact number of folders below the target left unread because the scan was
    /// cancelled, including the target itself when it is such a folder. Other-volume and
    /// already-counted folders, gaps and disconnects are not counted. `0` confirms none only
    /// for a sealed real scan with `Complete` or `Missing` status and no cancelled marks.
    ///
    /// Absent when the source cannot prove the total: a sample, an unfinished publication, a
    /// partial or cancelled result (even when it has marks), a complete or missing result
    /// containing a cancelled mark, an incremental replacement, or a target that is not a
    /// single subtree (a query result).
    /// Absence is not zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub cancelled_unread_folders: Option<u32>,
    /// Observation-time bounds of the items this total read in the pinned result version.
    ///
    /// A new producer sets it for a scope root, a folder and a query result; for a query
    /// result it covers the pinned items the set total actually reads. A group total that
    /// reuses this body does not carry it. A producer does not write null, and an explicit
    /// null decodes as absent. Absent means unknown, never zero or exact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observation_range: Option<SpaceObservationRange>,
}
