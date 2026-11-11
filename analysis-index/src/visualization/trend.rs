//! Pure geometry for caller-supplied, already-decimated trend observations.
//!
//! The caller owns source selection, time bucketing, and decimation. This
//! strategy places only supplied observations: it preserves their timestamp
//! spacing, leaves unknown observations as gaps, and never interpolates values.
use super::geometry::{Rect, Weight, valid_bounds};
use std::collections::HashSet;

/// A value observed at a Unix timestamp expressed in milliseconds.
///
/// `Weight::Unknown` represents an observation gap. A lower bound remains a
/// positioned lower-bound observation; it is not changed into an exact value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point<V = Weight> {
    pub observed_at_ms: i64,
    pub value: V,
}

/// One pre-decimated series in strictly increasing timestamp order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series<V = Weight> {
    pub series_no: u32,
    pub points: Vec<Point<V>>,
}

/// Integer coordinate of an observed point within the supplied bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coordinate {
    pub x: u32,
    pub y: u32,
}

/// A line between two consecutive known observations in one series.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSegment {
    pub from: Coordinate,
    pub to: Coordinate,
    /// Index of the earlier observation in the owning series.
    pub from_sample: usize,
    /// Index of the later observation in the owning series.
    pub to_sample: usize,
}

/// An input observation and its optional geometry. Unknown points have no y
/// coordinate and split line runs; no synthetic points are returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample<V = Weight> {
    pub observed_at_ms: i64,
    pub value: V,
    /// Horizontal position is retained even when the observation is unknown.
    pub x: u32,
    pub coordinate: Option<Coordinate>,
}

/// Positioned observations and line segments for one caller-supplied series.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesLayout<V = Weight> {
    pub series_no: u32,
    pub samples: Vec<Sample<V>>,
    pub segments: Vec<LineSegment>,
}

/// Point geometries retain their sample index; lines connect only observed runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesGeometry {
    pub points: Vec<(usize, du_core_contracts::visualization::geometry::Geometry)>,
    pub lines: Vec<du_core_contracts::visualization::geometry::Geometry>,
}

impl<V> SeriesLayout<V> {
    /// Converts existing samples and line segments to geometry v1.
    /// Coordinates must already be on the normalized grid. Unknown samples
    /// have no point and the original line gaps are preserved.
    pub fn geometry_v1(
        &self,
    ) -> Result<SeriesGeometry, du_core_contracts::visualization::geometry::GeometryError> {
        use du_core_contracts::visualization::geometry::{Geometry, Point};
        let convert = |coordinate: Coordinate| Point {
            x: coordinate.x,
            y: coordinate.y,
        };
        let points = self
            .samples
            .iter()
            .enumerate()
            .filter_map(|(index, sample)| sample.coordinate.map(|coordinate| (index, coordinate)))
            .map(|(index, coordinate)| {
                Ok((index, Geometry::new("point", vec![convert(coordinate)])?))
            })
            .collect::<Result<_, _>>()?;
        let lines = self
            .segments
            .iter()
            .map(|segment| {
                Geometry::new("polyline", vec![convert(segment.from), convert(segment.to)])
            })
            .collect::<Result<_, _>>()?;
        Ok(SeriesGeometry { points, lines })
    }
}

/// Inclusive numeric extent used for the shared vertical axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueRange<N = u64> {
    pub minimum: N,
    pub maximum: N,
}

/// Trend geometry aligned to one shared time and value domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trend<V = Weight, N = u64> {
    pub time_range_ms: Option<(i64, i64)>,
    /// `None` means every supplied observation is unknown.
    pub value_range: Option<ValueRange<N>>,
    pub series: Vec<SeriesLayout<V>>,
}

/// Invalid geometry, ambiguous input, or a cancelled calculation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidBounds,
    DuplicateSeries,
    NonIncreasingTime,
    Cancelled,
}

/// Places supplied trend observations in a shared plot rectangle.
///
/// All timestamps across all series share one horizontal axis, so their actual
/// temporal spacing is retained. Known values share one vertical axis. A
/// constant value domain is placed at the vertical midpoint. Unknown values
/// retain their timestamp as a gap and prevent a line from crossing that gap.
/// Input series order and point order are preserved for deterministic output.
pub fn layout(bounds: Rect, input: &[Series]) -> Result<Trend, Error> {
    layout_cancellable(bounds, input, &mut || false)
}

/// Places the observations, checking cancellation during validation and placement.
///
/// Cancellation discards the entire candidate, including any positioned series.
pub fn layout_cancellable(
    bounds: Rect,
    input: &[Series],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Trend, Error> {
    layout_values(
        bounds,
        input,
        |value| match value {
            Weight::Exact(value) | Weight::LowerBound(value) => Some(*value),
            Weight::Unknown => None,
        },
        cancelled,
    )
}

/// Positions numeric evidence without changing the original value in any sample.
pub(super) fn layout_values<V: Clone, N: Copy + Ord + Into<i128>>(
    bounds: Rect,
    input: &[Series<V>],
    value_of: impl Fn(&V) -> Option<N>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Trend<V, N>, Error> {
    check_cancelled(cancelled)?;
    if !valid_bounds(bounds) {
        return Err(Error::InvalidBounds);
    }
    let mut seen = HashSet::new();
    let mut time_min = None;
    let mut time_max = None;
    let mut value_min = None;
    let mut value_max = None;
    for series in input {
        check_cancelled(cancelled)?;
        if !seen.insert(series.series_no) {
            return Err(Error::DuplicateSeries);
        }
        let mut previous = None;
        for point in &series.points {
            check_cancelled(cancelled)?;
            if previous.is_some_and(|time| point.observed_at_ms <= time) {
                return Err(Error::NonIncreasingTime);
            }
            previous = Some(point.observed_at_ms);
            time_min =
                Some(time_min.map_or(point.observed_at_ms, |v: i64| v.min(point.observed_at_ms)));
            time_max =
                Some(time_max.map_or(point.observed_at_ms, |v: i64| v.max(point.observed_at_ms)));
            if let Some(value) = value_of(&point.value) {
                value_min = Some(value_min.map_or(value, |v: N| v.min(value)));
                value_max = Some(value_max.map_or(value, |v: N| v.max(value)));
            }
        }
    }
    let time_range_ms = time_min.zip(time_max);
    let value_range = value_min
        .zip(value_max)
        .map(|(minimum, maximum)| ValueRange { minimum, maximum });
    let series = input
        .iter()
        .map(|series| {
            check_cancelled(cancelled)?;
            let mut samples = Vec::with_capacity(series.points.len());
            let mut segments = Vec::new();
            let mut previous_known = None;
            for (index, point) in series.points.iter().enumerate() {
                check_cancelled(cancelled)?;
                let x = map_time(bounds, point.observed_at_ms, time_range_ms);
                let coordinate = match value_of(&point.value) {
                    None => {
                        previous_known = None;
                        None
                    }
                    Some(value) => {
                        let coordinate = Coordinate {
                            x,
                            y: map_value(bounds, value, value_range),
                        };
                        if let Some((from_sample, from)) = previous_known {
                            segments.push(LineSegment {
                                from,
                                to: coordinate,
                                from_sample,
                                to_sample: index,
                            });
                        }
                        previous_known = Some((index, coordinate));
                        Some(coordinate)
                    }
                };
                samples.push(Sample {
                    observed_at_ms: point.observed_at_ms,
                    value: point.value.clone(),
                    x,
                    coordinate,
                });
            }
            Ok(SeriesLayout {
                series_no: series.series_no,
                samples,
                segments,
            })
        })
        .collect::<Result<_, _>>()?;
    check_cancelled(cancelled)?;
    Ok(Trend {
        time_range_ms,
        value_range,
        series,
    })
}

fn check_cancelled(cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(())
}

fn map_time(bounds: Rect, timestamp: i64, range: Option<(i64, i64)>) -> u32 {
    let Some((minimum, maximum)) = range else {
        return bounds.left;
    };
    if minimum == maximum {
        return bounds.left + (bounds.right - bounds.left) / 2;
    }
    let offset = (i128::from(timestamp) - i128::from(minimum)) as u128;
    let span = (i128::from(maximum) - i128::from(minimum)) as u128;
    bounds.left + proportional(bounds.right - bounds.left, offset, span)
}

fn map_value<N: Copy + Ord + Into<i128>>(
    bounds: Rect,
    value: N,
    range: Option<ValueRange<N>>,
) -> u32 {
    let Some(range) = range else {
        return bounds.top + (bounds.bottom - bounds.top) / 2;
    };
    let height = bounds.bottom - bounds.top;
    if range.minimum == range.maximum {
        return bounds.top + height / 2;
    }
    let offset = (value.into() - range.minimum.into()) as u128;
    let span = (range.maximum.into() - range.minimum.into()) as u128;
    bounds.bottom - proportional(height, offset, span)
}

fn proportional(length: u32, value: u128, total: u128) -> u32 {
    let product = u128::from(length) * value;
    (product / total) as u32
}
