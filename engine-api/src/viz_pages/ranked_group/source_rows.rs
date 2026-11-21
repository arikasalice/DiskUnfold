//! The rows of a ranked model: the children with their original or kept values.
use super::*;

pub(super) fn original_rows(
    origin: Origin<'_>,
    selected: &SelectedBar,
    fingerprint: &str,
    presentations: Vec<production::RowPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<HashMap<u32, VizRow>, Error> {
    let Origin {
        source,
        scope,
        basis,
        filter,
    } = origin;
    let narrowed = filter.is_some();
    let map_error = |error| {
        if error == production::Error::Cancelled {
            Error::Cancelled
        } else {
            Error::InvalidModel
        }
    };
    // Narrowed, the rows are the ranking's children with their kept bytes; the
    // filter removed the rest, which the ranking counts apart (README 47).
    let rows = if narrowed {
        let sized: Vec<_> = selected
            .output
            .rows
            .iter()
            .map(|row| (row.item.clone(), row.size.clone()))
            .collect();
        let shown: HashSet<u32> = sized.iter().map(|(item, _)| item.item_no).collect();
        let presentations = presentations
            .into_iter()
            .filter(|presentation| shown.contains(&presentation.item.item_no))
            .collect();
        production::project_sized(source, scope, basis, &sized, presentations, cancelled)
            .map_err(map_error)?
    } else {
        production::project_fallback(
            source.clone(),
            scope,
            basis,
            fingerprint.to_owned(),
            presentations,
            cancelled,
        )
        .map_err(map_error)?
        .rows
    };
    let scope_total = source.space(scope.item_no).ok_or(Error::InvalidModel)?;
    let scope_total = match basis {
        SizeBasis::Logical => &scope_total.logical,
        SizeBasis::Allocated => &scope_total.allocated,
        SizeBasis::UniquePhysical => &scope_total.unique_physical,
    };
    if selected.output.scope_total.result_version != *source.version_id()
        || selected.output.scope_total.target
            != du_analysis_index::visualization::bar_projection::AggregateTarget::Folder(
                scope.clone(),
            )
        || &selected.output.scope_total.size != scope_total
        || selected.output.denominator != selected.output.scope_total
    {
        return Err(Error::InvalidModel);
    }
    let mut sum = selected.output.filtered_bytes;
    for row in &rows {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        sum = sum
            .checked_add(exact(&row.size).ok_or(Error::InvalidModel)?)
            .ok_or(Error::InvalidModel)?;
    }
    if Some(sum) != exact(scope_total) {
        return Err(Error::InvalidModel);
    }
    let mut by_item = HashMap::new();
    for row in rows {
        let item = row.item_ref.as_ref().ok_or(Error::InvalidModel)?;
        if by_item.insert(item.item_no, row).is_some() {
            return Err(Error::InvalidModel);
        }
    }
    Ok(by_item)
}
