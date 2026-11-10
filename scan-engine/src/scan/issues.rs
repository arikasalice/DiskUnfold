//! Classification from collection operations, independent of error text.
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_failures::{ScanFailureKind as Kind, ScanFailureStage as Stage},
    scan_observation::{ObservedEntry, ObservedKind},
};

pub(super) fn unread(
    build: &mut du_analysis_index::BuildHandle,
    root_index: u32,
    relative: Vec<Vec<u8>>,
    stage: Stage,
    reason: &str,
    observed_at: Option<du_core_contracts::Timestamp>,
) -> Result<(), du_analysis_index::BuildError> {
    build.record_scan_issue(du_core_contracts::scan_failures::ScanIssue {
        root_index,
        relative,
        kind: Kind::UnreadRange,
        stage,
        reason_key: reason.into(),
        observed_at,
    })
}

pub(super) fn reason(error: &PortError) -> &'static str {
    match error.kind {
        PortErrorKind::PermissionDenied => "scan.path_denied",
        PortErrorKind::NotFound => "scan.path_vanished",
        _ => "scan.unclassified",
    }
}

pub(super) fn classify(entry: &ObservedEntry) -> Option<(Kind, Stage, &'static str)> {
    let meta = match &entry.metadata {
        Err(error) => return Some((Kind::CollectionFailed, Stage::Metadata, reason(error))),
        Ok(meta) => meta,
    };
    if entry.enumerated_inode == 0
        || entry.enumerated_inode != meta.inode
        || (entry.enumerated_kind != ObservedKind::Unknown && entry.enumerated_kind != meta.kind)
        || matches!(meta.kind, ObservedKind::Unknown | ObservedKind::Other)
    {
        return Some((Kind::Uncertain, Stage::Metadata, "scan.unclassified"));
    }
    if meta.kind == ObservedKind::Link {
        return Some((
            Kind::UntraversedLink,
            Stage::Traverse,
            "scan.link_untraversed",
        ));
    }
    if meta.kind == ObservedKind::File
        && let Err(error) = &meta.size
    {
        return Some((Kind::CollectionFailed, Stage::Metadata, reason(error)));
    }
    None
}
