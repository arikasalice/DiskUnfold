//! One-parent squarified layout over an already ordered, attributed input.
//!
//! This is an internal calculation unit, not the F06 wire model or a policy for
//! choosing a size basis, sorting equal values, grouping, or overall totals.
pub use super::geometry::{Rect, Weight};
use super::geometry::{rectangle_geometry, valid_bounds};
use std::num::NonZeroU64;

/// A shape refers back to an index in the supplied ordered sibling slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub input_index: usize,
    pub rect: Rect,
    pub weight: Weight,
}

/// An input with no drawable rectangle, kept separate from measured zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    Unknown,
    ExactZero,
    LowerBoundZero,
    /// The integer coordinate grid cannot represent a positive rectangle.
    CoordinatePrecision,
}

/// A non-drawn input and the reason it has no area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub input_index: usize,
    pub reason: SkipReason,
}

/// Disjoint shapes and the inputs that could not be drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub shapes: Vec<Shape>,
    pub skipped: Vec<Skipped>,
}

impl Layout {
    /// Converts drawn rectangles to geometry v1 without changing coordinates.
    ///
    /// Coordinates must already be in the normalized 0..=1,000,000 grid.
    /// The returned index identifies the original ordered input. Skipped items
    /// remain in `skipped` and are not converted to fabricated shapes.
    pub fn geometry_v1(
        &self,
    ) -> Result<
        Vec<(usize, du_core_contracts::visualization::geometry::Geometry)>,
        du_core_contracts::visualization::geometry::GeometryError,
    > {
        self.shapes
            .iter()
            .map(|shape| Ok((shape.input_index, rectangle_geometry(shape.rect)?)))
            .collect()
    }
}

/// Input or arithmetic failure. No partial layout is returned on failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    InvalidBounds,
    NotDescending,
    ArithmeticOverflow,
}

#[derive(Clone, Copy)]
struct Item {
    index: usize,
    amount: NonZeroU64,
    weight: Weight,
}

/// Partitions `parent` into squarified rectangles in caller-supplied order.
///
/// Positive weights must be nonincreasing; equal weights retain their supplied
/// order. Unknown and zero inputs have no rectangle. Rounding can make a small
/// positive item unrepresentable; it is reported instead of being assigned a
/// fabricated nonzero area. All output is contained in the supplied rectangle.
/// Row aspect ratios are compared in `f64`; very close ratios at large integer
/// magnitudes can compare equal. Rectangle boundaries use checked integer math.
/// The caller must supply weights attributed to disjoint sibling areas; this
/// unit cannot establish hard-link or shared-extent accounting on its own.
pub fn layout(parent: Rect, ordered: &[Weight]) -> Result<Layout, LayoutError> {
    if !valid_bounds(parent) {
        return Err(LayoutError::InvalidBounds);
    }
    let mut result = Layout {
        shapes: Vec::new(),
        skipped: Vec::new(),
    };
    let mut items = Vec::new();
    for (index, weight) in ordered.iter().copied().enumerate() {
        match weight {
            Weight::Unknown => result.skipped.push(Skipped {
                input_index: index,
                reason: SkipReason::Unknown,
            }),
            Weight::Exact(0) => result.skipped.push(Skipped {
                input_index: index,
                reason: SkipReason::ExactZero,
            }),
            Weight::LowerBound(0) => result.skipped.push(Skipped {
                input_index: index,
                reason: SkipReason::LowerBoundZero,
            }),
            Weight::Exact(n) | Weight::LowerBound(n) => {
                let amount = NonZeroU64::new(n).ok_or(LayoutError::ArithmeticOverflow)?;
                if items.last().is_some_and(|last: &Item| last.amount < amount) {
                    return Err(LayoutError::NotDescending);
                }
                items.push(Item {
                    index,
                    amount,
                    weight,
                });
            }
        }
    }
    let mut remaining_weight = items.iter().try_fold(0_u128, |sum, item| {
        sum.checked_add(u128::from(item.amount.get()))
            .ok_or(LayoutError::ArithmeticOverflow)
    })?;
    let mut remaining = parent;
    let mut start = 0;
    while start < items.len() {
        if remaining.left == remaining.right || remaining.top == remaining.bottom {
            skip_precision(&mut result, &items[start..]);
            break;
        }
        let (end, row_weight) = select_row(remaining, remaining_weight, &items, start)?;
        if !lay_row(
            &mut result,
            &mut remaining,
            remaining_weight,
            &items[start..end],
            row_weight,
        )? {
            skip_precision(&mut result, &items[start..]);
            break;
        }
        remaining_weight -= row_weight;
        start = end;
    }
    result.skipped.sort_by_key(|entry| entry.input_index);
    Ok(result)
}

fn select_row(
    rect: Rect,
    total: u128,
    items: &[Item],
    start: usize,
) -> Result<(usize, u128), LayoutError> {
    let mut end = start + 1;
    let mut sum = u128::from(items[start].amount.get());
    let short = f64::from((rect.right - rect.left).min(rect.bottom - rect.top));
    let area = f64::from(rect.right - rect.left) * f64::from(rect.bottom - rect.top);
    let scale = area / total as f64;
    let mut worst = aspect(
        short,
        scale,
        sum,
        items[start].amount.get(),
        items[start].amount.get(),
    );
    while let Some(next) = items.get(end) {
        let candidate_sum = sum
            .checked_add(u128::from(next.amount.get()))
            .ok_or(LayoutError::ArithmeticOverflow)?;
        let candidate = aspect(
            short,
            scale,
            candidate_sum,
            items[start].amount.get(),
            next.amount.get(),
        );
        if candidate > worst {
            break;
        }
        worst = candidate;
        sum = candidate_sum;
        end += 1;
    }
    Ok((end, sum))
}

fn aspect(short: f64, scale: f64, sum: u128, max: u64, min: u64) -> f64 {
    let sum_area = sum as f64 * scale;
    let short_squared = short * short;
    ((short_squared * max as f64 * scale) / (sum_area * sum_area))
        .max((sum_area * sum_area) / (short_squared * min as f64 * scale))
}

fn lay_row(
    result: &mut Layout,
    remaining: &mut Rect,
    total: u128,
    items: &[Item],
    row_weight: u128,
) -> Result<bool, LayoutError> {
    let width = remaining.right - remaining.left;
    let height = remaining.bottom - remaining.top;
    let vertical = width >= height;
    let long = if vertical { width } else { height };
    let short = if vertical { height } else { width };
    let strip = proportion(long, row_weight, total)?;
    if strip == 0 {
        return Ok(false);
    }
    let mut previous = 0;
    let mut cumulative = 0_u128;
    for item in items {
        cumulative += u128::from(item.amount.get());
        let edge = proportion(short, cumulative, row_weight)?;
        if edge == previous {
            result.skipped.push(Skipped {
                input_index: item.index,
                reason: SkipReason::CoordinatePrecision,
            });
        } else {
            let rect = if vertical {
                Rect {
                    left: remaining.left,
                    right: remaining.left + strip,
                    top: remaining.top + previous,
                    bottom: remaining.top + edge,
                }
            } else {
                Rect {
                    left: remaining.left + previous,
                    right: remaining.left + edge,
                    top: remaining.top,
                    bottom: remaining.top + strip,
                }
            };
            result.shapes.push(Shape {
                input_index: item.index,
                rect,
                weight: item.weight,
            });
        }
        previous = edge;
    }
    if vertical {
        remaining.left += strip;
    } else {
        remaining.top += strip;
    }
    Ok(true)
}

fn proportion(length: u32, value: u128, total: u128) -> Result<u32, LayoutError> {
    let product = value
        .checked_mul(u128::from(length))
        .ok_or(LayoutError::ArithmeticOverflow)?;
    let rounded = product / total + u128::from(product % total >= total / 2 + total % 2);
    u32::try_from(rounded).map_err(|_| LayoutError::ArithmeticOverflow)
}

fn skip_precision(result: &mut Layout, items: &[Item]) {
    result.skipped.extend(items.iter().map(|item| Skipped {
        input_index: item.index,
        reason: SkipReason::CoordinatePrecision,
    }));
}
