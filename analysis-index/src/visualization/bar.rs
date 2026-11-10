//! Pure ranked and stacked bar layout over caller-supplied numeric facts.
//!
//! These local models are not the serialized F06 contract. The caller owns the
//! pinned source, size basis, shared-space attribution, and legend definitions.
use super::squarify::{Rect, Weight};
use du_core_contracts::result::ResultCompleteness;
use std::collections::{BTreeMap, HashSet};

/// An item with its source value; `item_no` is unique within the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub item_no: u32,
    pub value: Weight,
}

/// An individual item or a deterministic group of remaining known items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarRef {
    Item(u32),
    Other(Vec<u32>),
}

/// Why a row or segment has no drawable area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoArea {
    Unknown,
    Zero,
    CoordinatePrecision,
    UncertainOffset,
    UncertainScale,
}

/// Uses an exact automatic maximum or a caller-confirmed fixed axis maximum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Automatic,
    Fixed(u64),
}

/// A bar, its original value state, and its optional integer rectangle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub reference: BarRef,
    pub value: Weight,
    pub rect: Option<Rect>,
    pub no_area: Option<NoArea>,
    /// The grouped amount exceeded u64; the displayed value is unknown.
    pub overflowed: bool,
}

/// Ranked known values and separately retained unknown items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranking {
    pub bars: Vec<Bar>,
    pub unknown_items: Vec<u32>,
    pub total: Weight,
    pub ranking_incomplete: bool,
    /// The established axis maximum, or `None` when it cannot be established.
    pub axis_max: Option<u64>,
    /// Completeness supplied by the pinned source for this input scope.
    pub source_completeness: ResultCompleteness,
    pub total_overflowed: bool,
}

/// Invalid calculation inputs. Arithmetic overflow in observed values is instead
/// represented by an unknown aggregate, without saturation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidBounds,
    DuplicateItem,
    DuplicateLegend,
    LegendMismatch,
    InvalidAxis,
}

/// Ranks known items descending by their reported numeric value, breaking ties by
/// stable item number. Unknown items are never ranked as zero. `top` controls
/// the number of known items before a single Other group; zero groups all known
/// items. Lower bounds make the resulting ranking incomplete. An automatic
/// scale is unavailable if any contributing value is not exact.
pub fn ranking(
    bounds: Rect,
    input: &[Item],
    top: usize,
    scale: Scale,
    completeness: ResultCompleteness,
) -> Result<Ranking, Error> {
    validate_bounds(bounds)?;
    let mut seen = HashSet::new();
    let mut known = Vec::new();
    let mut unknown_items = Vec::new();
    let mut total = Sum::default();
    let mut ranking_incomplete = completeness != ResultCompleteness::Complete;
    if ranking_incomplete {
        total.add(Weight::Unknown);
    }
    for item in input {
        if !seen.insert(item.item_no) {
            return Err(Error::DuplicateItem);
        }
        total.add(item.value);
        match item.value {
            Weight::Unknown => {
                unknown_items.push(item.item_no);
                ranking_incomplete = true;
            }
            Weight::LowerBound(_) => {
                known.push(*item);
                ranking_incomplete = true;
            }
            Weight::Exact(_) => known.push(*item),
        }
    }
    known.sort_by(|a, b| {
        amount(b.value)
            .cmp(&amount(a.value))
            .then_with(|| a.item_no.cmp(&b.item_no))
    });
    unknown_items.sort_unstable();
    let split = top.min(known.len());
    let mut bars: Vec<_> = known[..split]
        .iter()
        .map(|item| Bar {
            reference: BarRef::Item(item.item_no),
            value: item.value,
            rect: None,
            no_area: None,
            overflowed: false,
        })
        .collect();
    if split < known.len() {
        let mut other = Sum::default();
        let mut members = Vec::new();
        for item in &known[split..] {
            other.add(item.value);
            members.push(item.item_no);
        }
        bars.push(Bar {
            reference: BarRef::Other(members),
            value: other.finish(),
            rect: None,
            no_area: None,
            overflowed: other.overflowed(),
        });
    }
    let mut axis = resolve_axis(
        scale,
        input
            .iter()
            .map(|item| item.value)
            .chain(bars.iter().map(|bar| bar.value)),
    )?;
    if completeness != ResultCompleteness::Complete && scale == Scale::Automatic {
        axis = None;
    }
    let count = bars.len();
    for (index, bar) in bars.iter_mut().enumerate() {
        (bar.rect, bar.no_area) = row_rect(bounds, index, count, bar.value, axis);
    }
    Ok(Ranking {
        bars,
        unknown_items,
        total: total.finish(),
        ranking_incomplete,
        axis_max: axis,
        source_completeness: completeness,
        total_overflowed: total.overflowed(),
    })
}

/// One value associated with a legend classification key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub legend_key: u32,
    pub value: Weight,
}

/// Segment output in legend order. An unknown or lower-bound predecessor makes
/// later offsets unknown, so those later segments do not receive rectangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentLayout {
    pub legend_key: u32,
    pub value: Weight,
    pub rect: Option<Rect>,
    pub no_area: Option<NoArea>,
}

/// A stacked bar and its conservatively combined value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack {
    pub segments: Vec<SegmentLayout>,
    pub total: Weight,
    pub total_overflowed: bool,
}

/// Orders every supplied segment by `legend` and uses the caller's fixed axis.
/// Missing or duplicate classifications are rejected instead of inferred as zero.
pub fn stacked(
    bounds: Rect,
    axis_max: u64,
    legend: &[u32],
    input: &[Segment],
) -> Result<Stack, Error> {
    validate_bounds(bounds)?;
    let mut values = BTreeMap::new();
    for item in input {
        if values.insert(item.legend_key, item.value).is_some() {
            return Err(Error::DuplicateLegend);
        }
    }
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    let mut total = Sum::default();
    for key in legend {
        if !seen.insert(*key) {
            return Err(Error::DuplicateLegend);
        }
        let value = *values.get(key).ok_or(Error::LegendMismatch)?;
        total.add(value);
        ordered.push(Segment {
            legend_key: *key,
            value,
        });
    }
    if values.len() != legend.len() {
        return Err(Error::LegendMismatch);
    }
    let sum = total.finish();
    if amount(sum).is_some_and(|value| value > axis_max) {
        return Err(Error::InvalidAxis);
    }
    let mut offset = Some(0u64);
    let mut segments = Vec::with_capacity(ordered.len());
    for item in ordered {
        let (rect, no_area) = stack_rect(bounds, axis_max, offset, item.value);
        segments.push(SegmentLayout {
            legend_key: item.legend_key,
            value: item.value,
            rect,
            no_area,
        });
        offset = match (offset, item.value) {
            (Some(start), Weight::Exact(bytes)) => start.checked_add(bytes),
            _ => None,
        };
    }
    Ok(Stack {
        segments,
        total: sum,
        total_overflowed: total.overflowed(),
    })
}

#[derive(Default)]
pub(super) struct Sum {
    value: Option<u64>,
    incomplete: bool,
    overflow: bool,
}
impl Sum {
    pub(super) fn overflowed(&self) -> bool {
        self.overflow
    }
    pub(super) fn add(&mut self, value: Weight) {
        let bytes = match value {
            Weight::Unknown => {
                self.incomplete = true;
                return;
            }
            Weight::LowerBound(n) => {
                self.incomplete = true;
                n
            }
            Weight::Exact(n) => n,
        };
        match self.value.unwrap_or(0).checked_add(bytes) {
            Some(value) => self.value = Some(value),
            None => self.overflow = true,
        }
    }
    pub(super) fn finish(&self) -> Weight {
        if self.overflow {
            return Weight::Unknown;
        }
        match (self.value, self.incomplete) {
            (Some(value), true) => Weight::LowerBound(value),
            (Some(value), false) => Weight::Exact(value),
            (None, true) => Weight::Unknown,
            (None, false) => Weight::Exact(0),
        }
    }
}

pub(super) fn amount(weight: Weight) -> Option<u64> {
    match weight {
        Weight::Exact(n) | Weight::LowerBound(n) => Some(n),
        Weight::Unknown => None,
    }
}
pub(super) fn validate_bounds(rect: Rect) -> Result<(), Error> {
    if rect.left >= rect.right || rect.top >= rect.bottom {
        Err(Error::InvalidBounds)
    } else {
        Ok(())
    }
}

pub(super) fn resolve_axis(
    scale: Scale,
    values: impl Iterator<Item = Weight>,
) -> Result<Option<u64>, Error> {
    let mut maximum = 0;
    let mut exact = true;
    for value in values {
        if let Some(bytes) = amount(value) {
            maximum = maximum.max(bytes);
        }
        exact &= matches!(value, Weight::Exact(_));
    }
    match scale {
        Scale::Automatic => Ok(exact.then_some(maximum)),
        Scale::Fixed(axis) if axis >= maximum => Ok(Some(axis)),
        Scale::Fixed(_) => Err(Error::InvalidAxis),
    }
}

/// Produces one horizontal row using an already chosen common axis maximum.
pub(super) fn row_rect(
    bounds: Rect,
    index: usize,
    count: usize,
    weight: Weight,
    axis: Option<u64>,
) -> (Option<Rect>, Option<NoArea>) {
    let Some(value) = amount(weight) else {
        return (None, Some(NoArea::Unknown));
    };
    if value == 0 {
        return (None, Some(NoArea::Zero));
    }
    let Some(axis) = axis else {
        return (None, Some(NoArea::UncertainScale));
    };
    if axis == 0 || count == 0 {
        return (None, Some(NoArea::CoordinatePrecision));
    }
    let height = u128::from(bounds.bottom - bounds.top);
    let top = bounds.top + (height * index as u128 / count as u128) as u32;
    let bottom = bounds.top + (height * (index + 1) as u128 / count as u128) as u32;
    let right = bounds.left + scale(bounds.right - bounds.left, value, axis);
    if top == bottom || right == bounds.left {
        return (None, Some(NoArea::CoordinatePrecision));
    }
    (
        Some(Rect {
            left: bounds.left,
            right,
            top,
            bottom,
        }),
        None,
    )
}

fn stack_rect(
    bounds: Rect,
    axis: u64,
    offset: Option<u64>,
    weight: Weight,
) -> (Option<Rect>, Option<NoArea>) {
    let Some(value) = amount(weight) else {
        return (None, Some(NoArea::Unknown));
    };
    if value == 0 {
        return (None, Some(NoArea::Zero));
    }
    let Some(start) = offset else {
        return (None, Some(NoArea::UncertainOffset));
    };
    let Some(end) = start.checked_add(value) else {
        return (None, Some(NoArea::UncertainOffset));
    };
    if axis == 0 || end > axis {
        return (None, Some(NoArea::UncertainOffset));
    }
    let left = bounds.left + scale(bounds.right - bounds.left, start, axis);
    let right = bounds.left + scale(bounds.right - bounds.left, end, axis);
    if left == right {
        return (None, Some(NoArea::CoordinatePrecision));
    }
    (
        Some(Rect {
            left,
            right,
            top: bounds.top,
            bottom: bounds.bottom,
        }),
        None,
    )
}
fn scale(length: u32, value: u64, maximum: u64) -> u32 {
    ((u128::from(length) * u128::from(value)) / u128::from(maximum)) as u32
}
