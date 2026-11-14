//! F32 sample experience commands, events and reason keys.
use crate::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// Event topic for sample session state changes.
pub const SAMPLE_STATE_CHANGED: &str = "sample.state_changed";

/// Unavailable: the embedded bundle is absent.
pub const SAMPLE_MISSING: &str = "sample.missing";
/// Unavailable: no reader is registered for the bundle format version.
pub const SAMPLE_FORMAT_UNSUPPORTED: &str = "sample.format_unsupported";
/// Unavailable: digest, limit or record validation failed.
pub const SAMPLE_BUNDLE_INVALID: &str = "sample.bundle_invalid";
/// Unavailable: the load job ended as partial or failed.
pub const SAMPLE_LOAD_FAILED: &str = "sample.load_failed";
/// `NOT_FOUND`: the session was closed or belongs to an earlier app session.
pub const SAMPLE_SESSION_CLOSED: &str = "sample.session_closed";
/// `INVALID_ARGUMENT`: a sample result cannot be saved as a snapshot.
pub const SNAPSHOT_SAMPLE_SOURCE: &str = "snapshot.sample_source";
/// Approval review outcome for a request plan that is a sample or cannot be found.
pub const APPROVAL_SAMPLE_SOURCE_DENIED: &str = "approval.sample_source_denied";
/// Capability reason: the operation is never available for sample data.
pub const CAPABILITY_SAMPLE_SOURCE: &str = "sample.source";
/// Capability reason: the value is an example recorded in the sample bundle.
pub const CAPABILITY_SAMPLE_EXAMPLE: &str = "sample.example";
/// Action key offered with an unavailable session: start a real analysis instead.
pub const SAMPLE_REAL_ANALYSIS_ACTION: &str = "sample.start_real_analysis";

/// The sample session state owned by the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SampleSessionState {
    Preparing,
    Active,
    Resetting,
    Unavailable,
    Closed,
}

impl SampleSessionState {
    /// Maps the internal state to the published event value.
    pub fn event_state(self) -> SampleEventState {
        match self {
            Self::Preparing | Self::Resetting => SampleEventState::Loading,
            Self::Active => SampleEventState::Active,
            Self::Unavailable => SampleEventState::Failed,
            Self::Closed => SampleEventState::Inactive,
        }
    }
}

/// The state value published by `sample.state_changed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SampleEventState {
    Inactive,
    Loading,
    Active,
    Failed,
}

/// Starts a sample session or returns the open one. The body is an empty object.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SampleOpenRequest {}

impl<'de> Deserialize<'de> for SampleOpenRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(Self {})
        } else {
            Err(serde::de::Error::custom("expected an empty object"))
        }
    }
}

/// Addresses one sample session for status, reset or close.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SampleSessionRequest {
    pub session_ref: Id,
}

/// The sample result version built for one observation point of the bundle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SampleObservationRef {
    /// Zero-based observation order in the bundle.
    pub index: u32,
    pub result_version: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_at: Option<Timestamp>,
}

/// The sample session as seen by the main window.
///
/// `unavailable_reason` and `real_analysis_action` are present only when the
/// state is `unavailable`; unavailability is a normal response, not an error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SampleSession {
    pub session_ref: Id,
    pub state: SampleSessionState,
    /// Monotonic per session; equals the sequence of the latest state event.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub bundle_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub job_ref: Option<Id>,
    /// Sample result versions in observation order; empty until loading completes.
    pub observations: Vec<SampleObservationRef>,
    /// Sample plan versions kept in this session.
    pub plans: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub unavailable_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub real_analysis_action: Option<String>,
}

/// Result of closing a session. Closing an already closed session repeats it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SampleClosed {
    pub session_ref: Id,
    pub state: SampleSessionState,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub released_versions: u64,
}

/// Body of `sample.state_changed`. Receivers drop sequences at or below the last seen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SampleStateChanged {
    pub session_ref: Id,
    pub state: SampleEventState,
    pub previous_state: SampleEventState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub job_ref: Option<Id>,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    pub observed_at: Timestamp,
}

#[cfg(test)]
mod tests;
