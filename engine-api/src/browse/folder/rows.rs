//! Folder row projection from the pinned page and its observed evidence.
use du_analysis_index::{
    browse,
    query::page::{Page, Total},
};
use du_core_contracts::{Id, SizeBasis, result::ResultItemKind};

use super::{Error, FolderPage, GapEvidence, Row, folder_error};

pub(super) fn project(query_ref: Id, page: Page) -> Result<FolderPage, Error> {
    let version = page.version();
    let rows = page
        .items()
        .iter()
        .map(|reference| {
            let item = version.detail(reference).map_err(folder_error)?.item;
            Ok(Row {
                item: reference.clone(),
                display_name: super::super::item_display_name(version, reference, item),
                observation: item.clone(),
                logical_size: version.item_size(item.item_no, SizeBasis::Logical).cloned(),
                allocated_size: version
                    .item_size(item.item_no, SizeBasis::Allocated)
                    .cloned(),
                modified_at: version.item_modified_at(item.item_no).cloned(),
                reported_gaps: version
                    .item_scope_gaps(reference)
                    .map_err(folder_error)?
                    .map(|gap| GapEvidence {
                        reason_key: gap.reason_key.clone(),
                        observed_at: gap.observed_at.clone(),
                    })
                    .collect(),
                has_observed_child_folder: if item.kind == ResultItemKind::Directory {
                    browse::has_observed_child_directory(version, reference)
                        .map_err(folder_error)?
                        .then_some(true)
                } else {
                    None
                },
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let metadata = page.metadata();
    let known_count = match metadata.total {
        Total::Exact(n) | Total::LowerBound(n) => n,
    };
    let next = u64::try_from(page.next_position()).map_err(|_| Error::Internal)?;
    Ok(FolderPage {
        query_ref,
        result_version: metadata.version.clone(),
        rows,
        total: metadata.total,
        unknown_count: metadata.unknown_count,
        completeness: metadata.completeness,
        observed_at: metadata.observed_at.cloned(),
        next_position: (next < known_count).then_some(next),
    })
}
