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
    root_ref, value,
};
use crate::ResultVersion;

/// Source identity and original F03 size for one ranked candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub item: ItemRef,
    pub size: SizeValue,
}

/// Identifies the pinned F03 aggregate that supplies a denominator.
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
/// Other is accepted only if F03 independently confirms an exact group total
/// equal to the layout sum. Unverifiable overlapping or incomplete groups fail
/// closed. `max_bars` limits returned bars, including Other, without truncation.
pub fn project(
    request: Request<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Ranking, Error> {
    check(cancelled)?;
    let scope = root_ref(request.version, request.scope).map_err(|_| Error::InvalidEvidence)?;
    if request.scope_total.target != AggregateTarget::Folder(request.scope.clone()) {
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
    let layout = bar::ranking_cancellable(
        request.bounds,
        &input,
        request.top,
        request.scale,
        request.version.completeness(),
        cancelled,
    )
    .map_err(|error| {
        if error == bar::Error::Cancelled {
            Error::Cancelled
        } else {
            Error::Layout(error)
        }
    })?;
    if layout.bars.len() > request.max_bars {
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
            bar::BarRef::Item(item) => {
                BarIdentity::Item(sources.get(item).cloned().ok_or(Error::InvalidEvidence)?)
            }
            bar::BarRef::Other(items) => verified_other(
                request.version,
                &request.basis,
                items,
                &sources,
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
        unknown_items.push(sources.get(item).cloned().ok_or(Error::InvalidEvidence)?);
    }
    check(cancelled)?;
    Ok(Ranking {
        result_version: request.version.version_id().clone(),
        rows: retained_rows,
        bars,
        unknown_items,
        scope_total: request.scope_total.clone(),
        denominator: request.denominator.clone(),
        completeness: layout.source_completeness,
        ranking_incomplete: layout.ranking_incomplete,
        axis_max: layout.axis_max,
    })
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
    if !matches!(sum, Weight::Exact(_)) || weight(&size)? != sum {
        return Err(Error::GroupAttributionUnavailable);
    }
    Ok(BarIdentity::Other { members, size })
}

fn weight(size: &SizeValue) -> Result<Weight, Error> {
    if size.evidence == SizeEvidence::Unknown {
        return Ok(Weight::Unknown);
    }
    match &size.amount {
        SizeAmount::Exact { bytes } => bytes
            .as_str()
            .parse()
            .map(Weight::Exact)
            .map_err(|_| Error::InvalidEvidence),
        SizeAmount::LowerBound { bytes } => bytes
            .as_str()
            .parse()
            .map(Weight::LowerBound)
            .map_err(|_| Error::InvalidEvidence),
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
