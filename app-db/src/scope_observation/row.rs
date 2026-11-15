//! Maps observation values to and from the columns of their table.

use du_core_contracts::{
    SizeEvidence,
    result::ResultCompleteness,
    scope_observation_log::{
        ObservedAmount, ObservedSize, ScopeObservation, ScopeObservationOrigin,
    },
};
use rusqlite::{Transaction, params};

/// A validated row in storage form.
pub(super) struct Encoded {
    pub(super) id: [u8; 16],
    pub(super) job_id: [u8; 16],
    pub(super) fingerprint: [u8; 32],
    pub(super) format: i64,
    observed_at_ms: i64,
    origin: &'static str,
    completeness: &'static str,
    size_rule_id: String,
    size_rule_revision: i64,
    sizes: [EncodedSize; 3],
    created_at_ms: i64,
}

struct EncodedSize {
    state: &'static str,
    bytes: Option<i64>,
    evidence: &'static str,
}

/// Converts a row, or returns `None` when a value cannot be stored as given.
///
/// A byte count of 2^63 or more does not fit a SQLite integer. It is refused instead
/// of being stored as unknown, because unknown would claim that nothing was measured.
pub(super) fn encode(observation: &ScopeObservation) -> Option<Encoded> {
    if observation.fingerprint_format == 0
        || observation.size_rule_revision == 0
        || observation.size_rule_id.is_empty()
    {
        return None;
    }
    Some(Encoded {
        id: observation.observation_id.v7_bytes()?,
        job_id: observation.job_id.v7_bytes()?,
        fingerprint: observation.scope_fingerprint,
        format: i64::from(observation.fingerprint_format),
        observed_at_ms: observation.observed_at_ms,
        origin: match observation.origin {
            ScopeObservationOrigin::Scan => "scan",
            ScopeObservationOrigin::Incremental => "incremental",
        },
        completeness: match observation.completeness {
            ResultCompleteness::Complete => "complete",
            ResultCompleteness::Partial => "partial",
            ResultCompleteness::Missing => "missing",
            ResultCompleteness::Cancelled => "cancelled",
        },
        size_rule_id: observation.size_rule_id.clone(),
        size_rule_revision: i64::from(observation.size_rule_revision),
        sizes: [
            encode_size(&observation.logical)?,
            encode_size(&observation.allocated)?,
            encode_size(&observation.unique_physical)?,
        ],
        created_at_ms: observation.created_at_ms,
    })
}

fn encode_size(size: &ObservedSize) -> Option<EncodedSize> {
    let (state, bytes) = match size.amount {
        ObservedAmount::Exact(bytes) => ("exact", Some(i64::try_from(bytes).ok()?)),
        ObservedAmount::LowerBound(bytes) => ("lower_bound", Some(i64::try_from(bytes).ok()?)),
        ObservedAmount::Unknown => ("unknown", None),
    };
    let evidence = match size.evidence {
        SizeEvidence::Measured => "measured",
        SizeEvidence::OsReported => "os_reported",
        SizeEvidence::Estimated => "estimated",
        SizeEvidence::Unknown => "unknown",
    };
    Some(EncodedSize {
        state,
        bytes,
        evidence,
    })
}

impl Encoded {
    pub(super) fn insert(&self, tx: &Transaction<'_>) -> rusqlite::Result<()> {
        let [logical, allocated, unique] = &self.sizes;
        tx.execute(
            "INSERT INTO analysis_index_scope_observation (
                 observation_id, scope_fingerprint, fingerprint_format, observed_at_ms, job_id,
                 origin, completeness, size_rule_id, size_rule_revision,
                 logical_state, logical_bytes, logical_evidence,
                 allocated_state, allocated_bytes, allocated_evidence,
                 unique_physical_state, unique_physical_bytes, unique_physical_evidence,
                 created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                self.id.as_slice(),
                self.fingerprint.as_slice(),
                self.format,
                self.observed_at_ms,
                self.job_id.as_slice(),
                self.origin,
                self.completeness,
                self.size_rule_id,
                self.size_rule_revision,
                logical.state,
                logical.bytes,
                logical.evidence,
                allocated.state,
                allocated.bytes,
                allocated.evidence,
                unique.state,
                unique.bytes,
                unique.evidence,
                self.created_at_ms,
            ],
        )
        .map(|_| ())
    }
}
