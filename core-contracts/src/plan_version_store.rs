use crate::{Id, data_source::DataSource, ports::PortError};
use std::fmt;

/// Maximum UTF-8 byte length of a serialized immutable plan body.
pub const PLAN_BODY_MAX_BYTES: usize = 128 * 1024 * 1024;

/// Preparation state persisted with a real-source plan version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanReadiness {
    /// The plan has an executable target and needs no further confirmation.
    Ready,
    /// The plan is held until the user confirms unresolved items.
    NeedsConfirmation,
    /// The plan contains no executable target.
    NoTarget,
}

/// Channel that fixed the immutable plan version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanOriginChannel {
    /// Fixed through the GUI.
    Gui,
    /// Fixed through the CLI.
    Cli,
    /// Fixed through the MCP gateway.
    Mcp,
}

/// One raw, owned row from the immutable plan-version store.
///
/// The body and digest are intentionally unverified storage values. The
/// change-planner owner must decode the body and recompute its canonical hash
/// before relying on the plan.
#[derive(Clone, PartialEq, Eq)]
pub struct PlanVersionRecord {
    /// UUID v7 identifier for this immutable version.
    pub plan_version_id: Id,
    /// Stable identifier shared by versions in one plan history.
    pub lineage_id: Id,
    /// Parent version when this version revises an earlier plan.
    pub parent_plan_version_id: Option<Id>,
    /// Stored digest for the canonical plan content.
    pub version_hash: [u8; 32],
    /// Tag selecting the canonical digest format.
    pub hash_format: String,
    /// Immutable protection ruleset version used to build the plan.
    pub ruleset_version: u64,
    /// Whether the fixed plan is ready for an approval request.
    pub readiness: PlanReadiness,
    /// Application channel that fixed the plan; excluded from its digest.
    pub origin_channel: PlanOriginChannel,
    /// Validated JSON containing the remaining plan header and item values (maximum [`PLAN_BODY_MAX_BYTES`]).
    pub body: String,
    /// Time the version was fixed, in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Samples are session-only and must be rejected by persistent stores.
    pub data_source: DataSource,
}

impl fmt::Debug for PlanVersionRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlanVersionRecord")
            .field("plan_version_id", &self.plan_version_id)
            .field("lineage_id", &self.lineage_id)
            .field("parent_plan_version_id", &self.parent_plan_version_id)
            .field("version_hash", &self.version_hash)
            .field("ruleset_version", &self.ruleset_version)
            .field("readiness", &self.readiness)
            .field("origin_channel", &self.origin_channel)
            .field("body_len", &self.body.len())
            .field("created_at_ms", &self.created_at_ms)
            .field("data_source", &self.data_source)
            .finish()
    }
}

/// Plan-version metadata without the potentially large serialized body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanVersionHead {
    /// UUID v7 identifier for this immutable version.
    pub plan_version_id: Id,
    /// Stable identifier shared by versions in one plan history.
    pub lineage_id: Id,
    /// Parent version when this version revises an earlier plan.
    pub parent_plan_version_id: Option<Id>,
    /// Stored digest for the canonical plan content.
    pub version_hash: [u8; 32],
    /// Tag selecting the canonical digest format.
    pub hash_format: String,
    /// Immutable protection ruleset version used to build the plan.
    pub ruleset_version: u64,
    /// Whether the fixed plan is ready for an approval request.
    pub readiness: PlanReadiness,
    /// Application channel that fixed the plan.
    pub origin_channel: PlanOriginChannel,
    /// Time the version was fixed, in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Persistent plan versions always have a real data source.
    pub data_source: DataSource,
}

impl From<PlanVersionRecord> for PlanVersionHead {
    fn from(version: PlanVersionRecord) -> Self {
        Self {
            plan_version_id: version.plan_version_id,
            lineage_id: version.lineage_id,
            parent_plan_version_id: version.parent_plan_version_id,
            version_hash: version.version_hash,
            hash_format: version.hash_format,
            ruleset_version: version.ruleset_version,
            readiness: version.readiness,
            origin_channel: version.origin_channel,
            created_at_ms: version.created_at_ms,
            data_source: version.data_source,
        }
    }
}

/// Storage contract for immutable plan versions.
///
/// Lineage reads are ordered by creation time and plan-version ID. Prefer
/// read_lineage_heads for history listings and read_body when the body is
/// needed; read_lineage retains its full-body behavior for compatibility.
/// The store does not verify parent existence or lineage equality; the
/// change-planner writer is responsible. Parallel fixations are stored
/// independently.
pub trait PlanVersionStore {
    /// Appends one complete fixed version in one transaction.
    ///
    /// Implementations reject sample data and return `db.busy` without retrying
    /// when the configured 5,000 ms SQLite wait expires.
    fn append_fixed(&mut self, version: &PlanVersionRecord) -> Result<(), PortError>;

    /// Reads exactly one stored version by ID.
    fn read(&self, version_id: &Id) -> Result<PlanVersionRecord, PortError>;

    /// Reads the body of exactly one stored version by ID.
    ///
    /// The default preserves compatibility with stores that only implement
    /// read; storage adapters should override it to avoid loading other
    /// version fields when that is useful.
    fn read_body(&self, version_id: &Id) -> Result<String, PortError> {
        self.read(version_id).map(|version| version.body)
    }

    /// Reads metadata for all versions in one lineage without their bodies.
    ///
    /// The result is ordered by creation time and plan-version ID. The default
    /// preserves compatibility with stores that only implement read_lineage;
    /// storage adapters should override it to avoid materializing bodies.
    fn read_lineage_heads(&self, lineage_id: &Id) -> Result<Vec<PlanVersionHead>, PortError> {
        self.read_lineage(lineage_id)
            .map(|versions| versions.into_iter().map(PlanVersionHead::from).collect())
    }

    /// Reads all stored versions for one lineage in creation-time and ID order.
    ///
    /// The result is not paged or capped and includes every full body. Prefer
    /// read_lineage_heads when a caller only needs history metadata.
    fn read_lineage(&self, lineage_id: &Id) -> Result<Vec<PlanVersionRecord>, PortError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(seed: u8) -> Id {
        let mut bytes = [0; 16];
        bytes[0] = seed;
        Id::from_v7_bytes(bytes)
    }

    #[test]
    fn debug_redacts_plan_body() {
        let path = r#"C:\Users\private\disk-plan.txt"#;
        let record = PlanVersionRecord {
            plan_version_id: id(1),
            lineage_id: id(2),
            parent_plan_version_id: None,
            version_hash: [3; 32],
            hash_format: "du.plan.hash.v1".into(),
            ruleset_version: 1,
            readiness: PlanReadiness::Ready,
            origin_channel: PlanOriginChannel::Gui,
            body: format!(r#"{{"path":"{path}"}}"#),
            created_at_ms: 10,
            data_source: DataSource::Real,
        };

        let debug = format!("{record:?}");
        assert!(debug.contains(&format!("body_len: {}", record.body.len())));
        assert!(!debug.contains(path));
    }
}
