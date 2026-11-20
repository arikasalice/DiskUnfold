//! The legend classes of the "type" color criterion (F06 FR6).
//!
//! Each file takes the class of its type in the type classification table. A
//! folder has no type: it takes [FOLDER], which a legend does not list. An item
//! whose type cannot be decided takes [NO_CLASS]. The listed classes come in
//! the fixed order of [LISTED]; the screen gives a fixed colour to each key.
use crate::{
    ResultVersion,
    file_type::{self, FileType, TABLE_REVISION},
    query::NameEncoding,
};
use du_core_contracts::{
    ItemRef, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, result::ResultItemKind,
};
use std::collections::BTreeMap;

/// The color criterion this table serves.
pub const CRITERION: &str = "type";
/// Revision of the type classification table the classes are drawn from.
pub const FILE_TYPE_TABLE_REVISION: u32 = TABLE_REVISION;
/// The class of a folder; never listed in a legend.
pub const FOLDER: &str = "type.folder";
/// The class of an item whose type cannot be decided ("no class").
pub const NO_CLASS: &str = "unclassified";

/// One listed legend class: its key and the message key of its name.
pub struct Class {
    pub key: &'static str,
    pub label_key: &'static str,
}

/// The listed classes in legend order (F06 design README 44.3): the named types,
/// the other types, names without an extension, then the items whose type is
/// unavailable.
pub const LISTED: [Class; 9] = [
    class("type.document", "ui.visualization.legend.type.document"),
    class("type.audio", "ui.visualization.legend.type.audio"),
    class("type.image", "ui.visualization.legend.type.image"),
    class(
        "type.installer_image",
        "ui.visualization.legend.type.installer_image",
    ),
    class("type.video", "ui.visualization.legend.type.video"),
    class("type.archive", "ui.visualization.legend.type.archive"),
    class(
        "type.unclassified",
        "ui.visualization.legend.type.unclassified",
    ),
    class(
        "type.no_extension",
        "ui.visualization.legend.type.no_extension",
    ),
    class(NO_CLASS, "ui.visualization.legend.type.unavailable"),
];

const fn class(key: &'static str, label_key: &'static str) -> Class {
    Class { key, label_key }
}

/// The legend class of one result item.
///
/// Links and items of unknown kind have no type; a file whose name cannot be
/// read in its encoding has no type either.
pub fn class_of(kind: ResultItemKind, name: &[u8], encoding: NameEncoding) -> &'static str {
    match kind {
        ResultItemKind::Directory => FOLDER,
        ResultItemKind::File => file_type::classify(name, encoding).map_or(NO_CLASS, key_of),
        ResultItemKind::UntraversedLink
        | ResultItemKind::Unknown
        | ResultItemKind::UnknownStructure => NO_CLASS,
    }
}

/// The type of one file of a pinned result, read in the encoding of its observed
/// root. A folder, a link, an item of unknown kind and a name that cannot be read
/// have no type.
///
/// It runs over every file under a filtered scope, so it reads the item and its
/// root by number and allocates nothing.
pub fn file_type_of(version: &ResultVersion, item_no: u32) -> Option<FileType> {
    let item = version.items.get(item_no as usize)?;
    if item.kind != ResultItemKind::File {
        return None;
    }
    let root = *version.root_indices.get(item_no as usize)?;
    let encoding = match version.roots.get(root as usize)?.path.encoding {
        PathEncoding::Unix => NameEncoding::Bytes,
        PathEncoding::WindowsUtf16le => NameEncoding::WindowsUtf16Le,
    };
    file_type::classify(&item.name, encoding)
}

/// The type a listed type key names; the class of no type and the folder name none.
pub fn type_of_key(key: &[u8]) -> Option<FileType> {
    FileType::ALL
        .into_iter()
        .find(|kind| key_of(*kind).as_bytes() == key)
}

/// The legend key of a type.
pub fn key_of(kind: FileType) -> &'static str {
    match kind {
        FileType::Document => "type.document",
        FileType::Audio => "type.audio",
        FileType::Image => "type.image",
        FileType::InstallerImage => "type.installer_image",
        FileType::Video => "type.video",
        FileType::Archive => "type.archive",
        FileType::NoExtension => "type.no_extension",
        FileType::Unclassified => "type.unclassified",
    }
}

/// Item count, summed bytes and whether the sum is only a lower bound, of one class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassTotal {
    pub count: u64,
    pub bytes: u64,
    pub lower_bound: bool,
}

/// Why the classes of a scope could not be counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotalsError {
    InvalidScope,
    Cancelled,
}

/// The legend classes of every item below a scope folder, its whole subtree
/// (F06 FR6, #301): a type's row counts the files of that type at any depth.
///
/// Folders are walked and never counted. A file outside the scan boundary was
/// not read and is left out. A size that is unknown or not in the basis adds
/// nothing and leaves the class total a lower bound, as does a lower bound. Each
/// file adds its own value, so a file linked twice under the scope is added twice.
pub fn scope_totals(
    version: &ResultVersion,
    scope: &ItemRef,
    basis: &SizeBasis,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<BTreeMap<&'static str, ClassTotal>, TotalsError> {
    let scope_no = version
        .detail(scope)
        .map_err(|_| TotalsError::InvalidScope)?
        .item
        .item_no;
    // One slot per type, in FileType::ALL order, then the class of no type.
    let mut slots = [ClassTotal::default(); FileType::ALL.len() + 1];
    let mut work: Vec<u32> = version
        .children(scope_no)
        .iter()
        .map(|c| c.item_no)
        .collect();
    while let Some(no) = work.pop() {
        if cancelled() {
            return Err(TotalsError::Cancelled);
        }
        let item = version
            .items
            .get(no as usize)
            .ok_or(TotalsError::InvalidScope)?;
        if item.kind == ResultItemKind::Directory {
            work.extend(version.children(no).iter().map(|child| child.item_no));
            continue;
        }
        if version.is_boundary_item(no) {
            continue;
        }
        let slot = file_type_of(version, no).map_or(FileType::ALL.len(), |kind| {
            FileType::ALL
                .iter()
                .position(|listed| *listed == kind)
                .unwrap_or(FileType::ALL.len())
        });
        let total = &mut slots[slot];
        total.count = total.count.saturating_add(1);
        match version.item_size(no, basis.clone()) {
            Some(size) if size.evidence != SizeEvidence::Unknown && size.basis == *basis => {
                match &size.amount {
                    SizeAmount::Exact { bytes } => {
                        total.bytes = total.bytes.saturating_add(bytes.get());
                    }
                    SizeAmount::LowerBound { bytes } => {
                        total.bytes = total.bytes.saturating_add(bytes.get());
                        total.lower_bound = true;
                    }
                    SizeAmount::Unknown => total.lower_bound = true,
                }
            }
            _ => total.lower_bound = true,
        }
    }
    let keys = FileType::ALL
        .iter()
        .map(|kind| key_of(*kind))
        .chain([NO_CLASS]);
    Ok(keys
        .zip(slots)
        .filter(|(_, total)| total.count > 0)
        .collect())
}
