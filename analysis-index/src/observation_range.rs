//! Observation-time evidence for immutable subtree and scope aggregates.
use std::cmp::Ordering;

use du_core_contracts::{
    Timestamp,
    result::{ResultCompleteness, ResultItemKind, SpaceAggregate},
};

/// Bounds of the timestamps actually present, and whether they cover the entire scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObservationRange<'a> {
    /// Earliest retained item observation time, if any.
    pub min: Option<&'a Timestamp>,
    /// Latest retained item observation time, if any.
    pub max: Option<&'a Timestamp>,
    /// True only when every item has a time and no scope coverage is missing.
    pub complete: bool,
}

/// Bounds reference the retained item timestamps rather than duplicating their strings.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RangeIndices {
    min: Option<u32>,
    max: Option<u32>,
    complete: bool,
}

impl RangeIndices {
    pub(crate) fn selection(complete: bool) -> Self {
        Self {
            complete,
            ..Self::default()
        }
    }

    pub(crate) fn observe_file(&mut self, item_no: u32, times: Option<&[Option<Timestamp>]>) {
        let observed = get_time(times, Some(item_no)).is_some();
        self.merge(
            Self {
                min: observed.then_some(item_no),
                max: observed.then_some(item_no),
                complete: observed,
            },
            times,
        );
    }

    pub(crate) fn mark_incomplete(&mut self) {
        self.complete = false;
    }

    fn merge(&mut self, other: Self, times: Option<&[Option<Timestamp>]>) {
        self.min = select(self.min, other.min, times, Ordering::Greater);
        self.max = select(self.max, other.max, times, Ordering::Less);
        self.complete &= other.complete;
    }

    pub(crate) fn resolve(self, times: Option<&[Option<Timestamp>]>) -> ObservationRange<'_> {
        ObservationRange {
            min: get_time(times, self.min),
            max: get_time(times, self.max),
            complete: self.complete,
        }
    }
}

fn get_time(times: Option<&[Option<Timestamp>]>, index: Option<u32>) -> Option<&Timestamp> {
    times?.get(index? as usize)?.as_ref()
}

fn select(
    left: Option<u32>,
    right: Option<u32>,
    times: Option<&[Option<Timestamp>]>,
    replace_on: Ordering,
) -> Option<u32> {
    match (left, right) {
        (Some(a), Some(b)) => match (get_time(times, Some(a)), get_time(times, Some(b))) {
            (Some(a_time), Some(b_time)) if compare(a_time, b_time) == replace_on => Some(b),
            _ => Some(a),
        },
        (Some(_), None) => left,
        (None, _) => right,
    }
}

// Timestamp validation requires exactly three fractional digits and UTC Z. RFC 3339
// accepts T, t or a space at byte 10, so exclude that non-semantic separator.
fn compare(a: &Timestamp, b: &Timestamp) -> Ordering {
    let a = a.as_str().as_bytes();
    let b = b.as_str().as_bytes();
    a[..10].cmp(&b[..10]).then_with(|| a[11..].cmp(&b[11..]))
}

pub(crate) fn build(
    built: &crate::tree::BuiltItems,
    aggregates: &[SpaceAggregate],
    completeness: ResultCompleteness,
) -> (Vec<RangeIndices>, RangeIndices) {
    let items = &built.items;
    let times = built.item_observed_times.as_deref();
    let mut ranges = vec![
        RangeIndices {
            complete: true,
            ..RangeIndices::default()
        };
        items.len()
    ];
    for item in items.iter().rev() {
        let index = item.item_no as usize;
        let time = get_time(times, Some(item.item_no));
        let mut own = RangeIndices {
            min: time.map(|_| item.item_no),
            max: time.map(|_| item.item_no),
            complete: time.is_some()
                && !matches!(
                    item.kind,
                    ResultItemKind::UnknownStructure | ResultItemKind::UntraversedLink
                ),
        };
        own.merge(ranges[index], times);
        ranges[index] = own;
        if let Some(parent) = item.parent {
            ranges[parent as usize].merge(own, times);
        }
    }
    let mut scope: Option<RangeIndices> = None;
    for item in items.iter().filter(|item| item.parent.is_none()) {
        let range = ranges[item.item_no as usize];
        if let Some(value) = &mut scope {
            value.merge(range, times);
        } else {
            scope = Some(range);
        }
    }
    // The aggregator uses None for no recorded gap; Some(count) is set only by
    // the gap marking pass. It is not a general proof of external scan coverage.
    for (range, aggregate) in ranges.iter_mut().zip(aggregates) {
        range.complete &= completeness == ResultCompleteness::Complete
            && aggregate.inaccessible_ranges.is_none_or(|count| count == 0);
    }
    let mut scope = scope.unwrap_or_default();
    scope.complete &= completeness == ResultCompleteness::Complete
        && aggregates
            .iter()
            .all(|a| a.inaccessible_ranges.is_none_or(|count| count == 0));
    (ranges, scope)
}
