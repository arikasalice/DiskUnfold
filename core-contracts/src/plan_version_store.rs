use crate::{Id, data_source::DataSource, ports::PortError};

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
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Validated JSON containing the remaining plan header and item values.
    pub body: String,
    /// Time the version was fixed, in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Samples are session-only and must be rejected by persistent stores.
    pub data_source: DataSource,
}

/// Storage contract for immutable plan versions.
///
/// A lineage read is ordered by creation time so the owner can inspect its
/// history and newest stored version. The store does not impose a stale-parent
/// rule: concurrent fixed versions are retained independently.
pub trait PlanVersionStore {
    /// Appends one complete fixed version in one transaction.
    ///
    /// Implementations reject sample data and return `db.busy` without retrying
    /// when the configured 5,000 ms SQLite wait expires.
    fn append_fixed(&mut self, version: &PlanVersionRecord) -> Result<(), PortError>;

    /// Reads exactly one stored version by ID.
    fn read(&self, version_id: &Id) -> Result<PlanVersionRecord, PortError>;

    /// Reads all stored versions for one lineage in creation-time order.
    fn read_lineage(&self, lineage_id: &Id) -> Result<Vec<PlanVersionRecord>, PortError>;
}
