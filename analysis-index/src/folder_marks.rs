//! Per-folder collection marks retained in immutable result versions.
//!
//! The marks are sparse: a folder without a boundary issue and without an
//! excluded-child record holds no memory. Both tables are sorted by item number
//! and read by binary search, so a lookup is O(log n) in the number of marked
//! folders.
use crate::{BuildError, BuildHandle, observation_store::ObservationStore, tree::BuiltItems};
use du_core_contracts::{
    result::ResultItemKind,
    scan_failures::{ScanIssue, unentered_reason},
};
use std::collections::BTreeMap;

/// Why a directory item was deliberately not entered by the scope rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BoundaryKind {
    OtherVolume,
    AlreadyCounted,
}

/// The excluded direct children the collector reported for one folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExcludedRecord {
    pub(crate) count: u32,
    /// The folder's enumeration ran to its end.
    pub(crate) complete: bool,
}

/// Collection facts of one publication, keyed by item number.
#[derive(Debug, Default)]
pub(crate) struct FolderMarks {
    /// Whether this publication is the sealed (final) one of its build.
    sealed: bool,
    /// Directory items with a boundary issue, ascending by item number, one per item.
    boundaries: Box<[(u32, BoundaryKind)]>,
    /// Folders with an excluded-child record, ascending by item number.
    excluded: Box<[(u32, ExcludedRecord)]>,
}

impl BuildHandle {
    /// Records how many direct children of one folder the exclusion rules dropped.
    ///
    /// `root` identifies the actual input root and an empty path selects that root.
    /// `count` is the number of dropped direct children and `complete` tells whether
    /// the folder's enumeration ran to its end (`false` when it was cut short, so
    /// `count` is a lower bound). Only reported folders are stored. A later record for
    /// the same folder replaces the earlier one in future publications only.
    ///
    /// # Errors
    /// Returns `Unavailable` after the build finishes and `InvalidTree` when the exact
    /// root and path do not name a previously registered directory.
    pub fn set_excluded_children(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        count: u32,
        complete: bool,
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
        self.excluded_children
            .insert(node, ExcludedRecord { count, complete });
        Ok(())
    }
}

impl FolderMarks {
    /// Resolves the recorded facts to the item numbers of one publication.
    ///
    /// Work is O(boundary issues x path depth) hash lookups plus a sort of the marked
    /// folders. It never visits the items and runs before the store lock is taken.
    pub(crate) fn bind(
        built: &BuiltItems,
        store: &ObservationStore,
        issues: &[ScanIssue],
        excluded: &BTreeMap<u32, ExcludedRecord>,
        sealed: bool,
    ) -> Self {
        let mut boundaries: Vec<(u32, BoundaryKind)> = issues
            .iter()
            .filter_map(|issue| boundary_mark(built, store, issue))
            .collect();
        // Sorted by (item, kind): the first mark of an item wins when both kinds exist.
        boundaries.sort_unstable();
        boundaries.dedup_by_key(|mark| mark.0);
        let mut excluded: Vec<(u32, ExcludedRecord)> = excluded
            .iter()
            .filter_map(|(node, record)| {
                let item = built.item_of_node.get(*node as usize)?;
                Some((*item, *record))
            })
            .collect();
        excluded.sort_unstable_by_key(|entry| entry.0);
        Self {
            sealed,
            boundaries: boundaries.into_boxed_slice(),
            excluded: excluded.into_boxed_slice(),
        }
    }

    pub(crate) fn sealed(&self) -> bool {
        self.sealed
    }

    pub(crate) fn boundary(&self, item_no: u32) -> Option<BoundaryKind> {
        let at = self
            .boundaries
            .binary_search_by_key(&item_no, |mark| mark.0)
            .ok()?;
        self.boundaries.get(at).map(|mark| mark.1)
    }

    pub(crate) fn excluded(&self, item_no: u32) -> Option<ExcludedRecord> {
        let at = self
            .excluded
            .binary_search_by_key(&item_no, |entry| entry.0)
            .ok()?;
        self.excluded.get(at).map(|entry| entry.1)
    }
}

/// The boundary mark of an issue whose path is an observed directory item.
///
/// An issue with another reason, or whose path is not an item, marks nothing.
fn boundary_mark(
    built: &BuiltItems,
    store: &ObservationStore,
    issue: &ScanIssue,
) -> Option<(u32, BoundaryKind)> {
    let kind = match issue.reason_key.as_str() {
        unentered_reason::BOUNDARY_OTHER_VOLUME => BoundaryKind::OtherVolume,
        unentered_reason::REVISIT_BLOCKED => BoundaryKind::AlreadyCounted,
        _ => return None,
    };
    let node = store.find(issue.root_index, &issue.relative)?;
    let item = *built.item_of_node.get(node as usize)?;
    let observed = built.items.get(item as usize)?;
    (observed.kind == ResultItemKind::Directory).then_some((item, kind))
}
