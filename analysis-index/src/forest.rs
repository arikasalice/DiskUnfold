//! Forest-scoped observation ingestion.
use crate::{BuildError, BuildHandle, Observation, SpaceObservation, StoredObservation};
use du_core_contracts::{
    Timestamp,
    result::{ResultItemKind, ResultScopeGap},
};

impl BuildHandle {
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
