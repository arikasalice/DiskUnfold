//! GUI query region lifetime and exact-target cancellation.

use crate::Id;
use serde::{Deserialize, Serialize};

/// The kind of query that may be admitted in one region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QueryRegionKind {
    Search,
    BrowseList,
    BrowseTreeBranch,
    WorkspaceClassification,
}

/// Requests a fresh region bound to the authenticated GUI session and window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct QueryRegionOpenRequest {
    pub kind: QueryRegionKind,
}

/// The server-issued reference to a single region lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryRegionOpened {
    pub region_ref: Id,
}

/// Retires the region and requests cancellation of its current evaluation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryRegionCloseRequest {
    pub region_ref: Id,
}

/// Acknowledges region retirement without waiting for a running job to finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(type = "Record<string, never>")
)]
#[serde(deny_unknown_fields)]
pub struct QueryRegionClosed {}

/// Exactly one selector within the supplied region lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum QueryCancelTarget {
    Request { request_seq: u32 },
    Job { job_ref: Id },
}

/// Cancels only a known request or job in this region.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryCancelRequest {
    pub region_ref: Id,
    pub target: QueryCancelTarget,
}

/// The supported F24 control outcomes for GUI query evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QueryCancelOutcome {
    Accepted,
    NoEffect,
    AlreadyTerminal,
}

/// A control acknowledgement. Job status supplies the eventual terminal state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct QueryCancelled {
    pub outcome: QueryCancelOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const REGION: &str = "018f0000-0000-7000-8000-000000000001";
    const JOB: &str = "018f0000-0000-7000-8000-000000000002";

    fn round_trip<T>(value: Value) -> Result<(), serde_json::Error>
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        let parsed: T = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(parsed)?, value);
        Ok(())
    }

    #[test]
    fn opens_and_closes_each_published_kind() -> Result<(), serde_json::Error> {
        for kind in [
            "search",
            "browse_list",
            "browse_tree_branch",
            "workspace_classification",
        ] {
            round_trip::<QueryRegionOpenRequest>(json!({"kind": kind}))?;
        }
        round_trip::<QueryRegionOpened>(json!({"regionRef": REGION}))?;
        round_trip::<QueryRegionCloseRequest>(json!({"regionRef": REGION}))?;
        round_trip::<QueryRegionClosed>(json!({}))?;
        assert!(serde_json::from_value::<QueryRegionClosed>(Value::Null).is_err());
        assert!(serde_json::from_value::<QueryRegionClosed>(json!({"closed": true})).is_err());
        assert!(
            serde_json::from_value::<QueryRegionOpenRequest>(
                json!({"kind": "search", "regionRef": REGION})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<QueryRegionOpenRequest>(json!({"kind": "folder"})).is_err()
        );
        Ok(())
    }

    #[test]
    fn requires_one_region_and_exactly_one_target() -> Result<(), serde_json::Error> {
        for target in [
            json!({"kind": "request", "requestSeq": 1}),
            json!({"kind": "request", "requestSeq": u32::MAX}),
            json!({"kind": "job", "jobRef": JOB}),
        ] {
            round_trip::<QueryCancelRequest>(json!({"regionRef": REGION, "target": target}))?;
        }
        for target in [
            json!({"kind": "request"}),
            json!({"kind": "job"}),
            json!({"kind": "request", "requestSeq": u64::from(u32::MAX) + 1}),
            json!({"kind": "request", "requestSeq": 1, "jobRef": JOB}),
            json!({"kind": "job", "jobRef": JOB, "requestSeq": 1}),
        ] {
            assert!(
                serde_json::from_value::<QueryCancelRequest>(
                    json!({"regionRef": REGION, "target": target})
                )
                .is_err()
            );
        }
        assert!(
            serde_json::from_value::<QueryCancelRequest>(
                json!({"target": {"kind": "job", "jobRef": JOB}})
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn acknowledgement_has_only_the_published_outcome() -> Result<(), serde_json::Error> {
        for outcome in ["accepted", "no_effect", "already_terminal"] {
            round_trip::<QueryCancelled>(json!({"outcome": outcome}))?;
        }
        assert!(
            serde_json::from_value::<QueryCancelled>(json!({"outcome": "unsupported"})).is_err()
        );
        assert!(
            serde_json::from_value::<QueryCancelled>(
                json!({"outcome": "accepted", "state": "cancelled"})
            )
            .is_err()
        );
        Ok(())
    }
}
