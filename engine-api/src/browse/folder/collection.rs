//! Per-folder scan state of the pinned version on the published folder rows.
//!
//! Only a scan-lineage version (a real data source) supplies these facts; it is the same
//! test that makes `folder_collection` answer `NotProvided`. A source that is not scan
//! lineage keeps the values it sent before folder scan state was wired.
use du_analysis_index::{
    ResultVersion,
    folder_collection::{FolderBoundary as Boundary, FolderCollection, Unenumerated},
};
use du_core_contracts::{
    ItemRef,
    browse_ipc::BrowseValue,
    browse_page_ipc::{FolderBoundary, FolderFlags, QueryTotal},
    result::ResultVersionRead,
};

use super::{Error, folder_error};

/// Answers the flags of every reference of one page, in page order, with one lookup.
///
/// # Errors
/// A reference the pinned version cannot answer fails the whole page, mapped as the
/// other row facts of the page are.
pub(super) fn page_flags(
    version: &ResultVersion,
    items: &[ItemRef],
) -> Result<Vec<FolderFlags>, Error> {
    if !version.data_source().is_real() {
        return Ok(items.iter().map(|_| not_provided()).collect());
    }
    Ok(version
        .folder_collections(items)
        .map_err(folder_error)?
        .into_iter()
        .map(flags)
        .collect())
}

/// Entries an exclusion rule left out directly under the opened folder.
///
/// `None` at the virtual top level, for a source that is not scan lineage and when the
/// pinned version cannot tell.
///
/// # Errors
/// The location was admitted as a folder of `version`; a failure is mapped as the
/// location's own admission failure.
pub(super) fn excluded_children(
    version: &ResultVersion,
    location: Option<&ItemRef>,
) -> Result<Option<QueryTotal>, Error> {
    let Some(location) = location else {
        return Ok(None);
    };
    if !version.data_source().is_real() {
        return Ok(None);
    }
    Ok(version
        .folder_excluded_children(location)
        .map_err(folder_error)?
        .map(|children| match children.exact {
            true => QueryTotal::Exact {
                count: children.count,
            },
            false => QueryTotal::LowerBound {
                count: children.count,
            },
        }))
}

fn flags(collection: FolderCollection<'_>) -> FolderFlags {
    // The collector keeps no excluded entry, so no row of a scan-lineage result is one.
    let excluded = BrowseValue::Present { value: false };
    match collection {
        FolderCollection::NotAFolder => FolderFlags {
            unenumerated: BrowseValue::Absent,
            excluded,
            boundary: None,
        },
        FolderCollection::Folder {
            unenumerated,
            boundary,
        } => FolderFlags {
            unenumerated: unenumerated_value(unenumerated),
            excluded,
            boundary: boundary_value(boundary),
        },
    }
}

fn unenumerated_value(value: Unenumerated<'_>) -> BrowseValue<bool> {
    match value {
        // The cause is read in the scan issue list and the result completeness.
        Unenumerated::Yes { .. } => BrowseValue::Present { value: true },
        Unenumerated::No => BrowseValue::Present { value: false },
        Unenumerated::Unknown { reason_key } => super::super::unknown(reason_key),
    }
}

fn boundary_value(value: Boundary) -> Option<BrowseValue<FolderBoundary>> {
    let present = |name| {
        Some(BrowseValue::Present {
            value: FolderBoundary::new(name),
        })
    };
    match value {
        Boundary::OtherVolume => present(FolderBoundary::OTHER_VOLUME),
        Boundary::AlreadyCounted => present(FolderBoundary::ALREADY_COUNTED),
        Boundary::NoBoundary => Some(BrowseValue::Absent),
        Boundary::Unknown { reason_key } => Some(super::super::unknown(reason_key)),
        Boundary::NotProvided => None,
    }
}

/// The flags a source that is not scan lineage sends.
fn not_provided() -> FolderFlags {
    FolderFlags {
        unenumerated: super::super::unknown("ref.unavailable"),
        excluded: super::super::unknown("ref.unavailable"),
        boundary: None,
    }
}
