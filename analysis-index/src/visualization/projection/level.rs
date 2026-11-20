//! One level of the projection: the immediate children of one parent.
//!
//! The root and every expanded folder go through this function, so they share one
//! level-of-detail rule: attribute each child, plan which siblings keep a shape, merge
//! the small tail into one group or omit it, and lay out the rest through the selected
//! strategy.
use super::Error;
use crate::{
    ResultVersion,
    visualization::{
        detail::{self, Limits, Tail},
        filter::ItemFilter,
        geometry::{NoArea, Rect},
        live_models::SizeRange,
        treemap::{self, AdmissionError, AdmittedChild, Unattributed},
        typed_layout::{self, Dispatcher, Input, NumericRow},
        value,
    },
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    visualization::{LayoutDescriptor, LayoutStrategyKey, geometry::Geometry},
};

/// Sources and limits of one level.
pub(super) struct Request<'a> {
    pub version: &'a ResultVersion,
    pub parent: &'a ItemRef,
    pub basis: &'a SizeBasis,
    pub bounds: Rect,
    pub dispatcher: &'a Dispatcher,
    pub format: &'a str,
    pub strategy: Option<&'a LayoutStrategyKey>,
    /// Smallest shape area shared by every level of one model.
    pub min_area: u64,
    /// Shapes this level may add, groups included.
    pub slots: usize,
    /// Whether the tail may become a group rather than only being counted.
    pub allow_group: bool,
    /// Files the level keeps; every file without one.
    pub filter: Option<ItemFilter>,
}

/// The group shape of a level, with the evidence for its members.
pub(super) struct LevelGroup {
    pub geometry: Geometry,
    pub total: SizeValue,
    pub bytes: u64,
    pub size_range: SizeRange,
    pub slot: usize,
}

/// A laid-out level. Rows are the attributed children in layout order: retained
/// siblings, then the tail, then exact zeros.
pub(super) struct Level {
    pub descriptor: LayoutDescriptor,
    pub parent_value: SizeValue,
    pub rows: Vec<AdmittedChild>,
    pub unattributed: Vec<Unattributed>,
    /// Rows with positive weight; the rest are zeros.
    pub positive: usize,
    /// Rows laid out individually; the tail is rows[retained..positive].
    pub retained: usize,
    /// Row index and geometry of each individually drawn child, in strategy order.
    pub shapes: Vec<(usize, Geometry)>,
    /// Rows without a shape for a layout reason (zero or coordinate precision).
    pub skipped: Vec<typed_layout::Omission>,
    pub group: Option<LevelGroup>,
    /// Tail rows that are neither grouped nor in a shape.
    pub omitted: usize,
    /// Bytes the filter removed from the attributed children.
    pub filtered_bytes: u64,
}

#[derive(Clone, Copy)]
enum Kind {
    Item(usize),
    Group,
    Blank,
    Zero(usize),
}

/// Builds one level; a failure never returns a partial level.
pub(super) fn build(
    request: Request<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Level, Error> {
    let Request {
        version,
        parent,
        basis,
        bounds,
        dispatcher,
        format,
        strategy,
        min_area,
        slots,
        allow_group,
        filter,
    } = request;
    let partial = treemap::admit_filtered_cancellable(version, parent, basis, filter, cancelled)
        .map_err(admission_error)?;
    let parent_value = value(version, parent.item_no, basis).map_err(|_| Error::InvalidSource)?;
    let treemap::Partial {
        rows,
        unattributed,
        parent_bytes,
        filtered_bytes,
    } = partial;
    let positive = rows.partition_point(|row| row.bytes > 0);
    let weights: Vec<u64> = rows[..positive].iter().map(|row| row.bytes).collect();
    let plan = detail::plan(
        &weights,
        parent_bytes,
        Limits {
            area: detail::rect_area(bounds),
            min_area,
            slots,
            allow_group,
        },
    );
    let group_value = match plan.tail {
        Tail::Group { bytes } => Some(tail_value(&rows[plan.retained..positive], basis, bytes)),
        _ => None,
    };
    let (entries, numeric) = layout_rows(
        &rows,
        &plan,
        positive,
        group_value.as_ref().map(|(total, _)| total),
        &parent_value,
        parent,
        cancelled,
    )?;
    let input = Input::planned(numeric, bounds).map_err(admission_error)?;
    let selected = dispatcher
        .execute(format, strategy, &input, cancelled)
        .map_err(|error| match error {
            typed_layout::DispatchError::Layout(typed_layout::Error::Cancelled) => Error::Cancelled,
            other => Error::Dispatch(other),
        })?;
    let level = Level {
        descriptor: selected.descriptor,
        parent_value,
        rows,
        unattributed,
        positive,
        retained: plan.retained,
        shapes: Vec::new(),
        skipped: Vec::new(),
        group: None,
        omitted: 0,
        filtered_bytes,
    };
    assign(
        level,
        &entries,
        selected.output,
        &plan,
        group_value,
        cancelled,
    )
}

// Maps the strategy's shapes and omissions back to siblings, the group and the blank,
// requiring that every planned row is accounted for exactly once.
fn assign(
    mut level: Level,
    entries: &[Kind],
    output: typed_layout::Output,
    plan: &detail::Plan,
    group_value: Option<(SizeValue, SizeRange)>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Level, Error> {
    let mut group_geometry = None;
    let mut seen = vec![false; entries.len()];
    for shape in output.shapes {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        match take(entries, &mut seen, shape.row_index)? {
            Kind::Item(row) => level.shapes.push((row, shape.geometry)),
            Kind::Group => group_geometry = Some(shape.geometry),
            // The blank only reserves the undrawn share of the parent.
            Kind::Blank => {}
            Kind::Zero(_) => return Err(Error::InvalidLayout),
        }
    }
    let mut group_dropped = false;
    for omitted in &output.skipped {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        match (take(entries, &mut seen, omitted.row_index)?, omitted.reason) {
            (Kind::Item(row) | Kind::Zero(row), reason) => {
                if !valid_omission(level.rows[row].bytes, reason) {
                    return Err(Error::InvalidLayout);
                }
                level.skipped.push(typed_layout::Omission {
                    row_index: row,
                    reason,
                });
            }
            (Kind::Group, NoArea::CoordinatePrecision) => group_dropped = true,
            (Kind::Blank, _) => {}
            (Kind::Group, _) => return Err(Error::InvalidLayout),
        }
    }
    if seen.iter().any(|visited| !visited) {
        return Err(Error::InvalidLayout);
    }
    match (plan.tail, group_geometry, group_value) {
        (Tail::None, ..) => {}
        (Tail::Omitted, ..) => level.omitted = level.positive - plan.retained,
        // A group the grid cannot represent is only counted, like an omitted tail.
        (Tail::Group { .. }, None, _) if group_dropped => {
            level.omitted = level.positive - plan.retained;
        }
        (Tail::Group { bytes }, Some(geometry), Some((total, size_range))) => {
            level.group = Some(LevelGroup {
                geometry,
                total,
                bytes,
                size_range,
                slot: plan.group_slot,
            });
        }
        _ => return Err(Error::InvalidLayout),
    }
    Ok(level)
}

// A zero row may only lack area because it is zero; a positive row only for precision.
fn valid_omission(bytes: u64, reason: NoArea) -> bool {
    if bytes == 0 {
        matches!(reason, NoArea::ExactZero | NoArea::LowerBoundZero)
    } else {
        reason == NoArea::CoordinatePrecision
    }
}

fn take(entries: &[Kind], seen: &mut [bool], index: usize) -> Result<Kind, Error> {
    let kind = *entries.get(index).ok_or(Error::InvalidLayout)?;
    if std::mem::replace(&mut seen[index], true) {
        return Err(Error::InvalidLayout);
    }
    Ok(kind)
}

// Layout order: retained siblings, the group and the undrawn blank by non-increasing
// weight (a sibling before the group before the blank when equal), then exact zeros.
fn layout_rows(
    rows: &[AdmittedChild],
    plan: &detail::Plan,
    positive: usize,
    group: Option<&SizeValue>,
    parent_value: &SizeValue,
    parent: &ItemRef,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(Vec<Kind>, Vec<NumericRow>), Error> {
    let mut weighted: Vec<(u64, u8, usize, Kind)> = Vec::with_capacity(plan.retained + 2);
    for (index, row) in rows[..plan.retained].iter().enumerate() {
        weighted.push((row.bytes, 0, index, Kind::Item(index)));
    }
    if let Tail::Group { bytes } = plan.tail {
        weighted.push((bytes, 1, 0, Kind::Group));
    }
    if plan.blank > 0 {
        weighted.push((plan.blank, 2, 0, Kind::Blank));
    }
    weighted.sort_by_key(|(bytes, rank, index, _)| (std::cmp::Reverse(*bytes), *rank, *index));
    let mut entries = Vec::with_capacity(weighted.len() + rows.len() - positive);
    let mut numeric = Vec::with_capacity(entries.capacity());
    for (bytes, _, _, kind) in weighted {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        entries.push(kind);
        numeric.push(match kind {
            Kind::Item(index) => {
                let row = &rows[index];
                NumericRow::planned(row.child.item.clone(), row.child.value.clone(), bytes)
            }
            Kind::Group => {
                let representative = rows
                    .get(plan.retained)
                    .ok_or(Error::InvalidLayout)?
                    .child
                    .item
                    .clone();
                NumericRow::planned(
                    representative,
                    group.ok_or(Error::InvalidLayout)?.clone(),
                    bytes,
                )
            }
            Kind::Blank | Kind::Zero(_) => {
                let blank = SizeValue {
                    basis: parent_value.basis.clone(),
                    evidence: parent_value.evidence.clone(),
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                };
                NumericRow::planned(parent.clone(), blank, bytes)
            }
        });
    }
    for (index, row) in rows.iter().enumerate().skip(positive) {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        entries.push(Kind::Zero(index));
        numeric.push(NumericRow::planned(
            row.child.item.clone(),
            row.child.value.clone(),
            0,
        ));
    }
    Ok((entries, numeric))
}

// The tail's F03-derived total and its size range. Members are attributed, so their
// observations are disjoint and the total is their sum; it is a lower bound whenever a
// member is, and carries the weakest member evidence.
fn tail_value(tail: &[AdmittedChild], basis: &SizeBasis, bytes: u64) -> (SizeValue, SizeRange) {
    let lower = tail
        .iter()
        .any(|row| matches!(row.child.value.amount, SizeAmount::LowerBound { .. }));
    let evidence = tail
        .iter()
        .map(|row| row.child.value.evidence.clone())
        .fold(SizeEvidence::Measured, weaker);
    let total = SizeValue {
        basis: basis.clone(),
        evidence,
        amount: if lower {
            SizeAmount::LowerBound {
                bytes: bytes.into(),
            }
        } else {
            SizeAmount::Exact {
                bytes: bytes.into(),
            }
        },
    };
    let proven = tail.iter().all(|row| {
        matches!(row.child.value.amount, SizeAmount::Exact { .. })
            && row.child.value.evidence != SizeEvidence::Unknown
            && row.child.value.basis == *basis
    });
    let range = match (tail.first(), tail.last()) {
        (Some(max), Some(min)) if proven => SizeRange::Known {
            min: min.child.value.clone(),
            max: max.child.value.clone(),
        },
        _ => SizeRange::Unknown,
    };
    (total, range)
}

fn weaker(left: SizeEvidence, right: SizeEvidence) -> SizeEvidence {
    let rank = |evidence: &SizeEvidence| match evidence {
        SizeEvidence::Measured => 0,
        SizeEvidence::OsReported => 1,
        SizeEvidence::Estimated => 2,
        SizeEvidence::Unknown => 3,
    };
    if rank(&right) > rank(&left) {
        right
    } else {
        left
    }
}

/// Maps a sibling admission failure to the projection error it has always meant.
pub(super) fn admission_error(error: AdmissionError) -> Error {
    match error {
        AdmissionError::Cancelled => Error::Cancelled,
        AdmissionError::NoAttribution => Error::UnavailableAttribution,
        _ => Error::InvalidSource,
    }
}
