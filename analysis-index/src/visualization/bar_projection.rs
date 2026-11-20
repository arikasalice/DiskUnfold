//! Pinned F03 evidence adapter for the ranked (not stacked) bar calculation.
//!
//! This internal result is not a public F06 model or a complete `viz.bar`
//! registration. The caller supplies scope and denominator evidence; this module
//! checks it against the same immutable source rather than summing displayed bars.
use std::collections::{HashMap, HashSet};

use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::geometry::Geometry,
};

use super::{
    bar, descendant,
    geometry::{NoArea, Rect, Scale, Weight},
    root_ref, treemap, value,
};
use crate::ResultVersion;

/// Source identity and original F03 size for one ranked candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub item: ItemRef,
    pub size: SizeValue,
}

/// Identifies a pinned F03 folder or whole-analysis aggregate.
///
/// The ranked projection accepts a folder denominator only for its own scope.
/// A distinct focus and its parent are not represented by this internal request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AggregateTarget {
    Folder(ItemRef),
    Analysis,
}

/// An aggregate supplied by the caller and checked against the pinned source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggregateEvidence {
    pub result_version: Id,
    pub target: AggregateTarget,
    pub size: SizeValue,
}

/// Pinned rows and independently supplied F03 aggregation facts.
pub struct Request<'a> {
    pub version: &'a ResultVersion,
    pub scope: &'a ItemRef,
    pub basis: SizeBasis,
    pub rows: &'a [Row],
    pub scope_total: &'a AggregateEvidence,
    pub denominator: &'a AggregateEvidence,
    pub bounds: Rect,
    pub scale: Scale,
    pub top: usize,
    pub max_bars: usize,
}

/// Ranked immediate children of a folder in one immutable result.
/// This is an internal projection, not a complete registered bar strategy.
pub struct ChildrenRequest<'a> {
    pub version: &'a ResultVersion,
    pub scope: &'a ItemRef,
    pub basis: SizeBasis,
    pub bounds: Rect,
    pub scale: Scale,
    pub top: usize,
    pub max_bars: usize,
}

/// Builds a ranked model from the folder's actual pinned children and F03 total.
/// Scope and denominator are the same folder; filtering and stacked bars require
/// separate owner evidence and are intentionally outside this projection.
pub fn project_children(
    request: ChildrenRequest<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Ranking, Error> {
    prepare_children(request, cancelled)?.render(cancelled)
}

/// Freezes immediate children and their F03 scope total without calculating geometry.
pub fn prepare_children<'a>(
    request: ChildrenRequest<'a>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Prepared<'a>, Error> {
    check(cancelled)?;
    let scope_no = root_ref(request.version, request.scope).map_err(|_| Error::InvalidEvidence)?;
    let total = AggregateEvidence {
        result_version: request.version.version_id().clone(),
        target: AggregateTarget::Folder(request.scope.clone()),
        size: value(request.version, scope_no, &request.basis)
            .map_err(|_| Error::InvalidEvidence)?,
    };
    let mut rows = Vec::new();
    let mut input = Vec::new();
    let mut sources = HashMap::new();
    for child in request.version.children(scope_no) {
        check(cancelled)?;
        let item = ItemRef {
            result_version: request.version.version_id().clone(),
            item_no: child.item_no,
        };
        let size = value(request.version, child.item_no, &request.basis)
            .map_err(|_| Error::InvalidEvidence)?;
        input.push(bar::Item {
            item_no: child.item_no,
            value: weight(&size)?,
        });
        sources.insert(child.item_no, item.clone());
        rows.push(Row { item, size });
    }
    Ok(Prepared {
        version: request.version,
        scope: request.scope.clone(),
        basis: request.basis,
        bounds: request.bounds,
        scale: request.scale,
        top: request.top,
        max_bars: request.max_bars,
        scope_total: total.clone(),
        denominator: total,
        rows,
        input,
        sources,
    })
}

/// Identity of a rendered individual or an F03-verified Other group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarIdentity {
    Item(ItemRef),
    Other {
        members: Vec<ItemRef>,
        size: SizeValue,
    },
}

/// One bar and its original state; missing geometry remains explicitly absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub identity: BarIdentity,
    pub value: Weight,
    pub geometry: Option<Geometry>,
    pub no_area: Option<NoArea>,
}

/// Internal ranking and its unchanged source and F03 aggregate evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranking {
    pub result_version: Id,
    pub rows: Vec<Row>,
    pub bars: Vec<Bar>,
    pub unknown_items: Vec<ItemRef>,
    pub scope_total: AggregateEvidence,
    pub denominator: AggregateEvidence,
    pub completeness: ResultCompleteness,
    pub ranking_incomplete: bool,
    pub axis_max: Option<u64>,
}

/// Inputs or an F03 grouping cannot be safely admitted, or calculation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidEvidence,
    GroupAttributionUnavailable,
    LimitExceeded,
    Cancelled,
    Layout(bar::Error),
    Geometry(bar::GeometryError),
}

/// Lays out a ranked bar view without changing the pinned values or aggregates.
///
/// Other is accepted only if its exact total equal to the layout sum is
/// independently established: by the F03 set total of its members, or, for
/// members that are children of the scope, by the same disjoint partition the
/// treemap admits (#242). Unverifiable overlapping or incomplete groups fail
/// closed. `max_bars` limits returned bars, including Other, without truncation.
/// The denominator must be the aggregation scope or the whole analysis; this
/// request cannot establish a separate focused item's parent denominator.
pub fn project(
    request: Request<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Ranking, Error> {
    prepare(request, cancelled)?.render(cancelled)
}

/// Pinned source facts validated before a ranked bar strategy is invoked.
pub struct Prepared<'a> {
    version: &'a ResultVersion,
    scope: ItemRef,
    basis: SizeBasis,
    bounds: Rect,
    scale: Scale,
    top: usize,
    max_bars: usize,
    scope_total: AggregateEvidence,
    denominator: AggregateEvidence,
    rows: Vec<Row>,
    input: Vec<bar::Item>,
    sources: HashMap<u32, ItemRef>,
}

/// Validates the source, aggregation evidence and members without calculating geometry.
pub fn prepare<'a>(
    request: Request<'a>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Prepared<'a>, Error> {
    check(cancelled)?;
    let scope = root_ref(request.version, request.scope).map_err(|_| Error::InvalidEvidence)?;
    if request.scope_total.target != AggregateTarget::Folder(request.scope.clone()) {
        return Err(Error::InvalidEvidence);
    }
    if let AggregateTarget::Folder(folder) = &request.denominator.target
        && folder != request.scope
    {
        return Err(Error::InvalidEvidence);
    }
    validate_aggregate(request.version, &request.basis, request.scope_total)?;
    validate_aggregate(request.version, &request.basis, request.denominator)?;
    let mut seen = HashSet::new();
    let mut input = Vec::with_capacity(request.rows.len());
    let mut sources = HashMap::new();
    let mut retained_rows = Vec::with_capacity(request.rows.len());
    for row in request.rows {
        check(cancelled)?;
        if row.item.result_version != *request.version.version_id()
            || !seen.insert(row.item.item_no)
            || !descendant(request.version.items(), row.item.item_no, scope)
            || value(request.version, row.item.item_no, &request.basis)
                .ok()
                .as_ref()
                != Some(&row.size)
        {
            return Err(Error::InvalidEvidence);
        }
        let amount = weight(&row.size)?;
        input.push(bar::Item {
            item_no: row.item.item_no,
            value: amount,
        });
        sources.insert(row.item.item_no, row.item.clone());
        retained_rows.push(row.clone());
    }
    Ok(Prepared {
        version: request.version,
        scope: request.scope.clone(),
        basis: request.basis,
        bounds: request.bounds,
        scale: request.scale,
        top: request.top,
        max_bars: request.max_bars,
        scope_total: request.scope_total.clone(),
        denominator: request.denominator.clone(),
        rows: retained_rows,
        input,
        sources,
    })
}

impl Prepared<'_> {
    /// Returns the exact item values retained from the pinned source.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Returns the numeric ranking input in the corresponding source order.
    pub fn items(&self) -> &[bar::Item] {
        &self.input
    }

    /// Returns the fixed layout window and scaling policy.
    pub fn viewport(&self) -> (Rect, Scale) {
        (self.bounds, self.scale)
    }

    /// Returns the admitted individual and total shape limits.
    pub fn limits(&self) -> (usize, usize) {
        (self.top, self.max_bars)
    }

    /// Calculates ranking and geometry from the already validated pinned facts.
    pub fn render(self, cancelled: &mut dyn FnMut() -> bool) -> Result<Ranking, Error> {
        let layout = bar::ranking_cancellable(
            self.bounds,
            &self.input,
            self.top,
            self.scale,
            self.version.completeness(),
            cancelled,
        )
        .map_err(|error| {
            if error == bar::Error::Cancelled {
                Error::Cancelled
            } else {
                Error::Layout(error)
            }
        })?;
        if layout.bars.len() > self.max_bars {
            return Err(Error::LimitExceeded);
        }
        let geometry = layout.geometry_v1_cancellable(cancelled).map_err(|e| {
            if e == bar::GeometryError::Cancelled {
                Error::Cancelled
            } else {
                Error::Geometry(e)
            }
        })?;
        let mut bars = Vec::with_capacity(layout.bars.len());
        for (bar, geometry) in layout.bars.iter().zip(geometry) {
            check(cancelled)?;
            let identity = match &bar.reference {
                bar::BarRef::Item(item) => BarIdentity::Item(
                    self.sources
                        .get(item)
                        .cloned()
                        .ok_or(Error::InvalidEvidence)?,
                ),
                bar::BarRef::Other(items) => verified_other(
                    self.version,
                    &self.scope,
                    &self.basis,
                    items,
                    &self.sources,
                    bar.value,
                    cancelled,
                )?,
            };
            bars.push(Bar {
                identity,
                value: bar.value,
                geometry,
                no_area: bar.no_area,
            });
        }
        let mut unknown_items = Vec::with_capacity(layout.unknown_items.len());
        for item in &layout.unknown_items {
            check(cancelled)?;
            unknown_items.push(
                self.sources
                    .get(item)
                    .cloned()
                    .ok_or(Error::InvalidEvidence)?,
            );
        }
        check(cancelled)?;
        Ok(Ranking {
            result_version: self.version.version_id().clone(),
            rows: self.rows,
            bars,
            unknown_items,
            scope_total: self.scope_total,
            denominator: self.denominator,
            completeness: layout.source_completeness,
            ranking_incomplete: layout.ranking_incomplete,
            axis_max: layout.axis_max,
        })
    }
}
fn validate_aggregate(
    version: &ResultVersion,
    basis: &SizeBasis,
    evidence: &AggregateEvidence,
) -> Result<(), Error> {
    if &evidence.result_version != version.version_id() {
        return Err(Error::InvalidEvidence);
    }
    let expected = match &evidence.target {
        AggregateTarget::Folder(item) => {
            root_ref(version, item).map_err(|_| Error::InvalidEvidence)?;
            value(version, item.item_no, basis).map_err(|_| Error::InvalidEvidence)?
        }
        AggregateTarget::Analysis => match basis {
            SizeBasis::Logical => &version.scope_total().logical,
            SizeBasis::Allocated => &version.scope_total().allocated,
            SizeBasis::UniquePhysical => &version.scope_total().unique_physical,
        }
        .clone(),
    };
    if expected == evidence.size {
        Ok(())
    } else {
        Err(Error::InvalidEvidence)
    }
}

fn verified_other(
    version: &ResultVersion,
    scope: &ItemRef,
    basis: &SizeBasis,
    items: &[u32],
    sources: &HashMap<u32, ItemRef>,
    sum: Weight,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<BarIdentity, Error> {
    let mut members = Vec::with_capacity(items.len());
    for item in items {
        check(cancelled)?;
        members.push(sources.get(item).cloned().ok_or(Error::InvalidEvidence)?);
    }
    if !matches!(sum, Weight::Exact(_)) {
        return Err(Error::GroupAttributionUnavailable);
    }
    let aggregate = version
        .selection_total(&members, true, &mut *cancelled)
        .map_err(|e| match e {
            crate::query::SpaceTotalError::Cancelled => Error::Cancelled,
            _ => Error::GroupAttributionUnavailable,
        })?;
    let size = match basis {
        SizeBasis::Logical => aggregate.logical,
        SizeBasis::Allocated => aggregate.allocated,
        SizeBasis::UniquePhysical => aggregate.unique_physical,
    };
    if weight(&size)? == sum {
        return Ok(BarIdentity::Other { members, size });
    }
    // A selection with folder rows has no independent file membership in the set
    // total; children of one admitted disjoint partition are attributed instead.
    let size = partition_total(version, scope, basis, items, sum, cancelled)?;
    Ok(BarIdentity::Other { members, size })
}

/// The exact total of `items` when they are children of `scope` in a partition the
/// treemap admits as disjoint, and that total equals `sum`.
fn partition_total(
    version: &ResultVersion,
    scope: &ItemRef,
    basis: &SizeBasis,
    items: &[u32],
    sum: Weight,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<SizeValue, Error> {
    let admitted =
        treemap::admit_cancellable(version, scope, basis.clone(), cancelled).map_err(|error| {
            match error {
                treemap::AdmissionError::Cancelled => Error::Cancelled,
                _ => Error::GroupAttributionUnavailable,
            }
        })?;
    let wanted: HashSet<u32> = items.iter().copied().collect();
    let mut bytes = 0_u64;
    let mut evidence = SizeEvidence::Measured;
    let mut found = 0;
    for row in &admitted.rows {
        check(cancelled)?;
        if !wanted.contains(&row.child.item.item_no) {
            continue;
        }
        if !matches!(row.child.value.amount, SizeAmount::Exact { .. }) {
            return Err(Error::GroupAttributionUnavailable);
        }
        found += 1;
        bytes = bytes
            .checked_add(row.bytes)
            .ok_or(Error::GroupAttributionUnavailable)?;
        evidence = weaker(evidence, row.child.value.evidence.clone());
    }
    if found != wanted.len() || Weight::Exact(bytes) != sum {
        return Err(Error::GroupAttributionUnavailable);
    }
    Ok(SizeValue {
        basis: basis.clone(),
        evidence,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    })
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

fn weight(size: &SizeValue) -> Result<Weight, Error> {
    if size.evidence == SizeEvidence::Unknown {
        return Ok(Weight::Unknown);
    }
    match &size.amount {
        SizeAmount::Exact { bytes } => Ok(Weight::Exact(bytes.get())),
        SizeAmount::LowerBound { bytes } => Ok(Weight::LowerBound(bytes.get())),
        SizeAmount::Unknown => Ok(Weight::Unknown),
    }
}

fn check(cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
