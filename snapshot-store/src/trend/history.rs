//! A trend revision that owns the source identity for each computed point.

use super::{Point, Trend, classify, segments_from_points};
use du_core_contracts::Id;
use std::collections::HashSet;

/// The recorded source of one observation in a trend history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservationSource {
    /// A historical observation tied to a snapshot's stable identity.
    Snapshot(Id),
    /// The final observation from a completed current result revision.
    CurrentResult(Id),
}

/// A value paired with its recorded source before trend calculation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    /// The source verified when selecting the observation.
    pub source: ObservationSource,
    /// The timestamp and size evidence read from that source.
    pub point: Point,
}

/// Invalid source membership or position within one trend history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryError {
    /// The same snapshot would appear twice in this history.
    DuplicateSnapshot,
    /// A current result appeared before another observation.
    CurrentResultNotLast,
}

/// Owns one trend revision, its computed points and their immutable source identities.
pub struct History {
    revision: Id,
    sources: Vec<ObservationSource>,
    trend: Option<Trend>,
}

impl History {
    /// Calculates an already-admitted observation sequence in its recorded slot order.
    /// The current result, when present, must be the last observation.
    pub fn build(
        revision: Id,
        observations: Vec<Observation>,
        evaluation_time_millis: i64,
    ) -> Result<Self, HistoryError> {
        let count = observations.len();
        let mut sources = Vec::with_capacity(count);
        let mut points = Vec::with_capacity(count);
        let mut snapshots = HashSet::new();
        for (index, observation) in observations.into_iter().enumerate() {
            match &observation.source {
                ObservationSource::Snapshot(id) if !snapshots.insert(id.clone()) => {
                    return Err(HistoryError::DuplicateSnapshot);
                }
                ObservationSource::CurrentResult(_) if index + 1 != count => {
                    return Err(HistoryError::CurrentResultNotLast);
                }
                _ => {}
            }
            points.push(classify(index, &observation.point, evaluation_time_millis));
            sources.push(observation.source);
        }
        Ok(Self {
            revision,
            sources,
            trend: segments_from_points(points),
        })
    }

    /// Returns the identity shared by this history and its session evidence.
    pub fn revision(&self) -> &Id {
        &self.revision
    }

    /// Returns the original source for a point slot without opening its snapshot.
    pub fn source(&self, slot: usize) -> Option<&ObservationSource> {
        self.sources.get(slot)
    }

    /// Returns the point and interval evidence calculated at admission.
    pub fn trend(&self) -> Option<&Trend> {
        self.trend.as_ref()
    }
}
