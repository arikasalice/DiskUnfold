//! Coordinates and conservative value handling shared by visualization strategies.

/// Integer rectangle using exclusive right and bottom edges in the caller's units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

// Conversion preserves the existing layout and never creates geometry for None.
pub(super) fn rect_geometry(
    rect: Option<Rect>,
) -> Result<
    Option<du_core_contracts::visualization::geometry::Geometry>,
    du_core_contracts::visualization::geometry::GeometryError,
> {
    use du_core_contracts::visualization::geometry::{Geometry, Point};
    rect.map(|rect| {
        Geometry::new(
            "rect",
            vec![
                Point {
                    x: rect.left,
                    y: rect.top,
                },
                Point {
                    x: rect.right,
                    y: rect.bottom,
                },
            ],
        )
    })
    .transpose()
}

/// A source value; a lower bound remains distinguishable from an exact value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Exact(u64),
    LowerBound(u64),
    Unknown,
}

/// Why a row or segment has no drawable area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoArea {
    Unknown,
    ExactZero,
    LowerBoundZero,
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
pub(super) fn positive_amount(weight: Weight) -> Result<u64, NoArea> {
    match weight {
        Weight::Unknown => Err(NoArea::Unknown),
        Weight::Exact(0) => Err(NoArea::ExactZero),
        Weight::LowerBound(0) => Err(NoArea::LowerBoundZero),
        Weight::Exact(value) | Weight::LowerBound(value) => Ok(value),
    }
}

pub(super) fn valid_bounds(rect: Rect) -> bool {
    rect.left < rect.right && rect.top < rect.bottom
}

pub(super) struct AxisTooSmall;

pub(super) fn resolve_axis(
    scale: Scale,
    values: impl Iterator<Item = Weight>,
) -> Result<Option<u64>, AxisTooSmall> {
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
        Scale::Fixed(_) => Err(AxisTooSmall),
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
    let value = match positive_amount(weight) {
        Ok(value) => value,
        Err(reason) => return (None, Some(reason)),
    };
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

pub(super) fn scale(length: u32, value: u64, maximum: u64) -> u32 {
    ((u128::from(length) * u128::from(value)) / u128::from(maximum)) as u32
}
