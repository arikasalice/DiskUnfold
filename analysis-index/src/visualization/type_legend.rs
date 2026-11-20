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
use du_core_contracts::{PathEncoding, result::ResultItemKind};

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
