//! Read the immutable item hierarchy and aggregates of a published version.
use crate::{ResultVersion, SpaceObservation};
use du_core_contracts::{
    Id, ItemRef, SizeBasis, SizeValue, Timestamp,
    result::{
        ResultCompleteness, ResultItem, ResultItemKind, ResultScopeGap, ResultScopeRoot,
        ResultVersionRead, SpaceAggregate,
    },
};

/// Failure to resolve an item or a current version without changing the pinned result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    NotFound,
    StaleVersion,
    InvalidPath,
    PathUnavailable,
    NotDirectory,
    KindUnknown,
    Unavailable,
}

/// Original observations for one item in a pinned result, without inferring missing values.
pub struct ItemDetail<'a> {
    pub item: &'a ResultItem,
    pub physical: Option<&'a SpaceObservation>,
}

impl ResultVersion {
    /// Returns the source that created this immutable result version.
    pub fn origin(&self) -> &'static str {
        self.origin.as_str()
    }

    /// Returns the exact result version used as the base for an incremental result.
    pub fn base_version_id(&self) -> Option<&Id> {
        self.base_version_id.as_ref()
    }

    /// Returns the identity of the aggregation implementation used for this pinned result.
    pub fn size_rule(&self) -> crate::SizeRuleIdentity {
        self.size_rule.identity()
    }
    /// Reads historical metadata flags, never a current locality guarantee.
    pub fn item_placeholder_observation(
        &self,
        item: &ItemRef,
    ) -> Result<Option<du_core_contracts::scan_observation::PlaceholderObservation>, QueryError>
    {
        let item_no = self.detail(item)?.item.item_no;
        Ok(self
            .placeholder_observations
            .as_ref()
            .and_then(|values| values.get(item_no as usize))
            .copied()
            .flatten())
    }
    /// Returns retained enumeration identity for an item in this exact snapshot.
    /// Absence means the collector could not establish the required evidence.
    pub fn item_enumerated_inode(
        &self,
        item: &ItemRef,
    ) -> Result<Option<std::num::NonZeroU64>, QueryError> {
        let item_no = self.detail(item)?.item.item_no;
        Ok(self
            .enumerated_inodes
            .as_ref()
            .and_then(|values| values.get(item_no as usize))
            .copied()
            .flatten())
    }
    /// Calculates the F03 set total for file references in this pinned version.
    ///
    /// The caller must establish whether membership is complete. Incomplete
    /// membership or a partial source never becomes an exact total. Duplicate
    /// references and shared file identities are handled by the F03 rule.
    ///
    /// # Errors
    /// Returns an item error for a missing or differently pinned reference, or
    /// cancellation without exposing a partial aggregate.
    pub fn selection_total(
        &self,
        items: &[ItemRef],
        membership_complete: bool,
        cancelled: impl FnMut() -> bool,
    ) -> Result<SpaceAggregate, crate::query::SpaceTotalError> {
        self.size_rule
            .selection_total(self, items, membership_complete, cancelled)
    }

    /// Reads reported gaps for the item's exact observed path in this immutable version.
    /// An empty iterator establishes only the absence of a report at that path.
    pub fn item_scope_gaps(
        &self,
        item: &ItemRef,
    ) -> Result<impl Iterator<Item = &ResultScopeGap>, QueryError> {
        let item_no = self.detail(item)?.item.item_no;
        Ok(self
            .exact_gaps
            .get(&item_no)
            .into_iter()
            .flatten()
            .map(|&index| &self.gaps[index]))
    }

    /// Volume reports frozen in this result version.
    pub fn volume_reports(&self) -> &[crate::VolumeReport] {
        &self.volume_reports
    }

    /// Reads the actual item observation time retained in this immutable version.
    pub fn item_observed_at(&self, item_no: u32) -> Option<&Timestamp> {
        self.item_observed_times
            .as_ref()?
            .get(item_no as usize)?
            .as_ref()
    }

    /// Reads the modification time retained for this immutable item, if available.
    pub fn item_modified_at(&self, item_no: u32) -> Option<&Timestamp> {
        self.modified_times
            .as_ref()?
            .get(item_no as usize)?
            .as_ref()
    }
    /// Returns native modification evidence retained for this exact result item.
    pub fn item_native_modified_at(
        &self,
        item_no: u32,
    ) -> Option<du_core_contracts::scan_observation::NativeModifiedTime> {
        self.native_modified_times
            .as_ref()?
            .get(item_no as usize)?
            .as_ref()
            .copied()
    }
    /// Returns the bound analysis lineage, or None for a legacy unbound build.
    pub fn lineage_id(&self) -> Option<&Id> {
        self.lineage_id.as_ref()
    }

    /// Returns cached observation bounds for an item subtree in this pinned version.
    pub fn observation_range(&self, item_no: u32) -> Option<crate::ObservationRange<'_>> {
        self.observation_ranges
            .get(item_no as usize)
            .map(|range| range.resolve(self.item_observed_times.as_deref()))
    }

    /// Returns cached observation bounds across all roots in this pinned version.
    pub fn scope_observation_range(&self) -> crate::ObservationRange<'_> {
        self.scope_observation_range
            .resolve(self.item_observed_times.as_deref())
    }

    /// The scope-wide aggregate, deduplicated across all observed roots.
    pub fn scope_total(&self) -> &SpaceAggregate {
        self.scope_total.as_ref().unwrap_or(&self.aggregates[0])
    }

    /// Actual scope-root items, in the producer's root order.
    pub fn scope_roots(&self) -> &[ResultItem] {
        let count = self.items.partition_point(|item| item.parent.is_none());
        &self.items[..count]
    }

    /// Returns the producer root index and its observed evidence for an item.
    pub fn observed_root_for(&self, item: &ItemRef) -> Result<(u32, &ResultScopeRoot), QueryError> {
        let detail = self.detail(item)?;
        let root = *self
            .root_indices
            .get(detail.item.item_no as usize)
            .ok_or(QueryError::PathUnavailable)?;
        let evidence = self
            .roots
            .get(root as usize)
            .ok_or(QueryError::PathUnavailable)?;
        Ok((root, evidence))
    }

    /// Returns the aggregate for an item in this pinned version, or `None` for an invalid number.
    pub fn space(&self, item_no: u32) -> Option<&SpaceAggregate> {
        self.aggregates.get(item_no as usize)
    }

    /// Reads a file size or folder aggregate only when its recorded basis matches `basis`.
    ///
    /// Missing or mismatched file evidence remains absent; this method does not convert
    /// between bases or infer a replacement value.
    pub fn item_size(&self, item_no: u32, basis: SizeBasis) -> Option<&SizeValue> {
        let item = self.items.get(item_no as usize)?;
        let size = if item.kind == ResultItemKind::Directory {
            let aggregate = self.space(item.item_no)?;
            match &basis {
                SizeBasis::Logical => &aggregate.logical,
                SizeBasis::Allocated => &aggregate.allocated,
                SizeBasis::UniquePhysical => &aggregate.unique_physical,
            }
        } else {
            match &basis {
                SizeBasis::Logical => item.size.as_ref(),
                SizeBasis::Allocated => self
                    .physical
                    .as_ref()?
                    .get(item.item_no as usize)?
                    .as_ref()?
                    .allocated
                    .as_ref(),
                SizeBasis::UniquePhysical => None,
            }?
        };
        (size.basis == basis).then_some(size)
    }

    /// Resolves an item reference against this snapshot, including an older snapshot held open.
    pub fn detail(&self, item: &ItemRef) -> Result<ItemDetail<'_>, QueryError> {
        if item.result_version != self.version_id {
            return Err(QueryError::StaleVersion);
        }
        let index = item.item_no as usize;
        let item = self.items.get(index).ok_or(QueryError::NotFound)?;
        let physical = self
            .physical
            .as_ref()
            .and_then(|values| values.get(index))
            .and_then(Option::as_ref);
        Ok(ItemDetail { item, physical })
    }

    /// Reads the immediate children in the stable parent/name order used at publication.
    pub(crate) fn children(&self, parent: u32) -> &[ResultItem] {
        let parent = Some(parent);
        let start = self.items.partition_point(|item| item.parent < parent);
        let end = self.items.partition_point(|item| item.parent <= parent);
        &self.items[start..end]
    }

    /// Returns the root-to-item chain in the same immutable hierarchy.
    pub(crate) fn ancestors<'a>(
        &'a self,
        item: &'a ResultItem,
    ) -> Result<Vec<&'a ResultItem>, QueryError> {
        let mut chain = Vec::new();
        let mut current = item;
        loop {
            chain.push(current);
            let Some(parent) = current.parent else {
                break;
            };
            current = self
                .items
                .get(parent as usize)
                .ok_or(QueryError::NotFound)?;
        }
        chain.reverse();
        Ok(chain)
    }

    /// Resolves a raw root-relative path to a confirmed directory's aggregate in this snapshot.
    pub fn folder(&self, version: &Id, path: &[Vec<u8>]) -> Result<&SpaceAggregate, QueryError> {
        if version != &self.version_id {
            return Err(QueryError::StaleVersion);
        }
        if path.iter().any(Vec::is_empty) {
            return Err(QueryError::InvalidPath);
        }
        if self.scope_roots().len() != 1 {
            return Err(QueryError::PathUnavailable);
        }
        let mut index = 0usize;
        for component in path {
            ensure_directory(self.items[index].kind)?;
            let parent = u32::try_from(index).map_err(|_| QueryError::NotFound)?;
            let children = self.children(parent);
            let offset = children
                .binary_search_by(|item| item.name.cmp(component))
                .map_err(|_| QueryError::NotFound)?;
            index = children[offset].item_no as usize;
        }
        ensure_directory(self.items[index].kind)?;
        self.aggregates.get(index).ok_or(QueryError::NotFound)
    }
}

pub(crate) fn ensure_directory(kind: ResultItemKind) -> Result<(), QueryError> {
    match kind {
        ResultItemKind::Directory => Ok(()),
        ResultItemKind::Unknown | ResultItemKind::UnknownStructure => Err(QueryError::KindUnknown),
        ResultItemKind::File | ResultItemKind::UntraversedLink => Err(QueryError::NotDirectory),
    }
}
impl ResultVersionRead for ResultVersion {
    fn directory_case_evidence(
        &self,
        item: &ItemRef,
    ) -> Option<&du_core_contracts::scan_observation::DirectoryCaseEvidence> {
        if item.result_version != self.version_id {
            return None;
        }
        self.directory_case.get(&item.item_no)
    }

    fn version_id(&self) -> &Id {
        &self.version_id
    }
    fn scope_version_id(&self) -> &Id {
        &self.scope_version_id
    }
    fn completeness(&self) -> ResultCompleteness {
        self.completeness
    }
    fn items(&self) -> &[ResultItem] {
        &self.items
    }

    fn observed_root(&self) -> Option<&ResultScopeRoot> {
        match self.roots.as_slice() {
            [root] => Some(root),
            _ => None,
        }
    }

    fn observed_roots(&self) -> &[ResultScopeRoot] {
        &self.roots
    }

    fn scope_gaps(&self) -> &[du_core_contracts::result::ResultScopeGap] {
        &self.gaps
    }

    fn observed_at(&self) -> Option<&Timestamp> {
        self.observed_at.as_ref()
    }
}
