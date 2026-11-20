//! The type classification table, revision 1.
use du_analysis_index::file_type::{FileType, TABLE, TABLE_REVISION, classify};
use du_analysis_index::query::NameEncoding;
use std::collections::HashSet;

fn utf16(name: &str) -> Vec<u8> {
    name.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn both(name: &str) -> [Option<FileType>; 2] {
    [
        classify(name.as_bytes(), NameEncoding::Bytes),
        classify(&utf16(name), NameEncoding::WindowsUtf16Le),
    ]
}

#[test]
fn the_extension_after_the_last_dot_decides_the_type_in_either_encoding() {
    for (name, expected) in [
        ("report.PDF", FileType::Document),
        ("song.flac", FileType::Audio),
        ("photo.final.JPG", FileType::Image),
        ("clip.mkv", FileType::Video),
        ("backup.tar.gz", FileType::Archive),
        ("ubuntu-24.04.ISO", FileType::InstallerImage),
        ("setup.msix", FileType::InstallerImage),
        ("program.exe", FileType::Unclassified),
        ("사진.png", FileType::Image),
        ("notes.unknownext", FileType::Unclassified),
        ("main.ts", FileType::Unclassified),
        ("bundle.zipx", FileType::Unclassified),
        ("README.markdown", FileType::Unclassified),
    ] {
        assert_eq!(both(name), [Some(expected); 2], "{name}");
    }
}

#[test]
fn a_name_without_an_extension_is_not_an_unlisted_extension() {
    for name in ["Makefile", ".profile", "trailing.", "."] {
        assert_eq!(both(name), [Some(FileType::NoExtension); 2], "{name}");
    }
    // A dot file with its own extension has that extension.
    assert_eq!(both(".config.json"), [Some(FileType::Unclassified); 2]);
}

#[test]
fn an_extension_outside_ascii_is_unclassified_and_an_unreadable_name_has_no_type() {
    assert_eq!(both("문서.한글"), [Some(FileType::Unclassified); 2]);
    assert_eq!(
        classify(b"bad\xffname.\xff", NameEncoding::Bytes),
        Some(FileType::Unclassified)
    );
    // Three bytes are not a whole number of UTF-16 units.
    assert_eq!(classify(b"a.b", NameEncoding::WindowsUtf16Le), None);
}

#[test]
fn revision_one_lists_each_extension_once_in_lowercase() {
    assert_eq!(TABLE_REVISION, 1);
    let mut seen = HashSet::new();
    for (_, extensions) in TABLE {
        for extension in extensions {
            assert_eq!(*extension, extension.to_ascii_lowercase());
            // The classifier compares at most four ASCII bytes without allocating.
            assert!(extension.len() <= 4 && extension.is_ascii(), "{extension}");
            assert!(seen.insert(*extension), "{extension} is listed twice");
        }
    }
    assert_eq!(FileType::ALL.len(), TABLE.len() + 2);
}
