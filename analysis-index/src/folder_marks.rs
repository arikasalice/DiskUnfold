//! Per-folder collection marks retained in immutable result versions.
//!
//! The marks are sparse: a folder without a boundary issue and without an
//! unread mark holds no memory. Both tables are sorted by item number
//! and read by binary search, so a lookup is O(log n) in the number of marked
//! folders. Cancelled marks are evidence for the named folders only; they do not
//! establish complete coverage for a cancelled-unread total.
use crate::{
    BuildError, BuildHandle, folder_evidence_api::FolderAttemptStatus,
    folder_evidence_snapshot::FolderEvidenceSnapshot, observation_store::ObservationStore,
    tree::BuiltItems,
};
use du_core_contracts::{
    result::ResultItemKind,
    scan_failures::{ScanIssue, unentered_reason},
    scan_observation::FilterDecision,
};
use std::collections::BTreeMap;

/// Why a directory item was deliberately not entered by the scope rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BoundaryKind {
    OtherVolume,
    AlreadyCounted,
}

/// Why the collector left a folder's entries unread without recording an unread range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnreadReason {
    /// The scan was cancelled before the folder was read.
    Cancelled,
    /// The volume disconnected before the folder was read.
    Disconnected,
}

/// How many folders of a version carry each [`UnreadReason`] mark.
///
/// Counts marks as recorded, including a folder that also has a gap. They are separate from
/// the inaccessible ranges of the aggregates and from the scan issues.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnreadFolderCounts {
    pub cancelled: u32,
    pub disconnected: u32,
}

/// Collection facts of one publication, keyed by item number.
#[derive(Debug, Default)]
pub(crate) struct FolderMarks {
    /// Whether this publication is the sealed (final) one of its build.
    sealed: bool,
    /// Directory items with a boundary issue, ascending by item number, one per item.
    boundaries: Box<[(u32, BoundaryKind)]>,
    /// Folders reported as left unread, ascending by item number.
    unread: Box<[(u32, UnreadReason)]>,
    /// Folders whose own pinned attempts all ended, ascending by item number.
    ended: Box<[u32]>,
    /// Strict ancestors of the folders left unread by a mark, ascending by item number.
    unread_below: Box<[u32]>,
    /// Folders with at least one pinned attempt, ascending by item number.
    attempted: Box<[u32]>,
    /// What folder attempt evidence this publication holds.
    evidence: EvidenceState,
    /// Why this version's lower bounds may have no row reason, decided once (#226).
    pub(crate) cause: std::sync::OnceLock<&'static str>,
    /// Excluded direct children of folders with exactly one pinned attempt that ended,
    /// ascending by item number.
    excluded: Box<[(u32, u32)]>,
    unread_counts: UnreadFolderCounts,
}

/// The folder attempt evidence a publication holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EvidenceState {
    /// No evidence was recorded, as for a replacement build or an older result.
    #[default]
    Absent,
    /// The evidence was recorded but dropped because it did not fit the budget.
    Dropped,
    /// The evidence is pinned in the publication.
    Pinned,
}

impl BuildHandle {
    /// Records how many direct children of one folder the exclusion rules dropped.
    ///
    /// `root` identifies the actual input root and an empty path selects that root.
    /// `count` is the number of dropped direct children and `complete` tells whether
    /// enumeration ran to its end. These records are not published or retained: the count
    /// comes from the folder's attempt evidence instead (see `excluded_children`).
    ///
    /// # Errors
    /// Returns `Unavailable` after the build finishes and `InvalidTree` when the exact
    /// root and path do not name a previously registered directory.
    pub fn set_excluded_children(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        _count: u32,
        _complete: bool,
    ) -> Result<(), BuildError> {
        self.registered_directory(root, path)?;
        Ok(())
    }

    /// Marks one folder as not read because the scan was cancelled or the volume
    /// disconnected, without an unread-range row.
    ///
    /// `root` identifies the actual input root and an empty path selects that root.
    /// The mark is for folders the collector entered or had to enter but did not read to
    /// the end: a folder it never opened and a folder whose enumeration it cut short. A
    /// folder with a recorded gap needs no mark, and a gap's reason wins over the mark. A
    /// mark is recorded only by a scan that ends cancelled or partial. The mark does
    /// not touch aggregation, so the caller seals the version as `Partial` or
    /// `Cancelled`, which already makes every size a lower bound or unknown. A later
    /// record for the same folder replaces the earlier one in future publications only.
    /// Each mark costs 8 bytes in a published version; unmarked folders cost nothing.
    ///
    /// # Errors
    /// Returns `Unavailable` after the build finishes and `InvalidTree` when the exact
    /// root and path do not name a previously registered directory.
    pub fn set_directory_unread(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        reason: UnreadReason,
    ) -> Result<(), BuildError> {
        let node = self.registered_directory(root, path)?;
        self.unread_folders.insert(node, reason);
        Ok(())
    }

    /// The store node of an already registered directory, with the build still open.
    fn registered_directory(&mut self, root: u32, path: &[Vec<u8>]) -> Result<u32, BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if !self.valid_root(root) {
            return Err(BuildError::InvalidTree);
        }
        self.observations
            .locate(root, path)
            .filter(|node| {
                self.observations.observation(*node).map(|item| item.kind)
                    == Some(ResultItemKind::Directory)
            })
            .ok_or(BuildError::InvalidTree)
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
        unread: &BTreeMap<u32, UnreadReason>,
        evidence: Option<&FolderEvidenceSnapshot>,
        evidence_dropped: bool,
        sealed: bool,
    ) -> Self {
        let mut boundaries: Vec<(u32, BoundaryKind)> = issues
            .iter()
            .filter_map(|issue| boundary_mark(built, store, issue))
            .collect();
        // Sorted by (item, kind): the first mark of an item wins when both kinds exist.
        boundaries.sort_unstable();
        boundaries.dedup_by_key(|mark| mark.0);
        let mut unread: Vec<(u32, UnreadReason)> = unread
            .iter()
            .filter_map(|(node, reason)| {
                let item = built.item_of_node.get(*node as usize)?;
                Some((*item, *reason))
            })
            .collect();
        unread.sort_unstable_by_key(|entry| entry.0);
        let count = |wanted| {
            let total = unread.iter().filter(|entry| entry.1 == wanted).count();
            u32::try_from(total).unwrap_or(u32::MAX)
        };
        let unread_counts = UnreadFolderCounts {
            cancelled: count(UnreadReason::Cancelled),
            disconnected: count(UnreadReason::Disconnected),
        };
        // Each mark walks up its ancestors once: O(marks x depth).
        let mut unread_below = Vec::new();
        for (item, _) in &unread {
            let mut parent = built.items.get(*item as usize).and_then(|item| item.parent);
            while let Some(folder) = parent {
                unread_below.push(folder);
                parent = built
                    .items
                    .get(folder as usize)
                    .and_then(|item| item.parent);
            }
        }
        unread_below.sort_unstable();
        unread_below.dedup();
        let mut attempted: Vec<u32> = evidence
            .map(|evidence| {
                evidence
                    .summaries()
                    .iter()
                    .map(|summary| summary.folder_item)
                    .collect()
            })
            .unwrap_or_default();
        attempted.sort_unstable();
        attempted.dedup();
        let state = match (evidence.is_some(), evidence_dropped) {
            (true, _) => EvidenceState::Pinned,
            (false, true) => EvidenceState::Dropped,
            (false, false) => EvidenceState::Absent,
        };
        Self {
            sealed,
            boundaries: boundaries.into_boxed_slice(),
            unread: unread.into_boxed_slice(),
            ended: ended_folders(evidence),
            unread_below: unread_below.into_boxed_slice(),
            attempted: attempted.into_boxed_slice(),
            evidence: state,
            cause: std::sync::OnceLock::new(),
            excluded: excluded_children(evidence),
            unread_counts,
        }
    }

    pub(crate) fn sealed(&self) -> bool {
        self.sealed
    }

    pub(crate) fn unread(&self, item_no: u32) -> Option<UnreadReason> {
        let at = self
            .unread
            .binary_search_by_key(&item_no, |entry| entry.0)
            .ok()?;
        self.unread.get(at).map(|entry| entry.1)
    }

    pub(crate) fn unread_counts(&self) -> UnreadFolderCounts {
        self.unread_counts
    }

    pub(crate) fn boundary(&self, item_no: u32) -> Option<BoundaryKind> {
        let at = self
            .boundaries
            .binary_search_by_key(&item_no, |mark| mark.0)
            .ok()?;
        self.boundaries.get(at).map(|mark| mark.1)
    }

    /// Whether every pinned attempt for this folder ended, so it was entered and read.
    pub(crate) fn ended(&self, item_no: u32) -> bool {
        self.ended.binary_search(&item_no).is_ok()
    }

    /// Whether a folder below this one carries an unread mark.
    pub(crate) fn unread_below(&self, item_no: u32) -> bool {
        self.unread_below.binary_search(&item_no).is_ok()
    }

    /// Whether this folder has any pinned attempt.
    pub(crate) fn attempted(&self, item_no: u32) -> bool {
        self.attempted.binary_search(&item_no).is_ok()
    }

    pub(crate) fn evidence(&self) -> EvidenceState {
        self.evidence
    }
    /// The excluded direct children counted from the folder's single ended attempt.
    pub(crate) fn excluded(&self, item_no: u32) -> Option<u32> {
        let at = self
            .excluded
            .binary_search_by_key(&item_no, |entry| entry.0)
            .ok()?;
        self.excluded.get(at).map(|entry| entry.1)
    }
}

/// Counts the excluded direct children of every folder whose only pinned attempt ended.
///
/// The count is the number of child IDs whose filter callback decided `Exclude` in that
/// attempt (see [`count_excluded`]). A folder with several attempts has no count: nothing
/// proves they listed the same children. Like the evidence itself, the value is what one
/// enumeration observed, not a guarantee about the folder now. Linear in the attempts and
/// events plus a sort.
fn excluded_children(evidence: Option<&FolderEvidenceSnapshot>) -> Box<[(u32, u32)]> {
    use du_core_contracts::scan_observation::CorrelatedDirectoryEvent;
    let Some(evidence) = evidence else {
        return Box::default();
    };
    let mut attempts: Vec<(u32, u64, bool, Option<u64>)> = evidence
        .summaries()
        .iter()
        .map(|summary| {
            (
                summary.folder_item,
                summary.attempt_id,
                summary.status == FolderAttemptStatus::EndedUnproven,
                summary.child_count_exclusive,
            )
        })
        .collect();
    attempts.sort_unstable_by_key(|attempt| (attempt.0, attempt.1));
    // (attempt ID, folder item, validated child count) of every folder whose only attempt ended.
    let mut single: Vec<(u64, u32, u64)> = attempts
        .chunk_by(|left, right| left.0 == right.0)
        .filter(|group| group.len() == 1 && group[0].2)
        .filter_map(|group| Some((group[0].1, group[0].0, group[0].3?)))
        .collect();
    single.sort_unstable();
    let mut callbacks: Vec<Vec<(u64, FilterDecision)>> = vec![Vec::new(); single.len()];
    let mut missing = vec![false; single.len()];
    for event in evidence.events() {
        let Ok(at) = single.binary_search_by_key(&event.attempt_id, |entry| entry.0) else {
            continue;
        };
        match &event.input {
            None => missing[at] = true,
            Some(crate::FolderAttemptInput::Adapter(
                CorrelatedDirectoryEvent::FilterCallback {
                    child_id, decision, ..
                },
            )) => callbacks[at].push((*child_id, *decision)),
            Some(_) => {}
        }
    }
    let mut counts: Vec<(u32, u32)> = single
        .iter()
        .zip(callbacks.iter_mut().zip(missing))
        .filter(|(_, (_, missing))| !missing)
        .filter_map(|(&(_, folder, children), (kept, _))| {
            Some((folder, count_excluded(children, kept)?))
        })
        .collect();
    counts.sort_unstable();
    counts.into_boxed_slice()
}

/// The excluded count of one ended attempt, from its kept filter callbacks.
///
/// Every direct-child record of an attempt has one callback, with child IDs dense from 0,
/// so the kept callbacks must cover `0..children` (the attempt's validated end) exactly
/// once each. A callback lost to the retention bound or repeated by a damaged record
/// would miscount, and an `Undecidable` decision leaves a child unclassified; each gives
/// no count.
fn count_excluded(children: u64, kept: &mut [(u64, FilterDecision)]) -> Option<u32> {
    kept.sort_unstable_by_key(|callback| callback.0);
    let dense = u64::try_from(kept.len()).is_ok_and(|kept| kept == children)
        && kept
            .iter()
            .enumerate()
            .all(|(index, callback)| u64::try_from(index).is_ok_and(|index| index == callback.0));
    if !dense
        || kept
            .iter()
            .any(|callback| callback.1 == FilterDecision::Undecidable)
    {
        return None;
    }
    u32::try_from(
        kept.iter()
            .filter(|callback| callback.1 == FilterDecision::Exclude)
            .count(),
    )
    .ok()
}

/// Folders with at least one pinned attempt, every one of them `EndedUnproven`: the
/// enumeration end and the direct-child terminals validated. Linear in the attempts.
fn ended_folders(evidence: Option<&FolderEvidenceSnapshot>) -> Box<[u32]> {
    let Some(evidence) = evidence else {
        return Box::default();
    };
    let mut attempts: Vec<(u32, bool)> = evidence
        .summaries()
        .iter()
        .map(|summary| {
            (
                summary.folder_item,
                summary.status == FolderAttemptStatus::EndedUnproven,
            )
        })
        .collect();
    attempts.sort_unstable();
    attempts
        .chunk_by(|left, right| left.0 == right.0)
        .filter(|group| group.iter().all(|attempt| attempt.1))
        .map(|group| group[0].0)
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use FilterDecision::{Admit, Exclude, Undecidable};

    #[test]
    fn an_attempt_whose_callbacks_cover_every_child_once_gives_its_exclusions() {
        let mut kept = vec![(2, Exclude), (0, Admit), (1, Exclude)];
        assert_eq!(count_excluded(3, &mut kept), Some(2));
        // Without exclusion rules every child is admitted: exactly none.
        let mut admitted = vec![(0, Admit), (1, Admit)];
        assert_eq!(count_excluded(2, &mut admitted), Some(0));
        assert_eq!(count_excluded(0, &mut []), Some(0));
    }

    #[test]
    fn a_damaged_or_incomplete_attempt_gives_no_count() {
        // A repeated child ID is damaged evidence, not one child.
        assert_eq!(count_excluded(2, &mut [(0, Exclude), (0, Exclude)]), None);
        // A callback lost to the retention bound would undercount.
        assert_eq!(count_excluded(3, &mut [(0, Exclude), (2, Admit)]), None);
        // More callbacks than the validated end, or an ID beyond it.
        assert_eq!(count_excluded(1, &mut [(0, Admit), (1, Exclude)]), None);
        assert_eq!(count_excluded(2, &mut [(0, Admit), (5, Exclude)]), None);
        // An undecided child leaves the folder unclassified.
        assert_eq!(
            count_excluded(2, &mut [(0, Exclude), (1, Undecidable)]),
            None
        );
    }
}
