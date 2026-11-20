//! The kind of the item a visualization shape or row refers to.

use crate::result::ResultItemKind;
use serde::{Deserialize, Deserializer, Serialize};

/// Kind of the item behind a shape or row, so a consumer can choose file or folder text.
///
/// Only shapes and rows with an item reference carry it; group shapes and rows never do.
/// An omitted value means the kind is unknown. A value this consumer does not know reads
/// as omitted, and null is rejected. A shape with child shapes is a folder; a folder need
/// not have child shapes. A shape and a row of the same model for the same item agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum VizItemKind {
    File,
    Folder,
    Other,
}

impl VizItemKind {
    /// Maps the kind of a pinned result item. A directory, including a mount point or
    /// another volume's boundary directory, is a folder; an unknown kind has none.
    pub fn from_result(kind: ResultItemKind) -> Option<Self> {
        match kind {
            ResultItemKind::Directory => Some(Self::Folder),
            ResultItemKind::File => Some(Self::File),
            ResultItemKind::UntraversedLink => Some(Self::Other),
            ResultItemKind::Unknown | ResultItemKind::UnknownStructure => None,
        }
    }
}

/// Reads a present kind. An unknown string reads as omitted; null is rejected.
pub(super) fn optional_known<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<VizItemKind>, D::Error> {
    Ok(match String::deserialize(d)?.as_str() {
        "file" => Some(VizItemKind::File),
        "folder" => Some(VizItemKind::Folder),
        "other" => Some(VizItemKind::Other),
        _ => None,
    })
}
