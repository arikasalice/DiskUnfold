//! Retention of per-directory case observations in immutable result versions.
use crate::{BuildError, BuildHandle, tree::BuiltItems};
use du_core_contracts::{result::ResultItemKind, scan_observation::DirectoryCaseEvidence};
use std::collections::BTreeMap;

type PathKey = (u32, Vec<Vec<u8>>);

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
        let key = (root, path.to_vec());
        if !self.valid_root(root)
            || self.observations.get(&key).map(|item| item.kind) != Some(ResultItemKind::Directory)
        {
            return Err(BuildError::InvalidTree);
        }
        self.directory_case.insert(key, evidence);
        Ok(())
    }
}

/// Resolves only explicitly recorded paths to the item numbers of this publication.
pub(crate) fn bind(
    built: &BuiltItems,
    evidence: &BTreeMap<PathKey, DirectoryCaseEvidence>,
) -> BTreeMap<u32, DirectoryCaseEvidence> {
    built
        .keys
        .iter()
        .zip(&built.items)
        .filter_map(|(key, item)| evidence.get(key).map(|value| (item.item_no, value.clone())))
        .collect()
}
