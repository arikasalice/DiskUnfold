//! Reads one stored observation back into its value form.

use du_core_contracts::{
    Id, SizeEvidence,
    result::ResultCompleteness,
    scope_observation_log::{
        ObservedAmount, ObservedSize, ScopeObservation, ScopeObservationOrigin,
    },
};
use rusqlite::Row;

/// The list query. Its column order is the one [`decode`] reads.
///
/// The equality prefix and the ordering follow the table's scope index, so a list is one
/// index seek followed by an ordered walk that stops at the limit.
pub(super) const LIST_SQL: &str = "SELECT observation_id, scope_fingerprint, fingerprint_format,
        observed_at_ms, job_id, origin, completeness, size_rule_id, size_rule_revision,
        logical_state, logical_bytes, logical_evidence,
        allocated_state, allocated_bytes, allocated_evidence,
        unique_physical_state, unique_physical_bytes, unique_physical_evidence, created_at_ms
     FROM analysis_index_scope_observation
     WHERE scope_fingerprint = ?1 AND fingerprint_format = ?2
       AND observed_at_ms >= ?3 AND observed_at_ms <= ?4
     ORDER BY observed_at_ms ASC, observation_id ASC LIMIT ?5";

/// Reads one row and returns `Ok(None)` when a stored value breaks the table contract.
///
/// SQLite failures surface as errors.
pub(super) fn decode(row: &Row<'_>) -> rusqlite::Result<Option<ScopeObservation>> {
    let id: Vec<u8> = row.get(0)?;
    let fingerprint: Vec<u8> = row.get(1)?;
    let format: i64 = row.get(2)?;
    let observed_at_ms: i64 = row.get(3)?;
    let job: Vec<u8> = row.get(4)?;
    let origin: String = row.get(5)?;
    let completeness: String = row.get(6)?;
    let size_rule_id: String = row.get(7)?;
    let revision: i64 = row.get(8)?;
    let logical = size_at(row, 9)?;
    let allocated = size_at(row, 12)?;
    let unique = size_at(row, 15)?;
    let created_at_ms: i64 = row.get(18)?;
    Ok((|| {
        Some(ScopeObservation {
            observation_id: id_from(&id)?,
            scope_fingerprint: fingerprint.try_into().ok()?,
            fingerprint_format: u32::try_from(format).ok().filter(|v| *v >= 1)?,
            observed_at_ms,
            job_id: id_from(&job)?,
            origin: match origin.as_str() {
                "scan" => ScopeObservationOrigin::Scan,
                "incremental" => ScopeObservationOrigin::Incremental,
                _ => return None,
            },
            completeness: match completeness.as_str() {
                "complete" => ResultCompleteness::Complete,
                "partial" => ResultCompleteness::Partial,
                "missing" => ResultCompleteness::Missing,
                "cancelled" => ResultCompleteness::Cancelled,
                _ => return None,
            },
            size_rule_id,
            size_rule_revision: u32::try_from(revision).ok().filter(|v| *v >= 1)?,
            logical: logical?,
            allocated: allocated?,
            unique_physical: unique?,
            created_at_ms,
        })
    })())
}

fn id_from(bytes: &[u8]) -> Option<Id> {
    let uuid = uuid::Uuid::from_slice(bytes).ok()?;
    Id::parse(&uuid.hyphenated().to_string()).ok()
}

fn size_at(row: &Row<'_>, first: usize) -> rusqlite::Result<Option<ObservedSize>> {
    let state: String = row.get(first)?;
    let bytes: Option<i64> = row.get(first + 1)?;
    let evidence: String = row.get(first + 2)?;
    let amount = match (state.as_str(), bytes) {
        ("exact", Some(bytes)) => u64::try_from(bytes).ok().map(ObservedAmount::Exact),
        ("lower_bound", Some(bytes)) => u64::try_from(bytes).ok().map(ObservedAmount::LowerBound),
        ("unknown", None) => Some(ObservedAmount::Unknown),
        _ => None,
    };
    let evidence = match evidence.as_str() {
        "measured" => Some(SizeEvidence::Measured),
        "os_reported" => Some(SizeEvidence::OsReported),
        "estimated" => Some(SizeEvidence::Estimated),
        "unknown" => Some(SizeEvidence::Unknown),
        _ => None,
    };
    Ok(amount
        .zip(evidence)
        .map(|(amount, evidence)| ObservedSize { amount, evidence }))
}
