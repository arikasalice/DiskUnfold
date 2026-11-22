use crate::ports::{
    AnalysisItemFacts, PlanItem, PlanItemState, PlanSelection, RestoreRestriction, TargetIdentity,
};
use du_core_contracts::PathEncoding;
use std::collections::HashMap;

type PathIndex = HashMap<(Vec<u8>, Vec<Vec<u8>>), Vec<usize>>;

struct Candidate {
    selection: PlanSelection,
    facts: AnalysisItemFacts,
    original_order: usize,
    path_order: Vec<u8>,
}

/// Sorts raw paths, absorbs duplicate targets, and resolves selected ancestry.
pub(crate) fn normalize(
    candidates: Vec<(PlanSelection, AnalysisItemFacts)>,
) -> Result<Vec<PlanItem>, &'static str> {
    let mut candidates = candidates
        .into_iter()
        .enumerate()
        .map(|(original_order, (selection, facts))| {
            facts
                .path
                .raw
                .bytes()
                .map(|path_order| Candidate {
                    selection,
                    facts,
                    original_order,
                    path_order,
                })
                .map_err(|_| "plan.item.invalid_path")
        })
        .collect::<Result<Vec<_>, _>>()?;
    candidates.sort_by(|left, right| {
        left.path_order
            .cmp(&right.path_order)
            .then(left.original_order.cmp(&right.original_order))
    });

    let mut items = candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| PlanItem {
            item_no: u32::try_from(index + 1).unwrap_or(u32::MAX),
            item: candidate.selection.item,
            path: candidate.facts.path,
            path_components: candidate.facts.path_components,
            volume_id: candidate.facts.volume_id,
            expected_identity: candidate.facts.identity,
            expected_kind: candidate.facts.kind,
            expected_size: candidate.facts.size,
            expected_modified_at: candidate.facts.modified_at,
            classification_id: candidate.facts.classification_id,
            observed_descendants: Some(candidate.facts.descendants),
            action: candidate.selection.action,
            destination: candidate.selection.destination,
            overwrite: candidate.selection.overwrite,
            displaced_size: candidate.selection.displaced_size,
            condition: candidate.selection.condition,
            selection_evidence: candidate.selection.selection_evidence,
            state: PlanItemState::Included,
            reason_key: None,
            absorbed_by: None,
            protection: None,
            restore: RestoreRestriction::Unconfirmed,
        })
        .collect::<Vec<_>>();

    absorb_duplicates(&mut items);
    absorb_nested_targets(&mut items);
    Ok(items)
}

fn absorb_duplicates(items: &mut [PlanItem]) {
    let mut keys = HashMap::<DuplicateKey, usize>::new();
    for index in 0..items.len() {
        let path_key = path_key(&items[index]);
        let identity_key = items[index]
            .expected_identity
            .as_ref()
            .map(|identity| DuplicateKey::Identity(identity.clone()));
        let keeper = identity_key
            .as_ref()
            .and_then(|key| keys.get(key).copied())
            .or_else(|| keys.get(&path_key).copied());
        if let Some(keeper) = keeper {
            let evidence = items[index].selection_evidence.clone();
            items[keeper].selection_evidence.extend(evidence);
            let keeper_no = items[keeper].item_no;
            reject_as_absorbed(&mut items[index], "plan.item.duplicate_target", keeper_no);
            keys.entry(path_key).or_insert(keeper);
            if let Some(key) = identity_key {
                keys.entry(key).or_insert(keeper);
            }
        } else {
            keys.insert(path_key, index);
            if let Some(key) = identity_key {
                keys.insert(key, index);
            }
        }
    }
    for item in items {
        item.selection_evidence
            .sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    }
}

fn absorb_nested_targets(items: &mut [PlanItem]) {
    let mut paths = PathIndex::new();
    for index in 0..items.len() {
        if items[index].state == PlanItemState::Rejected {
            continue;
        }
        let ancestors = ancestors(&items[index], &paths);
        let same_condition = ancestors
            .iter()
            .filter(|ancestor| items[ancestor.index].condition == items[index].condition)
            .max_by_key(|ancestor| ancestor.depth)
            .map(|ancestor| ancestor.index);
        if let Some(ancestor) = same_condition {
            let ancestor_no = items[ancestor].item_no;
            reject_as_absorbed(&mut items[index], "plan.item.nested_target", ancestor_no);
            continue;
        }
        for ancestor in ancestors {
            set_held(&mut items[ancestor.index], "plan.item.overlap_conflict");
        }
        paths
            .entry((
                items[index].volume_id.clone(),
                items[index].path_components.clone(),
            ))
            .or_default()
            .push(index);
    }
}

struct Ancestor {
    index: usize,
    depth: usize,
}

fn ancestors(item: &PlanItem, paths: &PathIndex) -> Vec<Ancestor> {
    (0..item.path_components.len())
        .filter_map(|depth| {
            let key = (
                item.volume_id.clone(),
                item.path_components[..depth].to_vec(),
            );
            paths.get(&key).map(|items| {
                items
                    .iter()
                    .map(|index| Ancestor {
                        index: *index,
                        depth,
                    })
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect()
}

fn path_key(item: &PlanItem) -> DuplicateKey {
    let encoding = match item.path.encoding {
        PathEncoding::Unix => 0,
        PathEncoding::WindowsUtf16le => 1,
    };
    let raw = item.path.raw.bytes().unwrap_or_default();
    DuplicateKey::Path(encoding, raw)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum DuplicateKey {
    Identity(TargetIdentity),
    Path(u8, Vec<u8>),
}

fn set_held(item: &mut PlanItem, reason: &'static str) {
    if item.state == PlanItemState::Included {
        item.state = PlanItemState::Held;
        item.reason_key = Some(reason.to_owned());
    }
}

fn reject_as_absorbed(item: &mut PlanItem, reason: &'static str, absorber_no: u32) {
    item.state = PlanItemState::Rejected;
    item.reason_key = Some(reason.to_owned());
    item.absorbed_by = Some(absorber_no);
}
