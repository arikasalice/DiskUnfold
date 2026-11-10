use std::collections::BTreeMap;

pub(crate) const IDENTITY_LIMIT: usize = 1_000_000;

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

struct Physical {
    limit: usize,
    identities: BTreeMap<FileIdentity, Option<Known>>,
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
    fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            identities: BTreeMap::new(),
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
    fn observe(&mut self, input: Option<&SpaceObservation>) {
        let Some(input) = input else {
            self.missing = true;
            return;
        };
        if input.link_count == Some(0) {
            self.missing = true;
            return;
        }
        let value = input
            .allocated
            .as_ref()
            .and_then(|v| {
                if v.basis != SizeBasis::Allocated || v.evidence != SizeEvidence::OsReported {
                    return None;
                }
                match &v.amount {
                    SizeAmount::Exact { bytes } => Some((bytes, false)),
                    SizeAmount::LowerBound { bytes } => Some((bytes, true)),
                    SizeAmount::Unknown => None,
                }
            })
            .and_then(|(bytes, lower)| {
                bytes.as_str().parse().ok().map(|bytes| Known {
                    bytes,
                    lower,
                    sharing: input.sharing,
                })
            });
        let Some(value) = value else {
            self.missing = true;
            return;
        };
        match &input.identity {
            Some(identity) if !identity.volume.is_empty() && !identity.file.is_empty() => {
                self.insert(identity.clone(), Some(value))
            }
            _ => {
                self.missing = true;
                self.identity_missing = true;
            }
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
        for (key, value) in other.identities {
            self.insert(key, value);
        }
    }

    fn finish(&self, source: ResultCompleteness, output: &mut SpaceAggregate) {
        let overflow = self.total > u128::from(u64::MAX);
        let sharing_unknown = self.sharing_unknown > 0;
        let lower = self.lower > 0;
        let mut missing = self.missing || self.conflicts > 0;
        let bytes = if self.known > 0 && !overflow {
            u64::try_from(self.total).ok()
        } else if self.known == 0 && !missing && source == ResultCompleteness::Complete {
            Some(0)
        } else {
            None
        };
        let capacity_exceeded = self.capacity_exceeded;
        missing |= bytes.is_none() || capacity_exceeded;
        let completeness = state(source, missing || lower || overflow);
        output.allocated_completeness = completeness;
        output.allocated = amount(
            SizeBasis::Allocated,
            bytes,
            completeness,
            overflow || capacity_exceeded,
        );
        output.unique_physical_completeness = state(completeness, sharing_unknown);
        output.unique_physical = if sharing_unknown || missing {
            amount(
                SizeBasis::UniquePhysical,
                None,
                output.unique_physical_completeness,
                false,
            )
        } else {
            amount(
                SizeBasis::UniquePhysical,
                bytes,
                output.unique_physical_completeness,
                overflow,
            )
        };
        if self.known == 0 && !missing && bytes == Some(0) {
            output.allocated.evidence = SizeEvidence::Measured;
            output.unique_physical.evidence = SizeEvidence::Measured;
        }
        self.set_limitations(
            output,
            !missing && bytes.is_some(),
            sharing_unknown,
            overflow,
        );
    }

    fn set_limitations(
        &self,
        output: &mut SpaceAggregate,
        has_data: bool,
        sharing_unknown: bool,
        overflow: bool,
    ) {
        if has_data {
            const KNOWN: &[&str] = &[
                "space.aux_not_included",
                "space.observation_detail_insufficient",
            ];
            const KNOWN_OVERFLOW: &[&str] = &[
                "space.aux_not_included",
                "space.observation_detail_insufficient",
                "space.aggregation_overflow",
            ];
            const SHARING_UNKNOWN: &[&str] = &[
                "space.sharing_not_collected",
                "space.aux_not_included",
                "space.observation_detail_insufficient",
            ];
            const SHARING_UNKNOWN_OVERFLOW: &[&str] = &[
                "space.sharing_not_collected",
                "space.aux_not_included",
                "space.observation_detail_insufficient",
                "space.aggregation_overflow",
            ];
            let logical_overflow = output.limitations.contains(&"space.aggregation_overflow");
            output.limitations =
                std::borrow::Cow::Borrowed(match (sharing_unknown, logical_overflow) {
                    (false, false) => KNOWN,
                    (false, true) => KNOWN_OVERFLOW,
                    (true, false) => SHARING_UNKNOWN,
                    (true, true) => SHARING_UNKNOWN_OVERFLOW,
                });
        }
        if self.identity_missing {
            output
                .limitations
                .to_mut()
                .push("space.hardlink_identity_unknown");
        }
        if self.capacity_exceeded {
            output.limitations.to_mut().push("space.dedup_capacity");
        }
        if overflow && !output.limitations.contains(&"space.aggregation_overflow") {
            output
                .limitations
                .to_mut()
                .push("space.aggregation_overflow");
        }
    }
}

fn state(source: ResultCompleteness, missing: bool) -> ResultCompleteness {
    match source {
        ResultCompleteness::Complete if missing => ResultCompleteness::Missing,
        other => other,
    }
}

fn amount(
    basis: SizeBasis,
    bytes: Option<u64>,
    completeness: ResultCompleteness,
    overflow: bool,
) -> SizeValue {
    let amount = match (bytes, completeness, overflow) {
        (_, _, true) | (None, _, _) => SizeAmount::Unknown,
        (Some(bytes), ResultCompleteness::Complete, false) => SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
        (Some(bytes), _, false) => SizeAmount::LowerBound {
            bytes: ByteSize::from(bytes),
        },
    };
    let evidence = if matches!(amount, SizeAmount::Unknown) {
        SizeEvidence::Unknown
    } else {
        SizeEvidence::OsReported
    };
    SizeValue {
        basis,
        amount,
        evidence,
    }
}

/// Computes the scope-wide physical total with one shared identity table.
pub(crate) fn scope_total(
    items: &[ResultItem],
    structures: &[bool],
    observations: Option<&[Option<SpaceObservation>]>,
    completeness: ResultCompleteness,
    limit: usize,
    output: &mut SpaceAggregate,
) {
    let mut sum = Physical::with_limit(limit);
    for item in items {
        let index = item.item_no as usize;
        if !structures[index] {
            sum.observe(observations.and_then(|values| values[index].as_ref()));
        }
    }
    sum.finish(completeness, output);
}

/// Reuses the same identity table for a sorted set of root observations.
pub(crate) fn selected_scope(
    result: &crate::ResultVersion,
    roots: &[u32],
    completeness: ResultCompleteness,
    output: &mut SpaceAggregate,
) {
    let mut sum = Physical::with_limit(result.shared_table_limit);
    for (index, item) in result.items.iter().enumerate() {
        if roots.binary_search(&result.root_indices[index]).is_ok()
            && !matches!(
                item.kind,
                du_core_contracts::result::ResultItemKind::Directory
                    | du_core_contracts::result::ResultItemKind::UnknownStructure
            )
        {
            let observed = result
                .physical
                .as_ref()
                .and_then(|values| values[index].as_ref());
            let root_volume = &result.roots[result.root_indices[index] as usize].volume_id;
            let consistent = observed.is_none_or(|value| {
                value.identity.as_ref().is_none_or(|identity| {
                    root_volume.is_empty() || identity.volume == *root_volume
                })
            });
            sum.observe(if consistent { observed } else { None });
        }
    }
    sum.finish(completeness, output);
}

pub(crate) fn apply(
    items: &[ResultItem],
    structures: &[bool],
    observations: Option<&[Option<SpaceObservation>]>,
    completeness: ResultCompleteness,
    limit: usize,
    output: &mut [SpaceAggregate],
) {
    let Some(observations) = observations else {
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
        return;
    };
    let mut sums: Vec<Option<Box<Physical>>> = (0..items.len()).map(|_| None).collect();
    for item in items.iter().rev() {
        let index = item.item_no as usize;
        if !structures[index] {
            sums[index]
                .get_or_insert_with(|| Box::new(Physical::with_limit(limit)))
                .observe(observations[index].as_ref());
        }
        sums[index]
            .as_deref()
            .unwrap_or(&Physical::with_limit(limit))
            .finish(completeness, &mut output[index]);
        if let Some(parent) = item.parent
            && let Some(child) = sums[index].take()
        {
            let slot = &mut sums[parent as usize];
            match slot {
                Some(parent) => parent.merge(*child),
                None => *slot = Some(child),
            }
        }
    }
}

#[cfg(test)]
mod tests;
