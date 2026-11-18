//! Unread ranges inside the scope: the only source of summary reason keys.
use super::Error;
use crate::ResultVersion;
use du_core_contracts::{ItemRef, result::ResultVersionRead};
use std::collections::BTreeSet;

/// Distinct reason keys, sorted, of the gaps the collector reported inside the
/// subtree of a folder. A gap above or at the folder is not listed: it makes the
/// folder unpartitionable instead. The keys are copied, never invented, so a
/// result without reported gaps gives an empty list.
pub(super) fn reasons(
    version: &ResultVersion,
    folder: &ItemRef,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<String>, Error> {
    let root = *version
        .root_indices
        .get(folder.item_no as usize)
        .ok_or(Error::InvalidSource)?;
    let mut path = Vec::new();
    let mut current = folder.item_no;
    while let Some(item) = version.items().get(current as usize) {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let Some(parent) = item.parent else { break };
        path.push(&item.name);
        current = parent;
    }
    path.reverse();
    let mut keys = BTreeSet::new();
    for gap in version.scope_gaps() {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if gap.root_index == root
            && gap.path.len() > path.len()
            && gap
                .path
                .iter()
                .zip(&path)
                .all(|(left, right)| left == *right)
        {
            keys.insert(gap.reason_key.clone());
        }
    }
    Ok(keys.into_iter().collect())
}
