//! The rows of the group that merged a parent's small siblings.
//!
//! The table keeps one row for every sibling except the members of the group. The
//! group is one row of its own, in the position of its shape, and its members are
//! read through viz.group_rows. Nothing is recomputed: the total, the size range and
//! the members come from the projection, and only their consistency is checked.
use super::{Error, OtherPresentation, quantity};
use du_analysis_index::{
    ResultVersion,
    visualization::{
        live_models::GroupRows,
        projection::{self, Lod},
        ratio,
    },
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeValue,
    result::ResultVersionRead,
    visualization::rows::{VizDenominator, VizRatio, VizRow},
};
use std::collections::HashMap;

/// Splits the member rows out of the table and adds the group row.
///
/// rows is one row per child of the projection's parent, in the projection's order.
/// A projection without a group is returned unchanged.
pub(super) fn assemble(
    version: &ResultVersion,
    model: &projection::Model,
    rows: Vec<VizRow>,
    other: Option<&OtherPresentation>,
    denominator: &VizDenominator,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(Vec<VizRow>, HashMap<String, GroupRows>), Error> {
    let mut groups = HashMap::new();
    let Some(group) = model.detail.groups.first() else {
        if model.totals.grouped_bytes != 0
            || model
                .rows
                .iter()
                .any(|row| matches!(row.lod, Lod::Grouped(_)))
        {
            return Err(Error::InvalidGroup);
        }
        return Ok((rows, groups));
    };
    // Groups of expanded folders are registered when the expansion is added.
    if model.detail.groups.len() != 1 || group.parent != model.parent {
        return Err(Error::InvalidGroup);
    }
    let other = usable(other)?;
    let mut table = Vec::with_capacity(rows.len());
    let mut members = Vec::with_capacity(group.members.len());
    for (row, source) in rows.into_iter().zip(&model.rows) {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        match source.lod {
            Lod::Grouped(0) => members.push((row, &source.item)),
            Lod::Grouped(_) => return Err(Error::InvalidGroup),
            Lod::Drawn | Lod::Omitted => table.push(row),
        }
    }
    check_members(model, &members, group, cancelled)?;
    if group.slot > table.len() {
        return Err(Error::InvalidGroup);
    }
    let quantity = quantity(&group.total)?;
    let ratio = ratio::calculate(&quantity, &denominator.value)
        .as_deref()
        .map(VizRatio::parse)
        .transpose()
        .map_err(|_| Error::InvalidRatio)?;
    let mut display_flags = other.display_flags.clone();
    display_flags.sort_unstable();
    display_flags.dedup();
    let row = VizRow {
        item_ref: None,
        group_ref: Some(group.group_ref.clone()),
        display_name: other.display_name.clone(),
        size: group.total.clone(),
        quantity: Some(quantity),
        ratio,
        denominator: denominator.clone(),
        legend_class_key: other.legend_class_key.clone(),
        display_flags,
        label_key: other.label_key.clone(),
    };
    row.validate_revision(version.version_id())
        .map_err(|_| Error::InvalidGroup)?;
    table.insert(group.slot, row);
    groups.insert(
        group.group_ref.clone(),
        GroupRows {
            rows: members.into_iter().map(|(row, _)| row).collect(),
            total: group.total.clone(),
            size_range: group.size_range.clone(),
        },
    );
    Ok((table, groups))
}

fn usable(other: Option<&OtherPresentation>) -> Result<&OtherPresentation, Error> {
    other
        .filter(|other| {
            !other.display_name.is_empty()
                && !other.legend_class_key.is_empty()
                && other.display_flags.iter().all(|flag| !flag.is_empty())
                && other.label_key.as_ref().is_none_or(|key| !key.is_empty())
        })
        .ok_or(Error::MissingGroupPresentation)
}

// The member rows must be the group's members in order, their sizes must be the F03
// values the group was built from, and they must add up to the group total.
fn check_members(
    model: &projection::Model,
    members: &[(VizRow, &ItemRef)],
    group: &projection::Group,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Error> {
    if members.len() != group.members.len() || members.is_empty() {
        return Err(Error::InvalidGroup);
    }
    let mut sum = 0_u64;
    for ((row, item), member) in members.iter().zip(&group.members) {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if **item != member.item || row.size != member.size {
            return Err(Error::InvalidGroup);
        }
        sum = sum
            .checked_add(bytes(&member.size).ok_or(Error::InvalidGroup)?)
            .ok_or(Error::InvalidGroup)?;
    }
    let lower = matches!(group.total.amount, SizeAmount::LowerBound { .. });
    if bytes(&group.total) != Some(sum)
        || sum != model.totals.grouped_bytes
        || model.totals.grouped_lower_bound != lower
    {
        return Err(Error::InvalidGroup);
    }
    Ok(())
}

fn bytes(size: &SizeValue) -> Option<u64> {
    match &size.amount {
        SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => Some(bytes.get()),
        SizeAmount::Unknown => None,
    }
}
