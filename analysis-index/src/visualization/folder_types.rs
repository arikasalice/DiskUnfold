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
    let below_unread = !read_through(version, folder);
    let mut classes: Vec<_> = type_legend::scope_totals(version, folder, basis, cancelled)?
        .into_iter()
        .collect();
    classes.sort_by_key(|(key, total)| (Reverse(total.bytes), legend_position(key)));
    Ok(Some(FolderTypes {
        classes,
        below_unread,
    }))
}

/// The type classes of the members of a group, such as the Other group of a bar
/// chart (F06 README 54.2), counted together as the legend counts them: a file
/// member is its own type, a folder member adds the files below it. `None` for a
/// sample result. A member folder that could not be read, or one with unread
/// ranges below it, makes the counts and bytes lower bounds.
///
/// # Errors
/// `InvalidScope` when a reference does not name an item of this version, and
/// `Cancelled` when `cancelled` asks to stop during the walk.
pub fn group_types(
    version: &ResultVersion,
    members: &[ItemRef],
    basis: &SizeBasis,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<FolderTypes>, TotalsError> {
    if !version.data_source.is_real() {
        return Ok(None);
    }
    let mut start = Vec::with_capacity(members.len());
    let mut below_unread = false;
    for member in members {
        let item = version
            .detail(member)
            .map_err(|_| TotalsError::InvalidScope)?
            .item;
        if item.kind == ResultItemKind::Directory {
            let unread_itself = matches!(
                version.folder_collection(member),
                Ok(FolderCollection::Folder {
                    unenumerated: Unenumerated::Yes { .. },
                    ..
                })
            );
            below_unread |= unread_itself || !read_through(version, member);
        }
        start.push(item.item_no);
    }
    let mut classes: Vec<_> = type_legend::walk(version, start, basis, cancelled)?
        .into_iter()
        .collect();
    classes.sort_by_key(|(key, total)| (Reverse(total.bytes), legend_position(key)));
    Ok(Some(FolderTypes {
        classes,
        below_unread,
    }))
}

/// Whether nothing below a read folder was recorded as unread, and the version can tell.
fn read_through(version: &ResultVersion, folder: &ItemRef) -> bool {
    matches!(
        version.unread_inside(folder),
        Ok(Some(UnreadInside::NoneRecorded))
    )
}

/// The place of a class in the legend; a key the legend does not list goes last.
fn legend_position(key: &str) -> usize {
    type_legend::LISTED
        .iter()
        .position(|class| class.key == key)
        .unwrap_or(type_legend::LISTED.len())
}
