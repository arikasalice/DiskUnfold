//! Level-of-detail policy table 1: which siblings keep a shape, which merge into one
//! group, and which are only counted.
//!
//! The decision uses integer arithmetic on sizes alone, so equal sizes always land on
//! the same side of every boundary and the result does not depend on row order,
//! ordering keys or item numbers.
use super::geometry::Rect;

/// Area of a rectangle in squared normalized coordinate units.
pub fn rect_area(rect: Rect) -> u64 {
    u64::from(rect.right.saturating_sub(rect.left))
        * u64::from(rect.bottom.saturating_sub(rect.top))
}

/// Smallest area a shape may have: the visible window divided by the shape limit,
/// rounded up, and at least one unit.
///
/// Disjoint shapes inside the window that each reach this area number at most
/// max_shapes, so the limit follows from the construction rather than from a check.
pub fn min_area(window_area: u64, max_shapes: usize) -> u64 {
    let limit = u64::try_from(max_shapes.max(1)).unwrap_or(u64::MAX);
    window_area.div_ceil(limit).max(1)
}

/// Area and limits of one parent: its rectangle, the shared minimum shape area, the
/// number of shapes this level may still add and whether a tail may become a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub area: u64,
    pub min_area: u64,
    pub slots: usize,
    /// When false the tail is only counted, even where a group would fit.
    pub allow_group: bool,
}

/// What the positive siblings after the retained prefix become.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tail {
    /// Every sibling keeps its own shape.
    None,
    /// One group shape holds the tail; its weight is the tail's byte sum.
    Group { bytes: u64 },
    /// The tail, even as a group, is below the minimum area, no slot remains, or the
    /// caller does not allow a group.
    Omitted,
}

/// Retained prefix, tail treatment and the parent bytes that stay undrawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Leading siblings that keep their own shapes. Always ends at a size boundary.
    pub retained: usize,
    pub tail: Tail,
    /// Retained siblings listed before the group in layout and table order. Siblings
    /// of the same size as the group come first.
    pub group_slot: usize,
    /// Parent bytes not covered by retained siblings or the group: the omitted tail
    /// and anything that could not be attributed. It is laid out as an undrawn blank.
    pub blank: u64,
}

/// Plans one parent's level.
///
/// bytes must be positive and non-increasing. A sibling is retained while its
/// proportional area, bytes / parent_bytes of the parent's area, reaches min_area. The
/// remaining tail becomes one group when its sum also reaches it. The retained count
/// is cut back to a size boundary when slots are short, so the cut never separates
/// siblings of equal size; one slot is reserved for the group.
pub fn plan(bytes: &[u64], parent_bytes: u64, limits: Limits) -> Plan {
    let Limits {
        area,
        min_area,
        slots,
        allow_group,
    } = limits;
    if bytes.is_empty() || slots == 0 {
        let tail = if bytes.is_empty() {
            Tail::None
        } else {
            Tail::Omitted
        };
        return Plan {
            retained: 0,
            tail,
            group_slot: 0,
            blank: parent_bytes,
        };
    }
    let meets =
        |value: u128| value * u128::from(area) >= u128::from(min_area) * u128::from(parent_bytes);
    let all: u128 = bytes.iter().map(|b| u128::from(*b)).sum();
    let mut retained = bytes.partition_point(|b| meets(u128::from(*b))).min(slots);
    retained = size_boundary(bytes, retained);
    let group = allow_group && retained < bytes.len() && meets(all - prefix_sum(bytes, retained));
    if group && retained >= slots {
        retained = size_boundary(bytes, slots - 1);
    }
    let head = prefix_sum(bytes, retained);
    let tail_sum = all - head;
    let tail = match (retained == bytes.len(), group) {
        (true, _) => Tail::None,
        (false, true) => Tail::Group {
            bytes: u64::try_from(tail_sum).unwrap_or(u64::MAX),
        },
        (false, false) => Tail::Omitted,
    };
    let group_slot = if group {
        bytes[..retained].partition_point(|b| u128::from(*b) >= tail_sum)
    } else {
        0
    };
    let covered = head + if group { tail_sum } else { 0 };
    Plan {
        retained,
        tail,
        group_slot,
        blank: u64::try_from(u128::from(parent_bytes).saturating_sub(covered)).unwrap_or(0),
    }
}

fn prefix_sum(bytes: &[u64], count: usize) -> u128 {
    bytes[..count].iter().map(|b| u128::from(*b)).sum()
}

// Moves a cut left to the start of the run of equal sizes it would split.
fn size_boundary(bytes: &[u64], cut: usize) -> usize {
    match bytes.get(cut) {
        Some(next) if cut > 0 && bytes[cut - 1] == *next => bytes.partition_point(|b| b > next),
        _ => cut,
    }
}
