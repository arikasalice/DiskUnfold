//! The type classification table: the file type of a name by its extension.
//!
//! This is the table the F04 type condition and the F06 type legend share, so a
//! name has one type wherever it is shown. One table revision fixes which
//! extensions belong to which type; changing any list is a new revision. A name
//! without an extension and a name whose extension the table does not list are
//! different values. Whether an item has a type at all (a folder does not) is
//! the caller's decision.
use crate::query::NameEncoding;

/// Revision of [TABLE]. A change to any list is a new revision.
pub const TABLE_REVISION: u32 = 1;

/// The type of a file name under one table revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileType {
    Document,
    Audio,
    Image,
    /// Disk images and installer packages: often large and left behind after use.
    /// Executables are not listed, so an installed program is never offered as one.
    InstallerImage,
    Video,
    Archive,
    /// The name has no extension: no dot, a leading dot only, or a trailing dot.
    NoExtension,
    /// The name has an extension that the table does not list.
    Unclassified,
}

impl FileType {
    /// Every type in the fixed order of a legend: listed types, then the two
    /// that are not a type of the table.
    pub const ALL: [FileType; 8] = [
        FileType::Document,
        FileType::Audio,
        FileType::Image,
        FileType::InstallerImage,
        FileType::Video,
        FileType::Archive,
        FileType::NoExtension,
        FileType::Unclassified,
    ];
}

/// Lowercase ASCII extensions of each listed type, revision [TABLE_REVISION].
///
/// "ts" is not listed: it is both a video stream and a TypeScript source.
pub const TABLE: [(FileType, &[&str]); 6] = [
    (
        FileType::Document,
        &[
            "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "xls", "xlsx", "ods", "csv", "ppt",
            "pptx", "odp", "hwp", "hwpx", "epub",
        ],
    ),
    (
        FileType::Audio,
        &[
            "mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "wma", "aiff", "mid", "midi",
        ],
    ),
    (
        FileType::Image,
        &[
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "heic", "heif", "tif", "tiff", "svg",
            "psd", "ico", "dng", "cr2", "nef", "arw",
        ],
    ),
    (
        FileType::Video,
        &[
            "mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "mpg", "mpeg", "flv", "3gp",
        ],
    ),
    (
        FileType::Archive,
        &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst"],
    ),
    (
        FileType::InstallerImage,
        &[
            "iso", "img", "dmg", "vhd", "vhdx", "vmdk", "msi", "msix", "appx", "pkg", "cab",
        ],
    ),
];

/// The type of a file name in its native encoding, or `None` when the name
/// cannot be read in that encoding (an odd number of UTF-16 bytes).
///
/// The extension is what follows the last dot. A name that starts with its only
/// dot (".profile") or ends with a dot has no extension. Matching ignores ASCII
/// case; an extension with any other character is not in the table.
///
/// It runs over every file of a result, so it reads the name once from its end
/// and allocates nothing.
pub fn classify(name: &[u8], encoding: NameEncoding) -> Option<FileType> {
    let found = match encoding {
        NameEncoding::Bytes => scan(name.iter().rev().map(|&byte| u16::from(byte)), name.len()),
        NameEncoding::WindowsUtf16Le => {
            let (units, rest) = name.as_chunks::<2>();
            if !rest.is_empty() {
                return None;
            }
            scan(
                units.iter().rev().map(|pair| u16::from_le_bytes(*pair)),
                units.len(),
            )
        }
    };
    // No dot, the name's first unit is its only dot, or the name ends with a dot.
    if !found.dot || found.leading || found.length == 0 {
        return Some(FileType::NoExtension);
    }
    if !found.fits {
        return Some(FileType::Unclassified);
    }
    let extension = &found.buf[LONGEST - found.length..];
    Some(
        TABLE
            .iter()
            .find(|(_, extensions)| extensions.iter().any(|known| known.as_bytes() == extension))
            .map_or(FileType::Unclassified, |(kind, _)| *kind),
    )
}

/// The longest extension in [TABLE]; a longer one cannot be listed.
const LONGEST: usize = 4;

/// What follows the last dot of a name, read from its end.
struct Found {
    /// The name has a dot.
    dot: bool,
    /// That dot is the name's first unit.
    leading: bool,
    /// Units after the dot.
    length: usize,
    /// Every unit after the dot is ASCII and there are at most [LONGEST].
    fits: bool,
    /// The lowercase extension, right-aligned, while it fits.
    buf: [u8; LONGEST],
}

fn scan(units: impl Iterator<Item = u16>, total: usize) -> Found {
    let mut found = Found {
        dot: false,
        leading: false,
        length: 0,
        fits: true,
        buf: [0; LONGEST],
    };
    for unit in units {
        if unit == u16::from(b'.') {
            found.dot = true;
            found.leading = found.length + 1 == total;
            return found;
        }
        match u8::try_from(unit).ok().filter(u8::is_ascii) {
            Some(byte) if found.length < LONGEST => {
                found.buf[LONGEST - 1 - found.length] = byte.to_ascii_lowercase();
            }
            _ => found.fits = false,
        }
        found.length += 1;
    }
    found
}
