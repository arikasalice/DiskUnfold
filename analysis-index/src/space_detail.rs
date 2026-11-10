//! Item-space evidence derived only from a single immutable result version.
use crate::{FileIdentity, QueryError, ResultVersion, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultItemKind},
};
use std::collections::BTreeMap;

/// Observed link evidence. `None` means that the corresponding fact was not established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardlinkSummary {
    /// Link count from this item's own observation, including out-of-scope links.
    pub observed_link_count: Option<u64>,
    /// Reason the bounded identity table could not be established, if applicable.
    pub lookup_unavailable_reason: Option<&'static str>,
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

/// Shared-extents observation, independent of whether links leave the scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedSpace {
    /// The source established that the file has no shared physical extents.
    ConfirmedZero,
    /// Sharing was not established by the retained observations.
    Unknown,
    /// The item kind has no file-level sharing observation.
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
    pub shared: SharedSpace,
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
        let (links, exclusive, shared) = if item.kind == ResultItemKind::File {
            let links = self.hardlinks(reference, detail.physical);
            let decision = decide(allocated.as_ref(), detail.physical, &links);
            let shared = shared_space(detail.physical);
            (Some(links), decision, shared)
        } else {
            (
                None,
                ExclusiveDecision::NotApplicable,
                SharedSpace::NotApplicable,
            )
        };
        Ok(SpaceDetail {
            item: reference.clone(),
            logical,
            allocated,
            unique_physical,
            links,
            exclusive,
            shared,
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
            lookup_unavailable_reason: None,
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
        let groups = match self
            .identity_groups
            .get_or_init(|| build_groups(self, crate::physical::IDENTITY_LIMIT))
        {
            Ok(groups) => groups,
            Err(()) => {
                summary.lookup_unavailable_reason = Some("space.dedup_capacity");
                return summary;
            }
        };
        let Some(members) = groups.0.get(identity) else {
            return summary;
        };
        let mut paths = Vec::with_capacity(members.len());
        let mut counts = Vec::with_capacity(members.len());
        let mut facts_consistent = true;
        let mut facts_missing = false;
        for &index in members {
            let Some(value) = observations[index].as_ref() else {
                continue;
            };
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
    if let Some(reason_key) = links.lookup_unavailable_reason {
        return unknown(reason_key);
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

#[derive(Debug)]
pub(crate) struct IdentityGroups(BTreeMap<FileIdentity, Vec<usize>>);

fn build_groups(result: &ResultVersion, limit: usize) -> Result<IdentityGroups, ()> {
    let mut groups = BTreeMap::<FileIdentity, Vec<usize>>::new();
    let mut entries = 0usize;
    if let Some(values) = &result.physical {
        for (index, observation) in values.iter().enumerate() {
            let Some(identity) = observation
                .as_ref()
                .and_then(|value| value.identity.as_ref())
            else {
                continue;
            };
            if result.items[index].kind != ResultItemKind::File
                || !result.valid_identity(index, identity)
            {
                continue;
            }
            if entries == limit {
                return Err(());
            }
            entries += 1;
            groups.entry(identity.clone()).or_default().push(index);
        }
    }
    Ok(IdentityGroups(groups))
}

fn shared_space(observation: Option<&SpaceObservation>) -> SharedSpace {
    if observation.is_some_and(|value| value.sharing == SharingEvidence::None) {
        SharedSpace::ConfirmedZero
    } else {
        SharedSpace::Unknown
    }
}

#[cfg(test)]
mod tests;
