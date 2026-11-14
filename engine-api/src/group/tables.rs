//! Table-driven F36 decisions: source admission, limitation markers and ratios.
use du_analysis_index::group::Classification;
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeValue,
    group_ipc::{GroupLimitation, GroupRatio, limitation},
    result::{ResultCompleteness, ResultVersionRead},
};

use super::Meta;

/// Result origins that are observations. A new origin is one row here.
const OBSERVED_SOURCES: &[&str] = &[
    "scan",
    "incremental",
    "snapshot",
    "catalog",
    "query_derived",
];
/// Origins that record a past observation rather than the current analysis.
const RECORDED_SOURCES: &[&str] = &["snapshot", "catalog"];

/// Whether classification accepts this result origin (F36 FR9.HD1.app-host.DD1).
pub(super) fn source_allowed(origin: &str) -> bool {
    OBSERVED_SOURCES.contains(&origin)
}

/// Inputs evaluated by every limitation row.
pub(super) struct Evidence<'a> {
    pub(super) classification: &'a Classification,
    pub(super) meta: &'a Meta,
    /// The stored ruleset version now, when the classification used a saved ruleset.
    pub(super) current_ruleset: Option<u32>,
}

type Row = fn(&Evidence<'_>) -> Option<GroupLimitation>;

/// Limitation table in published order; every row is checked and markers accumulate.
const ROWS: &[Row] = &[
    recorded,
    partial,
    stale,
    undecidable,
    role,
    size_bound,
    time_zone,
    ruleset_changed,
];

/// Returns the limitation markers in table order (F36 FR7.HD1.app-host.DD1).
pub(super) fn limitations(evidence: &Evidence<'_>) -> Vec<GroupLimitation> {
    ROWS.iter().filter_map(|row| row(evidence)).collect()
}

fn marker(key: &str, count: Option<u32>) -> GroupLimitation {
    GroupLimitation {
        key: key.into(),
        count,
    }
}

fn recorded(evidence: &Evidence<'_>) -> Option<GroupLimitation> {
    let origin = evidence.classification.version().origin();
    RECORDED_SOURCES
        .contains(&origin)
        .then(|| marker(limitation::OBSERVED_RECORD, None))
}

fn partial(evidence: &Evidence<'_>) -> Option<GroupLimitation> {
    (evidence.classification.version().completeness() != ResultCompleteness::Complete)
        .then(|| marker(limitation::PARTIAL, None))
}

/// The current result source supplies no staleness marker; the row stays in the table.
fn stale(_: &Evidence<'_>) -> Option<GroupLimitation> {
    None
}

fn undecidable(evidence: &Evidence<'_>) -> Option<GroupLimitation> {
    let count = evidence.classification.summary().undecidable;
    (count > 0).then(|| marker(limitation::UNDECIDABLE, Some(count)))
}

fn role(evidence: &Evidence<'_>) -> Option<GroupLimitation> {
    evidence
        .classification
        .predicates()
        .iter()
        .any(|(id, _)| id == "pred.role")
        .then(|| marker(limitation::ROLE_UNCONFIRMED, None))
}

fn size_bound(evidence: &Evidence<'_>) -> Option<GroupLimitation> {
    let version = evidence.classification.version();
    let aggregate = match evidence.classification.root() {
        Some(root) => version.space(root.item_no)?,
        None => version.scope_total(),
    };
    let bounded = [&aggregate.logical, &aggregate.allocated]
        .iter()
        .any(|size| !matches!(size.amount, SizeAmount::Exact { .. }));
    bounded.then(|| marker(limitation::SIZE_BOUND, None))
}

/// Relative-time predicates are not registered in this release; the reference time
/// and zone are fixed only when a rule uses one.
fn time_zone(evidence: &Evidence<'_>) -> Option<GroupLimitation> {
    let relative = evidence
        .classification
        .predicates()
        .iter()
        .any(|(id, _)| RELATIVE_TIME_PREDICATES.contains(&id.as_str()));
    relative.then(|| marker(limitation::TIMEZONE, None))
}

/// Predicates whose meaning depends on the reference time (none registered in 0.0.1).
const RELATIVE_TIME_PREDICATES: &[&str] = &[];

fn ruleset_changed(evidence: &Evidence<'_>) -> Option<GroupLimitation> {
    let used = evidence.meta.ruleset_version;
    let current = evidence.current_ruleset?;
    (used != 0 && used != current).then(|| marker(limitation::RULESET_CHANGED, None))
}

/// Ratio table (F36 FR4.HD1.app-host.DD2): exact pair, undefined, or inexact.
pub(super) fn ratio(
    basis: SizeBasis,
    numerator: &SizeValue,
    denominator: &SizeValue,
) -> GroupRatio {
    fn exact(value: &SizeValue) -> Option<(u64, &du_core_contracts::ByteSize)> {
        match &value.amount {
            SizeAmount::Exact { bytes } => bytes.as_str().parse::<u64>().ok().map(|n| (n, bytes)),
            _ => None,
        }
    }
    match (exact(numerator), exact(denominator)) {
        (_, Some((0, _))) => undefined(basis, "group.ratio_undefined"),
        (Some((_, top)), Some((_, bottom))) => GroupRatio::Value {
            basis,
            numerator: top.clone(),
            denominator: bottom.clone(),
        },
        _ => undefined(basis, "group.ratio_inexact"),
    }
}

fn undefined(basis: SizeBasis, reason: &str) -> GroupRatio {
    GroupRatio::Undefined {
        basis,
        reason_key: reason.into(),
    }
}
