//! Per-folder allocation and unique-physical totals for a whole build.
//!
//! A folder's totals come from the identity table of the files below it. An
//! identity carried by only one file of the whole build cannot meet another
//! observation of itself in any folder, so it never conflicts. Such an
//! identity is summed into the folder's counters but not stored in the folder's
//! table, and it does not count toward the capacity limit (#270). Only
//! identities seen twice or more are tabled and merged, which keeps the work and
//! the memory of a large publication proportional to the repeated identities.
//!
//! The results equal those of one table per subtree that tables the same
//! repeated identities. Over the capacity both give the lower bound of the
//! untabled identities, which does not depend on the order of observations.

use super::*;
use crate::folder_completion::ProvenFolders;
use crate::item_kind::is_structure;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{Hash, Hasher};

/// An identity compared by its bytes. `FileIdentity` is a public type and does
/// not implement `Hash`; this key does so only for the counting pass.
struct IdentityKey<'a>(&'a FileIdentity);

impl Hash for IdentityKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.volume.hash(state);
        self.0.file.hash(state);
    }
}

impl PartialEq for IdentityKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for IdentityKey<'_> {}

/// Marks every file whose usable identity also belongs to another file.
///
/// Only files that `Physical::record` would put in an identity table count.
pub(super) fn repeated_identities(
    items: &[ResultItem],
    observations: &[Option<Arc<SpaceObservation>>],
) -> Vec<bool> {
    let mut repeated = vec![false; items.len()];
    let files = observations.iter().flatten().count();
    let mut first: HashMap<IdentityKey<'_>, usize> = HashMap::with_capacity(files);
    for item in items {
        let index = item.item_no as usize;
        if is_structure(item.kind) {
            continue;
        }
        let Observed::Known(identity, _) = Observed::read(observations[index].as_deref()) else {
            continue;
        };
        match first.entry(IdentityKey(identity)) {
            Entry::Occupied(earlier) => {
                repeated[*earlier.get()] = true;
                repeated[index] = true;
            }
            Entry::Vacant(slot) => {
                slot.insert(index);
            }
        }
    }
    repeated
}

/// Computes the totals of every item from the evidence of its subtree.
pub(crate) fn apply(
    items: &[ResultItem],
    evidence: FileEvidence<'_>,
    boundary: &[u32],
    completeness: ResultCompleteness,
    proven: Option<&ProvenFolders>,
    limit: usize,
    output: &mut [SpaceAggregate],
) {
    let source = |index| ProvenFolders::source(proven, index, completeness);
    let Some(observations) = evidence.observations else {
        apply_unobserved(items, boundary, &source, limit, output);
        return;
    };
    let repeated = repeated_identities(items, observations);
    // Only a folder with observed descendants holds an accumulator.
    let mut sums: Vec<Option<Box<Physical>>> = (0..items.len()).map(|_| None).collect();
    for item in items.iter().rev() {
        let index = item.item_no as usize;
        let observed =
            (!is_structure(item.kind)).then(|| Observed::read(observations[index].as_deref()));
        let placeholder = local_unknown(evidence.placeholders, index);
        let tabled = repeated[index];
        if let Some(mut sum) = sums[index].take() {
            if let Some(observed) = &observed {
                sum.note_placeholder(observed, placeholder);
                let _ = sum.record(observed, tabled);
            }
            sum.finish(source(index), &mut output[index]);
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
            own.note_placeholder(observed, placeholder);
            let _ = own.record(observed, false);
        }
        own.finish(source(index), &mut output[index]);
        // A file outside the boundary keeps its own totals and adds nothing above it.
        let counts_above = boundary.binary_search(&item.item_no).is_err();
        if let (Some(parent), Some(observed), true) = (item.parent, &observed, counts_above) {
            let sum =
                sums[parent as usize].get_or_insert_with(|| Box::new(Physical::with_limit(limit)));
            sum.note_placeholder(observed, placeholder);
            let _ = sum.record(observed, tabled);
        }
    }
}

/// Without any evidence column, every file below a folder is missing.
fn apply_unobserved(
    items: &[ResultItem],
    boundary: &[u32],
    source: &dyn Fn(usize) -> ResultCompleteness,
    limit: usize,
    output: &mut [SpaceAggregate],
) {
    let mut missing = vec![false; items.len()];
    for item in items.iter().rev() {
        let index = item.item_no as usize;
        missing[index] |= !is_structure(item.kind);
        Physical {
            missing: missing[index],
            ..Physical::with_limit(limit)
        }
        .finish(source(index), &mut output[index]);
        if let Some(parent) = item
            .parent
            .filter(|_| boundary.binary_search(&item.item_no).is_err())
        {
            missing[parent as usize] |= missing[index];
        }
    }
}
