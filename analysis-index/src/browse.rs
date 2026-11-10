//! Pinned, read-only hierarchy access for folder browsing.
use crate::{QueryError, ResultVersion, ensure_directory};
use du_core_contracts::{ItemRef, result::ResultItem};

/// A directory and its immediate children in one immutable result version.
///
/// `breadcrumbs` contains the scope root through the selected directory,
/// inclusive. `children` is in stored order; query sorting belongs to F04.
pub struct FolderView<'a> {
    pub location: &'a ResultItem,
    pub breadcrumbs: Vec<&'a ResultItem>,
    pub children: &'a [ResultItem],
}

/// Opens a confirmed directory in an already pinned result version.
///
/// `None` selects the scope root. A reference from another version fails with
/// `StaleVersion`; an absent item, non-directory, or unknown kind is reported
/// separately. No filesystem I/O or result-version lookup occurs here.
pub fn open<'a>(
    version: &'a ResultVersion,
    location: Option<&ItemRef>,
) -> Result<FolderView<'a>, QueryError> {
    let item = directory(version, location)?;
    let mut breadcrumbs = Vec::new();
    let mut current = item;
    loop {
        breadcrumbs.push(current);
        let Some(parent) = current.parent else {
            break;
        };
        current = version
            .items
            .get(parent as usize)
            .ok_or(QueryError::NotFound)?;
    }
    breadcrumbs.reverse();
    Ok(FolderView {
        location: item,
        breadcrumbs,
        children: version.children(item.item_no),
    })
}

/// Resolves a browse location without computing its breadcrumb list.
pub(crate) fn directory<'a>(
    version: &'a ResultVersion,
    location: Option<&ItemRef>,
) -> Result<&'a ResultItem, QueryError> {
    let item = match location {
        Some(reference) => version.detail(reference)?.item,
        None => version.items.first().ok_or(QueryError::NotFound)?,
    };
    ensure_directory(item.kind)?;
    Ok(item)
}
