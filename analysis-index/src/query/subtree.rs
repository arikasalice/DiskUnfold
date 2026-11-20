//! The subtree access path of query.run (F04 DLD, query plan step 3): the items
//! below one folder at any depth, or below every scope root, evaluated as a search.
use super::{
    Condition, Error, FolderLocation, QueryResult, Registry, Sort, page::RowKind,
    run_in_result_with_sorts,
};
use crate::ResultVersion;
use du_core_contracts::{ItemRef, result::ResultVersionRead};
use std::sync::Arc;

/// Evaluates a condition over every item below a folder, or below every scope
/// root, and returns a search result: rows from many folders, each with its path.
///
/// The folder and the roots themselves are not candidates. Folders below them are,
/// so the condition decides whether a folder is listed. Cancellation returns no
/// result.
///
/// # Errors
/// Returns cancellation, a location that is not a folder of the version, or an
/// invalid condition or sort.
pub fn run_subtree_with_sorts(
    registry: &Registry,
    version: Arc<ResultVersion>,
    location: FolderLocation,
    condition: &Condition,
    first: Sort<'_>,
    additional: &[Sort<'_>],
    mut cancelled: impl FnMut() -> bool,
) -> Result<QueryResult, Error> {
    let items = below(&version, location, &mut cancelled)?;
    let candidates = QueryResult {
        version: version.clone(),
        items,
        unknown_count: 0,
        condition: Condition::All(vec![]),
        predicate_strategies: Vec::new(),
        sort_strategy: (first.id.to_owned(), first.revision),
        sort_basis: first.basis,
        sort_descending: first.descending,
        sorts: Vec::new(),
        folder: Some(location),
        row_kind: RowKind::Search,
        inherited_uncertainty: false,
        limitations: &[],
    };
    run_in_result_with_sorts(
        registry,
        &candidates,
        condition,
        first,
        additional,
        cancelled,
    )
}

/// Every item below the location, depth first; the location itself is left out.
fn below(
    version: &ResultVersion,
    location: FolderLocation,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<ItemRef>, Error> {
    let mut work: Vec<u32> = match location {
        FolderLocation::Item(item) => {
            let folder = version
                .items()
                .get(item as usize)
                .ok_or(Error::InvalidCondition)?;
            if folder.kind != du_core_contracts::result::ResultItemKind::Directory {
                return Err(Error::InvalidCondition);
            }
            version.children(item).iter().map(|c| c.item_no).collect()
        }
        FolderLocation::ScopeRoots => version
            .scope_roots()
            .iter()
            .flat_map(|root| version.children(root.item_no))
            .map(|child| child.item_no)
            .collect(),
    };
    let mut items = Vec::new();
    while let Some(no) = work.pop() {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        items.push(ItemRef {
            result_version: version.version_id().clone(),
            item_no: no,
        });
        work.extend(version.children(no).iter().map(|child| child.item_no));
    }
    Ok(items)
}
