//! Folder row projection from the pinned page and its observed evidence.
use du_analysis_index::{browse, query::page::Page};
use du_core_contracts::{SizeBasis, result::ResultItemKind};

use super::{Error, FolderPage, GapEvidence, Row, folder_error};

pub(super) fn project(page: Page) -> Result<FolderPage, Error> {
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
    let next = u64::try_from(page.next_position()).map_err(|_| Error::Internal)?;
    Ok(FolderPage {
        query_ref: page.query_ref().clone(),
        result_version: metadata.version.clone(),
        rows,
        total: metadata.total,
        unknown_count: metadata.unknown_count,
        completeness: metadata.completeness,
        observed_at: metadata.observed_at.cloned(),
        next_position: page.has_next().then_some(next),
    })
}
