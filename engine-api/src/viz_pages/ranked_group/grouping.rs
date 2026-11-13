//! F03 verification and group-row assembly.
use super::*;

pub(super) struct GroupRequest<'a> {
    pub(super) source: &'a ResultVersion,
    pub(super) basis: &'a SizeBasis,
    pub(super) members: Vec<ItemRef>,
    pub(super) size: SizeValue,
    pub(super) by_item: &'a mut HashMap<u32, VizRow>,
    pub(super) seen: &'a mut HashSet<u32>,
    pub(super) other: &'a OtherPresentation,
    pub(super) cancelled: &'a mut dyn FnMut() -> bool,
}

pub(super) fn make_group(request: GroupRequest<'_>) -> Result<(VizRow, GroupRows), Error> {
    let GroupRequest {
        source,
        basis,
        members,
        size,
        by_item,
        seen,
        other,
        cancelled,
    } = request;
    if members.is_empty() {
        return Err(Error::InvalidModel);
    }
    let mut rows = Vec::with_capacity(members.len());
    for item in &members {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if item.result_version != *source.version_id() || !seen.insert(item.item_no) {
            return Err(Error::InvalidModel);
        }
        let row = by_item.remove(&item.item_no).ok_or(Error::InvalidModel)?;
        if row.item_ref.as_ref() != Some(item) {
            return Err(Error::InvalidModel);
        }
        rows.push(row);
    }
    let sum = source
        .selection_total(&members, true, &mut *cancelled)
        .map_err(|_| {
            if cancelled() {
                Error::Cancelled
            } else {
                Error::InvalidModel
            }
        })?;
    let total = match basis {
        SizeBasis::Logical => sum.logical,
        SizeBasis::Allocated => sum.allocated,
        SizeBasis::UniquePhysical => sum.unique_physical,
    };
    if total != size || exact(&size).is_none() {
        return Err(Error::InvalidModel);
    }
    let denominator = rows.first().ok_or(Error::InvalidModel)?.denominator.clone();
    let quantity = VizQuantity::exact(
        "bytes",
        i128::from(exact(&size).ok_or(Error::InvalidModel)?),
    )
    .ok_or(Error::InvalidModel)?;
    let row_ratio = ratio::calculate(&quantity, &denominator.value)
        .as_deref()
        .map(du_core_contracts::visualization::rows::VizRatio::parse)
        .transpose()
        .map_err(|_| Error::InvalidModel)?;
    let row = VizRow {
        item_ref: None,
        group_ref: Some(other.group_ref.clone()),
        display_name: other.display_name.clone(),
        size: size.clone(),
        quantity: Some(quantity),
        ratio: row_ratio,
        denominator,
        legend_class_key: other.legend_class_key.clone(),
        display_flags: other.display_flags.clone(),
    };
    Ok((
        row,
        GroupRows {
            rows,
            total: size,
            size_range: SizeRange::Unknown,
        },
    ))
}
