//! Read the immutable item hierarchy and aggregates of a published version.
use crate::{QueryError, ResultVersion, SpaceObservation};
use du_core_contracts::{
    Id, ItemRef, SizeBasis, SizeValue, Timestamp,
    result::{
        ResultCompleteness, ResultItem, ResultItemKind, ResultScopeRoot, ResultVersionRead,
        SpaceAggregate,
    },
};

/// Original observations for one item in a pinned result, without inferring missing values.
pub struct ItemDetail<'a> {
    pub item: &'a ResultItem,
    pub physical: Option<&'a SpaceObservation>,
}

impl ResultVersion {
    /// Reads the modification time retained for this immutable item, if available.
    pub fn item_modified_at(&self, item_no: u32) -> Option<&Timestamp> {
        self.modified_times
            .as_ref()?
            .get(item_no as usize)?
            .as_ref()
    }
    /// Returns the bound analysis lineage, or None for a legacy unbound build.
    pub fn lineage_id(&self) -> Option<&Id> {
        self.lineage_id.as_ref()
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

    /// Reads observed file size or the same folder aggregate used by the space query.
    pub fn item_size(&self, item_no: u32, basis: SizeBasis) -> Option<&SizeValue> {
        let item = self.items.get(item_no as usize)?;
        if item.kind == ResultItemKind::Directory {
            let aggregate = self.space(item.item_no)?;
            return Some(match basis {
                SizeBasis::Logical => &aggregate.logical,
                SizeBasis::Allocated => &aggregate.allocated,
                SizeBasis::UniquePhysical => &aggregate.unique_physical,
            });
        }
        match basis {
            SizeBasis::Logical => item.size.as_ref(),
            SizeBasis::Allocated => self
                .physical
                .as_ref()?
                .get(item.item_no as usize)?
                .as_ref()?
                .allocated
                .as_ref(),
            SizeBasis::UniquePhysical => None,
        }
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
