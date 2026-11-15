//! Cancelled-unread folder counts per aggregation target.
//!
//! A subtree is not a contiguous range of item numbers, so a count cannot be read from the
//! marks by range. Publication walks the ancestor chain of every cancelled mark once and
//! keeps only the folders that have at least one cancelled-unread folder at or below them.
//!
//! # Cost
//!
//! Building is O(cancelled marks x tree depth) and runs in `FolderMarks::bind`, before the
//! store lock is taken; it never visits the items. A folder holds 8 bytes (item number and
//! count) only when a cancelled mark is at or below it, and nothing otherwise. A lookup is a
//! binary search, O(log n) in the number of such folders, with no allocation.
use crate::{QueryError, ResultVersion, result_read::ensure_directory};
use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultItem},
};
use std::collections::BTreeMap;

/// Counts, ascending by item number, of the cancelled marks at or below each folder.
///
/// `items` is indexed by item number and a parent always has a lower number than its child.
/// A folder appears once, only with a count of at least one.
pub(crate) fn build(
    items: &[ResultItem],
    cancelled: impl Iterator<Item = u32>,
) -> Box<[(u32, u32)]> {
    let mut counts: BTreeMap<u32, u32> = BTreeMap::new();
    for marked in cancelled {
        let mut at = Some(marked);
        while let Some(item_no) = at {
            let count = counts.entry(item_no).or_insert(0);
            *count = count.saturating_add(1);
            // The strict decrease also ends the walk on a malformed parent reference.
            at = items
                .get(item_no as usize)
                .and_then(|item| item.parent)
                .filter(|parent| *parent < item_no);
        }
    }
    counts.into_iter().collect()
}

impl ResultVersion {
    /// How many folders at or below one folder were left unread because the scan was
    /// cancelled, or `None` when this version does not provide the fact.
    ///
    /// The set is that of the unread marks with [`crate::folder_collection::UnreadReason::Cancelled`]:
    /// the folder itself when it is marked, plus every marked folder below it. Other-volume
    /// and already-counted folders, open or read failures and disconnected folders are not
    /// counted. `Some(0)` confirms that none exists in this version.
    ///
    /// `None` for a version that is not a real scan (a sample), for a publication whose
    /// scan has not finished, and for a cancelled or partial result whose collector recorded
    /// no unread folder: without a single cancelled mark the zero cannot be told from a
    /// collector that does not record them. Otherwise the sealed publication answers. The
    /// call is O(log n).
    ///
    /// # Errors
    /// `StaleVersion` when the reference pins another version, `NotFound` when the item
    /// number is not in this version, and `NotDirectory` or `KindUnknown` when the item is
    /// not a directory.
    pub fn cancelled_unread_folders(&self, item: &ItemRef) -> Result<Option<u32>, QueryError> {
        let item = self.detail(item)?.item;
        ensure_directory(item.kind)?;
        Ok(self
            .provides_unread_counts()
            .then(|| self.folder_marks.cancelled_at_or_below(item.item_no)))
    }

    /// How many folders of the whole scope were left unread because the scan was cancelled,
    /// or `None` when this version does not provide the fact.
    ///
    /// Equals `unread_folder_counts().cancelled` whenever a value is provided, and follows
    /// the omission rules of [`ResultVersion::cancelled_unread_folders`], including the one
    /// for a cancelled or partial result whose collector recorded no unread folder. The call
    /// is O(1).
    pub fn cancelled_unread_folders_in_scope(&self) -> Option<u32> {
        self.provides_unread_counts()
            .then(|| self.folder_marks.unread_counts().cancelled)
    }

    /// Whether the count is trustworthy: the sealed publication of a real scan that either
    /// was not cancelled (complete or missing, so nothing can be cancelled-unread) or carries
    /// at least one cancelled mark (the collector reports them).
    fn provides_unread_counts(&self) -> bool {
        self.data_source.is_real()
            && self.folder_marks.sealed()
            && (matches!(
                self.completeness,
                ResultCompleteness::Complete | ResultCompleteness::Missing
            ) || self.folder_marks.unread_counts().cancelled > 0)
    }
}
