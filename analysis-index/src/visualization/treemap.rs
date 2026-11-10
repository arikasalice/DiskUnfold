//! Admission of one treemap partition from immutable F03 observations.
//!
//! The current index computes physical totals per subtree. It does not retain a
//! selectable attribution rule; this boundary only admits cases whose disjoint
//! attribution is independently established by the retained observations.
use super::{reference, root_ref, squarify, value};
use crate::{FileIdentity, ResultVersion, SharingEvidence};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultItemKind, ResultVersionRead},
};
use std::collections::{BTreeMap, HashMap};

/// A comparison key produced by the caller using the pinned path comparison rule.
#[derive(Clone, Debug)]
pub struct ChildKey {
    pub item: ItemRef,
    pub path_compare_key: Vec<u8>,
}

/// Original F03 value and item associated with an index in the resulting layout.
#[derive(Clone, Debug)]
pub struct Child {
    pub item: ItemRef,
    pub value: SizeValue,
    /// False when this child is retained without a provable area.
    pub area_attributed: bool,
}

#[derive(Clone, Debug)]
pub struct Partition {
    pub children: Vec<Child>,
    pub layout: squarify::Layout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    InvalidParent,
    InvalidKeys,
    /// `viz.unavailable.no_attribution`: the source does not establish a partition.
    NoAttribution,
    Layout(squarify::LayoutError),
}

/// Validates attribution before sending weights to the squarify calculation.
/// Keys must cover every immediate child exactly once and must be computed using
/// the pinned path comparison rule, not display names. They determine ties only.
pub fn layout_parent(
    version: &ResultVersion,
    parent: &ItemRef,
    basis: SizeBasis,
    bounds: squarify::Rect,
    keys: &[ChildKey],
) -> Result<Partition, AdmissionError> {
    let parent_no = root_ref(version, parent).map_err(|_| AdmissionError::InvalidParent)?;
    squarify::layout(bounds, &[]).map_err(AdmissionError::Layout)?;
    let children = version.children(parent_no);
    if keys.len() != children.len() {
        return Err(AdmissionError::InvalidKeys);
    }
    let mut lookup = HashMap::new();
    for key in keys {
        if key.item.result_version != *version.version_id()
            || lookup
                .insert(key.item.item_no, &key.path_compare_key)
                .is_some()
        {
            return Err(AdmissionError::InvalidKeys);
        }
    }
    if children
        .iter()
        .any(|child| !lookup.contains_key(&child.item_no))
    {
        return Err(AdmissionError::InvalidKeys);
    }
    if gap_covers_parent(version, parent_no)? {
        return Err(AdmissionError::NoAttribution);
    }
    let mut identities = BTreeMap::new();
    let mut admitted = Vec::with_capacity(children.len());
    let mut total = 0_u64;
    for child in children {
        let key = lookup
            .get(&child.item_no)
            .ok_or(AdmissionError::InvalidKeys)?;
        let observed = inspect_subtree(version, child.item_no, &basis, &mut identities)?;
        let raw =
            value(version, child.item_no, &basis).map_err(|_| AdmissionError::NoAttribution)?;
        if exact(&raw, &basis) != Some(observed) {
            return Err(AdmissionError::NoAttribution);
        }
        total = total
            .checked_add(observed)
            .ok_or(AdmissionError::NoAttribution)?;
        admitted.push((
            Child {
                item: reference(version, child.item_no),
                value: raw,
                area_attributed: true,
            },
            observed,
            *key,
        ));
    }
    let parent_value =
        value(version, parent_no, &basis).map_err(|_| AdmissionError::NoAttribution)?;
    if exact(&parent_value, &basis) != Some(total) {
        return Err(AdmissionError::NoAttribution);
    }
    admitted.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.2.cmp(b.2))
            .then_with(|| a.0.item.item_no.cmp(&b.0.item.item_no))
    });
    let weights: Vec<_> = admitted
        .iter()
        .map(|entry| squarify::Weight::Exact(entry.1))
        .collect();
    let layout = squarify::layout(bounds, &weights).map_err(AdmissionError::Layout)?;
    Ok(Partition {
        children: admitted.into_iter().map(|entry| entry.0).collect(),
        layout,
    })
}

// A gap at an ancestor is not reflected in a descendant aggregate. In that
// case the retained source cannot establish that the selected subtree was read.
fn gap_covers_parent(version: &ResultVersion, parent_no: u32) -> Result<bool, AdmissionError> {
    let item = version
        .items()
        .get(parent_no as usize)
        .ok_or(AdmissionError::NoAttribution)?;
    let root = *version
        .root_indices
        .get(parent_no as usize)
        .ok_or(AdmissionError::NoAttribution)?;
    let chain = version
        .ancestors(item)
        .map_err(|_| AdmissionError::NoAttribution)?;
    let path: Vec<_> = chain.iter().skip(1).map(|item| &item.name).collect();
    Ok(version.scope_gaps().iter().any(|gap| {
        gap.root_index == root
            && gap.path.len() <= path.len()
            && gap
                .path
                .iter()
                .zip(&path)
                .all(|(gap_part, part)| gap_part == *part)
    }))
}

/// Lays out a previously admitted, non-root branch, or retains all of its
/// children without geometry when this branch cannot be partitioned.
///
/// The caller must use `layout_parent` for the navigation root and call this
/// only for a branch whose enclosing rectangle was already admitted. Suppressed
/// children keep their original F03 values; no remainder is redistributed.
pub fn layout_nested_parent(
    version: &ResultVersion,
    parent: &ItemRef,
    basis: SizeBasis,
    bounds: squarify::Rect,
    keys: &[ChildKey],
) -> Result<Partition, AdmissionError> {
    match layout_parent(version, parent, basis.clone(), bounds, keys) {
        Ok(partition) => Ok(partition),
        Err(AdmissionError::NoAttribution) => {
            let parent_no = root_ref(version, parent).map_err(|_| AdmissionError::InvalidParent)?;
            let children = version
                .children(parent_no)
                .iter()
                .map(|child| {
                    Ok(Child {
                        item: reference(version, child.item_no),
                        value: value(version, child.item_no, &basis)
                            .map_err(|_| AdmissionError::NoAttribution)?,
                        area_attributed: false,
                    })
                })
                .collect::<Result<Vec<_>, AdmissionError>>()?;
            Ok(Partition {
                children,
                layout: squarify::Layout {
                    shapes: Vec::new(),
                    skipped: Vec::new(),
                },
            })
        }
        Err(error) => Err(error),
    }
}

// Identity, sibling owner, allocation, link count, observed paths.
type Identities = BTreeMap<FileIdentity, (u32, u64, u64, u64)>;
fn inspect_subtree(
    version: &ResultVersion,
    sibling: u32,
    basis: &SizeBasis,
    identities: &mut Identities,
) -> Result<u64, AdmissionError> {
    let mut work = vec![sibling];
    let mut sum = 0_u64;
    while let Some(no) = work.pop() {
        let item = version
            .items()
            .get(no as usize)
            .ok_or(AdmissionError::NoAttribution)?;
        match item.kind {
            ResultItemKind::Directory => {
                work.extend(version.children(no).iter().map(|child| child.item_no))
            }
            ResultItemKind::File => {
                let bytes = if *basis == SizeBasis::Logical {
                    item.size
                        .as_ref()
                        .and_then(|v| exact(v, basis))
                        .ok_or(AdmissionError::NoAttribution)?
                } else {
                    physical_file(version, no, sibling, identities)?
                };
                sum = sum
                    .checked_add(bytes)
                    .ok_or(AdmissionError::NoAttribution)?;
            }
            _ => return Err(AdmissionError::NoAttribution),
        }
    }
    Ok(sum)
}

fn physical_file(
    version: &ResultVersion,
    no: u32,
    sibling: u32,
    identities: &mut Identities,
) -> Result<u64, AdmissionError> {
    let observation = version
        .physical
        .as_ref()
        .and_then(|v| v.get(no as usize))
        .and_then(Option::as_ref)
        .ok_or(AdmissionError::NoAttribution)?;
    let identity = observation
        .identity
        .as_ref()
        .filter(|id| !id.volume.is_empty() && !id.file.is_empty())
        .ok_or(AdmissionError::NoAttribution)?;
    let count = observation
        .link_count
        .filter(|n| *n > 0)
        .ok_or(AdmissionError::NoAttribution)?;
    if observation.sharing != SharingEvidence::None {
        return Err(AdmissionError::NoAttribution);
    }
    let size = observation
        .allocated
        .as_ref()
        .filter(|v| v.evidence == SizeEvidence::OsReported)
        .and_then(|v| exact(v, &SizeBasis::Allocated))
        .ok_or(AdmissionError::NoAttribution)?;
    if let Some((owner, previous, links, seen)) = identities.get_mut(identity) {
        if *owner != sibling || *previous != size || *links != count || *seen >= count {
            return Err(AdmissionError::NoAttribution);
        }
        *seen += 1;
        Ok(0)
    } else {
        identities.insert(identity.clone(), (sibling, size, count, 1));
        Ok(size)
    }
}

fn exact(value: &SizeValue, basis: &SizeBasis) -> Option<u64> {
    if value.basis != *basis || value.evidence == SizeEvidence::Unknown {
        return None;
    }
    match &value.amount {
        SizeAmount::Exact { bytes } => bytes.as_str().parse().ok(),
        _ => None,
    }
}
