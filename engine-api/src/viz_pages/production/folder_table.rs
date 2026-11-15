//! The table of one expanded folder: its drawn children and the row of its group.
//!
//! A folder opened inside the treemap draws some of its children as shapes of their own
//! and merges the small ones into a group. The page for that folder lists exactly
//! those, in the order of the shapes and then the group, so that every shape inside the
//! folder has a row that names it, by its item reference or its group reference. A child
//! that has no shape (counted as unknown or left behind its count) is not in this
//! table; the root table still lists every child of the scope.
use super::{
    Error, OtherPresentation, RowPresentation, denominator, groups, make_row, original_size,
};
use du_analysis_index::{
    ResultVersion,
    visualization::projection::{self, DescendantShape},
};
use du_core_contracts::{ItemRef, result::ResultVersionRead, visualization::rows::VizRow};
use std::collections::HashMap;

/// Builds the rows of the folder `folder` from what its expansion added to `model`.
///
/// `children` are the shapes added directly under the folder, in projection order, and
/// `group` is the group that merged its small children. `presentations` holds the
/// classification of every child of the folder. The group row needs `other`; without it
/// the projection cannot have made a group.
pub(crate) fn rows(
    version: &ResultVersion,
    model: &projection::Model,
    folder: &ItemRef,
    children: &[DescendantShape],
    group: Option<&projection::Group>,
    presentations: &HashMap<u32, RowPresentation>,
    other: Option<&OtherPresentation>,
) -> Result<Vec<VizRow>, Error> {
    let denominator = denominator(&model.totals.scope, &model.basis)?;
    let mut table = Vec::with_capacity(children.len() + usize::from(group.is_some()));
    for shape in children {
        let item = version
            .items()
            .get(shape.item.item_no as usize)
            .filter(|item| item.parent == Some(folder.item_no))
            .ok_or(Error::SourceMismatch)?;
        if shape.item.result_version != model.result_revision
            || original_size(version, item.item_no, &model.basis) != Some(&shape.size)
        {
            return Err(Error::SourceMismatch);
        }
        let presentation = presentations
            .get(&item.item_no)
            .ok_or(Error::MissingClassification)?;
        table.push(make_row(
            version,
            &item.name,
            &shape.size,
            RowPresentation {
                item: presentation.item.clone(),
                legend_class_key: presentation.legend_class_key.clone(),
                display_flags: presentation.display_flags.clone(),
            },
            &denominator,
        )?);
    }
    if let Some(group) = group {
        if group.parent != *folder {
            return Err(Error::InvalidGroup);
        }
        let other = groups::usable(other)?;
        table.push(groups::group_row(version, group, other, &denominator)?);
    }
    Ok(table)
}
