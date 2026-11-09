use std::collections::BTreeMap;

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

#[derive(Default)]
struct Physical {
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

impl Physical {
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
        self.insert_with_limit(identity, value, 1_000_000);
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

pub(crate) fn apply(
    items: &[ResultItem],
    structures: &[bool],
    observations: Option<&[Option<SpaceObservation>]>,
    completeness: ResultCompleteness,
    output: &mut [SpaceAggregate],
) {
    let Some(observations) = observations else {
        let mut missing = vec![false; items.len()];
        for item in items.iter().rev() {
            let index = item.item_no as usize;
            missing[index] |= !structures[index];
            Physical {
                missing: missing[index],
                ..Physical::default()
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
                .get_or_insert_with(Box::default)
                .observe(observations[index].as_ref());
        }
        sums[index]
            .as_deref()
            .unwrap_or(&Physical::default())
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
mod tests {
    use super::*;

    #[test]
    fn identity_capacity_boundary_and_conflict_update_summary() {
        let mut sum = Physical::default();
        let identity = |n| FileIdentity {
            volume: vec![1],
            file: vec![n],
        };
        let known = |bytes| {
            Some(Known {
                bytes,
                lower: false,
                sharing: SharingEvidence::None,
            })
        };
        sum.insert_with_limit(identity(1), known(10), 2);
        sum.insert_with_limit(identity(2), known(20), 2);
        assert_eq!(
            (sum.total, sum.known, sum.capacity_exceeded),
            (30, 2, false)
        );
        let item = ResultItem {
            item_no: 0,
            parent: None,
            name: vec![],
            size: None,
            kind: du_core_contracts::result::ResultItemKind::Directory,
        };
        let mut aggregates =
            crate::space::aggregate(&[item], &[true], ResultCompleteness::Complete);
        sum.finish(ResultCompleteness::Complete, &mut aggregates[0]);
        assert_eq!(
            aggregates[0].allocated.amount,
            SizeAmount::Exact {
                bytes: ByteSize::from(30)
            }
        );
        sum.insert_with_limit(identity(1), known(11), 2);
        assert_eq!((sum.total, sum.known, sum.conflicts), (20, 1, 1));
        sum.insert_with_limit(identity(3), known(5), 2);
        assert!(sum.capacity_exceeded);
        assert_eq!(sum.identities.len(), 2);
        sum.finish(ResultCompleteness::Complete, &mut aggregates[0]);
        assert_eq!(aggregates[0].allocated.amount, SizeAmount::Unknown);
        assert!(aggregates[0].limitations.contains(&"space.dedup_capacity"));
    }

    #[test]
    fn merge_inserts_only_the_smaller_identity_map() {
        let mut small = Physical::default();
        let mut large = Physical::default();
        for n in 0u8..128 {
            large.insert(
                FileIdentity {
                    volume: vec![1],
                    file: vec![n],
                },
                Some(Known {
                    bytes: 1,
                    lower: false,
                    sharing: SharingEvidence::None,
                }),
            );
        }
        small.insert(
            FileIdentity {
                volume: vec![2],
                file: vec![0],
            },
            Some(Known {
                bytes: 2,
                lower: false,
                sharing: SharingEvidence::None,
            }),
        );
        small.merge(large);
        assert_eq!(small.identities.len(), 129);
        assert_eq!(small.insertions, 129);
        assert_eq!(small.total, 130);
    }
}
