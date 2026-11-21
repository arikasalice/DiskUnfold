//! Ranked children of a folder under a filter (F06 #301): each child by the bytes
//! of the files the filter keeps, decided by the same attribution as the treemap.
use std::collections::HashMap;

use du_core_contracts::result::ResultVersionRead;

use super::{
    AggregateEvidence, AggregateTarget, ChildrenRequest, Error, Prepared, Row, bar, check, weight,
};
use crate::visualization::{filter::ItemFilter, root_ref, treemap, value};

/// Freezes the kept bytes of each immediate child and the scope's original total.
///
/// A child that keeps nothing has no row. A child whose kept share cannot be
/// proven stays with an unknown value, which the ranking reports as unknown. The
/// bytes the filter removed from attributed children are kept apart.
pub(super) fn prepare<'a>(
    request: ChildrenRequest<'a>,
    filter: ItemFilter,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Prepared<'a>, Error> {
    check(cancelled)?;
    let version = request.version;
    let scope_no = root_ref(version, request.scope).map_err(|_| Error::InvalidEvidence)?;
    let total = AggregateEvidence {
        result_version: version.version_id().clone(),
        target: AggregateTarget::Folder(request.scope.clone()),
        size: value(version, scope_no, &request.basis).map_err(|_| Error::InvalidEvidence)?,
    };
    let partial = treemap::admit_filtered_cancellable(
        version,
        request.scope,
        &request.basis,
        Some(filter),
        cancelled,
    )
    .map_err(|error| match error {
        treemap::AdmissionError::Cancelled => Error::Cancelled,
        _ => Error::InvalidEvidence,
    })?;
    let kept = partial
        .rows
        .into_iter()
        .map(|row| (row.child.item, row.child.value));
    let unproven = partial
        .unattributed
        .into_iter()
        .map(|row| (row.item, row.value));
    let mut rows = Vec::new();
    let mut input = Vec::new();
    let mut sources = HashMap::new();
    for (item, size) in kept.chain(unproven) {
        check(cancelled)?;
        input.push(bar::Item {
            item_no: item.item_no,
            value: weight(&size)?,
        });
        sources.insert(item.item_no, item.clone());
        rows.push(Row { item, size });
    }
    Ok(Prepared {
        version,
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
        filtered_bytes: partial.filtered_bytes,
    })
}
