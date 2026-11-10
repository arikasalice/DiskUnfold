//! Item-space evidence derived only from a single immutable result version.
use crate::{FileIdentity, QueryError, ResultVersion, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultItemKind},
};

/// Observed link evidence. `None` means that the corresponding fact was not established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardlinkSummary {
    /// Link count from this item's own observation, including out-of-scope links.
    pub observed_link_count: Option<u64>,
    /// Known item references in the same result and on the same verified volume.
    pub in_scope: Option<Vec<ItemRef>>,
    /// Whether all observed counts in the identity group agree and are nonzero.
    pub counts_consistent: Option<bool>,
    /// Whether grouped allocation and sharing observations agree.
    pub facts_consistent: Option<bool>,
    /// True or false only when the observed facts establish that conclusion.
    pub outside_scope: Option<bool>,
}

/// First applicable item-level exclusivity decision, not a reclaimable-space claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExclusiveDecision {
    Confirmed,
    Unknown { reason_key: &'static str },
    NotApplicable,
}

/// Retained values and identity facts for a single pinned item.
/// Item observation time, capability, placeholder and auxiliary facts are not
/// present in this result model and are not inferred by this query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceDetail {
    pub item: ItemRef,
    pub logical: Option<SizeValue>,
    pub allocated: Option<SizeValue>,
    pub unique_physical: Option<SizeValue>,
    pub links: Option<HardlinkSummary>,
    pub exclusive: ExclusiveDecision,
}

impl ResultVersion {
    /// Reads recorded item facts, without consulting or reopening the filesystem.
    pub fn space_detail(&self, reference: &ItemRef) -> Result<SpaceDetail, QueryError> {
        let detail = self.detail(reference)?;
        let item = detail.item;
        let logical = self.item_size(item.item_no, SizeBasis::Logical).cloned();
        let allocated = self.item_size(item.item_no, SizeBasis::Allocated).cloned();
        let unique_physical = self
            .item_size(item.item_no, SizeBasis::UniquePhysical)
            .cloned();
        let (links, exclusive) = if item.kind == ResultItemKind::File {
            let links = self.hardlinks(reference, detail.physical);
            let decision = decide(allocated.as_ref(), detail.physical, &links);
            (Some(links), decision)
        } else {
            (None, ExclusiveDecision::NotApplicable)
        };
        Ok(SpaceDetail {
            item: reference.clone(),
            logical,
            allocated,
            unique_physical,
            links,
            exclusive,
        })
    }

    fn valid_identity(&self, index: usize, identity: &FileIdentity) -> bool {
        !identity.volume.is_empty()
            && !identity.file.is_empty()
            && self
                .root_indices
                .get(index)
                .and_then(|root| self.roots.get(*root as usize))
                .is_some_and(|root| !root.volume_id.is_empty() && root.volume_id == identity.volume)
    }

    fn hardlinks(
        &self,
        reference: &ItemRef,
        observation: Option<&SpaceObservation>,
    ) -> HardlinkSummary {
        let observed_link_count = observation.and_then(|value| value.link_count);
        let mut summary = HardlinkSummary {
            observed_link_count,
            in_scope: None,
            counts_consistent: None,
            facts_consistent: None,
            outside_scope: None,
        };
        let Some(identity) = observation
            .and_then(|value| value.identity.as_ref())
            .filter(|id| self.valid_identity(reference.item_no as usize, id))
        else {
            return summary;
        };
        let Some(observations) = &self.physical else {
            return summary;
        };
        let mut paths = Vec::new();
        let mut counts = Vec::new();
        let mut facts_consistent = true;
        let mut facts_missing = false;
        for (index, value) in observations.iter().enumerate() {
            let Some(value) = value else {
                continue;
            };
            if self.items[index].kind == ResultItemKind::File
                && value.identity.as_ref() == Some(identity)
                && self.valid_identity(index, identity)
            {
                paths.push(ItemRef {
                    result_version: self.version_id.clone(),
                    item_no: self.items[index].item_no,
                });
                counts.push(value.link_count);
                if let Some(original) = observation {
                    match (&original.allocated, &value.allocated) {
                        (Some(first), Some(other)) => facts_consistent &= first == other,
                        _ => facts_missing = true,
                    }
                    facts_missing |= original.sharing == SharingEvidence::Unknown
                        || value.sharing == SharingEvidence::Unknown;
                } else {
                    facts_missing = true;
                }
            }
        }
        let consistent = observed_link_count.and_then(|count| {
            if count == 0
                || (paths.len() as u64) > count
                || counts.iter().flatten().any(|other| *other != count)
            {
                Some(false)
            } else if counts.iter().any(Option::is_none) {
                None
            } else {
                Some(true)
            }
        });
        summary.counts_consistent = consistent;
        summary.facts_consistent = if !facts_consistent {
            Some(false)
        } else if facts_missing {
            None
        } else {
            Some(true)
        };
        if consistent == Some(true)
            && let Some(count) = observed_link_count
        {
            if paths.len() as u64 == count {
                summary.outside_scope = Some(false);
            } else if self.completeness == ResultCompleteness::Complete {
                summary.outside_scope = Some(true);
            }
        }
        summary.in_scope = Some(paths);
        summary
    }
}

fn decide(
    allocated: Option<&SizeValue>,
    observation: Option<&SpaceObservation>,
    links: &HardlinkSummary,
) -> ExclusiveDecision {
    let unknown = |reason_key| ExclusiveDecision::Unknown { reason_key };
    if !allocated.is_some_and(|v| {
        v.basis == SizeBasis::Allocated
            && v.evidence == SizeEvidence::OsReported
            && matches!(v.amount, SizeAmount::Exact { .. })
    }) {
        return unknown("space.data_insufficient");
    }
    if observation.is_some_and(|value| value.identity.is_some()) && links.in_scope.is_none() {
        return unknown("space.identity_unsupported");
    }
    if links.in_scope.is_none() && links.observed_link_count != Some(1) {
        return unknown("space.identity_unsupported");
    }
    if links.counts_consistent == Some(false) || links.facts_consistent == Some(false) {
        return unknown("space.data_insufficient");
    }
    if links.outside_scope == Some(true) {
        return unknown("space.out_of_range_links");
    }
    if links.in_scope.is_some() && links.outside_scope.is_none() {
        return unknown("space.data_insufficient");
    }
    if !observation.is_some_and(|value| value.sharing == SharingEvidence::None) {
        return unknown("space.sharing_unknown");
    }
    if links.in_scope.is_some() && links.facts_consistent.is_none() {
        return unknown("space.data_insufficient");
    }
    ExclusiveDecision::Confirmed
}
