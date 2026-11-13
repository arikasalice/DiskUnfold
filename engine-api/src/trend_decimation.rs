//! Pure, slot-preserving decimation for trend histories.
//!
//! This module accepts a borrowed view of points instead of owning snapshot or
//! trend contracts. The caller supplies the time range and the already
//! classified point and segment facts.

/// Inclusive time range used to assign points to integer buckets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    /// Inclusive beginning of the range in UTC epoch milliseconds.
    pub start_millis: i64,
    /// Inclusive end of the range in UTC epoch milliseconds.
    pub end_millis: i64,
}

/// Borrowed facts needed from one point in a trend.
pub trait TrendPoint {
    /// Observation time in UTC epoch milliseconds.
    fn observed_at_millis(&self) -> i64;

    /// Exact value used for bucket minima and maxima, if available.
    fn exact_value(&self) -> Option<u64>;

    /// Whether this point must be retained regardless of bucket selection.
    ///
    /// Callers should return true for unverifiable, order-uncertain, and
    /// deleted points, and for any other point the published contract marks
    /// as mandatory. Points with no exact value are preserved automatically.
    fn must_preserve(&self) -> bool;
}

/// Result of projecting a trend onto selected original slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decimated {
    /// Selected zero-based indices in the caller's original slot order.
    pub slot_indices: Vec<usize>,
    /// Unknown status for each adjacent pair in `slot_indices`.
    ///
    /// Each projected value is true if any original segment spanned by that
    /// pair was unknown.
    pub segment_unknown: Vec<bool>,
    /// Whether the decimation rule was applied because input exceeded budget.
    pub decimated: bool,
    /// Number of input points before decimation.
    pub original_count: usize,
    /// Original input point counts per displayed segment, including endpoints.
    pub segment_original_point_count: Vec<usize>,
}

/// Errors in the decimation request or its explicit range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The point budget is outside the published inclusive range 2..=2000.
    InvalidBudget,
    /// The inclusive range start is later than its end.
    InvalidRange,
    /// A point falls outside the explicit inclusive time range.
    PointOutsideRange,
    /// The supplied adjacency flags do not describe all original segments.
    InvalidSegmentCount,
}

/// Selects first, last, minimum, and maximum values in time buckets.
///
/// `budget` is validated even when the input already fits. Points remain in
/// their supplied slot order, including non-monotonic times. Every mandatory
/// point is retained. The range is inclusive and every input point must lie
/// inside it. Minima and maxima use exact values only; points without exact
/// values are automatically preserved.
pub fn decimate<P: TrendPoint>(
    points: &[P],
    segment_unknown: &[bool],
    budget: usize,
    range: TimeRange,
) -> Result<Decimated, Error> {
    if !(2..=2000).contains(&budget) {
        return Err(Error::InvalidBudget);
    }
    if range.start_millis > range.end_millis {
        return Err(Error::InvalidRange);
    }
    if segment_unknown.len() != points.len().saturating_sub(1) {
        return Err(Error::InvalidSegmentCount);
    }
    let mut facts = Vec::with_capacity(points.len());
    for point in points {
        let time = point.observed_at_millis();
        if time < range.start_millis || time > range.end_millis {
            return Err(Error::PointOutsideRange);
        }
        facts.push(PointFacts {
            time,
            value: point.exact_value(),
            preserve: point.must_preserve(),
        });
    }

    let original_count = points.len();
    if original_count <= budget {
        return Ok(Decimated {
            slot_indices: (0..original_count).collect(),
            segment_unknown: segment_unknown.to_vec(),
            decimated: false,
            original_count,
            segment_original_point_count: (0..original_count.saturating_sub(1))
                .map(|_| 2)
                .collect(),
        });
    }

    let mut selected = vec![false; original_count];
    let duration = i128::from(range.end_millis) - i128::from(range.start_millis) + 1;
    let bucket_for = |time: i64| -> usize {
        let offset = i128::from(time) - i128::from(range.start_millis);
        ((offset * budget as i128) / duration) as usize
    };

    // Accumulate by bucket across the entire input. Input order is retained
    // separately, so a later slot that returns to an earlier bucket is merged.
    let mut buckets = vec![Bucket::default(); budget];
    for (slot, point) in facts.iter().enumerate() {
        if point.preserve || point.value.is_none() {
            selected[slot] = true;
        }
        let bucket_index = bucket_for(point.time);
        buckets[bucket_index].observe(slot, point.value);
    }
    for bucket in buckets {
        for slot in [bucket.first, bucket.last, bucket.minimum, bucket.maximum]
            .into_iter()
            .flatten()
        {
            selected[slot] = true;
        }
    }

    let slot_indices = selected
        .iter()
        .enumerate()
        .filter_map(|(slot, keep)| keep.then_some(slot))
        .collect::<Vec<_>>();
    let projected = slot_indices
        .windows(2)
        .map(|pair| {
            (
                segment_unknown[pair[0]..pair[1]]
                    .iter()
                    .any(|unknown| *unknown),
                pair[1] - pair[0] + 1,
            )
        })
        .collect::<Vec<_>>();
    Ok(Decimated {
        segment_unknown: projected.iter().map(|(unknown, _)| *unknown).collect(),
        segment_original_point_count: projected.iter().map(|(_, count)| *count).collect(),
        slot_indices,
        decimated: true,
        original_count,
    })
}

struct PointFacts {
    time: i64,
    value: Option<u64>,
    preserve: bool,
}

#[derive(Clone, Default)]
struct Bucket {
    first: Option<usize>,
    last: Option<usize>,
    minimum: Option<usize>,
    maximum: Option<usize>,
    min_value: Option<u64>,
    max_value: Option<u64>,
}

impl Bucket {
    fn observe(&mut self, slot: usize, value: Option<u64>) {
        let Some(value) = value else { return };
        self.first.get_or_insert(slot);
        self.last = Some(slot);
        if self.min_value.is_none_or(|current| value < current) {
            self.min_value = Some(value);
            self.minimum = Some(slot);
        }
        if self.max_value.is_none_or(|current| value > current) {
            self.max_value = Some(value);
            self.maximum = Some(slot);
        }
    }
}

#[cfg(test)]
mod tests;
