//! Forest-scoped observation ingestion.
use std::collections::{BTreeMap, HashMap};

use crate::{
    BuildError, BuildHandle, Observation, SpaceObservation, StoredObservation,
    observation_store::ObservationStore, space, tree::BuiltItems,
};
use du_core_contracts::{
    Timestamp,
    result::{ResultItemKind, ResultScopeGap, SpaceAggregate},
};

/// Marks ancestors of unread ranges and indexes gaps with an exact observed item.
///
/// See [`mark_unread_folders`] for folders left unread without a range.
pub(crate) fn mark_gaps(
    built: &BuiltItems,
    store: &ObservationStore,
    gaps: &[ResultScopeGap],
    aggregates: &mut [SpaceAggregate],
) -> Result<BTreeMap<u32, Vec<usize>>, BuildError> {
    let mut exact = BTreeMap::<u32, Vec<usize>>::new();
    let mut counts = HashMap::<usize, u64>::new();
    for (offset, gap) in gaps.iter().enumerate() {
        // The deepest observed item on the gap's path; the root always exists in a built forest.
        let (node, whole_path) = store
            .deepest(gap.root_index, &gap.path)
            .ok_or(BuildError::InvalidTree)?;
        let index = built.item_of_node[node as usize] as usize;
        if whole_path {
            exact
                .entry(built.items[index].item_no)
                .or_default()
                .push(offset);
        }
        let mut current = Some(index);
        while let Some(index) = current {
            let count = counts.entry(index).or_default();
            *count = count.saturating_add(1);
            current = built.items[index].parent.map(|parent| parent as usize);
        }
    }
    for (index, count) in counts {
        space::mark_inaccessible(&mut aggregates[index], count, built.physical.is_some());
    }
    Ok(exact)
}

/// Folders left unread by a cancellation or disconnection mark, and every folder
/// above them, are at least the bytes read below (#341).
pub(crate) fn mark_unread_folders<'a>(
    built: &BuiltItems,
    nodes: impl Iterator<Item = &'a u32>,
    aggregates: &mut [SpaceAggregate],
) {
    for &node in nodes {
        let mut current = built
            .item_of_node
            .get(node as usize)
            .map(|&index| index as usize);
        while let Some(index) = current {
            if let Some(aggregate) = aggregates.get_mut(index) {
                space::mark_unread(aggregate, built.physical.is_some());
            }
            current = built.items[index].parent.map(|parent| parent as usize);
        }
    }
}

impl BuildHandle {
    /// Retains historical metadata flags only for a same-volume matched item.
    /// The observation is not a content-open authorization.
    pub fn retain_placeholder_observation(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        value: du_core_contracts::scan_observation::PlaceholderObservation,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::InvalidTree);
        }
        let item = self
            .observations
            .get_mut(root, path)
            .ok_or(BuildError::InvalidTree)?;
        if item.kind != ResultItemKind::File {
            return Err(BuildError::InvalidTree);
        }
        if item.enumerated_inode.is_some() {
            item.placeholder_observation = Some(value);
        }
        Ok(())
    }
    /// Retains a matched enumeration discriminator only with same-root volume identity.
    ///
    /// The producer must compare enumeration and metadata from the same observation.
    /// A missing root or physical identity leaves this evidence unavailable.
    pub fn retain_enumerated_inode(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        inode: std::num::NonZeroU64,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::InvalidTree);
        }
        let observation = self
            .observations
            .get_mut(root, path)
            .ok_or(BuildError::InvalidTree)?;
        if observation.kind != ResultItemKind::File {
            return Err(BuildError::InvalidTree);
        }
        let root_volume = self
            .roots
            .get(root as usize)
            .map(|root| root.volume_id.as_slice());
        let identity = observation
            .physical
            .as_deref()
            .and_then(|physical| physical.identity.as_ref());
        if let (Some(volume), Some(identity)) = (root_volume, identity)
            && !volume.is_empty()
            && !identity.file.is_empty()
            && identity.volume == volume
        {
            observation.enumerated_inode = Some(inode);
        }
        Ok(())
    }
    /// Retains native modification evidence for an admitted file observation.
    ///
    /// The collector must supply evidence from the same verified observation.
    pub fn set_native_modified_at(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        time: du_core_contracts::scan_observation::NativeModifiedTime,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::InvalidTree);
        }
        let observation = self
            .observations
            .get_mut(root, path)
            .ok_or(BuildError::InvalidTree)?;
        if observation.kind != ResultItemKind::File {
            return Err(BuildError::InvalidTree);
        }
        observation.native_modified_at = Some(time);
        Ok(())
    }
    /// Adds a root, directory, or deliberately excluded link with no logical byte contribution.
    ///
    /// This does not indicate that an unread directory was successfully enumerated;
    /// the caller must preserve any omitted work in the result completeness.
    pub fn add_structure(&mut self, path: Vec<Vec<u8>>) -> Result<(), BuildError> {
        self.insert(
            path,
            StoredObservation {
                size: None,
                kind: ResultItemKind::UnknownStructure,
                physical: None,
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
    }

    /// Retains an observed modification time for an already admitted item.
    pub fn set_modified_at(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        time: Timestamp,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::InvalidTree);
        }
        let observation = self
            .observations
            .get_mut(root, path)
            .ok_or(BuildError::InvalidTree)?;
        observation.modified_at = Some(time);
        observation.native_modified_at = None;
        Ok(())
    }
    /// Retains a time captured by the adapter immediately after item observation.
    pub fn set_item_observed_at(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        time: Timestamp,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::InvalidTree);
        }
        let observation = self
            .observations
            .get_mut(root, path)
            .ok_or(BuildError::InvalidTree)?;
        observation.observed_at = Some(time);
        Ok(())
    }

    /// Adds an observation using the same kind inference as legacy `add`.
    /// An unavailable size remains an unknown kind, not a confirmed file.
    pub fn add_at(&mut self, root_index: u32, observation: Observation) -> Result<(), BuildError> {
        let kind = if observation.size.is_some() {
            ResultItemKind::File
        } else {
            ResultItemKind::Unknown
        };
        self.insert_at(
            root_index,
            observation.path,
            StoredObservation {
                size: observation.size,
                kind,
                physical: None,
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
    }

    /// Adds a directory at a path relative to the indexed forest root.
    pub fn add_directory_at(
        &mut self,
        root_index: u32,
        path: Vec<Vec<u8>>,
    ) -> Result<(), BuildError> {
        self.insert_at(
            root_index,
            path,
            StoredObservation {
                size: None,
                kind: ResultItemKind::Directory,
                physical: None,
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
    }

    /// Adds a file at a path relative to the indexed forest root.
    pub fn add_file_at(
        &mut self,
        root_index: u32,
        observation: Observation,
    ) -> Result<(), BuildError> {
        self.insert_at(
            root_index,
            observation.path,
            StoredObservation {
                size: observation.size,
                kind: ResultItemKind::File,
                physical: None,
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
    }

    /// Adds a file and its independently observed physical-space evidence.
    pub fn add_with_space_at(
        &mut self,
        root_index: u32,
        observation: Observation,
        space: SpaceObservation,
    ) -> Result<(), BuildError> {
        self.insert_at(
            root_index,
            observation.path,
            StoredObservation {
                size: observation.size,
                kind: ResultItemKind::File,
                physical: Some(std::sync::Arc::new(space)),
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
    }

    /// Adds a link without following it, relative to the indexed forest root.
    ///
    /// The link's own size is unknown, so it keeps its folder and the scope total a lower
    /// bound. Use [`Self::add_observed_untraversed_link_at`] when the link object was observed.
    pub fn add_untraversed_link_at(
        &mut self,
        root_index: u32,
        path: Vec<Vec<u8>>,
    ) -> Result<(), BuildError> {
        self.insert_at(
            root_index,
            path,
            StoredObservation {
                size: None,
                kind: ResultItemKind::UntraversedLink,
                physical: None,
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
    }

    /// Adds a link without following it, with the observation of the link object itself.
    ///
    /// The untraversed target is only counted in `untraversed_links` and does not lower any
    /// value or completeness. The link's own logical size and optional space evidence count
    /// like a file's; a missing or unknown own size keeps the values a lower bound.
    pub fn add_observed_untraversed_link_at(
        &mut self,
        root_index: u32,
        observation: Observation,
        space: Option<SpaceObservation>,
    ) -> Result<(), BuildError> {
        self.insert_at(
            root_index,
            observation.path,
            StoredObservation {
                size: observation.size,
                kind: ResultItemKind::UntraversedLink,
                physical: space.map(std::sync::Arc::new),
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
    }

    /// Adds a file entry outside the scan boundary, such as a file bind-mounted from another
    /// volume or a file on another volume. It is an unread range (F03 FR5.HD1, DEC-006): its
    /// own size stays unknown, and it adds neither bytes nor a missing item to its folder,
    /// its ancestors or the scope. The collector reports the range separately.
    pub fn add_boundary_file_at(
        &mut self,
        root_index: u32,
        path: Vec<Vec<u8>>,
    ) -> Result<(), BuildError> {
        self.insert_at(
            root_index,
            path.clone(),
            StoredObservation {
                size: None,
                kind: ResultItemKind::File,
                physical: None,
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )?;
        let node = self
            .observations
            .locate(root_index, &path)
            .ok_or(BuildError::InvalidTree)?;
        self.observations.mark_boundary(node);
        Ok(())
    }

    /// Records an unread range; an empty relative path identifies the root itself.
    pub fn record_gap_at(
        &mut self,
        root_index: u32,
        path: Vec<Vec<u8>>,
        reason_key: String,
        observed_at: Option<Timestamp>,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if !self.valid_root(root_index) || path.iter().any(Vec::is_empty) || reason_key.is_empty() {
            return Err(BuildError::InvalidTree);
        }
        self.gaps.push(ResultScopeGap {
            root_index,
            path,
            reason_key,
            observed_at,
        });
        Ok(())
    }
}
