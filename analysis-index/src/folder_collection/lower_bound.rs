//! Why a folder's values stay lower bounds, so a screen can tell the user (#226).
//!
//! A read folder with unread ranges inside it names them ([`UnreadInside`]). A lower bound
//! that no row explains comes from missing folder attempt evidence, and the version names
//! its cause once ([`ResultVersion::lower_bound_cause`]).
use super::REASON_IN_PROGRESS;
use crate::{QueryError, ResultVersion, folder_marks::EvidenceState, origin::ResultOrigin};
use du_core_contracts::{ItemRef, result::ResultItemKind};

/// The result recorded no folder attempt evidence: a replacement build or an older
/// result. Analyzing again records it.
pub const REASON_EVIDENCE_NOT_RECORDED: &str = "scan.evidence_not_recorded";
/// Some folder attempt evidence did not fit the evidence budget.
pub const REASON_EVIDENCE_BUDGET_EXCEEDED: &str = "scan.evidence_budget_exceeded";
/// The cause cannot be told.
pub const REASON_LOWER_BOUND_UNEXPLAINED: &str = "scan.lower_bound_unexplained";

/// Unread ranges recorded inside a folder that was itself read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnreadInside {
    /// The version cannot tell yet.
    Unknown { reason_key: &'static str },
    /// Nothing inside the folder was recorded as unread.
    NoneRecorded,
    /// Some ranges inside were not read. `count` is the number of unread ranges (gaps)
    /// recorded below the folder; it is `None` when a folder below was left unread by a
    /// cancellation or disconnection mark, which is not counted as a range.
    Recorded { count: Option<u64> },
}

impl ResultVersion {
    /// Answers the unread ranges inside one folder, or `None` when this fact does not
    /// apply: the folder itself was not read, the item is not a folder, or the version is
    /// a sample. Reads the folder total and the marks already bound; it walks nothing.
    ///
    /// # Errors
    /// `StaleVersion` when the reference pins another version and `NotFound` when the item
    /// number is not in this version.
    pub fn unread_inside(&self, item: &ItemRef) -> Result<Option<UnreadInside>, QueryError> {
        let item = self.detail(item)?.item;
        if !self.data_source.is_real()
            || item.kind != ResultItemKind::Directory
            || self.recorded_unread(item.item_no).is_some()
        {
            return Ok(None);
        }
        if !self.folder_marks.sealed() {
            return Ok(Some(UnreadInside::Unknown {
                reason_key: REASON_IN_PROGRESS,
            }));
        }
        let count = self
            .space(item.item_no)
            .and_then(|aggregate| aggregate.inaccessible_ranges)
            .unwrap_or(0);
        let marked = self.folder_marks.unread_below(item.item_no);
        Ok(Some(if count == 0 && !marked {
            UnreadInside::NoneRecorded
        } else {
            UnreadInside::Recorded {
                count: (!marked).then_some(count),
            }
        }))
    }

    /// Why a lower bound that no row explains may appear in this version: one of the
    /// `REASON_*` keys of this module. `None` for a sample and while the scan has not
    /// ended, when lower bounds are the normal interim state. Decided once per version.
    pub fn lower_bound_cause(&self) -> Option<&'static str> {
        if !self.data_source.is_real() || !self.folder_marks.sealed() {
            return None;
        }
        Some(
            *self
                .folder_marks
                .cause
                .get_or_init(|| self.evidence_cause()),
        )
    }

    fn evidence_cause(&self) -> &'static str {
        if self.origin == ResultOrigin::Incremental {
            return REASON_EVIDENCE_NOT_RECORDED;
        }
        match self.folder_marks.evidence() {
            EvidenceState::Absent => REASON_EVIDENCE_NOT_RECORDED,
            EvidenceState::Dropped => REASON_EVIDENCE_BUDGET_EXCEEDED,
            // A folder that was entered without any attempt was read while the scan's
            // evidence budget was spent. One pass over the items, once per version.
            EvidenceState::Pinned
                if self.items.iter().any(|item| {
                    item.kind == ResultItemKind::Directory
                        && !self.folder_marks.attempted(item.item_no)
                        && self.recorded_unread(item.item_no).is_none()
                        && self.folder_marks.boundary(item.item_no).is_none()
                        && !self.below_boundary(item)
                }) =>
            {
                REASON_EVIDENCE_BUDGET_EXCEEDED
            }
            EvidenceState::Pinned => REASON_LOWER_BOUND_UNEXPLAINED,
        }
    }
}
