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
//!    recorded gap, and the boundary is [`FolderBoundary::NoBoundary`]. A gap whose path
//!    is not an item has no row and marks nothing.
//! 3. The folder has a `scan.boundary_other_volume` issue ([`BOUNDARY_OTHER_VOLUME`]) or a
//!    `scan.revisit_blocked` issue ([`BOUNDARY_ALREADY_COUNTED`]): the boundary is that
//!    kind and unenumerated is No. With both issues the other-volume boundary is reported.
//! 4. No mark and the publication is not the sealed one of its build: both are Unknown with
//!    [`REASON_IN_PROGRESS`].
//! 5. No mark and the sealed version is Complete or Missing: unenumerated is No and the
//!    boundary is [`FolderBoundary::NoBoundary`].
//! 6. No mark and the sealed version is Partial or Cancelled: both are Unknown with
//!    [`REASON_DATA_INSUFFICIENT`], because an unmarked folder cannot yet be told from an
//!    empty one.
//!
//! Rules 2 and 3 are exclusive, so a folder is never both unenumerated and bounded.
//!
//! Replacement builds (`open_replacement`) start without gaps, issues and records, keep the
//! data source of their base and are always sealed. They follow the same rules from what
//! their own build recorded; nothing is inherited from the base version.
//!
//! # Excluded children
//!
//! A reported folder answers `count` and `exact = complete && no gap on the folder`. An
//! unreported folder of a real sealed Complete or Missing version answers 0, exact unless
//! the folder has a gap (its entries were not read, so 0 is a lower bound). Any other
//! unreported folder answers `None`.
//!
//! # Cost
//!
//! Gaps use the existing item index. Boundary issues are resolved to item numbers once per
//! publication outside the store lock (see `FolderMarks::bind`). Each lookup is O(log n)
//! and performs no allocation; reason keys are borrowed from the version.
use crate::{QueryError, ResultVersion, result_read::ensure_directory};
use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultItem, ResultItemKind},
};

use crate::folder_marks::BoundaryKind;

/// Reason key reported while the scan that produced the publication has not finished.
pub const REASON_IN_PROGRESS: &str = "scan.in_progress";
/// Reason key reported when the version cannot tell the fact.
pub const REASON_DATA_INSUFFICIENT: &str = "space.data_insufficient";
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
    Folder {
        unenumerated: Unenumerated<'a>,
        boundary: FolderBoundary,
    },
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
        let unread = self.exact_gaps.contains_key(&item.item_no);
        if let Some(record) = self.folder_marks.excluded(item.item_no) {
            return Ok(Some(ExcludedChildren {
                count: record.count,
                exact: record.complete && !unread,
            }));
        }
        Ok(self.sealed_scan().then_some(ExcludedChildren {
            count: 0,
            exact: !unread,
        }))
    }

    /// Whether this is the sealed publication of a real scan that finished its walk.
    fn sealed_scan(&self) -> bool {
        self.data_source.is_real()
            && self.folder_marks.sealed()
            && matches!(
                self.completeness,
                ResultCompleteness::Complete | ResultCompleteness::Missing
            )
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
        } else if let Some(gap) = self.first_gap(item.item_no) {
            (
                Unenumerated::Yes {
                    reason_key: gap.as_str(),
                },
                FolderBoundary::NoBoundary,
            )
        } else if let Some(kind) = self.folder_marks.boundary(item.item_no) {
            (Unenumerated::No, boundary_of(kind))
        } else if !self.folder_marks.sealed() {
            return unknown(REASON_IN_PROGRESS);
        } else if self.sealed_scan() {
            (Unenumerated::No, FolderBoundary::NoBoundary)
        } else {
            return unknown(REASON_DATA_INSUFFICIENT);
        };
        FolderCollection::Folder {
            unenumerated,
            boundary,
        }
    }

    /// The reason key of the first gap recorded for exactly this item.
    fn first_gap(&self, item_no: u32) -> Option<&String> {
        let index = *self.exact_gaps.get(&item_no)?.first()?;
        self.gaps.get(index).map(|gap| &gap.reason_key)
    }
}

fn boundary_of(kind: BoundaryKind) -> FolderBoundary {
    match kind {
        BoundaryKind::OtherVolume => FolderBoundary::OtherVolume,
        BoundaryKind::AlreadyCounted => FolderBoundary::AlreadyCounted,
    }
}
