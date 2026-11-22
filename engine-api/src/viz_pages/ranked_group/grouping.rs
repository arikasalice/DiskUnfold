//! F03 verification and group-row assembly.
use super::*;

pub(super) struct GroupRequest<'a> {
    pub(super) source: &'a ResultVersion,
    pub(super) scope: &'a ItemRef,
    pub(super) basis: &'a SizeBasis,
    pub(super) members: Vec<ItemRef>,
    pub(super) size: SizeValue,
    pub(super) by_item: &'a mut HashMap<u32, VizRow>,
    pub(super) seen: &'a mut HashSet<u32>,
    pub(super) other: &'a OtherPresentation,
    pub(super) filter: Option<du_analysis_index::visualization::filter::ItemFilter>,
    pub(super) cancelled: &'a mut dyn FnMut() -> bool,
}

pub(super) fn make_group(request: GroupRequest<'_>) -> Result<(VizRow, GroupRows), Error> {
    let GroupRequest {
        source,
        scope,
        basis,
        members,
        size,
        by_item,
        seen,
        other,
        filter,
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
    let bytes = exact(&size).ok_or(Error::InvalidModel)?;
    // The same independent check as the ranking, folder members included (#242).
    let total = bar_projection::other_total(
        source,
        scope,
        basis,
        &members,
        Weight::Exact(bytes),
        filter,
        &mut *cancelled,
    )
    .map_err(|error| match error {
        bar_projection::Error::Cancelled => Error::Cancelled,
        _ => Error::InvalidModel,
    })?;
    if total != size {
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
        label_key: other.label_key.clone(),
        // The member rows are the whole group, so this count equals its totalCount.
        member_count: Some(
            VizQuantity::exact(
                "items",
                i128::try_from(rows.len()).map_err(|_| Error::InvalidModel)?,
            )
            .ok_or(Error::InvalidModel)?,
        ),
        item_kind: None,
        condition: None,
        shares_space: None,
        shared_counted_item: None,
        unread: None,
        type_slices: crate::viz_pages::type_slices::group_slices(
            source,
            &members,
            basis,
            other.type_slices && filter.is_none(),
            cancelled,
        )
        .map_err(|error| match error {
            crate::viz_pages::type_slices::SlicesError::Cancelled => Error::Cancelled,
            crate::viz_pages::type_slices::SlicesError::Invalid => Error::InvalidModel,
        })?,
        links: None,
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
