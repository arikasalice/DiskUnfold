//! Per-folder allocation and unique-physical totals for a whole build.
//!
//! A folder's totals come from the identity table of the files below it. An
//! identity carried by only one file of the whole build cannot meet another
//! observation of itself in any folder, so it never conflicts. Such an
//! identity is summed into the folder's counters and counted toward the
//! capacity limit, but it is not stored in the folder's table. Only identities
//! seen twice or more are tabled and merged, which keeps the work and the
//! memory of a large publication proportional to the repeated identities.
//!
//! The results equal those of one table per subtree, with one known
//! difference: when a subtree exceeds the identity capacity and its allocation
//! sum also exceeds `u64`, the `space.aggregation_overflow` limitation can
//! differ. The old code stopped adding identities once its table was full, so
//! its partial sum depended on the order of observations; this code adds every
//! untabled identity. The allocated and unique-physical values are Unknown in
//! that case either way.

use super::*;

/// Marks every file whose usable identity also belongs to another file.
///
/// Only files that `Physical::record` would put in an identity table count.
pub(super) fn repeated_identities(
    items: &[ResultItem],
    structures: &[bool],
    observations: &[Option<Arc<SpaceObservation>>],
) -> Vec<bool> {
    let mut repeated = vec![false; items.len()];
    let mut identified = Vec::new();
    for item in items {
        let index = item.item_no as usize;
        if structures[index] {
            continue;
        }
        if matches!(
            Observed::read(observations[index].as_deref()),
            Observed::Known(..)
        ) {
            identified.push(item.item_no);
        }
    }

    identified.sort_unstable_by(|left, right| {
        identity_at(observations, *left).cmp(&identity_at(observations, *right))
    });
    let mut start = 0;
    while start < identified.len() {
        let identity = identity_at(observations, identified[start]);
        let mut end = start + 1;
        while end < identified.len() && identity_at(observations, identified[end]) == identity {
            end += 1;
        }
        if end - start > 1 {
            for item in &identified[start..end] {
                repeated[*item as usize] = true;
            }
        }
        start = end;
    }
    repeated
}

fn identity_at(observations: &[Option<Arc<SpaceObservation>>], item: u32) -> Option<&FileIdentity> {
    observations
        .get(item as usize)
        .and_then(Option::as_deref)
        .and_then(|observation| observation.identity.as_ref())
}

/// Computes the totals of every item from the evidence of its subtree.
pub(crate) fn apply(
    items: &[ResultItem],
    structures: &[bool],
    observations: Option<&[Option<Arc<SpaceObservation>>]>,
    completeness: ResultCompleteness,
    limit: usize,
    output: &mut [SpaceAggregate],
) {
    let Some(observations) = observations else {
        apply_unobserved(items, structures, completeness, limit, output);
        return;
    };
    let repeated = repeated_identities(items, structures, observations);
    // Only a folder with observed descendants holds an accumulator.
    let mut sums: Vec<Option<Box<Physical>>> = (0..items.len()).map(|_| None).collect();
    for item in items.iter().rev() {
        let index = item.item_no as usize;
        let observed = (!structures[index]).then(|| Observed::read(observations[index].as_deref()));
        let tabled = repeated[index];
        if let Some(mut sum) = sums[index].take() {
            if let Some(observed) = &observed {
                let _ = sum.record(observed, tabled);
            }
            sum.finish(completeness, &mut output[index]);
            if let Some(parent) = item.parent {
                let slot = &mut sums[parent as usize];
                match slot {
                    Some(parent) => parent.merge(*sum),
                    None => *slot = Some(sum),
                }
            }
            continue;
        }
        // An item without descendants is its own whole scope: it has no other
        // file to conflict with, so its totals need no table.
        let mut own = Physical::with_limit(limit);
        if let Some(observed) = &observed {
            let _ = own.record(observed, false);
        }
        own.finish(completeness, &mut output[index]);
        if let (Some(parent), Some(observed)) = (item.parent, &observed) {
            let _ = sums[parent as usize]
                .get_or_insert_with(|| Box::new(Physical::with_limit(limit)))
                .record(observed, tabled);
        }
    }
}

/// Without any evidence column, every file below a folder is missing.
fn apply_unobserved(
    items: &[ResultItem],
    structures: &[bool],
    completeness: ResultCompleteness,
    limit: usize,
    output: &mut [SpaceAggregate],
) {
    let mut missing = vec![false; items.len()];
    for item in items.iter().rev() {
        let index = item.item_no as usize;
        missing[index] |= !structures[index];
        Physical {
            missing: missing[index],
            ..Physical::with_limit(limit)
        }
        .finish(completeness, &mut output[index]);
        if let Some(parent) = item.parent {
            missing[parent as usize] |= missing[index];
        }
    }
}
