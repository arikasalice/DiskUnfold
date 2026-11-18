//! Observation adders of [`BuildHandle`].
use crate::{
    BuildError, BuildHandle, Observation, SpaceObservation, observation::StoredObservation,
};
use du_core_contracts::result::ResultItemKind;
use std::sync::Arc;

impl BuildHandle {
    /// Adds a confirmed directory with no independent logical-size contribution.
    pub fn add_directory(&mut self, path: Vec<Vec<u8>>) -> Result<(), BuildError> {
        self.insert(
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
    /// Adds a link whose target was not traversed; its unknown own size propagates as missing.
    pub fn add_untraversed_link(&mut self, path: Vec<Vec<u8>>) -> Result<(), BuildError> {
        self.insert(
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
    /// Adds a file and its separately observed physical-space evidence.
    ///
    /// Identity must be an OS-provided volume/file pair, never a path or content hash.
    /// Missing or inconsistent evidence is retained as unknown during aggregation.
    pub fn add_with_space(
        &mut self,
        observation: Observation,
        space: SpaceObservation,
    ) -> Result<(), BuildError> {
        self.insert(
            observation.path,
            StoredObservation {
                size: observation.size,
                kind: ResultItemKind::File,
                physical: Some(Arc::new(space)),
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
    }
    /// Adds a confirmed file even if its logical size could not be read.
    pub fn add_file(&mut self, observation: Observation) -> Result<(), BuildError> {
        self.insert(
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
    pub fn add(&mut self, observation: Observation) -> Result<(), BuildError> {
        let kind = if observation.size.is_some() {
            ResultItemKind::File
        } else {
            ResultItemKind::Unknown
        };
        self.insert(
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
}
