//! Pinned, read-only hierarchy access for folder browsing.
use crate::{QueryError, ResultVersion, ensure_directory};
use du_core_contracts::{
    ItemRef,
    result::{ResultItem, ResultItemKind},
};

/// A directory and its immediate children in one immutable result version.
///
/// `breadcrumbs` contains the scope root through the selected directory,
/// inclusive. `children` is in stored order; query sorting belongs to F04.
pub struct FolderView<'a> {
    pub location: &'a ResultItem,
    pub breadcrumbs: Vec<&'a ResultItem>,
    pub children: &'a [ResultItem],
}

/// Default navigation without inventing an item for a forest's virtual top.
pub enum ScopeView<'a> {
    /// A single actual root retains the existing folder view.
    Single(FolderView<'a>),
    /// Multiple actual roots are children of a separate virtual location.
    Forest {
        roots: &'a [ResultItem],
        total: &'a du_core_contracts::result::SpaceAggregate,
    },
}

/// Opens the default navigation location for a single root or a forest.
pub fn open_scope(version: &ResultVersion) -> Result<ScopeView<'_>, QueryError> {
    match version.scope_roots().len() {
        0 => Err(QueryError::NotFound),
        1 => open(version, None).map(ScopeView::Single),
        _ => Ok(ScopeView::Forest {
            roots: version.scope_roots(),
            total: version.scope_total(),
        }),
    }
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
    Ok(FolderView {
        location: item,
        breadcrumbs: version.ancestors(item)?,
        children: version.children(item.item_no),
    })
}

/// Reports whether the immutable hierarchy contains an observed child directory.
/// A false result alone does not establish that the folder was fully enumerated.
pub fn has_observed_child_directory(
    version: &ResultVersion,
    folder: &ItemRef,
) -> Result<bool, QueryError> {
    let item = directory(version, Some(folder))?;
    Ok(version
        .children(item.item_no)
        .iter()
        .any(|child| child.kind == ResultItemKind::Directory))
}

/// Resolves a browse location without computing its breadcrumb list.
pub(crate) fn directory<'a>(
    version: &'a ResultVersion,
    location: Option<&ItemRef>,
) -> Result<&'a ResultItem, QueryError> {
    let item = match location {
        Some(reference) => version.detail(reference)?.item,
        None if version.scope_roots().len() == 1 => {
            version.items.first().ok_or(QueryError::NotFound)?
        }
        None => return Err(QueryError::PathUnavailable),
    };
    ensure_directory(item.kind)?;
    Ok(item)
}
