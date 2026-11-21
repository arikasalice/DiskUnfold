//! The type classes of one folder, beside its bar, row and rectangle (F06 README 54,
//! #298).
//!
//! The classes are counted as the legend counts them ([`type_legend::scope_totals`]),
//! so a folder's slices never disagree with the legend of a scope that holds it.
use super::type_legend::{self, ClassTotal, TotalsError};
use crate::{
    ResultVersion,
    folder_collection::{FolderCollection, Unenumerated, UnreadInside},
};
use du_core_contracts::{ItemRef, SizeBasis, result::ResultItemKind};
use std::cmp::Reverse;

/// The classes of one folder's files at any depth, largest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderTypes {
    /// One entry per class with at least one file: bytes descending, ties in legend
    /// order. A folder without a counted file has none.
    pub classes: Vec<(&'static str, ClassTotal)>,
    /// A range below the folder was not read, or whether one was cannot be told yet:
    /// the counts and bytes are then only lower bounds.
    pub below_unread: bool,
}

/// The type classes of `folder`, or `None` where they do not apply: an item that is
/// not a folder, a sample result, and a folder whose own listing failed, which has
/// nothing counted below it.
///
/// # Errors
/// `InvalidScope` when the reference does not name an item of this version, and
/// `Cancelled` when `cancelled` asks to stop during the walk.
pub fn folder_types(
    version: &ResultVersion,
    folder: &ItemRef,
    basis: &SizeBasis,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<FolderTypes>, TotalsError> {
    let item = version
        .detail(folder)
        .map_err(|_| TotalsError::InvalidScope)?
        .item;
    if !version.data_source.is_real() || item.kind != ResultItemKind::Directory {
        return Ok(None);
    }
    let collection = version
        .folder_collection(folder)
        .map_err(|_| TotalsError::InvalidScope)?;
    if let FolderCollection::Folder {
        unenumerated: Unenumerated::Yes { .. },
        ..
    } = collection
    {
        return Ok(None);
    }
    let below_unread = !matches!(
        version.unread_inside(folder),
        Ok(Some(UnreadInside::NoneRecorded))
    );
    let mut classes: Vec<_> = type_legend::scope_totals(version, folder, basis, cancelled)?
        .into_iter()
        .collect();
    classes.sort_by_key(|(key, total)| (Reverse(total.bytes), legend_position(key)));
    Ok(Some(FolderTypes {
        classes,
        below_unread,
    }))
}

/// The place of a class in the legend; a key the legend does not list goes last.
fn legend_position(key: &str) -> usize {
    type_legend::LISTED
        .iter()
        .position(|class| class.key == key)
        .unwrap_or(type_legend::LISTED.len())
}
