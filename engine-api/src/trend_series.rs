//! Projects one F27 trend history using deletion evidence from its open session.

use crate::trend_decimation::{self, Decimated, TimeRange, TrendPoint};
use du_core_contracts::Id;
use du_snapshot_store::trend::{
    Flag, History, ObservationSource, PointResult, PointValue, SegmentStatus, Trend,
};
use std::collections::HashSet;

/// Snapshot deletions reported by the owner of this open trend revision.
pub struct DeletedSnapshots<'a> {
    /// The exact revision used when collecting the deletion markers.
    pub revision: &'a Id,
    /// Stable identifiers of the deleted historical snapshots.
    pub ids: &'a HashSet<Id>,
}

/// A rejected evidence or projection request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Deletion evidence belongs to another open history.
    RevisionMismatch,
    /// The computed observation has no corresponding source identity.
    MissingSource,
    /// The range or point budget failed the published decimation checks.
    Decimation(trend_decimation::Error),
}

struct InputPoint<'a> {
    point: &'a PointResult,
    deleted: bool,
}

impl TrendPoint for InputPoint<'_> {
    fn observed_at_millis(&self) -> i64 {
        self.point.observed_at_millis
    }

    fn exact_value(&self) -> Option<u64> {
        match self.point.value {
            PointValue::Observed(value) => Some(value),
            PointValue::SizeUnknown | PointValue::Unverifiable(_) => None,
        }
    }

    fn must_preserve(&self) -> bool {
        self.deleted || self.point.flags.contains(&Flag::OrderUncertain)
    }
}

/// Selects original slots for display without modifying the immutable history.
/// An explicit time range must contain the admitted points supplied for projection.
pub fn project(
    history: &History,
    deleted: DeletedSnapshots<'_>,
    budget: usize,
    range: Option<TimeRange>,
) -> Result<Decimated, Error> {
    if deleted.revision != history.revision() {
        return Err(Error::RevisionMismatch);
    }
    let mut points = Vec::new();
    let mut unknown = Vec::new();
    match history.trend() {
        None => {}
        Some(Trend::SingleObservation(point)) => points.push(point),
        Some(Trend::Segments(segments)) => {
            if let Some(first) = segments.first() {
                points.push(&first.before);
            }
            for segment in segments {
                points.push(&segment.after);
                unknown.push(segment.status == SegmentStatus::Unknown);
            }
        }
    }
    let points = points
        .into_iter()
        .map(|point| {
            let source = history
                .source(point.slot_index)
                .ok_or(Error::MissingSource)?;
            let deleted =
                matches!(source, ObservationSource::Snapshot(id) if deleted.ids.contains(id));
            Ok(InputPoint { point, deleted })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let range = range.unwrap_or_else(|| observed_range(&points));
    trend_decimation::decimate(&points, &unknown, budget, range).map_err(Error::Decimation)
}

fn observed_range(points: &[InputPoint<'_>]) -> TimeRange {
    let first = points.first().map_or(0, TrendPoint::observed_at_millis);
    let (start_millis, end_millis) = points.iter().fold((first, first), |(min, max), point| {
        let time = point.observed_at_millis();
        (min.min(time), max.max(time))
    });
    TimeRange {
        start_millis,
        end_millis,
    }
}

#[cfg(test)]
mod tests;
