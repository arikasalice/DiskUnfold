//! Forest-scoped observation ingestion.
use crate::{
    BuildError, BuildHandle, Observation, SpaceObservation, StoredObservation, space,
    tree::BuiltItems,
};
use du_core_contracts::{
    Timestamp,
    result::{ResultItemKind, ResultScopeGap, SpaceAggregate},
};

/// Marks only ancestors of unread ranges; no index or count map is made without gaps.
pub(crate) fn mark_gaps(
    built: &BuiltItems,
    gaps: &[ResultScopeGap],
    aggregates: &mut [SpaceAggregate],
) -> Result<(), BuildError> {
    if gaps.is_empty() {
        return Ok(());
    }
    let key_indices: std::collections::HashMap<_, _> = built
        .keys
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, key)| (key, index))
        .collect();
    let mut counts = std::collections::HashMap::<usize, u64>::new();
    for gap in gaps {
        let mut path = gap.path.clone();
        let index = loop {
            if let Some(&index) = key_indices.get(&(gap.root_index, path.clone())) {
                break index;
            }
            if path.pop().is_none() {
                return Err(BuildError::InvalidTree);
            }
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
    Ok(())
}

impl BuildHandle {
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
