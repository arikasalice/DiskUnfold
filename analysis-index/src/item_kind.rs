use du_core_contracts::result::ResultItemKind;

pub(crate) fn is_directory(kind: ResultItemKind) -> bool {
    kind == ResultItemKind::Directory
}

pub(crate) fn is_structure(kind: ResultItemKind) -> bool {
    matches!(
        kind,
        ResultItemKind::Directory | ResultItemKind::UnknownStructure
    )
}
