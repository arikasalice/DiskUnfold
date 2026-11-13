//! Pure ranked and stacked bar layout over caller-supplied numeric facts.
//!
//! These local models are not the serialized F06 contract. The caller owns the
//! pinned source, size basis, shared-space attribution, and legend definitions.
use super::cancellable_sort;
use super::geometry::{
    NoArea, Rect, Scale, Sum, Weight, amount, positive_amount, rect_geometry, resolve_axis,
    row_rect, scale, valid_bounds,
};
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

impl Ranking {
    /// Converts existing bar rectangles to geometry v1 in the same bar order.
    ///
    /// Each slot corresponds to `bars`; `None` preserves a bar without an
    /// area. Unknown items remain separately available in `unknown_items`.
    /// Coordinates must already be on the normalized 0..=1,000,000 grid.
    pub fn geometry_v1(
        &self,
    ) -> Result<
        Vec<Option<du_core_contracts::visualization::geometry::Geometry>>,
        du_core_contracts::visualization::geometry::GeometryError,
    > {
        self.bars
            .iter()
            .map(|bar| rect_geometry(bar.rect))
            .collect()
    }

    /// Converts existing rectangles while allowing cancellation between rows.
    pub fn geometry_v1_cancellable(
        &self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<Option<du_core_contracts::visualization::geometry::Geometry>>, GeometryError>
    {
        convert_geometry(self.bars.iter().map(|bar| bar.rect), cancelled)
    }
}

/// Geometry conversion failure, separate from layout calculation errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeometryError {
    Cancelled,
    Invalid(du_core_contracts::visualization::geometry::GeometryError),
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
    /// The calculation was cancelled; no partial layout is returned.
    Cancelled,
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
    rank(bounds, input, top, scale, completeness, None)
}

/// Calculates the same ranking while polling cancellation throughout the work.
///
/// Sorting uses a stable merge with a check per output element. Allocation and
/// an individual comparison, hash operation, or rectangle calculation cannot be
/// interrupted. Cancellation discards the partially constructed result.
pub fn ranking_cancellable(
    bounds: Rect,
    input: &[Item],
    top: usize,
    scale: Scale,
    completeness: ResultCompleteness,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Ranking, Error> {
    rank(bounds, input, top, scale, completeness, Some(cancelled))
}

fn rank(
    bounds: Rect,
    input: &[Item],
    top: usize,
    scale: Scale,
    completeness: ResultCompleteness,
    cancelled: Option<&mut dyn FnMut() -> bool>,
) -> Result<Ranking, Error> {
    let cancellable_sort = cancelled.is_some();
    let mut never_cancel = || false;
    let cancelled = cancelled.unwrap_or(&mut never_cancel);
    check(cancelled)?;
    if !valid_bounds(bounds) {
        return Err(Error::InvalidBounds);
    }
    let mut seen = HashSet::new();
    let mut known = Vec::new();
    let mut unknown_items = Vec::new();
    let mut total = Sum::default();
    let mut ranking_incomplete = completeness != ResultCompleteness::Complete;
    if ranking_incomplete {
        total.add(Weight::Unknown);
    }
    for item in input {
        check(cancelled)?;
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
    let rank_order = |a: &Item, b: &Item| {
        amount(b.value)
            .cmp(&amount(a.value))
            .then_with(|| a.item_no.cmp(&b.item_no))
    };
    if cancellable_sort {
        cancellable_sort::sort(&mut known, rank_order, cancelled).map_err(|_| Error::Cancelled)?;
        cancellable_sort::sort(&mut unknown_items, u32::cmp, cancelled)
            .map_err(|_| Error::Cancelled)?;
    } else {
        known.sort_by(rank_order);
        unknown_items.sort_unstable();
    }
    let split = top.min(known.len());
    let mut bars = Vec::with_capacity(split);
    for item in &known[..split] {
        check(cancelled)?;
        bars.push(Bar {
            reference: BarRef::Item(item.item_no),
            value: item.value,
            rect: None,
            no_area: None,
            overflowed: false,
        });
    }
    if split < known.len() {
        let mut other = Sum::default();
        let mut members = Vec::new();
        for item in &known[split..] {
            check(cancelled)?;
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
    let mut interrupted = false;
    let axis_result = resolve_axis(
        scale,
        input
            .iter()
            .map(|item| item.value)
            .chain(bars.iter().map(|bar| bar.value))
            .take_while(|_| {
                if cancelled() {
                    interrupted = true;
                    false
                } else {
                    true
                }
            }),
    );
    if interrupted {
        return Err(Error::Cancelled);
    }
    let mut axis = axis_result.map_err(|_| Error::InvalidAxis)?;
    if completeness != ResultCompleteness::Complete && scale == Scale::Automatic {
        axis = None;
    }
    let count = bars.len();
    for (index, bar) in bars.iter_mut().enumerate() {
        check(cancelled)?;
        (bar.rect, bar.no_area) = row_rect(bounds, index, count, bar.value, axis);
    }
    check(cancelled)?;
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

impl Stack {
    /// Converts existing rectangles in `segments` legend order to geometry v1.
    ///
    /// Each slot corresponds to its source segment, including `None` for zero,
    /// unknown, or unrepresentable geometry. Coordinates must already be on the
    /// normalized 0..=1,000,000 grid. Values and category keys stay in `segments`.
    pub fn geometry_v1(
        &self,
    ) -> Result<
        Vec<Option<du_core_contracts::visualization::geometry::Geometry>>,
        du_core_contracts::visualization::geometry::GeometryError,
    > {
        self.segments
            .iter()
            .map(|segment| rect_geometry(segment.rect))
            .collect()
    }

    /// Converts existing rectangles while allowing cancellation between segments.
    pub fn geometry_v1_cancellable(
        &self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<Option<du_core_contracts::visualization::geometry::Geometry>>, GeometryError>
    {
        convert_geometry(self.segments.iter().map(|segment| segment.rect), cancelled)
    }
}

fn convert_geometry(
    rectangles: impl Iterator<Item = Option<Rect>>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<Option<du_core_contracts::visualization::geometry::Geometry>>, GeometryError> {
    let mut output = Vec::new();
    for rect in rectangles {
        if cancelled() {
            return Err(GeometryError::Cancelled);
        }
        output.push(rect_geometry(rect).map_err(GeometryError::Invalid)?);
    }
    if cancelled() {
        return Err(GeometryError::Cancelled);
    }
    Ok(output)
}

/// Orders every supplied segment by `legend` and uses the caller's fixed axis.
/// Missing or duplicate classifications are rejected instead of inferred as zero.
pub fn stacked(
    bounds: Rect,
    axis_max: u64,
    legend: &[u32],
    input: &[Segment],
) -> Result<Stack, Error> {
    stacked_cancellable(bounds, axis_max, legend, input, &mut || false)
}

/// Builds the same legend-ordered stack with cooperative cancellation.
///
/// Polls each input, legend entry, and output segment; no partial stack escapes.
pub fn stacked_cancellable(
    bounds: Rect,
    axis_max: u64,
    legend: &[u32],
    input: &[Segment],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Stack, Error> {
    check(cancelled)?;
    if !valid_bounds(bounds) {
        return Err(Error::InvalidBounds);
    }
    let mut values = BTreeMap::new();
    for item in input {
        check(cancelled)?;
        if values.insert(item.legend_key, item.value).is_some() {
            return Err(Error::DuplicateLegend);
        }
    }
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    let mut total = Sum::default();
    for key in legend {
        check(cancelled)?;
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
        check(cancelled)?;
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
    check(cancelled)?;
    Ok(Stack {
        segments,
        total: sum,
        total_overflowed: total.overflowed(),
    })
}

fn check(cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn stack_rect(
    bounds: Rect,
    axis: u64,
    offset: Option<u64>,
    weight: Weight,
) -> (Option<Rect>, Option<NoArea>) {
    let value = match positive_amount(weight) {
        Ok(value) => value,
        Err(reason) => return (None, Some(reason)),
    };
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
