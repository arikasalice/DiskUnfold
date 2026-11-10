//! Conservative peer evidence for the read-free size stage.
use crate::{
    candidates::{EntryKind, PhysicalFile, Size, Target},
    verification_input::{VerificationTarget, valid_approval},
    verification_result::{DistinctStage, Outcome, State},
};
use std::collections::BTreeMap;

pub(crate) struct Evidence {
    exact_counts: BTreeMap<u64, usize>,
    minimum_uncertain: Option<u64>,
}
impl Evidence {
    pub(crate) fn new(targets: &[Target]) -> Self {
        let mut result = Self {
            exact_counts: BTreeMap::new(),
            minimum_uncertain: None,
        };
        for target in targets {
            if target.kind == EntryKind::Other {
                continue;
            }
            match target.size {
                Size::Exact(size) => *result.exact_counts.entry(size).or_default() += 1,
                Size::Unknown => result.minimum_uncertain = Some(0),
                Size::LowerBound(size) => {
                    result.minimum_uncertain = Some(
                        result
                            .minimum_uncertain
                            .map_or(size, |previous| previous.min(size)),
                    )
                }
            }
        }
        result
    }
    pub(crate) fn no_other_peer(&self, file: &PhysicalFile) -> bool {
        self.no_unrepresented_peer(file.size, file.item_nos.len())
    }
    pub(crate) fn no_unrepresented_peer(&self, size: u64, represented: usize) -> bool {
        self.minimum_uncertain.is_none_or(|minimum| minimum > size)
            && self.exact_counts.get(&size).copied() == Some(represented)
    }
}

pub(crate) fn settle_unpaired(
    files: Vec<PhysicalFile>,
    sizes: &Evidence,
    source: &BTreeMap<u32, &VerificationTarget>,
    scope_proven: bool,
    cancelled: &mut impl FnMut() -> bool,
    output: &mut Outcome,
) -> bool {
    for file in files {
        if cancelled() {
            return false;
        }
        if scope_proven && known_approval(&file, source) && sizes.no_other_peer(&file) {
            for number in file.item_nos {
                output.states.insert(number, State::Distinct);
                output.distinct_by.insert(number, DistinctStage::Size);
            }
        }
    }
    true
}

fn known_approval(file: &PhysicalFile, source: &BTreeMap<u32, &VerificationTarget>) -> bool {
    let first = file
        .item_nos
        .first()
        .and_then(|number| source.get(number))
        .and_then(|item| item.snapshot.as_ref());
    file.identity.as_ref().is_some_and(|identity| {
        !file.placeholder_unknown
            && file.item_nos.iter().all(|number| {
                source.get(number).is_some_and(|item| {
                    valid_approval(item, identity, file.size) && item.snapshot.as_ref() == first
                })
            })
    })
}
