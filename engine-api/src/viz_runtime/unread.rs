//! The summary lines of a treemap: the items it does not draw (F06 README 51) and the
//! folders its scope could not read (README 48, #346).
use super::internal;
use du_analysis_index::{
    ResultVersion,
    folder_collection::{FolderCollection, REASON_DATA_INSUFFICIENT, Unenumerated, UnreadInside},
    visualization::projection::{self, UnknownShare},
};
use du_core_contracts::{ItemRef, visualization::quantity::VizQuantity};

use crate::error::EngineError;

/// The unread folders below the model's scope and the table row of the first one.
///
/// The count is the scope's recorded gaps: folders whose listing failed. It is exact
/// once the scan sealed without cancellation or disconnection marks below the scope; a
/// mark leaves at least one more folder unread, and a running scan has counted only the
/// gaps so far, so both make it a lower bound. Nothing is reported for a sample, a scope
/// that was not read itself, or a scope with nothing unread.
pub(super) fn unread_folders(
    version: &ResultVersion,
    projection: &projection::Model,
) -> Result<(Option<VizQuantity>, Option<u32>), EngineError> {
    let parent = &projection.parent;
    let Some(inside) = version.unread_inside(parent).map_err(|_| internal())? else {
        return Ok((None, None));
    };
    let gaps = version
        .space(parent.item_no)
        .and_then(|aggregate| aggregate.inaccessible_ranges)
        .unwrap_or(0);
    let quantity = match inside {
        UnreadInside::NoneRecorded => None,
        UnreadInside::Recorded { count: Some(count) } => {
            (count > 0).then(|| VizQuantity::exact("items", i128::from(count)))
        }
        UnreadInside::Recorded { count: None } => {
            Some(VizQuantity::lower_bound("items", gaps.max(1)))
        }
        UnreadInside::Unknown { .. } => (gaps > 0).then(|| VizQuantity::lower_bound("items", gaps)),
    };
    let Some(quantity) = quantity else {
        return Ok((None, None));
    };
    let quantity = quantity.ok_or_else(internal)?;
    Ok((Some(quantity), first_row(version, projection)?))
}

/// The first root table row that is an unread folder or holds one, by table position.
fn first_row(
    version: &ResultVersion,
    projection: &projection::Model,
) -> Result<Option<u32>, EngineError> {
    let mut first: Option<usize> = None;
    for (index, row) in projection.rows.iter().enumerate() {
        let Some(position) = projection.table_position(index) else {
            continue;
        };
        if first.is_some_and(|first| first <= position) || !unread(version, &row.item)? {
            continue;
        }
        first = Some(position);
    }
    first
        .map(|position| u32::try_from(position).map_err(|_| internal()))
        .transpose()
}

fn unread(version: &ResultVersion, item: &ItemRef) -> Result<bool, EngineError> {
    let itself = matches!(
        version.folder_collection(item).map_err(|_| internal())?,
        FolderCollection::Folder {
            unenumerated: Unenumerated::Yes { .. },
            ..
        }
    );
    Ok(itself
        || matches!(
            version.unread_inside(item).map_err(|_| internal())?,
            Some(UnreadInside::Recorded { .. })
        ))
}

/// The share of the scope no root item could take as area (F06 README 51.3), in bytes.
/// Under a type condition the kept part of that share cannot be proven.
pub(super) fn unknown_size(share: UnknownShare) -> Option<Result<VizQuantity, EngineError>> {
    let quantity = match share {
        UnknownShare::None => return None,
        UnknownShare::Exact(bytes) => VizQuantity::exact("bytes", i128::from(bytes)),
        UnknownShare::LowerBound(bytes) => VizQuantity::lower_bound("bytes", bytes),
        UnknownShare::Unknown => VizQuantity::unknown("bytes", REASON_DATA_INSUFFICIENT.into()),
    };
    Some(quantity.ok_or_else(internal))
}

/// The table position of the first row without a provable area, or of the first
/// expanded folder holding one; the table drops the root group's members.
pub(super) fn first_unknown_row(
    projection: &projection::Model,
) -> Result<Option<u32>, EngineError> {
    projection
        .detail
        .first_unknown_row
        .map(|row| {
            projection
                .table_position(row)
                .and_then(|position| u32::try_from(position).ok())
                .ok_or_else(internal)
        })
        .transpose()
}
