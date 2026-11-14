//! Version change notifications emitted by the immutable result store.
use crate::{Id, Timestamp};
use serde::{Deserialize, Serialize};

pub const RESULT_VERSION_CHANGED: &str = "results.version_changed";

/// The kind of replacement committed to a lineage's current result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ResultVersionChange {
    Published,
    Withdrawn,
}

/// The completeness visible in a result notification, separate from job state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PublishedCompleteness {
    Complete,
    Partial,
    Unknown,
}

/// The topic body. Its sequence is scoped to the lineage, not the transport queue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResultVersionChanged {
    pub lineage_id: Id,
    pub source_ref: Id,
    pub origin: String,
    /// Data source of the pinned version; absent means a sample (F32 FR2.HD1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub data_source: Option<crate::data_source::DataSource>,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    pub change: ResultVersionChange,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub previous_result_version: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub completeness: Option<PublishedCompleteness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_at: Option<Timestamp>,
}

/// A committed version change and the current version, if one remains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultVersionNotice {
    pub result_version: Option<Id>,
    pub body: ResultVersionChanged,
}

impl ResultVersionNotice {
    /// Checks invariants spanning the envelope and the topic-specific body.
    pub fn validate(&self) -> bool {
        let body = &self.body;
        let has_current = self.result_version.is_some();
        let valid_change = match body.change {
            ResultVersionChange::Published => has_current,
            ResultVersionChange::Withdrawn => body.previous_result_version.is_some(),
        };
        valid_change
            && body.sequence > 0
            && body.sequence < (1_u64 << 53)
            && !body.origin.is_empty()
            && body.previous_result_version != self.result_version
            && body.completeness.is_some() == has_current
            && (has_current || body.observed_at.is_none())
    }
}

/// Enqueues a committed result notification without invoking its subscribers.
pub trait ResultVersionPublisher: Send + Sync {
    fn publish_result_version(&self, notice: ResultVersionNotice);
}
