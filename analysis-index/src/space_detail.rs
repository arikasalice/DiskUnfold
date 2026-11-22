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
    /// The one in-scope path whose allocation a total counts (#359), set when this
    /// identity has two or more in-scope paths. It is the first path in scope root
    /// order, then path component order, so enumeration order never changes it.
    pub counted: Option<ItemRef>,
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
            let physical = detail.physical.as_ref();
            let links = self.hardlinks(reference, physical);
            let decision = decide(allocated.as_ref(), physical, &links);
            let shared = shared_space(physical);
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

    fn valid_identity(&self, index: usize, volume: &[u8], file: &[u8]) -> bool {
        self.identity_on_root_volume(index, volume, file, true)
    }

    /// An identity usable for counting one path of a shared file (#359). It matches the
    /// reading of the allocation totals: a root without a verified volume id does not
    /// reject the identity, a root with one must carry the same volume.
    fn countable_identity(&self, index: usize, volume: &[u8], file: &[u8]) -> bool {
        self.identity_on_root_volume(index, volume, file, false)
    }

    fn identity_on_root_volume(
        &self,
        index: usize,
        volume: &[u8],
        file: &[u8],
        verified: bool,
    ) -> bool {
        !volume.is_empty()
            && !file.is_empty()
            && self
                .root_indices
                .get(index)
                .and_then(|root| self.roots.get(*root as usize))
                .map_or(!verified, |root| {
                    if verified {
                        !root.volume_id.is_empty() && root.volume_id == volume
                    } else {
                        root.volume_id.is_empty() || root.volume_id == volume
                    }
                })
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
            counted: None,
        };
        let Some(identity) = observation
            .and_then(|value| value.identity.as_ref())
            .filter(|id| self.valid_identity(reference.item_no as usize, &id.volume, &id.file))
        else {
            return summary;
        };
        let Some(observations) = &self.physical else {
            return summary;
        };
        let groups = match self
            .groups
            .all
            .get_or_init(|| build_groups(self, self.shared_table_limit, false))
        {
            Ok(groups) => groups,
            Err(()) => {
                summary.lookup_unavailable_reason = Some("space.dedup_capacity");
                return summary;
            }
        };
        let Some(group) = groups.0.get(identity) else {
            return summary;
        };
        let members = &group.members;
        let mut paths = Vec::with_capacity(members.len());
        let mut counts = Vec::with_capacity(members.len());
        let mut facts_consistent = true;
        let mut facts_missing = false;
        for &index in members {
            let Some(value) = observations.get(index as u32) else {
                continue;
            };
            paths.push(ItemRef {
                result_version: self.version_id.clone(),
                item_no: self.items[index].item_no,
            });
            counts.push(value.link_count());
            if let Some(original) = observation {
                let allocation = value.allocated();
                match (&original.allocated, allocation) {
                    (Some(first), Some(other)) => facts_consistent &= first == other,
                    _ => facts_missing = true,
                }
                facts_missing |= original.sharing == SharingEvidence::Unknown
                    || value.sharing() == SharingEvidence::Unknown;
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
        if paths.len() > 1 {
            summary.counted = group.counted.map(|index| ItemRef {
                result_version: self.version_id.clone(),
                item_no: self.items[index].item_no,
            });
        }
        summary.in_scope = Some(paths);
        summary
    }

    /// Returns the deterministic counted path when an in-scope identity group is shared.
    ///
    /// The group index is lazy and bounded with the same table as `space.detail`.
    /// `None` means no multi-path group was established; lookup exhaustion is an error.
    pub fn shared_counted_item(&self, reference: &ItemRef) -> Result<Option<ItemRef>, QueryError> {
        if reference.result_version != self.version_id {
            return Err(QueryError::StaleVersion);
        }
        let item = self
            .items
            .get(reference.item_no as usize)
            .ok_or(QueryError::NotFound)?;
        if item.kind != ResultItemKind::File {
            return Ok(None);
        }
        if !self.aggregates.is_shared_file(item.item_no) {
            return Ok(None);
        }
        let Some(identity) = self
            .physical
            .as_ref()
            .and_then(|observations| observations.get(item.item_no))
            .and_then(|observation| observation.identity())
            .filter(|id| self.countable_identity(reference.item_no as usize, id.volume, id.file))
        else {
            return Ok(None);
        };
        // Identities are matched by bytes, so a shared group needs no verified root.
        let Ok(groups) = self
            .groups
            .shared
            .get_or_init(|| build_groups(self, usize::MAX, true))
        else {
            // A group that cannot be built leaves the file counted at its own path.
            return Ok(None);
        };
        let identity = identity.to_owned();
        let Some(group) = groups.0.get(&identity) else {
            return Ok(None);
        };
        if group.members.len() < 2 {
            return Ok(None);
        }
        Ok(group.counted.map(|index| ItemRef {
            result_version: self.version_id.clone(),
            item_no: self.items[index].item_no,
        }))
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
        // DLD FR11.HD1.app-host.DD1: sharing evidence is not collected yet
        // (attr.space_sharing is not implemented), so an unproven absence of sharing is
        // "not collected", not "unsupported".
        return unknown("space.sharing_not_collected");
    }
    if links.in_scope.is_some() && links.facts_consistent.is_none() {
        return unknown("space.data_insufficient");
    }
    ExclusiveDecision::Confirmed
}

#[derive(Debug, Default)]
struct IdentityGroup {
    members: Vec<usize>,
    counted: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct IdentityGroups(BTreeMap<FileIdentity, IdentityGroup>);

/// Lazily built identity tables: every file (space detail) and the shared files only.
#[derive(Debug, Default)]
pub(crate) struct GroupCaches {
    all: std::sync::OnceLock<Result<IdentityGroups, ()>>,
    shared: std::sync::OnceLock<Result<IdentityGroups, ()>>,
}

fn build_groups(
    result: &ResultVersion,
    limit: usize,
    shared_only: bool,
) -> Result<IdentityGroups, ()> {
    let mut groups = BTreeMap::<FileIdentity, IdentityGroup>::new();
    let mut entries = 0usize;
    if let Some(values) = &result.physical {
        for item in &result.items {
            if shared_only && !result.aggregates.is_shared_file(item.item_no) {
                continue;
            }
            let index = item.item_no as usize;
            let Some(identity) = values.get(item.item_no).and_then(|value| value.identity()) else {
                continue;
            };
            if item.kind != ResultItemKind::File
                || !(if shared_only {
                    result.countable_identity(index, identity.volume, identity.file)
                } else {
                    result.valid_identity(index, identity.volume, identity.file)
                })
            {
                continue;
            }
            if entries == limit {
                return Err(());
            }
            entries += 1;
            groups
                .entry(identity.to_owned())
                .or_default()
                .members
                .push(index);
        }
    }
    for group in groups.values_mut() {
        let mut counted = None;
        for &candidate in &group.members {
            if let Some(best) = counted {
                if path_order(result, candidate, best)?.is_lt() {
                    counted = Some(candidate);
                }
            } else {
                counted = Some(candidate);
            }
        }
        group.counted = counted;
    }
    Ok(IdentityGroups(groups))
}

fn path_order(result: &ResultVersion, left: usize, right: usize) -> Result<std::cmp::Ordering, ()> {
    let key = |index: usize| -> Result<(u32, Vec<&[u8]>), ()> {
        let mut parts = Vec::new();
        let mut entry = result.items.get(index).ok_or(())?;
        while let Some(parent) = entry.parent {
            parts.push(entry.name.as_slice());
            entry = result.items.get(parent as usize).ok_or(())?;
        }
        parts.reverse();
        let root = result.root_indices.get(index).copied().unwrap_or(0);
        Ok((root, parts))
    };
    Ok(key(left)?.cmp(&key(right)?))
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
