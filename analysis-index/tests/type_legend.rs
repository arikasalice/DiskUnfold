//! The legend classes of the "type" color criterion.
use du_analysis_index::file_type::FileType;
use du_analysis_index::query::NameEncoding;
use du_analysis_index::visualization::type_legend::{FOLDER, LISTED, NO_CLASS, class_of};
use du_core_contracts::result::ResultItemKind;
use std::collections::HashSet;

#[test]
fn files_take_their_type_folders_their_own_class_and_the_rest_no_class() {
    let file = |name: &str| class_of(ResultItemKind::File, name.as_bytes(), NameEncoding::Bytes);
    assert_eq!(file("report.pdf"), "type.document");
    assert_eq!(file("setup.iso"), "type.installer_image");
    assert_eq!(file("Makefile"), "type.no_extension");
    assert_eq!(file("main.rs"), "type.unclassified");
    assert_eq!(
        class_of(
            ResultItemKind::Directory,
            b"photos.jpg",
            NameEncoding::Bytes
        ),
        FOLDER
    );
    for kind in [
        ResultItemKind::UntraversedLink,
        ResultItemKind::Unknown,
        ResultItemKind::UnknownStructure,
    ] {
        assert_eq!(class_of(kind, b"clip.mp4", NameEncoding::Bytes), NO_CLASS);
    }
    // An odd number of UTF-16 bytes is not a readable name.
    assert_eq!(
        class_of(ResultItemKind::File, b"a.b", NameEncoding::WindowsUtf16Le),
        NO_CLASS
    );
}

#[test]
fn the_legend_lists_every_type_once_in_order_and_never_the_folder() {
    let keys: Vec<_> = LISTED.iter().map(|class| class.key).collect();
    assert_eq!(
        keys,
        [
            "type.document",
            "type.audio",
            "type.image",
            "type.installer_image",
            "type.video",
            "type.archive",
            "type.no_extension",
            "type.unclassified",
            "unclassified",
        ]
    );
    assert_eq!(keys.iter().collect::<HashSet<_>>().len(), keys.len());
    assert!(!keys.contains(&FOLDER));
    // Every type of the table has a listed class, plus the class of no type.
    assert_eq!(LISTED.len(), FileType::ALL.len() + 1);
    assert!(LISTED.iter().all(|class| !class.label_key.is_empty()));
}
