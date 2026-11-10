//! Pure waterfall layout over caller-supplied comparison values.
//!
//! The caller supplies the comparison start and end values, ordered signed
//! changes, and values that could not be compared. This module does not infer
//! comparison sources or reconcile missing values.
use super::geometry::{NoArea, Rect, Scale, Weight, valid_bounds};

/// Direction of an exact comparison change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Increase,
    Decrease,
}

/// A signed change amount supplied by the comparison result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub item_no: u32,
    pub direction: Direction,
    pub value: Weight,
}

/// One start, change, or end bar and its optional integer rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    /// `None` identifies the start or end total; changes carry their item number.
    pub item_no: Option<u32>,
    /// Changes retain their sign even when no rectangle can be established.
    pub direction: Option<Direction>,
    pub value: Weight,
    pub rect: Option<Rect>,
    pub no_area: Option<NoArea>,
}

/// Waterfall bars with uncertain comparison rows retained separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waterfall {
    pub start: Bar,
    pub changes: Vec<Bar>,
    pub end: Bar,
    pub uncertain_changes: Vec<Change>,
    /// The established vertical axis maximum, or `None` if values are uncertain.
    pub axis_max: Option<u64>,
}

/// Invalid geometry, duplicate change identity, or a fixed axis below a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidBounds,
    DuplicateItem,
    InvalidAxis,
}

/// Lays out start total, ordered increases/decreases, and end total.
///
/// Non-exact changes are returned in `uncertain_changes` and make later change
/// offsets uncertain. The exact end total remains independently drawable.
/// Arithmetic underflow/overflow likewise makes subsequent offsets uncertain;
/// it does not clamp values or invent a replacement total.
pub fn layout(
    bounds: Rect,
    start: Weight,
    changes: &[Change],
    end: Weight,
    scale: Scale,
) -> Result<Waterfall, Error> {
    if !valid_bounds(bounds) {
        return Err(Error::InvalidBounds);
    }
    let uncertain_changes = validate_changes(changes)?;
    let count = changes.len() + 2;
    let mut cumulative = match start {
        Weight::Exact(value) => Some(value),
        Weight::LowerBound(_) | Weight::Unknown => None,
    };
    let mut fully_known = cumulative.is_some();
    let mut maximum = match start {
        Weight::Exact(value) => Some(value),
        _ => None,
    };
    let mut levels = Vec::with_capacity(changes.len());
    for change in changes {
        let before = cumulative;
        let after = match (cumulative, change.value, change.direction) {
            (Some(value), Weight::Exact(delta), Direction::Increase) => value.checked_add(delta),
            (Some(value), Weight::Exact(delta), Direction::Decrease) => value.checked_sub(delta),
            _ => None,
        };
        if after.is_none() {
            fully_known = false;
        }
        if let Some(value) = after {
            maximum = Some(maximum.unwrap_or(0).max(value));
        }
        levels.push((before, after));
        cumulative = after;
    }
    if let Weight::Exact(value) = end {
        maximum = Some(maximum.unwrap_or(0).max(value));
    } else {
        fully_known = false;
    }
    let axis = match scale {
        Scale::Automatic => fully_known.then_some(maximum).flatten(),
        Scale::Fixed(axis) if maximum.is_none_or(|max| axis >= max) => Some(axis),
        Scale::Fixed(_) => return Err(Error::InvalidAxis),
    };
    let start_bar = total_bar(bounds, 0, count, start, axis);
    let mut bars = Vec::with_capacity(changes.len());
    for (index, change) in changes.iter().enumerate() {
        let x = column(bounds, index + 1, count);
        let (rect, no_area) = change_rect(bounds, x, levels[index], axis, change.value);
        bars.push(Bar {
            item_no: Some(change.item_no),
            direction: Some(change.direction),
            value: change.value,
            rect,
            no_area,
        });
    }
    let end_bar = total_bar(bounds, count - 1, count, end, axis);
    Ok(Waterfall {
        start: start_bar,
        changes: bars,
        end: end_bar,
        uncertain_changes,
        axis_max: axis,
    })
}

fn validate_changes(changes: &[Change]) -> Result<Vec<Change>, Error> {
    let mut seen = std::collections::HashSet::new();
    let mut uncertain = Vec::new();
    for change in changes {
        if !seen.insert(change.item_no) {
            return Err(Error::DuplicateItem);
        }
        if !matches!(change.value, Weight::Exact(_)) {
            uncertain.push(*change);
        }
    }
    Ok(uncertain)
}

fn column(bounds: Rect, index: usize, count: usize) -> (u32, u32) {
    let width = (bounds.right - bounds.left) as u128;
    let left = bounds.left + (width * index as u128 / count as u128) as u32;
    let right = bounds.left + (width * (index + 1) as u128 / count as u128) as u32;
    (left, right)
}

fn total_bar(bounds: Rect, index: usize, count: usize, value: Weight, axis: Option<u64>) -> Bar {
    let (left, right) = column(bounds, index, count);
    let amount = match value {
        Weight::Exact(value) => Some(value),
        _ => None,
    };
    let (rect, no_area) = match (amount, axis) {
        (Some(0), _) => (None, Some(NoArea::ExactZero)),
        (Some(value), Some(max)) if max > 0 && right > left => {
            let height = ((u128::from(bounds.bottom - bounds.top) * u128::from(value))
                / u128::from(max)) as u32;
            if height == 0 {
                (None, Some(NoArea::CoordinatePrecision))
            } else {
                (
                    Some(Rect {
                        left,
                        right,
                        top: bounds.bottom - height,
                        bottom: bounds.bottom,
                    }),
                    None,
                )
            }
        }
        (Some(_), Some(_)) => (None, Some(NoArea::CoordinatePrecision)),
        (Some(_), None) => (None, Some(NoArea::UncertainScale)),
        (None, _) if value == Weight::LowerBound(0) => (None, Some(NoArea::LowerBoundZero)),
        _ => (None, Some(NoArea::Unknown)),
    };
    Bar {
        item_no: None,
        direction: None,
        value,
        rect,
        no_area,
    }
}

fn change_rect(
    bounds: Rect,
    (left, right): (u32, u32),
    (before, after): (Option<u64>, Option<u64>),
    axis: Option<u64>,
    value: Weight,
) -> (Option<Rect>, Option<NoArea>) {
    let (before, after, axis) = match (before, after, axis, value) {
        (_, _, _, Weight::Exact(0)) => return (None, Some(NoArea::ExactZero)),
        (_, _, _, Weight::LowerBound(0)) => return (None, Some(NoArea::LowerBoundZero)),
        (_, _, _, Weight::Unknown) => return (None, Some(NoArea::Unknown)),
        (Some(before), Some(after), Some(axis), Weight::Exact(_)) => (before, after, axis),
        (Some(_), Some(_), None, Weight::Exact(_)) => return (None, Some(NoArea::UncertainScale)),
        _ => return (None, Some(NoArea::UncertainOffset)),
    };
    if axis == 0 || right <= left {
        return (None, Some(NoArea::CoordinatePrecision));
    }
    let y = |value: u64| -> u32 {
        bounds.bottom
            - ((u128::from(bounds.bottom - bounds.top) * u128::from(value)) / u128::from(axis))
                as u32
    };
    let top = y(before.max(after));
    let bottom = y(before.min(after));
    if top == bottom {
        return (None, Some(NoArea::CoordinatePrecision));
    }
    (
        Some(Rect {
            left,
            right,
            top,
            bottom,
        }),
        None,
    )
}
