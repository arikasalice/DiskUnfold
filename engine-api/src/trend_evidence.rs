//! Assemble revision-pinned trend evidence from the immutable trend history.
//!
//! This module exposes only values present in [`History`]. The current model does
//! not carry path scope, observation completeness, storage correspondence, or
//! F08 comparison results, so those fields cannot be assembled here.

use crate::{trend_decimation::TimeRange, trend_series, trend_series::DeletedSnapshots};
use du_core_contracts::Id;
use du_snapshot_store::trend::{
    Flag, History, ObservationSource, PointResult, PointValue, Segment, SegmentStatus, Trend,
};

/// Explanation keys emitted for facts already present in the selected history.
pub mod guidance {
    /// The application did not observe the cause between observations.
    pub const CAUSE_NOT_OBSERVED: &str = "trend.explain.cause_not_observed";
    /// A comparable interval has a stored observation-to-observation delta.
    pub const OBSERVED_DELTA: &str = "trend.explain.observed_delta";
    /// A point or interval is unverifiable from its stored observations.
    pub const UNVERIFIABLE: &str = "trend.explain.unverifiable";
    /// The history contains one observation and no interval.
    pub const SINGLE_OBSERVATION: &str = "trend.explain.single_observation";
    /// At least one selected observation is later than the evaluation time.
    pub const ORDER_UNCERTAIN: &str = "trend.explain.order_uncertain";
}

/// The original source category stored with one trend point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// A point copied from a historical snapshot.
    HistoricalObservation,
    /// A point copied from the completed current result.
    CurrentResult,
}

/// A point reference in the projected series.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Select a point by its stable original slot index.
    Point { slot_index: usize },
    /// Select a displayed interval by its stable original endpoint slots.
    Segment { first_slot: usize, last_slot: usize },
}

/// One source point and the facts copied into its trend history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointEvidence {
    /// Whether this came from an older snapshot or the current result.
    pub source_kind: SourceKind,
    /// Stable ID of the source already recorded in the history.
    pub source_id: Id,
    /// UTC epoch milliseconds recorded with the point.
    pub observed_at_millis: i64,
    /// Stored value classification; this does not trigger a new observation.
    pub value: PointValue,
    /// Stored point flags, including future-time uncertainty.
    pub flags: Vec<Flag>,
    /// True only when this is a historical snapshot marked deleted for this revision.
    pub deleted: bool,
    /// Stable point reason key, when the stored classification has one.
    pub reason_key: Option<&'static str>,
}

/// Facts for one original adjacent pair in the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalSegmentEvidence {
    /// Evidence from the earlier slot in the original input order.
    pub before: PointEvidence,
    /// Evidence from the later slot in the original input order.
    pub after: PointEvidence,
    /// Signed elapsed time in the original input order.
    pub interval_millis: i128,
    /// Comparability copied from the original trend segment.
    pub status: SegmentStatus,
    /// Stored signed byte difference, when both original endpoints are exact.
    pub delta_bytes: Option<i128>,
    /// Stored bytes-per-day value; unavailable when the source interval is unsafe.
    pub bytes_per_day: Option<i128>,
}

/// Evidence for one displayed interval, expanded to its original adjacent segments.
///
/// A decimated interval can cover several original intervals. `segments` preserves
/// each original pair and its status; this structure intentionally has no aggregate
/// delta or rate for the projected endpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedSegmentEvidence {
    /// First original slot represented by this displayed interval.
    pub first_slot: usize,
    /// Last original slot represented by this displayed interval.
    pub last_slot: usize,
    /// Original point count including both endpoints.
    pub original_point_count: usize,
    /// True when any original interval in this span is unverifiable.
    pub has_unverifiable_segment: bool,
    /// Every original adjacent pair, in original slot order.
    pub segments: Vec<OriginalSegmentEvidence>,
}

/// Point or interval evidence tied to one immutable trend revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    /// The history revision from which all included values were read.
    pub revision: Id,
    /// Evidence for the selected projected point or interval.
    pub target: EvidenceTarget,
    /// All applicable explanation keys, including cause-not-observed.
    pub guidance_keys: Vec<&'static str>,
}

/// Supported point and interval evidence from the current history model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceTarget {
    /// One point from the recomputed projected series.
    Point(PointEvidence),
    /// One displayed interval expanded into original adjacent pairs.
    Segment(ProjectedSegmentEvidence),
}

/// A selection or projection could not be resolved from this history revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Trend projection failed, including a deletion set from another revision.
    Projection(trend_series::Error),
    /// The requested selection was created for a different history revision.
    RevisionMismatch,
    /// The selected projected point index is not present.
    PointNotFound,
    /// The selected projected segment index is not present.
    SegmentNotFound,
    /// Stored history slots do not form the expected original adjacent sequence.
    InvalidHistory,
    /// The recomputed projection metadata does not match its original slot span.
    InvalidProjection,
}

/// Recomputes the display projection and assembles evidence for a selected point or interval.
///
/// This accepts the same budget and optional range as the series request, but it
/// does not accept a caller-supplied projection or caller-supplied status flags.
/// The original result reference must match `expected_revision`. The projection
/// is derived from `history`, and deletion markers must carry the same revision.
/// No snapshot or filesystem port is called.
pub fn assemble(
    history: &History,
    expected_revision: &Id,
    deleted: DeletedSnapshots<'_>,
    budget: usize,
    range: Option<TimeRange>,
    target: Target,
) -> Result<Evidence, Error> {
    if expected_revision != history.revision() {
        return Err(Error::RevisionMismatch);
    }
    let deletion_revision = deleted.revision;
    let deleted_ids = deleted.ids;
    let projection =
        trend_series::project(history, deleted, budget, range).map_err(Error::Projection)?;
    let deleted = DeletedSnapshots {
        revision: deletion_revision,
        ids: deleted_ids,
    };
    let (target, mut keys) = match target {
        Target::Point { slot_index } => {
            let slot = projection
                .slot_indices
                .iter()
                .copied()
                .find(|slot| *slot == slot_index)
                .ok_or(Error::PointNotFound)?;
            let point = point_for_slot(history, slot, &deleted)?;
            let mut keys = Vec::new();
            if matches!(history.trend(), Some(Trend::SingleObservation(_))) {
                push_key(&mut keys, guidance::SINGLE_OBSERVATION);
            }
            add_point_keys(&mut keys, &point);
            (EvidenceTarget::Point(point), keys)
        }
        Target::Segment {
            first_slot,
            last_slot,
        } => {
            let projected_index = projection
                .slot_indices
                .windows(2)
                .position(|pair| pair == [first_slot, last_slot])
                .ok_or(Error::SegmentNotFound)?;
            let point_count = last_slot
                .checked_sub(first_slot)
                .and_then(|count| count.checked_add(1))
                .ok_or(Error::InvalidProjection)?;
            if projection.segment_original_point_count.get(projected_index) != Some(&point_count) {
                return Err(Error::InvalidProjection);
            }
            let original = original_segments(history).ok_or(Error::InvalidHistory)?;
            let source_segments = original
                .get(first_slot..last_slot)
                .ok_or(Error::InvalidProjection)?;
            let mut segments = Vec::with_capacity(source_segments.len());
            for (offset, segment) in source_segments.iter().enumerate() {
                let before_slot = first_slot + offset;
                if segment.before.slot_index != before_slot
                    || segment.after.slot_index != before_slot + 1
                {
                    return Err(Error::InvalidHistory);
                }
                segments.push(original_segment_evidence(history, &deleted, segment)?);
            }
            let has_unverifiable_segment = segments
                .iter()
                .any(|segment| segment.status == SegmentStatus::Unknown);
            if projection.segment_unknown.get(projected_index) != Some(&has_unverifiable_segment) {
                return Err(Error::InvalidProjection);
            }
            let mut keys = Vec::new();
            if has_unverifiable_segment {
                push_key(&mut keys, guidance::UNVERIFIABLE);
            } else {
                push_key(&mut keys, guidance::OBSERVED_DELTA);
            }
            for segment in &segments {
                add_point_keys(&mut keys, &segment.before);
                add_point_keys(&mut keys, &segment.after);
            }
            (
                EvidenceTarget::Segment(ProjectedSegmentEvidence {
                    first_slot,
                    last_slot,
                    original_point_count: point_count,
                    has_unverifiable_segment,
                    segments,
                }),
                keys,
            )
        }
    };
    push_key(&mut keys, guidance::CAUSE_NOT_OBSERVED);
    Ok(Evidence {
        revision: history.revision().clone(),
        target,
        guidance_keys: keys,
    })
}

fn original_segments(history: &History) -> Option<&[Segment]> {
    match history.trend()? {
        Trend::SingleObservation(_) => Some(&[]),
        Trend::Segments(segments) => Some(segments),
    }
}

fn point_for_slot(
    history: &History,
    slot: usize,
    deleted: &DeletedSnapshots<'_>,
) -> Result<PointEvidence, Error> {
    let point = point_result_for_slot(history, slot).ok_or(Error::InvalidHistory)?;
    point_evidence(history, deleted, point)
}

fn point_result_for_slot(history: &History, slot: usize) -> Option<&PointResult> {
    match history.trend()? {
        Trend::SingleObservation(point) => (point.slot_index == slot).then_some(point),
        Trend::Segments(segments) => segments.iter().find_map(|segment| {
            if segment.before.slot_index == slot {
                Some(&segment.before)
            } else if segment.after.slot_index == slot {
                Some(&segment.after)
            } else {
                None
            }
        }),
    }
}

fn original_segment_evidence(
    history: &History,
    deleted: &DeletedSnapshots<'_>,
    segment: &Segment,
) -> Result<OriginalSegmentEvidence, Error> {
    Ok(OriginalSegmentEvidence {
        before: point_evidence(history, deleted, &segment.before)?,
        after: point_evidence(history, deleted, &segment.after)?,
        interval_millis: segment.interval_millis,
        status: segment.status,
        delta_bytes: segment.delta_bytes,
        bytes_per_day: segment.bytes_per_day,
    })
}

fn point_evidence(
    history: &History,
    deleted: &DeletedSnapshots<'_>,
    point: &PointResult,
) -> Result<PointEvidence, Error> {
    let source = history
        .source(point.slot_index)
        .ok_or(Error::Projection(trend_series::Error::MissingSource))?;
    let (source_kind, source_id, is_deleted) = match source {
        ObservationSource::Snapshot(id) => (
            SourceKind::HistoricalObservation,
            id.clone(),
            deleted.ids.contains(id),
        ),
        ObservationSource::CurrentResult(id) => (SourceKind::CurrentResult, id.clone(), false),
    };
    Ok(PointEvidence {
        source_kind,
        source_id,
        observed_at_millis: point.observed_at_millis,
        value: point.value,
        flags: point.flags.clone(),
        deleted: is_deleted,
        reason_key: point.value.reason_key(),
    })
}

fn add_point_keys(keys: &mut Vec<&'static str>, point: &PointEvidence) {
    match point.value {
        PointValue::SizeUnknown | PointValue::Unverifiable(_) => {
            push_key(keys, guidance::UNVERIFIABLE);
        }
        PointValue::Observed(_) => {}
    }
    if point.flags.contains(&Flag::OrderUncertain) {
        push_key(keys, guidance::ORDER_UNCERTAIN);
    }
}

fn push_key(keys: &mut Vec<&'static str>, key: &'static str) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

#[cfg(test)]
mod tests;
