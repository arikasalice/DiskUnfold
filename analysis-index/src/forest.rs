//! Forest-scoped observation ingestion.
use std::collections::{BTreeMap, HashMap};

use crate::{
    BuildError, BuildHandle, Observation, SpaceObservation, StoredObservation, space,
    tree::BuiltItems,
};
use du_core_contracts::{
    Timestamp,
    result::{ResultItemKind, ResultScopeGap, SpaceAggregate},
};

/// Marks ancestors of unread ranges and indexes gaps with an exact observed item.
pub(crate) fn mark_gaps(
    built: &BuiltItems,
    gaps: &[ResultScopeGap],
    aggregates: &mut [SpaceAggregate],
) -> Result<BTreeMap<u32, Vec<usize>>, BuildError> {
    let mut exact = BTreeMap::<u32, Vec<usize>>::new();
    if gaps.is_empty() {
        return Ok(exact);
    }
    let key_indices: HashMap<_, _> = built
        .keys
        .iter()
        .enumerate()
        .map(|(index, (root, path))| ((*root, path.as_slice()), index))
        .collect();
    let mut counts = HashMap::<usize, u64>::new();
    for (offset, gap) in gaps.iter().enumerate() {
        let mut path = gap.path.as_slice();
        let index = loop {
            if let Some(&index) = key_indices.get(&(gap.root_index, path)) {
                if path.len() == gap.path.len() {
                    exact
                        .entry(built.items[index].item_no)
                        .or_default()
                        .push(offset);
                }
                break index;
            }
            let (_, parent) = path.split_last().ok_or(BuildError::InvalidTree)?;
            path = parent;
        };
        let mut current = Some(index);
        while let Some(index) = current {
            let count = counts.entry(index).or_default();
            *count = count.saturating_add(1);
            current = built.items[index].parent.map(|parent| parent as usize);
        }
    }
    for (index, count) in counts {
        space::mark_inaccessible(&mut aggregates[index], count);
    }
    Ok(exact)
}

impl BuildHandle {
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
            .get_mut(&(root, path.to_vec()))
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
            .get_mut(&(root, path.to_vec()))
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
            .get_mut(&(root, path.to_vec()))
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
            .get_mut(&(root, path.to_vec()))
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
                physical: Some(Box::new(space)),
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                observed_at: None,
            },
        )
    }

    /// Adds a link without following it, relative to the indexed forest root.
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
                observed_at: None,
            },
        )
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
