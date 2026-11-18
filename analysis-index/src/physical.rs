use crate::item_kind::is_structure;
use std::collections::BTreeMap;
use std::sync::Arc;

mod finish;
mod observed;
mod subtree;

use finish::amount;
use observed::Observed;
pub(crate) use subtree::apply;

pub(crate) const IDENTITY_LIMIT: usize =
    du_core_contracts::settings::SharedTableLimit::DEFAULT_ENTRIES;

use du_core_contracts::result::{ResultCompleteness, ResultItem, SpaceAggregate};
use du_core_contracts::{ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

/// An OS-observed volume and file identity, scoped to the current observation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileIdentity {
    pub volume: Vec<u8>,
    pub file: Vec<u8>,
}

/// Evidence about sharing with other file identities, not hard-link count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharingEvidence {
    /// The source establishes that this file has no shared physical extents.
    None,
    /// Shared extents may exist or were not observed.
    Unknown,
}

/// OS-supplied allocation and identity facts for a single file path.
///
/// `link_count` is the observed total link count, including links outside the scope.
/// Missing facts are not inferred from path names, logical sizes, or content hashes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceObservation {
    pub allocated: Option<SizeValue>,
    pub identity: Option<FileIdentity>,
    pub link_count: Option<u64>,
    pub sharing: SharingEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Known {
    bytes: u64,
    lower: bool,
    sharing: SharingEvidence,
}

/// Facts of identities that no other file in the build shares.
///
/// Such an identity cannot conflict anywhere, so its facts are only summed. The
/// identity itself is not kept, which keeps the table small.
#[derive(Default)]
struct Untabled {
    count: usize,
    total: u128,
    lower: usize,
    sharing_unknown: usize,
}

impl Untabled {
    fn add(&mut self, value: &Known) {
        self.count += 1;
        self.total += u128::from(value.bytes);
        self.lower += usize::from(value.lower);
        self.sharing_unknown += usize::from(value.sharing == SharingEvidence::Unknown);
    }
}

pub(crate) struct Physical {
    limit: usize,
    identities: BTreeMap<FileIdentity, Option<Known>>,
    untabled: Untabled,
    total: u128,
    known: usize,
    conflicts: usize,
    lower: usize,
    sharing_unknown: usize,
    missing: bool,
    identity_missing: bool,
    capacity_exceeded: bool,
    #[cfg(test)]
    insertions: usize,
}

impl Default for Physical {
    fn default() -> Self {
        Self::with_limit(IDENTITY_LIMIT)
    }
}

impl Physical {
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            identities: BTreeMap::new(),
            untabled: Untabled::default(),
            total: 0,
            known: 0,
            conflicts: 0,
            lower: 0,
            sharing_unknown: 0,
            missing: false,
            identity_missing: false,
            capacity_exceeded: false,
            #[cfg(test)]
            insertions: 0,
        }
    }
    fn observe(&mut self, input: Option<&SpaceObservation>) -> Option<u64> {
        self.record(&Observed::read(input), true)
    }

    /// Adds one file's reading. A `tabled` identity goes into the identity table;
    /// otherwise the caller guarantees that no other file of this table's scope
    /// shares it, so only its facts are summed.
    /// Returns the exact per-path allocation before identity deduplication.
    fn record(&mut self, observed: &Observed<'_>, tabled: bool) -> Option<u64> {
        match observed {
            Observed::Missing => {
                self.missing = true;
                None
            }
            Observed::IdentityMissing => {
                self.missing = true;
                self.identity_missing = true;
                None
            }
            Observed::Known(identity, value) => {
                if tabled {
                    self.insert((*identity).clone(), Some(value.clone()));
                } else {
                    self.add_known(value);
                    self.untabled.add(value);
                }
                (!value.lower).then_some(value.bytes)
            }
        }
    }

    /// Reads a file observation only when its identity agrees with its root evidence.
    /// Returns its exact per-path allocation before identity deduplication.
    pub(crate) fn observe_pinned(
        &mut self,
        result: &crate::ResultVersion,
        index: usize,
    ) -> Option<u64> {
        let observed = result
            .physical
            .as_ref()
            .and_then(|values| values[index].as_deref());
        let root = result.roots.get(result.root_indices[index] as usize);
        let consistent = observed.is_none_or(|value| {
            value.identity.as_ref().is_none_or(|identity| {
                root.is_none_or(|root| {
                    root.volume_id.is_empty() || identity.volume == root.volume_id
                })
            })
        });
        self.observe(if consistent { observed } else { None })
    }

    /// Finalizes a set total without treating missing attributes as partial matches.
    pub(crate) fn finish_selection(&self, output: &mut SpaceAggregate) {
        self.finish(ResultCompleteness::Complete, output);
        if self.missing || self.conflicts > 0 {
            output.allocated = amount(
                SizeBasis::Allocated,
                None,
                output.allocated_completeness,
                false,
            );
        }
    }

    fn insert(&mut self, identity: FileIdentity, value: Option<Known>) {
        self.insert_with_limit(identity, value, self.limit);
    }

    fn insert_with_limit(&mut self, identity: FileIdentity, value: Option<Known>, limit: usize) {
        #[cfg(test)]
        {
            self.insertions += 1;
        }
        if self.identities.len() >= limit && !self.identities.contains_key(&identity) {
            self.capacity_exceeded = true;
            return;
        }
        if let Some(previous) = self.identities.get(&identity) {
            if previous == &value || previous.is_none() {
                return;
            }
            // Sharing uncertainty does not invalidate an otherwise identical
            // allocation observation. Retain the allocation once, while
            // propagating uncertainty to the unique-physical calculation.
            if let (Some(previous), Some(next)) = (previous, &value)
                && previous.bytes == next.bytes
                && previous.lower == next.lower
            {
                if previous.sharing == SharingEvidence::None {
                    self.sharing_unknown += 1;
                    let combined = Known {
                        sharing: SharingEvidence::Unknown,
                        ..previous.clone()
                    };
                    self.identities.insert(identity, Some(combined));
                }
                return;
            }
            if let Some(previous) = previous.clone() {
                self.remove_known(&previous);
            }
            self.conflicts += 1;
            self.identities.insert(identity, None);
        } else {
            if let Some(ref known) = value {
                self.add_known(known);
            } else {
                self.conflicts += 1;
            }
            self.identities.insert(identity, value);
        }
    }

    fn add_known(&mut self, value: &Known) {
        self.total += u128::from(value.bytes);
        self.known += 1;
        self.lower += usize::from(value.lower);
        self.sharing_unknown += usize::from(value.sharing == SharingEvidence::Unknown);
    }

    fn remove_known(&mut self, value: &Known) {
        self.total -= u128::from(value.bytes);
        self.known -= 1;
        self.lower -= usize::from(value.lower);
        self.sharing_unknown -= usize::from(value.sharing == SharingEvidence::Unknown);
    }

    fn merge(&mut self, mut other: Self) {
        if self.identities.len() < other.identities.len() {
            std::mem::swap(self, &mut other);
        }
        self.missing |= other.missing;
        self.identity_missing |= other.identity_missing;
        self.capacity_exceeded |= other.capacity_exceeded;
        // Untabled identities occur in exactly one of the merged scopes, so their
        // facts add up without any comparison.
        self.untabled.count += other.untabled.count;
        self.untabled.total += other.untabled.total;
        self.untabled.lower += other.untabled.lower;
        self.untabled.sharing_unknown += other.untabled.sharing_unknown;
        self.total += other.untabled.total;
        self.known += other.untabled.count;
        self.lower += other.untabled.lower;
        self.sharing_unknown += other.untabled.sharing_unknown;
        for (key, value) in other.identities {
            self.insert(key, value);
        }
    }

    /// Whether the scope has more distinct identities than the table limit.
    ///
    /// The table itself never holds more than `limit` entries. Untabled
    /// identities are distinct from every tabled one, so they count in full.
    fn capacity_exceeded(&self) -> bool {
        self.capacity_exceeded
            || self.untabled.count.saturating_add(self.identities.len()) > self.limit
    }
}

/// Computes the scope-wide physical total with one shared identity table.
pub(crate) fn scope_total(
    items: &[ResultItem],
    observations: Option<&[Option<Arc<SpaceObservation>>]>,
    completeness: ResultCompleteness,
    limit: usize,
    output: &mut SpaceAggregate,
) {
    let mut sum = Physical::with_limit(limit);
    for item in items {
        let index = item.item_no as usize;
        if !is_structure(item.kind) {
            let _ = sum.observe(observations.and_then(|values| values[index].as_deref()));
        }
    }
    sum.finish(completeness, output);
}

/// Reuses the same identity table for a sorted set of root observations and
/// returns the exact difference from counting each path separately, if known.
pub(crate) fn selected_scope(
    result: &crate::ResultVersion,
    roots: &[u32],
    completeness: ResultCompleteness,
    output: &mut SpaceAggregate,
) -> Option<u64> {
    let mut sum = Physical::with_limit(result.shared_table_limit);
    let mut per_path = 0u128;
    for (index, item) in result.items.iter().enumerate() {
        if roots.binary_search(&result.root_indices[index]).is_ok()
            && !is_structure(item.kind)
            && let Some(bytes) = sum.observe_pinned(result, index)
        {
            per_path += u128::from(bytes);
        }
    }
    sum.finish(completeness, output);
    let SizeAmount::Exact { bytes } = &output.allocated.amount else {
        return None;
    };
    u64::try_from(per_path).ok()?.checked_sub(bytes.get())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod equivalence_tests;
