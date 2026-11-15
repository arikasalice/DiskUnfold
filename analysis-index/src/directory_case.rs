//! Retention of per-directory case observations in immutable result versions.
use crate::{BuildError, BuildHandle, tree::BuiltItems};
use du_core_contracts::{result::ResultItemKind, scan_observation::DirectoryCaseEvidence};
use std::collections::BTreeMap;

impl BuildHandle {
    /// Binds an observation to an already registered, confirmed directory.
    ///
    /// `root` identifies the actual input root and an empty path selects that
    /// root. This records evidence supplied by the verified handle owner; it
    /// does not derive a path comparison rule or query the operating system.
    /// A later observation replaces the earlier one only in future publications.
    ///
    /// # Errors
    /// Returns `Unavailable` after the build finishes and `InvalidTree` when
    /// the exact root and path do not name a previously registered directory.
    pub fn set_directory_case_evidence(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        evidence: DirectoryCaseEvidence,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if !self.valid_root(root) {
            return Err(BuildError::InvalidTree);
        }
        let node = self
            .observations
            .locate(root, path)
            .filter(|node| {
                self.observations.observation(*node).map(|item| item.kind)
                    == Some(ResultItemKind::Directory)
            })
            .ok_or(BuildError::InvalidTree)?;
        self.directory_case.insert(node, evidence);
        Ok(())
    }
}

/// Resolves the recorded directories to the item numbers of this publication.
pub(crate) fn bind(
    built: &BuiltItems,
    evidence: &BTreeMap<u32, DirectoryCaseEvidence>,
) -> BTreeMap<u32, DirectoryCaseEvidence> {
    evidence
        .iter()
        .map(|(node, value)| (built.item_of_node[*node as usize], value.clone()))
        .collect()
}
