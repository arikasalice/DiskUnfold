//! Per-folder scan state and excluded child counts of a pinned result version.
//!
//! # Decision table
//!
//! Applies to a directory item; any other item is [`FolderCollection::NotAFolder`].
//! The rules are checked in this order and the first match answers.
//!
//! 1. The version is not a real scan (a sample): unenumerated is Unknown with
//!    [`REASON_DATA_INSUFFICIENT`] and the boundary is [`FolderBoundary::NotProvided`].
//! 2. The folder has an exact gap (a gap whose whole path is this item), in any
//!    publication and any completeness: unenumerated is Yes with the reason of the first
//!    recorded gap, and the boundary is unknown. A gap whose path
//!    is not an item has no row and marks nothing.
//! 3. The folder carries an unread mark ([`crate::BuildHandle::set_directory_unread`]): unenumerated
//!    is Yes with [`REASON_CANCELLED`] or [`REASON_DISCONNECTED`] and the boundary is
//!    unknown boundary. A gap on the same folder wins (rule 2), so the gap's
//!    reason is reported. The mark outranks a boundary issue on the same folder.
//! 4. The folder has a `scan.boundary_other_volume` issue ([`BOUNDARY_OTHER_VOLUME`]) or a
//!    `scan.revisit_blocked` issue ([`BOUNDARY_ALREADY_COUNTED`]): the boundary is that
//!    kind and unenumerated is No. With both issues the other-volume boundary is reported.
//! 5. The folder was entered and read to its end: unenumerated is No and the boundary is
//!    [`FolderBoundary::NoBoundary`]. Two kinds of evidence prove it:
//!    - the sealed version of a real `Scan` origin whose completeness is `Complete`. Such
//!      a seal is produced only without cancellation or collection failure, and it is
//!      completion evidence for every folder (F03 DLD FR5.HD1.app-host.DD1). A folder
//!      without a gap, unread mark or boundary issue there was entered. An incremental
//!      replacement records no gaps or issues of its base, so it never uses this rule;
//!    - every pinned attempt for the folder ended (`EndedUnproven`), in any publication.
//!      Attempts of a replacement build are not used.
//! 6. A publication that is not the sealed one of its build is Unknown with
//!    [`REASON_IN_PROGRESS`].
//! 7. Otherwise both are Unknown with [`REASON_DATA_INSUFFICIENT`]. A seal that is not
//!    `Complete` does not prove that a particular folder was enumerated.
//!
//! Rules 2 to 4 are exclusive, so a folder is never both unenumerated and bounded.
//!
//! Replacement builds (`open_replacement`) start without gaps, issues and records, keep the
//! data source of their base and are always sealed. They follow the same rules from what
//! their own build recorded; nothing is inherited from the base version.
//!
//! # Excluded children
//!
//! Excluded counts are not provided: the current producer does not establish that
//! repeated enumeration cannot count the same direct child twice. Neither a reported
//! count nor an absent record proves an exact count or a positive lower bound.
//!
//! # Cost
//!
//! Gaps use the existing item index. Boundary issues are resolved
//! to item numbers once per publication outside the store lock (see `FolderMarks::bind`).
//! Each lookup is O(log n) and performs no allocation; reason keys are borrowed from the version.
pub use crate::folder_marks::{UnreadFolderCounts, UnreadReason};
use crate::{QueryError, ResultVersion, result_read::ensure_directory};
use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultItem, ResultItemKind},
};

use crate::folder_marks::BoundaryKind;
use crate::origin::ResultOrigin;

/// Reason key reported while the scan that produced the publication has not finished.
pub const REASON_IN_PROGRESS: &str = "scan.in_progress";
/// Reason key reported when the version cannot tell the fact.
pub const REASON_DATA_INSUFFICIENT: &str = "space.data_insufficient";
/// Reason key of a folder left unread because the scan was cancelled. Defined here only; it
/// is not a shared contract key.
pub const REASON_CANCELLED: &str = "scan.cancelled";
/// Reason key of a folder left unread because its volume disconnected.
pub const REASON_DISCONNECTED: &str = "scan.disconnected";
/// Name of the boundary of a folder on another volume than its parent.
pub const BOUNDARY_OTHER_VOLUME: &str = "other_volume";
/// Name of the boundary of a folder that was already collected through another path.
pub const BOUNDARY_ALREADY_COUNTED: &str = "already_counted";

/// Whether the collector tried to read a folder's entries and could not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unenumerated<'a> {
    /// Entries were not read; `reason_key` is that of the first recorded gap.
    Yes { reason_key: &'a str },
    /// The folder's entries were read or the folder was not meant to be read.
    No,
    /// The version cannot tell.
    Unknown { reason_key: &'static str },
}

/// Whether the scope rules deliberately did not enter a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderBoundary {
    /// Another volume than its parent, including a mount point.
    OtherVolume,
    /// Already collected through another path or by an earlier root.
    AlreadyCounted,
    /// Confirmed: the folder is not a boundary.
    NoBoundary,
    /// The version cannot tell.
    Unknown { reason_key: &'static str },
    /// This source does not provide the fact.
    NotProvided,
}

impl FolderBoundary {
    /// The stable name of a boundary, or `None` for every other answer.
    pub fn name(self) -> Option<&'static str> {
        match self {
            Self::OtherVolume => Some(BOUNDARY_OTHER_VOLUME),
            Self::AlreadyCounted => Some(BOUNDARY_ALREADY_COUNTED),
            _ => None,
        }
    }
}

/// The scan state of one item of a pinned version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderCollection<'a> {
    /// The item is not a directory.
    NotAFolder,
    /// The item is a directory.
    Folder {
        /// Whether the collector tried to read the folder's entries and could not.
        unenumerated: Unenumerated<'a>,
        /// Whether the scope rules deliberately did not enter the folder.
        boundary: FolderBoundary,
    },
}

impl UnreadReason {
    /// The reason key a query reports for this reason.
    pub fn reason_key(self) -> &'static str {
        match self {
            Self::Cancelled => REASON_CANCELLED,
            Self::Disconnected => REASON_DISCONNECTED,
        }
    }
}

/// Direct children dropped by exclusion rules in one folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcludedChildren {
    pub count: u32,
    /// `false` when `count` is a lower bound.
    pub exact: bool,
}

impl ResultVersion {
    /// Answers the scan state of one item.
    ///
    /// # Errors
    /// `StaleVersion` when the reference pins another version and `NotFound` when the item
    /// number is not in this version.
    pub fn folder_collection(&self, item: &ItemRef) -> Result<FolderCollection<'_>, QueryError> {
        Ok(self.collection_of(self.detail(item)?.item))
    }

    /// Answers the scan state of every reference of one page, in the order of `items`.
    ///
    /// # Errors
    /// The first failing reference fails the page with the error of
    /// [`ResultVersion::folder_collection`]; no partial page is returned.
    pub fn folder_collections(
        &self,
        items: &[ItemRef],
    ) -> Result<Vec<FolderCollection<'_>>, QueryError> {
        items
            .iter()
            .map(|item| self.folder_collection(item))
            .collect()
    }

    /// Answers how many direct children of one folder the exclusion rules dropped, or
    /// `None` when this version cannot tell.
    ///
    /// # Errors
    /// `StaleVersion` or `NotFound` as for [`ResultVersion::folder_collection`];
    /// `NotDirectory` or `KindUnknown` when the item is not a directory.
    pub fn folder_excluded_children(
        &self,
        item: &ItemRef,
    ) -> Result<Option<ExcludedChildren>, QueryError> {
        let item = self.detail(item)?.item;
        ensure_directory(item.kind)?;
        Ok(None)
    }

    /// How many folders of this version carry each unread mark.
    ///
    /// Computed when the version was published; the call is O(1).
    pub fn unread_folder_counts(&self) -> UnreadFolderCounts {
        self.folder_marks.unread_counts()
    }

    fn collection_of(&self, item: &ResultItem) -> FolderCollection<'_> {
        if item.kind != ResultItemKind::Directory {
            return FolderCollection::NotAFolder;
        }
        let unknown = |reason_key| FolderCollection::Folder {
            unenumerated: Unenumerated::Unknown { reason_key },
            boundary: FolderBoundary::Unknown { reason_key },
        };
        let (unenumerated, boundary) = if !self.data_source.is_real() {
            (
                Unenumerated::Unknown {
                    reason_key: REASON_DATA_INSUFFICIENT,
                },
                FolderBoundary::NotProvided,
            )
        } else if let Some(reason_key) = self.recorded_unread(item.item_no) {
            (
                Unenumerated::Yes { reason_key },
                FolderBoundary::Unknown {
                    reason_key: REASON_DATA_INSUFFICIENT,
                },
            )
        } else if let Some(kind) = self.folder_marks.boundary(item.item_no) {
            (Unenumerated::No, boundary_of(kind))
        } else if self.entered(item.item_no) {
            (Unenumerated::No, FolderBoundary::NoBoundary)
        } else if !self.folder_marks.sealed() {
            return unknown(REASON_IN_PROGRESS);
        } else {
            return unknown(REASON_DATA_INSUFFICIENT);
        };
        FolderCollection::Folder {
            unenumerated,
            boundary,
        }
    }

    /// Whether an unmarked folder of a real scan was entered and read (rule 5).
    fn entered(&self, item_no: u32) -> bool {
        let complete_scan = self.origin == ResultOrigin::Scan
            && self.folder_marks.sealed()
            && self.completeness == ResultCompleteness::Complete;
        complete_scan || self.folder_marks.ended(item_no)
    }

    /// The reason the folder's entries were not read: the first gap recorded for exactly
    /// this item, else its unread mark.
    fn recorded_unread(&self, item_no: u32) -> Option<&str> {
        let gap = self
            .exact_gaps
            .get(&item_no)
            .and_then(|indices| indices.first())
            .and_then(|index| self.gaps.get(*index));
        match gap {
            Some(gap) => Some(gap.reason_key.as_str()),
            None => self
                .folder_marks
                .unread(item_no)
                .map(UnreadReason::reason_key),
        }
    }
}

fn boundary_of(kind: BoundaryKind) -> FolderBoundary {
    match kind {
        BoundaryKind::OtherVolume => FolderBoundary::OtherVolume,
        BoundaryKind::AlreadyCounted => FolderBoundary::AlreadyCounted,
    }
}
