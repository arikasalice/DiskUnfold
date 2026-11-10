//! Pure histogram calculation over caller-supplied boundaries and observations.
//!
//! Boundary generation, attribution, and the source of observations belong to
//! the caller and F26/F03; this module does not infer a missing boundary table.
use super::cancellable_sort;
use super::geometry::{
    NoArea, Rect, Scale, Sum, Weight, rect_geometry, resolve_axis, row_rect, valid_bounds,
};
use du_core_contracts::result::ResultCompleteness;
use std::collections::HashSet;

/// A half-open interval. `None` includes all values from `lower` through u64::MAX.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bin {
    pub lower: u64,
    pub upper: Option<u64>,
}

/// An item and its source value. Identifiers must be unique within the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub item_no: u32,
    pub value: Weight,
}

/// Chooses whether bar length represents items or bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    ItemCount,
    Bytes,
}

/// A bin's distinct count and byte amounts; neither is inferred from the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinLayout {
    pub bin: Bin,
    pub item_count: Weight,
    pub bytes: Weight,
    pub rect: Option<Rect>,
    pub no_area: Option<NoArea>,
    pub count_overflowed: bool,
    pub bytes_overflowed: bool,
}

/// Histogram data, including observations whose bin could not be established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Histogram {
    pub basis: Basis,
    pub bins: Vec<BinLayout>,
    /// Unknown values and lower bounds that can cross a bin boundary.
    pub uncertain_items: Vec<u32>,
    /// Exact values outside the caller's supplied intervals.
    pub unclassified_items: Vec<u32>,
    /// Number of supplied observations, not the estimated total in a partial scope.
    pub observed_items: u64,
    pub total_bytes: Weight,
    /// The established axis maximum, or `None` when it cannot be established.
    pub axis_max: Option<u64>,
    pub source_completeness: ResultCompleteness,
    pub total_bytes_overflowed: bool,
}

impl Histogram {
    /// Converts the calculated bin rectangles to geometry v1 in bin order.
    ///
    /// Each slot corresponds to `bins`; `None` preserves a bin without area.
    /// Bin boundaries, values, and uncertain items remain in this result.
    /// Coordinates must already be on the normalized 0..=1,000,000 grid.
    pub fn geometry_v1(
        &self,
    ) -> Result<
        Vec<Option<du_core_contracts::visualization::geometry::Geometry>>,
        du_core_contracts::visualization::geometry::GeometryError,
    > {
        self.bins
            .iter()
            .map(|bin| rect_geometry(bin.rect))
            .collect()
    }
}

/// Invalid boundaries or input identity. Overflow in a bin remains unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidBounds,
    InvalidBins,
    DuplicateItem,
    InvalidAxis,
    /// Cancellation discards the partial result.
    Cancelled,
}

#[derive(Default)]
struct Accumulator {
    count: Sum,
    bytes: Sum,
}

/// Calculates half-open bins and optional bar rectangles without reading a source.
///
/// Intervals must be ordered and nonoverlapping. Gaps are allowed and exposed as
/// unclassified items. A lower bound is assigned only when it guarantees the
/// item belongs to an open-ended bin; otherwise every bin it could reach remains
/// incomplete. Unknown values never become zero-valued observations.
pub fn layout(
    bounds: Rect,
    bins: &[Bin],
    input: &[Item],
    basis: Basis,
    scale: Scale,
    completeness: ResultCompleteness,
) -> Result<Histogram, Error> {
    calculate(bounds, bins, input, basis, scale, completeness, None)
}

/// Calculates the same histogram with cooperative cancellation.
///
/// Checks each boundary, observation, bin and merge-sort output element.
/// Allocation, an individual hash operation and rectangle calculation cannot
/// be interrupted. No partial result is returned on cancellation.
pub fn layout_cancellable(
    bounds: Rect,
    bins: &[Bin],
    input: &[Item],
    basis: Basis,
    scale: Scale,
    completeness: ResultCompleteness,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Histogram, Error> {
    calculate(
        bounds,
        bins,
        input,
        basis,
        scale,
        completeness,
        Some(cancelled),
    )
}

fn calculate(
    bounds: Rect,
    bins: &[Bin],
    input: &[Item],
    basis: Basis,
    scale: Scale,
    completeness: ResultCompleteness,
    cancelled: Option<&mut dyn FnMut() -> bool>,
) -> Result<Histogram, Error> {
    let cancellable_sort = cancelled.is_some();
    let mut never_cancel = || false;
    let cancelled = cancelled.unwrap_or(&mut never_cancel);
    check(cancelled)?;
    if !valid_bounds(bounds) {
        return Err(Error::InvalidBounds);
    }
    validate_bins(bins, cancelled)?;
    let mut accumulators = Vec::with_capacity(bins.len());
    for _ in bins {
        check(cancelled)?;
        accumulators.push(Accumulator::default());
    }
    let mut seen = HashSet::new();
    let mut total = Sum::default();
    let mut uncertain_items = Vec::new();
    let mut unclassified_items = Vec::new();
    let mut first_uncertain_bin = if completeness == ResultCompleteness::Complete {
        bins.len()
    } else {
        0
    };
    if completeness != ResultCompleteness::Complete {
        total.add(Weight::Unknown);
    }
    for item in input {
        check(cancelled)?;
        if !seen.insert(item.item_no) {
            return Err(Error::DuplicateItem);
        }
        total.add(item.value);
        match item.value {
            Weight::Exact(value) => match containing(bins, value) {
                Some(index) => add(&mut accumulators[index], item.value),
                None => unclassified_items.push(item.item_no),
            },
            Weight::LowerBound(value) => {
                match containing(bins, value).filter(|&index| bins[index].upper.is_none()) {
                    Some(index) => add(&mut accumulators[index], item.value),
                    None => {
                        uncertain_items.push(item.item_no);
                        let first = bins
                            .partition_point(|bin| bin.upper.is_some_and(|upper| upper <= value));
                        first_uncertain_bin = first_uncertain_bin.min(first);
                    }
                }
            }
            Weight::Unknown => {
                uncertain_items.push(item.item_no);
                first_uncertain_bin = 0;
            }
        }
    }
    for accumulator in accumulators.iter_mut().skip(first_uncertain_bin) {
        check(cancelled)?;
        accumulator.count.add(Weight::Unknown);
        accumulator.bytes.add(Weight::Unknown);
    }
    let mut output = Vec::with_capacity(bins.len());
    for (&bin, acc) in bins.iter().zip(&accumulators) {
        check(cancelled)?;
        output.push(BinLayout {
            bin,
            item_count: acc.count.finish(),
            bytes: acc.bytes.finish(),
            rect: None,
            no_area: None,
            count_overflowed: acc.count.overflowed(),
            bytes_overflowed: acc.bytes.overflowed(),
        });
    }
    let mut interrupted = false;
    let axis_result = resolve_axis(
        scale,
        output
            .iter()
            .map(|row| selected(row, basis))
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
    let axis = axis_result.map_err(|_| Error::InvalidAxis)?;
    let count = output.len();
    for (index, row) in output.iter_mut().enumerate() {
        check(cancelled)?;
        (row.rect, row.no_area) = row_rect(bounds, index, count, selected(row, basis), axis);
    }
    if cancellable_sort {
        cancellable_sort::sort(&mut uncertain_items, u32::cmp, cancelled)
            .map_err(|_| Error::Cancelled)?;
        cancellable_sort::sort(&mut unclassified_items, u32::cmp, cancelled)
            .map_err(|_| Error::Cancelled)?;
    } else {
        uncertain_items.sort_unstable();
        unclassified_items.sort_unstable();
    }
    check(cancelled)?;
    Ok(Histogram {
        basis,
        bins: output,
        uncertain_items,
        unclassified_items,
        observed_items: input.len() as u64,
        total_bytes: total.finish(),
        axis_max: axis,
        source_completeness: completeness,
        total_bytes_overflowed: total.overflowed(),
    })
}

fn validate_bins(bins: &[Bin], cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if bins.is_empty() {
        return Err(Error::InvalidBins);
    }
    let mut previous = None;
    for (index, bin) in bins.iter().enumerate() {
        check(cancelled)?;
        if bin.upper.is_some_and(|upper| upper <= bin.lower) {
            return Err(Error::InvalidBins);
        }
        if index > 0 && previous.is_none_or(|upper| bin.lower < upper) {
            return Err(Error::InvalidBins);
        }
        previous = bin.upper;
    }
    Ok(())
}
fn containing(bins: &[Bin], value: u64) -> Option<usize> {
    let index = bins
        .partition_point(|bin| bin.lower <= value)
        .checked_sub(1)?;
    bins[index]
        .upper
        .is_none_or(|upper| value < upper)
        .then_some(index)
}
fn add(acc: &mut Accumulator, value: Weight) {
    acc.count.add(Weight::Exact(1));
    acc.bytes.add(value);
}
fn selected(row: &BinLayout, basis: Basis) -> Weight {
    match basis {
        Basis::ItemCount => row.item_count,
        Basis::Bytes => row.bytes,
    }
}

fn check(cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
