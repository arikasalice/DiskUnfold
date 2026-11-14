//! Raw serde shapes of the format version 1 header and payload JSON.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RawHeader {
    pub(super) bundle_version: String,
    pub(super) digest_algorithm: String,
    pub(super) digest: String,
    pub(super) declared_item_count: u64,
    pub(super) scenario_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RawPayload {
    pub(super) scenarios: Vec<RawScenario>,
    pub(super) observations: Vec<RawObservation>,
    pub(super) content_fragments: Vec<String>,
    pub(super) example_capabilities: RawCapabilities,
    pub(super) protection_rules: Vec<RawRule>,
    pub(super) cleanup_candidates: Vec<RawCandidate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawScenario {
    pub(super) id: String,
    pub(super) summary: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RawObservation {
    pub(super) index: u32,
    pub(super) observed_at: i64,
    pub(super) items: Vec<RawItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RawItem {
    pub(super) path: Vec<String>,
    pub(super) kind: RawKind,
    #[serde(default)]
    pub(super) size: u64,
    pub(super) allocated: Option<u64>,
    pub(super) modified_at: Option<i64>,
    pub(super) state: Option<RawState>,
    pub(super) content: Option<usize>,
    pub(super) scenario: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RawKind {
    Dir,
    File,
    Link,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RawState {
    AccessDenied,
    Vanished,
    LinkUntraversed,
    SizeUnknown,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RawCapability {
    Supported,
    Partial,
    Unsupported,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RawCapabilities {
    pub(super) allocated_size: RawCapability,
    pub(super) link_detection: RawCapability,
    pub(super) placeholder_detection: RawCapability,
    pub(super) extra_attributes: RawCapability,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RawRule {
    pub(super) id: String,
    pub(super) path: Vec<String>,
    pub(super) reason_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RawOperation {
    Trash,
    Move,
    Delete,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawCandidate {
    pub(super) operation: RawOperation,
    pub(super) path: Vec<String>,
}
