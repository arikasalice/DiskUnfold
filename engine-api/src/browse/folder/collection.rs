//! Per-folder scan state of the pinned version on the published folder rows.
//!
//! Only a scan-lineage version (a real data source) supplies these facts; it is the same
//! test that makes `folder_collection` answer `NotProvided`. A source that is not scan
//! lineage keeps the values it sent before folder scan state was wired.
use du_analysis_index::{
    ResultVersion,
    folder_collection::{
        FolderBoundary as Boundary, FolderCollection, Unenumerated, UnreadInside as Inside,
    },
};
use du_core_contracts::{
    ItemRef, OsPath, PathBytes, PathEncoding,
    browse_ipc::BrowseValue,
    browse_page_ipc::{FolderBoundary, FolderFlags, QueryTotal, UnreadInside},
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
    version
        .folder_collections(items)
        .map_err(folder_error)?
        .into_iter()
        .zip(items)
        .map(|(collection, item)| {
            let inside = version.unread_inside(item).map_err(folder_error)?;
            let group = group_facts(version, item, &collection);
            Ok(flags(collection, inside, group))
        })
        .collect()
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

fn flags(
    collection: FolderCollection<'_>,
    inside: Option<Inside>,
    group: GroupFacts,
) -> FolderFlags {
    // The collector keeps no excluded entry, so no row of a scan-lineage result is one.
    let excluded = BrowseValue::Present { value: false };
    match collection {
        FolderCollection::NotAFolder => FolderFlags {
            unenumerated: BrowseValue::Absent,
            excluded,
            boundary: None,
            unread_inside: None,
            counted_at: None,
            counted_at_item: None,
            group_volume_role: None,
        },
        FolderCollection::Folder {
            unenumerated,
            boundary,
        } => FolderFlags {
            unenumerated: unenumerated_value(unenumerated),
            excluded,
            boundary: boundary_value(boundary),
            unread_inside: inside.map(unread_inside_value),
            counted_at: group.counted_at,
            counted_at_item: group.counted_at_item,
            group_volume_role: group.role,
        },
    }
}

/// Root volume group facts of one folder row (#263), decided on the server from
/// the result's frozen group so the screen never compares paths.
#[derive(Default)]
struct GroupFacts {
    counted_at: Option<OsPath>,
    counted_at_item: Option<ItemRef>,
    role: Option<String>,
}

fn group_facts(
    version: &ResultVersion,
    item: &ItemRef,
    collection: &FolderCollection<'_>,
) -> GroupFacts {
    let Some((group, absolute, root)) = group_path(version, item) else {
        return GroupFacts::default();
    };
    let role = (absolute == group.data_mount).then(|| FolderFlags::GROUP_ROLE_DATA.to_owned());
    let already_counted = matches!(
        collection,
        FolderCollection::Folder {
            boundary: Boundary::AlreadyCounted,
            ..
        }
    );
    let source = already_counted
        .then(|| group.firmlinks.iter().find(|link| link.target == absolute))
        .flatten()
        .map(|link| link.source.as_slice());
    GroupFacts {
        counted_at: source.map(unix_path),
        counted_at_item: source.and_then(|source| item_at_path(version, root, source)),
        role,
    }
}

/// The confirmed group, the folder's absolute path and the root item, for a
/// result scanned from the Unix root `/`.
fn group_path<'a>(
    version: &'a ResultVersion,
    item: &ItemRef,
) -> Option<(&'a du_core_contracts::volume::RootVolumeGroup, Vec<u8>, u32)> {
    let group = version.volume_group()?;
    let (_, root) = version.observed_root_for(item).ok()?;
    if root.path.encoding != PathEncoding::Unix || root.path.raw.bytes().ok()? != b"/" {
        return None;
    }
    let items = version.items();
    let mut index = item.item_no as usize;
    let mut components = Vec::new();
    let root_item = loop {
        let row = items.get(index)?;
        let Some(parent) = row.parent else {
            break row.item_no;
        };
        components.push(row.name.as_slice());
        index = parent as usize;
    };
    let mut absolute = b"/".to_vec();
    for component in components.into_iter().rev() {
        if component.is_empty() {
            return None;
        }
        if absolute.last() != Some(&b'/') {
            absolute.push(b'/');
        }
        absolute.extend_from_slice(component);
    }
    Some((group, absolute, root_item))
}

/// The folder at an absolute path below the root item, in the same version.
fn item_at_path(version: &ResultVersion, root: u32, path: &[u8]) -> Option<ItemRef> {
    let mut current = root;
    for component in path
        .split(|&byte| byte == b'/')
        .filter(|part| !part.is_empty())
    {
        current = version.child_named(current, component)?.item_no;
    }
    (current != root).then(|| ItemRef {
        result_version: version.version_id().clone(),
        item_no: current,
    })
}

fn unix_path(raw: &[u8]) -> OsPath {
    OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(raw),
        lossy: std::str::from_utf8(raw).is_err(),
        display: String::from_utf8_lossy(raw).into_owned(),
    }
}

fn unread_inside_value(value: Inside) -> BrowseValue<UnreadInside> {
    match value {
        Inside::Unknown { reason_key } => super::super::unknown(reason_key),
        Inside::NoneRecorded => BrowseValue::Absent,
        Inside::Recorded { count } => BrowseValue::Present {
            value: UnreadInside {
                count: count.map(|count| count.to_string()),
            },
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
        unread_inside: None,
        counted_at: None,
        counted_at_item: None,
        group_volume_role: None,
    }
}

#[cfg(test)]
mod tests;
