//! Exact cancelled-unread totals for sealed result targets.
//!
//! A cancelled mark records a known folder, not proof that the collector marked every such
//! folder. Totals are therefore available only for a sealed real `Scan` origin with
//! `Complete` or `Missing` completeness and no cancelled marks. A cancelled mark in either a
//! complete or an incomplete result makes the total unknown.
use crate::{QueryError, ResultVersion, origin::ResultOrigin, result_read::ensure_directory};
use du_core_contracts::{ItemRef, result::ResultCompleteness};

impl ResultVersion {
    /// The exact number of cancelled-unread folders at or below one folder.
    ///
    /// `Some(0)` is provided only for a sealed real `Scan` origin with `Complete` or
    /// `Missing` completeness and no cancelled marks. A mark identifies one known folder
    /// but does not prove complete coverage, and a mark in a complete result contradicts the
    /// scan's completeness; both cases return `None`. Incremental replacements, partial,
    /// cancelled, sample, and unfinished results also return `None`. Other-volume and
    /// already-counted folders, gaps and disconnected folders are not counted.
    ///
    /// The count is available for the whole scope under the same conditions.
    ///
    /// # Errors
    /// `StaleVersion` when the reference pins another version, `NotFound` when the item
    /// number is not in this version, and `NotDirectory` or `KindUnknown` when the item is
    /// not a directory.
    pub fn cancelled_unread_folders(&self, item: &ItemRef) -> Result<Option<u32>, QueryError> {
        let item = self.detail(item)?.item;
        ensure_directory(item.kind)?;
        Ok(self.confirms_no_cancelled_unread().then_some(0))
    }

    /// The exact number of cancelled-unread folders in the whole scope.
    ///
    /// A cancelled mark does not prove complete coverage and a mark contradicts a
    /// `Complete` or `Missing` result, so either case returns `None`. `Some(0)` is provided
    /// only for a sealed real `Scan` origin with `Complete` or `Missing` completeness and no
    /// cancelled marks. Incremental replacements return `None`.
    pub fn cancelled_unread_folders_in_scope(&self) -> Option<u32> {
        self.confirms_no_cancelled_unread().then_some(0)
    }

    /// Whether the scan's terminal evidence proves the cancelled-unread total is zero.
    fn confirms_no_cancelled_unread(&self) -> bool {
        self.data_source.is_real()
            && self.origin == ResultOrigin::Scan
            && self.folder_marks.sealed()
            && matches!(
                self.completeness,
                ResultCompleteness::Complete | ResultCompleteness::Missing
            )
            && self.folder_marks.unread_counts().cancelled == 0
    }
}
