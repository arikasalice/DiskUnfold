//! Walks the files below a classification scope in observed path order.

use du_core_contracts::{
    ItemRef,
    result::{ResultItem, ResultItemKind},
};

use super::{Control, Error};
use crate::ResultVersion;

/// Items are visited this many apart between cancellation checks.
const CANCEL_INTERVAL: usize = 4096;

/// The member files of a scope and whether the source covers all of it.
pub(super) struct ScopeFiles {
    /// Item numbers of the files in observed path order.
    pub(super) files: Vec<u32>,
    /// False when the scope holds an item that is neither a file nor a folder.
    pub(super) coverage_complete: bool,
}

/// Counts the files that a classification of the same scope would evaluate.
///
/// `root` is `None` for the whole result (every scope root, including a forest) or a folder
/// of this result version. The count is the progress total of a classification.
///
/// # Errors
/// Returns [`Error::InvalidRoot`] when `root` is not a folder of `version`.
pub fn scope_file_count(version: &ResultVersion, root: Option<&ItemRef>) -> Result<u64, Error> {
    let mut count = 0u64;
    walk(version, root, |item| {
        count += u64::from(item.kind == ResultItemKind::File);
        Ok(())
    })?;
    Ok(count)
}

/// Checks that the scope root is acceptable without visiting any item.
pub(super) fn validate_root(version: &ResultVersion, root: Option<&ItemRef>) -> Result<(), Error> {
    starting_items(version, root).map(|_| ())
}

/// Collects the member files, checking for cancellation at regular intervals.
pub(super) fn collect(
    version: &ResultVersion,
    root: Option<&ItemRef>,
    control: &dyn Control,
) -> Result<ScopeFiles, Error> {
    let mut files = Vec::new();
    let mut visited = 0usize;
    let coverage_complete = walk(version, root, |item| {
        visited += 1;
        if visited.is_multiple_of(CANCEL_INTERVAL) && control.cancelled() {
            return Err(Error::Cancelled);
        }
        if item.kind == ResultItemKind::File {
            files.push(item.item_no);
        }
        Ok(())
    })?;
    Ok(ScopeFiles {
        files,
        coverage_complete,
    })
}

fn starting_items<'a>(
    version: &'a ResultVersion,
    root: Option<&ItemRef>,
) -> Result<&'a [ResultItem], Error> {
    let Some(root) = root else {
        return Ok(version.scope_roots());
    };
    let folder = version.detail(root).map_err(|_| Error::InvalidRoot)?.item;
    if folder.kind != ResultItemKind::Directory {
        return Err(Error::InvalidRoot);
    }
    Ok(version.children(folder.item_no))
}

/// Visits every item below the scope, parents before children and siblings in stored path
/// order, without recursion. Returns whether every visited item was a file or a folder.
fn walk(
    version: &ResultVersion,
    root: Option<&ItemRef>,
    mut visit: impl FnMut(&ResultItem) -> Result<(), Error>,
) -> Result<bool, Error> {
    let mut complete = true;
    let mut branches = vec![starting_items(version, root)?.iter()];
    while let Some(branch) = branches.last_mut() {
        let Some(item) = branch.next() else {
            branches.pop();
            continue;
        };
        visit(item)?;
        complete &= matches!(item.kind, ResultItemKind::File | ResultItemKind::Directory);
        let children = version.children(item.item_no);
        if !children.is_empty() {
            branches.push(children.iter());
        }
    }
    Ok(complete)
}
