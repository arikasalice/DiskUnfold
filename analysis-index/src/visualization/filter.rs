//! The item filter of a filtered projection: which files a condition keeps.
use crate::{ResultVersion, file_type::FileType, visualization::type_legend};

/// A condition a projection applies to the files under its scope.
///
/// A filtered projection counts only the bytes of the files it keeps. A folder is
/// never kept or dropped itself: it stands for the kept files under it. A link and
/// an item of unknown kind are never kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemFilter {
    /// Files of one type of the type classification table (pred.file_type revision 1).
    FileType(FileType),
}

impl ItemFilter {
    /// Whether the filter keeps this item of the pinned result.
    pub fn keeps(self, version: &ResultVersion, item_no: u32) -> bool {
        match self {
            Self::FileType(kind) => type_legend::file_type_of(version, item_no) == Some(kind),
        }
    }
}
