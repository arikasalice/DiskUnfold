//! What a sealed result says about each scope root, computed once when the result is built.
//!
//! The freshness ledger settles refreshing rows from these values. They are derived from
//! the items and gaps while the result is still private to its builder, outside the
//! result-store lock, and stored on the immutable result. Keep this module out of the
//! ledger's dependencies: the ledger runs under the store lock and must not reach items.
use du_core_contracts::result::{ResultCompleteness, ResultItem, ResultScopeGap};

/// What a sealed result says about one root.
///
/// The default is a root the observation did not process and that did not fail, which
/// settles to the root's earlier state, or to `unverifiable` without a reason.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RootOutcome {
    /// The reason key of a gap that covers the whole root.
    pub(crate) failed: Option<String>,
    pub(crate) processed: bool,
}

/// Reads each root's outcome from a result's gaps and items, in root order.
///
/// A complete result processed every root it has no whole-root gap for. For a partial or
/// cancelled result a root counts as processed only when an item below the root exists.
/// `root_indices` is parallel to `items`. The work is linear in the items and gaps, so call
/// it before the result is published and never while holding the result-store lock.
pub(crate) fn compute(
    root_count: usize,
    completeness: ResultCompleteness,
    items: &[ResultItem],
    root_indices: &[u32],
    gaps: &[ResultScopeGap],
) -> Box<[RootOutcome]> {
    let complete = matches!(
        completeness,
        ResultCompleteness::Complete | ResultCompleteness::Missing
    );
    let mut processed = vec![complete; root_count];
    if !complete {
        for (item, root) in items.iter().zip(root_indices) {
            if let Some(slot) = item.parent.and(processed.get_mut(*root as usize)) {
                *slot = true;
            }
        }
    }
    let mut failed: Vec<Option<String>> = vec![None; root_count];
    for gap in gaps.iter().filter(|gap| gap.path.is_empty()) {
        if let Some(slot) = failed.get_mut(gap.root_index as usize) {
            slot.get_or_insert_with(|| gap.reason_key.clone());
        }
    }
    failed
        .into_iter()
        .zip(processed)
        .map(|(failed, processed)| RootOutcome { failed, processed })
        .collect()
}
