//! F27 single-observation and adjacent-snapshot trend segments.

use crate::comparison::Gap;
mod history;
pub use history::{History, HistoryError, Observation, ObservationSource};

/// Rule identifier for the published F27 trend segment.
pub const TREND_SEGMENT_RULE_ID: &str = "trend.segment";
/// Revision of the trend segment rule.
pub const TREND_SEGMENT_RULE_REVISION: u32 = 1;

/// Evidence for one normalized path at one snapshot time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointEvidence {
    /// Exact observed byte count, including zero.
    Exact(u64),
    /// A lower bound does not establish an exact size.
    LowerBound,
    /// A present entry has no usable size.
    UnknownSize,
    /// An already-resolved F08 unverifiability reason.
    Gap(Gap),
    /// The entry is absent and its parent was completely observed.
    AbsentComplete,
    /// The path was outside the requested scan scope.
    OutsideScope,
    /// The path was excluded from observation.
    Excluded,
    /// Access to the path was denied.
    AccessDenied,
    /// The required observation is missing.
    Missing,
}

/// A point supplied in the caller's already-fixed snapshot slot order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    /// UTC epoch milliseconds from the validated snapshot record.
    pub observed_at_millis: i64,
    /// Exact, absent, or already-resolved F08 gap evidence.
    pub evidence: PointEvidence,
}

/// Point classification without interpolation or extrapolation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointValue {
    /// Exact observed bytes; complete-parent absence is represented as zero.
    Observed(u64),
    /// Present evidence was lower-bound or lacked a usable size.
    SizeUnknown,
    /// F08 coverage or identity evidence prevents verification.
    Unverifiable(Gap),
}

impl PointValue {
    /// Returns the stable `trend.point.*` reason key when this value has one.
    ///
    /// F08 reasons without a corresponding trend-point key return `None`.
    #[must_use]
    pub fn reason_key(self) -> Option<&'static str> {
        match self {
            Self::SizeUnknown => Some("trend.point.size_unknown"),
            Self::Unverifiable(Gap::OutsideScope) => Some("trend.point.out_of_scope"),
            Self::Unverifiable(Gap::Excluded) => Some("trend.point.excluded"),
            Self::Unverifiable(Gap::AccessDenied) => Some("trend.point.denied"),
            Self::Unverifiable(Gap::MissingObservation) => Some("trend.point.missing"),
            Self::Observed(_) | Self::Unverifiable(_) => None,
        }
    }
}

/// Flags attached to a point or segment result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    /// Observation time is later than the evaluation time.
    OrderUncertain,
}

/// One input point's result. A one-point history returns exactly one of these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointResult {
    /// Zero-based index in the caller's original snapshot slot list.
    ///
    /// Use this index to recover the source snapshot identity; this module does
    /// not copy or redefine that identity.
    pub slot_index: usize,
    /// UTC epoch milliseconds of this observation.
    pub observed_at_millis: i64,
    /// Value status from this point alone.
    pub value: PointValue,
    /// Flags raised by the evaluation-time check.
    pub flags: Vec<Flag>,
}

/// Comparison of two adjacent points in caller-provided slot order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// Earlier slot endpoint.
    pub before: PointResult,
    /// Later slot endpoint.
    pub after: PointResult,
    /// Signed elapsed milliseconds in input slot order.
    pub interval_millis: i128,
    /// Whether both endpoint sizes are exact and comparable.
    pub status: SegmentStatus,
    /// Signed byte difference, available only when both points are exact.
    pub delta_bytes: Option<i128>,
    /// Signed bytes per 86,400,000 ms, truncated toward zero.
    pub bytes_per_day: Option<i128>,
}

/// Comparability of one adjacent interval's endpoint values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentStatus {
    /// Both endpoint values are exact or complete-parent absences.
    Comparable,
    /// At least one endpoint lacks an exact value or has an F08 gap.
    Unknown,
}

/// Result for one point or the adjacent segments of a history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trend {
    /// One snapshot, with no interpolation or rate.
    SingleObservation(PointResult),
    /// All adjacent intervals in the original slot order.
    Segments(Vec<Segment>),
}

/// Builds point or adjacent-segment trend results without reordering snapshots.
///
/// Snapshot slots are ordered upstream by `(observed_at, snapshot_id)`; an
/// optional current result is appended afterward, even if its time is earlier.
/// This module preserves the supplied order, including reverse or equal-time intervals.
/// `evaluation_time_millis` is compared to every observation time. Future
/// observations are retained and marked `OrderUncertain`. Gaps must already be
/// resolved by the F08 caller, including its documented reason precedence.
#[must_use]
pub fn segment(points: &[Point], evaluation_time_millis: i64) -> Option<Trend> {
    let results = points
        .iter()
        .enumerate()
        .map(|(slot_index, point)| classify(slot_index, point, evaluation_time_millis))
        .collect();
    segments_from_points(results)
}

fn segments_from_points(mut results: Vec<PointResult>) -> Option<Trend> {
    match results.len() {
        0 => None,
        1 => results.pop().map(Trend::SingleObservation),
        _ => Some(Trend::Segments(
            results
                .windows(2)
                .map(|pair| make_segment(pair[0].clone(), pair[1].clone()))
                .collect(),
        )),
    }
}

fn classify(slot_index: usize, point: &Point, evaluation_time_millis: i64) -> PointResult {
    let value = match point.evidence {
        PointEvidence::Exact(bytes) => PointValue::Observed(bytes),
        PointEvidence::AbsentComplete => PointValue::Observed(0),
        PointEvidence::LowerBound | PointEvidence::UnknownSize => PointValue::SizeUnknown,
        PointEvidence::Gap(gap) => PointValue::Unverifiable(gap),
        PointEvidence::OutsideScope => PointValue::Unverifiable(Gap::OutsideScope),
        PointEvidence::Excluded => PointValue::Unverifiable(Gap::Excluded),
        PointEvidence::AccessDenied => PointValue::Unverifiable(Gap::AccessDenied),
        PointEvidence::Missing => PointValue::Unverifiable(Gap::MissingObservation),
    };
    let flags = if point.observed_at_millis > evaluation_time_millis {
        vec![Flag::OrderUncertain]
    } else {
        Vec::new()
    };
    PointResult {
        slot_index,
        observed_at_millis: point.observed_at_millis,
        value,
        flags,
    }
}

fn make_segment(before: PointResult, after: PointResult) -> Segment {
    let delta_bytes = match (before.value, after.value) {
        (PointValue::Observed(left), PointValue::Observed(right)) => {
            Some(i128::from(right) - i128::from(left))
        }
        _ => None,
    };
    let interval_millis =
        i128::from(after.observed_at_millis) - i128::from(before.observed_at_millis);
    let has_future =
        before.flags.contains(&Flag::OrderUncertain) || after.flags.contains(&Flag::OrderUncertain);
    let bytes_per_day = rate(delta_bytes, interval_millis, has_future);
    Segment {
        before,
        after,
        interval_millis,
        status: if delta_bytes.is_some() {
            SegmentStatus::Comparable
        } else {
            SegmentStatus::Unknown
        },
        delta_bytes,
        bytes_per_day,
    }
}

fn rate(delta_bytes: Option<i128>, interval_millis: i128, has_future: bool) -> Option<i128> {
    if has_future || interval_millis <= 0 {
        return None;
    }
    delta_bytes.and_then(|delta| delta.checked_mul(86_400_000)?.checked_div(interval_millis))
}

#[cfg(test)]
mod tests;
